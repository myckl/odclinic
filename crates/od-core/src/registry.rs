//! Pure logic for the integrated patient registry.
//!
//! The registry is a patient-centred view that folds in contact and payment
//! details, the treatments and conditions already on the clinical record,
//! clinic-card issuing and simplified A5 invoices. Keeping the rules here makes
//! them testable without a GUI or a database.

use chrono::NaiveDate;
use od_domain::{Id, InvoiceDocument, InvoiceLine, PaySplit, Procedure, ProcedureStatus};

/// Build an A5 invoice from the patient's completed procedures and the
/// payments credited to them.
///
/// Open Dental credits payments through `paysplit.SplitAmt`, not the payment
/// header's total (a single cheque can cover several patients), so the amount
/// paid is the sum of this patient's splits. One line per completed procedure
/// keeps the invoice tied to the Account balance.
pub fn build_invoice_document(
    pat_num: Id,
    patient_name: &str,
    procedures: &[Procedure],
    splits: &[PaySplit],
    invoice_date: NaiveDate,
    note: &str,
) -> InvoiceDocument {
    let lines = procedures
        .iter()
        .filter(|proc| proc.pat_num == pat_num && proc.proc_status == ProcedureStatus::Complete)
        .map(|proc| {
            let description = if proc.description_override.trim().is_empty() {
                format!("Procedure code #{}", proc.code_num)
            } else {
                proc.description_override.clone()
            };
            let tooth_site = if proc.tooth_num.trim().is_empty() {
                "-".to_string()
            } else {
                proc.tooth_num.clone()
            };
            InvoiceLine {
                description,
                tooth_site,
                quantity: 1.0,
                unit_price: proc.fee,
                discount: 0.0,
                tax_rate: 0.0,
            }
        })
        .collect();

    let amount_paid = splits
        .iter()
        .filter(|split| split.pat_num == pat_num)
        .map(|split| split.split_amount)
        .sum();

    InvoiceDocument {
        pat_num,
        patient_name: patient_name.to_string(),
        invoice_date,
        lines,
        amount_paid,
        note: note.to_string(),
    }
}

/// Guard against the original system's problem: a card being issued for a
/// patient who does not need one, which created unwanted financial detail.
///
/// A card is only allowed with a billing authorisation code and an explicit
/// acknowledgement that it is required for billing. Issuing a card never
/// writes a statement, adjustment or payment (enforced by the store API).
pub fn validate_clinic_card_issue(
    authorization: &str,
    acknowledgement: bool,
) -> Result<(), String> {
    if authorization.trim().chars().count() < 3 {
        return Err("Enter a billing authorisation code (at least 3 characters).".into());
    }
    if !acknowledgement {
        return Err("Confirm that this patient needs a card for billing.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_domain::ProcedureStatus;

    fn proc(fee: f64, status: ProcedureStatus) -> Procedure {
        Procedure {
            proc_num: 1,
            pat_num: 7,
            code_num: 111,
            proc_date: NaiveDate::from_ymd_opt(2026, 11, 21).unwrap(),
            proc_status: status,
            tooth_num: "16".into(),
            fee,
            ..Default::default()
        }
    }

    fn split(amount: f64, pat_num: Id) -> PaySplit {
        PaySplit {
            pat_num,
            split_amount: amount,
            ..Default::default()
        }
    }

    #[test]
    fn invoice_uses_completed_procedures_and_patient_splits() {
        let doc = build_invoice_document(
            7,
            "Ada O'Brien",
            &[
                proc(100.0, ProcedureStatus::Complete),
                proc(40.0, ProcedureStatus::TreatmentPlanned),
            ],
            &[split(25.0, 7), split(999.0, 8)],
            NaiveDate::from_ymd_opt(2026, 11, 21).unwrap(),
            "Paid at reception",
        );
        assert_eq!(doc.lines.len(), 1, "planned procedures are not charged");
        assert_eq!(doc.lines[0].tooth_site, "16");
        assert_eq!(doc.total(), 100.0);
        assert_eq!(doc.amount_paid, 25.0);
        assert_eq!(doc.balance(), 75.0);
    }

    #[test]
    fn invoice_totals_include_line_discount_and_tax() {
        let doc = InvoiceDocument {
            lines: vec![
                InvoiceLine {
                    quantity: 2.0,
                    unit_price: 50.0,
                    discount: 10.0,
                    tax_rate: 10.0,
                    ..Default::default()
                },
                InvoiceLine {
                    unit_price: 20.0,
                    ..Default::default()
                },
            ],
            amount_paid: 30.0,
            ..Default::default()
        };
        assert_eq!(doc.discount_total(), 10.0);
        assert_eq!(doc.tax_total(), 9.0);
        assert_eq!(doc.total(), 119.0);
        assert_eq!(doc.balance(), 89.0);
    }

    #[test]
    fn clinic_card_requires_authorisation_and_acknowledgement() {
        assert!(validate_clinic_card_issue("CORP-42", true).is_ok());
        assert!(validate_clinic_card_issue("  ", true).is_err());
        assert!(validate_clinic_card_issue("CORP-42", false).is_err());
    }
}
