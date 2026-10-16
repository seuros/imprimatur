use super::*;

#[test]
fn keeps_a_heading_with_what_follows() {
    let mut layout = Layout::new(PageSize::A4);
    // Fill the page to just above the bottom margin.
    let near_bottom = MARGIN + 30.0;
    for _ in 0..100 {
        if layout.y <= near_bottom {
            break;
        }
        layout.paragraph(&Paragraph::new().text("filler")).unwrap();
    }
    assert!(layout.y <= near_bottom && layout.pages.len() == 1, "filled the first page");
    layout.heading(2, "Section").unwrap();
    let pages = layout.finish(false);
    assert_eq!(pages.len(), 2);
    assert!(String::from_utf8_lossy(&pages[1]).contains("(Section) Tj"));
}

#[test]
fn numbers_are_short() {
    for (value, text) in [
        (1.0, "1"),
        (0.5, "0.5"),
        (12.345, "12.35"),
        (-0.001, "0"),
        (595.276, "595.28"),
        (-2.5, "-2.5"),
        (0.3, "0.3"),
        (10.0, "10"),
    ] {
        assert_eq!(Num(value).to_string(), text, "{value}");
    }
}
