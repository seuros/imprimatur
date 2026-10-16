use std::assert_matches;

use super::*;
use crate::Error;

fn comments(entries: &[((u32, u16), Comment)]) -> BTreeMap<(u32, u16), Comment> {
    entries.iter().cloned().collect()
}

#[test]
fn writes_authors_once_and_comments_in_row_major_order() {
    let note = |text: &str, author: &str| Comment::new(text).unwrap().author(author);
    let xml = comments_xml(&comments(&[
        ((1, 0), note("Second", "Instructions")),
        ((0, 2), note("Field: Name\nRequired: Yes", "Instructions")),
        ((0, 0), Comment::new(" padded ").unwrap()),
    ]));
    let expected = concat!(
        r#"<authors><author></author><author>Instructions</author></authors><commentList>"#,
        r#"<comment ref="A1" authorId="0"><text><t xml:space="preserve"> padded </t></text>"#,
        r#"</comment>"#,
        r#"<comment ref="C1" authorId="1"><text><t>Field: Name"#,
        "\nRequired: Yes</t></text></comment>",
        r#"<comment ref="A2" authorId="1"><text><t>Second</t></text></comment></commentList>"#,
    );
    assert!(xml.contains(expected), "{xml}");
}

#[test]
fn validates_text() {
    assert_matches!(Comment::new("x".repeat(32_768)), Err(Error::TextTooLong(32_768)));
    let comment = Comment::new("ok").unwrap().author("");
    assert_eq!(comment.author, None, "an empty author is no author");
}

#[test]
fn draws_one_note_shape_per_comment() {
    let hidden = Comment::new("a").unwrap();
    let vml = vml(
        &comments(&[((0, 0), hidden.clone()), ((4, 2), hidden.visible(true))]),
        &BTreeMap::new(),
        3,
    );
    assert!(!vml.starts_with("<?xml"), "legacy VML has no declaration");
    assert!(vml.contains(r#"<o:idmap v:ext="edit" data="3"/>"#), "{vml}");
    assert!(vml.contains(r##"<v:shape id="_x0000_s3073" type="#_x0000_t202""##), "{vml}");
    assert!(vml.contains(r#"<v:shape id="_x0000_s3074""#), "{vml}");
    // A1: from B1 + 15px, 128 by 74 pixels on the default 64 by 20 grid.
    assert!(vml.contains("<x:Anchor>1, 15, 0, 2, 3, 15, 3, 16</x:Anchor>"), "{vml}");
    assert!(vml.contains("margin-left:59.25pt;margin-top:1.5pt;width:96pt;height:55.5pt;"));
    assert!(vml.contains("z-index:1;visibility:hidden"), "{vml}");
    // C5: from D4 + 15px and 10px.
    assert!(vml.contains("<x:Anchor>3, 15, 3, 10, 5, 15, 7, 4</x:Anchor>"), "{vml}");
    assert!(vml.contains("z-index:2;visibility:visible"), "{vml}");
    assert!(vml.contains("<x:Row>4</x:Row><x:Column>2</x:Column><x:Visible/>"), "{vml}");
}

#[test]
fn anchors_follow_custom_column_widths() {
    // B is 20 characters (145px), C is hidden.
    let widths = BTreeMap::from([(1, 20.0), (2, 0.0)]);
    let drawn = |column| vml(&comments(&[((0, column), Comment::new("a").unwrap())]), &widths, 1);
    // 15 + 128 = 143 pixels fit in B's 145.
    let xml = drawn(0);
    assert!(xml.contains("<x:Anchor>1, 15, 0, 2, 1, 143, 3, 16</x:Anchor>"), "{xml}");
    // From C1, which is 0 wide: the box starts in D, after B's 145 pixels.
    let xml = drawn(1);
    assert!(xml.contains("<x:Anchor>3, 15, 0, 2, 5, 15, 3, 16</x:Anchor>"), "{xml}");
    assert!(xml.contains("margin-left:168pt;"), "(64 + 145 + 0 + 15) * 0.75: {xml}");
}

#[test]
fn shape_ids_take_whole_blocks() {
    assert_eq!(blocks_for(1), 1);
    assert_eq!(blocks_for(1_023), 1);
    assert_eq!(blocks_for(1_024), 2, "ids from 1025 to 2048 spill into the next block");
    let many: BTreeMap<(u32, u16), Comment> =
        (0..1_024).map(|row| ((row, 0), Comment::new("a").unwrap())).collect();
    assert!(vml(&many, &BTreeMap::new(), 2).contains(r#"data="2,3""#));
}
