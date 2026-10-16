use super::*;

/// A page of `(text, baseline, size, in the margin)` lines.
fn page(lines: &[(&str, f64, f64, bool)]) -> PageLines {
    let mut page = PageLines { height: 792.0, ..PageLines::default() };
    for &(text, y, size, margin) in lines {
        let span = span::append(&mut page.text, |page| page.push_str(text));
        let margin = if margin { margin_key(text) } else { 0 };
        page.lines.push(Line {
            span,
            y,
            left: 72.0,
            right: 300.0,
            size,
            bold: false,
            turns: 0,
            margin,
        });
    }
    page
}

#[test]
fn puts_right_to_left_lines_in_reading_order() {
    // Each line as a PDF draws it, left to right, then as it reads.
    const CASES: &[(&str, &[&str], &str)] = &[
        ("Arabic", &["2", "0", "2", "4", " ", "ب", "ر", "ع"], "عرب 2024"),
        ("Hebrew", &["2", "0", "2", "4", " ", "ם", "ו", "ל", "ש"], "שלום 2024"),
        ("Persian", &["م", "ا", "ل", "س"], "سلام"),
        ("Urdu", &["و", "د", "ر", "ا"], "اردو"),
        ("Yiddish", &["ש", "י", "ד", "י", "י"], "יידיש"),
        ("Syriac", &["ܐ", "ܡ", "ܠ", "ܫ"], "ܫܠܡܐ"),
        ("Dhivehi, vowel signs on their letters", &["ހި", "ވެ", "ދި"], "ދިވެހި"),
        ("N'Ko", &["ߏ", "ߞ", "ߒ"], "ߒߞߏ"),
        ("Adlam", &["𞤥", "𞤢", "𞤤", "𞤣", "𞤀"], "𞤀𞤣𞤤𞤢𞤥"),
        (
            "Hebrew around Latin and digits",
            &["I", "S", "O", " ", "9", "0", "0", "1", " ", "ר", "ו", "ש", "י", "א"],
            "אישור ISO 9001",
        ),
    ];
    for (script, drawn, reading) in CASES {
        let mut pieces = drawn.to_vec();
        assert!(right_to_left(&pieces), "{script}");
        reorder(&mut pieces);
        assert_eq!(pieces.concat(), *reading, "{script}");
    }
    let latin = ["H", "i", " ", "ب"];
    assert!(!right_to_left(&latin));
}

#[test]
fn knows_the_right_to_left_scripts_and_their_marks() {
    const MARKS: &[(char, bool)] = &[
        ('\u{05B4}', true),
        ('\u{064E}', true),
        ('\u{0730}', true),
        ('\u{07A8}', true),
        ('\u{07EB}', true),
        ('\u{0817}', true),
        ('\u{0859}', true),
        ('\u{10D24}', true),
        ('\u{1E944}', true),
        ('\u{05E9}', false),
        ('a', false),
    ];
    const DIRECTIONS: &[(char, Option<bool>)] = &[
        ('\u{05D0}', Some(true)),
        ('\u{0710}', Some(true)),
        ('\u{0780}', Some(true)),
        ('\u{07CA}', Some(true)),
        ('\u{0800}', Some(true)),
        ('\u{0840}', Some(true)),
        ('\u{10D00}', Some(true)),
        ('\u{1E900}', Some(true)),
        ('\u{200F}', Some(true)),
        ('\u{200E}', Some(false)),
        ('a', Some(false)),
        ('7', Some(false)),
        ('\u{FEFF}', None),
        (' ', None),
    ];
    for &(ch, expected) in DIRECTIONS {
        assert_eq!(direction(ch), expected, "U+{:04X}", u32::from(ch));
    }
    for &(ch, expected) in MARKS {
        assert_eq!(is_mark(ch), expected, "U+{:04X}", u32::from(ch));
    }
}

#[test]
fn joins_lines_into_paragraphs_and_headings() {
    let page = page(&[
        ("Annual review", 700.0, 20.0, false),
        ("The certifi-", 660.0, 10.0, false),
        ("cation body is accredited.", 648.0, 10.0, false),
        ("E-", 636.0, 10.0, false),
        ("Mail us.", 624.0, 10.0, false),
        ("# not a heading", 580.0, 10.0, false),
    ]);
    let pages = pages(&[page]);
    let bounds: Vec<Option<[f64; 4]>> = pages[0]
        .blocks
        .iter()
        .map(|placed| placed.bounds.map(|b| [b.left, b.top, b.right, b.bottom]))
        .collect();
    let expected =
        [[72.0, 76.0, 300.0, 97.0], [72.0, 124.0, 300.0, 170.5], [72.0, 204.0, 300.0, 214.5]];
    assert_eq!(bounds, expected.map(Some), "from the first line's ascent to the last's descent");
    let text = crate::read::content::Content::Pdf(pages).to_markdown();
    assert_eq!(
        text,
        "# Annual review\n\nThe certification body is accredited.\nE-\nMail us.\n\n\
         \\# not a heading\n"
    );

    let mut turned = self::page(&[("Sideways", 300.0, 10.0, false)]);
    turned.lines[0].turns = 1;
    assert_eq!(super::pages(&[turned])[0].blocks[0].bounds, None);
}

#[test]
fn removes_running_lines_from_page_margins() {
    let pages: Vec<PageLines> = (1..=4)
        .map(|number| {
            page(&[
                (&format!("Body {number}"), 700.0, 10.0, false),
                (&format!("Page {number}"), 20.0, 8.0, true),
            ])
        })
        .collect();
    let text = crate::read::content::Content::Pdf(super::pages(&pages)).to_markdown();
    assert!(!text.contains("Page"), "{text}");
    assert!(text.contains("Body 4"), "{text}");
}
