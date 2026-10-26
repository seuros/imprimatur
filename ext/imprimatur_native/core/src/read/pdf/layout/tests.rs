use std::assert_matches;

use super::*;

/// A page of `(text, baseline, size, in the margin)` lines.
fn page(lines: &[(&str, f64, f64, bool)]) -> PageLines {
    bold_page(
        &lines
            .iter()
            .map(|&(text, y, size, margin)| (text, y, size, margin, false))
            .collect::<Vec<_>>(),
    )
}

/// A page of `(text, baseline, size, in the margin, bold)` lines.
fn bold_page(lines: &[(&str, f64, f64, bool, bool)]) -> PageLines {
    let mut page = PageLines { height: 792.0, ..PageLines::default() };
    for &(text, y, size, margin, bold) in lines {
        let span = span::append(&mut page.text, |page| page.push_str(text));
        let margin = if margin { margin_key(text) } else { 0 };
        page.lines.push(Line { span, y, left: 72.0, right: 300.0, size, bold, turns: 0, margin });
    }
    page
}

/// A page drawn left to right, glyph by glyph, from `(text, baseline)`
/// lines: a space is a gap, not a glyph.
fn drawn(lines: &[(&str, f64)]) -> PageText {
    let mut page = PageText { size: (612.0, 792.0), ..PageText::default() };
    for &(text, y) in lines {
        let mut x = 72.0;
        for ch in text.chars() {
            if ch != ' ' {
                let start = u32::try_from(page.text.len()).unwrap_or(u32::MAX);
                page.text.push(ch);
                let end = u32::try_from(page.text.len()).unwrap_or(u32::MAX);
                let text = start..end;
                page.glyphs.push(Glyph {
                    x,
                    y,
                    width: 5.0,
                    size: 10.0,
                    turns: 0,
                    bold: false,
                    text,
                });
            }
            x += 6.0;
        }
    }
    page
}

/// `(text, baseline)` lines, drawn left to right.
type Drawn = &'static [(&'static str, f64)];

fn glyphs(words: &[(&str, f64, f64, f64)]) -> PageText {
    let mut page = PageText { size: (612.0, 792.0), ..PageText::default() };
    for &(text, mut x, y, size) in words {
        let width = size * 0.6;
        for ch in text.chars() {
            let start = u32::try_from(page.text.len()).unwrap_or(u32::MAX);
            page.text.push(ch);
            let end = u32::try_from(page.text.len()).unwrap_or(u32::MAX);
            page.glyphs.push(Glyph { x, y, width, size, turns: 0, bold: false, text: start..end });
            x += width;
        }
    }
    page
}

#[test]
fn ordinal_superscript_stays_attached() {
    let page = lines(&glyphs(&[
        ("8", 72.0, 700.0, 10.0),
        ("th", 78.0, 704.6, 6.5),
        ("DWORD", 90.0, 700.0, 10.0),
    ]));
    let texts: Vec<&str> = page.lines.iter().map(|line| page.text(line)).collect();
    assert_eq!(texts, ["8th DWORD"]);
    assert_eq!(page.lines[0].size, 10.0);
    assert_eq!(page.lines[0].y, 700.0);
}

#[test]
fn caption_below_heading_is_not_merged_as_superscript() {
    let page = lines(&glyphs(&[
        ("TITLE", 72.0, 700.0, 20.0),
        ("Ed.", 72.0, 665.0, 7.0),
        ("Notes", 72.0, 660.0, 10.0),
    ]));
    let texts: Vec<&str> = page.lines.iter().map(|line| page.text(line)).collect();
    assert_eq!(texts, ["TITLE", "Ed.", "Notes"]);
}

#[test]
fn merged_line_size_is_the_base_runs_not_blended() {
    let page = lines(&glyphs(&[
        ("1", 72.0, 700.0, 24.0),
        ("st", 86.4, 712.0, 15.6),
        ("DWORD", 108.0, 700.0, 24.0),
    ]));
    let texts: Vec<&str> = page.lines.iter().map(|line| page.text(line)).collect();
    assert_eq!(texts, ["1st DWORD"]);
    assert_eq!(page.lines[0].size, 24.0);
}

