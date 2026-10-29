use std::collections::HashMap;

use super::{Line, PageLines};
use crate::read::content::{Block, Bounds, Page, Placed, Text};

/// The document's pages, from their lines: blocks of the lines that are
/// not running headers or footers, and, with `drop_repeated_anywhere`, not
/// text repeated at the same spot on most pages anywhere on the page.
pub fn pages(
    pages: &[PageLines],
    labels: Option<&[Option<String>]>,
    drop_repeated_anywhere: bool,
) -> Vec<Page> {
    let boilerplate = repeated_keys(pages, |line| line.margin);
    let margin_kept =
        |line: &&Line| line.margin == 0 || boilerplate.binary_search(&line.margin).is_err();
    let repeated =
        if drop_repeated_anywhere { repeated_keys(pages, |line| line.repeat) } else { Vec::new() };
    let kept = |line: &&Line| {
        margin_kept(line) && (repeated.is_empty() || repeated.binary_search(&line.repeat).is_err())
    };
    let body = body_size(pages, &margin_kept);
    let levels = heading_sizes(pages, &margin_kept, body);
    let mut out = Vec::with_capacity(pages.len());
    let mut paragraph: Vec<&Line> = Vec::new();
    // Each page's blocks, collected here, then moved out at their size.
    let mut blocks = Vec::new();
    for (index, (number, page)) in (1..).zip(pages).enumerate() {
        paragraph.clear();
        for line in page.lines.iter().filter(kept) {
            if let Some(last) = paragraph.last()
                && !continues(last, line)
            {
                blocks.push(block(page, &paragraph, body, &levels));
                paragraph.clear();
            }
            paragraph.push(line);
        }
        if !paragraph.is_empty() {
            blocks.push(block(page, &paragraph, body, &levels));
        }
        #[expect(
            clippy::drain_collect,
            clippy::iter_with_drain,
            reason = "`blocks` keeps its buffer for the next page"
        )]
        let blocks = blocks.drain(..).collect();
        let label = labels
            .map_or_else(|| printed_label(page), |labels| labels.get(index).cloned().flatten());
        out.push(Page { number, blocks, images: page.images, label });
    }
    out
}

/// The page's own printed label, from a single header or footer line that
/// looks like one: a bare page number or roman numeral ("`-i-`", "`7`"), or
/// the same after a short label word ("`Page 70`" -> "`70`"). Unlike the
/// boilerplate this document's other margin lines are checked against, a
/// running "`Page 70`", "`Page 71`", ... header is not noise here: each
/// instance *is* that page's label, not a repeated line to drop.
fn printed_label(page: &PageLines) -> Option<String> {
    let mut candidates =
        page.lines.iter().filter(|line: &&Line| line.margin != 0).filter_map(|line| {
            let text = page.text(line).trim();
            page_number_label(text)
        });
    let label = candidates.next()?;
    candidates.next().is_none().then_some(label)
}

/// `text` as a page label, if it looks like one, stripped of a leading
/// label word such as "Page" when there is one.
fn page_number_label(text: &str) -> Option<String> {
    if looks_like_page_number(text) {
        return Some(text.to_owned());
    }
    let word_end = text.find(|ch: char| !ch.is_ascii_alphabetic())?;
    if word_end == 0 {
        return None;
    }
    let rest = text[word_end..].trim_start_matches(|ch: char| !ch.is_alphanumeric());
    looks_like_page_number(rest).then(|| rest.to_owned())
}

