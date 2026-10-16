//! The Open Packaging Conventions parts every OOXML package carries
//! (ECMA-376 Part 2): the content types and the relationships.

use crate::xml::XmlWriter;

/// The relationship namespace (`xmlns:r`), and the prefix of every
/// relationship type: `{RELATIONSHIPS}/officeDocument`, `.../styles`, ...
pub const RELATIONSHIPS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const RELATIONSHIPS_CONTENT_TYPE: &str = "application/vnd.openxmlformats-package.relationships+xml";

/// `[Content_Types].xml`: defaults for `.rels`, `.xml` and each
/// `(extension, content type)` in `defaults`, then one override per
/// `(part name, content type)`.
pub fn content_types<P: AsRef<str>, T: AsRef<str>>(
    defaults: &[(&str, &str)],
    overrides: &[(P, T)],
) -> String {
    let mut xml = XmlWriter::new();
    xml.open("Types", &[("xmlns", CONTENT_TYPES_NS)]);
    xml.empty("Default", &[("Extension", "rels"), ("ContentType", RELATIONSHIPS_CONTENT_TYPE)]);
    xml.empty("Default", &[("Extension", "xml"), ("ContentType", "application/xml")]);
    for (extension, content_type) in defaults {
        xml.empty("Default", &[("Extension", extension), ("ContentType", content_type)]);
    }
    for (part, content_type) in overrides {
        xml.empty(
            "Override",
            &[("PartName", part.as_ref()), ("ContentType", content_type.as_ref())],
        );
    }
    xml.close("Types");
    xml.finish()
}

/// A relationships part: `(type, target)` pairs become `rId1`, `rId2`, ...
/// in order; the type is the last segment, such as `styles`.
pub fn relationships<T: AsRef<str>>(targets: &[(&str, T)]) -> String {
    let mut xml = XmlWriter::new();
    xml.open("Relationships", &[("xmlns", PACKAGE_RELATIONSHIPS_NS)]);
    for (index, (kind, target)) in targets.iter().enumerate() {
        let id = format!("rId{}", index + 1);
        let kind = format!("{RELATIONSHIPS}/{kind}");
        xml.empty("Relationship", &[("Id", &id), ("Type", &kind), ("Target", target.as_ref())]);
    }
    xml.close("Relationships");
    xml.finish()
}
