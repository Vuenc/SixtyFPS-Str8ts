/* LICENSE BEGIN
    This file is part of the SixtyFPS Project -- https://sixtyfps.io
    Copyright (c) 2021 Olivier Goffart <olivier.goffart@sixtyfps.io>
    Copyright (c) 2021 Simon Hausmann <simon.hausmann@sixtyfps.io>

    SPDX-License-Identifier: GPL-3.0-only
    This file is also available under commercial licensing terms.
    Please contact info@sixtyfps.io for more information.
LICENSE END */

use rand::prelude::SliceRandom;
use sixtyfps::Model;
use sixtyfps::ModelHandle;
use sixtyfps::VecModel;
use sixtyfps::re_exports::KeyEvent;
use std::cell::RefCell;
use std::rc::Rc;
use rand::Rng;
use serde_json;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

sixtyfps::include_modules!();

const SAVEGAME_PATH: &str = "./game_state.json";
const P_FIXED: f64 = 0.0;
const P_WHITE: f64 = 1.0;

// Generates a random puzzle with given probabilities for
// fixed-number cells and white cells. Usually the resulting
// puzzle is not valid, let alone has a unique solution.
fn random_puzzle(p_fixed: f64, p_white: f64) -> Vec<Cell> {
    let mut rng = rand::thread_rng();
    let mut vec = vec!();
    for i in 0..81 {
        // Determine is_fixed and is_white randomly
        let is_fixed = rng.gen_range(0.0..1.0) < p_fixed;
        let is_white = rng.gen_range(0.0..1.0) < p_white;
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

// Stores the UI state
struct AppState {
    cells: Rc<sixtyfps::VecModel<Cell>>,
    main_window: sixtyfps::Weak<MainWindow>,
    was_just_solved_timer: sixtyfps::Timer,
    editing_cell_index: Option<i8>,
    rows_columns: Vec<Row>,
    mode: Mode,
}

// Stores either a Vec<T> (for working outside of UI) or a
// sixtyfps VecModel<T> (for working with the UI), provides interface
// to get/set functionality. Used here for T = Cell to implement
// methods that can handle both formats.
#[derive(Clone)]
enum VecOrVecModel<T> where T: Clone {
    Vec(Vec<T>),
    VecModel(Rc<sixtyfps::VecModel<T>>)
}

// Implement get/set for VecOrVecModel
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

// Represents a row/column and its straights
struct Row {
    row_cells: Vec<usize>, // indices of cells in row/column
    straights: Vec<Vec<usize>>, // straights, stored as index vectors
}

// Represents game modes
#[derive(PartialEq)]
enum Mode {
    None,
    EditBlackWhite,
    EditFixedNumbers,
    PlayEnterNumbers,
    PlayEnterSmallNumbers
}

// Represents whether the game has no/one/multiple solutions
// (including one solution in the latter cases)
enum Str8tsSolution {
    None,
    Unique(Vec<Cell>),
    Multiple(Vec<Cell>)
}

impl Row {
    // Create new row, recognize it straights
    fn new(row_cells: Vec<usize>, all_cells: &VecOrVecModel<Cell>) -> Row {
        let straights = row_cells.iter()
            .map(|&i| (i, all_cells.get(i).is_white)).collect::<Vec<(usize, bool)>>()
            .split(|(_, is_white)| !is_white)
            .filter(|&slice| slice.len() > 0)
            .map(|slice| slice.iter().map(|(i, _)| *i).collect()).collect();
        Row { row_cells, straights }
    }

    // Validate a row: find duplicate values and invalid straights
    fn validate(&self, all_cells: &VecOrVecModel<Cell>) -> Option<(Vec<Vec<usize>>, Vec<Vec<usize>>)> {
        // Per value, store the indices of cells it occurs in
        let mut occurrences: [Vec<usize>; 9] = Default::default();
        for &i in &self.row_cells {
            let value = all_cells.get(i).value;
            if value > 0 {
                occurrences[(value - 1) as usize].push(i);
            }
        }
        // Find the values with multiple occurences
        let multiple_occurrences = occurrences.iter()
            .filter(|num_occs| num_occs.len() > 1)
            .cloned().collect::<Vec<_>>();

        // Map each straight to a Vec of the values of its non-empty cells
        let straights_values = self.straights.iter()
            .map(|straight| 
                straight.iter().map(|&i| all_cells.get(i).value)
                .filter(|&value| value > 0)
                .collect::<Vec<_>>())
            .enumerate()
            .filter(|(_, values)| values.len() > 0)
            .collect::<Vec<_>>();
        // Find straights where the min and max value are too far apart
        let invalid_straights = straights_values.iter()
            .filter(|(k, values)|
                (values.iter().max().unwrap() - values.iter().min().unwrap()) as usize >= self.straights[*k].len())
            .map(|(k, _)| self.straights[*k].clone())
            .collect::<Vec<_>>();

        // If some duplicate occurence or invalid straight exists, return it
        if multiple_occurrences.len() > 0 || invalid_straights.len() > 0 {
            return Some((multiple_occurrences, invalid_straights));
        } else {
            return None;
        }
    }

    // Compute the values not yet present in the row, intersected with candidate_values if provided
    fn missing_values_cells(&self, candidate_values: Option<&[i32]>, all_cells: &VecOrVecModel<Cell>)
            -> Vec<i32> {
        let mut values_present = [false; 9];
        let mut candidate_values_present = [false; 9];

        // Compute which values are present in the row
        for &i in &self.row_cells {
            let val = all_cells.get(i).value;
            if val > 0 {
                values_present[(val - 1) as usize] = true;
            }
        }
        // Compute which candidate values are present
        if let Some(values) = candidate_values {
            for &val in values {
                candidate_values_present[(val - 1) as usize] = true;
            }
        }
        // Return values which are not present in row, but present in candidate values
        values_present.iter().enumerate()
            .filter(|&(val, is_present)| !is_present 
                && (candidate_values_present[val] || candidate_values.is_none()))
            .map(|(val, _)| (val + 1) as i32).collect()
    }

    // Compute the values possible in the cell's straight without violating the
    // straight rule, intersected with candidate_values if provided
    fn possible_straight_values_cells(&self, cell_index: usize, candidate_values: &[i32], 
            all_cells: &VecOrVecModel<Cell>) -> Vec<i32> {
        // Find the straight the cell is in
        let straight_indices = self.straights.iter()
            .find(|s| s.contains(&cell_index))
            .expect("Cell not in any straight.");

        // Get the values of non-empty cells in the straight
        let straight = straight_indices.iter()
            .map(|&i| all_cells.get(i).value).filter(|&v| v > 0).collect::<Vec<_>>();

        // If the straight is not empty (i.e. min/max exist):
        // Keep the candidate values that would not extend the straight too far
        if let (Some(&min), Some(&max)) = (straight.iter().min(), straight.iter().max()) {
            let len = straight_indices.len() as i32;
            candidate_values.iter().filter(|&&val| {
                (min < val && val < max)
                || (val < min && max - val < len)
                || (max < val && val - min < len)
            }).map(|&val| val)
            .collect()
        }
        // For an empty straight, return all candidate values
        else {
            return candidate_values.into();
        }
    }
}

impl AppState {
    // Generate random puzzle and set UI state to this puzzle
    fn randomize(&mut self, p_fixed: f64, p_white: f64) {
        let puzzle_cells = random_puzzle(p_fixed, p_white);
        for (i, cell) in puzzle_cells.iter().enumerate() {
            self.cells.set_row_data(i, cell.clone());
        }
    }

    // Serialize current game state to a JSON file
    fn save_to_file(&self, path: &str) {
        // For each cell: save a tuple (value, is_white, is_fixed, small_values)
        let cells_data = self.cells.iter()
            .map(|cell| (cell.value, cell.is_white, cell.is_fixed, cell.small_values.iter().collect::<Vec<bool>>()))
            .collect::<Vec<_>>();

        let json_data = serde_json::to_string(&cells_data)
            .expect("Unable to save game: unable to create JSON.");
        std::fs::write(path, json_data)
            .expect(&format!("Unable to save game: unable to write file {}.", path));
    }

    // Load game state from a JSON file
    fn load_from_file(&mut self, path: &str) {
        let json_data = std::fs::read_to_string(path)
            .expect(&format!("Unable to load game: unable to read file {}.", path));
        let mut cells_data: Vec<(i32, bool, bool, Vec<bool>)> = 
            serde_json::from_str(&json_data)
            .expect("Unable to load game: unable to parse JSON.");
        for (i, data) in cells_data.drain(..).enumerate() {
            // Check validity of cell data
            assert!(data.0 == -1 || (data.0 >= 1 && data.0 <= 9), "Unable to load game: invalid cell value.");
            assert!(data.3.len() == 9, "Unable to load game: invalid small values.");

            // Write data into a new cell
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

    // Recognize row/column straights structure
    fn compute_rows_columns(cells: &VecOrVecModel<Cell>) -> Vec<Row> {
        let mut rows_columns = vec![];
        for row in 0..9 {
            let indices = (0..9).map(|j| 9*row + j).collect::<Vec<_>>();
            rows_columns.push(Row::new(indices, cells));
        }
        for column in 0..9 {
            let indices = (0..9).map(|j| column + 9*j).collect::<Vec<_>>();
            rows_columns.push(Row::new(indices, cells));
        }
        rows_columns
    }

    // Compute currently possible values in a cell that are not duplicate in the
    // row and column and do not violate the straights rule
    fn compute_possible_values(cell_index: usize, all_cells: &VecOrVecModel<Cell>, rows_columns: &Vec<Row>)
            -> Vec<i32> {
        // Missing values row
        let mut possible_values = rows_columns[cell_index / 9]
            .missing_values_cells(None, &all_cells);
        
        // \cup Missing values column
        possible_values = rows_columns[9 + cell_index % 9]
            .missing_values_cells(Some(&possible_values), &all_cells);
        
        if all_cells.get(cell_index).is_white {
            // \cup possible straight in row values
            possible_values = rows_columns[cell_index / 9]
                .possible_straight_values_cells(cell_index, &possible_values, &all_cells);
            
            // \cup possible straight in row values
            possible_values = rows_columns[9 + cell_index % 9]
                .possible_straight_values_cells(cell_index, &possible_values, &all_cells);
        }

        possible_values
    }

    // Solve puzzle via backtracking (can take a long time). Returns if the puzzle
    // has no solution, a unique solution or multiple solutions.
    fn solve_backtrack(mut cells: Vec<Cell>) -> Str8tsSolution {
        // Works on a copy of the board, so recompute the rows/columns
        let rows_columns = Self::compute_rows_columns(&VecOrVecModel::Vec(cells.clone()));

        // Backtracking stacks: if i > j, cell j either had value beforehand, is black, or 
        // has the value possible_values_stack[i][indices_stack[i]]
        let mut indices_stack = vec![];
        let mut possible_values_stack = vec![];
        let mut i = 0;

        // Continue until at least 2 solutions are found or the backtracking terminates
        let mut found_solutions = vec![];
        while found_solutions.len() < 2 {
            while i < cells.len() {
                // Skip new cells where no value is needed (already had a value or black)
                if (!cells[i].is_white || cells[i].value > 0) && i >= possible_values_stack.len() {
                    possible_values_stack.push(vec![]);
                    indices_stack.push(0);
                    i += 1;
                    continue;
                }

                // If no possible values are computed yet, compute and put on stack
                if i >= possible_values_stack.len() {
                    // all_cells: Clone of current state wrapped in VecOrVecModel abstraction
                    let all_cells = VecOrVecModel::Vec(cells.clone());
                    let possible_values = Self::compute_possible_values(i, &all_cells, &rows_columns);
                    possible_values_stack.push(possible_values);
                    indices_stack.push(0);
                }
                let possible_values = &possible_values_stack[i];

                // If not all possible values have been exhausted, try the next one
                if indices_stack[i] < possible_values.len() {
                    cells[i].value = possible_values[indices_stack[i]];
                    indices_stack[i] += 1;
                    i += 1;
                } 
                // Otherwise, give up this cell and backtrack
                else {
                    let number_of_possibilities = possible_values_stack.pop().unwrap().len();
                    indices_stack.pop();
                    if number_of_possibilities > 0 {
                        cells[i].value = -1;
                    }
                    i = if i > 0 { i - 1 } else { break; }
                }
            }
            // If the inner loop finishes and i != 0, a solution has been found
            if i != 0 {
                found_solutions.push(cells.clone());
                i -= 1;
            } 
            // If i = 0, no (further) solutions exist
            else {
                break;
            }
        }

        // If at least one solution was found, return it. Return information if no/one/multiple solution exist.
        match found_solutions.len() {
            0 => Str8tsSolution::None,
            1 => Str8tsSolution::Unique(found_solutions[0].clone()),
            2 => Str8tsSolution::Multiple(found_solutions[0].clone()),
            _ => panic!("Number of solutions not in [0, 1, 2] found, this should not happen!")
        }
    }

    // Function that should generate a puzzle. Non-functional as of yet.
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

    // Run backtracking and write solution to UI
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

    // Check if board is valid, and mark invalid cells along the way
    fn validate_board(&mut self) -> bool {
        // Clone cells from UI with valid values set to true
        let mut cell_data = (0..81).map(|index| {
            let mut cell = self.cells.row_data(index);
            cell.is_valid_in_row = true;
            cell.is_valid_in_straight = true;
            cell
        }).collect::<Vec<_>>();

        let all_cells = VecOrVecModel::VecModel(self.cells.clone());
        // Validate each row and handle the invalid cells if some exist
        for row in &self.rows_columns {
            if let Some((multiple_occurrences, invalid_straights))
                    = row.validate(&all_cells) {
                // Mark cells that are duplicate in the row/column
                for occurence_indices in multiple_occurrences {
                    for p in occurence_indices {
                        cell_data[p].is_valid_in_row = false;
                    }
                }
                // Mark all cells of straights that are invalid
                for straight in invalid_straights {
                    for p in straight {
                        cell_data[p].is_valid_in_straight = false;
                    }
                }
            }
        }

        // Write back updated cells to UI
        for (p, cell) in cell_data.iter().enumerate() {
            self.cells.set_row_data(p, cell.clone());
        }
        // Determine and return whether overall board is valid
        cell_data.iter().all(|cell| cell.is_valid_in_row && cell.is_valid_in_straight)
    }

    // Handle a click on a cell
    fn cell_clicked(&mut self, p: i8) -> bool {
        let mut cell = self.cells.row_data(p as usize);
        
        match self.mode {
            // Edit black/white mode: switch black/white re-setup row/column structure and revalidate
            Mode::EditBlackWhite => {
                cell.is_white = !cell.is_white;
                self.cells.set_row_data(p as usize, cell.clone());
                self.setup_rows_columns();
                self.validate_board();
            },
            // Edit fixed/non-fixed/small numbers modes: enter editing mode
            Mode::EditFixedNumbers | Mode::PlayEnterNumbers | Mode::PlayEnterSmallNumbers => {
                // Reset currently editing cell
                if let Some(index) = self.editing_cell_index {
                    let mut editing_cell = self.cells.row_data(index as usize);
                    editing_cell.is_editing = false;
                    self.cells.set_row_data(index as usize, editing_cell);
                    self.editing_cell_index = None;
                }
                // If new cell can be edited, set it to editing mode
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

    // Handle keyboard inputs on cells
    fn cell_key_pressed(&mut self, p: i32, e: KeyEvent) -> Option<bool> {
        // Only proceed if game is in number editing mode
        match self.mode {
            Mode::EditFixedNumbers | Mode::PlayEnterNumbers | Mode::PlayEnterSmallNumbers => {},
            _ => return None
        }

        // Only proceed if the cell is in editing mode
        let mut cell = self.cells.row_data(p as usize);
        if !cell.is_editing {
            return None;
        }

        // Only process digits 1-9, backspace, del keys
        let new_value = if let Ok(k) = e.text.parse::<i32>() {
            Some(k).filter(|&k| k >= 1 && k <= 9)
        } 
        else if e.text == "\u{7}" || e.text == "\u{7f}" {
            Some(-1)
        }
        else { None };
        
        if let Some(val) = new_value {
            // Enter cell value (fixed or non-fixed)
            if self.mode == Mode::EditFixedNumbers || self.mode == Mode::PlayEnterNumbers {
                cell.value = val;
                cell.is_editing = false;
                cell.is_fixed = if self.mode == Mode::EditFixedNumbers && val > 0 {true} else {false};
                self.editing_cell_index = None;
            } 
            // Enter small number
            else if self.mode == Mode::PlayEnterSmallNumbers && val > 0 {
                let mut small_numbers = cell.small_values.iter().collect::<Vec<bool>>();
                small_numbers[(val - 1) as usize] = !small_numbers[(val - 1) as usize];
                // Necessary to write the whole array, can't change a single value
                cell.small_values = ModelHandle::new(Rc::new(VecModel::from(small_numbers)));
            }
            self.cells.set_row_data(p as usize, cell)
        }

        // Determine and return if puzzle is solved (board is complete and valid)
        let is_valid = self.validate_board();
        let is_complete = self.is_complete();
        Some(is_valid && is_complete)
    }

    // Check if board is complete (no empty white cells)
    fn is_complete(&self) -> bool {
        !self.cells.iter().any(|cell| cell.value <= 0 && cell.is_white)
    }

    // Set game mode (editing board/entering numbers for playing)
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
        cells: Rc::new(sixtyfps::VecModel::<Cell>::from(random_puzzle(P_FIXED, P_WHITE))),
        main_window: main_window.as_weak(),
        was_just_solved_timer: Default::default(),
        editing_cell_index: None,
        rows_columns: vec![],
        mode: Mode::None,
    }));

    // Load a savegame if it exists, otherwise randomize the board
    if std::path::Path::new(SAVEGAME_PATH).exists() {
        state.borrow_mut().load_from_file(SAVEGAME_PATH);
    } 
    else {
        state.borrow_mut().randomize(P_FIXED, P_WHITE);
    }
    // Setup cells, compute row/column straight structure, validate
    main_window.set_cells(sixtyfps::ModelHandle::new(state.borrow().cells.clone()));
    state.borrow_mut().setup_rows_columns();
    state.borrow_mut().validate_board();

    // Handle cell-clicked callback
    let state_copy = state.clone();
    main_window.on_cell_clicked(move |p| {
        state_copy.borrow_mut().cell_clicked(p as i8);
    });

    // Handle cell-key-pressed callback
    let state_copy = state.clone();
    main_window.on_cell_key_pressed(move |p, e| {
        let was_just_solved = state_copy.borrow_mut().cell_key_pressed(p, e);
        if let Some(true) = was_just_solved {
            // If the game was solved: start timer to realize flashing animation
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

    // Handle set-mode callback
    let state_copy = state.clone();
    main_window.on_set_mode(move |mode| {
        state_copy.borrow_mut().set_mode(&mode);
    });

    // Handle solve-puzzle callback
    let state_copy = state.clone();
    main_window.on_solve_puzzle(move || {
        state_copy.borrow_mut().solve_puzzle();
    });

    // Handle save-game callback
    let state_copy = state.clone();
    main_window.on_save_game(move || {
        state_copy.borrow_mut().save_to_file(SAVEGAME_PATH);
    });

    // Handle generate-puzzle callback (currently deactivated)
    let state_copy = state.clone();
    main_window.on_generate_puzzle(move || {
        state_copy.borrow_mut().generate_puzzle();
    });

    // Handle reset callback
    let state_copy = state.clone();
    main_window.on_reset(move || {
        state_copy.borrow_mut().randomize(0.0, 0.0);
    });

    main_window.run();
}
