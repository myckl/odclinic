//! Posting patient charges, payments and adjustments into OD's double-entry
//! `journalentry` table.
//!
//! The accounting reports (Balance Sheet, General Ledger, Profit & Loss) read
//! only `journalentry`. Before this module the port could record these events
//! on a patient account but had no write path to the journal, so those reports
//! stayed read-only. This module builds the balanced lines for each posted
//! event; `Store::post_journal_transaction` and `Store::post_journal_batch`
//! write them atomically.

use od_domain::{Account, AccountType, Adjustment, Id, JournalEntry, Payment, Procedure};

/// Stable token included in every posted journal memo. It lets the account
/// ledger detect an already-posted adjustment and refuse a duplicate posting,
/// even when staff customise the human-readable part of the memo.
pub fn adjustment_journal_reference(adj_num: Id, pat_num: Id) -> String {
    format!("[Adj #{adj_num} pat #{pat_num}]")
}

/// Memo stored on both journal lines of a posted adjustment. The reference
/// token ties the line back to its source adjustment; the note makes it
/// readable in the General Ledger without opening the account.
pub fn adjustment_journal_memo(adj: &Adjustment) -> String {
    adjustment_journal_memo_with_note(adj, &adj.note)
}

/// Build a posted memo from a free-form note while always keeping the stable
/// reference token. Staff can edit the readable part without breaking
/// duplicate detection.
pub fn adjustment_journal_memo_with_note(adj: &Adjustment, note: &str) -> String {
    let reference = adjustment_journal_reference(adj.adj_num, adj.pat_num);
    let trimmed = note.trim();
    if trimmed.is_empty() {
        reference
    } else {
        format!("{reference}: {trimmed}")
    }
}

/// The memo format written before the stable reference token was introduced.
/// Recognising it avoids a second posting of an adjustment booked by an older
/// build of this port.
fn legacy_adjustment_memo(adj_num: Id, pat_num: Id) -> String {
    format!("Adjustment #{adj_num} patient #{pat_num}")
}

fn matches_adjustment(entry: &JournalEntry, adj_num: Id, pat_num: Id) -> bool {
    entry.memo.contains(&adjustment_journal_reference(adj_num, pat_num))
        || entry.memo.contains(&legacy_adjustment_memo(adj_num, pat_num))
}

/// Whether any journal line already carries this adjustment's reference.
pub fn is_adjustment_posted(journal: &[JournalEntry], adj_num: Id, pat_num: Id) -> bool {
    journal.iter().any(|e| matches_adjustment(e, adj_num, pat_num))
}

/// Transaction number of the first posting for this adjustment, for display in
/// the ledger's hover text.
pub fn journal_transaction_for_adjustment(
    journal: &[JournalEntry],
    adj_num: Id,
    pat_num: Id,
) -> Option<Id> {
    journal
        .iter()
        .find(|e| matches_adjustment(e, adj_num, pat_num))
        .map(|e| e.transaction_num)
}

/// Stable token included in every journal line posted from a patient payment.
pub fn payment_journal_reference(pay_num: Id, pat_num: Id) -> String {
    format!("[Pay #{pay_num} pat #{pat_num}]")
}

/// Memo stored on both journal lines of a posted payment.
pub fn payment_journal_memo(payment: &Payment) -> String {
    payment_journal_memo_with_note(payment, &payment.note)
}

/// Build a payment memo from a free-form note while always keeping the stable
/// reference token, so custom notes cannot break duplicate detection.
pub fn payment_journal_memo_with_note(payment: &Payment, note: &str) -> String {
    let reference = payment_journal_reference(payment.pay_num, payment.pat_num);
    let trimmed = note.trim();
    if trimmed.is_empty() {
        reference
    } else {
        format!("{reference}: {trimmed}")
    }
}

fn legacy_payment_memo(pay_num: Id, pat_num: Id) -> String {
    format!("Payment #{pay_num} patient #{pat_num}")
}

fn payment_matches(entry: &JournalEntry, pay_num: Id, pat_num: Id) -> bool {
    entry.memo.contains(&payment_journal_reference(pay_num, pat_num))
        || entry.memo.contains(&legacy_payment_memo(pay_num, pat_num))
}

