/* LICENSE BEGIN
    This file is part of the SixtyFPS Project -- https://sixtyfps.io
    Copyright (c) 2021 Olivier Goffart <olivier.goffart@sixtyfps.io>
    Copyright (c) 2021 Simon Hausmann <simon.hausmann@sixtyfps.io>

    SPDX-License-Identifier: GPL-3.0-only
    This file is also available under commercial licensing terms.
    Please contact info@sixtyfps.io for more information.
LICENSE END */

use rand::prelude::SliceRandom;
use rand::prelude::IteratorRandom;
use sixtyfps::Model;
use sixtyfps::ModelHandle;
use sixtyfps::VecModel;
use sixtyfps::re_exports::KeyEvent;
use std::borrow::Cow;
use std::cell;
use std::cell::RefCell;
use std::ops::Deref;
use std::ops::DerefMut;
use std::rc::Rc;
use rand::Rng;
use serde::{Serialize, Deserialize};
use serde_json;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

sixtyfps::include_modules!();

const SAVEGAME_PATH: &str = "./game_state.json";

fn random_puzzle() -> Vec<Cell> {
    let mut rng = rand::thread_rng();
    let mut vec = vec!();
    for i in 0..81 {
        const P_FIXED: f64 = 0.0;
        const P_WHITE: f64 = 1.0;
        let is_fixed = rng.gen_range(0.0..1.0) < P_FIXED;
        let is_white = rng.gen_range(0.0..1.0) < P_WHITE;
        let cell = Cell {
            pos_x: i % 9, pos_y: i / 9,
            value: if is_fixed {rng.gen_range(1..10)} else {-1},
            small_values: ModelHandle::new(Rc::new(VecModel::from(vec![false; 9]))),
            is_editing: false,
            is_valid_in_row: true,
            is_valid_in_straight: true,
            index: i,
            is_fixed,
            is_white,
        };
        vec.push(cell);
    }
    vec
}

struct AppState {
    cells: Rc<sixtyfps::VecModel<Cell>>,
    main_window: sixtyfps::Weak<MainWindow>,
    /// An array of 16 values which represent a 4x4 matrix containing the piece number in that
    /// position. -1 is no piece.
    // positions: Vec<i8>,
    auto_play_timer: sixtyfps::Timer,
    was_just_solved_timer: sixtyfps::Timer,
    /// The speed in the x and y direction for the associated tile
    finished: bool,
    editing_cell_index: Option<i8>,
    rows_columns: Vec<Row>,
    mode: Mode,
}

#[derive(Clone)]
enum VecOrVecModel<T> where T: Clone {
    Vec(Vec<T>),
    VecModel(Rc<sixtyfps::VecModel<T>>)
}

impl<T: 'static> VecOrVecModel<T> where T: Clone {
    fn get(&self, index: usize) -> T {
        match self {
            Self::Vec(vec) => {
                vec[index].clone()
            },
            Self::VecModel(vec_model) => {
                vec_model.row_data(index).clone()
            }
        }
    }

    fn set(&mut self, index: usize, value: T) {
        match self {
            Self::Vec(vec) => {
                vec[index] = value;
            },
            Self::VecModel(vec_model) => {
                vec_model.set_row_data(index, value);
            }
        }
    }
}

struct Row {
    row_cells: Vec<usize>,
    straights: Vec<Vec<usize>>,
    all_cells: VecOrVecModel<Cell>// Rc<sixtyfps::VecModel<Cell>>
}

#[derive(PartialEq)]
enum Mode {
    None,
    EditBlackWhite,
    EditFixedNumbers,
    PlayEnterNumbers,
    PlayEnterSmallNumbers
}

enum Str8tsSolution {
    None,
    Unique(Vec<Cell>),
    Multiple(Vec<Cell>)
}

impl Row {
    fn new(row_cells: Vec<usize>, all_cells: VecOrVecModel<Cell>) -> Row {
        let straights = row_cells.iter()
            .map(|&i| (i, all_cells.get(i).is_white)).collect::<Vec<(usize, bool)>>()
            .split(|(_, is_white)| !is_white)
            // .filter(|&slice| slice.len() > 0)
            .map(|slice| slice.iter().map(|(i, _)| *i).collect()).collect();
        Row { row_cells, straights, all_cells }
    }

