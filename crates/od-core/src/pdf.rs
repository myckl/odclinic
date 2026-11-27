//! A small, dependency-free PDF writer for printable patient statements and
//! A5 invoices.
//!
//! Open Dental produces statements through MigraDoc; this port emits PDF 1.4
//! directly with the two standard Type 1 fonts, so no native library or
//! downloadable crate is needed. The writer supports flowing text across
//! multiple pages, A4 for statements and A5 for invoices, and a correct
//! cross-reference table.

use crate::{PatientFormDocument, RefundTransferReceipt, StatementDocument};
use od_domain::{Id, InvoiceDocument};

/// Page geometry in PDF points (72 per inch). A5 is 148 x 210 mm.
#[derive(Clone, Copy)]
struct PageSpec {
    width: u32,
    height: u32,
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
}

const A4: PageSpec = PageSpec {
    width: 595,
    height: 842,
    left: 40.0,
    right: 555.0,
    top: 802.0,
    bottom: 56.0,
};

const A5: PageSpec = PageSpec {
    width: 420,
    height: 595,
    left: 28.0,
    right: 392.0,
    top: 565.0,
    bottom: 32.0,
};

/// Escape a string for a PDF literal `(...)` and map characters into
/// WinAnsiEncoding. Anything outside Latin-1 becomes `?` because the standard
/// Helvetica fonts have no glyph for it.
fn literal(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '\n' | '\r' => out.push(' '),
            c if (c as u32) >= 32 && (c as u32) <= 255 => {
                let byte = c as u32;
                if byte <= 126 {
                    out.push(c);
                } else {
                    out.push_str(&format!("\\{:03o}", byte));
                }
            }
            _ => out.push('?'),
        }
    }
    out
}

/// Approximate the x coordinate that right-aligns `text` at `right_edge`.
/// Digits in Helvetica average a little over half the font size.
fn right_x(text: &str, size: f64, right_edge: f64) -> f64 {
    right_edge - text.chars().count() as f64 * size * 0.52
}

struct Writer {
    /// Completed page content streams, in order.
    pages: Vec<String>,
    content: String,
    y: f64,
    title: String,
    page: PageSpec,
}

impl Writer {
    fn new(title: &str) -> Self {
        Self::with_page(title, A4)
    }

    fn with_page(title: &str, page: PageSpec) -> Self {
        let mut writer = Self {
            pages: Vec::new(),
            content: String::new(),
            y: page.top,
            title: title.to_string(),
            page,
        };
        writer.put(page.left, writer.y, "F2", 16.0, title);
        writer.y -= 22.0;
        writer
    }

    fn put(&mut self, x: f64, y: f64, font: &str, size: f64, text: &str) {
        self.content.push_str(&format!(
            "BT /{} {:.1} Tf 1 0 0 1 {:.2} {:.2} Tm ({}) Tj ET\n",
            font,
            size,
            x,
            y,
            literal(text)
        ));
    }

    /// Start a new page when the cursor reaches the bottom margin.
    fn ensure_room(&mut self, step: f64) {
        if self.y - step >= self.page.bottom {
            return;
        }
        let previous = std::mem::take(&mut self.content);
        self.pages.push(previous);
        self.y = self.page.top;
        let continuation = format!("{} (continued)", self.title);
        self.put(self.page.left, self.y, "F2", 11.0, &continuation);
        self.y -= 20.0;
    }

    /// One full-width line, advancing the vertical cursor.
    fn line(&mut self, font: &str, size: f64, text: &str, step: f64) {
        self.ensure_room(step);
        self.put(self.page.left, self.y, font, size, text);
        self.y -= step;
    }

    /// Several columns on one line. Each tuple is `(x, text)`.
    fn columns(&mut self, font: &str, size: f64, cols: &[(f64, String)], step: f64) {
        self.ensure_room(step);
        for (x, text) in cols {
            self.put(*x, self.y, font, size, text);
        }
        self.y -= step;
    }

    fn finish(mut self) -> Vec<String> {
        self.pages.push(self.content);
        self.pages
    }
}

fn money(value: f64) -> String {
    format!("{value:.2}")
}