/// Whether any journal line already carries this payment's reference.
pub fn is_payment_posted(journal: &[JournalEntry], pay_num: Id, pat_num: Id) -> bool {
    journal.iter().any(|e| payment_matches(e, pay_num, pat_num))
}

/// Transaction number of the first posting for this payment, for display in
/// the account ledger's hover text.
pub fn journal_transaction_for_payment(
    journal: &[JournalEntry],
    pay_num: Id,
    pat_num: Id,
) -> Option<Id> {
    journal
        .iter()
        .find(|e| payment_matches(e, pay_num, pat_num))
        .map(|e| e.transaction_num)
}

fn first_active(accounts: &[Account], account_type: AccountType) -> Option<Id> {
    accounts
        .iter()
        .find(|a| a.account_type == account_type && !a.inactive)
        .map(|a| a.account_num)
}

/// Prefer the account explicitly labelled "Receivable"; a chart of accounts
/// can hold several Asset accounts (cash, supplies, etc.) and a patient
/// adjustment belongs against receivables, not petty cash.
fn first_receivable(accounts: &[Account]) -> Option<Id> {
    accounts
        .iter()
        .find(|a| {
            a.account_type == AccountType::Asset
                && !a.inactive
                && a.description.to_lowercase().contains("receivable")
        })
        .or_else(|| {
            accounts
                .iter()
                .find(|a| a.account_type == AccountType::Asset && !a.inactive)
        })
        .map(|a| a.account_num)
}

fn two_distinct(accounts: &[Account]) -> Option<(Id, Id)> {
    let mut nums = accounts.iter().filter(|a| !a.inactive).map(|a| a.account_num);
    let debit = nums.next()?;
    let credit = nums.find(|n| *n != debit)?;
    Some((debit, credit))
}

/// Suggest the `(debit account, credit account)` pair for an adjustment from
/// its sign and the chart of accounts. A positive adjustment raises the patient
/// balance, so it debits Accounts Receivable and credits Income. A negative
/// adjustment (discount/write-off) debits Expense and credits Accounts
/// Receivable. When the preferred account type is missing, the first two active
/// accounts are used so staff still get a usable default they can override.
pub fn suggest_ledger_accounts(accounts: &[Account], adj_amount: f64) -> Option<(Id, Id)> {
    let receivable = first_receivable(accounts);
    let preferred: Option<(Id, Id)> = if adj_amount >= 0.0 {
        match (receivable, first_active(accounts, AccountType::Income)) {
            (Some(debit), Some(credit)) => Some((debit, credit)),
            _ => None,
        }
    } else {
        match (first_active(accounts, AccountType::Expense), receivable) {
            (Some(debit), Some(credit)) => Some((debit, credit)),
            _ => None,
        }
    };
    preferred
        .filter(|(debit, credit)| debit != credit)
        .or_else(|| two_distinct(accounts))
}

/// Build the two balanced `journalentry` lines for one adjustment.
///
/// `transaction_num` groups the pair so the General Ledger shows one event.
/// The amount is always positive on each side; OD keeps direction in the
/// debit/credit columns and the account type decides which side increases the
/// account (see `AccountType::debit_is_pos`).
pub fn build_adjustment_journal(
    adj: &Adjustment,
    debit_account: Id,
    credit_account: Id,
    transaction_num: Id,
) -> Result<[JournalEntry; 2], String> {
    if !adj.adj_amount.is_finite() || adj.adj_amount.abs() <= 0.005 {
        return Err("Adjustment amount must be non-zero to post to the ledger".into());
    }
    if debit_account == 0 || credit_account == 0 {
        return Err("Select both a debit and a credit account".into());
    }
    if debit_account == credit_account {
        return Err("Debit and credit accounts must differ".into());
    }
    if transaction_num == 0 {
        return Err("Transaction number must be positive".into());
    }
    let amount = (adj.adj_amount.abs() * 100.0).round() / 100.0;
    let memo = adjustment_journal_memo(adj);
    Ok([
        JournalEntry {
            account_num: debit_account,
            date_displayed: adj.adj_date,
            debit_amt: amount,
            credit_amt: 0.0,
            memo: memo.clone(),
            transaction_num,
            ..Default::default()
        },
        JournalEntry {
            account_num: credit_account,
            date_displayed: adj.adj_date,
            debit_amt: 0.0,
            credit_amt: amount,
            memo,
            transaction_num,
            ..Default::default()
        },
    ])
}

