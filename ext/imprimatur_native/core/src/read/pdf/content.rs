//! Runs a page's content streams (ISO 32000-1, 8 and 9.4) far enough to
//! know where each glyph lands: graphics and text state, text operators,
//! and form `XObjects`. Images are counted, or collected to be extracted;
//! paths and colours are skipped.

mod geometry;
mod operands;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use super::document::{Document, Page};
use super::font::Font;
use super::glyphs::push_clean;
use super::lexer::{Lexer, Token, hex_into, literal_into, name_bytes};
use super::object::{Dict, Object};
use crate::Result;
use crate::read::span;
use geometry::{Frame, IDENTITY, Matrix, multiply, quarter_turns};
use operands::{Operand, inline_image, operand};

/// A glyph where it shows: its origin and advance along the baseline, in
/// the page's displayed frame (rotation applied), and its text.
#[derive(Debug, Clone)]
pub struct Glyph {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub size: f64,
    /// Quarter turns of the baseline from left-to-right: 0 is upright.
    pub turns: u8,
    pub bold: bool,
    pub text: Range<u32>,
}

/// An image a page draws: an image `XObject` (its object number, when it
/// has one) or an inline image, and the transformation it is drawn with.
pub struct Drawn {
    pub image: Drawing,
    pub ctm: Matrix,
}

pub enum Drawing {
    Object { number: Option<u32>, object: Rc<Object> },
    Inline { dict: Dict, data: Vec<u8> },
}

/// A page's glyphs, their text in one buffer, and how many images it draws.
#[derive(Debug, Default)]
pub struct PageText {
    pub text: String,
    pub glyphs: Vec<Glyph>,
    pub images: usize,
    /// The displayed page: width and height.
    pub size: (f64, f64),
}

/// The text matrix and the text line matrix (ISO 32000-1, 9.4.2).
#[derive(Clone, Copy)]
struct Cursor {
    matrix: Matrix,
    line: Matrix,
}

impl Cursor {
    const START: Self = Self { matrix: IDENTITY, line: IDENTITY };

    const fn at(matrix: Matrix) -> Self {
        Self { matrix, line: matrix }
    }

    /// `Td`: the start of a new line, offset from the current line's start.
    fn next_line(&mut self, x: f64, y: f64) {
        self.line = multiply(&[1.0, 0.0, 0.0, 1.0, x, y], &self.line);
        self.matrix = self.line;
    }
}

#[derive(Clone)]
struct State {
    ctm: Matrix,
    font: Option<Rc<Font>>,
    size: f64,
    char_spacing: f64,
    word_spacing: f64,
    scale: f64,
    leading: f64,
    rise: f64,
}

impl State {
    const START: Self = Self {
        ctm: IDENTITY,
        font: None,
        size: 0.0,
        char_spacing: 0.0,
        word_spacing: 0.0,
        scale: 1.0,
        leading: 0.0,
        rise: 0.0,
    };
}

/// Fonts by the object number of their dictionary, shared across pages.
pub type Fonts = HashMap<u32, Rc<Font>>;

struct Run<'d, 'p> {
    document: &'d Document<'p>,
    fonts: &'d mut Fonts,
    page: &'d mut PageText,
    rotation: Matrix,
    bounds: [f64; 4],
    forms: Vec<u32>,
    scratch: String,
    /// A string operand's decoded bytes.
    bytes: Vec<u8>,
    /// Where the images go, when reading them rather than text.
    drawn: Option<&'d mut Vec<Drawn>>,
}

/// The page's glyphs, into `out` (cleared first: its buffers are reused
/// from page to page).
pub fn page_text(
    document: &Document<'_>,
    page: &Page,
    fonts: &mut Fonts,
    out: &mut PageText,
) -> Result<()> {
    run(document, page, fonts, out, None).map(|_| ())
}

/// The images the page draws, into `out`; and how the page is shown.
pub fn page_images(document: &Document<'_>, page: &Page, out: &mut Vec<Drawn>) -> Result<Frame> {
    let mut text = PageText::default();
    run(document, page, &mut Fonts::new(), &mut text, Some(out))
}

fn run(
    document: &Document<'_>,
    page: &Page,
    fonts: &mut Fonts,
    out: &mut PageText,
    drawn: Option<&mut Vec<Drawn>>,
) -> Result<Frame> {
    let frame = Frame::of(page);
    out.size = frame.size;
    out.text.clear();
    out.glyphs.clear();
    out.images = 0;
    let content = match page.contents.as_slice() {
        [single] => document.stream_data(single)?.unwrap_or_default(),
        parts => {
            let mut content = Vec::new();
            for part in parts {
                if let Some(data) = document.stream_data(part)? {
                    content.extend_from_slice(&data);
                    content.push(b'\n');
                }
            }
            content
        }
    };
    let empty = Dict::default();
    let resources = page.resources.as_deref().and_then(Object::as_dict).unwrap_or(&empty);
    let mut run = Run {
        document,
        fonts,
        page: out,
        rotation: frame.rotation,
        bounds: frame.bounds,
        forms: Vec::new(),
        scratch: String::new(),
        bytes: Vec::new(),
        drawn,
    };
    run.stream(&content, resources, State::START, 0)?;
    Ok(frame)
}

