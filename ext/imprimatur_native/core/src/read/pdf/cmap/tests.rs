use super::*;

const CMAP: &[u8] = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap
1 begincodespacerange <0000> <FFFF> endcodespacerange
2 beginbfchar <0003> <0020> <0024> <D83DDE00> endbfchar
2 beginbfrange <0010> <0012> <0041> <0020> <0021> [<0066006C> <00660069>] endbfrange
1 begincidrange <0100> <01FF> 5 endcidrange
endcmap CMapName currentdict /CMap defineresource pop end end";

#[test]
fn maps_codes_to_text_and_cids() {
    let cmap = CMap::parse(CMAP);
    let text = |code| {
        let mut out = String::new();
        cmap.push_text(code, &mut out).then_some(out)
    };
    assert_eq!(text(0x03).as_deref(), Some(" "));
    assert_eq!(text(0x24).as_deref(), Some("😀"), "a surrogate pair");
    assert_eq!(text(0x11).as_deref(), Some("B"), "a range counts up");
    assert_eq!(text(0x21).as_deref(), Some("fi"), "a range of listed texts");
    assert_eq!(text(0x99), None);
    assert_eq!(cmap.code_length(&[0x00, 0x03, 0x01]), 2);
    assert_eq!(cmap.cid(0x0102), Some(7));
    assert_eq!(cmap.cid(0x0002), None);
}