/// Prefer a cash/bank account for the money side of a patient payment. OD
/// charts often hold several Asset accounts (A/R, supplies, deposits), so a
/// plain "first Asset" default would book collections against receivables.
fn first_cash(accounts: &[Account]) -> Option<Id> {
    accounts
        .iter()
        .find(|a| {
            a.account_type == AccountType::Asset
                && !a.inactive
                && {
                    let description = a.description.to_lowercase();
                    description.contains("cash")
                        || description.contains("bank")
                        || description.contains("petty")
                }
        })
        .map(|a| a.account_num)
        .or_else(|| first_active(accounts, AccountType::Asset))
}

/// Suggest `(debit account, credit account)` for a patient payment. A normal
/// payment debits cash and credits A/R; a negative payment (refund) reverses
/// that direction. Falls back to the first two active accounts so staff still
/// get an editable default on a minimal chart of accounts.
pub fn suggest_payment_ledger_accounts(accounts: &[Account], pay_amount: f64) -> Option<(Id, Id)> {
    let receivable = first_receivable(accounts);
    let cash = first_cash(accounts).filter(|account| Some(*account) != receivable);
    let preferred: Option<(Id, Id)> = if pay_amount >= 0.0 {
        match (cash, receivable) {
            (Some(debit), Some(credit)) => Some((debit, credit)),
            _ => None,
        }
    } else {
        match (receivable, cash) {
            (Some(debit), Some(credit)) => Some((debit, credit)),
            _ => None,
        }
    };
    preferred
        .filter(|(debit, credit)| debit != credit)
        .or_else(|| two_distinct(accounts))
}

/// Build the two balanced `journalentry` lines for one patient payment. The
/// payment's cheque number is carried onto both lines for bank reconciliation,
/// and the date comes from the payment itself so posted journals follow the
/// collection date rather than the posting date.
pub fn build_payment_journal(
    payment: &Payment,
    debit_account: Id,
    credit_account: Id,
    transaction_num: Id,
) -> Result<[JournalEntry; 2], String> {
    if !payment.pay_amount.is_finite() || payment.pay_amount.abs() <= 0.005 {
        return Err("Payment amount must be non-zero to post to the ledger".into());
    }
    if debit_account == 0 || credit_account == 0 {
        return Err("Select both a debit and a credit account".into());
    }
    if debit_account == credit_account {
        return Err("Debit and credit accounts must differ".into());
    }
    if transaction_num == 0 {
        return Err("Transaction number must be positive".into());
    }
    let amount = (payment.pay_amount.abs() * 100.0).round() / 100.0;
    let memo = payment_journal_memo(payment);
    Ok([
        JournalEntry {
            account_num: debit_account,
            date_displayed: payment.pay_date,
            debit_amt: amount,
            credit_amt: 0.0,
            memo: memo.clone(),
            check_number: payment.check_num.clone(),
            transaction_num,
            ..Default::default()
        },
        JournalEntry {
            account_num: credit_account,
            date_displayed: payment.pay_date,
            debit_amt: 0.0,
            credit_amt: amount,
            memo,
            check_number: payment.check_num.clone(),
            transaction_num,
            ..Default::default()
        },
    ])
}

/// Stable token included in every journal line posted from a completed
/// procedure's charge.
pub fn procedure_journal_reference(proc_num: Id, pat_num: Id) -> String {
    format!("[Proc #{proc_num} pat #{pat_num}]")
}

fn procedure_description(proc: &Procedure) -> String {
    let code = if proc.description_override.trim().is_empty() {
        format!("Code #{}", proc.code_num)
    } else {
        proc.description_override.trim().to_string()
    };
    if proc.tooth_num.trim().is_empty() {
        code
    } else {
        format!("{code} · tooth {}", proc.tooth_num.trim())
    }
}

/// Memo stored on both journal lines of a posted charge. The reference token
/// ties the line back to its procedure; the description makes it readable in
/// the General Ledger without opening the chart.
pub fn procedure_journal_memo(proc: &Procedure) -> String {
    procedure_journal_memo_with_note(proc, &procedure_description(proc))
}

