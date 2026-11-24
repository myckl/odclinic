//! Posting patient charges, payments and adjustments into OD's double-entry
//! `journalentry` table.
//!
//! The accounting reports (Balance Sheet, General Ledger, Profit & Loss) read
//! only `journalentry`. Before this module the port could record these events
//! on a patient account but had no write path to the journal, so those reports
//! stayed read-only. This module builds the balanced lines for each posted
//! event; `Store::post_journal_transaction` and `Store::post_journal_batch`
//! write them atomically.

use std::collections::BTreeSet;

use od_domain::{
    Account, AccountType, Adjustment, Id, JournalEntry, PaySplit, Payment, Procedure,
};

use crate::ReportTable;

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

/// The four account pairs used when posting a mixed batch. Charges and positive
/// adjustments debit A/R and credit income; a collection debits cash and credits
/// A/R (a refund reverses that); a write-off debits expense and credits A/R.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PostingAccountDefaults {
    pub charge: (Id, Id),
    pub payment: (Id, Id),
    pub positive_adjustment: (Id, Id),
    pub negative_adjustment: (Id, Id),
}

impl PostingAccountDefaults {
    /// Take the same defaults the individual posting windows offer from the
    /// chart of accounts, so a bulk run books to the accounts staff would pick.
    pub fn from_chart(accounts: &[Account]) -> Self {
        Self {
            charge: suggest_charge_ledger_accounts(accounts).unwrap_or((0, 0)),
            payment: suggest_payment_ledger_accounts(accounts, 1.0).unwrap_or((0, 0)),
            positive_adjustment: suggest_ledger_accounts(accounts, 1.0).unwrap_or((0, 0)),
            negative_adjustment: suggest_ledger_accounts(accounts, -1.0).unwrap_or((0, 0)),
        }
    }

    /// Every pair must name two different, real accounts before any write.
    pub fn is_complete(&self) -> bool {
        [
            self.charge,
            self.payment,
            self.positive_adjustment,
            self.negative_adjustment,
        ]
        .iter()
        .all(|(debit, credit)| *debit != 0 && *credit != 0 && debit != credit)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostingSourceKind {
    Charge,
    Payment,
    Adjustment,
}

impl PostingSourceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Charge => "Charge",
            Self::Payment => "Payment",
            Self::Adjustment => "Adjustment",
        }
    }
}

/// A source record that can be posted to the journal.
#[derive(Clone, Debug)]
pub enum PostingSource {
    Charge(Procedure),
    Payment(Payment),
    Adjustment(Adjustment),
}

impl PostingSource {
    pub fn kind(&self) -> PostingSourceKind {
        match self {
            Self::Charge(_) => PostingSourceKind::Charge,
            Self::Payment(_) => PostingSourceKind::Payment,
            Self::Adjustment(_) => PostingSourceKind::Adjustment,
        }
    }

    pub fn number(&self) -> Id {
        match self {
            Self::Charge(proc) => proc.proc_num,
            Self::Payment(payment) => payment.pay_num,
            Self::Adjustment(adj) => adj.adj_num,
        }
    }

    pub fn pat_num(&self) -> Id {
        match self {
            Self::Charge(proc) => proc.pat_num,
            Self::Payment(payment) => payment.pat_num,
            Self::Adjustment(adj) => adj.pat_num,
        }
    }

    pub fn date(&self) -> chrono::NaiveDate {
        match self {
            Self::Charge(proc) => proc.proc_date,
            Self::Payment(payment) => payment.pay_date,
            Self::Adjustment(adj) => adj.adj_date,
        }
    }

    /// Signed source amount, matching the clinical reports (a negative
    /// adjustment is a write-off; a negative payment is a refund).
    pub fn amount(&self) -> f64 {
        match self {
            Self::Charge(proc) => proc.fee,
            Self::Payment(payment) => payment.pay_amount,
            Self::Adjustment(adj) => adj.adj_amount,
        }
    }