    fn validate(&self) -> Option<(Vec<Vec<usize>>, Vec<Vec<usize>>)> {
        let mut occurrences: [Vec<usize>; 9] = Default::default();
        for &i in &self.row_cells {
            let value = self.all_cells.get(i).value;
            if value > 0 {
                occurrences[(value - 1) as usize].push(i);
            }
        }
        let multiple_occurrences = occurrences.iter()
            .filter(|num_occs| num_occs.len() > 1)
            .cloned().collect::<Vec<_>>();

        let straights_values = self.straights.iter()
            .map(|straight| 
                straight.iter().map(|&i| self.all_cells.get(i).value)
                .filter(|&value| value > 0)
                .collect::<Vec<_>>())
            .enumerate()
            .filter(|(_, values)| values.len() > 0)
            .collect::<Vec<_>>();
        let invalid_straights = straights_values.iter()
            .filter(|(k, values)|
                (values.iter().max().unwrap() - values.iter().min().unwrap()) as usize >= self.straights[*k].len())
            .map(|(k, _)| self.straights[*k].clone())
            .collect::<Vec<_>>();

        if multiple_occurrences.len() > 0 || invalid_straights.len() > 0 {
            return Some((multiple_occurrences, invalid_straights));
        } else {
            return None;
        }
    }

    fn missing_values(&self, candidate_values: Option<&[i32]>) -> Vec<i32> {
        self.missing_values_cells(candidate_values, &self.all_cells)
    }

    fn missing_values_cells(&self, candidate_values: Option<&[i32]>, all_cells: &VecOrVecModel<Cell>) -> Vec<i32> {
        let mut values_present = [false; 9];
        let mut candidate_values_present = [false; 9];
        // Value in output if NOT PRESENT and PRESENT IN CANDIDATE VALUE
        // !(present || !candidate-present)

        for &i in &self.row_cells {
            let val = all_cells.get(i).value;
            if val > 0 {
                values_present[(val - 1) as usize] = true;
            }
        }
        if let Some(values) = candidate_values {
            for &val in values {
                candidate_values_present[(val - 1) as usize] = true;
            }
        }
        values_present.iter().enumerate()
            .filter(|&(val, is_present)| !is_present 
                && (candidate_values_present[val] || candidate_values.is_none()))
            .map(|(val, _)| (val + 1) as i32).collect()
    }

    fn possible_straight_values(&self, cell_index: usize, candidate_values: &[i32]) -> Vec<i32>{
        self.possible_straight_values_cells(cell_index, candidate_values, &self.all_cells)
    }

    fn possible_straight_values_cells(&self, cell_index: usize, candidate_values: &[i32], 
            all_cells: &VecOrVecModel<Cell>) -> Vec<i32>{
        if let Some(straight_indices) = self.straights.iter().find(|s| s.contains(&cell_index)) {
            let straight = straight_indices.iter()
                .map(|&i| all_cells.get(i).value).filter(|&v| v > 0).collect::<Vec<_>>();
            if let (Some(&min), Some(&max)) = (straight.iter().min(), straight.iter().max()) {
                let len = straight_indices.len() as i32;
                candidate_values.iter().filter(|&&val| {
                    (min < val && val < max)
                    || (val < min && max - val < len)
                    || (max < val && val - min < len)
                }).map(|&val| val)
                .collect()
            } else {
                return candidate_values.into();
            }
        } else {
            panic!("Cell not in any straight!");
        }
    }
}

impl AppState {
    fn randomize(&mut self) {
        // self.positions = random_puzzle();
        let puzzle_cells = random_puzzle();
        for (i, cell) in puzzle_cells.iter().enumerate() {
            self.cells.set_row_data(i, cell.clone());
        }
        // for (i, p) in self.positions.iter().enumerate() {
        //     self.set_pieces_pos(*p, i as _);
        // }
        self.main_window.unwrap().set_moves(0);
        // self.apply_tiles_left();
    }

    fn save_to_file(&self, path: &str) {
        let cells_data = self.cells.iter()
            .map(|cell| (cell.value, cell.is_white, cell.is_fixed, cell.small_values.iter().collect::<Vec<bool>>()))
            .collect::<Vec<_>>();

        let json_data = serde_json::to_string(&cells_data)
            .expect("Unable to save game: unable to create JSON.");
        std::fs::write(path, json_data)
            .expect(&format!("Unable to save game: unable to write file {}.", path));
    }

