use super::*;
use crate::read::Limits;
use crate::test_support::stream_object;

#[test]
fn reads_cross_reference_streams_and_object_streams() {
    // Objects 1 and 2 live in object stream 3; the xref stream is object 4.
    let members =
        b"1 0 2 33 << /Type /Catalog /Pages 2 0 R >> << /Type /Pages /Kids [] /Count 0 >>";
    let mut pdf = b"%PDF-1.5\n".to_vec();
    let stream_at = pdf.len();
    pdf.extend_from_slice(b"3 0 obj\n");
    pdf.extend_from_slice(&stream_object("/Type /ObjStm /N 2 /First 9", members));
    pdf.extend_from_slice(b"\nendobj\n");
    let xref_at = pdf.len();
    let mut rows = Vec::new();
    for (kind, second, third) in
        [(0_u8, 0, 255_u8), (2, 3, 0), (2, 3, 1), (1, stream_at, 0), (1, xref_at, 0)]
    {
        rows.push(kind);
        rows.extend_from_slice(&u16::try_from(second).unwrap().to_be_bytes());
        rows.push(third);
    }
    pdf.extend_from_slice(b"4 0 obj\n");
    pdf.extend_from_slice(&stream_object("/Type /XRef /Size 5 /W [1 2 1] /Root 1 0 R", &rows));
    pdf.extend_from_slice(format!("\nendobj\nstartxref\n{xref_at}\n%%EOF").as_bytes());

    let limits = Limits::default();
    let budget = Budget::new(&limits, pdf.len()).unwrap();
    let xref = Xref::read(&pdf, &budget).unwrap();
    assert_eq!(xref.entries.get(&2), Some(&Entry::Compressed { stream: 3, index: 1 }));
    assert_eq!(xref.entries.get(&3), Some(&Entry::Offset { offset: stream_at, generation: 0 }));
    assert_eq!(xref.trailer.get(b"Root"), Some(&Object::Ref(1, 0)));

    let document = super::super::document::Document::open(&pdf, &budget).unwrap();
    let pages = document.get(2).unwrap();
    assert!(pages.as_dict().is_some_and(|pages| pages.has_name(b"Type", b"Pages")), "{pages:?}");
}