fn looks_like_page_number(text: &str) -> bool {
    let core = text.trim_matches(|ch: char| !ch.is_alphanumeric());
    !core.is_empty()
        && core.len() <= 8
        && (core.chars().all(|ch| ch.is_ascii_digit())
            || core.chars().all(|ch| {
                matches!(ch.to_ascii_lowercase(), 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm')
            }))
}

/// Whether `next` carries on the paragraph `last` is in: same direction,
/// similar size, and no more than a line and a half below it.
fn continues(last: &Line, next: &Line) -> bool {
    let size = last.size.max(next.size).max(1.0);
    last.turns == next.turns
        && (last.size - next.size).abs() <= 0.15 * size
        && last.y - next.y > 0.0
        && last.y - next.y <= 1.55 * size
        && last.bold == next.bold
}

/// A paragraph of lines as a block: a heading when its few short lines
/// are set in a heading size, otherwise a paragraph with its lines on lines
/// of their own, words hyphenated across them joined again.
fn block(page: &PageLines, lines: &[&Line], body: f64, levels: &[f64]) -> Placed {
    let first = lines[0];
    let length: usize = lines.iter().map(|line| line.span.len()).sum();
    let short = lines.len() <= 3 && length <= 200;
    let text = paragraph_text(page, lines);
    let level =
        if short { levels.iter().position(|&size| first.size >= size - 0.25) } else { None };
    let sized = level
        .filter(|_| first.size >= body * 1.15)
        .map(|level| u8::try_from(level.min(2) + 1).unwrap_or(3));
    let numbered = (short && first.bold && first.size >= body - 0.25)
        .then(|| numbered_heading_level(&text))
        .flatten();
    let block = match sized.or(numbered) {
        Some(level) => Block::Heading { level, text: Text::plain(text) },
        None => Block::Paragraph { text: Text::plain(text) },
    };
    Placed { block, bounds: bounds(page, lines) }
}

fn paragraph_text(page: &PageLines, lines: &[&Line]) -> String {
    let length: usize = lines.iter().map(|line| line.span.len()).sum();
    let mut text = String::with_capacity(length + lines.len());
    for (index, line) in lines.iter().enumerate() {
        let line = page.text(line);
        if index > 0 {
            let joined = text.ends_with('-') && {
                let mut before = text.chars().rev().skip(1);
                before.next().is_some_and(char::is_lowercase)
                    && line.chars().next().is_some_and(char::is_lowercase)
            };
            if joined {
                text.pop();
            } else {
                text.push('\n');
            }
        }
        text.push_str(line);
    }
    text
}

fn numbered_heading_level(text: &str) -> Option<u8> {
    let (depth, rest) = numbered_prefix(text)?;
    if depth < 2 || !(rest.is_empty() || rest.starts_with(' ')) {
        return None;
    }
    let title = rest.trim_start();
    if title.contains("....") || title.starts_with(|ch: char| !ch.is_alphanumeric()) {
        return None;
    }
    Some(u8::try_from(depth).unwrap_or(6).min(6))
}

fn numbered_prefix(text: &str) -> Option<(usize, &str)> {
    let bytes = text.as_bytes();
    let mut groups = 0_usize;
    let mut pos = 0_usize;
    loop {
        let start = pos;
        while bytes.get(pos).is_some_and(u8::is_ascii_digit) {
            pos += 1;
        }
        if pos == start {
            break;
        }
        groups += 1;
        if groups >= 4
            || bytes.get(pos) != Some(&b'.')
            || !bytes.get(pos + 1).is_some_and(u8::is_ascii_digit)
        {
            break;
        }
        pos += 1;
    }
    (groups > 0).then(|| (groups, &text[pos..]))
}

/// Where upright lines lie on the page, from its top-left corner: from the
/// top of the first line's capitals to below the last line's descenders.
fn bounds(page: &PageLines, lines: &[&Line]) -> Option<Bounds> {
    if lines.iter().any(|line| line.turns != 0) {
        return None;
    }
    let (first, last) = (lines.first()?, lines.last()?);
    let left = lines.iter().map(|line| line.left).fold(f64::MAX, f64::min);
    let right = lines.iter().map(|line| line.right).fold(f64::MIN, f64::max);
    let top = page.height - first.size.mul_add(0.8, first.y);
    let bottom = page.height - last.size.mul_add(-0.25, last.y);
    Some(Bounds { left, top, right, bottom })
}

/// The keys `key` returns for lines that repeat on enough pages to be
/// boilerplate rather than coincidence, sorted.
fn repeated_keys(pages: &[PageLines], key: impl Fn(&Line) -> u64) -> Vec<u64> {
    if pages.len() < 3 {
        return Vec::new();
    }
    let mut counts: HashMap<u64, usize> = HashMap::new();
    let mut seen: Vec<u64> = Vec::new();
    for page in pages {
        seen.clear();
        seen.extend(page.lines.iter().map(&key).filter(|&key| key != 0));
        seen.sort_unstable();
        seen.dedup();
        for &key in &seen {
            *counts.entry(key).or_default() += 1;
        }
    }
    let threshold = (pages.len() * 2 / 5).max(3);
    let mut repeated: Vec<u64> =
        counts.into_iter().filter(|&(_, count)| count >= threshold).map(|(key, _)| key).collect();
    repeated.sort_unstable();
    repeated
}

/// Every page's lines that `kept` keeps.
fn kept_lines<'p>(
    pages: &'p [PageLines],
    kept: &'p impl Fn(&&Line) -> bool,
) -> impl Iterator<Item = &'p Line> {
    pages.iter().flat_map(|page| &page.lines).filter(kept)
}

/// The most common font size, by characters.
fn body_size(pages: &[PageLines], kept: &impl Fn(&&Line) -> bool) -> f64 {
    let mut counts: HashMap<i64, usize> = HashMap::new();
    for line in kept_lines(pages, kept) {
        #[expect(clippy::cast_possible_truncation, reason = "font sizes are small")]
        let key = (line.size * 2.0).round() as i64;
        *counts.entry(key).or_default() += line.span.len();
    }
    #[expect(clippy::cast_precision_loss, reason = "a small integer")]
    counts
        .into_iter()
        .max_by_key(|&(size, count)| (count, -size))
        .map_or(0.0, |(size, _)| size as f64 / 2.0)
}

/// The sizes of headings, largest first: distinct sizes clearly above the
/// body's, three at most.
fn heading_sizes(pages: &[PageLines], kept: &impl Fn(&&Line) -> bool, body: f64) -> Vec<f64> {
    let mut sizes: Vec<f64> = kept_lines(pages, kept)
        .filter(|line| line.size >= body * 1.15 && line.span.len() <= 200)
        .map(|line| (line.size * 2.0).round() / 2.0)
        .collect();
    sizes.sort_by(|a, b| b.total_cmp(a));
    sizes.dedup();
    sizes.truncate(3);
    sizes
}