    pub fn description(&self) -> String {
        match self {
            Self::Charge(proc) => procedure_description(proc),
            Self::Payment(payment) => {
                let mut text = if payment.note.trim().is_empty() {
                    format!("Payment type {}", payment.pay_type)
                } else {
                    payment.note.trim().to_string()
                };
                if !payment.check_num.trim().is_empty() {
                    text.push_str(&format!(" · cheque {}", payment.check_num.trim()));
                }
                text
            }
            Self::Adjustment(adj) => {
                if adj.note.trim().is_empty() {
                    format!("Adjustment #{}", adj.adj_num)
                } else {
                    adj.note.trim().to_string()
                }
            }
        }
    }

    pub fn is_posted(&self, journal: &[JournalEntry]) -> bool {
        match self {
            Self::Charge(proc) => is_procedure_posted(journal, proc.proc_num, proc.pat_num),
            Self::Payment(payment) => is_payment_posted(journal, payment.pay_num, payment.pat_num),
            Self::Adjustment(adj) => is_adjustment_posted(journal, adj.adj_num, adj.pat_num),
        }
    }
}

/// One reviewed item in a mixed batch: the source, its account pair and the
/// transaction group it will be written under.
#[derive(Clone, Debug)]
pub struct PostingBatchItem {
    pub source: PostingSource,
    pub debit_account: Id,
    pub credit_account: Id,
    pub transaction_num: Id,
}

impl PostingBatchItem {
    /// Build the balanced pair for this item from its current account pair.
    pub fn entries(&self) -> Result<[JournalEntry; 2], String> {
        match &self.source {
            PostingSource::Charge(proc) => build_charge_journal(
                proc,
                self.debit_account,
                self.credit_account,
                self.transaction_num,
            ),
            PostingSource::Payment(payment) => build_payment_journal(
                payment,
                self.debit_account,
                self.credit_account,
                self.transaction_num,
            ),
            PostingSource::Adjustment(adj) => build_adjustment_journal(
                adj,
                self.debit_account,
                self.credit_account,
                self.transaction_num,
            ),
        }
    }
}

fn default_account_pair(defaults: &PostingAccountDefaults, source: &PostingSource) -> (Id, Id) {
    match source {
        PostingSource::Charge(_) => defaults.charge,
        // A refund reverses the normal cash/A/R direction.
        PostingSource::Payment(payment) if payment.pay_amount < 0.0 => {
            (defaults.payment.1, defaults.payment.0)
        }
        PostingSource::Payment(_) => defaults.payment,
        PostingSource::Adjustment(adj) if adj.adj_amount < 0.0 => defaults.negative_adjustment,
        PostingSource::Adjustment(_) => defaults.positive_adjustment,
    }
}

