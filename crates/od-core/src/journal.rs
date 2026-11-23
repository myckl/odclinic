//! Posting patient adjustments into OD's double-entry `journalentry` table.
//!
//! The accounting reports (Balance Sheet, General Ledger, Profit & Loss) read
//! only `journalentry`. Before this module the port could record adjustments on
//! a patient account but had no write path to the journal, so those reports
//! stayed read-only. This module builds the two balanced lines for one posted
//! adjustment; `Store::post_journal_transaction` writes them atomically.

use od_domain::{Account, AccountType, Adjustment, Id, JournalEntry};

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
}
