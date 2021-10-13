use crate::sixtyfps_generated_MainWindow::Cell;
use rand::prelude::SliceRandom;
use rand::Rng;
use crate::vec_or_vec_model::VecOrVecModel;
use crate::str8ts_row::Row;

// Represents whether the game has no/one/multiple solutions
// (including one solution in the latter cases)
pub enum Str8tsSolution {
    None,
    Unique(Vec<Cell>),
    Multiple(Vec<Cell>)
}

// Generates a random puzzle with given probabilities for
// fixed-number cells and white cells. Usually the resulting
// puzzle is not valid, let alone has a unique solution.
pub fn random_board(p_fixed: f64, p_white: f64) -> Vec<Cell> {
    let mut rng = rand::thread_rng();
    let mut vec = vec!();
    for i in 0..81 {
        // Determine is_fixed and is_white randomly
        let is_fixed = rng.gen_range(0.0..1.0) < p_fixed;
        let is_white = rng.gen_range(0.0..1.0) < p_white;
        let value = if is_fixed {rng.gen_range(1..10)} else {-1};
        vec.push(Cell::new(i, value, is_white, is_fixed));
    }
    vec
}

// Generates an empty board
pub fn empty_board() -> Vec<Cell> {
    random_board(0.0, 1.0)
}

// Recognize row/column straights structure
pub fn compute_rows_columns(cells: &VecOrVecModel<Cell>) -> Vec<Row> {
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
pub fn compute_possible_values(cell_index: usize, all_cells: &VecOrVecModel<Cell>, rows_columns: &Vec<Row>)
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
pub fn solve_backtrack(mut cells: Vec<Cell>) -> Str8tsSolution {
    // Works on a copy of the board, so recompute the rows/columns
    let rows_columns = compute_rows_columns(&VecOrVecModel::Vec(cells.clone()));

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
                let possible_values = compute_possible_values(i, &all_cells, &rows_columns);
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
pub fn generate_puzzle() -> Option<Vec<Cell>> {
    const P_WHITE: f64 = 0.6;
    let mut cells = random_board(0.0, P_WHITE);
    let mut rng = rand::thread_rng();
    let mut fixed_indices = vec![];

    let rows_columns = compute_rows_columns(&VecOrVecModel::Vec(cells.clone()));
    for i in 0..cells.len() {
        const P_FIXED: f64 = 0.0;
        if rng.gen_range(0.0..1.0) < P_FIXED {
            let all_cells = VecOrVecModel::Vec(cells.clone());
            let cell = &mut cells[i];
            let possible_values = compute_possible_values(i, &all_cells, &rows_columns);
            cell.value = *possible_values.choose(&mut rng).unwrap_or(&-1);
            if cell.value > 0 {
                fixed_indices.push(i);
                cell.is_fixed = true;
            }
        }
    }

    let mut solution = None;
    for i in 0..1000 {
        match solve_backtrack(cells.clone()) {
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
                        || compute_possible_values(cell_index, &all_cells, &rows_columns).is_empty()
                        || rng.gen_range(0.0..1.0) > P_FILL_BLACK) {
                    cell_index = rng.gen_range(0..cells.len());
                }
                if solution_cells[cell_index].value > 0 {
                    // Make an empty white cell fixed
                    cells[cell_index].value = solution_cells[cell_index].value;
                } else {
                    // Make an empty black cell fixed
                    cells[cell_index].value = *compute_possible_values(cell_index, &all_cells, &rows_columns).choose(&mut rng).unwrap();
                }
                cells[cell_index].is_fixed = true;
                fixed_indices.push(cell_index);
                println!("Generating puzzle: i = {}. Imposing restriction. cell {} = {}", i, cell_index, cells[cell_index].value);
            },
        }
    }
    if let Some(solution_cells) = solution {
        for i in 0..solution_cells.len() {
            let cell = &mut solution_cells[i].clone();
            if !cell.is_fixed {
                cell.value = -1;
            }
        }
        Some(solution_cells)       
    } else {
        None
    }
}