/// Plan every unposted charge, payment and adjustment in the date range in one
/// pass. Each item gets its own transaction number, continuing after the largest
/// one already in the journal. Already-posted, zero-value and out-of-range
/// records are skipped, as are items whose account pair is incomplete.
pub fn build_posting_batch_plan(
    procedures: &[Procedure],
    payments: &[Payment],
    adjustments: &[Adjustment],
    journal: &[JournalEntry],
    defaults: &PostingAccountDefaults,
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> Vec<PostingBatchItem> {
    let mut next = next_journal_transaction_num(journal);
    let mut items = Vec::new();
    let in_range = |date: chrono::NaiveDate| date >= from && date <= to;
    let mut add = |source: PostingSource| {
        if !in_range(source.date())
            || source.is_posted(journal)
            || !source.amount().is_finite()
            || source.amount().abs() <= 0.005
        {
            return;
        }
        let (debit_account, credit_account) = default_account_pair(defaults, &source);
        if debit_account == 0 || credit_account == 0 || debit_account == credit_account {
            return;
        }
        items.push(PostingBatchItem {
            source,
            debit_account,
            credit_account,
            transaction_num: next,
        });
        next += 1;
    };
    for proc in procedures {
        if proc.proc_status.is_completed() {
            add(PostingSource::Charge(proc.clone()));
        }
    }
    for payment in payments {
        add(PostingSource::Payment(payment.clone()));
    }
    for adj in adjustments {
        add(PostingSource::Adjustment(adj.clone()));
    }
    items
}

/// Turn a reviewed plan into journal lines for `Store::post_journal_batch`.
/// Every pair is checked for balance and transaction uniqueness before any line
/// is handed to the store.
pub fn build_journal_batches(items: &[PostingBatchItem]) -> Result<Vec<Vec<JournalEntry>>, String> {
    let mut seen = BTreeSet::new();
    let mut batches = Vec::with_capacity(items.len());
    for item in items {
        let entries = item.entries()?;
        validate_balanced_journal(&entries)?;
        if !seen.insert(item.transaction_num) {
            return Err(format!(
                "Transaction number {} is used more than once",
                item.transaction_num
            ));
        }
        batches.push(entries.to_vec());
    }
    Ok(batches)
}

/// Read-only audit of every posting source in a date range. Each charge,
/// payment and adjustment shows whether it reached `journalentry`, the journal
/// transaction that carries it, and a running posted/unposted total per source.
/// This makes the manual posting workflow verifiable before month-end: nothing
/// can be silently left out of the accounting reports.
pub fn journal_posting_audit_report(
    procedures: &[Procedure],
    payments: &[Payment],
    adjustments: &[Adjustment],
    journal: &[JournalEntry],
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Type",
        "Number",
        "Patient",
        "Date",
        "Description",
        "Amount",
        "Status",
        "Transaction",
    ]);
    // (amount, posted count, unposted count) for charges, payments, adjustments.
    let mut totals: [(f64, usize, usize); 3] = [(0.0, 0, 0); 3];

    let mut charges: Vec<&Procedure> = procedures
        .iter()
        .filter(|p| p.proc_status.is_completed() && p.proc_date >= from && p.proc_date <= to)
        .collect();
    charges.sort_by_key(|p| (p.proc_date, p.proc_num));
    for proc in charges {
        let posted = is_procedure_posted(journal, proc.proc_num, proc.pat_num);
        let amount = proc.fee.abs();
        table.push(&[
            "Charge",
            &proc.proc_num.to_string(),
            &proc.pat_num.to_string(),
            &proc.proc_date.to_string(),
            &procedure_description(proc),
            &format!("{amount:.2}"),
            if posted { "Posted" } else { "UNPOSTED" },
            &journal_transaction_for_procedure(journal, proc.proc_num, proc.pat_num)
                .map(|n| n.to_string())
                .unwrap_or_default(),
        ]);
        totals[0].0 += amount;
        if posted {
            totals[0].1 += 1;
        } else {
            totals[0].2 += 1;
        }
    }

    let mut collected: Vec<&Payment> = payments
        .iter()
        .filter(|p| p.pay_date >= from && p.pay_date <= to)
        .collect();
    collected.sort_by_key(|p| (p.pay_date, p.pay_num));
    for payment in collected {
        let posted = is_payment_posted(journal, payment.pay_num, payment.pat_num);
        let mut description = if payment.note.trim().is_empty() {
            format!("Payment type {}", payment.pay_type)
        } else {
            payment.note.trim().to_string()
        };
        if !payment.check_num.trim().is_empty() {
            description.push_str(&format!(" · cheque {}", payment.check_num.trim()));
        }
        table.push(&[
            "Payment",
            &payment.pay_num.to_string(),
            &payment.pat_num.to_string(),
            &payment.pay_date.to_string(),
            &description,
            &format!("{:.2}", payment.pay_amount),
            if posted { "Posted" } else { "UNPOSTED" },
            &journal_transaction_for_payment(journal, payment.pay_num, payment.pat_num)
                .map(|n| n.to_string())
                .unwrap_or_default(),
        ]);
        totals[1].0 += payment.pay_amount;
        if posted {
            totals[1].1 += 1;
        } else {
            totals[1].2 += 1;
        }
    }

    let mut collected: Vec<&Adjustment> = adjustments
        .iter()
        .filter(|a| a.adj_date >= from && a.adj_date <= to)
        .collect();
    collected.sort_by_key(|a| (a.adj_date, a.adj_num));
    for adj in collected {
        let posted = is_adjustment_posted(journal, adj.adj_num, adj.pat_num);
        let description = if adj.note.trim().is_empty() {
            format!("Adjustment #{}", adj.adj_num)
        } else {
            adj.note.trim().to_string()
        };
        table.push(&[
            "Adjustment",
            &adj.adj_num.to_string(),
            &adj.pat_num.to_string(),
            &adj.adj_date.to_string(),
            &description,
            &format!("{:.2}", adj.adj_amount),
            if posted { "Posted" } else { "UNPOSTED" },
            &journal_transaction_for_adjustment(journal, adj.adj_num, adj.pat_num)
                .map(|n| n.to_string())
                .unwrap_or_default(),
        ]);
        totals[2].0 += adj.adj_amount;
        if posted {
            totals[2].1 += 1;
        } else {
            totals[2].2 += 1;
        }
    }

    for (label, (amount, posted, unposted)) in [
        ("Charges", totals[0]),
        ("Payments", totals[1]),
        ("Adjustments", totals[2]),
    ] {
        table.push(&[
            "Total",
            label,
            "",
            "",
            &format!("{posted} posted / {unposted} unposted"),
            &format!("{amount:.2}"),
            "",
            "",
        ]);
    }
    table
}