impl Run<'_, '_> {
    fn stream(
        &mut self,
        content: &[u8],
        resources: &Dict,
        mut state: State,
        depth: usize,
    ) -> Result<()> {
        let budget = self.document.budget();
        budget.nest(depth)?;
        let mut stack: Vec<State> = Vec::new();
        let mut cursor = Cursor::START;
        let (mut operands, mut items): (Vec<Operand<'_>>, Vec<Operand<'_>>) =
            (Vec::new(), Vec::new());
        let mut lexer = Lexer::new(content, 0);
        while let Some(token) = lexer.next() {
            budget.tick()?;
            let Token::Keyword(operator) = token else {
                let operand = operand(token, &mut lexer, &mut items);
                if operands.len() < 64 {
                    operands.push(operand);
                }
                continue;
            };
            let number = |index: usize| match operands.get(index) {
                Some(&Operand::Number(number)) => number,
                _ => 0.0,
            };
            let name = |index: usize| match operands.get(index) {
                Some(&Operand::Name(raw)) => Some(name_bytes(raw)),
                _ => None,
            };
            match operator {
                b"q" => {
                    if stack.len() < 256 {
                        stack.push(state.clone());
                    }
                }
                b"Q" => {
                    if let Some(saved) = stack.pop() {
                        state = saved;
                    }
                }
                b"cm" if operands.len() == 6 => {
                    state.ctm = multiply(&std::array::from_fn(number), &state.ctm);
                }
                b"BT" => cursor = Cursor::START,
                b"Tf" => {
                    state.size = number(1);
                    state.font = match name(0) {
                        // Reading images, fonts are no use.
                        Some(name) if self.drawn.is_none() => self.font(resources, &name)?,
                        _ => None,
                    };
                }
                b"Tc" => state.char_spacing = number(0),
                b"Tw" => state.word_spacing = number(0),
                b"Tz" => state.scale = number(0) / 100.0,
                b"TL" => state.leading = number(0),
                b"Ts" => state.rise = number(0),
                b"Td" | b"TD" => {
                    if operator == b"TD" {
                        state.leading = -number(1);
                    }
                    cursor.next_line(number(0), number(1));
                }
                b"Tm" if operands.len() == 6 => cursor = Cursor::at(std::array::from_fn(number)),
                b"T*" => cursor.next_line(0.0, -state.leading),
                b"Tj" | b"'" | b"\"" | b"TJ" => {
                    if operator == b"\"" {
                        state.word_spacing = number(0);
                        state.char_spacing = number(1);
                    }
                    if operator == b"'" || operator == b"\"" {
                        cursor.next_line(0.0, -state.leading);
                    }
                    self.show_operand(operands.last(), &items, &state, &mut cursor.matrix);
                }
                b"Do" => {
                    if let Some(name) = name(0) {
                        self.draw(resources, &name, &state, depth)?;
                    }
                }
                b"BI" => {
                    self.page.images += 1;
                    let image = inline_image(&mut lexer, budget.max_depth());
                    if let (Some(drawn), Some((dict, data))) = (self.drawn.as_deref_mut(), image) {
                        let image = Drawing::Inline { dict, data: data.to_vec() };
                        drawn.push(Drawn { image, ctm: state.ctm });
                    }
                }
                _ => {}
            }
            operands.clear();
            items.clear();
        }
        Ok(())
    }

