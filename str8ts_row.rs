use crate::vec_or_vec_model::VecOrVecModel;
use crate::sixtyfps_generated_MainWindow::Cell;

// Represents a row/column and its straights
pub struct Row {
    row_cells: Vec<usize>, // indices of cells in row/column
    straights: Vec<Vec<usize>>, // straights, stored as index vectors
}

impl Row {
    // Create new row, recognize it straights
    pub fn new(row_cells: Vec<usize>, all_cells: &VecOrVecModel<Cell>) -> Row {
        let straights = row_cells.iter()
            .map(|&i| (i, all_cells.get(i).is_white)).collect::<Vec<(usize, bool)>>()
            .split(|(_, is_white)| !is_white)
            .filter(|&slice| slice.len() > 0)
            .map(|slice| slice.iter().map(|(i, _)| *i).collect()).collect();
        Row { row_cells, straights }
    }

    // Validate a row: find duplicate values and invalid straights
    pub fn validate(&self, all_cells: &VecOrVecModel<Cell>) -> Option<(Vec<Vec<usize>>, Vec<Vec<usize>>)> {
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
    pub fn missing_values_cells(&self, candidate_values: Option<&[i32]>, all_cells: &VecOrVecModel<Cell>)
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
    pub fn possible_straight_values_cells(&self, cell_index: usize, candidate_values: &[i32], 
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