/// Build a charge memo from a free-form note while always keeping the stable
/// reference token, so custom notes cannot break duplicate detection.
pub fn procedure_journal_memo_with_note(proc: &Procedure, note: &str) -> String {
    let reference = procedure_journal_reference(proc.proc_num, proc.pat_num);
    let trimmed = note.trim();
    if trimmed.is_empty() {
        reference
    } else {
        format!("{reference}: {trimmed}")
    }
}

fn legacy_procedure_memo(proc_num: Id, pat_num: Id) -> String {
    format!("Procedure #{proc_num} patient #{pat_num}")
}

fn procedure_matches(entry: &JournalEntry, proc_num: Id, pat_num: Id) -> bool {
    entry.memo.contains(&procedure_journal_reference(proc_num, pat_num))
        || entry.memo.contains(&legacy_procedure_memo(proc_num, pat_num))
}

/// Whether any journal line already carries this procedure's reference.
pub fn is_procedure_posted(journal: &[JournalEntry], proc_num: Id, pat_num: Id) -> bool {
    journal.iter().any(|e| procedure_matches(e, proc_num, pat_num))
}

/// Transaction number of the first posting for this procedure, for display in
/// the ledger's hover text.
pub fn journal_transaction_for_procedure(
    journal: &[JournalEntry],
    proc_num: Id,
    pat_num: Id,
) -> Option<Id> {
    journal
        .iter()
        .find(|e| procedure_matches(e, proc_num, pat_num))
        .map(|e| e.transaction_num)
}

/// Completed-procedure charges debit A/R and credit income — the same
/// direction as a positive adjustment. Exposed under its own name so batch
/// posting reads clearly at the call site.
pub fn suggest_charge_ledger_accounts(accounts: &[Account]) -> Option<(Id, Id)> {
    suggest_ledger_accounts(accounts, 1.0)
}

/// Build the two balanced `journalentry` lines for one completed procedure's
/// charge. The amount is the procedure fee — the same figure the production
/// reports use — and the date is the procedure date, so posted charges follow
/// clinical production rather than the posting run date.
pub fn build_charge_journal(
    proc: &Procedure,
    debit_account: Id,
    credit_account: Id,
    transaction_num: Id,
) -> Result<[JournalEntry; 2], String> {
    if !proc.fee.is_finite() || proc.fee.abs() <= 0.005 {
        return Err("Procedure fee must be non-zero to post to the ledger".into());
    }
    if debit_account == 0 || credit_account == 0 {
        return Err("Select both a debit and a credit account".into());
    }
    if debit_account == credit_account {
        return Err("Debit and credit accounts must differ".into());
    }
    if transaction_num == 0 {
        return Err("Transaction number must be positive".into());
    }
    let amount = (proc.fee.abs() * 100.0).round() / 100.0;
    let memo = procedure_journal_memo(proc);
    Ok([
        JournalEntry {
            account_num: debit_account,
            date_displayed: proc.proc_date,
            debit_amt: amount,
            credit_amt: 0.0,
            memo: memo.clone(),
            transaction_num,
            ..Default::default()
        },
        JournalEntry {
            account_num: credit_account,
            date_displayed: proc.proc_date,
            debit_amt: 0.0,
            credit_amt: amount,
            memo,
            transaction_num,
            ..Default::default()
        },
    ])
}

/// One charge in a batch posting plan: the source procedure, the transaction
/// group assigned to it and the two journal lines ready to write.
#[derive(Clone)]
pub struct ChargePostingPlan {
    pub procedure: Procedure,
    pub transaction_num: Id,
    pub entries: [JournalEntry; 2],
}

/// Build a plan for every completed procedure that is not already in the
/// journal. Transaction numbers continue after the largest one already stored,
/// so each charge posts as its own event in the General Ledger. Procedures with
/// no fee (or a non-finite fee) are skipped: there is nothing to book.
pub fn build_charge_posting_plan(
    procedures: &[Procedure],
    journal: &[JournalEntry],
    debit_account: Id,
    credit_account: Id,
) -> Vec<ChargePostingPlan> {
    let mut next = next_journal_transaction_num(journal);
    let mut plan = Vec::new();
    if debit_account == 0 || credit_account == 0 || debit_account == credit_account {
        return plan;
    }
    for proc in procedures {
        if !proc.proc_status.is_completed()
            || is_procedure_posted(journal, proc.proc_num, proc.pat_num)
        {
            continue;
        }
        if !proc.fee.is_finite() || proc.fee.abs() <= 0.005 {
            continue;
        }
        if let Ok(entries) = build_charge_journal(proc, debit_account, credit_account, next) {
            plan.push(ChargePostingPlan {
                procedure: proc.clone(),
                transaction_num: next,
                entries,
            });
            next += 1;
        }
    }
    plan
}

