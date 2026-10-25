//! Physical sizing shared by pictures and barcode symbols.

/// The largest side in points allowed by PDF.
const MAX_POINTS: f64 = 14_400.0;

pub(super) fn points_in_range(points: f64) -> Option<f64> {
    (1.0..=MAX_POINTS).contains(&points).then_some(points)
}

/// Fits into the available room without enlarging or changing proportions.
pub(super) fn fit((width, height): (f64, f64), room: (f64, f64)) -> (f64, f64) {
    let scale = (room.0 / width).min(room.1 / height).min(1.0);
    (width * scale, height * scale)
}