    fn load_from_file(&mut self, path: &str) {
        let json_data = std::fs::read_to_string(path)
            .expect(&format!("Unable to load game: unable to read file {}", path));
        let mut cells_data: Vec<(i32, bool, bool, Vec<bool>)> = 
            serde_json::from_str(&json_data)
            .expect("Unable to load game: unable to parse JSON");
        for (i, data) in cells_data.drain(..).enumerate() {
            let mut cell = self.cells.row_data(i);
            cell.value = data.0;
            cell.is_white = data.1;
            cell.is_fixed = data.2;
            cell.small_values = ModelHandle::new(Rc::new(VecModel::from(data.3)));
            self.cells.set_row_data(i, cell);
        }
    }

    fn setup_rows_columns(&mut self) {
        self.rows_columns = Self::compute_rows_columns(&VecOrVecModel::VecModel(self.cells.clone()));
    }

    fn compute_rows_columns(cells: &VecOrVecModel<Cell>) -> Vec<Row> {
        let mut rows_columns = vec![];
        for row in 0..9 {
            let indices = (0..9).map(|j| row + 9*j).collect::<Vec<_>>();
            rows_columns.push(Row::new(indices, cells.clone()));
        }
        for column in 0..9 {
            let indices = (0..9).map(|j| j + column*9).collect::<Vec<_>>();
            rows_columns.push(Row::new(indices, cells.clone()));
        }
        rows_columns
    }

    fn compute_possible_values(cell_index: usize, all_cells: &VecOrVecModel<Cell>, rows_columns: &Vec<Row>)
            -> Vec<i32> {
        // Missing values row
        let mut possible_values = rows_columns[cell_index % 9]
            .missing_values_cells(None, &all_cells);
        
        // \cup Missing values column
        possible_values = rows_columns[9 + cell_index / 9]
            .missing_values_cells(Some(&possible_values), &all_cells);
        
        if all_cells.get(cell_index).is_white {
            // \cup possible straight in row values
            possible_values = rows_columns[cell_index % 9]
                .possible_straight_values_cells(cell_index, &possible_values, &all_cells);
            
            // \cup possible straight in row values
            possible_values = rows_columns[9 + cell_index / 9]
                .possible_straight_values_cells(cell_index, &possible_values, &all_cells);
        }

        possible_values
    }

    fn solve_backtrack(mut cells: Vec<Cell>) -> Str8tsSolution {
        let rows_columns = Self::compute_rows_columns(&VecOrVecModel::Vec(cells.clone()));
        let mut indices_stack = vec![];
        let mut possible_values_stack = vec![];
        let mut i = 0;
        let mut found_solutions = vec![];
        while found_solutions.len() < 2 {
            while i < cells.len() {
                if (!cells[i].is_white || cells[i].value > 0) && i >= possible_values_stack.len() {
                    possible_values_stack.push(vec![]);
                    indices_stack.push(0);
                    // println!("cell {} ->", i);
                    i += 1;
                    continue;
                }

                if i >= possible_values_stack.len() {
                    let all_cells = VecOrVecModel::Vec(cells.clone());
                    
                    let possible_values = Self::compute_possible_values(i, &all_cells, &rows_columns);

                    possible_values_stack.push(possible_values);
                    indices_stack.push(0);
                }
                let possible_values = &possible_values_stack[i];

                if indices_stack[i] < possible_values.len() {
                    cells[i].value = possible_values[indices_stack[i]];
                    // println!("cell {} = {} | {:#?}", i, cells[i].value, &possible_values);
                    indices_stack[i] += 1;
                    i += 1;
                } else {
                    let number_of_possibilities = possible_values_stack.pop().unwrap().len();
                    indices_stack.pop();
                    // println!("cell {} <-", i);
                    if number_of_possibilities > 0 {
                        cells[i].value = -1;
                    }
                    i = if i > 0 { i - 1 } else { break; }
                }
            }
            if i != 0 {
                found_solutions.push(cells.clone());
            }
            if found_solutions.len() == 2 || i == 0 {
                break;
            }
            i -= 1;
        }

        match found_solutions.len() {
            0 => Str8tsSolution::None,
            1 => Str8tsSolution::Unique(found_solutions[0].clone()),
            2 => Str8tsSolution::Multiple(found_solutions[0].clone()),
            _ => panic!("Number of solutions not in [0, 1, 2] found, this should not happen!")
        }
    }

