use std::assert_matches;

use super::*;

fn written(validation: &Validation, range: &str) -> String {
    let mut xml = XmlWriter::bare();
    validation.write(&mut xml, &range.parse().unwrap());
    xml.finish()
}

#[test]
fn writes_inline_and_range_lists() {
    let yes_no = Validation::list(["Yes", "No"]).unwrap();
    assert_eq!(
        written(&yes_no, "G2:G1000"),
        concat!(
            r#"<dataValidation type="list" allowBlank="1" showErrorMessage="1" sqref="G2:G1000">"#,
            r#"<formula1>"Yes,No"</formula1></dataValidation>"#,
        )
    );
    let quoted = Validation::list([r#"6" pipe"#, "m²"]).unwrap();
    assert!(written(&quoted, "A1").contains(r#"<formula1>"6"" pipe,m²"</formula1>"#));

    for source in ["Connections!$A$2:$A$1000", "=Connections!$A$2:$A$1000"] {
        let xml = written(&Validation::list_from(source).unwrap(), "F2:F1000");
        assert!(xml.contains("<formula1>Connections!$A$2:$A$1000</formula1>"), "{source}: {xml}");
    }
}

#[test]
fn writes_options_prompts_and_alerts() {
    let validation = Validation::custom("=LEN(A2)<=100")
        .unwrap()
        .allow_blank(false)
        .alert(Alert::Warning)
        .prompt("Name", "Up to 100 characters")
        .unwrap()
        .error("Too long", "Names stop at 100 characters & no more.\nTry again.")
        .unwrap();
    let xml = written(&validation, "A2:A100");
    let expected = concat!(
        r#"<dataValidation type="custom" errorStyle="warning" showInputMessage="1" "#,
        r#"showErrorMessage="1" errorTitle="Too long" "#,
        r#"error="Names stop at 100 characters &amp; no more.&#10;Try again." "#,
        r#"promptTitle="Name" prompt="Up to 100 characters" sqref="A2:A100">"#,
        r#"<formula1>LEN(A2)&lt;=100</formula1></dataValidation>"#,
    );
    assert_eq!(xml, expected);

    let quiet = Validation::list(["a"]).unwrap().dropdown(false).alert(Alert::Off);
    let xml = written(&quiet, "B2");
    assert!(xml.contains(r#"allowBlank="1" showDropDown="1" sqref="B2""#), "{xml}");
    assert!(!xml.contains("showErrorMessage"), "{xml}");

    let titled = Validation::list(["a"]).unwrap().prompt("Country", "").unwrap();
    let xml = written(&titled, "B2");
    assert!(
        xml.contains(r#"showInputMessage="1" showErrorMessage="1" promptTitle="Country" sqref"#)
    );
    assert!(!xml.contains("prompt="), "an empty message is left out: {xml}");
}

#[test]
fn rejects_what_excel_would() {
    let long = "x".repeat(256);
    let rejected: [(&str, Result<Validation>); 8] = [
        ("no values", Validation::list(Vec::<&str>::new())),
        ("only empty", Validation::list([""])),
        ("comma", Validation::list(["a,b"])),
        ("control", Validation::list(["a\nb"])),
        ("long list", Validation::list(["x".repeat(128), "y".repeat(127)])),
        ("empty source", Validation::list_from("=")),
        ("long formula", Validation::custom("1".repeat(8_193))),
        ("long title", Validation::list(["a"]).unwrap().error("t".repeat(33), "")),
    ];
    for (label, result) in rejected {
        assert_matches!(result, Err(Error::InvalidValidation(_)), "{label}");
    }
    assert_matches!(
        Validation::list(["a"]).unwrap().prompt("", long),
        Err(Error::InvalidValidation(_))
    );
    assert!(Validation::list(["x".repeat(127), "y".repeat(127)]).is_ok(), "255 with the comma");
}
