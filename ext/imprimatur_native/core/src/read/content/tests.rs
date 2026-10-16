use super::*;

#[test]
fn grids_keep_values_and_text_apart() {
    let mut grid = Grid::default();
    grid.buffer().push_str("2023-03-15");
    let date = DateTime::date(2023, 3, 15).unwrap();
    grid.end_cell(1, 0, Value::Date(date));
    grid.push(0, 1, "Closed");
    grid.push(0, 0, "Day");
    grid.buffer().push_str("   ");
    grid.end_cell(1, 1, Value::Text);
    let grid = grid.finish();
    let cells: Vec<(u32, u32, &str)> =
        grid.cells().iter().map(|cell| (cell.row, cell.column, grid.text(cell))).collect();
    assert_eq!(
        cells,
        [(0, 0, "Day"), (0, 1, "Closed"), (1, 0, "2023-03-15")],
        "a blank cell is dropped"
    );
    assert_eq!(grid.cells()[2].value, Value::Date(date));
    assert_eq!(grid.rows().map(<[Cell]>::len).collect::<Vec<_>>(), [2, 1]);
    assert_eq!(Block::Table(grid).plain_text(), "Day\nClosed\n2023-03-15");
}