    /// The operand of `Tj` and its kin: a string, or `TJ`'s array of
    /// strings and adjustments in thousandths of the font size.
    fn show_operand(
        &mut self,
        operand: Option<&Operand<'_>>,
        items: &[Operand<'_>],
        state: &State,
        matrix: &mut Matrix,
    ) {
        let shown = match operand {
            Some(&Operand::Array(start, end)) => items.get(start..end).unwrap_or_default(),
            Some(single) => std::slice::from_ref(single),
            None => &[],
        };
        let mut bytes = std::mem::take(&mut self.bytes);
        for item in shown {
            match *item {
                Operand::String { raw, hex } => {
                    bytes.clear();
                    if hex {
                        hex_into(raw, &mut bytes);
                    } else {
                        literal_into(raw, &mut bytes);
                    }
                    self.show(&bytes, state, matrix);
                }
                Operand::Number(adjustment) => {
                    let shift = -adjustment / 1000.0 * state.size * state.scale;
                    matrix[4] = shift.mul_add(matrix[0], matrix[4]);
                    matrix[5] = shift.mul_add(matrix[1], matrix[5]);
                }
                _ => {}
            }
        }
        self.bytes = bytes;
    }

    /// A font resource, loaded once per document.
    fn font(&mut self, resources: &Dict, name: &[u8]) -> Result<Option<Rc<Font>>> {
        let document = self.document;
        let Some(fonts) = document.entry(resources, b"Font")? else { return Ok(None) };
        let Some(entry) = fonts.as_dict().and_then(|fonts| fonts.get(name)) else {
            return Ok(None);
        };
        if let Some((number, _)) = entry.as_reference()
            && let Some(font) = self.fonts.get(&number)
        {
            return Ok(Some(Rc::clone(font)));
        }
        let resolved = document.resolve(entry)?;
        let Some(dict) = resolved.as_dict() else { return Ok(None) };
        let font = Rc::new(Font::load(document, dict)?);
        if let Some((number, _)) = entry.as_reference() {
            self.fonts.insert(number, Rc::clone(&font));
        }
        Ok(Some(font))
    }

    /// Emits a string's glyphs and advances the text matrix past them.
    fn show(&mut self, bytes: &[u8], state: &State, matrix: &mut Matrix) {
        let Some(font) = state.font.clone() else { return };
        let device = multiply(matrix, &state.ctm);
        let shown = multiply(&device, &self.rotation);
        let along = shown[0].hypot(shown[1]);
        let size = state.size * shown[2].hypot(shown[3]);
        let turns = quarter_turns(shown[0], shown[1]);
        let rise = (state.rise * shown[2], state.rise * shown[3]);
        let mut advance = 0.0;
        let mut scratch = std::mem::take(&mut self.scratch);
        let bounds = self.bounds;
        font.glyphs(bytes, &mut scratch, |text, width, space| {
            let x = f64::mul_add(advance, shown[0], shown[4]) + rise.0;
            let y = f64::mul_add(advance, shown[1], shown[5]) + rise.1;
            let user = (
                f64::mul_add(advance, device[0], device[4]),
                f64::mul_add(advance, device[1], device[5]),
            );
            let visible = user.0 >= bounds[0] - 1.0
                && user.0 <= bounds[2] + 1.0
                && user.1 >= bounds[1] - 1.0
                && user.1 <= bounds[3] + 1.0;
            if visible && !text.is_empty() {
                let span = span::append(&mut self.page.text, |page| push_clean(page, text));
                if !span.is_empty() {
                    let width = width * state.size * state.scale * along;
                    self.page.glyphs.push(Glyph {
                        x,
                        y,
                        width,
                        size,
                        turns,
                        bold: font.bold,
                        text: span,
                    });
                }
            }
            advance = f64::mul_add(
                f64::mul_add(width, state.size, state.char_spacing)
                    + if space { state.word_spacing } else { 0.0 },
                state.scale,
                advance,
            );
        });
        self.scratch = scratch;
        matrix[4] = f64::mul_add(advance, matrix[0], matrix[4]);
        matrix[5] = f64::mul_add(advance, matrix[1], matrix[5]);
    }

    /// `Do`: a form `XObject`'s content, in its own matrix and resources;
    /// images are counted.
    fn draw(&mut self, resources: &Dict, name: &[u8], state: &State, depth: usize) -> Result<()> {
        let document = self.document;
        let Some(objects) = document.entry(resources, b"XObject")? else { return Ok(()) };
        let Some(entry) = objects.as_dict().and_then(|objects| objects.get(name)).cloned() else {
            return Ok(());
        };
        let number = entry.as_reference().map(|(number, _)| number);
        if number.is_some_and(|number| self.forms.contains(&number)) {
            return Ok(());
        }
        let resolved = document.resolve(&entry)?;
        let Object::Stream(stream) = &*resolved else { return Ok(()) };
        if stream.dict.has_name(b"Subtype", b"Image") {
            self.page.images += 1;
            if let Some(drawn) = self.drawn.as_deref_mut() {
                let object = Rc::clone(match &resolved {
                    super::document::Resolved::Shared(object) => object,
                    super::document::Resolved::Borrowed(_) => return Ok(()),
                });
                drawn.push(Drawn { image: Drawing::Object { number, object }, ctm: state.ctm });
            }
            return Ok(());
        }
        if !stream.dict.has_name(b"Subtype", b"Form") {
            return Ok(());
        }
        let content = document.decode(stream, number)?;
        let own = document.entry(&stream.dict, b"Resources")?;
        let form_resources = own.as_deref().and_then(Object::as_dict).unwrap_or(resources);
        let matrix =
            match document.entry(&stream.dict, b"Matrix")?.as_deref().and_then(Object::as_array) {
                Some(values) if values.len() == 6 => {
                    std::array::from_fn(|index| values[index].as_number().unwrap_or(0.0))
                }
                _ => IDENTITY,
            };
        let mut inner = state.clone();
        inner.ctm = multiply(&matrix, &state.ctm);
        if let Some(number) = number {
            self.forms.push(number);
        }
        let result = self.stream(&content, form_resources, inner, depth + 1);
        if number.is_some() {
            self.forms.pop();
        }
        result
    }
}
