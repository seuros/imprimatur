use super::*;

#[test]
fn names_columns() {
    const CASES: &[(u16, &str)] = &[
        (0, "A"),
        (25, "Z"),
        (26, "AA"),
        (51, "AZ"),
        (52, "BA"),
        (701, "ZZ"),
        (702, "AAA"),
        (16_383, "XFD"),
    ];
    for (column, name) in CASES {
        assert_eq!(column_name(*column), *name, "column {column}");
    }
}

#[test]
fn references_cells() {
    assert_eq!(cell_reference(0, 0), "A1");
    assert_eq!(cell_reference(1_048_575, 16_383), "XFD1048576");
}

#[test]
fn converts_values() {
    assert_eq!(Cell::from("x"), Cell::Text("x".into()));
    assert_eq!(Cell::from(3_u8), Cell::Number(3.0));
    assert_eq!(Cell::from(-7_i32), Cell::Number(-7.0));
    assert_eq!(Cell::from(true), Cell::Bool(true));
    assert_eq!(Cell::formula("=SUM(A1:A2)"), Cell::Formula("SUM(A1:A2)".into()));
    assert_eq!(Cell::formula("A1*2"), Cell::Formula("A1*2".into()));
}
