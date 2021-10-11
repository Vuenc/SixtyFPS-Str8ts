/* LICENSE BEGIN
    This file is part of the SixtyFPS Project -- https://sixtyfps.io
    Copyright (c) 2021 Olivier Goffart <olivier.goffart@sixtyfps.io>
    Copyright (c) 2021 Simon Hausmann <simon.hausmann@sixtyfps.io>

    SPDX-License-Identifier: GPL-3.0-only
    This file is also available under commercial licensing terms.
    Please contact info@sixtyfps.io for more information.
LICENSE END */

use sixtyfps::Model;
use sixtyfps::ModelHandle;
use sixtyfps::VecModel;
use sixtyfps::re_exports::KeyEvent;
use std::cell::RefCell;
use std::rc::Rc;
use rand::Rng;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

sixtyfps::include_modules!();

fn random_puzzle() -> Vec<Cell> {
    let mut rng = rand::thread_rng();
    let mut vec = vec!();
    for i in 0..81 {
        const P_FIXED: f64 = 0.15;
        const P_WHITE: f64 = 0.8;
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
    /// The speed in the x and y direction for the associated tile
    finished: bool,
    editing_cell_index: Option<i8>,
    rows_columns: Vec<Row>,
    mode: Mode,
}

struct Row {
    row_cells: Vec<usize>,
    straights: Vec<Vec<usize>>,
    all_cells: Rc<sixtyfps::VecModel<Cell>>
}

#[derive(PartialEq)]
enum Mode {
    None,
    EditBlackWhite,
    EditFixedNumbers,
    PlayEnterNumbers,
    PlayEnterSmallNumbers
}

impl Row {
    fn new(row_cells: Vec<usize>, all_cells: Rc<sixtyfps::VecModel<Cell>>) -> Row {
        let straights = row_cells.iter()
            .map(|&i| (i, all_cells.row_data(i).is_white)).collect::<Vec<(usize, bool)>>()
            .split(|(_, is_white)| !is_white)
            // .filter(|&slice| slice.len() > 0)
            .map(|slice| slice.iter().map(|(i, _)| *i).collect()).collect();
        Row { row_cells, straights, all_cells }
    }

    fn validate(&self) -> Option<(Vec<Vec<usize>>, Vec<Vec<usize>>)> {
        let mut occurrences: [Vec<usize>; 9] = Default::default();
        for &i in &self.row_cells {
            let value = self.all_cells.row_data(i).value;
            if value > 0 {
                occurrences[(value - 1) as usize].push(i);
            }
        }
        let multiple_occurrences = occurrences.iter()
            .filter(|num_occs| num_occs.len() > 1)
            .cloned().collect::<Vec<_>>();

        let straights_values = self.straights.iter()
            .map(|straight| 
                straight.iter().map(|&i| self.all_cells.row_data(i).value)
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
}

impl AppState {
    fn randomize(&mut self) {
        // self.positions = random_puzzle();
        let puzzle_cells = random_puzzle();
        for (i, cell) in puzzle_cells.iter().enumerate() {
            self.cells.set_row_data(i, cell.clone());
        }
        self.setup_rows_columns();
        self.validate_board();
        // for (i, p) in self.positions.iter().enumerate() {
        //     self.set_pieces_pos(*p, i as _);
        // }
        self.main_window.unwrap().set_moves(0);
        // self.apply_tiles_left();
    }

    fn setup_rows_columns(&mut self) {
        self.rows_columns = vec![];
        for row in 0..9 {
            let indices = (0..9).map(|j| row + 9*j).collect::<Vec<_>>();
            self.rows_columns.push(Row::new(indices, self.cells.clone()));
        }
        for column in 0..9 {
            let indices = (0..9).map(|j| j + column*9).collect::<Vec<_>>();
            self.rows_columns.push(Row::new(indices, self.cells.clone()));
        }
    }

    fn validate_board(&mut self) {
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

        for (p, cell) in cell_data.iter_mut().enumerate() {
            self.cells.set_row_data(p, cell.clone());
        }
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

    fn cell_key_pressed(&mut self, p: i32, e: KeyEvent) {
        match self.mode {
            Mode::EditFixedNumbers | Mode::PlayEnterNumbers | Mode::PlayEnterSmallNumbers => {},
            _ => return
        }

        let mut cell = self.cells.row_data(p as usize);
        if !cell.is_editing {
            return;
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

        self.validate_board();
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
        finished: false,
        editing_cell_index: None,
        rows_columns: vec![],
        mode: Mode::None,
    }));
    state.borrow_mut().randomize();
    main_window.set_cells(sixtyfps::ModelHandle::new(state.borrow().cells.clone()));

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
        state_copy.borrow_mut().cell_key_pressed(p, e);
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
    main_window.on_reset(move || {
        state_copy.borrow().auto_play_timer.stop();
        state_copy.borrow().main_window.unwrap().set_auto_play(false);
        state_copy.borrow_mut().randomize();
    });

    let state_copy = state;
    main_window.on_enable_auto_mode(move |enabled| {
        if enabled {
            let state_weak = Rc::downgrade(&state_copy);
            state_copy.borrow().auto_play_timer.start(
                sixtyfps::TimerMode::Repeated,
                std::time::Duration::from_millis(200),
                move || {
                    if let Some(state) = state_weak.upgrade() {
                        // state.borrow_mut().random_move();
                    }
                },
            );
        } else {
            state_copy.borrow().auto_play_timer.stop();
        }
    });
    main_window.run();
}