/// One more than the largest `TransactionNum` already stored, so a new posting
/// never reuses an existing transaction group.
pub fn next_journal_transaction_num(journal: &[JournalEntry]) -> Id {
    journal.iter().map(|e| e.transaction_num).max().unwrap_or(0) + 1
}

/// Guard used before writing: all lines share one transaction and date, and
/// debit/credit totals match to the cent.
pub fn validate_balanced_journal(entries: &[JournalEntry]) -> Result<(), String> {
    let first = entries.first().ok_or("No journal lines to post")?;
    if entries
        .iter()
        .any(|e| e.transaction_num != first.transaction_num)
    {
        return Err("All journal lines must share one transaction number".into());
    }
    if entries.iter().any(|e| e.date_displayed != first.date_displayed) {
        return Err("All journal lines must share one date".into());
    }
    let debit: f64 = entries.iter().map(|e| e.debit_amt).sum();
    let credit: f64 = entries.iter().map(|e| e.credit_amt).sum();
    if (debit - credit).abs() > 0.005 {
        return Err(format!(
            "Journal is not balanced: debit {debit:.2}, credit {credit:.2}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use od_domain::AccountType;

    fn accounts() -> Vec<Account> {
        vec![
            Account {
                account_num: 1,
                description: "Petty Cash".into(),
                account_type: AccountType::Asset,
                ..Default::default()
            },
            Account {
                account_num: 2,
                description: "Accounts Receivable".into(),
                account_type: AccountType::Asset,
                ..Default::default()
            },
            Account {
                account_num: 5,
                description: "Patient Income".into(),
                account_type: AccountType::Income,
                ..Default::default()
            },
            Account {
                account_num: 6,
                description: "Discounts".into(),
                account_type: AccountType::Expense,
                ..Default::default()
            },
        ]
    }

    fn adjustment(amount: f64) -> Adjustment {
        Adjustment {
            adj_num: 12,
            adj_date: NaiveDate::from_ymd_opt(2026, 11, 23).unwrap(),
            adj_amount: amount,
            pat_num: 7,
            note: "Courtesy discount".into(),
            ..Default::default()
        }
    }

    #[test]
    fn adjustment_pair_is_balanced_and_shares_one_transaction() {
        let entries = build_adjustment_journal(&adjustment(-25.0), 6, 2, 9).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].account_num, 6);
        assert_eq!(entries[0].debit_amt, 25.0);
        assert_eq!(entries[0].credit_amt, 0.0);
        assert_eq!(entries[1].account_num, 2);
        assert_eq!(entries[1].debit_amt, 0.0);
        assert_eq!(entries[1].credit_amt, 25.0);
        assert!(entries.iter().all(|e| e.transaction_num == 9));
        assert!(entries
            .iter()
            .all(|e| e.date_displayed == NaiveDate::from_ymd_opt(2026, 11, 23).unwrap()));
        assert!(entries
            .iter()
            .all(|e| e.memo.contains("[Adj #12 pat #7]")));
        assert!(validate_balanced_journal(&entries).is_ok());
    }

    #[test]
    fn posted_adjustment_is_detected_even_with_a_custom_memo() {
        let adj = adjustment(10.0);
        let mut entries = build_adjustment_journal(&adj, 2, 5, 3).unwrap();
        assert!(is_adjustment_posted(&entries, 12, 7));
        assert_eq!(journal_transaction_for_adjustment(&entries, 12, 7), Some(3));
        assert_eq!(adjustment_journal_memo(&adj), adjustment_journal_memo_with_note(&adj, &adj.note));
        for entry in &mut entries {
            entry.memo = format!(
                "{} posted by reception",
                adjustment_journal_reference(adj.adj_num, adj.pat_num)
            );
        }
        assert!(is_adjustment_posted(&entries, 12, 7));
        assert_eq!(journal_transaction_for_adjustment(&entries, 12, 7), Some(3));
        assert!(!is_adjustment_posted(&entries, 99, 7));

        // A custom note must never drop the reference token.
        let memo = adjustment_journal_memo_with_note(&adj, "  edited by reception  ");
        assert!(memo.contains(&adjustment_journal_reference(12, 7)));
        assert!(memo.ends_with("edited by reception"));

        // Pre-fix postings remain detectable so they cannot be booked twice.
        let legacy = [JournalEntry {
            memo: "Adjustment #12 patient #7: old note".into(),
            transaction_num: 8,
            ..Default::default()
        }];
        assert!(is_adjustment_posted(&legacy, 12, 7));
        assert_eq!(journal_transaction_for_adjustment(&legacy, 12, 7), Some(8));
    }

    #[test]
    fn positive_and_negative_amounts_post_the_same_absolute_value() {
        let positive = build_adjustment_journal(&adjustment(40.0), 2, 5, 1).unwrap();
        let negative = build_adjustment_journal(&adjustment(-40.0), 6, 2, 2).unwrap();
        assert_eq!(positive[0].debit_amt, 40.0);
        assert_eq!(negative[0].debit_amt, 40.0);
        assert_eq!(positive[1].credit_amt, negative[1].credit_amt);
    }

    #[test]
    fn account_suggestions_follow_the_adjustment_sign() {
        let accounts = accounts();
        assert_eq!(
            suggest_ledger_accounts(&accounts, 30.0),
            Some((2, 5)),
            "a charge debits A/R and credits income"
        );
        assert_eq!(
            suggest_ledger_accounts(&accounts, -30.0),
            Some((6, 2)),
            "a write-off debits discount expense and credits A/R"
        );
    }

    #[test]
    fn invalid_postings_are_rejected_before_any_write() {
        let adj = adjustment(10.0);
        assert!(build_adjustment_journal(&adj, 2, 2, 1).is_err());
        assert!(build_adjustment_journal(&adj, 0, 5, 1).is_err());
        assert!(build_adjustment_journal(&adj, 2, 5, 0).is_err());
        assert!(build_adjustment_journal(&adjustment(0.0), 2, 5, 1).is_err());
    }

    #[test]
    fn journal_validation_rejects_unbalanced_or_mixed_transactions() {
        let entries = build_adjustment_journal(&adjustment(10.0), 2, 5, 3).unwrap();
        assert!(validate_balanced_journal(&[]).is_err());
        let mut mixed = entries.clone();
        mixed[1].transaction_num = 4;
        assert!(validate_balanced_journal(&mixed).is_err());
        let mut lopsided = entries.clone();
        lopsided[1].credit_amt = 9.0;
        assert!(validate_balanced_journal(&lopsided).is_err());
    }

    #[test]
    fn transaction_number_never_reuses_an_existing_group() {
        let entries = build_adjustment_journal(&adjustment(10.0), 2, 5, 4).unwrap();
        assert_eq!(next_journal_transaction_num(&entries), 5);
        assert_eq!(next_journal_transaction_num(&[]), 1);
    }

    #[test]
    fn posted_pair_feeds_the_accounting_reports() {
        let accounts = accounts();
        let as_of = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        let before = crate::balance_sheet_report(&accounts, &[], as_of, false);
        let entries = build_adjustment_journal(&adjustment(-25.0), 6, 2, 9).unwrap();
        let after = crate::balance_sheet_report(&accounts, &entries, as_of, false);

        assert_ne!(
            before.rows, after.rows,
            "posting must change the previously read-only balance sheet"
        );
        let expense_total = after
            .rows
            .iter()
            .find(|r| r[0] == "Total Expenses")
            .expect("expense section");
        assert_eq!(expense_total[2], "25.00");
        let asset_total = after
            .rows
            .iter()
            .find(|r| r[0] == "Total Assets")
            .expect("asset section");
        assert_eq!(asset_total[2], "-25.00");

        let ledger = crate::general_ledger_report(
            &accounts,
            &entries,
            NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            as_of,
            false,
        );
        let flat = ledger.rows.concat().join("|");
        assert!(flat.contains("Discounts"));
        assert!(flat.contains("Accounts Receivable"));
        assert!(
            flat.contains("Courtesy discount"),
            "the adjustment note reaches the ledger memo"
        );
    }

    fn payment(amount: f64) -> Payment {
        Payment {
            pay_num: 31,
            pat_num: 7,
            pay_date: NaiveDate::from_ymd_opt(2026, 11, 24).unwrap(),
            pay_amount: amount,
            pay_type: 0,
            check_num: "CHQ-8842".into(),
            note: "Cash at desk".into(),
        }
    }

    #[test]
    fn payment_pair_is_balanced_and_carries_the_cheque_number() {
        let entries = build_payment_journal(&payment(30.0), 1, 2, 10).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].account_num, 1);
        assert_eq!(entries[0].debit_amt, 30.0);
        assert_eq!(entries[0].credit_amt, 0.0);
        assert_eq!(entries[1].account_num, 2);
        assert_eq!(entries[1].debit_amt, 0.0);
        assert_eq!(entries[1].credit_amt, 30.0);
        assert!(entries
            .iter()
            .all(|e| e.check_number == "CHQ-8842"
                && e.transaction_num == 10
                && e.date_displayed == NaiveDate::from_ymd_opt(2026, 11, 24).unwrap()));
        assert!(entries.iter().all(|e| e.memo.contains("[Pay #31 pat #7]")));
        assert!(validate_balanced_journal(&entries).is_ok());
    }

    #[test]
    fn payment_accounts_follow_the_collection_direction() {
        let accounts = accounts();
        assert_eq!(
            suggest_payment_ledger_accounts(&accounts, 30.0),
            Some((1, 2)),
            "a collection debits petty cash and credits A/R"
        );
        assert_eq!(
            suggest_payment_ledger_accounts(&accounts, -30.0),
            Some((2, 1)),
            "a refund debits A/R and credits cash"
        );
    }

    #[test]
    fn posted_payments_are_detected_with_custom_and_legacy_memos() {
        let pay = payment(20.0);
        let mut entries = build_payment_journal(&pay, 1, 2, 11).unwrap();
        assert!(is_payment_posted(&entries, 31, 7));
        assert_eq!(journal_transaction_for_payment(&entries, 31, 7), Some(11));
        for entry in &mut entries {
            entry.memo = payment_journal_memo_with_note(&pay, "  edited by cashier  ");
        }
        assert!(is_payment_posted(&entries, 31, 7));
        assert!(payment_journal_memo_with_note(&pay, "edited").ends_with("edited"));
        assert!(!is_payment_posted(&entries, 99, 7));
        let legacy = [JournalEntry {
            memo: "Payment #31 patient #7: old note".into(),
            transaction_num: 12,
            ..Default::default()
        }];
        assert!(is_payment_posted(&legacy, 31, 7));
        assert_eq!(journal_transaction_for_payment(&legacy, 31, 7), Some(12));
    }

    #[test]
    fn invalid_payment_postings_are_rejected_before_any_write() {
        let pay = payment(10.0);
        assert!(build_payment_journal(&pay, 1, 1, 1).is_err());
        assert!(build_payment_journal(&pay, 0, 2, 1).is_err());
        assert!(build_payment_journal(&pay, 1, 2, 0).is_err());
        assert!(build_payment_journal(&payment(0.0), 1, 2, 1).is_err());
    }

    #[test]
    fn posted_payment_reaches_the_general_ledger() {
        let accounts = accounts();
        let entries = build_payment_journal(&payment(30.0), 1, 2, 13).unwrap();
        let ledger = crate::general_ledger_report(
            &accounts,
            &entries,
            NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 11, 30).unwrap(),
            false,
        );
        let flat = ledger.rows.concat().join("|");
        assert!(flat.contains("Petty Cash"));
        assert!(flat.contains("Accounts Receivable"));
        assert!(flat.contains("Cash at desk"), "the payment note reaches the memo");
    }

    fn procedure(fee: f64) -> Procedure {
        Procedure {
            proc_num: 44,
            pat_num: 7,
            code_num: 3,
            proc_date: NaiveDate::from_ymd_opt(2026, 11, 22).unwrap(),
            proc_status: od_domain::ProcedureStatus::Complete,
            tooth_num: "16".into(),
            fee,
            description_override: "Composite filling".into(),
            ..Default::default()
        }
    }

    #[test]
    fn charge_pair_is_balanced_and_carries_the_fee_and_date() {
        let entries = build_charge_journal(&procedure(73.5), 2, 5, 14).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].account_num, 2);
        assert_eq!(entries[0].debit_amt, 73.5);
        assert_eq!(entries[1].account_num, 5);
        assert_eq!(entries[1].credit_amt, 73.5);
        assert!(entries.iter().all(|e| e.transaction_num == 14
            && e.date_displayed == NaiveDate::from_ymd_opt(2026, 11, 22).unwrap()));
        assert!(entries.iter().all(|e| e.memo.contains("[Proc #44 pat #7]")
            && e.memo.contains("Composite filling")
            && e.memo.contains("tooth 16")));
        assert!(validate_balanced_journal(&entries).is_ok());
    }

    #[test]
    fn charge_accounts_debit_receivable_and_credit_income() {
        let accounts = accounts();
        assert_eq!(
            suggest_charge_ledger_accounts(&accounts),
            Some((2, 5)),
            "a completed charge debits A/R and credits income"
        );
    }

    #[test]
    fn posted_charges_are_detected_with_custom_and_legacy_memos() {
        let proc = procedure(20.0);
        let mut entries = build_charge_journal(&proc, 2, 5, 15).unwrap();
        assert!(is_procedure_posted(&entries, 44, 7));
        assert_eq!(journal_transaction_for_procedure(&entries, 44, 7), Some(15));
        for entry in &mut entries {
            entry.memo = procedure_journal_memo_with_note(&proc, "  posted by reception  ");
        }
        assert!(is_procedure_posted(&entries, 44, 7));
        assert!(procedure_journal_memo_with_note(&proc, "edited").ends_with("edited"));
        assert!(!is_procedure_posted(&entries, 99, 7));
        let legacy = [JournalEntry {
            memo: "Procedure #44 patient #7: old note".into(),
            transaction_num: 16,
            ..Default::default()
        }];
        assert!(is_procedure_posted(&legacy, 44, 7));
        assert_eq!(journal_transaction_for_procedure(&legacy, 44, 7), Some(16));
    }

    #[test]
    fn batch_plan_skips_planned_posted_and_zero_fee_procedures() {
        let done = procedure(30.0);
        let planned = Procedure {
            proc_num: 45,
            proc_status: od_domain::ProcedureStatus::TreatmentPlanned,
            ..done.clone()
        };
        let zero_fee = Procedure {
            proc_num: 46,
            fee: 0.0,
            ..done.clone()
        };
        let already = Procedure {
            proc_num: 47,
            ..done.clone()
        };
        let mut existing = build_charge_journal(&already, 2, 5, 4).unwrap().to_vec();
        // Give the already-posted procedure a distinct reference.
        let already_memo = procedure_journal_reference(47, 7);
        for entry in &mut existing {
            entry.memo = already_memo.clone();
        }

        let plan = build_charge_posting_plan(
            &[done.clone(), planned, zero_fee, already],
            &existing,
            2,
            5,
        );
        assert_eq!(plan.len(), 1, "only the unposted completed charge is planned");
        assert_eq!(plan[0].procedure.proc_num, 44);
        assert_eq!(plan[0].transaction_num, 5, "continues after the largest used");
        validate_balanced_journal(&plan[0].entries).unwrap();
    }

    #[test]
    fn invalid_charge_postings_are_rejected_before_any_write() {
        let proc = procedure(10.0);
        assert!(build_charge_journal(&proc, 2, 2, 1).is_err());
        assert!(build_charge_journal(&proc, 0, 5, 1).is_err());
        assert!(build_charge_journal(&proc, 2, 5, 0).is_err());
        assert!(build_charge_journal(&procedure(0.0), 2, 5, 1).is_err());
    }

    #[test]
    fn posted_charge_reaches_the_accounting_reports() {
        let accounts = accounts();
        let as_of = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        let before = crate::profit_loss_report(&accounts, &[], NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), as_of);
        let entries = build_charge_journal(&procedure(73.5), 2, 5, 17).unwrap();
        let after = crate::profit_loss_report(&accounts, &entries, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), as_of);
        assert_ne!(before.rows, after.rows, "posting must change the previously read-only P&L");

        let ledger = crate::general_ledger_report(
            &accounts,
            &entries,
            NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            as_of,
            false,
        );
        let flat = ledger.rows.concat().join("|");
        assert!(flat.contains("Accounts Receivable"));
        assert!(flat.contains("Patient Income"));
        assert!(flat.contains("Composite filling"), "the charge description reaches the memo");
    }
}
