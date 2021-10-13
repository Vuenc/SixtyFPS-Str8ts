/* LICENSE BEGIN
    This file is part of SixtyFPS-Str8ts, a demo implementing the
    Str8ts puzzle in the SixtyFPS framework. Based on the SixtyFPS
    Slide Puzzle demo.
    Copyright (c) 2021 Vincent Bürgin <v.buergin@gmx.de>

    SPDX-License-Identifier: GPL-3.0-only
LICENSE END */

use std::rc::Rc;
use sixtyfps::Model;

// Stores either a Vec<T> (for working outside of UI) or a
// sixtyfps VecModel<T> (for working with the UI), provides interface
// to get/set functionality. Used here for T = Cell to implement
// methods that can handle both formats.
#[derive(Clone)]
pub enum VecOrVecModel<T> where T: Clone {
    Vec(Vec<T>),
    VecModel(Rc<sixtyfps::VecModel<T>>)
}

// Implement get/set for VecOrVecModel
impl<T: 'static> VecOrVecModel<T> where T: Clone {
    pub fn get(&self, index: usize) -> T {
        match self {
            Self::Vec(vec) => {
                vec[index].clone()
            },
            Self::VecModel(vec_model) => {
                vec_model.row_data(index).clone()
            }
        }
    }

    pub fn set(&mut self, index: usize, value: T) {
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
