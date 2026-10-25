//! Document authoring with no third-party format stack: the XML and zip
//! writers underneath every format are written here, on `miniz_oxide`
//! (deflate) and `crc32fast`.
//!
//! [`xlsx`] writes workbooks; [`doc`] is one document model that renders to
//! PDF and DOCX; [`barcode`] draws QR Code, Data Matrix, EAN, UPC and Code
//! 128 symbols, GS1 ones included.
//!
//! ```
//! use imprimatur::xlsx::{Cell, Workbook};
//!
//! let mut book = Workbook::new();
//! let sheet = book.add_sheet("Sales")?;
//! sheet.append_row(["Region", "Q1"])?;
//! sheet.append_row([Cell::from("North"), Cell::from(12.5)])?;
//! sheet.append_row([Cell::from("Total"), Cell::formula("SUM(B2:B2)")])?;
//! let bytes = book.to_bytes()?;
//! assert!(bytes.starts_with(b"PK\x03\x04"));
//! # Ok::<(), imprimatur::Error>(())
//! ```

#![cfg_attr(
    all(test, feature = "isolation", unix),
    expect(dead_code_pub_in_binary, reason = "worker entry point is integration-tested")
)]

#[cfg(feature = "archive")]
pub mod archive;
pub mod barcode;
mod bytes;
pub mod doc;
mod docx;
mod error;
#[cfg(feature = "fonts")]
pub mod font;
mod opc;
mod pdf;
pub mod read;
mod script;
pub mod style;
#[cfg(test)]
mod test_support;
pub mod xlsx;
pub(crate) mod xml;
pub(crate) mod zip;

pub use error::{Error, Result};

#[doc = include_str!("../README.md")]
#[cfg(doctest)]
pub struct ReadmeDoctests;
