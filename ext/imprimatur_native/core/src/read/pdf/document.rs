//! A PDF file's objects on demand: loaded once and shared, decrypted, and
//! their streams decoded; then the pages, with what they inherit.

mod pages;

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;

use super::crypt::Crypt;
use super::filter;
use super::lexer::{Lexer, Token};
use super::object::{Dict, Name, Object, Parser, Stream, malformed};
use super::xref::{Entry, Xref, is_limit};
use crate::read::Budget;
use crate::{Error, Result};

/// How many references in a row resolve before giving up on a cycle.
const MAX_HOPS: usize = 16;

/// An object borrowed from its container, or a loaded indirect one.
pub enum Resolved<'a> {
    Borrowed(&'a Object),
    Shared(Rc<Object>),
}

impl Resolved<'_> {
    /// Shared ownership of the object: the loaded one, or a copy of a
    /// borrowed direct one.
    pub fn into_shared(self) -> Rc<Object> {
        match self {
            Self::Borrowed(object) => Rc::new(object.clone()),
            Self::Shared(object) => object,
        }
    }
}

impl Deref for Resolved<'_> {
    type Target = Object;

    fn deref(&self) -> &Object {
        match self {
            Self::Borrowed(object) => object,
            Self::Shared(object) => object,
        }
    }
}

/// An object stream's decoded data and where each of its objects starts.
struct ObjectStream {
    data: Vec<u8>,
    offsets: Vec<(u32, usize)>,
}

pub struct Page {
    pub resources: Option<Rc<Object>>,
    pub contents: Vec<Object>,
    pub media_box: [f64; 4],
    pub rotate: i64,
}

pub struct Document<'a> {
    data: &'a [u8],
    budget: &'a Budget<'a>,
    xref: Xref,
    crypt: Option<Crypt>,
    objects: RefCell<HashMap<u32, Rc<Object>>>,
    streams: RefCell<HashMap<u32, Rc<ObjectStream>>>,
    loading: RefCell<Vec<u32>>,
    strict: bool,
}

impl<'a> Document<'a> {
    /// Reads the cross-reference table and opens the encryption, if any.
    pub fn open(data: &'a [u8], budget: &'a Budget<'a>) -> Result<Self> {
        let xref = Xref::read(data, budget)?;
        let mut document = Self {
            data,
            budget,
            xref,
            crypt: None,
            objects: RefCell::default(),
            streams: RefCell::default(),
            loading: RefCell::default(),
            strict: false,
        };
        if let Some(encrypt) = document.xref.trailer.get(b"Encrypt").cloned() {
            let encrypt = document.resolve(&encrypt)?;
            if let Some(encrypt) = encrypt.as_dict() {
                let ids = document.xref.trailer.get(b"ID").and_then(Object::as_array);
                let id =
                    ids.and_then(|ids| ids.first()).and_then(Object::as_bytes).unwrap_or_default();
                document.crypt = Some(Crypt::new(encrypt, id)?);
                // Objects read while opening it were not decrypted.
                document.objects.borrow_mut().clear();
            }
        }
        Ok(document)
    }

    pub const fn budget(&self) -> &'a Budget<'a> {
        self.budget
    }

    pub const fn strict(mut self) -> Self {
        self.strict = true;
        self
    }

    pub const fn trailer(&self) -> &Dict {
        &self.xref.trailer
    }

    /// Object `number`, `Null` when it is missing or unreadable.
    pub fn get(&self, number: u32) -> Result<Rc<Object>> {
        if let Some(found) = self.objects.borrow().get(&number) {
            return Ok(Rc::clone(found));
        }
        if self.loading.borrow().contains(&number) || self.loading.borrow().len() > MAX_HOPS * 4 {
            return Ok(Rc::new(Object::Null));
        }
        self.budget.tick()?;
        self.loading.borrow_mut().push(number);
        let loaded = self.load(number);
        self.loading.borrow_mut().pop();
        let object = match loaded {
            Ok(object) => Rc::new(object),
            Err(error) if is_limit(&error) || matches!(error, Error::PasswordRequired) => {
                return Err(error);
            }
            Err(_) => Rc::new(Object::Null),
        };
        self.objects.borrow_mut().insert(number, Rc::clone(&object));
        Ok(object)
    }