    fn generate_puzzle(&mut self) {
        let mut cells = self.cells.iter().collect::<Vec<_>>();
        let mut rng = rand::thread_rng();
        let mut fixed_indices = vec![];

        for cell in cells.iter_mut() {
            const P_WHITE: f64 = 0.6;
            cell.is_white = rng.gen_range(0.0..1.0) < P_WHITE;
            cell.value = -1;
            cell.is_fixed = false;
        }
        let rows_columns = Self::compute_rows_columns(&VecOrVecModel::Vec(cells.clone()));
        for i in 0..cells.len() {
            const P_FIXED: f64 = 0.0;
            if rng.gen_range(0.0..1.0) < P_FIXED {
                let all_cells = VecOrVecModel::Vec(cells.clone());
                let cell = &mut cells[i];
                let possible_values = Self::compute_possible_values(i, &all_cells, &rows_columns);
                cell.value = *possible_values.choose(&mut rng).unwrap_or(&-1);
                if cell.value > 0 {
                    fixed_indices.push(i);
                    cell.is_fixed = true;
                }
            }
        }

        let mut solution = None;
        for i in 0..1000 {
            match Self::solve_backtrack(cells.clone()) {
                Str8tsSolution::None => {
                    println!("Generating puzzle: i = {}. Lifting restriction.", i);
                    // Lift some restriction
                    if let Some((j, &cell_index)) = fixed_indices.iter().enumerate().last() { //.choose(&mut rng) {
                        cells[cell_index].value = -1;
                        cells[cell_index].is_fixed = false;
                        fixed_indices.remove(j);
                    } else {
                        println!("Cannot find any solution even without fixed numbers.");
                        break;
                    }
                },
                Str8tsSolution::Unique(solution_cells) => {
                    solution = Some(solution_cells);
                    break;
                },
                Str8tsSolution::Multiple(ref solution_cells) => {
                    // Impose more restrictions from found solution
                    let mut cell_index = rng.gen_range(0..cells.len());
                    let all_cells = VecOrVecModel::Vec(cells.clone());

                    const P_FILL_BLACK: f64 = 0.3;
                    while (cells[cell_index].is_fixed || solution_cells[cell_index].value < 0) && 
                            (cells[cell_index].is_white || solution_cells[cell_index].value > 0 
                            || Self::compute_possible_values(cell_index, &all_cells, &rows_columns).is_empty()
                            || rng.gen_range(0.0..1.0) > P_FILL_BLACK) {
                        cell_index = rng.gen_range(0..cells.len());
                    }
                    if solution_cells[cell_index].value > 0 {
                        // Make an empty white cell fixed
                        cells[cell_index].value = solution_cells[cell_index].value;
                    } else {
                        // Make an empty black cell fixed
                        cells[cell_index].value = *Self::compute_possible_values(cell_index, &all_cells, &rows_columns).choose(&mut rng).unwrap();
                    }
                    cells[cell_index].is_fixed = true;
                    fixed_indices.push(cell_index);
                    println!("Generating puzzle: i = {}. Imposing restriction. cell {} = {}", i, cell_index, cells[cell_index].value);
                },
            }
        }
        if let Some(solution_cells) = solution {
            println!("Puzzle with unique solution generated.");
            for i in 0..solution_cells.len() {
                let mut cell = solution_cells[i].clone();
                if !cell.is_fixed {
                    cell.value = -1;
                }
                self.cells.set_row_data(i, cell);
            }
        } else {
            println!("No puzzle generated.")
        }
    }

    fn solve_puzzle(&mut self) {
        let cells = self.cells.iter().collect::<Vec<_>>();
        let solution = Self::solve_backtrack(cells);
        match solution {
            Str8tsSolution::None => println!("No solution found."),
            Str8tsSolution::Unique(ref cells) | Str8tsSolution::Multiple(ref cells) => {
                for i in 0..cells.len() {
                    self.cells.set_row_data(i, cells[i].clone());
                }
                if let Str8tsSolution::Unique(_) = solution {
                    println!("Unique solution found.")
                } else {
                    println!("Multiple solutions found.")
                }
            }
        }
    }