#[test]
fn reads_lines_in_their_page_direction() {
    // (page, as drawn, as it reads)
    const CASES: &[(&str, Drawn, &[&str])] = &[
        (
            "Arabic: its row of figures reads from the right",
            &[("ةكرش ةداهش", 700.0), ("100 200 300", 680.0), ("Page 2", 30.0)],
            &["شهادة شركة", "300 200 100", "Page 2"],
        ),
        (
            "English: the same row as drawn",
            &[("Certificate", 700.0), ("100 200 300", 680.0)],
            &["Certificate", "100 200 300"],
        ),
    ];
    for (case, drawn_lines, reading) in CASES {
        let page = lines(&drawn(drawn_lines));
        let texts: Vec<&str> = page.lines.iter().map(|line| page.text(line)).collect();
        assert_eq!(texts, *reading, "{case}");
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
    let pages = pages(&[page], None);
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
    assert_eq!(super::pages(&[turned], None)[0].blocks[0].bounds, None);
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
    let out = super::pages(&pages, None);
    let text = crate::read::content::Content::Pdf(out.clone()).to_markdown();
    assert!(!text.contains("Page"), "{text}");
    assert!(text.contains("Body 4"), "{text}");
    assert!(out.iter().all(|page| page.label.is_none()), "{out:?}");
}

#[test]
fn falls_back_to_a_lone_footer_that_looks_like_a_page_number() {
    let pages: Vec<PageLines> = [("-i-", 1), ("-ii-", 2), ("7", 3)]
        .into_iter()
        .map(|(footer, number)| {
            page(&[(&format!("Body {number}"), 700.0, 10.0, false), (footer, 20.0, 8.0, true)])
        })
        .collect();
    let labels: Vec<Option<String>> =
        super::pages(&pages, None).into_iter().map(|page| page.label).collect();
    assert_eq!(labels, [Some("-i-".to_owned()), Some("-ii-".to_owned()), Some("7".to_owned())]);
}

#[test]
fn skips_a_footer_with_more_than_one_candidate() {
    let page =
        page(&[("Body", 700.0, 10.0, false), ("-i-", 20.0, 8.0, true), ("3", 18.0, 8.0, true)]);
    assert_eq!(super::pages(&[page, self::page(&[]), self::page(&[])], None)[0].label, None);
}

#[test]
fn promotes_a_bold_numbered_heading_at_body_size() {
    let page = bold_page(&[
        (
            "Body text at the common size used throughout this short document.",
            700.0,
            10.0,
            false,
            false,
        ),
        ("6.2.1 SFDP Header: 1st DWORD", 600.0, 10.0, false, true),
    ]);
    let pages = super::pages(&[page], None);
    assert_matches!(
        &pages[0].blocks[1].block,
        Block::Heading { level: 3, text } if text.text == "6.2.1 SFDP Header: 1st DWORD"
    );
}

#[test]
fn does_not_promote_a_toc_entrys_dot_leader() {
    let dots = ".".repeat(120);
    let entry = format!("6.2.1 SFDP Header: 1st DWORD {dots} 13");
    let page = bold_page(&[
        (
            "Body text at the common size used throughout this short document.",
            700.0,
            10.0,
            false,
            false,
        ),
        (&entry, 600.0, 10.0, false, true),
    ]);
    let pages = super::pages(&[page], None);
    assert_matches!(&pages[0].blocks[1].block, Block::Paragraph { .. });
}

#[test]
fn a_large_numbered_heading_is_not_double_counted() {
    let page = bold_page(&[
        (
            "Body text at the common size used throughout this short document.",
            700.0,
            10.0,
            false,
            false,
        ),
        ("6.2 SFDP Header", 600.0, 14.0, false, true),
    ]);
    let pages = super::pages(&[page], None);
    assert_matches!(
        &pages[0].blocks[1].block,
        Block::Heading { level: 1, text } if text.text == "6.2 SFDP Header"
    );
}