    /// `object`, or what it refers to (through references to references).
    pub fn resolve<'b>(&self, object: &'b Object) -> Result<Resolved<'b>> {
        let Object::Ref(mut number, _) = *object else {
            return Ok(Resolved::Borrowed(object));
        };
        for _ in 0..MAX_HOPS {
            let found = self.get(number)?;
            match *found {
                Object::Ref(next, _) => number = next,
                _ => return Ok(Resolved::Shared(found)),
            }
        }
        Ok(Resolved::Shared(Rc::new(Object::Null)))
    }

    /// A dictionary entry, resolved.
    pub fn entry<'b>(&self, dict: &'b Dict, key: &[u8]) -> Result<Option<Resolved<'b>>> {
        dict.get(key).map(|value| self.resolve(value)).transpose()
    }

    fn load(&self, number: u32) -> Result<Object> {
        match self.xref.entries.get(&number) {
            Some(&Entry::Offset { offset, generation }) => {
                let mut parser = Parser::new(self.data, offset, self.budget.max_depth());
                let length = |length: &Object| match *length {
                    Object::Integer(length) => usize::try_from(length).ok(),
                    Object::Ref(number, _) => self
                        .get(number)
                        .ok()?
                        .as_integer()
                        .and_then(|length| usize::try_from(length).ok()),
                    _ => None,
                };
                let (found, _, mut object) = parser.indirect(&length)?;
                if found != number {
                    return Err(malformed("the cross-reference table points at the wrong object"));
                }
                if let Some(crypt) = &self.crypt {
                    decrypt_strings(&mut object, crypt, (number, generation));
                }
                Ok(object)
            }
            Some(&Entry::Compressed { stream, index }) => self.compressed(stream, number, index),
            _ => Ok(Object::Null),
        }
    }

    /// An object inside an object stream (never encrypted on its own: the
    /// stream is).
    fn compressed(&self, stream_number: u32, number: u32, index: u32) -> Result<Object> {
        let stream = self.object_stream(stream_number)?;
        let at = usize::try_from(index)
            .ok()
            .and_then(|index| stream.offsets.get(index))
            .filter(|(found, _)| *found == number)
            .or_else(|| stream.offsets.iter().find(|(found, _)| *found == number));
        let Some(&(_, offset)) = at else { return Ok(Object::Null) };
        Parser::new(&stream.data, offset, self.budget.max_depth()).object()
    }

    fn object_stream(&self, number: u32) -> Result<Rc<ObjectStream>> {
        if let Some(found) = self.streams.borrow().get(&number) {
            return Ok(Rc::clone(found));
        }
        let object = self.get(number)?;
        let Object::Stream(stream) = &*object else {
            return Err(malformed("an object stream that is not a stream"));
        };
        let data = self.decode(stream, Some(number))?;
        let size = |key: &[u8]| {
            stream.dict.get(key).and_then(Object::as_integer).and_then(|n| usize::try_from(n).ok())
        };
        let (count, first) = (size(b"N").unwrap_or(0), size(b"First").unwrap_or(0));
        let mut lexer = Lexer::new(&data, 0);
        let mut offsets = Vec::with_capacity(count.min(100_000));
        for _ in 0..count {
            let (Some(Token::Integer(number)), Some(Token::Integer(offset))) =
                (lexer.next(), lexer.next())
            else {
                break;
            };
            if let (Ok(number), Ok(offset)) = (u32::try_from(number), usize::try_from(offset)) {
                offsets.push((number, first.saturating_add(offset)));
            }
        }
        let stream = Rc::new(ObjectStream { data, offsets });
        self.streams.borrow_mut().insert(number, Rc::clone(&stream));
        Ok(stream)
    }

    /// A stream's data: decrypted (with its object's key) and decoded.
    pub fn decode(&self, stream: &Stream, number: Option<u32>) -> Result<Vec<u8>> {
        let raw = self.raw(stream, number)?;
        let filters = self.filters(&stream.dict)?;
        if self.strict {
            return filter::decode_strict(&raw, &filter_params(&filters), self.budget);
        }
        decode_with(&raw, &filters, self.budget)
    }

    /// A stream's data decrypted, not yet decoded.
    pub fn raw(&self, stream: &Stream, number: Option<u32>) -> Result<Cow<'a, [u8]>> {
        let raw = self
            .data
            .get(stream.data.clone())
            .ok_or_else(|| malformed("stream data past the end"))?;
        Ok(match (&self.crypt, number) {
            (Some(crypt), Some(number)) if !stream.dict.has_name(b"Type", b"XRef") => {
                let generation = match self.xref.entries.get(&number) {
                    Some(&Entry::Offset { generation, .. }) => generation,
                    _ => 0,
                };
                Cow::Owned(crypt.decrypt(true, (number, generation), raw))
            }
            _ => Cow::Borrowed(raw),
        })
    }

    /// A stream's filters in order, each with its decode parameters.
    pub fn filters(&self, dict: &Dict) -> Result<Vec<Filter>> {
        let filter = self.entry(dict, b"Filter")?;
        let params = self.entry(dict, b"DecodeParms")?;
        let mut filters = Vec::new();
        match filter.as_deref() {
            Some(Object::Name(name)) => {
                filters.push((name.clone(), self.params(params.as_deref(), None)?));
            }
            Some(Object::Array(names)) => {
                for (index, name) in names.iter().enumerate() {
                    if let Object::Name(name) = name {
                        filters.push((name.clone(), self.params(params.as_deref(), Some(index))?));
                    }
                }
            }
            _ => {}
        }
        Ok(filters)
    }

    /// The decode parameters of one filter: the dictionary, or the
    /// `index`th of an array of them.
    fn params(&self, params: Option<&Object>, index: Option<usize>) -> Result<Option<Rc<Object>>> {
        let chosen = match (params, index) {
            (Some(Object::Array(all)), Some(index)) => all.get(index),
            (Some(Object::Array(_)), None) | (None, _) => None,
            (Some(single), _) => Some(single),
        };
        chosen.map(|object| self.resolve(object).map(Resolved::into_shared)).transpose()
    }

    /// A stream given by reference (or directly), decoded; `None` when it is
    /// not a stream.
    pub fn stream_data(&self, object: &Object) -> Result<Option<Vec<u8>>> {
        let number = object.as_reference().map(|(number, _)| number);
        let resolved = self.resolve(object)?;
        match &*resolved {
            Object::Stream(stream) => self.decode(stream, number).map(Some),
            _ => Ok(None),
        }
    }

    pub const fn encrypted(&self) -> bool {
        self.crypt.is_some()
    }
}

/// A filter's name and its decode parameters.
pub type Filter = (Name, Option<Rc<Object>>);

/// `raw` through `filters`, in order.
pub fn decode_with(raw: &[u8], filters: &[Filter], budget: &Budget<'_>) -> Result<Vec<u8>> {
    filter::decode(raw, &filter_params(filters), budget)
}

fn filter_params(filters: &[Filter]) -> Vec<(&[u8], Option<&Dict>)> {
    filters
        .iter()
        .map(|(name, params)| (&**name, params.as_deref().and_then(Object::as_dict)))
        .collect()
}

/// Decrypts every string in an object freshly read from the file.
fn decrypt_strings(object: &mut Object, crypt: &Crypt, id: (u32, u16)) {
    match object {
        Object::String(bytes) => *bytes = crypt.decrypt(false, id, bytes),
        Object::Array(items) => items.iter_mut().for_each(|item| decrypt_strings(item, crypt, id)),
        Object::Dict(dict) | Object::Stream(Stream { dict, .. }) => {
            dict.0.iter_mut().for_each(|(_, value)| decrypt_strings(value, crypt, id));
        }
        _ => {}
    }
}