/// Render a statement as PDF bytes ready to be written to disk.
pub fn render_statement_pdf(doc: &StatementDocument) -> Vec<u8> {
    let title = format!("Statement - {}", doc.patient_name);
    let mut writer = Writer::new(&title);
    let page = writer.page;
    writer.line(
        "F1",
        9.0,
        &format!(
            "Patient #{}  |  Generated {}",
            doc.pat_num, doc.generated_on
        ),
        18.0,
    );

    writer.line("F2", 11.0, "Account activity", 20.0);
    writer.columns(
        "F2",
        9.0,
        &[
            (page.left, "Date".into()),
            (110.0, "Type".into()),
            (190.0, "Description".into()),
            (right_x("Charge", 9.0, 440.0), "Charge".into()),
            (right_x("Credit", 9.0, page.right), "Credit".into()),
        ],
        16.0,
    );
    for line in &doc.lines {
        let description: String = line.description.chars().take(58).collect();
        writer.columns(
            "F1",
            9.0,
            &[
                (page.left, line.date.to_string()),
                (110.0, line.kind.clone()),
                (190.0, description),
                (right_x(&money(line.charge), 9.0, 440.0), money(line.charge)),
                (right_x(&money(line.credit), 9.0, page.right), money(line.credit)),
            ],
            14.0,
        );
    }

    writer.y -= 6.0;
    writer.line(
        "F1",
        9.0,
        &format!(
            "Charges {}  |  Adjustments {}  |  Payments {}",
            money(doc.charges),
            money(doc.adjustments),
            money(doc.payments)
        ),
        16.0,
    );
    writer.line("F2", 12.0, &format!("Balance due: {}", money(doc.balance)), 22.0);

    if !doc.plan_balances.is_empty() {
        writer.y -= 4.0;
        writer.line("F2", 11.0, "Payment plans", 18.0);
        for (label, balance) in &doc.plan_balances {
            let label: String = label.chars().take(60).collect();
            let amount = money(*balance);
            writer.columns(
                "F1",
                9.0,
                &[
                    (page.left, label),
                    (right_x(&amount, 9.0, page.right), amount),
                ],
                14.0,
            );
        }
    }

    if !doc.note.trim().is_empty() {
        writer.y -= 4.0;
        writer.line("F2", 11.0, "Note", 18.0);
        for note_line in doc.note.lines().take(8) {
            let note_line: String = note_line.chars().take(90).collect();
            writer.line("F1", 9.0, &note_line, 13.0);
        }
    }

    assemble_pdf(&writer.finish(), doc.pat_num, page, "Statement")
}

/// Render a patient intake/medical-history form as A4 PDF bytes. The title and
/// footer carry the patient number and generation date, so a filed copy is
/// self-describing even outside the application.
pub fn render_patient_form_pdf(doc: &PatientFormDocument) -> Vec<u8> {
    let title = format!("Patient form - {}", doc.patient_name);
    let mut writer = Writer::new(&title);
    let page = writer.page;
    writer.line(
        "F1",
        9.0,
        &format!(
            "{}  |  Patient #{}  |  Generated {}",
            doc.practice, doc.pat_num, doc.generated_on
        ),
        18.0,
    );
    writer.line("F2", 11.0, "Demographics", 18.0);
    for (label, value) in [
        ("Birth date", doc.birth_date.map(|d| d.to_string()).unwrap_or_default()),
        ("Gender", doc.gender.clone()),
        ("Home phone", doc.home_phone.clone()),
        ("Mobile phone", doc.mobile_phone.clone()),
        ("Email", doc.email.clone()),
        ("Chart number", doc.chart_number.clone()),
        ("Address", format!("{} {}, {} {}", doc.address, doc.city, doc.state, doc.zip)),
    ] {
        let value: String = value.chars().take(70).collect();
        writer.line("F1", 9.0, &format!("{label}: {value}"), 14.0);
    }

    writer.y -= 4.0;
    writer.line("F2", 11.0, "Medical history", 18.0);
    if doc.problems.is_empty() {
        writer.line("F1", 9.0, "No problems recorded.", 14.0);
    }
    for problem in &doc.problems {
        let line = format!(
            "{} - {} ({}{}) {}",
            problem.name.chars().take(40).collect::<String>(),
            problem.status,
            problem.date_start,
            problem
                .date_stop
                .map(|d| format!(" to {d}"))
                .unwrap_or_default(),
            problem.note.chars().take(30).collect::<String>()
        );
        writer.line("F1", 9.0, &line.chars().take(100).collect::<String>(), 14.0);
    }

    writer.y -= 4.0;
    writer.line("F2", 11.0, "Notes", 18.0);
    for (label, value) in [
        ("Medical", doc.medical.as_str()),
        ("Treatment", doc.treatment.as_str()),
        ("Service", doc.service.as_str()),
    ] {
        for (index, text) in value.lines().take(4).enumerate() {
            let prefix = if index == 0 { label } else { "" };
            writer.line(
                "F1",
                9.0,
                &format!("{prefix}: {}", text.chars().take(90).collect::<String>()),
                13.0,
            );
        }
    }

    writer.y -= 8.0;
    writer.line("F1", 9.0, "Patient / guardian: ____________________  Date: __________", 16.0);
    assemble_pdf(&writer.finish(), doc.pat_num, page, "PatientForm")
}

