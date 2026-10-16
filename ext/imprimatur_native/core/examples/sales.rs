//! Writes `sales.xlsx` (or the path given): a formatted header, dates,
//! money, a total formula and column widths, plus a notes sheet.

use imprimatur::xlsx::{Align, Border, Cell, Color, DateTime, Format, Workbook};

fn main() -> imprimatur::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "sales.xlsx".to_owned());
    let header = Format::new()
        .bold()
        .font_color(Color::WHITE)
        .fill(Color::from_hex("#1F4E79")?)
        .align(Align::Center)
        .border(Border::Thin);
    let money = Format::new().number_format("#,##0.00")?;
    let total = money.clone().bold().border(Border::Medium);

    let mut book = Workbook::new();
    let sales = book.add_sheet("Sales")?;
    sales.append_row_with_format(["Region", "Closed", "Amount", "On target"], &header)?;
    let closed = DateTime::date(2026, 10, 15)?;
    sales.append_row([Cell::from("North & West"), closed.into(), 12_500.5.into(), true.into()])?;
    sales.write_with_format(1, 2, 12_500.5, &money)?;
    sales.append_row([
        Cell::from("  South  "),
        closed.at(17, 45, 0, 0)?.into(),
        9_750.25.into(),
        false.into(),
    ])?;
    sales.write_with_format(2, 2, 9_750.25, &money)?;
    sales.append_row_with_format([Cell::from("Total")], &Format::new().bold())?;
    sales.write_with_format(3, 2, Cell::formula("=SUM(C2:C3)"), &total)?;
    sales.set_column_width(0, 18.0)?.set_column_width(1, 20.0)?.set_column_width(2, 14.0)?;

    let notes = book.add_sheet("Notes")?;
    notes.write_with_format(
        0,
        0,
        "Wrapped and italic.\nSecond line.",
        &Format::new().italic().wrap(),
    )?;

    book.save(&path)?;
    println!("wrote {path}");
    Ok(())
}
