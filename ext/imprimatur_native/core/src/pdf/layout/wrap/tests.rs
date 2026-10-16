use super::*;

const PLAIN: Look = Look { face: Face::Regular, color: None, underline: false };
const BOLD: Look = Look { face: Face::Bold, color: None, underline: false };

fn text(lines: &[Line]) -> Vec<String> {
    lines
        .iter()
        .map(|line| line.fragments.iter().map(|f| String::from_utf8_lossy(&f.bytes)).collect())
        .collect()
}

fn run(text: &str, look: Look) -> (Vec<u8>, Look) {
    (text.as_bytes().to_vec(), look)
}

#[test]
fn breaks_at_spaces_and_drops_them() {
    // At 10pt "aaaa" is 22.24pt wide and a space 2.78pt.
    let lines = wrap(&[run("aaaa aaaa aaaa", PLAIN)], 10.0, 50.0);
    assert_eq!(text(&lines), ["aaaa aaaa", "aaaa"]);
    assert!(lines.iter().all(|line| line.width <= 50.0));
}

#[test]
fn keeps_runs_on_one_line_and_merges_fragments() {
    let lines = wrap(&[run("a ", PLAIN), run("b", BOLD), run("c d", BOLD)], 10.0, 500.0);
    assert_eq!(text(&lines), ["a bc d"]);
    assert_eq!(lines[0].fragments.len(), 2, "plain 'a ', bold 'bc d'");
}

#[test]
fn honours_newlines_and_tabs() {
    let lines = wrap(&[run("one\n\ttwo\n\nthree", PLAIN)], 10.0, 500.0);
    assert_eq!(text(&lines), ["one", "    two", "", "three"]);
}

#[test]
fn splits_words_wider_than_a_line() {
    let lines = wrap(&[run("aaaaaaaaaa", PLAIN)], 10.0, 12.0);
    assert_eq!(text(&lines), ["aa", "aa", "aa", "aa", "aa"]);
    let narrow = wrap(&[run("WW", PLAIN)], 10.0, 1.0);
    assert_eq!(text(&narrow), ["W", "W"], "at least one character per line");
}

#[test]
fn an_empty_paragraph_is_one_empty_line() {
    assert_eq!(text(&wrap(&[], 10.0, 100.0)), [""]);
    assert_eq!(text(&wrap(&[run("   ", PLAIN)], 10.0, 100.0)), [""]);
}
