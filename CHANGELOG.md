# Changelog

## Unreleased

- Isolate PDF metadata and text reads in the bundled resource-limited Rust
  worker, as rendering already is; all three accept `memory_bytes:`.
- On Linux/FreeBSD, re-exec the worker under its address-space limit before
  reading the PDF, including tiny metadata-only reads.
- Preserve completed pages on worker termination and recover healthy neighbors
  after page-local stream limits without renewing the overall deadline.
- Allow a bounded exit grace when macOS cannot inspect an exiting worker;
  persistent monitor failures still terminate and reap a live worker.

## [0.8.1](https://github.com/seuros/imprimatur/compare/v0.8.0...v0.8.1) (2026-10-29)


### Bug Fixes

* **pdf:** recover timeout neighbours within the overall deadline ([#16](https://github.com/seuros/imprimatur/issues/16)) ([e86fbbf](https://github.com/seuros/imprimatur/commit/e86fbbfef85f2c645090c43359c0744962e951a1))
* **pdf:** step timeout recovery so a Ruby wakeup cannot restart it ([75c3894](https://github.com/seuros/imprimatur/commit/75c3894c8a791c5ef4af562dc99de4e745770ad3))

## [0.8.0](https://github.com/seuros/imprimatur/compare/v0.7.0...v0.8.0) (2026-10-27)


### Features

* **reader:** accept drop_repeated in the Ruby API ([4e537d6](https://github.com/seuros/imprimatur/commit/4e537d641bafd8d3cdb8b1c20d16f4cb766ad073))


### Bug Fixes

* **pdf:** read printed page numbers from headers too, not just footers ([c036434](https://github.com/seuros/imprimatur/commit/c0364341ee8fc36be2c97d25fd06da0aa76a4ad6))

## [0.7.0](https://github.com/seuros/imprimatur/compare/v0.6.1...v0.7.0) (2026-10-26)


### Features

* **pdf:** add --drop-repeated to remove watermarks anywhere on the page ([8ea82c9](https://github.com/seuros/imprimatur/commit/8ea82c98f6b507ff7ffd296f8e0ec8f30aed529f))
* **pdf:** emit printed page labels from /PageLabels or footer text ([f761555](https://github.com/seuros/imprimatur/commit/f76155578acfc8514465b93b5b019df100d54de7))
* **pdf:** recognize numbered sections as headings in markdown output ([341f651](https://github.com/seuros/imprimatur/commit/341f651631b74ff93879f9eba71d0874ae261fd1))


### Bug Fixes

* **pdf:** keep ordinal superscripts attached to their base text ([21fbb51](https://github.com/seuros/imprimatur/commit/21fbb510f90ab7c029d22e4e396b887fa66e16c6))

## [0.6.1](https://github.com/seuros/imprimatur/compare/v0.6.0...v0.6.1) (2026-10-25)


### Bug Fixes

* **pdf:** isolate bounded reads and preserve page recovery ([fbfbbc5](https://github.com/seuros/imprimatur/commit/fbfbbc5e7e44768efae293b37be255b8b797f831))

## [0.6.0](https://github.com/seuros/imprimatur/compare/v0.5.0...v0.6.0) (2026-10-25)


### Features

* bound PDF reads and rendering; deduplicate Rust ([420e096](https://github.com/seuros/imprimatur/commit/420e096591ef08190427c7b090840dcd761010b4))
* **doc:** set the body font size ([44f7617](https://github.com/seuros/imprimatur/commit/44f76170fd1ad29f143c1ef22c355fd1c1cbf8e1))


### Bug Fixes

* enforce render limits across Unix platforms ([5c7f0ba](https://github.com/seuros/imprimatur/commit/5c7f0bab2076681782c6bfa2de6062ad53c0e7f9))

## [0.5.0](https://github.com/seuros/imprimatur/compare/v0.4.0...v0.5.0) (2026-10-18)


### Features

* a binary String is always a document's bytes, a path must name a regular file, and reads stop at max_bytes ([3484a64](https://github.com/seuros/imprimatur/commit/3484a64b791beb2e9f90cd485441fae255e223f0))

## [0.4.0](https://github.com/seuros/imprimatur/compare/v0.3.0...v0.4.0) (2026-10-18)


### Features

* barcodes: QR Code, Data Matrix, EAN-13, EAN-8, UPC-A and Code 128, GS1 forms included, as SVG, PNG and in documents ([ade7b68](https://github.com/seuros/imprimatur/commit/ade7b68f4c5adcab9d2ee0b935288eea7ec2eea6))
* **pdf:** add embedded TrueType font resources ([c0e3be4](https://github.com/seuros/imprimatur/commit/c0e3be4ef46ede94fcc840840e82d8ccb6679f3e))
* **pdf:** add embedded TrueType font resources ([313075f](https://github.com/seuros/imprimatur/commit/313075fe8ba1bedc9f4156060aeedcbb0e0a8bef))

## [0.3.0](https://github.com/seuros/imprimatur/compare/v0.2.0...v0.3.0) (2026-10-17)


### Features

* a spreadsheet's sheets give their comments, columns, validations and styles ([1797632](https://github.com/seuros/imprimatur/commit/1797632aad2948c1c0a493501de3684187062246))
* draw PNG and JPEG pictures, on pages of any size ([419524a](https://github.com/seuros/imprimatur/commit/419524afe36bbb06e3ef0ecb48a4fab4f80fb989))
* **font:** load static TrueType fonts and metrics ([fd85e3d](https://github.com/seuros/imprimatur/commit/fd85e3d78ece99506b2dde2c61bab6997cb20d21))
* read column widths, data validations and cell styles ([2930964](https://github.com/seuros/imprimatur/commit/2930964cde82cc998113aa83336a547cda8bd9f8))
* read whether a list validation shows its drop-down ([b75ef53](https://github.com/seuros/imprimatur/commit/b75ef530ef790876c58e40bb02dfc7257a48aab5))
* read workbooks as typed rows with Imprimatur::Spreadsheet ([927033b](https://github.com/seuros/imprimatur/commit/927033bb5ef9725c38726bc0785bbd90be8f1dc8))
* size columns to their cells, and give workbooks a font ([c88a058](https://github.com/seuros/imprimatur/commit/c88a0580a7ba052c4994faf4f53b42e75f48f587))
* write right-to-left text in its own direction ([137206e](https://github.com/seuros/imprimatur/commit/137206e08622f33ae21b6e52013e3ec7b071bb6b))


### Bug Fixes

* build the gem with every feature of the crate ([7b48599](https://github.com/seuros/imprimatur/commit/7b485998abdb8b14b59e22a6ff97ca7dec143986))
* **font:** put the TrueType loader behind a `fonts` feature ([f9d1915](https://github.com/seuros/imprimatur/commit/f9d1915f3c98c0c7351477c6e8f21799d01e02f9))
* read right-to-left PDF text in its page's direction ([ca173ef](https://github.com/seuros/imprimatur/commit/ca173efd019513f25df004feafeda0e8551e64c0))
* show numbers with their format's decimals and grouping ([d0089ee](https://github.com/seuros/imprimatur/commit/d0089ee2dcf364ac32c7f92dc6c3af9aa6a9e9cb))

## [0.2.0](https://github.com/seuros/imprimatur/compare/v0.1.0...v0.2.0) (2026-10-16)


### Features

* read legacy XLS and ODS workbooks ([96b86fa](https://github.com/seuros/imprimatur/commit/96b86faab8eb04b80d33885c61b17ebad4f1f63a))
* zip archive comments and entry listings ([a87f223](https://github.com/seuros/imprimatur/commit/a87f22330859ee4a583943ddbf107571cca9e841))
* zip archives of any files ([d5ea579](https://github.com/seuros/imprimatur/commit/d5ea579ae1fc80a8ec6a1b213f442d490ededfba))

## 0.1.0 (2026-10-16)


### Features

* extract from forgejo ([d6c0937](https://github.com/seuros/imprimatur/commit/d6c0937b8cbdda8f6770f43e0423ef6aa4c8c646))