    fn validate_board(&mut self) -> bool {
        let mut cell_data = (0..81).map(|index| {
            let mut cell = self.cells.row_data(index);
            cell.is_valid_in_row = true;
            cell.is_valid_in_straight = true;
            cell
        }).collect::<Vec<_>>();

        for row in &self.rows_columns {
            if let Some((multiple_occurrences, invalid_straights)) = row.validate() {
                for occurence_indices in multiple_occurrences {
                    for p in occurence_indices {
                        cell_data[p].is_valid_in_row = false;
                    }
                }
                for straight in invalid_straights {
                    for p in straight {
                        cell_data[p].is_valid_in_straight = false;
                    }
                }
            }
        }

        let mut is_valid = true;
        for (p, cell) in cell_data.iter_mut().enumerate() {
            self.cells.set_row_data(p, cell.clone());
            is_valid = is_valid && cell.is_valid_in_row && cell.is_valid_in_straight;
        }
        is_valid
    }

    fn piece_clicked(&mut self, p: i8) -> bool {
        let mut cell = self.cells.row_data(p as usize);
        
        match self.mode {
            Mode::EditBlackWhite => {
                cell.is_white = !cell.is_white;
                self.cells.set_row_data(p as usize, cell.clone());
                self.setup_rows_columns();
                self.validate_board();
            },
            Mode::EditFixedNumbers | Mode::PlayEnterNumbers | Mode::PlayEnterSmallNumbers => {
                if let Some(index) = self.editing_cell_index {
                    let mut editing_cell = self.cells.row_data(index as usize);
                    editing_cell.is_editing = false;
                    self.cells.set_row_data(index as usize, editing_cell);
                    self.editing_cell_index = None;
                }
                if !cell.is_fixed && cell.is_white || self.mode == Mode::EditFixedNumbers {
                    cell.is_editing = !cell.is_editing;
                    if cell.is_editing {
                        self.editing_cell_index = Some(p);
                    }
                }
                self.cells.set_row_data(p as usize, cell.clone());
            },
            _ => {}
        }
        true
    }

    fn cell_key_pressed(&mut self, p: i32, e: KeyEvent) -> Option<bool> {
        match self.mode {
            Mode::EditFixedNumbers | Mode::PlayEnterNumbers | Mode::PlayEnterSmallNumbers => {},
            _ => return None
        }

        let mut cell = self.cells.row_data(p as usize);
        if !cell.is_editing {
            return None;
        }

        let new_value = if let Ok(k) = e.text.parse::<i32>() {
            Some(k).filter(|&k| k >= 1 && k <= 9)
        } 
        else if e.text == "\u{7}" || e.text == "\u{7f}" {
            Some(-1)
        }
        else { None };
        
        if let Some(val) = new_value {
            if self.mode == Mode::EditFixedNumbers || self.mode == Mode::PlayEnterNumbers {
                cell.value = val;
                cell.is_editing = false;
                cell.is_fixed = if self.mode == Mode::EditFixedNumbers && val > 0 {true} else {false};
                self.editing_cell_index = None;
            } else if self.mode == Mode::PlayEnterSmallNumbers && val > 0 {
                let mut small_numbers = cell.small_values.iter().collect::<Vec<bool>>();
                small_numbers[(val - 1) as usize] = !small_numbers[(val - 1) as usize];
                // let number_toggled = cell.small_values.row_data((val - 1) as usize);
                // cell.small_values.0.s[(val - 1) as usize] = !number_toggled;
                // cell.small_values.set_row_data(3, true);
                cell.small_values = ModelHandle::new(Rc::new(VecModel::from(small_numbers)));
            }
            self.cells.set_row_data(p as usize, cell)
        }

        let is_valid = self.validate_board();
        let is_complete = self.is_complete();
        Some(is_valid && is_complete)
    }

    fn is_complete(&self) -> bool {
        if self.cells.iter().any(|cell| cell.value <= 0 && cell.is_white) {
            return false;
        }
        true
    }

