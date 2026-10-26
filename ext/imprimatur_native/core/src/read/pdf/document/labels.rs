use std::collections::HashSet;

use super::{Dict, Document, Object, Result};

struct LabelRange {
    start: u32,
    style: Option<u8>,
    prefix: Option<String>,
    first: i64,
}

const ROMAN: &[(i64, &str)] = &[
    (1000, "M"),
    (900, "CM"),
    (500, "D"),
    (400, "CD"),
    (100, "C"),
    (90, "XC"),
    (50, "L"),
    (40, "XL"),
    (10, "X"),
    (9, "IX"),
    (5, "V"),
    (4, "IV"),
    (1, "I"),
];

impl Document<'_> {
    pub fn page_labels(&self, count: usize) -> Result<Option<Vec<Option<String>>>> {
        let Some(ranges) = self.label_ranges()? else { return Ok(None) };
        Ok(Some(
            (0..count)
                .map(|index| label_for(&ranges, u32::try_from(index).unwrap_or(u32::MAX)))
                .collect(),
        ))
    }

    fn label_ranges(&self) -> Result<Option<Vec<LabelRange>>> {
        let Some(root) = self.trailer().get(b"Root") else { return Ok(None) };
        let root = self.resolve(root)?;
        let Some(catalog) = root.as_dict() else { return Ok(None) };
        let Some(tree) = catalog.get(b"PageLabels") else { return Ok(None) };
        let mut ranges = Vec::new();
        self.walk_labels(tree, 0, &mut HashSet::new(), &mut ranges)?;
        ranges.sort_by_key(|range| range.start);
        ranges.dedup_by_key(|range| range.start);
        Ok(Some(ranges))
    }

    fn walk_labels(
        &self,
        node: &Object,
        depth: usize,
        visited: &mut HashSet<u32>,
        out: &mut Vec<LabelRange>,
    ) -> Result<()> {
        self.budget.check()?;
        self.budget.nest(depth)?;
        if let Some((number, _)) = node.as_reference()
            && !visited.insert(number)
        {
            return Ok(());
        }
        let node = self.resolve(node)?;
        let Some(dict) = node.as_dict() else { return Ok(()) };
        if let Some(Object::Array(nums)) = self.entry(dict, b"Nums")?.as_deref() {
            let mut nums = nums.iter();
            while let (Some(start), Some(value)) = (nums.next(), nums.next()) {
                let Some(start) =
                    self.resolve(start)?.as_integer().and_then(|start| u32::try_from(start).ok())
                else {
                    continue;
                };
                let value = self.resolve(value)?;
                if let Some(label) = value.as_dict() {
                    out.push(self.label_range(start, label)?);
                }
            }
        }
        if let Some(Object::Array(kids)) = self.entry(dict, b"Kids")?.as_deref() {
            for kid in kids {
                self.walk_labels(kid, depth + 1, visited, out)?;
            }
        }
        Ok(())
    }

    fn label_range(&self, start: u32, dict: &Dict) -> Result<LabelRange> {
        let style = self
            .entry(dict, b"S")?
            .as_deref()
            .and_then(Object::as_name)
            .and_then(|name| name.first().copied());
        let prefix = self
            .entry(dict, b"P")?
            .as_deref()
            .and_then(Object::as_bytes)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned());
        let first = self.entry(dict, b"St")?.as_deref().and_then(Object::as_integer).unwrap_or(1);
        Ok(LabelRange { start, style, prefix, first })
    }
}

fn label_for(ranges: &[LabelRange], index: u32) -> Option<String> {
    let range = ranges.iter().rev().find(|range| range.start <= index)?;
    let offset = i64::from(index - range.start);
    let number = range.first.checked_add(offset)?;
    let mut label = range.prefix.clone().unwrap_or_default();
    if let Some(style) = range.style {
        label.push_str(&numeral(style, number));
    }
    Some(label)
}

fn numeral(style: u8, number: i64) -> String {
    match style {
        b'R' => roman(number, true),
        b'r' => roman(number, false),
        b'A' => alpha(number, true),
        b'a' => alpha(number, false),
        _ => number.to_string(),
    }
}

fn roman(number: i64, upper: bool) -> String {
    if number < 1 {
        return number.to_string();
    }
    let mut remaining = number;
    let mut out = String::new();
    for &(value, symbol) in ROMAN {
        while remaining >= value {
            out.push_str(symbol);
            remaining -= value;
        }
    }
    if upper { out } else { out.to_ascii_lowercase() }
}

fn alpha(number: i64, upper: bool) -> String {
    if number < 1 {
        return String::new();
    }
    let zero_based = number - 1;
    let letter = u8::try_from(zero_based % 26).unwrap_or(0);
    let repeat = usize::try_from(zero_based / 26 + 1).unwrap_or(1);
    let letter = if upper { b'A' + letter } else { b'a' + letter };
    std::iter::repeat_n(char::from(letter), repeat).collect()
}

#[cfg(test)]
mod tests;