/// Render a refund/transfer receipt as A4 PDF bytes.
pub fn render_refund_transfer_receipt_pdf(doc: &RefundTransferReceipt) -> Vec<u8> {
    let title = format!("Refund/Transfer Receipt - {}", doc.patient_name);
    let mut writer = Writer::new(&title);
    let page = writer.page;
    writer.line(
        "F1",
        9.0,
        &format!(
            "Patient #{}  |  Payment #{}  |  Date {}  |  Generated {}",
            doc.pat_num, doc.payment_num, doc.pay_date, doc.generated_on
        ),
        16.0,
    );
    if !doc.recorded_by.trim().is_empty() {
        writer.line("F1", 9.0, &format!("Recorded by: {}", doc.recorded_by), 14.0);
    }
    if !doc.note.trim().is_empty() {
        writer.line("F2", 10.0, "Reason / note", 14.0);
        for note_line in doc.note.lines().take(4) {
            let note_line: String = note_line.chars().take(90).collect();
            writer.line("F1", 9.0, &note_line, 12.0);
        }
    }
    writer.line("F2", 11.0, "Details", 18.0);
    writer.columns(
        "F2",
        8.5,
        &[
            (page.left, "Date".into()),
            (85.0, "Direction".into()),
            (310.0, "Amount".into()),
            (360.0, "Pay#".into()),
            (405.0, "Plan".into()),
            (450.0, "Charge".into()),
        ],
        14.0,
    );
    for line in &doc.lines {
        let direction: String = line.direction.chars().take(42).collect();
        writer.columns(
            "F1",
            8.5,
            &[
                (page.left, line.date.to_string()),
                (85.0, direction),
                (right_x(&money(line.amount), 8.5, 345.0), money(line.amount)),
                (360.0, if line.payment_num == 0 { String::new() } else { line.payment_num.to_string() }),
                (405.0, if line.plan_num == 0 { String::new() } else { line.plan_num.to_string() }),
                (450.0, if line.charge_num == 0 { String::new() } else { line.charge_num.to_string() }),
            ],
            12.0,
        );
    }
    writer.y -= 6.0;
    writer.line(
        "F2",
        11.0,
        &format!("Total moved/refunded: {:.2}", doc.total_moved),
        18.0,
    );
    writer.line("F1", 8.0, "odclinic - refund/transfer receipt", 12.0);
    assemble_pdf(&writer.finish(), doc.pat_num, A4, "RefundTransferReceipt")
}

/// Render a simplified patient invoice on A5 paper, ready to write to disk.
///
/// Required fields follow the clinic billing SOP: patient, date, items with
/// tooth/site, unit price, discount and tax, plus total, amount paid and
/// balance. Two compact rows per item keep all of them legible on A5.
pub fn render_invoice_pdf(doc: &InvoiceDocument) -> Vec<u8> {
    let title = format!("Invoice - {}", doc.patient_name);
    let mut writer = Writer::with_page(&title, A5);
    writer.line("F1", 8.0, &format!("Patient #{}", doc.pat_num), 13.0);
    writer.line(
        "F1",
        8.0,
        &format!("Date {}", doc.invoice_date),
        13.0,
    );
    writer.line("F2", 10.0, "Charge items", 16.0);

    if doc.lines.is_empty() {
        writer.line("F1", 9.0, "No completed procedures to invoice.", 14.0);
    }

    for (index, line) in doc.lines.iter().enumerate() {
        let description: String = line.description.chars().take(34).collect();
        writer.line("F2", 9.0, &format!("{}. {}", index + 1, description), 12.0);
        let tooth: String = line.tooth_site.chars().take(16).collect();
        writer.line(
            "F1",
            8.0,
            &format!("Tooth/site: {}", tooth),
            11.0,
        );
        writer.line(
            "F1",
            8.0,
            &format!(
                "Qty {:.0} x {:.2}   Disc {:.2}   Tax {:.1}% {:.2}   Amount {:.2}",
                line.quantity,
                line.unit_price,
                line.discount,
                line.tax_rate,
                line.tax(),
                line.total()
            ),
            14.0,
        );
    }

    writer.y -= 4.0;
    writer.line(
        "F1",
        9.0,
        &format!(
            "Subtotal {:.2}  |  Discount {:.2}  |  Tax {:.2}",
            doc.subtotal(),
            doc.discount_total(),
            doc.tax_total()
        ),
        15.0,
    );
    writer.line(
        "F1",
        9.0,
        &format!("Amount paid {:.2}", doc.amount_paid),
        14.0,
    );
    writer.line(
        "F2",
        11.0,
        &format!("Balance due: {:.2}", doc.balance()),
        20.0,
    );

    if !doc.note.trim().is_empty() {
        writer.y -= 2.0;
        writer.line("F2", 9.0, "Note", 13.0);
        for note_line in doc.note.lines().take(5) {
            let note_line: String = note_line.chars().take(58).collect();
            writer.line("F1", 8.0, &note_line, 11.0);
        }
    }

    writer.y -= 4.0;
    writer.line("F1", 7.0, "odclinic - A5 invoice", 10.0);
    assemble_pdf(&writer.finish(), doc.pat_num, A5, "Invoice")
}