    fn set_mode(&mut self, mode: &str) {
        self.mode = match mode {
            "edit-black-white" => { 
                // Make editing cell non-editing
                if let Some(editing_cell_index) = self.editing_cell_index {
                    let mut cell = self.cells.row_data(editing_cell_index as usize);
                    cell.is_editing = false;
                    self.cells.set_row_data(editing_cell_index as usize, cell);
                    self.editing_cell_index = None;
                }
                Mode::EditBlackWhite
            },
            "edit-fixed-numbers" => Mode::EditFixedNumbers,
            "play-enter-numbers" => Mode::PlayEnterNumbers,
            "play-enter-small-numbers" => Mode::PlayEnterSmallNumbers,
            "none" => Mode::None,
            _ => panic!("Unknown mode: \"{}\"", mode)
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
pub fn main() {
    // This provides better error messages in debug mode.
    // It's disabled in release mode so it doesn't bloat up the file size.
    #[cfg(all(debug_assertions, target_arch = "wasm32"))]
    console_error_panic_hook::set_once();

    let main_window = MainWindow::new();
    let state = Rc::new(RefCell::new(AppState {
        cells: Rc::new(sixtyfps::VecModel::<Cell>::from(random_puzzle())),
        main_window: main_window.as_weak(),
        // positions: vec![],
        auto_play_timer: Default::default(),
        was_just_solved_timer: Default::default(),
        finished: false,
        editing_cell_index: None,
        rows_columns: vec![],
        mode: Mode::None,
    }));

    if std::path::Path::new(SAVEGAME_PATH).exists() {
        state.borrow_mut().load_from_file(SAVEGAME_PATH);
    } else {
        state.borrow_mut().randomize();
    }
    main_window.set_cells(sixtyfps::ModelHandle::new(state.borrow().cells.clone()));
    state.borrow_mut().setup_rows_columns();
    state.borrow_mut().validate_board();

    let state_copy = state.clone();
    main_window.on_piece_clicked(move |p| {
        state_copy.borrow().auto_play_timer.stop();
        state_copy.borrow().main_window.unwrap().set_auto_play(false);
        if state_copy.borrow().finished {
            return;
        }
        state_copy.borrow_mut().piece_clicked(p as i8);
    });

    let state_copy = state.clone();
    main_window.on_cell_key_pressed(move |p, e| {
        state_copy.borrow().auto_play_timer.stop();
        state_copy.borrow().main_window.unwrap().set_auto_play(false);
        if state_copy.borrow().finished {
            return;
        }
        let was_just_solved = state_copy.borrow_mut().cell_key_pressed(p, e);
        if let Some(true) = was_just_solved {
            state_copy.borrow().main_window.unwrap().set_was_just_solved(true);
            let state_weak = Rc::downgrade(&state_copy);
            state_copy.borrow().was_just_solved_timer.start(
                sixtyfps::TimerMode::SingleShot,
                std::time::Duration::from_millis(400),
                move || {
                    if let Some(state) = state_weak.upgrade() {
                        state.borrow().main_window.unwrap().set_was_just_solved(false);
                    }
                }
            );
        }
    });

    let state_copy = state.clone();
    main_window.on_set_mode(move |mode| {
        state_copy.borrow().auto_play_timer.stop();
        state_copy.borrow().main_window.unwrap().set_auto_play(false);
        if state_copy.borrow().finished {
            return;
        }
        state_copy.borrow_mut().set_mode(&mode);
    });

    let state_copy = state.clone();
    main_window.on_solve_puzzle(move || {
        state_copy.borrow().auto_play_timer.stop();
        state_copy.borrow().main_window.unwrap().set_auto_play(false);
        if state_copy.borrow().finished {
            return;
        }
        state_copy.borrow_mut().solve_puzzle();
    });

    let state_copy = state.clone();
    main_window.on_save_game(move || {
        state_copy.borrow().auto_play_timer.stop();
        state_copy.borrow().main_window.unwrap().set_auto_play(false);
        if state_copy.borrow().finished {
            return;
        }
        state_copy.borrow_mut().save_to_file(SAVEGAME_PATH);
    });

    let state_copy = state.clone();
    main_window.on_generate_puzzle(move || {
        state_copy.borrow().auto_play_timer.stop();
        state_copy.borrow().main_window.unwrap().set_auto_play(false);
        if state_copy.borrow().finished {
            return;
        }
        state_copy.borrow_mut().generate_puzzle();
    });

    let state_copy = state.clone();
    main_window.on_reset(move || {
        state_copy.borrow().auto_play_timer.stop();
        state_copy.borrow().main_window.unwrap().set_auto_play(false);
        state_copy.borrow_mut().randomize();
    });

    // let state_copy = state.clone();
    // main_window.on_enable_auto_mode(move |enabled| {
    //     if enabled {
    //         let state_weak = Rc::downgrade(&state_copy);
    //         state_copy.borrow().auto_play_timer.start(
    //             sixtyfps::TimerMode::Repeated,
    //             std::time::Duration::from_millis(200),
    //             move || {
    //                 if let Some(state) = state_weak.upgrade() {
    //                     // state.borrow_mut().random_move();
    //                 }
    //             },
    //         );
    //     } else {
    //         state_copy.borrow().auto_play_timer.stop();
    //     }
    // });

    main_window.run();
}
