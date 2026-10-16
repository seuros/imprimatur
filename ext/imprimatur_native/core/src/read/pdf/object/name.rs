//! PDF names: `Type`, `Font`, `F1`. Nearly all fit in a few bytes, so they
//! live inline; a long one goes to the heap.

use std::fmt;
use std::ops::Deref;

/// The bytes a name keeps inline: its enum is no larger than a `Vec`.
const INLINE: usize = 22;

#[derive(Clone, PartialEq, Eq)]
pub enum Name {
    Inline { length: u8, bytes: [u8; INLINE] },
    Heap(Box<[u8]>),
}

impl Name {
    pub fn new(bytes: &[u8]) -> Self {
        u8::try_from(bytes.len()).ok().filter(|&length| usize::from(length) <= INLINE).map_or_else(
            || Self::Heap(bytes.into()),
            |length| {
                let mut inline = [0; INLINE];
                inline[..bytes.len()].copy_from_slice(bytes);
                Self::Inline { length, bytes: inline }
            },
        )
    }
}

impl Deref for Name {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            Self::Inline { length, bytes } => &bytes[..usize::from(*length)],
            Self::Heap(bytes) => bytes,
        }
    }
}

impl fmt::Debug for Name {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "/{}", String::from_utf8_lossy(self))
    }
}
