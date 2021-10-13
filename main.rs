/* LICENSE BEGIN
    This file is part of SixtyFPS-Str8ts, a demo implementing the
    Str8ts puzzle in the SixtyFPS framework. Based on the SixtyFPS
    Slide Puzzle demo.
    Copyright (c) 2021 Vincent Bürgin <v.buergin@gmx.de>

    SPDX-License-Identifier: GPL-3.0-only
LICENSE END */

mod vec_or_vec_model;
mod str8ts_row;
mod str8ts_board;

use sixtyfps::Model;
use sixtyfps::ModelHandle;
use sixtyfps::VecModel;
use sixtyfps::re_exports::KeyEvent;
use std::cell::RefCell;
use std::rc::Rc;
use serde_json;
use vec_or_vec_model::VecOrVecModel;
use str8ts_row::Row;
use str8ts_board::{solve_backtrack, generate_puzzle, compute_rows_columns,
    empty_board, random_board, Str8tsSolution};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

sixtyfps::include_modules!();

const SAVEGAME_PATH: &str = "./game_state.json";
const P_FIXED: f64 = 0.0;
const P_WHITE: f64 = 1.0;

impl Cell {
    fn new(i: i32, value: i32, is_white: bool, is_fixed: bool) -> Cell {
        Cell {
            index: i, value, is_white, is_fixed,
            pos_x: i % 9, pos_y: i / 9,
            small_values: ModelHandle::new(Rc::new(VecModel::from(vec![false; 9]))),
            is_editing: false,
            is_valid_in_row: true,
            is_valid_in_straight: true,
        }
    }
}

// Stores the UI state
struct AppState {
    cells: Rc<sixtyfps::VecModel<Cell>>,
    main_window: sixtyfps::Weak<MainWindow>,
    was_just_solved_timer: sixtyfps::Timer,
    editing_cell_index: Option<i8>,
    rows_columns: Vec<Row>,
    mode: GameMode,
}

// Represents game modes
#[derive(PartialEq)]
enum GameMode {
    None,
    EditBlackWhite,
    EditFixedNumbers,
    PlayEnterNumbers,
    PlayEnterSmallNumbers
}

impl AppState {
    // Set UI state to a board state
    fn set_board(&mut self, cells: &Vec<Cell>) {
        for (i, cell) in cells.iter().enumerate() {
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
        self.rows_columns = compute_rows_columns(&VecOrVecModel::VecModel(self.cells.clone()));
    }

    // Run backtracking and write solution to UI
    fn solve_puzzle(&mut self) {
        let cells = self.cells.iter().collect::<Vec<_>>();
        let solution = solve_backtrack(cells);
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

    fn generate_puzzle(&mut self) {
        if let Some(puzzle) = generate_puzzle()  {
            println!("Puzzle with unique solution generated.");
            self.set_board(&puzzle);
        } else {
            println!("No puzzle generated.")
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
        self.set_board(&cell_data);
        
        // Determine and return whether overall board is valid
        cell_data.iter().all(|cell| cell.is_valid_in_row && cell.is_valid_in_straight)
    }

    // Handle a click on a cell
    fn cell_clicked(&mut self, p: i8) -> bool {
        let mut cell = self.cells.row_data(p as usize);
        
        match self.mode {
            // Edit black/white mode: switch black/white re-setup row/column structure and revalidate
            GameMode::EditBlackWhite => {
                cell.is_white = !cell.is_white;
                self.cells.set_row_data(p as usize, cell.clone());
                self.setup_rows_columns();
                self.validate_board();
            },
            // Edit fixed/non-fixed/small numbers modes: enter editing mode
            GameMode::EditFixedNumbers | GameMode::PlayEnterNumbers | GameMode::PlayEnterSmallNumbers => {
                // Reset currently editing cell
                if let Some(index) = self.editing_cell_index {
                    let mut editing_cell = self.cells.row_data(index as usize);
                    editing_cell.is_editing = false;
                    self.cells.set_row_data(index as usize, editing_cell);
                    self.editing_cell_index = None;
                }
                // If new cell can be edited, set it to editing mode
                if !cell.is_fixed && cell.is_white || self.mode == GameMode::EditFixedNumbers {
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
            GameMode::EditFixedNumbers | GameMode::PlayEnterNumbers | GameMode::PlayEnterSmallNumbers => {},
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
            if self.mode == GameMode::EditFixedNumbers || self.mode == GameMode::PlayEnterNumbers {
                cell.value = val;
                cell.is_editing = false;
                cell.is_fixed = if self.mode == GameMode::EditFixedNumbers && val > 0 {true} else {false};
                self.editing_cell_index = None;
            } 
            // Enter small number
            else if self.mode == GameMode::PlayEnterSmallNumbers && val > 0 {
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
                GameMode::EditBlackWhite
            },
            "edit-fixed-numbers" => GameMode::EditFixedNumbers,
            "play-enter-numbers" => GameMode::PlayEnterNumbers,
            "play-enter-small-numbers" => GameMode::PlayEnterSmallNumbers,
            "none" => GameMode::None,
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
        cells: Rc::new(sixtyfps::VecModel::<Cell>::from(random_board(P_FIXED, P_WHITE))),
        main_window: main_window.as_weak(),
        was_just_solved_timer: Default::default(),
        editing_cell_index: None,
        rows_columns: vec![],
        mode: GameMode::None,
    }));

    // Load a savegame if it exists, otherwise randomize the board
    if std::path::Path::new(SAVEGAME_PATH).exists() {
        state.borrow_mut().load_from_file(SAVEGAME_PATH);
    } 
    else {
        state.borrow_mut().set_board(&random_board(P_FIXED, P_WHITE));
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
        state_copy.borrow_mut().set_board(&empty_board());
    });

    main_window.run();
}