fn has_charge_reference(entry: &JournalEntry) -> bool {
    entry.memo.contains("[Proc #") || entry.memo.contains("Procedure #")
}

fn has_payment_reference(entry: &JournalEntry) -> bool {
    entry.memo.contains("[Pay #") || entry.memo.contains("Payment #")
}

fn has_adjustment_reference(entry: &JournalEntry) -> bool {
    entry.memo.contains("[Adj #") || entry.memo.contains("Adjustment #")
}

/// Period-end reconciliation between the clinical reports and the accounting
/// reports. The first column is the figure the clinical side reports (completed
/// procedure fees, payments, signed adjustments), the second is the figure the
/// General Ledger actually books, and the third is the difference. A zero
/// variance on every source means the posting workflow can be trusted for a
/// period close: the accounting reports are not missing a charge or carrying a
/// duplicate. The final rows reconcile the A/R movement and its closing balance.
pub fn ledger_tie_out_report(
    procedures: &[Procedure],
    payments: &[Payment],
    adjustments: &[Adjustment],
    splits: &[PaySplit],
    journal: &[JournalEntry],
    accounts: &[Account],
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Check",
        "Clinical source",
        "Journal",
        "Variance",
        "Status",
        "Detail",
    ]);
    let tolerance = 0.005;
    let in_range = |date: chrono::NaiveDate| date >= from && date <= to;
    let money = |value: f64| format!("{:.2}", if value == 0.0 { 0.0 } else { value });
    let status = |variance: f64| {
        if variance.abs() <= tolerance {
            "OK"
        } else {
            "OUT OF BALANCE"
        }
    };

    // Use the same chart defaults the posting UI offers, so the tie-out reads
    // the accounts the journal would have been written to. Both helpers fall
    // back to the first active accounts on a minimal chart of accounts.
    let receivable = suggest_charge_ledger_accounts(accounts).map(|(debit, _)| debit);
    let cash = suggest_payment_ledger_accounts(accounts, 1.0).map(|(debit, _)| debit);
    let ar_lines: Vec<&JournalEntry> = match receivable {
        Some(account) => journal
            .iter()
            .filter(|e| e.account_num == account && in_range(e.date_displayed))
            .collect(),
        None => Vec::new(),
    };

    let production: f64 = procedures
        .iter()
        .filter(|p| p.proc_status.is_completed() && in_range(p.proc_date))
        .map(|p| p.fee.abs())
        .sum();
    let posted_charges: f64 = ar_lines
        .iter()
        .filter(|e| has_charge_reference(e))
        .map(|e| e.debit_amt)
        .sum();

    let collected: f64 = payments
        .iter()
        .filter(|p| in_range(p.pay_date))
        .map(|p| p.pay_amount)
        .sum();
    let posted_collections: f64 = match cash {
        Some(account) => journal
            .iter()
            .filter(|e| e.account_num == account && in_range(e.date_displayed))
            .filter(|e| has_payment_reference(e))
            .map(|e| e.debit_amt)
            .sum(),
        None => 0.0,
    };
    let split_collections: f64 = splits
        .iter()
        .filter(|s| in_range(s.date_pay))
        .map(|s| s.split_amount)
        .sum();

    let adjustments_total: f64 = adjustments
        .iter()
        .filter(|a| in_range(a.adj_date))
        .map(|a| a.adj_amount)
        .sum();
    let posted_adjustments: f64 = ar_lines
        .iter()
        .filter(|e| has_adjustment_reference(e))
        .map(|e| e.debit_amt - e.credit_amt)
        .sum();

    // Charges and positive adjustments debit A/R; collections and write-offs
    // credit it. The expected movement uses the clinical figures, the actual
    // one uses every A/R journal line in the range, including manual entries.
    let expected_ar = production + adjustments_total - collected;
    let actual_ar: f64 = ar_lines.iter().map(|e| e.debit_amt - e.credit_amt).sum();
    let closing_ar: f64 = match receivable {
        Some(account) => journal
            .iter()
            .filter(|e| e.account_num == account && e.date_displayed <= to)
            .map(|e| e.debit_amt - e.credit_amt)
            .sum(),
        None => 0.0,
    };

    for (label, source, posted, detail) in [
        (
            "Charges",
            production,
            posted_charges,
            "Completed procedure fees vs posted A/R charge debits",
        ),
        (
            "Collections",
            collected,
            posted_collections,
            "Payment amounts vs posted cash debits",
        ),
        (
            "Adjustments",
            adjustments_total,
            posted_adjustments,
            "Signed adjustments vs posted A/R movement",
        ),
        (
            "A/R movement",
            expected_ar,
            actual_ar,
            "Charges + adjustments - collections vs all A/R journal activity",
        ),
    ] {
        let variance = posted - source;
        table.push(&[
            label,
            &money(source),
            &money(posted),
            &money(variance),
            status(variance),
            detail,
        ]);
    }

    // A payment can be split across procedures, so the split total is shown
    // for comparison with the Collections report but is not a hard gate: the
    // journal books one cash debit per payment.
    let split_variance = posted_collections - split_collections;
    table.push(&[
        "Paysplit collections",
        &money(split_collections),
        &money(posted_collections),
        &money(split_variance),
        "INFO",
        "Collections report basis vs cash booked per payment",
    ]);
    table.push(&[
        "A/R balance",
        "",
        &money(closing_ar),
        "",
        "INFO",
        &format!("Accounts receivable at {to}"),
    ]);

    let unposted = procedures
        .iter()
        .filter(|p| p.proc_status.is_completed() && in_range(p.proc_date))
        .filter(|p| !is_procedure_posted(journal, p.proc_num, p.pat_num))
        .count()
        + payments
            .iter()
            .filter(|p| in_range(p.pay_date))
            .filter(|p| !is_payment_posted(journal, p.pay_num, p.pat_num))
            .count()
        + adjustments
            .iter()
            .filter(|a| in_range(a.adj_date))
            .filter(|a| !is_adjustment_posted(journal, a.adj_num, a.pat_num))
            .count();
    table.push(&[
        "Unposted sources",
        &unposted.to_string(),
        "",
        "",
        if unposted == 0 { "OK" } else { "CHECK" },
        "See Journal posting audit for the individual records",
    ]);
    if receivable.is_none() || cash.is_none() {
        table.push(&[
            "Account setup",
            "",
            "",
            "",
            "CHECK",
            "Chart of accounts has no receivable or cash account to tie against",
        ]);
    }
    table
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

    #[test]
    fn posting_audit_lists_every_source_with_its_status_and_totals() {
        let from = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 11, 30).unwrap();
        let posted_charge = procedure(40.0);
        let unposted = Procedure {
            proc_num: 45,
            fee: 15.0,
            ..posted_charge.clone()
        };
        let out_of_range = Procedure {
            proc_num: 46,
            proc_date: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            ..posted_charge.clone()
        };
        let mut journal = build_charge_journal(&posted_charge, 2, 5, 23).unwrap().to_vec();
        let pay = Payment {
            pay_num: 31,
            pat_num: 7,
            pay_date: NaiveDate::from_ymd_opt(2026, 11, 24).unwrap(),
            pay_amount: 20.0,
            pay_type: 0,
            check_num: "CHQ-1".into(),
            note: "Desk cash".into(),
        };
        let mut pay_journal = build_payment_journal(&pay, 1, 2, 24).unwrap().to_vec();
        journal.extend(pay_journal.drain(..));
        let unposted_adj = adjustment(-5.0);

        let table = journal_posting_audit_report(
            &[posted_charge.clone(), unposted, out_of_range],
            &[pay],
            &[unposted_adj],
            &journal,
            from,
            to,
        );
        assert_eq!(table.headers.len(), 8);
        assert_eq!(table.headers[0], "Type");
        assert_eq!(table.headers[6], "Status");
        // Two in-range charges + one payment + one adjustment + three totals.
        assert_eq!(table.rows.len(), 7);
        assert_eq!(table.rows[0][6], "Posted");
        assert_eq!(table.rows[0][7], "23");
        assert_eq!(table.rows[1][6], "UNPOSTED");
        assert_eq!(table.rows[1][7], "");
        assert_eq!(table.rows[2][6], "Posted");
        assert!(table.rows[2][4].contains("CHQ-1"));
        assert_eq!(table.rows[3][6], "UNPOSTED");
        assert_eq!(table.rows[4], vec!["Total", "Charges", "", "", "1 posted / 1 unposted", "55.00", "", ""]);
        assert_eq!(table.rows[5], vec!["Total", "Payments", "", "", "1 posted / 0 unposted", "20.00", "", ""]);
        assert_eq!(table.rows[6], vec!["Total", "Adjustments", "", "", "0 posted / 1 unposted", "-5.00", "", ""]);
    }

    #[test]
    fn tie_out_reconciles_when_every_source_is_posted() {
        let accounts = accounts();
        let from = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 11, 30).unwrap();
        let proc = procedure(40.0);
        let pay = Payment {
            pay_num: 31,
            pat_num: proc.pat_num,
            pay_date: NaiveDate::from_ymd_opt(2026, 11, 24).unwrap(),
            pay_amount: 20.0,
            pay_type: 0,
            ..Default::default()
        };
        let adj = adjustment(-5.0);
        let splits = [PaySplit {
            split_num: 1,
            pay_num: pay.pay_num,
            pat_num: pay.pat_num,
            split_amount: 20.0,
            date_pay: pay.pay_date,
            ..Default::default()
        }];
        let mut journal = Vec::new();
        journal.extend(build_charge_journal(&proc, 2, 5, 1).unwrap());
        journal.extend(build_payment_journal(&pay, 1, 2, 2).unwrap());
        journal.extend(build_adjustment_journal(&adj, 6, 2, 3).unwrap());

        let table = ledger_tie_out_report(
            &[proc], &[pay], &[adj], &splits, &journal, &accounts, from, to,
        );
        assert_eq!(
            table.headers,
            vec!["Check", "Clinical source", "Journal", "Variance", "Status", "Detail"]
        );
        let row = |label: &str| table.rows.iter().find(|r| r[0] == label).unwrap();
        let charges = row("Charges");
        assert_eq!(charges[1], "40.00");
        assert_eq!(charges[2], "40.00");
        assert_eq!(charges[3], "0.00");
        assert_eq!(charges[4], "OK");
        let collections = row("Collections");
        assert_eq!(collections[1], "20.00");
        assert_eq!(collections[2], "20.00");
        assert_eq!(collections[4], "OK");
        let adjustments = row("Adjustments");
        assert_eq!(adjustments[1], "-5.00");
        assert_eq!(adjustments[2], "-5.00");
        assert_eq!(adjustments[4], "OK");
        let movement = row("A/R movement");
        assert_eq!(movement[1], "15.00");
        assert_eq!(movement[2], "15.00");
        assert_eq!(movement[3], "0.00");
        assert_eq!(movement[4], "OK");
        assert_eq!(row("A/R balance")[2], "15.00");
        assert_eq!(row("Paysplit collections")[4], "INFO");
        assert_eq!(row("Unposted sources")[1], "0");
        assert_eq!(row("Unposted sources")[4], "OK");
        assert!(table.rows.iter().all(|r| r[4] != "OUT OF BALANCE"));
    }

    #[test]
    fn tie_out_flags_unposted_charges_and_journal_only_drift() {
        let accounts = accounts();
        let from = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 11, 30).unwrap();
        let proc = procedure(40.0);
        let pay = Payment {
            pay_num: 31,
            pat_num: proc.pat_num,
            pay_date: NaiveDate::from_ymd_opt(2026, 11, 24).unwrap(),
            pay_amount: 20.0,
            pay_type: 0,
            ..Default::default()
        };
        let adj = adjustment(-5.0);
        // The charge was never posted; a stray manual A/R debit was.
        let mut journal = build_payment_journal(&pay, 1, 2, 2).unwrap().to_vec();
        journal.extend(build_adjustment_journal(&adj, 6, 2, 3).unwrap());
        journal.push(JournalEntry {
            account_num: 2,
            date_displayed: NaiveDate::from_ymd_opt(2026, 11, 28).unwrap(),
            debit_amt: 7.0,
            memo: "Manual entry with no source record".into(),
            transaction_num: 4,
            ..Default::default()
        });

        let table = ledger_tie_out_report(
            &[proc], &[pay], &[adj], &[], &journal, &accounts, from, to,
        );
        let row = |label: &str| table.rows.iter().find(|r| r[0] == label).unwrap();
        let charges = row("Charges");
        assert_eq!(charges[2], "0.00");
        assert_eq!(charges[3], "-40.00");
        assert_eq!(charges[4], "OUT OF BALANCE");
        assert_eq!(row("Collections")[4], "OK");
        assert_eq!(row("Adjustments")[4], "OK");
        let movement = row("A/R movement");
        assert_eq!(movement[1], "15.00");
        assert_eq!(movement[2], "-18.00");
        assert_eq!(movement[3], "-33.00");
        assert_eq!(movement[4], "OUT OF BALANCE");
        assert_eq!(row("Unposted sources")[1], "1");
        assert_eq!(row("Unposted sources")[4], "CHECK");
    }

    #[test]
    fn mixed_batch_plans_every_source_once_and_is_idempotent() {
        use od_domain::ProcedureStatus;

        let accounts = accounts();
        let defaults = PostingAccountDefaults::from_chart(&accounts);
        assert!(defaults.is_complete());
        assert_eq!(defaults.charge, (2, 5));
        assert_eq!(defaults.payment, (1, 2));
        assert_eq!(defaults.positive_adjustment, (2, 5));
        assert_eq!(defaults.negative_adjustment, (6, 2));
        let from = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 11, 30).unwrap();
        let charge = procedure(40.0);
        let already = Procedure {
            proc_num: 45,
            ..charge.clone()
        };
        let no_fee = Procedure {
            proc_num: 46,
            fee: 0.0,
            ..charge.clone()
        };
        let old = Procedure {
            proc_num: 47,
            proc_date: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            ..charge.clone()
        };
        let draft = Procedure {
            proc_num: 48,
            proc_status: ProcedureStatus::TreatmentPlanned,
            ..charge.clone()
        };
        let posted_charge = build_charge_journal(&already, 2, 5, 1).unwrap();
        let journal = posted_charge.to_vec();
        let pay = Payment {
            pay_num: 31,
            pat_num: 7,
            pay_date: NaiveDate::from_ymd_opt(2026, 11, 24).unwrap(),
            pay_amount: 20.0,
            pay_type: 0,
            ..Default::default()
        };
        let adj = adjustment(-5.0);

        let plan = build_posting_batch_plan(
            &[charge.clone(), already, no_fee, old, draft],
            &[pay.clone()],
            &[adj.clone()],
            &journal,
            &defaults,
            from,
            to,
        );
        assert_eq!(plan.len(), 3, "one charge, one payment, one adjustment");
        assert_eq!(plan[0].source.kind(), PostingSourceKind::Charge);
        assert_eq!(plan[1].source.kind(), PostingSourceKind::Payment);
        assert_eq!(plan[2].source.kind(), PostingSourceKind::Adjustment);
        assert_eq!((plan[0].debit_account, plan[0].credit_account), (2, 5));
        assert_eq!((plan[1].debit_account, plan[1].credit_account), (1, 2));
        assert_eq!((plan[2].debit_account, plan[2].credit_account), (6, 2));
        assert_eq!(
            plan.iter().map(|i| i.transaction_num).collect::<Vec<_>>(),
            vec![2, 3, 4]
        );
        let batches = build_journal_batches(&plan).unwrap();
        assert_eq!(batches.len(), 3);
        assert!(batches.iter().all(|b| b.len() == 2));

        let mut after = journal;
        for batch in batches {
            after.extend(batch);
        }
        assert!(build_posting_batch_plan(
            &[charge], &[pay], &[adj], &after, &defaults, from, to
        )
        .is_empty());
    }

    #[test]
    fn mixed_batch_directs_refunds_and_positive_adjustments() {
        let defaults = PostingAccountDefaults::from_chart(&accounts());
        let from = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 11, 30).unwrap();
        let refund = Payment {
            pay_num: 32,
            pat_num: 7,
            pay_date: NaiveDate::from_ymd_opt(2026, 11, 25).unwrap(),
            pay_amount: -10.0,
            pay_type: 0,
            ..Default::default()
        };
        let plan = build_posting_batch_plan(
            &[],
            &[refund],
            &[adjustment(10.0), adjustment(-5.0)],
            &[],
            &defaults,
            from,
            to,
        );
        assert_eq!(plan.len(), 3);
        assert_eq!((plan[0].debit_account, plan[0].credit_account), (2, 1));
        assert_eq!((plan[1].debit_account, plan[1].credit_account), (2, 5));
        assert_eq!((plan[2].debit_account, plan[2].credit_account), (6, 2));
        assert!(build_journal_batches(&plan).is_ok());
    }

    #[test]
    fn mixed_batch_refuses_reused_transaction_numbers() {
        let defaults = PostingAccountDefaults::from_chart(&accounts());
        let from = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 11, 30).unwrap();
        let mut plan = build_posting_batch_plan(
            &[procedure(40.0)],
            &[],
            &[adjustment(-5.0)],
            &[],
            &defaults,
            from,
            to,
        );
        plan[1].transaction_num = plan[0].transaction_num;
        assert!(build_journal_batches(&plan)
            .unwrap_err()
            .contains("used more than once"));
    }
}
