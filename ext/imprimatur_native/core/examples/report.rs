//! Writes `report.pdf` and `report.docx` (or `<stem>.pdf` and `<stem>.docx`)
//! from one document: headings, styled runs, a table and page numbers.

use imprimatur::doc::{Document, PageSize, Paragraph, Run, Table};
use imprimatur::style::{Align, Color};

fn main() -> imprimatur::Result<()> {
    let stem = std::env::args().nth(1).unwrap_or_else(|| "report".to_owned());

    let mut doc = Document::new();
    doc.set_page_size(PageSize::A4).set_page_numbers(true);
    doc.heading(1, "Quarterly report")?;
    doc.push_paragraph(
        Paragraph::new()
            .text("Revenue was ")
            .run(Run::new("up 12%").bold().color(Color::rgb(0x1F_7A_1F)))
            .text(" this quarter, which the board called “encouraging” — again."),
    )?;
    doc.heading(2, "Sales by region")?;
    let mut rows = vec![vec!["Region".to_owned(), "Q1".to_owned(), "Q2".to_owned()]];
    rows.extend(
        (1..=40).map(|n| {
            vec![format!("Region {n}"), format!("{}.00", n * 100), format!("{}.50", n * 90)]
        }),
    );
    doc.table(Table::new(rows).header())?;
    doc.heading(2, "Notes")?;
    doc.push_paragraph(Paragraph::new().italic("Figures are unaudited.").align(Align::Center))?;
    doc.push_paragraph(Paragraph::new().run(Run::new("Underlined, and a café.").underline()))?;

    doc.save_pdf(format!("{stem}.pdf"))?;
    doc.save_docx(format!("{stem}.docx"))?;
    println!("wrote {stem}.pdf and {stem}.docx");
    Ok(())
}
