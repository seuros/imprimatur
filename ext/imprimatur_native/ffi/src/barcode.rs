//! `Imprimatur::Barcode`: a barcode symbol, drawn as SVG or PNG or placed in
//! a document. `lib/imprimatur/barcode.rb` owns the keyword interface.

use imprimatur::barcode::{self, Level, Look, Symbology};
use imprimatur::style::Color;
use magnus::{Error, RString, Ruby, Symbol};

use crate::{OrRaise, argument_error, keyword};

#[magnus::wrap(class = "Imprimatur::Barcode", free_immediately, size)]
pub struct Barcode(pub barcode::Barcode);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Qr,
    DataMatrix,
    Ean13,
    Ean8,
    UpcA,
    Code128,
}

const KINDS: [(&str, Kind); 6] = [
    ("qr", Kind::Qr),
    ("datamatrix", Kind::DataMatrix),
    ("ean13", Kind::Ean13),
    ("ean8", Kind::Ean8),
    ("upca", Kind::UpcA),
    ("code128", Kind::Code128),
];
const LEVELS: [(&str, Level); 4] =
    [("l", Level::Low), ("m", Level::Medium), ("q", Level::Quartile), ("h", Level::High)];

#[expect(clippy::needless_pass_by_value, reason = "magnus passes Ruby arguments as owned values")]
impl Barcode {
    /// Called by `Imprimatur::Barcode.encode`.
    pub fn encode(
        ruby: &Ruby,
        symbology: String,
        data: String,
        gs1: bool,
        level: Option<String>,
    ) -> Result<Self, Error> {
        let kind = keyword(ruby, "symbology", &symbology, &KINDS)?;
        let level = level.map(|level| keyword(ruby, "level", &level, &LEVELS)).transpose()?;
        if level.is_some() && kind != Kind::Qr {
            return Err(argument_error(ruby, "level: applies to :qr only"));
        }
        if gs1 && !matches!(kind, Kind::Qr | Kind::DataMatrix | Kind::Code128) {
            return Err(argument_error(ruby, "gs1: applies to :qr, :datamatrix and :code128"));
        }
        let level = level.unwrap_or_default();
        let code = match (kind, gs1) {
            (Kind::Qr, false) => barcode::Barcode::qr(&data, level),
            (Kind::Qr, true) => barcode::Barcode::gs1_qr(&data, level),
            (Kind::DataMatrix, false) => barcode::Barcode::data_matrix(&data),
            (Kind::DataMatrix, true) => barcode::Barcode::gs1_data_matrix(&data),
            (Kind::Ean13, _) => barcode::Barcode::ean13(&data),
            (Kind::Ean8, _) => barcode::Barcode::ean8(&data),
            (Kind::UpcA, _) => barcode::Barcode::upc_a(&data),
            (Kind::Code128, false) => barcode::Barcode::code128(&data),
            (Kind::Code128, true) => barcode::Barcode::gs1_128(&data),
        };
        Ok(Self(code.or_raise(ruby)?))
    }

    fn svg(
        ruby: &Ruby,
        rb_self: &Self,
        module: f64,
        look: (String, Option<String>, bool, Option<u32>),
    ) -> Result<String, Error> {
        rb_self.0.svg(module, &self::look(ruby, look)?).or_raise(ruby)
    }

    /// The PNG as a binary String.
    fn png(
        ruby: &Ruby,
        rb_self: &Self,
        scale: u32,
        look: (String, Option<String>, bool, Option<u32>),
    ) -> Result<RString, Error> {
        let png = rb_self.0.png(scale, &self::look(ruby, look)?).or_raise(ruby)?;
        Ok(ruby.str_from_slice(&png))
    }

    fn symbology(ruby: &Ruby, rb_self: &Self) -> Symbol {
        let name = match rb_self.0.symbology() {
            Symbology::QrCode => "qr",
            Symbology::DataMatrix => "datamatrix",
            Symbology::Ean13 => "ean13",
            Symbology::Ean8 => "ean8",
            Symbology::UpcA => "upca",
            _ => "code128",
        };
        ruby.to_symbol(name)
    }

    fn text(&self) -> String {
        self.0.text().to_owned()
    }

    const fn is_gs1(&self) -> bool {
        self.0.is_gs1()
    }

    const fn width(&self) -> usize {
        self.0.size().0
    }

    const fn height(&self) -> usize {
        self.0.size().1
    }

    /// Rows of modules, true for dark ones.
    fn modules(&self) -> Vec<Vec<bool>> {
        let (columns, rows) = self.0.size();
        (0..rows).map(|y| (0..columns).map(|x| self.0.is_dark(x, y)).collect()).collect()
    }
}

/// `[color, background, quiet_zone, bar_height]`, as `Imprimatur::Barcode`
/// passes them.
fn look(
    ruby: &Ruby,
    (dark, light, quiet_zone, bar_height): (String, Option<String>, bool, Option<u32>),
) -> Result<Look, Error> {
    let dark = Color::from_hex(&dark).or_raise(ruby)?;
    let light = light.map(|light| Color::from_hex(&light)).transpose().or_raise(ruby)?;
    Ok(Look { dark, light, quiet_zone, bar_height })
}

pub fn define(ruby: &Ruby, imprimatur: magnus::RModule) -> Result<(), Error> {
    use magnus::{function, method, prelude::*};

    let class = imprimatur.define_class("Barcode", ruby.class_object())?;
    class.define_singleton_method("__encode", function!(Barcode::encode, 4))?;
    class.define_method("__svg", method!(Barcode::svg, 2))?;
    class.define_method("__png", method!(Barcode::png, 2))?;
    class.define_method("symbology", method!(Barcode::symbology, 0))?;
    class.define_method("text", method!(Barcode::text, 0))?;
    class.define_method("gs1?", method!(Barcode::is_gs1, 0))?;
    class.define_method("width", method!(Barcode::width, 0))?;
    class.define_method("height", method!(Barcode::height, 0))?;
    class.define_method("modules", method!(Barcode::modules, 0))?;
    matryona_machines::ruby::Class::pin_wrapped::<Barcode>(ruby, class).map(drop)
}
