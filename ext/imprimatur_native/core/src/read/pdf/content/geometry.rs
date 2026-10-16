//! Where things land on a page: transforms, and the frame a page is shown
//! in once its rotation is applied.

use super::super::document::Page;
use crate::read::content::Bounds;

/// A 2D affine transform `[a b c d e f]`, as PDF writes them.
pub type Matrix = [f64; 6];
pub const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

pub fn multiply(m: &Matrix, n: &Matrix) -> Matrix {
    [
        m[1].mul_add(n[2], m[0] * n[0]),
        m[1].mul_add(n[3], m[0] * n[1]),
        m[3].mul_add(n[2], m[2] * n[0]),
        m[3].mul_add(n[3], m[2] * n[1]),
        m[5].mul_add(n[2], m[4] * n[0]) + n[4],
        m[5].mul_add(n[3], m[4] * n[1]) + n[5],
    ]
}

/// How a page is shown: its rotation into the displayed frame, its size
/// there, and its media box.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub rotation: Matrix,
    pub size: (f64, f64),
    pub bounds: [f64; 4],
}

impl Frame {
    pub fn of(page: &Page) -> Self {
        let [x0, y0, x1, y1] = page.media_box;
        let (left, bottom, right, top) = (x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1));
        // Displayed frame: rotated clockwise by /Rotate, y upward from the
        // bottom-left corner.
        let rotation = match page.rotate.rem_euclid(360) {
            90 => [0.0, -1.0, 1.0, 0.0, -bottom, right],
            180 => [-1.0, 0.0, 0.0, -1.0, right, top],
            270 => [0.0, 1.0, -1.0, 0.0, top, -left],
            _ => [1.0, 0.0, 0.0, 1.0, -left, -bottom],
        };
        let (width, height) = (right - left, top - bottom);
        let size =
            if page.rotate.rem_euclid(180) == 90 { (height, width) } else { (width, height) };
        Self { rotation, size, bounds: [left, bottom, right, top] }
    }

    /// Where the unit square, drawn with `ctm`, lands: in points from the
    /// top-left corner of the page as shown.
    pub fn place(&self, ctm: &Matrix) -> Bounds {
        let shown = multiply(ctm, &self.rotation);
        let corners = [(0.0_f64, 0.0_f64), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].map(|(x, y)| {
            (
                x.mul_add(shown[0], y.mul_add(shown[2], shown[4])),
                x.mul_add(shown[1], y.mul_add(shown[3], shown[5])),
            )
        });
        let (mut left, mut right, mut low, mut high) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for (x, y) in corners {
            (left, right, low, high) = (left.min(x), right.max(x), low.min(y), high.max(y));
        }
        let height = self.size.1;
        Bounds { left, top: height - high, right, bottom: height - low }
    }
}

/// How many quarter turns, counterclockwise, a baseline along `(dx, dy)`
/// makes from left-to-right.
pub fn quarter_turns(dx: f64, dy: f64) -> u8 {
    let angle = dy.atan2(dx).to_degrees().rem_euclid(360.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "0 to 4 after rounding"
    )]
    let turns = ((angle / 90.0).round() as u8) % 4;
    turns
}
