//! Graphics and text state for a content stream.

use std::rc::Rc;

use super::Font;
use super::geometry::{IDENTITY, Matrix, multiply};

#[derive(Clone, Copy)]
pub(super) struct Cursor {
    pub matrix: Matrix,
    line: Matrix,
}

impl Cursor {
    pub const START: Self = Self { matrix: IDENTITY, line: IDENTITY };

    pub const fn at(matrix: Matrix) -> Self {
        Self { matrix, line: matrix }
    }

    pub fn next_line(&mut self, x: f64, y: f64) {
        self.line = multiply(&[1.0, 0.0, 0.0, 1.0, x, y], &self.line);
        self.matrix = self.line;
    }
}

#[derive(Clone)]
pub(super) struct State {
    pub ctm: Matrix,
    pub font: Option<Rc<Font>>,
    pub size: f64,
    pub char_spacing: f64,
    pub word_spacing: f64,
    pub scale: f64,
    pub leading: f64,
    pub rise: f64,
}

impl State {
    pub const START: Self = Self {
        ctm: IDENTITY,
        font: None,
        size: 0.0,
        char_spacing: 0.0,
        word_spacing: 0.0,
        scale: 1.0,
        leading: 0.0,
        rise: 0.0,
    };
}