/// Build the final PDF objects, xref table and trailer from page streams.
fn assemble_pdf(pages: &[String], pat_num: Id, page: PageSpec, title_kind: &str) -> Vec<u8> {
    let mut objects: Vec<String> = Vec::with_capacity(4 + pages.len() * 2);

    let kids = (0..pages.len())
        .map(|i| format!("{} 0 R", 5 + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_string());
    objects.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids,
        pages.len()
    ));
    objects.push(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_string(),
    );
    objects.push(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
            .to_string(),
    );
    for (index, content) in pages.iter().enumerate() {
        let contents_obj = 6 + index * 2;
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>",
            page.width, page.height, contents_obj
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{}endstream",
            content.len(),
            content
        ));
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets: Vec<usize> = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
    }

    let xref_offset = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{:010} 00000 n \n", offset).as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info << /Title ({} {}) /Producer (odclinic) >> >>\nstartxref\n{}\n%%EOF\n",
            objects.len() + 1,
            title_kind,
            pat_num,
            xref_offset
        )
        .as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn statement(line_count: usize) -> StatementDocument {
        StatementDocument {
            pat_num: 5,
            patient_name: "Ada O'Brien".into(),
            generated_on: NaiveDate::from_ymd_opt(2026, 11, 21).unwrap(),
            lines: (0..line_count)
                .map(|i| crate::StatementLine {
                    date: NaiveDate::from_ymd_opt(2026, 11, 21).unwrap(),
                    kind: "Procedure".into(),
                    description: format!("Code #{i}"),
                    charge: 10.0,
                    credit: 0.0,
                })
                .collect(),
            charges: 100.0,
            adjustments: -5.0,
            payments: 20.0,
            balance: 75.0,
            plan_balances: vec![("Plan #1".into(), 40.0)],
            note: "Please pay (at reception)".into(),
        }
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|window| window == needle)
    }

    /// Verify the structural pieces a PDF reader needs: header, xref offset,
    /// object offsets, page count and EOF marker. Offsets are byte positions,
    /// so this works on the raw bytes rather than a UTF-8 decoded string.
    fn assert_valid_pdf(bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(bytes.ends_with(b"%%EOF\n"));
        assert!(text.contains("/Type /Catalog /Pages 2 0 R"));
        assert!(text.contains("/BaseFont /Helvetica"));
        assert!(text.contains("/F2 16.0 Tf"));

        let marker = b"startxref\n";
        let pos = find_bytes(bytes, marker).unwrap() + marker.len();
        let line_end = pos + bytes[pos..].iter().position(|&b| b == b'\n').unwrap();
        let offset: usize = std::str::from_utf8(&bytes[pos..line_end])
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(&bytes[offset..offset + 4], b"xref");

        // The first xref entry must point at the first object header.
        let free = b"0000000000 65535 f \n";
        let free_pos = find_bytes(bytes, free).unwrap() + free.len();
        let entry_end = free_pos
            + bytes[free_pos..].iter().position(|&b| b == b'\n').unwrap();
        let first_offset: usize = std::str::from_utf8(&bytes[free_pos..entry_end])
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(first_offset, find_bytes(bytes, b"1 0 obj\n").unwrap());
    }

    #[test]
    fn statement_pdf_is_structurally_valid() {
        let bytes = render_statement_pdf(&statement(3));
        assert_valid_pdf(&bytes);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("Statement - Ada O'Brien"));
        assert!(text.contains("Balance due: 75.00"));
        // Parentheses are escaped in PDF literal strings.
        assert!(text.contains("Please pay \\(at reception\\)"));
    }

    #[test]
    fn patient_form_pdf_is_structurally_valid() {
        use chrono::NaiveDate;
        use od_domain::{Gender, ProblemStatus};
        let doc = PatientFormDocument {
            pat_num: 7,
            patient_name: "Ada Lovelace".into(),
            generated_on: NaiveDate::from_ymd_opt(2026, 11, 26).unwrap(),
            practice: "Highland Dental".into(),
            birth_date: None,
            gender: Gender::Female.label().into(),
            home_phone: "+251 11 000 000".into(),
            mobile_phone: String::new(),
            email: String::new(),
            address: String::new(),
            city: String::new(),
            state: String::new(),
            zip: String::new(),
            chart_number: String::new(),
            medical: "No known allergies".into(),
            treatment: String::new(),
            service: String::new(),
            problems: vec![crate::PatientFormProblem {
                name: "Caries".into(),
                status: ProblemStatus::Active.label().into(),
                date_start: NaiveDate::from_ymd_opt(2024, 5, 1).unwrap(),
                date_stop: None,
                note: String::new(),
            }],
        };
        let bytes = render_patient_form_pdf(&doc);
        assert_valid_pdf(&bytes);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("Patient form - Ada Lovelace"));
        assert!(text.contains("Patient #7"));
        assert!(text.contains("Highland Dental"));
        assert!(text.contains("No known allergies"));
    }

    #[test]
    fn statement_pdf_flows_onto_more_than_one_page() {
        let bytes = render_statement_pdf(&statement(120));
        assert_valid_pdf(&bytes);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Count 3"));
        assert!(text.contains("\\(continued\\)"));
    }

    fn invoice(line_count: usize) -> InvoiceDocument {
        InvoiceDocument {
            pat_num: 7,
            patient_name: "Ada O'Brien".into(),
            invoice_date: NaiveDate::from_ymd_opt(2026, 11, 21).unwrap(),
            lines: (0..line_count)
                .map(|i| od_domain::InvoiceLine {
                    description: format!("Composite filling #{i}"),
                    tooth_site: "16".into(),
                    quantity: 1.0,
                    unit_price: 50.0,
                    discount: 5.0,
                    tax_rate: 10.0,
                })
                .collect(),
            amount_paid: 20.0,
            note: "Paid at reception".into(),
        }
    }

    #[test]
    fn invoice_pdf_is_a5_and_structurally_valid() {
        let doc = invoice(2);
        let bytes = render_invoice_pdf(&doc);
        assert_valid_pdf(&bytes);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/MediaBox [0 0 420 595]"), "A5 page size");
        assert!(text.contains("Invoice - Ada O'Brien"));
        assert!(text.contains("Tooth/site: 16"));
        assert!(text.contains("Discount 10.00"));
        assert!(text.contains("Tax 9.00"));
        assert!(text.contains(&format!("Balance due: {:.2}", doc.balance())));
        assert!(text.contains("/Title (Invoice 7)"));
    }

    #[test]
    fn invoice_pdf_flows_across_a5_pages() {
        let bytes = render_invoice_pdf(&invoice(40));
        assert_valid_pdf(&bytes);
        let text = String::from_utf8_lossy(&bytes);
        let marker = "/Count ";
        let start = text.find(marker).unwrap() + marker.len();
        let end = start + text[start..].find(' ').unwrap();
        let count: usize = text[start..end].parse().unwrap();
        assert!(count >= 2, "long invoices need more than one A5 page");
        assert!(text.contains("\\(continued\\)"));
    }

    #[test]
    fn refund_transfer_receipt_pdf_is_valid() {
        let doc = crate::RefundTransferReceipt {
            pat_num: 1,
            patient_name: "Ada".into(),
            payment_num: 77,
            pay_date: NaiveDate::from_ymd_opt(2026, 1, 5).unwrap(),
            generated_on: NaiveDate::from_ymd_opt(2026, 1, 6).unwrap(),
            note: "Moved credit".into(),
            recorded_by: "admin".into(),
            total_moved: 25.0,
            lines: vec![crate::RefundTransferReceiptLine {
                date: NaiveDate::from_ymd_opt(2026, 1, 5).unwrap(),
                direction: "Plan #9 -> Patient account (unallocated)".into(),
                source: "Plan #9".into(),
                target: "Patient account (unallocated)".into(),
                amount: 25.0,
                payment_num: 77,
                plan_num: 9,
                charge_num: 0,
                note: "Moved credit".into(),
            }],
        };
        let bytes = render_refund_transfer_receipt_pdf(&doc);
        assert_valid_pdf(&bytes);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("Refund/Transfer Receipt - Ada"));
        assert!(text.contains("Total moved/refunded: 25.00"));
        assert!(text.contains("/Title (RefundTransferReceipt 1)"));
    }
}
