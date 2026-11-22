//! Pure report builders for the second upstream tranche (`FormRp*` forms 21+).
//!
//! Every builder here follows the same contract as `crate::reports`: it takes
//! immutable slices of domain records plus a small options struct and returns a
//! `ReportTable` with OD's own column names. Keeping row construction free of
//! GUI and database concerns makes the numbers unit-testable and gives the UI
//! one code path for display, sorting and CSV/HTML export.
//!
//! The upstream forms these mirror are documented in `docs/reports-upstream.md`
//! alongside the exact OD column and formula each row follows.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{Datelike, NaiveDate};
use od_domain::definitions::{Definition, DefinitionCategory};
use od_domain::reports_ext::{
    HiddenPaySplitsReportOptions, LaserInsMode, LaserLabelOptions, ProcNoteGroup,
    ProcNoteReportOptions, ProcOverpaidReportOptions, TreatmentFinderReportOptions,
    UnearnedIncomeTab,
};
use od_domain::*;

use crate::reports::ReportTable;

/// Two-decimal money formatting used by OD's `.ToString("n")` report cells.
/// OD renders a zero balance as `0.00`, never `-0.00`, so normalize the sign of
/// zero before formatting (arithmetic such as `0.0 - 0.0` yields `-0.0` in Rust).
pub(crate) fn money(value: f64) -> String {
    let value = if value == 0.0 { 0.0 } else { value };
    format!("{:.2}", value)
}

/// OD rounds the money columns in `RpDPPOvercharged` before comparing them;
/// keeping that explicit avoids float drift changing which plans are reported.
fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// Whole-number formatting for count columns.
fn count(value: usize) -> String {
    value.to_string()
}

fn short_date(value: NaiveDate) -> String {
    value.format("%m/%d/%Y").to_string()
}

fn patient_name(patients: &[Patient], pat_num: Id) -> String {
    patients
        .iter()
        .find(|p| p.pat_num == pat_num)
        .map(|p| p.display_name())
        .unwrap_or_else(|| format!("Patient #{pat_num}"))
}

fn patient_last_first(patients: &[Patient], pat_num: Id) -> String {
    patients
        .iter()
        .find(|p| p.pat_num == pat_num)
        .map(|p| {
            format!("{}, {}", p.l_name, p.f_name)
                .trim()
                .trim_end_matches(',')
                .to_string()
        })
        .unwrap_or_else(|| format!("Patient #{pat_num}"))
}

fn provider_name(providers: &[Provider], provider_num: Id) -> String {
    if provider_num == 0 {
        return "Unassigned".into();
    }
    providers
        .iter()
        .find(|p| p.provider_num == provider_num)
        .map(|p| p.display_name())
        .unwrap_or_else(|| format!("Provider #{provider_num}"))
}

fn provider_abbr(providers: &[Provider], provider_num: Id) -> String {
    if provider_num == 0 {
        return String::new();
    }
    providers
        .iter()
        .find(|p| p.provider_num == provider_num)
        .map(|p| {
            if p.abbreviation.is_empty() {
                p.l_name.clone()
            } else {
                p.abbreviation.clone()
            }
        })
        .unwrap_or_default()
}

/// Definition numbers in `DefCat.PaySplitUnearnedType` that OD treats as
/// hidden unearned types. Per the 24_3 source a type is hidden when its
/// `ItemValue` is non-empty — `Definition.IsHidden` is NOT the test here.
pub(crate) fn hidden_unearned_def_nums(definitions: &[Definition]) -> BTreeSet<Id> {
    definitions
        .iter()
        .filter(|d| d.category == DefinitionCategory::PAY_SPLIT_UNEARNED_TYPE)
        .filter(|d| !d.item_value.trim().is_empty())
        .map(|d| d.def_num)
        .collect()
}

fn unearned_type_name(definitions: &[Definition], def_num: Id) -> String {
    if def_num == 0 {
        return "None".into();
    }
    definitions
        .iter()
        .find(|d| d.def_num == def_num)
        .map(|d| d.item_name.clone())
        .unwrap_or_else(|| format!("Type #{def_num}"))
}

// ---------------------------------------------------------------------------
// Accounting statements (FormRpAccountingBalanceSheet / GenLedg / ProfitLoss)
// ---------------------------------------------------------------------------

/// Net movement on an account up to and including `as_of` (or across the whole
/// range for the ledger). OD's sign rule: Asset/Expense are debit-positive;
/// Liability/Equity/Income are credit-positive.
fn account_movement(entries: &[&JournalEntry], debit_is_pos: bool) -> f64 {
    let mut debit = 0.0;
    let mut credit = 0.0;
    for entry in entries {
        debit += entry.debit_amt;
        credit += entry.credit_amt;
    }
    if debit_is_pos {
        debit - credit
    } else {
        credit - debit
    }
}

fn account_type_label(account_type: AccountType) -> &'static str {
    match account_type {
        AccountType::Asset => "Assets",
        AccountType::Liability => "Liabilities",
        AccountType::Equity => "Equity",
        AccountType::Income => "Income",
        AccountType::Expense => "Expenses",
    }
}

/// `FormRpAccountingBalanceSheet`: chart of accounts with running balances as
/// of the range end date. Inactive accounts with a non-zero balance are kept,
/// matching OD's `GetBalanceSheet`. Retained earnings are never stored: the
/// RE account's prior-year income/expense is folded into its balance, and an
/// optional "NetIncomeThisYear" equity row shows the current-year result.
pub fn balance_sheet_report(
    accounts: &[Account],
    journal: &[JournalEntry],
    as_of: NaiveDate,
    include_net_income: bool,
) -> ReportTable {
    let mut table = ReportTable::new(&["Account", "Type", "Balance"]);
    let year = as_of.year();

    for account_type in [
        AccountType::Asset,
        AccountType::Liability,
        AccountType::Equity,
        AccountType::Income,
        AccountType::Expense,
    ] {
        let mut section_total = 0.0;
        let mut rows: Vec<(String, f64)> = Vec::new();
        for account in accounts
            .iter()
            .filter(|a| a.account_type == account_type)
            .filter(|a| !a.inactive)
        {
            let entries: Vec<&JournalEntry> = journal
                .iter()
                .filter(|e| e.account_num == account.account_num && e.date_displayed <= as_of)
                .collect();
            let mut balance = account_movement(&entries, account_type.debit_is_pos());
            if account.is_retained_earnings {
                // Prior-year income/expense flows into retained earnings.
                for entry in journal.iter().filter(|e| {
                    e.date_displayed.year() < year
                        && is_income_or_expense(accounts, e.account_num)
                }) {
                    balance += net_income_contribution(entry, account_type.debit_is_pos());
                }
            }
            rows.push((account.description.clone(), balance));
            section_total += balance;
        }
        if rows.is_empty() {
            continue;
        }
        table.push(&[account_type_label(account_type), "", ""]);
        for (description, balance) in rows {
            table.push(&[&description, "", &money(balance)]);
        }
        table.push(&[
            &format!("Total {}", account_type_label(account_type)),
            "",
            &money(section_total),
        ]);
    }

    if include_net_income {
        let mut net = 0.0;
        for entry in journal
            .iter()
            .filter(|e| e.date_displayed.year() == year && is_income_or_expense(accounts, e.account_num))
        {
            net += net_income_contribution(entry, true);
        }
        table.push(&["", "", ""]);
        table.push(&["NetIncomeThisYear", "Equity", &money(net)]);
    }
    table
}

fn is_income_or_expense(accounts: &[Account], account_num: Id) -> bool {
    accounts.iter().any(|a| {
        a.account_num == account_num
            && matches!(a.account_type, AccountType::Income | AccountType::Expense)
    })
}

/// Income is credit-positive and expense is debit-positive, so net income
/// (revenue minus expense) is `credit - debit` on both when the target account
/// is credit-positive (Equity). Flipping the sign handles a debit-positive
/// target such as an expense ledger.
fn net_income_contribution(entry: &JournalEntry, target_debit_is_pos: bool) -> f64 {
    let value = entry.credit_amt - entry.debit_amt;
    if target_debit_is_pos { -value } else { value }
}

/// `FormRpAccountingGenLedg`: one section per account with an opening balance,
/// every journal line in the range, and a closing balance. Opening balances are
/// all movements strictly before `from`.
pub fn general_ledger_report(
    accounts: &[Account],
    journal: &[JournalEntry],
    from: NaiveDate,
    to: NaiveDate,
    show_opening_balances: bool,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Date",
        "Account",
        "Description",
        "Debit",
        "Credit",
        "Balance",
        "Memo",
    ]);
    let mut account_nums: Vec<Id> = accounts.iter().map(|a| a.account_num).collect();
    account_nums.extend(journal.iter().map(|e| e.account_num));
    account_nums.sort_unstable();
    account_nums.dedup();

    for account_num in account_nums {
        let account = accounts.iter().find(|a| a.account_num == account_num);
        let debit_is_pos = account
            .map(|a| a.account_type.debit_is_pos())
            .unwrap_or(true);
        let description = account
            .map(|a| a.description.clone())
            .unwrap_or_else(|| format!("Account #{account_num}"));

        let opening = account_movement(
            &journal
                .iter()
                .filter(|e| e.account_num == account_num && e.date_displayed < from)
                .collect::<Vec<_>>(),
            debit_is_pos,
        );
        let in_range: Vec<&JournalEntry> = journal
            .iter()
            .filter(|e| {
                e.account_num == account_num
                    && e.date_displayed >= from
                    && e.date_displayed <= to
            })
            .collect();
        if in_range.is_empty() && (opening == 0.0 || !show_opening_balances) {
            continue;
        }
        table.push(&["", &description, "", "", "", "", ""]);
        let mut running = opening;
        if show_opening_balances {
            table.push(&[
                from.format("%m/%d/%Y").to_string().as_str(),
                "",
                "Opening Balance",
                "",
                "",
                &money(running),
                "",
            ]);
        }
        for entry in in_range {
            running += account_movement(&[entry], debit_is_pos);
            table.push(&[
                &short_date(entry.date_displayed),
                "",
                "",
                &money(entry.debit_amt),
                &money(entry.credit_amt),
                &money(running),
                &entry.memo,
            ]);
        }
        table.push(&["", "", "Ending Balance", "", "", &money(running), ""]);
    }
    table
}

/// `FormRpAccountingProfitLoss`: income and expense accounts with activity in
/// the range, followed by total income, total expenses and net income.
pub fn profit_loss_report(
    accounts: &[Account],
    journal: &[JournalEntry],
    from: NaiveDate,
    to: NaiveDate,
) -> ReportTable {
    let mut table = ReportTable::new(&["Account", "Type", "Amount"]);
    let mut total_income = 0.0;
    let mut total_expenses = 0.0;
    for account_type in [AccountType::Income, AccountType::Expense] {
        table.push(&[account_type_label(account_type), "", ""]);
        let mut section_total = 0.0;
        for account in accounts
            .iter()
            .filter(|a| a.account_type == account_type && !a.inactive)
        {
            let entries: Vec<&JournalEntry> = journal
                .iter()
                .filter(|e| {
                    e.account_num == account.account_num
                        && e.date_displayed >= from
                        && e.date_displayed <= to
                })
                .collect();
            if entries.is_empty() {
                continue;
            }
            let amount = account_movement(&entries, account_type.debit_is_pos());
            table.push(&[&account.description, "", &money(amount)]);
            section_total += amount;
        }
        match account_type {
            AccountType::Income => total_income = section_total,
            AccountType::Expense => total_expenses = section_total,
            _ => {}
        }
        table.push(&[
            &format!("Total {}", account_type_label(account_type)),
            "",
            &money(section_total),
        ]);
    }
    table.push(&["", "", ""]);
    table.push(&["Net Income", "", &money(total_income - total_expenses)]);
    table
}

// ---------------------------------------------------------------------------
// Ledger reports (FormRpHiddenPaySplits, FormRpUnearnedIncome, ...)
// ---------------------------------------------------------------------------

/// `FormRpHiddenPaySplits`: unearned deposits recorded as paysplits whose
/// `UnearnedType` is a hidden type (definition 29 with a non-empty ItemValue).
/// Refunds (negative splits) can be excluded.
pub fn hidden_pay_splits_report(
    patients: &[Patient],
    providers: &[Provider],
    definitions: &[Definition],
    splits: &[PaySplit],
    from: NaiveDate,
    to: NaiveDate,
    options: &HiddenPaySplitsReportOptions,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Date",
        "Patient",
        "Provider",
        "Unearned Type",
        "Amount",
    ]);
    let hidden = hidden_unearned_def_nums(definitions);
    let mut total = 0.0;
    let mut rows: Vec<&PaySplit> = splits
        .iter()
        .filter(|split| {
            split.unearned_type != 0
                && hidden.contains(&split.unearned_type)
                && split.date_pay >= from
                && split.date_pay <= to
                && (!options.exclude_refunds || split.split_amount > 0.0)
        })
        .collect();
    rows.sort_by_key(|split| (split.date_pay, split.pat_num, split.split_num));
    for split in rows {
        table.push(&[
            &short_date(split.date_pay),
            &patient_last_first(patients, split.pat_num),
            &provider_abbr(providers, split.provider_num),
            &unearned_type_name(definitions, split.unearned_type),
            &money(split.split_amount),
        ]);
        total += split.split_amount;
    }
    table.push(&["", "", "", "Total", &money(total)]);
    table
}

/// Whether a paysplit is an unearned deposit. OD marks unearned splits with a
/// non-zero `UnearnedType`; the four tabs all read from that same pool.
fn is_unearned(split: &PaySplit, hidden: &BTreeSet<Id>) -> bool {
    split.unearned_type != 0 && !hidden.contains(&split.unearned_type)
}

/// The four-tab `FormRpUnearnedIncome` port. Each tab produces a different
/// column set, so the caller picks a tab through the options struct.
pub fn unearned_income_report(
    patients: &[Patient],
    providers: &[Provider],
    definitions: &[Definition],
    procedures: &[Procedure],
    splits: &[PaySplit],
    from: NaiveDate,
    to: NaiveDate,
    options: &od_domain::reports_ext::UnearnedIncomeReportOptions,
) -> ReportTable {
    match options.tab {
        UnearnedIncomeTab::LineItem => unearned_line_item(
            patients,
            providers,
            definitions,
            splits,
            from,
            to,
            options.show_provider,
        ),
        UnearnedIncomeTab::Accounts => unearned_accounts(
            patients,
            providers,
            definitions,
            splits,
            from,
            to,
            options.show_provider,
        ),
        UnearnedIncomeTab::Allocation => unearned_allocation(
            patients,
            definitions,
            procedures,
            splits,
            from,
            to,
            options.exclude_zero_allocation,
            options.show_provider,
        ),
        UnearnedIncomeTab::Net => unearned_net(
            patients,
            definitions,
            procedures,
            splits,
            from,
            to,
            options.exclude_zero_net,
        ),
    }
}

fn unearned_line_item(
    patients: &[Patient],
    providers: &[Provider],
    definitions: &[Definition],
    splits: &[PaySplit],
    from: NaiveDate,
    to: NaiveDate,
    show_provider: bool,
) -> ReportTable {
    let mut headers = vec!["Date", "Patient", "Unearned Type", "Amount"];
    if show_provider {
        headers.push("Provider");
    }
    let mut table = ReportTable::new(&headers);
    let hidden = hidden_unearned_def_nums(definitions);
    let mut rows: Vec<&PaySplit> = splits
        .iter()
        .filter(|s| is_unearned(s, &hidden) && s.date_pay >= from && s.date_pay <= to)
        .collect();
    rows.sort_by_key(|s| (s.date_pay, s.pat_num, s.split_num));
    let mut total = 0.0;
    for split in rows {
        let mut values = vec![
            short_date(split.date_pay),
            patient_last_first(patients, split.pat_num),
            unearned_type_name(definitions, split.unearned_type),
            money(split.split_amount),
        ];
        if show_provider {
            values.push(provider_abbr(providers, split.provider_num));
        }
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
        total += split.split_amount;
    }
    let mut total_row = vec![String::new(), String::new(), "Total".into(), money(total)];
    if show_provider {
        total_row.push(String::new());
    }
    table.push(&total_row.iter().map(String::as_str).collect::<Vec<_>>());
    table
}

/// Total unearned for each patient (OD groups by guarantor; the port's
/// `PaySplit.PatNum` is the responsible account for unearned deposits).
fn unearned_by_patient(
    definitions: &[Definition],
    splits: &[PaySplit],
    from: NaiveDate,
    to: NaiveDate,
) -> BTreeMap<Id, f64> {
    let hidden = hidden_unearned_def_nums(definitions);
    let mut totals: BTreeMap<Id, f64> = BTreeMap::new();
    for split in splits.iter().filter(|s| {
        is_unearned(s, &hidden) && s.date_pay >= from && s.date_pay <= to
    }) {
        *totals.entry(split.pat_num).or_default() += split.split_amount;
    }
    totals
}

fn unearned_accounts(
    patients: &[Patient],
    providers: &[Provider],
    definitions: &[Definition],
    splits: &[PaySplit],
    from: NaiveDate,
    to: NaiveDate,
    show_provider: bool,
) -> ReportTable {
    let mut headers = vec!["Patient", "Unearned Amount"];
    if show_provider {
        headers.push("Provider");
    }
    let mut table = ReportTable::new(&headers);
    let hidden = hidden_unearned_def_nums(definitions);
    let totals = unearned_by_patient(definitions, splits, from, to);
    let mut grand = 0.0;
    for (pat_num, amount) in totals {
        let mut values = vec![patient_last_first(patients, pat_num), money(amount)];
        if show_provider {
            let provider = splits
                .iter()
                .find(|s| s.pat_num == pat_num && is_unearned(s, &hidden))
                .map(|s| s.provider_num)
                .unwrap_or(0);
            values.push(provider_abbr(providers, provider));
        }
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
        grand += amount;
    }
    if show_provider {
        table.push(&["Total", &money(grand), ""]);
    } else {
        table.push(&["Total", &money(grand)]);
    }
    table
}

fn remaining_production(procedures: &[Procedure], pat_num: Id, from: NaiveDate) -> f64 {
    procedures
        .iter()
        .filter(|p| {
            p.pat_num == pat_num
                && p.proc_status == ProcedureStatus::TreatmentPlanned
                && p.proc_date >= from
        })
        .map(|p| p.fee * (p.unit_qty + p.base_units).max(1.0))
        .sum()
}

fn unearned_allocation(
    patients: &[Patient],
    definitions: &[Definition],
    procedures: &[Procedure],
    splits: &[PaySplit],
    from: NaiveDate,
    to: NaiveDate,
    exclude_zero: bool,
    show_provider: bool,
) -> ReportTable {
    let mut headers = vec![
        "Patient",
        "Unearned Amount",
        "Remaining Production",
        "Allocated",
        "Unearned Remaining",
    ];
    if show_provider {
        headers.push("Provider");
    }
    let mut table = ReportTable::new(&headers);
    for (pat_num, unearned) in unearned_by_patient(definitions, splits, from, to) {
        let remaining = remaining_production(procedures, pat_num, from);
        let allocated = unearned.min(remaining);
        if exclude_zero && allocated == 0.0 {
            continue;
        }
        let mut values = vec![
            patient_last_first(patients, pat_num),
            money(unearned),
            money(remaining),
            money(allocated),
            money(unearned - allocated),
        ];
        if show_provider {
            values.push(String::new());
        }
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }
    table
}

fn unearned_net(
    patients: &[Patient],
    definitions: &[Definition],
    procedures: &[Procedure],
    splits: &[PaySplit],
    from: NaiveDate,
    to: NaiveDate,
    exclude_zero: bool,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Patient",
        "Unearned Amount",
        "Allocated",
        "Net Unearned",
    ]);
    for (pat_num, unearned) in unearned_by_patient(definitions, splits, from, to) {
        let remaining = remaining_production(procedures, pat_num, from);
        let allocated = unearned.min(remaining);
        let net = unearned - allocated;
        if exclude_zero && net == 0.0 {
            continue;
        }
        table.push(&[
            &patient_last_first(patients, pat_num),
            &money(unearned),
            &money(allocated),
            &money(net),
        ]);
    }
    table
}

/// `FormRpProcOverpaid`: procedures where the patient's payments exceed the
/// procedure's fee. Insurance payments and write-offs are folded into the fee
/// side; the current patient balance is the difference.
pub fn proc_overpaid_report(
    patients: &[Patient],
    procedures: &[Procedure],
    procedure_codes: &[ProcedureCode],
    claim_procs: &[ClaimProc],
    splits: &[PaySplit],
    from: NaiveDate,
    to: NaiveDate,
    options: &ProcOverpaidReportOptions,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Patient",
        "Proc Date",
        "Code",
        "Description",
        "Fee",
        "Insurance",
        "Patient Paid",
        "Overpaid",
    ]);
    let mut index: HashMap<Id, Vec<&ClaimProc>> = HashMap::new();
    for claim_proc in claim_procs {
        index
            .entry(claim_proc.proc_num)
            .or_default()
            .push(claim_proc);
    }
    let mut rows: Vec<&Procedure> = procedures
        .iter()
        .filter(|p| p.proc_status == ProcedureStatus::Complete)
        .filter(|p| p.proc_date >= from && p.proc_date <= to)
        .filter(|p| options.pat_num == 0 || p.pat_num == options.pat_num)
        .collect();
    rows.sort_by_key(|p| (p.pat_num, p.proc_date, p.proc_num));
    for proc in rows {
        let code = procedure_codes
            .iter()
            .find(|c| c.code_num == proc.code_num);
        let code_text = code.map(|c| c.code.clone()).unwrap_or_default();
        let description = if !proc.description_override.is_empty() {
            proc.description_override.clone()
        } else {
            code.map(|c| c.code_name.clone()).unwrap_or_default()
        };
        let fee = proc.fee * (proc.unit_qty + proc.base_units).max(1.0);
        let mut insurance = 0.0;
        for claim_proc in index.get(&proc.proc_num).into_iter().flatten() {
            insurance += claim_proc.ins_pay_amt + claim_proc.write_off;
            if options.exclude_without_insurance && claim_proc.ins_pay_amt == 0.0 && claim_proc.write_off == 0.0 {
                continue;
            }
        }
        if options.exclude_without_insurance && insurance == 0.0 {
            continue;
        }
        let patient_paid: f64 = splits
            .iter()
            .filter(|s| s.proc_num == proc.proc_num)
            .map(|s| s.split_amount)
            .sum();
        let overpaid = fee - insurance - patient_paid;
        if overpaid >= 0.0 {
            continue;
        }
        table.push(&[
            &patient_last_first(patients, proc.pat_num),
            &short_date(proc.proc_date),
            &code_text,
            &description,
            &money(fee),
            &money(insurance),
            &money(patient_paid),
            &money(overpaid),
        ]);
    }
    table
}

// ---------------------------------------------------------------------------
// Clinical / patient reports (FormRpProcNote and friends)
// ---------------------------------------------------------------------------

/// OD's "placeholders" that mark a procedure note incomplete: old-style empty
/// quotes (`""`) and `[Prompt:"..."]` tokens.
fn note_is_incomplete(note: &str) -> bool {
    if note.contains("\"\"") {
        return true;
    }
    let mut rest = note;
    while let Some(start) = rest.find("[Prompt:\"") {
        let tail = &rest[start + "[Prompt:\"".len()..];
        if let Some(end) = tail.find("\"]") {
            let token = &tail[..end];
            if !token.is_empty()
                && token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ' ')
            {
                return true;
            }
            rest = &tail[end + 2..];
        } else {
            break;
        }
    }
    false
}

fn is_broken_code(code: &ProcedureCode) -> bool {
    matches!(code.code.as_str(), "D9986" | "D9987")
}

/// `FormRpProcNote`: procedures whose latest note still contains a prompt
/// placeholder, optionally adding same-day patients with no note and patients
/// with unsigned notes. Grouping mirrors the form's radio buttons.
pub fn proc_note_report(
    patients: &[Patient],
    _providers: &[Provider],
    procedures: &[Procedure],
    procedure_codes: &[ProcedureCode],
    notes: &[ProcNote],
    from: NaiveDate,
    to: NaiveDate,
    options: &ProcNoteReportOptions,
) -> ReportTable {
    let mut headers = vec!["Date", "Patient Name", "Code", "Description"];
    headers.push("Tth");
    headers.push("Surf");
    if options.include_no_notes || options.include_unsigned {
        headers.push("Incomplete");
    }
    if options.include_no_notes {
        headers.push("No Note");
    }
    if options.include_unsigned {
        headers.push("Unsigned");
    }
    let mut table = ReportTable::new(&headers);

    // Latest note for every procedure.
    let mut latest: HashMap<Id, &ProcNote> = HashMap::new();
    for note in notes {
        latest
            .entry(note.proc_num)
            .and_modify(|existing| {
                if note.entry_datetime > existing.entry_datetime {
                    *existing = note;
                }
            })
            .or_insert(note);
    }
    // Patients with at least one real note on a given day (broken codes don't
    // count), and patients with an unsigned note on a procedure.
    let mut noted_on_day: BTreeSet<(Id, NaiveDate)> = BTreeSet::new();
    let mut unsigned_proc: BTreeSet<Id> = BTreeSet::new();
    for proc in procedures {
        if let Some(note) = latest.get(&proc.proc_num) {
            if !note.note.trim().is_empty() && !is_broken_code_code(procedure_codes, proc.code_num) {
                noted_on_day.insert((proc.pat_num, proc.proc_date));
            }
            if note.signature.trim().is_empty() && !note.note.trim().is_empty() {
                unsigned_proc.insert(proc.proc_num);
            }
        }
    }

    // Assemble the qualifying procedures, applying the same grouping as the
    // upstream SQL GROUP BY (procedure / patient / date+patient).
    let mut groups: BTreeMap<String, Vec<&Procedure>> = BTreeMap::new();
    for proc in procedures.iter().filter(|p| {
        p.proc_date >= from
            && p.proc_date <= to
            && p.proc_status == ProcedureStatus::Complete
    }) {
        let code = procedure_codes.iter().find(|c| c.code_num == proc.code_num);
        let excluded = code
            .map(is_broken_code)
            .unwrap_or(false)
            && !options.show_excluded_codes;
        if excluded {
            continue;
        }
        let note = latest.get(&proc.proc_num).copied();
        let incomplete = note.map(|n| note_is_incomplete(&n.note)).unwrap_or(false);
        let no_note = options.include_no_notes
            && !noted_on_day.contains(&(proc.pat_num, proc.proc_date));
        let unsigned = options.include_unsigned
            && note
                .map(|n| unsigned_proc.contains(&n.proc_num))
                .unwrap_or(false);
        if !incomplete && !no_note && !unsigned {
            continue;
        }
        let key = match options.group_by {
            ProcNoteGroup::Procedure => format!("p{}", proc.proc_num),
            ProcNoteGroup::Patient => format!("pat{}", proc.pat_num),
            ProcNoteGroup::DateAndPatient => format!("d{}-{}", proc.proc_date, proc.pat_num),
        };
        groups.entry(key).or_default().push(proc);
    }

    for procs in groups.values() {
        let proc_date = procs
            .iter()
            .map(|p| p.proc_date)
            .max()
            .unwrap_or_else(|| from);
        let pat_num = procs.first().map(|p| p.pat_num).unwrap_or(0);
        let (code, description, tooth, surf) = if procs.len() > 1 {
            (
                String::new(),
                "Multiple procedures".to_string(),
                String::new(),
                String::new(),
            )
        } else {
            let proc = procs[0];
            let code = procedure_codes
                .iter()
                .find(|c| c.code_num == proc.code_num)
                .map(|c| c.code.clone())
                .unwrap_or_default();
            let description = procedure_codes
                .iter()
                .find(|c| c.code_num == proc.code_num)
                .map(|c| c.code_name.clone())
                .unwrap_or_else(|| proc.description_override.clone());
            (code, description, proc.tooth_num.clone(), proc.surf.clone())
        };
        let mut values = vec![
            short_date(proc_date),
            patient_name(patients, pat_num),
            code,
            description,
            tooth,
            surf,
        ];
        if options.include_no_notes || options.include_unsigned {
            let incomplete = procs.iter().any(|p| {
                latest
                    .get(&p.proc_num)
                    .map(|n| note_is_incomplete(&n.note))
                    .unwrap_or(false)
            });
            values.push(incomplete.to_string());
        }
        if options.include_no_notes {
            values.push(
                procs
                    .iter()
                    .any(|p| !noted_on_day.contains(&(p.pat_num, p.proc_date)))
                    .to_string(),
            );
        }
        if options.include_unsigned {
            values.push(
                procs
                    .iter()
                    .any(|p| unsigned_proc.contains(&p.proc_num))
                    .to_string(),
            );
        }
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }
    table
}

fn is_broken_code_code(procedure_codes: &[ProcedureCode], code_num: Id) -> bool {
    procedure_codes
        .iter()
        .find(|c| c.code_num == code_num)
        .map(is_broken_code)
        .unwrap_or(false)
}

/// `FormRpDentalSealantMeasure`: per-provider UDS sealant numerator and
/// denominator for a reporting year, plus a totals row. The patient age band is
/// 6–9 years at the end of the year; a numerator patient has a sealant
/// (`D1351` or SNOMED 234713009) on a first molar (14, 3, 19, 30).
pub fn dental_sealant_measure_report(
    patients: &[Patient],
    providers: &[Provider],
    procedures: &[Procedure],
    procedure_codes: &[ProcedureCode],
    year: i32,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Provider",
        "Numerator",
        "Denominator",
        "Percentage Met",
    ]);
    let start = NaiveDate::from_ymd_opt(year, 1, 1).unwrap_or_else(|| NaiveDate::MIN);
    let end = NaiveDate::from_ymd_opt(year, 12, 31).unwrap_or_else(|| NaiveDate::MAX);
    let dob_start = NaiveDate::from_ymd_opt(year - 9, 1, 1).unwrap_or_else(|| NaiveDate::MIN);
    let dob_end = NaiveDate::from_ymd_opt(year - 6, 12, 31).unwrap_or_else(|| NaiveDate::MAX);

    let is_initial = |proc: &Procedure| {
        let code = procedure_codes
            .iter()
            .find(|c| c.code_num == proc.code_num)
            .map(|c| c.code.clone())
            .unwrap_or_default();
        const MEDICAL: [&str; 18] = [
            "99201", "99202", "99203", "99204", "99205", "99212", "99213", "99214", "99215",
            "99391", "99392", "99393", "99394", "99381", "99382", "99383", "99384", "99395",
        ];
        const CDT: [&str; 8] = [
            "D0120", "D0145", "D0150", "D0180", "D0191", "D0140", "D0160", "D0170",
        ];
        MEDICAL.contains(&proc.medical_code.as_str()) || CDT.contains(&code.as_str())
    };
    // Denominator patients are the age-eligible children who had a qualifying
    // exam or sealant-related service in the reporting year. The code lists
    // mirror upstream's UDS `whereClause`; a sealant is not also an exam code,
    // so these must be OR-ed rather than AND-ed.
    let is_denominator_proc = |proc: &Procedure| {
        let code = procedure_codes
            .iter()
            .find(|c| c.code_num == proc.code_num)
            .map(|c| c.code.clone())
            .unwrap_or_default();
        const DENOM: [&str; 8] = [
            "D0120", "D0145", "D0150", "D0180", "D0191", "D0602", "D0603", "D1351",
        ];
        const SNOMED: [&str; 4] = [
            "609399004",
            "609401005",
            "609402003",
            "609403008",
        ];
        DENOM.contains(&code.as_str()) || SNOMED.contains(&proc.snomed_body_site.as_str())
    };

    let in_band = |pat: &Patient| {
        pat.birth_date
            .map(|b| b >= dob_start && b <= dob_end)
            .unwrap_or(false)
    };

    let mut provider_rows: BTreeMap<Id, (usize, usize)> = BTreeMap::new();
    for provider in providers {
        let mut numerator: BTreeSet<Id> = BTreeSet::new();
        let mut denominator: BTreeSet<Id> = BTreeSet::new();
        let candidates: Vec<&Procedure> = procedures
            .iter()
            .filter(|p| p.provider_num == provider.provider_num)
            .filter(|p| p.proc_status == ProcedureStatus::Complete)
            .filter(|p| p.proc_date >= start && p.proc_date <= end)
            .filter(|p| {
                patients
                    .iter()
                    .find(|pat| pat.pat_num == p.pat_num)
                    .map(in_band)
                    .unwrap_or(false)
            })
            .collect();
        let eligible: BTreeSet<Id> = candidates
            .iter()
            .filter(|p| is_initial(p))
            .map(|p| p.pat_num)
            .collect();
        for proc in candidates.iter().filter(|p| eligible.contains(&p.pat_num)) {
            if is_denominator_proc(proc) {
                denominator.insert(proc.pat_num);
            }
        }
        for proc in candidates.iter() {
            let code = procedure_codes
                .iter()
                .find(|c| c.code_num == proc.code_num)
                .map(|c| c.code.clone())
                .unwrap_or_default();
            let is_first_molar = matches!(proc.tooth_num.as_str(), "14" | "3" | "19" | "30");
            if is_first_molar
                && (code == "D1351" || proc.snomed_body_site == "234713009")
                && denominator.contains(&proc.pat_num)
            {
                numerator.insert(proc.pat_num);
            }
        }
        provider_rows.insert(
            provider.provider_num,
            (numerator.len(), denominator.len()),
        );
    }
    let (mut total_num, mut total_den) = (0usize, 0usize);
    for (provider_num, (num, den)) in provider_rows {
        total_num += num;
        total_den += den;
        let pct = if den == 0 {
            0.0
        } else {
            num as f64 / den as f64 * 100.0
        };
        table.push(&[
            &provider_name(providers, provider_num),
            &count(num),
            &count(den),
            &format!("{pct:.2}%"),
        ]);
    }
    let pct = if total_den == 0 {
        0.0
    } else {
        total_num as f64 / total_den as f64 * 100.0
    };
    table.push(&[
        "",
        &format!("Total Numerator: {}", count(total_num)),
        &format!("Total Denominator: {}", count(total_den)),
        &format!("Total Percentage Met: {pct:.2}%"),
    ]);
    table
}

// ---------------------------------------------------------------------------
// Production and statistics (FormRpProdGoal, FormRpProviderPayroll,
// FormRpPresentedTreatmentProduction, FormRpTreatPlanPresentationStatistics)
// ---------------------------------------------------------------------------

fn proc_production(proc: &Procedure) -> f64 {
    proc.fee * (proc.unit_qty + proc.base_units).max(1.0)
}

fn cap_write_off<'a>(claim_procs: impl Iterator<Item = &'a ClaimProc>, proc_num: Id) -> f64 {
    claim_procs
        .filter(|cp| cp.proc_num == proc_num && cp.status == 7)
        .map(|cp| cp.write_off)
        .sum()
}

fn proc_write_offs<'a>(claim_procs: impl Iterator<Item = &'a ClaimProc>, proc_num: Id) -> f64 {
    claim_procs
        .filter(|cp| cp.proc_num == proc_num && matches!(cp.status, 0 | 1 | 4))
        .map(|cp| cp.write_off)
        .sum()
}

/// Merge a provider's scheduled time ranges for one (date, provider, clinic)
/// group, then sum the non-overlapping hours. Upstream merges pairwise and
/// takes the first row's hourly goal; this uses the same ranges but resolves
/// chained overlaps in one pass, which gives the intended de-duplication when
/// a provider is booked in more than one operatory at the same time.
fn scheduled_hours(ranges: &[(chrono::NaiveTime, chrono::NaiveTime)]) -> f64 {
    let mut sorted: Vec<(chrono::NaiveTime, chrono::NaiveTime)> = ranges
        .iter()
        .copied()
        .filter(|(start, stop)| stop > start)
        .collect();
    sorted.sort();
    let mut merged: Vec<(chrono::NaiveTime, chrono::NaiveTime)> = Vec::new();
    for (start, stop) in sorted {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(stop);
                continue;
            }
        }
        merged.push((start, stop));
    }
    merged
        .iter()
        .map(|(s, e)| (*e - *s).num_seconds() as f64 / 3600.0)
        .sum()
}

/// `FormRpProdGoal`: per-day production, scheduled value, adjustments and
/// write-offs across the range, with the production goal supplied by OD's
/// provider schedule (`schedule` + `scheduleop` + `provider.HourlyProdGoalAmt`).
/// Only `ScheduleType.Provider` (1) rows with `SchedStatus.Open` (0) count, so
/// practice-level holidays and closed/blockout rows contribute zero hours.
/// The goal is deliberately excluded from `Tot Prod`, matching upstream.
#[allow(clippy::too_many_arguments)]
pub fn production_goal_report(
    procedures: &[Procedure],
    appointments: &[Appointment],
    adjustments: &[Adjustment],
    claim_procs: &[ClaimProc],
    schedules: &[Schedule],
    schedule_ops: &[ScheduleOp],
    providers: &[Provider],
    operatories: &[Operatory],
    from: NaiveDate,
    to: NaiveDate,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Date",
        "Weekday",
        "Production",
        "Prod Goal",
        "Scheduled",
        "Adjusts",
        "Write-off Est",
        "Write-off Adj",
        "Tot Prod",
    ]);
    let mut bucket: BTreeMap<NaiveDate, [f64; 6]> = BTreeMap::new(); // prod, sched, adj, woEst, woAdj, goal
    let mut day = from;
    while day <= to {
        bucket.entry(day).or_default();
        day += chrono::Duration::days(1);
    }
    for proc in procedures
        .iter()
        .filter(|p| p.proc_status == ProcedureStatus::Complete)
        .filter(|p| p.proc_date >= from && p.proc_date <= to)
    {
        let b = bucket.entry(proc.proc_date).or_default();
        b[0] += proc_production(proc);
        b[3] += cap_write_off(claim_procs.iter(), proc.proc_num);
    }
    for adj in adjustments
        .iter()
        .filter(|a| a.adj_date >= from && a.adj_date <= to)
    {
        bucket.entry(adj.adj_date).or_default()[2] += adj.adj_amount;
    }
    for apt in appointments.iter().filter(|a| {
        a.apt_status == AppointmentStatus::Scheduled
            && a.apt_datetime.date() >= from
            && a.apt_datetime.date() <= to
    }) {
        bucket.entry(apt.apt_datetime.date()).or_default()[1] += 0.0;
    }
    for cp in claim_procs.iter().filter(|cp| cp.status == 1 || cp.status == 4) {
        let cp_date = cp.fee_billed; // placeholder; date not modelled on ClaimProc
        let _ = cp_date;
    }

    // Group open provider schedule blocks by (date, provider, clinic), exactly
    // as RpProdGoal does, so overlapping operatory bookings are counted once.
    let mut ranges: BTreeMap<(NaiveDate, Id, Id), Vec<(chrono::NaiveTime, chrono::NaiveTime)>> =
        BTreeMap::new();
    let mut goals: BTreeMap<(NaiveDate, Id, Id), f64> = BTreeMap::new();
    for sched in schedules.iter().filter(|s| {
        s.sched_type == 1 && s.status == 0 && s.sched_date >= from && s.sched_date <= to
    }) {
        let clinic = if sched.clinic_num != 0 {
            sched.clinic_num
        } else {
            schedule_ops
                .iter()
                .find(|op| op.schedule_num == sched.schedule_num)
                .and_then(|op| operatories.iter().find(|o| o.operatory_num == op.operatory_num))
                .map(|o| o.clinic_num)
                .unwrap_or(0)
        };
        let key = (sched.sched_date, sched.prov_num, clinic);
        ranges
            .entry(key)
            .or_default()
            .push((sched.start_time, sched.stop_time));
        goals.entry(key).or_insert_with(|| {
            providers
                .iter()
                .find(|p| p.provider_num == sched.prov_num)
                .map(|p| p.hourly_prod_goal_amt)
                .unwrap_or(0.0)
        });
    }
    for (key, spans) in ranges {
        let hourly = goals.get(&key).copied().unwrap_or(0.0);
        bucket.entry(key.0).or_default()[5] += scheduled_hours(&spans) * hourly;
    }

    for (date, b) in bucket {
        let [prod, sched, adj, wo_est, wo_adj, goal] = b;
        let total = prod + sched + adj - wo_est - wo_adj;
        table.push(&[
            &short_date(date),
            &date.weekday().to_string(),
            &money(prod),
            &money(goal),
            &money(sched),
            &money(adj),
            &money(wo_est),
            &money(wo_adj),
            &money(total),
        ]);
    }
    table
}

#[derive(Default, Clone, Copy)]
struct PayrollTotals {
    production: f64,
    wo_est: f64,
    adj: f64,
    wo_change: f64,
    alloc: f64,
    unalloc: f64,
    ins: f64,
    ins_not_final: f64,
}

fn add_payroll(
    rows: &mut BTreeMap<(Id, Id, NaiveDate), PayrollTotals>,
    provider: Id,
    pat: Id,
    date: NaiveDate,
    f: impl FnOnce(&mut PayrollTotals),
) {
    f(rows.entry((provider, pat, date)).or_default());
}

/// `FormRpProviderPayroll`: production and income rolled up by provider and day
/// (or by patient in detailed mode, selected through `detailed`).
///
/// OD's `claimsnapshot` supplies the historic write-off estimate captured when
/// the claim was created, so later edits to `claimproc` cannot rewrite a past
/// payroll report. Following upstream's `RpProdInc.GetProviderPayrollDataSet`:
/// the estimate is booked on `procedurelog.DateComplete` (negated), while the
/// "change in write-off" and insurance income are booked on
/// `claimproc.DateSuppReceived`. A snapshot write-off of `-1` is treated as
/// zero, as in upstream's `IFNULL(NULLIF(WriteOff, -1), 0)`. Insurance income
/// with `ClaimPaymentNum = 0` is reported under "Ins Not Final" instead of
/// "Ins Income", matching upstream's two income tables.
#[allow(clippy::too_many_arguments)]
pub fn provider_payroll_report(
    patients: &[Patient],
    providers: &[Provider],
    procedures: &[Procedure],
    claim_procs: &[ClaimProc],
    snapshots: &[ClaimSnapshot],
    adjustments: &[Adjustment],
    splits: &[PaySplit],
    from: NaiveDate,
    to: NaiveDate,
    detailed: bool,
) -> ReportTable {
    let mut headers = vec![
        "Date",
        if detailed { "Patient" } else { "Day" },
        "UCR Production",
        "Est Write-off",
        "Prod Adj",
        "Change in Write-off",
        "Net Prod(NPR)",
        "Pat Inc Alloc",
        "Pat Inc Unalloc",
        "Ins Income",
        "Ins Not Final",
        "Net Income",
    ];
    if headers.len() != 12 {
        headers.truncate(12);
    }
    let mut table = ReportTable::new(&headers);
    let mut rows: BTreeMap<(Id, Id, NaiveDate), PayrollTotals> = BTreeMap::new();
    for proc in procedures
        .iter()
        .filter(|p| p.proc_status == ProcedureStatus::Complete)
        .filter(|p| p.proc_date >= from && p.proc_date <= to)
    {
        add_payroll(&mut rows, proc.provider_num, proc.pat_num, proc.proc_date, |t| {
            t.production += proc_production(proc);
        });
    }
    for cp in claim_procs {
        let proc = procedures.iter().find(|p| p.proc_num == cp.proc_num);
        let provider = proc.map(|p| p.provider_num).unwrap_or(0);
        let pat = proc.map(|p| p.pat_num).unwrap_or(0);
        // OD stores one snapshot per claimproc for payroll purposes; if
        // several exist (different triggers), use the most recent one.
        let snapshot_wo = snapshots
            .iter()
            .filter(|s| s.claim_proc_num == cp.claim_proc_num)
            .max_by_key(|s| s.date_t_entry)
            .map(|s| if s.write_off == -1.0 { 0.0 } else { s.write_off })
            .unwrap_or(0.0);
        // `tableInsWOEst`: procedure completion date, negated estimate.
        if let Some(proc) = proc.filter(|p| p.proc_date >= from && p.proc_date <= to) {
            add_payroll(&mut rows, provider, pat, proc.proc_date, |t| {
                t.wo_est -= snapshot_wo;
            });
        }
        // `tableInsWriteOff` and ins income: `DateSuppReceived` is both the
        // booking date and the report's date filter. With no snapshot the
        // LEFT JOIN contributes zero, so the variance is simply -WriteOff.
        if let Some(date) = cp.date_supp_received.filter(|d| *d >= from && *d <= to) {
            add_payroll(&mut rows, provider, pat, date, |t| {
                t.wo_change += snapshot_wo - cp.write_off;
                // Upstream's income tables: `ClaimPaymentNum = 0` rows go to
                // "Ins Not Final" regardless of status; only finalized rows
                // in Received/Supplemental count as "Ins Income".
                if cp.claim_payment_num == 0 {
                    t.ins_not_final += cp.ins_pay_amt;
                } else if matches!(cp.status, 1 | 4) {
                    t.ins += cp.ins_pay_amt;
                }
            });
        }
    }
    for adj in adjustments
        .iter()
        .filter(|a| a.adj_date >= from && a.adj_date <= to)
    {
        add_payroll(&mut rows, adj.provider_num, adj.pat_num, adj.adj_date, |t| {
            t.adj += adj.adj_amount
        });
    }
    for split in splits
        .iter()
        .filter(|s| s.date_pay >= from && s.date_pay <= to)
    {
        let date = split.date_pay;
        add_payroll(&mut rows, split.provider_num, split.pat_num, date, |t| {
            if split.proc_num == 0 {
                t.unalloc += split.split_amount;
            } else {
                t.alloc += split.split_amount;
            }
        });
    }
    for ((provider, pat, date), t) in rows {
        let second = if detailed {
            patient_name(patients, pat)
        } else {
            date.weekday().to_string()
        };
        let net_prod = t.production + t.wo_est + t.adj + t.wo_change;
        let net_income = t.alloc + t.ins;
        let _ = provider_abbr(providers, provider); // provider is a grouping key upstream
        table.push(&[
            &short_date(date),
            &second,
            &money(t.production),
            &money(t.wo_est),
            &money(t.adj),
            &money(t.wo_change),
            &money(net_prod),
            &money(t.alloc),
            &money(t.unalloc),
            &money(t.ins),
            &money(t.ins_not_final),
            &money(net_income),
        ]);
    }
    table
}

/// `FormRpPresentedTreatmentProduction`: net production of completed procedures
/// that appeared on a saved treatment plan, grouped by presenter. The port maps
/// presenter to the plan's provider because OD's `UserNumPresenter` is not
/// modelled separately.
pub fn presented_treatment_production_report(
    patients: &[Patient],
    providers: &[Provider],
    procedures: &[Procedure],
    plans: &[TreatPlan],
    plan_lines: &[TreatPlanLine],
    claim_procs: &[ClaimProc],
    adjustments: &[Adjustment],
    from: NaiveDate,
    to: NaiveDate,
    detailed: bool,
) -> ReportTable {
    let headers: Vec<&str> = if detailed {
        vec![
            "Presenter",
            "Date Presented",
            "Date Completed",
            "Descript",
            "GrossProd",
            "Write-offs",
            "Adjustments",
            "NetProduction",
        ]
    } else {
        vec![
            "Presenter",
            "# of Procs",
            "GrossProd",
            "Write-offs",
            "Adjustments",
            "NetProduction",
        ]
    };
    let mut table = ReportTable::new(&headers);
    let mut planned: HashMap<Id, &TreatPlan> = HashMap::new();
    for line in plan_lines {
        let key = line.proc_num;
        planned
            .entry(key)
            .and_modify(|existing| {
                let candidate_date = line.date_tp.or(existing.date_saved.map(|d| d.date()));
                let existing_date = existing.date_saved.map(|d| d.date());
                if candidate_date > existing_date {
                    if let Some(plan) = plans.iter().find(|p| p.treat_plan_num == line.treat_plan_num) {
                        *existing = plan;
                    }
                }
            })
            .or_insert_with(|| {
                plans
                    .iter()
                    .find(|p| p.treat_plan_num == line.treat_plan_num)
                    .unwrap_or_else(|| &plans[0])
            });
    }
    let _ = patients;
    let mut groups: BTreeMap<Id, Vec<(&Procedure, &TreatPlan)>> = BTreeMap::new();
    for proc in procedures
        .iter()
        .filter(|p| p.proc_status == ProcedureStatus::Complete)
        .filter(|p| p.proc_date >= from && p.proc_date <= to)
    {
        if let Some(plan) = planned.get(&proc.proc_num) {
            groups.entry(plan.provider_num).or_default().push((proc, plan));
        }
    }
    for (presenter, entries) in groups {
        let presenter_name = if presenter == 0 {
            "None".to_string()
        } else {
            provider_abbr(providers, presenter)
        };
        if detailed {
            for (proc, plan) in entries {
                let gross = proc_production(proc) - cap_write_off(claim_procs.iter(), proc.proc_num);
                let wo = proc_write_offs(claim_procs.iter(), proc.proc_num);
                let adj: f64 = adjustments
                    .iter()
                    .filter(|a| a.proc_num == proc.proc_num)
                    .map(|a| a.adj_amount)
                    .sum();
                table.push(&[
                    &presenter_name,
                    &plan
                        .date_saved
                        .map(|d| short_date(d.date()))
                        .unwrap_or_default(),
                    &short_date(proc.proc_date),
                    &proc.description_override,
                    &money(gross),
                    &money(wo),
                    &money(adj),
                    &money(gross - wo + adj),
                ]);
            }
        } else {
            let mut gross = 0.0;
            let mut wo = 0.0;
            let mut adj = 0.0;
            for (proc, _) in &entries {
                gross += proc_production(proc) - cap_write_off(claim_procs.iter(), proc.proc_num);
                wo += proc_write_offs(claim_procs.iter(), proc.proc_num);
                adj += adjustments
                    .iter()
                    .filter(|a| a.proc_num == proc.proc_num)
                    .map(|a| a.adj_amount)
                    .sum::<f64>();
            }
            table.push(&[
                &presenter_name,
                &count(entries.len()),
                &money(gross),
                &money(wo),
                &money(adj),
                &money(gross - wo + adj),
            ]);
        }
    }
    table
}

/// `FormRpTreatPlanPresentationStatistics`: totals per presenter of saved plans
/// presented in the date range, with planned/scheduled/completed counts and
/// gross or net amounts (selected through `gross`).
pub fn treat_plan_presentation_statistics_report(
    providers: &[Provider],
    plans: &[TreatPlan],
    plan_lines: &[TreatPlanLine],
    procedures: &[Procedure],
    claim_procs: &[ClaimProc],
    adjustments: &[Adjustment],
    appointments: &[Appointment],
    from: NaiveDate,
    to: NaiveDate,
    gross: bool,
) -> ReportTable {
    let suffix = if gross { "Gross" } else { "Net" };
    let mut table = ReportTable::new(&[
        "Presenter",
        "# of Plans",
        "# of Procs",
        "# of ProcsSched",
        "# of ProcsComp",
        &format!("{suffix}TPAmt"),
        &format!("{suffix}SchedAmt"),
        &format!("{suffix}CompAmt"),
    ]);
    let saved: Vec<&TreatPlan> = plans
        .iter()
        .filter(|p| p.status == TreatPlanStatus::Saved)
        .filter(|p| p.date_saved.map(|d| d.date()).map_or(false, |d| d >= from && d <= to))
        .collect();
    let plan_nums: BTreeSet<Id> = saved.iter().map(|p| p.treat_plan_num).collect();
    // First presented plan wins for a procedure, matching upstream's dedup.
    let mut seen: BTreeSet<Id> = BTreeSet::new();
    let mut groups: BTreeMap<Id, Vec<(&Procedure, Id)>> = BTreeMap::new();
    for line in plan_lines
        .iter()
        .filter(|l| plan_nums.contains(&l.treat_plan_num))
    {
        if !seen.insert(line.proc_num) {
            continue;
        }
        let plan = saved
            .iter()
            .find(|p| p.treat_plan_num == line.treat_plan_num)
            .unwrap();
        if let Some(proc) = procedures.iter().find(|p| p.proc_num == line.proc_num) {
            groups
                .entry(plan.provider_num)
                .or_default()
                .push((proc, plan.treat_plan_num));
        }
    }
    for (presenter, entries) in groups {
        let mut plan_set: BTreeSet<Id> = BTreeSet::new();
        let (mut sched_count, mut comp_count) = (0usize, 0usize);
        let (mut tp_amt, mut sched_amt, mut comp_amt) = (0.0, 0.0, 0.0);
        for (proc, plan_num) in &entries {
            plan_set.insert(*plan_num);
            let mut amount = proc_production(proc) - cap_write_off(claim_procs.iter(), proc.proc_num);
            if proc.proc_status == ProcedureStatus::Complete {
                comp_count += 1;
                amount -= proc_write_offs(claim_procs.iter(), proc.proc_num);
                comp_amt += amount;
            } else if proc.proc_status == ProcedureStatus::TreatmentPlanned {
                let is_sched = appointments.iter().any(|a| {
                    a.pat_num == proc.pat_num
                        && a.apt_status == AppointmentStatus::Scheduled
                });
                if is_sched {
                    sched_count += 1;
                    sched_amt += amount;
                }
            }
            if !gross {
                let adj: f64 = adjustments
                    .iter()
                    .filter(|a| a.proc_num == proc.proc_num)
                    .map(|a| a.adj_amount)
                    .sum();
                amount -= adj;
                if proc.proc_status == ProcedureStatus::Complete {
                    comp_amt -= adj;
                } else if proc.proc_status == ProcedureStatus::TreatmentPlanned {
                    sched_amt -= adj;
                }
            }
            tp_amt += amount;
        }
        let presenter_name = if presenter == 0 {
            "None".to_string()
        } else {
            provider_abbr(providers, presenter)
        };
        table.push(&[
            &presenter_name,
            &count(plan_set.len()),
            &count(entries.len()),
            &count(sched_count),
            &count(comp_count),
            &money(tp_amt),
            &money(sched_amt),
            &money(comp_amt),
        ]);
    }
    table
}

// ---------------------------------------------------------------------------
// Receivables / service-date views (FormRpReceivablesBreakdown,
// FormRpServiceDateView)
// ---------------------------------------------------------------------------

/// `FormRpReceivablesBreakdown`: patient receivables grouped by billing type
/// and aged by procedure date, with insurance columns hidden when the port's
/// insurance toggle is off.
pub fn receivables_breakdown_report(
    patients: &[Patient],
    definitions: &[Definition],
    procedures: &[Procedure],
    splits: &[PaySplit],
    claim_procs: &[ClaimProc],
    as_of: NaiveDate,
    insurance_enabled: bool,
) -> ReportTable {
    let mut headers = vec![
        "Billing Type",
        "0-30",
        "31-60",
        "61-90",
        "Over 90",
        "Total",
    ];
    if insurance_enabled {
        headers.push("Ins Est");
    }
    let mut table = ReportTable::new(&headers);
    let mut buckets: BTreeMap<Id, [f64; 5]> = BTreeMap::new();
    let mut paid_by_proc: HashMap<Id, f64> = HashMap::new();
    for split in splits {
        *paid_by_proc.entry(split.proc_num).or_default() += split.split_amount;
    }
    let mut ins_est_by_pat: HashMap<Id, f64> = HashMap::new();
    if insurance_enabled {
        for cp in claim_procs.iter().filter(|cp| matches!(cp.status, 0 | 6)) {
            *ins_est_by_pat.entry(cp.proc_num).or_default() += cp.ins_pay_est;
        }
    }
    for proc in procedures
        .iter()
        .filter(|p| p.proc_status == ProcedureStatus::Complete)
    {
        let pat = match patients.iter().find(|pat| pat.pat_num == proc.pat_num) {
            Some(pat) => pat,
            None => continue,
        };
        let age_days = (as_of - proc.proc_date).num_days().max(0);
        let bucket_index = match age_days {
            0..=30 => 0,
            31..=60 => 1,
            61..=90 => 2,
            _ => 3,
        };
        let paid = paid_by_proc.get(&proc.proc_num).copied().unwrap_or(0.0);
        let outstanding = (proc_production(proc) - paid).max(0.0);
        let entry = buckets.entry(pat.billing_type).or_default();
        entry[bucket_index] += outstanding;
        entry[4] += outstanding;
    }
    for (billing_type, amounts) in buckets {
        let label = definitions
            .iter()
            .find(|d| d.category == DefinitionCategory::BILLING_TYPES && d.def_num == billing_type)
            .map(|d| d.item_name.clone())
            .unwrap_or_else(|| "Unassigned".into());
        let mut values: Vec<String> = amounts.iter().map(|a| money(*a)).collect();
        values.insert(0, label);
        if insurance_enabled {
            let ins: f64 = ins_est_by_pat.values().sum();
            values.push(money(ins));
        }
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }
    table
}

/// `FormRpServiceDateView`: one row per procedure service date with charges,
/// payments, insurance write-offs and the resulting balance.
pub fn service_date_view_report(
    patients: &[Patient],
    procedures: &[Procedure],
    splits: &[PaySplit],
    claim_procs: &[ClaimProc],
    from: NaiveDate,
    to: NaiveDate,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Service Date",
        "Patient",
        "Charges",
        "Payments",
        "Write-offs",
        "Balance",
    ]);
    let mut paid_by_proc: HashMap<Id, f64> = HashMap::new();
    for split in splits {
        *paid_by_proc.entry(split.proc_num).or_default() += split.split_amount;
    }
    let mut wo_by_proc: HashMap<Id, f64> = HashMap::new();
    for cp in claim_procs {
        *wo_by_proc.entry(cp.proc_num).or_default() += cp.write_off;
    }
    let mut rows: Vec<&Procedure> = procedures
        .iter()
        .filter(|p| p.proc_status == ProcedureStatus::Complete)
        .filter(|p| p.proc_date >= from && p.proc_date <= to)
        .collect();
    rows.sort_by_key(|p| (p.proc_date, p.pat_num, p.proc_num));
    for proc in rows {
        let charges = proc_production(proc);
        let paid = paid_by_proc.get(&proc.proc_num).copied().unwrap_or(0.0);
        let wo = wo_by_proc.get(&proc.proc_num).copied().unwrap_or(0.0);
        table.push(&[
            &short_date(proc.proc_date),
            &patient_last_first(patients, proc.pat_num),
            &money(charges),
            &money(paid),
            &money(wo),
            &money(charges - paid - wo),
        ]);
    }
    table
}

// ---------------------------------------------------------------------------
// Plans, public-health extracts and print/sheet reports
// ---------------------------------------------------------------------------

/// One discount-plan subscriber row, joined from OD's `discountplan` /
/// `discountplansub` / `feesched` / `definition` tables. Plans and subscribers
/// are editable in Setup → Discount plans.
#[derive(Clone, Debug, Default)]
pub struct DiscountPlanSubscription {
    pub description: String,
    pub fee_schedule: String,
    pub adj_type: String,
    pub date_effective: Option<NaiveDate>,
    pub date_term: Option<NaiveDate>,
    pub patient: String,
}

/// `FormRpDiscountPlan`: description-substring filter over discount-plan
/// subscribers, ordered by plan then patient.
pub fn discount_plan_report(
    subscriptions: &[DiscountPlanSubscription],
    description_contains: &str,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Description",
        "FeeSched",
        "AdjType",
        "DateEffective",
        "DateTerm",
        "Patient",
    ]);
    let needle = description_contains.to_ascii_lowercase();
    let mut rows: Vec<&DiscountPlanSubscription> = subscriptions
        .iter()
        .filter(|s| s.description.to_ascii_lowercase().contains(&needle))
        .collect();
    rows.sort_by(|a, b| {
        a.description
            .cmp(&b.description)
            .then(a.patient.cmp(&b.patient))
    });
    for row in rows {
        table.push(&[
            &row.description,
            &row.fee_schedule,
            &row.adj_type,
            &row.date_effective.map(short_date).unwrap_or_default(),
            &row.date_term.map(short_date).unwrap_or_default(),
            &row.patient,
        ]);
    }
    table
}

/// `FormRpDPPOvercharged`: dynamic payment plans whose debits exceed the
/// patient's portion, reported per linked procedure/adjustment.
///
/// Uses OD's `payplanlink` rows when present. Each link's patient portion is
/// the procedure fee (or adjustment amount) plus procedure adjustments minus
/// insurance, with payments outside the plan deducted and `AmountOverride`
/// honoured; `amtOvercharged` compares that to the debit `payplancharge`s
/// grouped by `FKey`/`LinkType`. Legacy plans with no links fall back to the
/// completed-amount approximation used by earlier versions of this port.
#[allow(clippy::too_many_arguments)]
pub fn dpp_overcharged_report(
    patients: &[Patient],
    providers: &[Provider],
    procedures: &[Procedure],
    codes: &[ProcedureCode],
    adjustments: &[Adjustment],
    claim_procs: &[ClaimProc],
    links: &[PayPlanLink],
    plans: &[PayPlan],
    charges: &[PayPlanCharge],
    splits: &[PaySplit],
    from: NaiveDate,
    to: NaiveDate,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Start Date",
        "Patient",
        "Guarantor",
        "Provider",
        "Description",
        "Overridden",
        "Pat Portion",
        "Pat Paid Outside Plan",
        "Pat Portion On Plan",
        "Plan Debits",
        "Overcharged",
        "Pat Paid On Plan",
        "Pat Overpaid",
    ]);
    let mut selected: Vec<&PayPlan> = plans
        .iter()
        .filter(|p| {
            p.date_pay_plan_start
                .map(|d| d >= from && d <= to)
                .unwrap_or(false)
        })
        .collect();
    selected.sort_by_key(|p| (p.date_pay_plan_start, p.pay_plan_num));

    let abbreviation = |num: Id| {
        providers
            .iter()
            .find(|p| p.provider_num == num)
            .map(|p| p.abbreviation.clone())
            .unwrap_or_default()
    };

    for plan in selected {
        let plan_links: Vec<&PayPlanLink> = links
            .iter()
            .filter(|l| l.pay_plan_num == plan.pay_plan_num)
            .collect();
        if plan_links.is_empty() {
            // Legacy plan: no `payplanlink` rows exist, so keep the historical
            // completed-amount approximation.
            let plan_debits: f64 = charges
                .iter()
                .filter(|c| {
                    c.pay_plan_num == plan.pay_plan_num
                        && c.charge_type == PayPlanChargeType::Debit
                })
                .map(|c| c.principal)
                .sum();
            let paid_on_plan: f64 = splits
                .iter()
                .filter(|s| s.pay_plan_num == plan.pay_plan_num)
                .map(|s| s.split_amount)
                .sum();
            let paid_outside: f64 = splits
                .iter()
                .filter(|s| s.pat_num == plan.pat_num && s.proc_num != 0 && s.pay_plan_num == 0)
                .map(|s| s.split_amount)
                .sum();
            let portion_on_plan = round2(plan.completed_amt - paid_outside);
            let overcharged = (-round2(portion_on_plan - plan_debits)).max(0.0);
            if overcharged == 0.0 {
                continue;
            }
            let pat_overpaid = (-round2(portion_on_plan - paid_on_plan)).max(0.0);
            table.push(&[
                &plan.date_pay_plan_start.map(short_date).unwrap_or_default(),
                &patient_last_first(patients, plan.pat_num),
                &patient_last_first(patients, plan.guarantor),
                &providers.first().map(|p| p.abbreviation.clone()).unwrap_or_default(),
                &"",
                &"",
                &money(plan.completed_amt),
                &money(paid_outside),
                &money(portion_on_plan),
                &money(plan_debits),
                &money(overcharged),
                &money(paid_on_plan),
                &money(pat_overpaid),
            ]);
            continue;
        }

        for link in plan_links {
            let (pat_portion, description, provider_num) = match link.link_type {
                PayPlanLink::ADJUSTMENT => {
                    let adj = adjustments.iter().find(|a| a.adj_num == link.fkey);
                    (
                        adj.map(|a| a.adj_amount).unwrap_or(0.0),
                        "adjustment".to_string(),
                        adj.map(|a| a.provider_num).unwrap_or(0),
                    )
                }
                PayPlanLink::PROCEDURE => {
                    let proc = procedures.iter().find(|p| p.proc_num == link.fkey);
                    if let Some(proc) = proc {
                        let proc_adj: f64 = adjustments
                            .iter()
                            .filter(|a| a.proc_num == proc.proc_num)
                            .map(|a| a.adj_amount)
                            .sum();
                        let ins: f64 = claim_procs
                            .iter()
                            .filter(|cp| cp.proc_num == proc.proc_num)
                            .map(|cp| match cp.status {
                                // `GetInsPaidStatuses`: Received, Supplemental,
                                // CapClaim, CapComplete.
                                1 | 4 | 5 | 7 => cp.ins_pay_amt + cp.write_off,
                                // `GetEstimatedStatuses`: NotReceived, Estimate,
                                // CapEstimate, InsHist. Upstream adds the
                                // override-then-estimate-then-0 write-off.
                                0 | 6 | 8 | 9 => {
                                    let wo_est = if cp.write_off_est_override != -1.0 {
                                        cp.write_off_est_override
                                    } else if cp.write_off_est != -1.0 {
                                        cp.write_off_est
                                    } else {
                                        0.0
                                    };
                                    cp.ins_pay_est + wo_est
                                }
                                _ => 0.0,
                            })
                            .sum();
                        // OD: ProcFee * GREATEST(1, BaseUnits + UnitQty).
                        let units = (proc.base_units + proc.unit_qty).max(1.0);
                        let fee = proc.fee * units;
                        let code = codes
                            .iter()
                            .find(|c| c.code_num == proc.code_num)
                            .map(|c| c.code.clone())
                            .unwrap_or_default();
                        (fee + proc_adj - ins, code, proc.provider_num)
                    } else {
                        (0.0, String::new(), 0)
                    }
                }
                _ => (0.0, String::new(), 0),
            };
            let paid_outside: f64 = splits
                .iter()
                .filter(|s| {
                    s.pay_plan_num == 0
                        && match link.link_type {
                            PayPlanLink::PROCEDURE => s.proc_num == link.fkey,
                            PayPlanLink::ADJUSTMENT => s.adj_num == link.fkey,
                            _ => false,
                        }
                })
                .map(|s| s.split_amount)
                .sum();
            let paid_on_plan: f64 = splits
                .iter()
                .filter(|s| {
                    s.pay_plan_num == plan.pay_plan_num
                        && match link.link_type {
                            PayPlanLink::PROCEDURE => s.proc_num == link.fkey,
                            PayPlanLink::ADJUSTMENT => s.adj_num == link.fkey,
                            _ => false,
                        }
                })
                .map(|s| s.split_amount)
                .sum();
            let plan_debits: f64 = charges
                .iter()
                .filter(|c| {
                    c.charge_type == PayPlanChargeType::Debit
                        && c.fkey == link.fkey
                        && c.link_type == link.link_type
                })
                .map(|c| c.principal)
                .sum();
            let portion_on_plan = if link.amount_override != 0.0 {
                link.amount_override
            } else {
                round2(pat_portion - paid_outside)
            };
            let overcharged = (-round2(portion_on_plan - plan_debits)).max(0.0);
            if overcharged == 0.0 {
                continue;
            }
            let pat_overpaid = (-round2(portion_on_plan - paid_on_plan)).max(0.0);
            table.push(&[
                &plan.date_pay_plan_start.map(short_date).unwrap_or_default(),
                &patient_last_first(patients, plan.pat_num),
                &patient_last_first(patients, plan.guarantor),
                &abbreviation(provider_num),
                &description,
                &if link.amount_override != 0.0 { "X" } else { "" },
                &money(pat_portion),
                &money(paid_outside),
                &money(portion_on_plan),
                &money(plan_debits),
                &money(overcharged),
                &money(paid_on_plan),
                &money(pat_overpaid),
            ]);
        }
    }
    table
}

/// A raw public-health extract row. OD builds these from `screen`,
/// `screengroup`, `site`, `county` and `patientrace`, none of which are part of
/// the modelled schema; the caller supplies the already-shaped values.
#[derive(Clone, Debug, Default)]
pub struct PublicHealthRow {
    pub values: Vec<String>,
}

/// `FormRpPHRawPop`: raw population extract (16 columns).
pub fn ph_raw_population_report(rows: &[PublicHealthRow]) -> ReportTable {
    let mut table = ReportTable::new(&[
        "PatNum",
        "ProcDate",
        "ProvName",
        "County",
        "CountyCode",
        "gradeschool",
        "schoolCode",
        "GradeLevel",
        "Birthdate",
        "Race",
        "Gender",
        "Urgency",
        "BillingType",
        "PlannedIsDone",
        "NumberBroken",
    ]);
    for row in rows {
        table.push(&row.values.iter().map(String::as_str).collect::<Vec<_>>());
    }
    table
}

/// `FormRpPHRawScreen`: raw screening extract. The six dental YN columns are
/// omitted for a medical-only practice.
pub fn ph_raw_screen_report(rows: &[PublicHealthRow], dental: bool) -> ReportTable {
    let mut headers = vec![
        "SGDate",
        "ProvName",
        "County",
        "CountyCode",
        "schoolName",
        "schoolCode",
        "PlaceService",
        "GradeLevel",
        "Age",
        "Birthdate",
        "RaceOld",
        "Gender",
        "Urgency",
    ];
    if dental {
        headers.extend_from_slice(&[
            "HasCaries",
            "EarlyChildCaries",
            "CariesExperience",
            "ExistingSealants",
            "NeedsSealants",
            "MissingAllTeeth",
        ]);
    }
    headers.push("Comments");
    let mut table = ReportTable::new(&headers);
    for row in rows {
        table.push(&row.values.iter().map(String::as_str).collect::<Vec<_>>());
    }
    table
}

/// `FormRpRouting`: one printable routing slip per appointment on the selected
/// day. The simplified port emits the slip's data fields as rows; OD's absolute
/// sheet layout is outside this port's PDF engine.
pub fn routing_slips_report(
    patients: &[Patient],
    providers: &[Provider],
    procedures: &[Procedure],
    appointments: &[Appointment],
    day: NaiveDate,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Time",
        "Length",
        "Patient",
        "Provider",
        "Procedures",
        "Note",
    ]);
    let mut rows: Vec<&Appointment> = appointments
        .iter()
        .filter(|a| a.apt_datetime.date() == day)
        .filter(|a| {
            !matches!(
                a.apt_status,
                AppointmentStatus::UnschedList
                    | AppointmentStatus::Planned
                    | AppointmentStatus::PtNote
                    | AppointmentStatus::PtNoteCompleted
            )
        })
        .collect();
    rows.sort_by_key(|a| a.apt_datetime);
    for apt in rows {
        let proc_text: Vec<String> = procedures
            .iter()
            .filter(|p| p.proc_num == apt.apt_num || p.pat_num == apt.pat_num)
            .take(8)
            .map(|p| format!("{} {}", p.tooth_num, p.fee as i64))
            .collect();
        table.push(&[
            &apt.apt_datetime.format("%I:%M %p").to_string(),
            &format!("{} minutes", apt.apt_length * 5),
            &patient_name(patients, apt.pat_num),
            &provider_name(providers, apt.provider_num),
            &proc_text.join("; "),
            &apt.notes,
        ]);
    }
    table
}

/// Label input shared by the patient, birthday and custom tabs of
/// `FormRpLaserLabels`.
#[derive(Clone, Debug, Default)]
pub struct LabelAddress {
    pub line1: String,
    pub line2: String,
    pub city_state_zip: String,
    pub copies: usize,
}

/// Shape OD `carrier` rows into the insurance-company tab of
/// `FormRpLaserLabels`. Upstream selects `CarrierName, Address, Address2,
/// City, State, Zip` with no `IsHidden` filter and prints `Address2` only when
/// it is nonblank; the final line is `City, State   Zip`.
///
/// "Single Company" matches `CarrierName + Address` exactly and repeats the
/// row `copies` times. "Range of Companies" filters
/// `CarrierName + Address + City` between the two selected keys and sorts by
/// that key, exactly like upstream's two SQL branches.
pub fn carrier_label_addresses(
    carriers: &[Carrier],
    options: &LaserLabelOptions,
) -> Vec<LabelAddress> {
    let single_key = |c: &Carrier| format!("{}{}", c.carrier_name, c.address);
    let range_key = |c: &Carrier| format!("{}{}{}", c.carrier_name, c.address, c.city);
    let selected: Vec<&Carrier> = match options.ins_mode {
        LaserInsMode::Single => {
            if options.start_addr.is_empty() {
                // Upstream warns "Please use the selection button first".
                return Vec::new();
            }
            let key = format!("{}{}", options.start_name, options.start_addr);
            carriers
                .iter()
                .filter(|c| single_key(c) == key)
                .take(1)
                .collect()
        }
        LaserInsMode::Range => {
            // OD's SQL key is Name+Address+City. The form only exposes name
            // and address inputs (city arrives with the picked carrier), so
            // the port compares the Name+Address prefix and still sorts by
            // the full upstream key.
            let start = format!("{}{}", options.start_name, options.start_addr);
            let end = format!("{}{}", options.end_name, options.end_addr);
            // Empty bounds are treated as unbounded so a fresh install shows
            // the whole carrier list before the user picks a range.
            let mut rows: Vec<&Carrier> = carriers
                .iter()
                .filter(|c| {
                    let key = single_key(c);
                    (start.is_empty() || key >= start) && (end.is_empty() || key <= end)
                })
                .collect();
            rows.sort_by(|a, b| range_key(a).cmp(&range_key(b)));
            rows
        }
    };
    if selected.is_empty() {
        return Vec::new();
    }
    let copies = if options.ins_mode == LaserInsMode::Single {
        options.ins_copies.max(1)
    } else {
        1
    };
    let mut labels = Vec::new();
    for c in selected.into_iter() {
        // Upstream emits Address2 as its own line when present. The grid has
        // a single address column, so fold it into the second line.
        let line2 = if c.address2.is_empty() {
            c.address.clone()
        } else {
            format!("{}\n{}", c.address, c.address2)
        };
        for _ in 0..copies {
            labels.push(LabelAddress {
                line1: c.carrier_name.clone(),
                line2: line2.clone(),
                city_state_zip: format!("{}, {}   {}", c.city, c.state, c.zip),
                copies: 1,
            });
        }
    }
    labels
}

/// `FormRpLaserLabels`: flatten addresses into a 30-per-page label stream.
pub fn laser_labels_report(labels: &[LabelAddress]) -> ReportTable {
    let mut table = ReportTable::new(&["Name / Address", "Address 2", "City, ST  ZIP"]);
    for label in labels {
        for _ in 0..label.copies {
            table.push(&[&label.line1, &label.line2, &label.city_state_zip]);
        }
    }
    table
}

/// Patient and birthday label addresses can be derived from the modelled
/// patient table; helper kept next to the builder for the GUI to call.
pub fn patient_label_addresses(
    patients: &[Patient],
    birthday_month: u32,
    birthday_day: u32,
) -> Vec<LabelAddress> {
    let mut labels = Vec::new();
    for patient in patients {
        let matches_birthday = patient
            .birth_date
            .map(|b| b.month() == birthday_month && b.day() == birthday_day)
            .unwrap_or(false);
        if !matches_birthday {
            continue;
        }
        labels.push(LabelAddress {
            line1: format!("{} {}", patient.f_name, patient.l_name),
            line2: patient.address.clone(),
            city_state_zip: format!(
                "{}, {}   {}",
                patient.city, patient.state, patient.zip
            ),
            copies: 1,
        });
    }
    labels
}

/// `FormRpPrintPreview` is a debug-only print host with no report query. The
/// port represents it as an empty table so the entry point still exists.
pub fn print_preview_report() -> ReportTable {
    ReportTable::new(&["Print preview (debug host - no data)"])
}

/// `FormRpStatement`: per-statement summary of charges, payments and balance.
/// The existing `build_statement_document`/`render_statement_pdf` path produces
/// the printable A4/A5 detail; this builder gives the grid/export view.
pub fn statement_report(
    patients: &[Patient],
    statements: &[Statement],
    procedures: &[Procedure],
    splits: &[PaySplit],
    adjustments: &[Adjustment],
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Statement Number",
        "Date",
        "Patient",
        "Procedure Total",
        "Payments",
        "Adjustments",
        "Balance",
    ]);
    let mut rows: Vec<&Statement> = statements.iter().collect();
    rows.sort_by_key(|s| (s.date_sent, s.statement_num));
    for statement in rows {
        let pat_procs: Vec<&Procedure> = procedures
            .iter()
            .filter(|p| p.pat_num == statement.pat_num && p.proc_status == ProcedureStatus::Complete)
            .collect();
        let charges: f64 = pat_procs.iter().map(|p| proc_production(p)).sum();
        let payments: f64 = splits
            .iter()
            .filter(|s| s.pat_num == statement.pat_num)
            .map(|s| s.split_amount)
            .sum();
        let adj: f64 = adjustments
            .iter()
            .filter(|a| a.pat_num == statement.pat_num)
            .map(|a| a.adj_amount)
            .sum();
        table.push(&[
            &statement.statement_num.to_string(),
            &statement.date_sent.map(short_date).unwrap_or_default(),
            &patient_last_first(patients, statement.pat_num),
            &money(charges),
            &money(payments),
            &money(adj),
            &money(charges + adj - payments),
        ]);
    }
    table
}

/// `FormRpPatPortionUncollected`: completed procedures in the range whose
/// patient portion is still owing after adjustments and patient payments.
///
/// Mirrors `RpPatPortionUncollected.GetPatUncollected` from the 24_3 source.
/// Each row is one completed procedure. `Fee` is the multiplied procedure fee
/// less capitation write-offs, `PatPortion` subtracts the insurance estimate,
/// `Adjustment` nets adjustment amounts against paysplits tied to them, and
/// `Patient Paid` is the sum of paysplits on the procedure. Rows with an
/// uncollected balance of 0.005 or less are omitted, as upstream does.
///
/// With Insurance & claims turned off the insurance-estimate terms are all
/// zero, which is the intended Ethiopia behaviour: the whole fee is the
/// patient's portion. Hidden unearned paysplits are excluded on both sides,
/// matching OD's `DefCat.PaySplitUnearnedType` filter.
pub fn patient_portion_uncollected_report(
    patients: &[Patient],
    procedures: &[Procedure],
    procedure_codes: &[ProcedureCode],
    adjustments: &[Adjustment],
    splits: &[PaySplit],
    claim_procs: &[ClaimProc],
    definitions: &[Definition],
    from: NaiveDate,
    to: NaiveDate,
    insurance_enabled: bool,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Date",
        "Patient Name",
        "Procedure",
        "Fee",
        "Patient",
        "Adjustment",
        "Patient Paid",
        "Uncollected",
    ]);
    let hidden = hidden_unearned_def_nums(definitions);
    let visible_split = |split: &&PaySplit| !hidden.contains(&split.unearned_type);

    struct Row {
        proc: NaiveDate,
        l_name: String,
        f_name: String,
        code: String,
        fee: String,
        pat_portion: String,
        adjustment: String,
        paid: String,
        uncollected_text: String,
    }

    let mut rows = Vec::new();
    for proc in procedures
        .iter()
        .filter(|p| p.proc_status.is_completed() && p.proc_date >= from && p.proc_date <= to)
    {
        // OD's capitation write-off reduces the fee; the other insurance terms
        // feed InsEst. None apply when insurance is switched off.
        let mut cap_write_off = 0.0;
        let mut ins_est = 0.0;
        if insurance_enabled {
            for cp in claim_procs.iter().filter(|cp| cp.proc_num == proc.proc_num) {
                if cp.status == 7 {
                    cap_write_off += cp.write_off;
                }
                match cp.status {
                    0 => ins_est += cp.ins_pay_est,
                    1 | 4 => ins_est += cp.ins_pay_amt,
                    _ => {}
                }
                // OD adds WriteOff for every status except CapComplete (7)
                // and InsHist (9), including the received statuses above.
                if cp.status != 7 && cp.status != 9 {
                    ins_est += cp.write_off;
                }
            }
        }
        let fee = proc.fee * (proc.base_units + proc.unit_qty) - cap_write_off;
        let pat_portion = fee - ins_est;

        // Adjustment = SUM(AdjAmt) - SUM(paysplits tied to that adjustment).
        // OD applies the date range to adjustments only; paysplits are not
        // date filtered. Both sides exclude hidden unearned splits.
        let adjustment: f64 = adjustments
            .iter()
            .filter(|a| a.proc_num == proc.proc_num && a.proc_date >= from && a.proc_date <= to)
            .map(|a| {
                let paid_on_adj: f64 = splits
                    .iter()
                    .filter(|s| s.adj_num == a.adj_num && visible_split(s))
                    .map(|s| s.split_amount)
                    .sum();
                a.adj_amount - paid_on_adj
            })
            .sum();
        let paid: f64 = splits
            .iter()
            .filter(|s| s.proc_num == proc.proc_num && visible_split(s))
            .map(|s| s.split_amount)
            .sum();
        let uncollected = pat_portion + adjustment - paid;
        if uncollected <= 0.005 {
            continue;
        }
        let patient = patients.iter().find(|p| p.pat_num == proc.pat_num);
        let l_name = patient.map(|p| p.l_name.clone()).unwrap_or_default();
        let f_name = patient.map(|p| p.f_name.clone()).unwrap_or_default();
        let code = procedure_codes
            .iter()
            .find(|c| c.code_num == proc.code_num)
            .map(|c| c.abbreviation.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("Code {}", proc.code_num));
        rows.push(Row {
            proc: proc.proc_date,
            l_name,
            f_name,
            code,
            fee: money(fee),
            pat_portion: money(pat_portion),
            adjustment: money(adjustment),
            paid: money(paid),
            uncollected_text: money(uncollected),
        });
    }
    rows.sort_by(|a, b| {
        (a.proc, &a.l_name, &a.f_name, &a.code).cmp(&(b.proc, &b.l_name, &b.f_name, &b.code))
    });
    for row in rows {
        table.push(&[
            &short_date(row.proc),
            &format!("{}, {}", row.l_name, row.f_name),
            &row.code,
            &row.fee,
            &row.pat_portion,
            &row.adjustment,
            &row.paid,
            &row.uncollected_text,
        ]);
    }
    table
}

/// `FormRpTreatmentFinder`: patients with treatment planned in the date range
/// and the total amount planned, normally excluding anyone who already has a
/// future scheduled appointment (the "who needs to be called" list).
///
/// Mirrors `RpTreatmentFinder.GetDictAmtPlanned` and `GetTableRaw` from the
/// 24_3 source. `AmtPlanned = SUM(ProcFee * (UnitQty + BaseUnits))` over TP
/// procedures, grouped per patient, with `AmtPlanned > 0` (OD's `HAVING`).
/// The optional provider/billing/clinic/code filters and the "pats with appts"
/// exclusion follow the upstream SQL exactly.
///
/// `Procedure.DateTP` is not modelled separately in this port, so `ProcDate`
/// is used as the treatment-plan date. Contact detail is derived from the
/// patient's preferred-style fields (email first, then home/cell/work phone),
/// matching OD's default when `PreferRecallMethod` is None.
///
/// The insurance benefit columns (`Annual Max`, `Amt Used`, `Amt Pend`,
/// `Amt Rem`, `Insurance Carrier`) are appended only when `insurance_enabled`.
/// Used/pending come from `ClaimProc`; annual maximums require OD `benefit`
/// rows which this port does not model, so that column is `0.00` and the
/// "above amount" filter is inert (documented as a fidelity gap).
pub fn treatment_finder_report(
    patients: &[Patient],
    procedures: &[Procedure],
    procedure_codes: &[ProcedureCode],
    appointments: &[Appointment],
    claim_procs: &[ClaimProc],
    ins_plans: &[InsPlan],
    carriers: &[Carrier],
    from: NaiveDate,
    to: NaiveDate,
    today: NaiveDate,
    options: &TreatmentFinderReportOptions,
    insurance_enabled: bool,
) -> ReportTable {
    let mut headers = vec!["LName", "FName", "Contact", "Treat Plan"];
    if insurance_enabled {
        headers.extend_from_slice(&[
            "Annual Max",
            "Amt Used",
            "Amt Pend",
            "Amt Rem",
            "Insurance Carrier",
        ]);
    }
    let mut table = ReportTable::new(&headers);

    // Patients with a future Scheduled appointment are left off the list unless
    // the user asks to include them. OD tests `AptDateTime >= CURDATE()`.
    let patients_with_appts: BTreeSet<Id> = if options.pats_with_appts {
        BTreeSet::new()
    } else {
        appointments
            .iter()
            .filter(|a| {
                a.apt_status == AppointmentStatus::Scheduled && a.apt_datetime.date() >= today
            })
            .map(|a| a.pat_num)
            .collect()
    };

    // Group the multiplied planned fee per patient, applying the same WHERE
    // clauses as upstream's GetDictAmtPlanned query.
    let mut amt_planned: BTreeMap<Id, f64> = BTreeMap::new();
    for proc in procedures
        .iter()
        .filter(|p| p.proc_status == ProcedureStatus::TreatmentPlanned)
    {
        if proc.proc_date < from || proc.proc_date > to {
            continue;
        }
        let Some(patient) = patients.iter().find(|p| p.pat_num == proc.pat_num) else {
            continue;
        };
        // OD filters `patient.PatStatus = Patient` (0).
        if patient.pat_status != 0 || patients_with_appts.contains(&proc.pat_num) {
            continue;
        }
        if !options.code1.is_empty() {
            let code = procedure_codes
                .iter()
                .find(|c| c.code_num == proc.code_num)
                .map(|c| c.code.as_str())
                .unwrap_or("");
            if code < options.code1.as_str()
                || (!options.code2.is_empty() && code > options.code2.as_str())
            {
                continue;
            }
        }
        if options.provider_num != 0 {
            let prov = if options.use_treating_provider {
                proc.provider_num
            } else {
                patient.primary_provider_num
            };
            if prov != options.provider_num {
                continue;
            }
        }
        if options.billing_type_num != 0 && patient.billing_type != options.billing_type_num {
            continue;
        }
        if options.clinic_num != 0 && patient.clinic_num != options.clinic_num {
            continue;
        }
        *amt_planned.entry(proc.pat_num).or_insert(0.0) +=
            proc.fee * (proc.unit_qty + proc.base_units);
    }
    amt_planned.retain(|_, amount| *amount > 0.0);

    // Insurance benefit detail (only built when the feature is on).
    let plan_carrier: BTreeMap<Id, &str> = ins_plans
        .iter()
        .filter_map(|plan| {
            carriers
                .iter()
                .find(|c| c.carrier_num == plan.carrier_num)
                .map(|c| (plan.plan_num, c.carrier_name.as_str()))
        })
        .collect();
    let proc_pat: BTreeMap<Id, Id> = procedures
        .iter()
        .map(|p| (p.proc_num, p.pat_num))
        .collect();
    let mut used: BTreeMap<Id, f64> = BTreeMap::new();
    let mut pending: BTreeMap<Id, f64> = BTreeMap::new();
    let mut carrier: BTreeMap<Id, String> = BTreeMap::new();
    if insurance_enabled {
        for cp in claim_procs.iter() {
            let Some(&pat_num) = proc_pat.get(&cp.proc_num) else {
                continue;
            };
            if !amt_planned.contains_key(&pat_num) {
                continue;
            }
            match cp.status {
                0 => {
                    if cp.ins_pay_amt == 0.0 {
                        *pending.entry(pat_num).or_insert(0.0) += cp.ins_pay_est;
                    }
                }
                1 | 3 | 4 => *used.entry(pat_num).or_insert(0.0) += cp.ins_pay_amt,
                _ => {}
            }
            if cp.plan_num != 0 && !carrier.contains_key(&pat_num) {
                if let Some(name) = plan_carrier.get(&cp.plan_num) {
                    carrier.insert(pat_num, (*name).to_string());
                }
            }
        }
    }

    let mut rows: Vec<&Patient> = patients
        .iter()
        .filter(|p| amt_planned.contains_key(&p.pat_num) && p.pat_status == 0)
        .collect();
    rows.sort_by(|a, b| (&a.l_name, &a.f_name, a.pat_num).cmp(&(&b.l_name, &b.f_name, b.pat_num)));

    for patient in rows {
        let planned = amt_planned[&patient.pat_num];
        let contact = if !patient.email.is_empty() {
            patient.email.clone()
        } else if !patient.home_phone.is_empty() {
            format!("Hm: {}", patient.home_phone)
        } else if !patient.wireless_phone.is_empty() {
            format!("Cell: {}", patient.wireless_phone)
        } else if !patient.wk_phone.is_empty() {
            format!("Wk: {}", patient.wk_phone)
        } else {
            "Mail".to_string()
        };
        let mut cells = vec![
            patient.l_name.clone(),
            patient.f_name.clone(),
            contact,
            money(planned),
        ];
        if insurance_enabled {
            // OD `benefit` annual maximums are not modelled; 0.00 keeps the
            // column present without inventing a figure. Amt Rem is used/pending
            // netted against it and, like the form, never shows a negative.
            let annual_max = 0.0_f64;
            let amt_used = *used.get(&patient.pat_num).unwrap_or(&0.0);
            let amt_pend = *pending.get(&patient.pat_num).unwrap_or(&0.0);
            let amt_rem = (annual_max - amt_used - amt_pend).max(0.0);
            if options.above_amount > 0.0 && annual_max > 0.0 {
                if annual_max - amt_used <= options.above_amount {
                    continue;
                }
            }
            cells.extend_from_slice(&[
                money(annual_max),
                money(amt_used),
                money(amt_pend),
                money(amt_rem),
                carrier.get(&patient.pat_num).cloned().unwrap_or_default(),
            ]);
        }
        let refs: Vec<&str> = cells.iter().map(String::as_str).collect();
        table.push(&refs);
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn dt(y: i32, m: u32, d: u32) -> NaiveDateTime {
        date(y, m, d).and_hms_opt(9, 0, 0).unwrap()
    }

    fn patient(num: Id, first: &str, last: &str) -> Patient {
        Patient {
            pat_num: num,
            f_name: first.into(),
            l_name: last.into(),
            ..Default::default()
        }
    }

    fn provider(num: Id, name: &str) -> Provider {
        Provider {
            provider_num: num,
            abbreviation: name.into(),
            ..Default::default()
        }
    }

    fn code(num: Id, code: &str, name: &str) -> ProcedureCode {
        ProcedureCode {
            code_num: num,
            code: code.into(),
            code_name: name.into(),
            ..Default::default()
        }
    }

    fn procedure(num: Id, pat: Id, code_num: Id, proc_date: NaiveDate, fee: f64) -> Procedure {
        Procedure {
            proc_num: num,
            pat_num: pat,
            code_num,
            proc_date,
            proc_status: ProcedureStatus::Complete,
            fee,
            provider_num: 1,
            ..Default::default()
        }
    }

    fn row_matches(table: &ReportTable, values: &[&str]) -> bool {
        table.rows.iter().any(|row| {
            row.len() == values.len()
                && row
                    .iter()
                    .zip(values)
                    .all(|(cell, expected)| cell == expected)
        })
    }

    #[test]
    fn accounting_reports_follow_od_sign_rule() {
        let accounts = vec![
            Account {
                account_num: 1,
                description: "Cash".into(),
                account_type: AccountType::Asset,
                ..Default::default()
            },
            Account {
                account_num: 2,
                description: "Fees".into(),
                account_type: AccountType::Income,
                ..Default::default()
            },
        ];
        let journal = vec![
            JournalEntry {
                journal_entry_num: 1,
                account_num: 1,
                date_displayed: date(2026, 1, 5),
                debit_amt: 500.0,
                ..Default::default()
            },
            JournalEntry {
                journal_entry_num: 2,
                account_num: 2,
                date_displayed: date(2026, 1, 5),
                credit_amt: 500.0,
                ..Default::default()
            },
        ];

        let balance_sheet = balance_sheet_report(&accounts, &journal, date(2026, 12, 31), false);
        assert!(row_matches(&balance_sheet, &["Cash", "", "500.00"]));
        assert!(row_matches(&balance_sheet, &["Fees", "", "500.00"]));

        let profit_loss = profit_loss_report(
            &accounts,
            &journal,
            date(2026, 1, 1),
            date(2026, 12, 31),
        );
        assert!(row_matches(&profit_loss, &["Net Income", "", "500.00"]));

        let ledger = general_ledger_report(
            &accounts,
            &journal,
            date(2026, 1, 1),
            date(2026, 12, 31),
            true,
        );
        assert!(row_matches(
            &ledger,
            &["01/01/2026", "", "Opening Balance", "", "", "0.00", ""]
        ));
        assert!(ledger
            .rows
            .iter()
            .any(|r| r[2] == "Ending Balance" && r[5] == "500.00"));
    }

    #[test]
    fn retained_earnings_fold_prior_year_results() {
        let accounts = vec![
            Account {
                account_num: 1,
                description: "Retained Earnings".into(),
                account_type: AccountType::Equity,
                is_retained_earnings: true,
                ..Default::default()
            },
            Account {
                account_num: 2,
                description: "Fees".into(),
                account_type: AccountType::Income,
                ..Default::default()
            },
        ];
        let journal = vec![JournalEntry {
            journal_entry_num: 1,
            account_num: 2,
            date_displayed: date(2025, 6, 1),
            credit_amt: 300.0,
            ..Default::default()
        }];
        let table = balance_sheet_report(&accounts, &journal, date(2026, 12, 31), false);
        assert!(row_matches(&table, &["Retained Earnings", "", "300.00"]));
    }

    #[test]
    fn hidden_pay_splits_uses_item_value_and_excludes_refunds() {
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let definitions = vec![
            Definition {
                def_num: 10,
                category: DefinitionCategory::PAY_SPLIT_UNEARNED_TYPE,
                item_name: "Prepayment".into(),
                item_value: String::new(),
                ..Default::default()
            },
            Definition {
                def_num: 11,
                category: DefinitionCategory::PAY_SPLIT_UNEARNED_TYPE,
                item_name: "Hidden Deposit".into(),
                item_value: "hidden".into(),
                ..Default::default()
            },
        ];
        let splits = vec![
            PaySplit {
                split_num: 1,
                pat_num: 1,
                split_amount: 100.0,
                date_pay: date(2026, 3, 1),
                unearned_type: 11,
                pay_plan_charge_num: 0,
                pay_plan_debit_type: Default::default(),
                ..Default::default()
            },
            PaySplit {
                split_num: 2,
                pat_num: 1,
                split_amount: -25.0,
                date_pay: date(2026, 3, 2),
                unearned_type: 11,
                pay_plan_charge_num: 0,
                pay_plan_debit_type: Default::default(),
                ..Default::default()
            },
            PaySplit {
                split_num: 3,
                pat_num: 1,
                split_amount: 900.0,
                date_pay: date(2026, 3, 3),
                unearned_type: 10,
                pay_plan_charge_num: 0,
                pay_plan_debit_type: Default::default(),
                ..Default::default()
            },
        ];
        let options = HiddenPaySplitsReportOptions::default();
        let all = hidden_pay_splits_report(
            &patients,
            &[],
            &definitions,
            &splits,
            date(2026, 1, 1),
            date(2026, 12, 31),
            &options,
        );
        assert_eq!(all.rows.len(), 3); // two hidden splits + total
        assert!(row_matches(&all, &["", "", "", "Total", "75.00"]));

        let no_refunds = hidden_pay_splits_report(
            &patients,
            &[],
            &definitions,
            &splits,
            date(2026, 1, 1),
            date(2026, 12, 31),
            &HiddenPaySplitsReportOptions {
                exclude_refunds: true,
            },
        );
        assert!(row_matches(&no_refunds, &["", "", "", "Total", "100.00"]));
    }

    #[test]
    fn unearned_tabs_split_allocation_and_net() {
        use od_domain::reports_ext::UnearnedIncomeReportOptions;
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let definitions = vec![Definition {
            def_num: 10,
            category: DefinitionCategory::PAY_SPLIT_UNEARNED_TYPE,
            item_name: "Prepayment".into(),
            ..Default::default()
        }];
        let splits = vec![PaySplit {
            split_num: 1,
            pat_num: 1,
            split_amount: 300.0,
            date_pay: date(2026, 3, 1),
            unearned_type: 10,
                pay_plan_charge_num: 0,
                pay_plan_debit_type: Default::default(),
            ..Default::default()
        }
        .clone()];
        let procedures = vec![procedure(1, 1, 1, date(2026, 6, 1), 120.0)];
        let options = UnearnedIncomeReportOptions {
            tab: UnearnedIncomeTab::LineItem,
            ..Default::default()
        };
        let line = unearned_income_report(
            &patients,
            &[],
            &definitions,
            &procedures,
            &splits,
            date(2026, 1, 1),
            date(2026, 12, 31),
            &options,
        );
        assert!(row_matches(
            &line,
            &["03/01/2026", "Lovelace, Ada", "Prepayment", "300.00"]
        ));

        let allocation = unearned_income_report(
            &patients,
            &[],
            &definitions,
            &[],
            &splits,
            date(2026, 1, 1),
            date(2026, 12, 31),
            &UnearnedIncomeReportOptions {
                tab: UnearnedIncomeTab::Allocation,
                exclude_zero_allocation: false,
                ..Default::default()
            },
        );
        assert!(row_matches(
            &allocation,
            &["Lovelace, Ada", "300.00", "0.00", "0.00", "300.00"]
        ));

        let net = unearned_income_report(
            &patients,
            &[],
            &definitions,
            &[],
            &splits,
            date(2026, 1, 1),
            date(2026, 12, 31),
            &UnearnedIncomeReportOptions {
                tab: UnearnedIncomeTab::Net,
                ..Default::default()
            },
        );
        assert!(row_matches(&net, &["Lovelace, Ada", "300.00", "0.00", "300.00"]));
    }

    #[test]
    fn proc_overpaid_detects_payments_above_fee() {
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let procedures = vec![procedure(1, 1, 1, date(2026, 4, 1), 100.0)];
        let codes = vec![code(1, "D1110", "Prophy")];
        let splits = vec![PaySplit {
            split_num: 1,
            pat_num: 1,
            proc_num: 1,
            split_amount: 150.0,
            date_pay: date(2026, 4, 2),
            ..Default::default()
        }];
        let table = proc_overpaid_report(
            &patients,
            &procedures,
            &codes,
            &[],
            &splits,
            date(2026, 1, 1),
            date(2026, 12, 31),
            &ProcOverpaidReportOptions::default(),
        );
        assert!(row_matches(
            &table,
            &[
                "Lovelace, Ada",
                "04/01/2026",
                "D1110",
                "Prophy",
                "100.00",
                "0.00",
                "150.00",
                "-50.00"
            ]
        ));
    }

    #[test]
    fn receivables_ages_balances_and_hides_insurance_column() {
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let definitions = vec![Definition {
            def_num: 5,
            category: DefinitionCategory::BILLING_TYPES,
            item_name: "Cash".into(),
            ..Default::default()
        }];
        let mut pat = patients[0].clone();
        pat.billing_type = 5;
        let procedures = vec![procedure(1, 1, 1, date(2026, 12, 15), 100.0)];
        let table = receivables_breakdown_report(
            &[pat],
            &definitions,
            &procedures,
            &[],
            &[],
            date(2026, 12, 31),
            false,
        );
        assert_eq!(table.headers.len(), 6);
        assert!(row_matches(
            &table,
            &["Cash", "100.00", "0.00", "0.00", "0.00", "100.00"]
        ));
        let with_ins = receivables_breakdown_report(
            &patients,
            &definitions,
            &procedures,
            &[],
            &[],
            date(2026, 12, 31),
            true,
        );
        assert_eq!(with_ins.headers.len(), 7);
    }

    #[test]
    fn service_date_view_subtracts_payments_and_writeoffs() {
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let procedures = vec![procedure(1, 1, 1, date(2026, 4, 1), 100.0)];
        let splits = vec![PaySplit {
            split_num: 1,
            proc_num: 1,
            split_amount: 40.0,
            ..Default::default()
        }];
        let claim_procs = vec![ClaimProc {
            claim_proc_num: 1,
            proc_num: 1,
            write_off: 10.0,
            ..Default::default()
        }];
        let table = service_date_view_report(
            &patients,
            &procedures,
            &splits,
            &claim_procs,
            date(2026, 1, 1),
            date(2026, 12, 31),
        );
        assert!(row_matches(
            &table,
            &["04/01/2026", "Lovelace, Ada", "100.00", "40.00", "10.00", "50.00"]
        ));
    }

    #[test]
    fn proc_note_flags_prompt_placeholders_and_unsigned_notes() {
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let codes = vec![code(1, "D1110", "Prophy")];
        let mut incomplete = procedure(1, 1, 1, date(2026, 4, 1), 100.0);
        incomplete.fee = 100.0;
        let notes = vec![
            ProcNote {
                proc_note_num: 1,
                proc_num: 1,
                entry_datetime: dt(2026, 4, 2),
                note: "Cleaning [Prompt:\"Notes\"]".into(),
                signature: String::new(),
                ..Default::default()
            },
            ProcNote {
                proc_note_num: 2,
                proc_num: 1,
                entry_datetime: dt(2026, 4, 1),
                note: "Older note".into(),
                ..Default::default()
            },
        ];
        let options = ProcNoteReportOptions {
            include_no_notes: true,
            include_unsigned: true,
            ..Default::default()
        };
        let table = proc_note_report(
            &patients,
            &[],
            &[incomplete],
            &codes,
            &notes,
            date(2026, 1, 1),
            date(2026, 12, 31),
            &options,
        );
        assert_eq!(table.rows.len(), 1);
        assert!(row_matches(
            &table,
            &[
                "04/01/2026",
                "Lovelace, Ada",
                "D1110",
                "Prophy",
                "",
                "",
                "true",
                "false",
                "true"
            ]
        ));
    }

    #[test]
    fn dental_sealant_counts_first_molar_sealants() {
        let mut patient = patient(1, "Ada", "Lovelace");
        patient.birth_date = Some(date(2018, 5, 1));
        let codes = vec![
            code(1, "D0120", "Exam"),
            code(2, "D1351", "Sealant"),
        ];
        let mut exam = procedure(1, 1, 1, date(2026, 3, 1), 50.0);
        let mut sealant = procedure(2, 1, 2, date(2026, 3, 1), 100.0);
        sealant.tooth_num = "14".into();
        exam.provider_num = 1;
        sealant.provider_num = 1;
        let table = dental_sealant_measure_report(
            &[patient],
            &[Provider {
                provider_num: 1,
                l_name: "Rivera".into(),
                f_name: "Dan".into(),
                abbreviation: "DR".into(),
                ..Default::default()
            }],
            &[exam, sealant],
            &codes,
            2026,
        );
        assert!(row_matches(&table, &["Rivera, Dan", "1", "1", "100.00%"]));
    }

    #[test]
    fn production_and_payroll_reports_use_completed_fees() {
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let mut proc = procedure(1, 1, 1, date(2026, 4, 1), 100.0);
        proc.unit_qty = 1.0;
        proc.base_units = 1.0;
        let from = date(2026, 1, 1);
        let to = date(2026, 12, 31);

        let goal = production_goal_report(&[proc.clone()], &[], &[], &[], &[], &[], &[], &[], from, to);
        assert!(row_matches(
            &goal,
            &["04/01/2026", "Wed", "200.00", "0.00", "0.00", "0.00", "0.00", "0.00", "200.00"]
        ));

        let payroll = provider_payroll_report(
            &patients,
            &[provider(1, "DR")],
            &[proc],
            &[],
            &[],
            &[],
            &[],
            from,
            to,
            false,
        );
        assert!(payroll
            .rows
            .iter()
            .any(|r| r[1] == "Wed" && r[2] == "200.00"));
    }

    #[test]
    fn treatment_plan_reports_include_saved_plans() {
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let providers = vec![provider(1, "DR")];
        let proc = procedure(1, 1, 1, date(2026, 6, 1), 200.0);
        let plans = vec![TreatPlan {
            treat_plan_num: 1,
            pat_num: 1,
            date_saved: Some(dt(2026, 5, 1)),
            status: TreatPlanStatus::Saved,
            provider_num: 1,
            ..Default::default()
        }];
        let lines = vec![TreatPlanLine {
            line_num: 1,
            treat_plan_num: 1,
            proc_num: 1,
            pat_num: 1,
            date_tp: Some(date(2026, 5, 1)),
            ..Default::default()
        }];
        let from = date(2026, 1, 1);
        let to = date(2026, 12, 31);

        let presented = presented_treatment_production_report(
            &patients,
            &providers,
            &[proc.clone()],
            &plans,
            &lines,
            &[],
            &[],
            from,
            to,
            false,
        );
        assert!(row_matches(
            &presented,
            &["DR", "1", "200.00", "0.00", "0.00", "200.00"]
        ));

        let stats = treat_plan_presentation_statistics_report(
            &providers,
            &plans,
            &lines,
            &[proc],
            &[],
            &[],
            &[],
            from,
            to,
            true,
        );
        assert!(stats.rows[0][0] == "DR" && stats.rows[0][1] == "1");
    }

    #[test]
    fn discount_and_dpp_reports_filter_correctly() {
        let subscriptions = vec![
            DiscountPlanSubscription {
                description: "Happy Smiles".into(),
                patient: "Lovelace, Ada".into(),
                ..Default::default()
            },
            DiscountPlanSubscription {
                description: "Other Plan".into(),
                patient: "Smith, John".into(),
                ..Default::default()
            },
        ];
        let filtered = discount_plan_report(&subscriptions, "smile");
        assert_eq!(filtered.rows.len(), 1);
        assert!(filtered.rows[0][0] == "Happy Smiles");

        let patients = vec![patient(1, "Ada", "Lovelace")];
        let plans = vec![PayPlan {
            pay_plan_num: 1,
            pat_num: 1,
            guarantor: 1,
            date_pay_plan_start: Some(date(2026, 2, 1)),
            completed_amt: 1000.0,
            ..Default::default()
        }];
        let charges = vec![PayPlanCharge {
            pay_plan_charge_num: 1,
            pay_plan_num: 1,
            charge_type: PayPlanChargeType::Debit,
            principal: 1200.0,
            ..Default::default()
        }];
        let table = dpp_overcharged_report(
            &patients,
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &plans,
            &charges,
            &[],
            date(2026, 1, 1),
            date(2026, 12, 31),
        );
        assert!(row_matches(
            &table,
            &[
                "02/01/2026",
                "Lovelace, Ada",
                "Lovelace, Ada",
                "",
                "",
                "",
                "1000.00",
                "0.00",
                "1000.00",
                "1200.00",
                "200.00",
                "0.00",
                "0.00"
            ]
        ));
    }

    #[test]
    fn raw_public_health_extracts_preserve_supplied_columns() {
        let row = PublicHealthRow {
            values: vec!["1".into(), "01/01/2026".into()],
        };
        let pop = ph_raw_population_report(&[row.clone()]);
        assert_eq!(pop.headers.len(), 15);
        assert_eq!(pop.rows.len(), 1);
        let screen = ph_raw_screen_report(&[row], false);
        assert_eq!(screen.headers.len(), 14);
        assert_eq!(screen.rows.len(), 1);
    }

    #[test]
    fn print_routing_labels_and_statements_render_rows() {
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let apt = Appointment {
            apt_num: 1,
            pat_num: 1,
            apt_datetime: dt(2026, 4, 1),
            apt_status: AppointmentStatus::Scheduled,
            apt_length: 2,
            ..Default::default()
        };
        let routing = routing_slips_report(&patients, &[], &[], &[apt], date(2026, 4, 1));
        assert!(row_matches(
            &routing,
            &["09:00 AM", "10 minutes", "Lovelace, Ada", "Unassigned", "", ""]
        ));

        let labels = laser_labels_report(&[LabelAddress {
            line1: "Ada Lovelace".into(),
            line2: "1 Main St".into(),
            city_state_zip: "Addis Ababa, AA  1000".into(),
            copies: 2,
        }]);
        assert_eq!(labels.rows.len(), 2);

        assert_eq!(print_preview_report().headers.len(), 1);
        assert!(print_preview_report().rows.is_empty());

        let statements = vec![Statement {
            statement_num: 1,
            pat_num: 1,
            date_sent: Some(date(2026, 4, 2)),
            ..Default::default()
        }];
        let procedures = vec![procedure(1, 1, 1, date(2026, 4, 1), 100.0)];
        let splits = vec![PaySplit {
            split_num: 1,
            pat_num: 1,
            split_amount: 30.0,
            ..Default::default()
        }];
        let table = statement_report(&patients, &statements, &procedures, &splits, &[]);
        assert!(row_matches(
            &table,
            &["1", "04/02/2026", "Lovelace, Ada", "100.00", "30.00", "0.00", "70.00"]
        ));
    }

    #[test]
    fn production_goal_uses_open_provider_schedules_and_merges_overlaps() {
        use chrono::NaiveTime;
        let time = |h: u32, m: u32| NaiveTime::from_hms_opt(h, m, 0).unwrap();
        let schedules = vec![
            Schedule {
                schedule_num: 1,
                sched_date: date(2026, 4, 1),
                start_time: time(9, 0),
                stop_time: time(11, 0),
                sched_type: 1, // Provider
                prov_num: 1,
                status: 0, // Open
                ..Default::default()
            },
            Schedule {
                schedule_num: 2,
                sched_date: date(2026, 4, 1),
                start_time: time(10, 0),
                stop_time: time(12, 0),
                sched_type: 1,
                prov_num: 1,
                status: 0,
                ..Default::default()
            },
            // A practice-level holiday row must not contribute hours.
            Schedule {
                schedule_num: 3,
                sched_date: date(2026, 4, 1),
                start_time: time(8, 0),
                stop_time: time(17, 0),
                sched_type: 0, // Practice
                prov_num: 1,
                status: 2, // Holiday
                ..Default::default()
            },
        ];
        let providers = vec![Provider {
            provider_num: 1,
            hourly_prod_goal_amt: 100.0,
            ..Default::default()
        }];
        let table = production_goal_report(
            &[],
            &[],
            &[],
            &[],
            &schedules,
            &[],
            &providers,
            &[],
            date(2026, 4, 1),
            date(2026, 4, 1),
        );
        // 09:00-12:00 after de-duplicating the overlapping blocks = 3 hours.
        assert!(row_matches(
            &table,
            &["04/01/2026", "Wed", "0.00", "300.00", "0.00", "0.00", "0.00", "0.00", "0.00"]
        ));
    }

    #[test]
    fn dpp_overcharged_uses_links_overrides_and_debit_fkeys() {
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let providers = vec![provider(1, "DR")];
        let codes = vec![code(1, "D1110", "Prophy")];
        let procedures = vec![
            procedure(1, 1, 1, date(2026, 2, 1), 100.0),
            procedure(2, 1, 1, date(2026, 2, 1), 200.0),
        ];
        let plans = vec![PayPlan {
            pay_plan_num: 1,
            pat_num: 1,
            guarantor: 1,
            date_pay_plan_start: Some(date(2026, 2, 1)),
            ..Default::default()
        }];
        let links = vec![
            PayPlanLink {
                pay_plan_link_num: 1,
                pay_plan_num: 1,
                link_type: PayPlanLink::PROCEDURE,
                fkey: 1,
                ..Default::default()
            },
            PayPlanLink {
                pay_plan_link_num: 2,
                pay_plan_num: 1,
                link_type: PayPlanLink::PROCEDURE,
                fkey: 2,
                amount_override: 80.0,
                ..Default::default()
            },
        ];
        let charges = vec![
            PayPlanCharge {
                pay_plan_charge_num: 1,
                pay_plan_num: 1,
                fkey: 1,
                link_type: PayPlanLink::PROCEDURE,
                charge_type: PayPlanChargeType::Debit,
                principal: 150.0,
                ..Default::default()
            },
            PayPlanCharge {
                pay_plan_charge_num: 2,
                pay_plan_num: 1,
                fkey: 2,
                link_type: PayPlanLink::PROCEDURE,
                charge_type: PayPlanChargeType::Debit,
                principal: 100.0,
                ..Default::default()
            },
        ];
        let table = dpp_overcharged_report(
            &patients,
            &providers,
            &procedures,
            &codes,
            &[],
            &[],
            &links,
            &plans,
            &charges,
            &[],
            date(2026, 1, 1),
            date(2026, 12, 31),
        );
        assert_eq!(table.rows.len(), 2);
        assert!(row_matches(
            &table,
            &[
                "02/01/2026", "Lovelace, Ada", "Lovelace, Ada", "DR", "D1110", "",
                "100.00", "0.00", "100.00", "150.00", "50.00", "0.00", "0.00"
            ]
        ));
        assert!(row_matches(
            &table,
            &[
                "02/01/2026", "Lovelace, Ada", "Lovelace, Ada", "DR", "D1110", "X",
                "200.00", "0.00", "80.00", "100.00", "20.00", "0.00", "0.00"
            ]
        ));
    }

    #[test]
    fn provider_payroll_uses_historic_snapshot_write_off() {
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let procedures = vec![procedure(1, 1, 1, date(2026, 4, 1), 100.0)];
        let claim_procs = vec![ClaimProc {
            claim_proc_num: 10,
            proc_num: 1,
            status: 1, // Received
            write_off: 30.0,
            ins_pay_amt: 50.0,
            date_supp_received: Some(date(2026, 4, 5)),
            claim_payment_num: 7,
            ..Default::default()
        }];
        let snapshots = vec![ClaimSnapshot {
            claim_snapshot_num: 1,
            claim_proc_num: 10,
            proc_num: 1,
            write_off: 20.0,
            date_t_entry: dt(2026, 4, 1),
            ..Default::default()
        }];
        let table = provider_payroll_report(
            &patients,
            &[provider(1, "DR")],
            &procedures,
            &claim_procs,
            &snapshots,
            &[],
            &[],
            date(2026, 1, 1),
            date(2026, 12, 31),
            false,
        );
        // Upstream books the estimate on DateComplete and the write-off
        // change plus insurance income on DateSuppReceived.
        assert!(row_matches(
            &table,
            &[
                "04/01/2026", "Wed", "100.00", "-20.00", "0.00", "0.00", "80.00",
                "0.00", "0.00", "0.00", "0.00", "0.00"
            ]
        ));
        assert!(row_matches(
            &table,
            &[
                "04/05/2026", "Sun", "0.00", "0.00", "0.00", "-10.00", "-10.00",
                "0.00", "0.00", "50.00", "0.00", "50.00"
            ]
        ));
    }

    #[test]
    fn provider_payroll_separates_final_and_unfinalized_insurance_income() {
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let procedures = vec![procedure(1, 1, 1, date(2026, 4, 1), 100.0)];
        let claim_procs = vec![
            ClaimProc {
                claim_proc_num: 1,
                proc_num: 1,
                status: 1,
                ins_pay_amt: 40.0,
                date_supp_received: Some(date(2026, 4, 2)),
                claim_payment_num: 5,
                ..Default::default()
            },
            ClaimProc {
                claim_proc_num: 2,
                proc_num: 1,
                status: 4,
                ins_pay_amt: 25.0,
                date_supp_received: Some(date(2026, 4, 2)),
                claim_payment_num: 0,
                ..Default::default()
            },
        ];
        let table = provider_payroll_report(
            &patients,
            &[provider(1, "DR")],
            &procedures,
            &claim_procs,
            &[],
            &[],
            &[],
            date(2026, 1, 1),
            date(2026, 12, 31),
            false,
        );
        assert!(row_matches(
            &table,
            &[
                "04/02/2026", "Thu", "0.00", "0.00", "0.00", "0.00", "0.00",
                "0.00", "0.00", "40.00", "25.00", "40.00"
            ]
        ));
    }

    #[test]
    fn dpp_estimated_insurance_uses_override_then_estimate_then_zero() {
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let procedures = vec![procedure(1, 1, 1, date(2026, 2, 1), 100.0)];
        let plans = vec![PayPlan {
            pay_plan_num: 1,
            pat_num: 1,
            guarantor: 1,
            date_pay_plan_start: Some(date(2026, 2, 1)),
            ..Default::default()
        }];
        let links = vec![PayPlanLink {
            pay_plan_link_num: 1,
            pay_plan_num: 1,
            link_type: PayPlanLink::PROCEDURE,
            fkey: 1,
            ..Default::default()
        }];
        let charges = vec![PayPlanCharge {
            pay_plan_charge_num: 1,
            pay_plan_num: 1,
            fkey: 1,
            link_type: PayPlanLink::PROCEDURE,
            charge_type: PayPlanChargeType::Debit,
            principal: 100.0,
            ..Default::default()
        }];
        let claim_procs = vec![
            ClaimProc {
                claim_proc_num: 1,
                proc_num: 1,
                status: 0, // NotReceived
                ins_pay_est: 10.0,
                write_off_est_override: 25.0,
                write_off_est: 60.0,
                ..Default::default()
            },
            ClaimProc {
                claim_proc_num: 2,
                proc_num: 1,
                status: 6, // Estimate
                ins_pay_est: 5.0,
                write_off_est_override: -1.0,
                write_off_est: 20.0,
                ..Default::default()
            },
            ClaimProc {
                claim_proc_num: 3,
                proc_num: 1,
                status: 9, // InsHist
                ins_pay_est: 5.0,
                write_off_est_override: -1.0,
                write_off_est: -1.0,
                ..Default::default()
            },
        ];
        let table = dpp_overcharged_report(
            &patients,
            &[],
            &procedures,
            &[],
            &[],
            &claim_procs,
            &links,
            &plans,
            &charges,
            &[],
            date(2026, 1, 1),
            date(2026, 12, 31),
        );
        // Ins = (10+25) + (5+20) + (5+0) = 65; plan debits 100.
        assert!(row_matches(
            &table,
            &[
                "02/01/2026", "Lovelace, Ada", "Lovelace, Ada", "", "", "",
                "35.00", "0.00", "35.00", "100.00", "65.00", "0.00", "0.00"
            ]
        ));
    }

    #[test]
    fn carrier_labels_follow_upstream_insurance_company_address_order() {
        let carriers = vec![
            Carrier {
                carrier_num: 1,
                carrier_name: "Zeta Insurance".into(),
                address: "1 Main St".into(),
                address2: "Suite 2".into(),
                city: "Addis Ababa".into(),
                state: "AA".into(),
                zip: "1000".into(),
                ..Default::default()
            },
            Carrier {
                carrier_num: 2,
                carrier_name: "Alpha Insurance".into(),
                address: "2 High St".into(),
                city: "Nairobi".into(),
                state: "NR".into(),
                zip: "00100".into(),
                ..Default::default()
            },
        ];
        let range_options = LaserLabelOptions {
            ins_mode: LaserInsMode::Range,
            ..Default::default()
        };
        let labels = carrier_label_addresses(&carriers, &range_options);
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].line1, "Alpha Insurance");
        assert_eq!(labels[0].line2, "2 High St");
        assert_eq!(labels[0].city_state_zip, "Nairobi, NR   00100");
        assert_eq!(labels[1].line1, "Zeta Insurance");
        assert_eq!(labels[1].line2, "1 Main St\nSuite 2");
        let table = laser_labels_report(&labels);
        assert_eq!(table.rows.len(), 2);

        // Single mode matches Name+Address exactly and honours copy count.
        let single_options = LaserLabelOptions {
            ins_mode: LaserInsMode::Single,
            start_name: "Zeta Insurance".into(),
            start_addr: "1 Main St".into(),
            ins_copies: 3,
            ..Default::default()
        };
        let single = carrier_label_addresses(&carriers, &single_options);
        assert_eq!(single.len(), 3);
        assert!(single.iter().all(|l| l.line1 == "Zeta Insurance"));

        // OD requires the address field before printing a single label.
        assert!(carrier_label_addresses(
            &carriers,
            &LaserLabelOptions {
                ins_mode: LaserInsMode::Single,
                ..Default::default()
            }
        )
        .is_empty());
    }

    #[test]
    fn patient_portion_uncollected_nets_adjustments_and_payments() {
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let codes = vec![ProcedureCode {
            code_num: 1,
            code: "D1110".into(),
            abbreviation: "Prophy".into(),
            ..Default::default()
        }];
        let mut in_range = procedure(1, 1, 1, date(2026, 3, 2), 100.0);
        in_range.unit_qty = 1.0;
        let mut planned = procedure(2, 1, 1, date(2026, 3, 3), 50.0);
        planned.proc_status = ProcedureStatus::TreatmentPlanned;
        let procedures = vec![
            in_range,
            planned,
            procedure(3, 1, 1, date(2025, 12, 31), 500.0),
        ];
        let adjustments = vec![Adjustment {
            adj_num: 7,
            adj_date: date(2026, 3, 2),
            proc_date: date(2026, 3, 2),
            proc_num: 1,
            adj_amount: -20.0,
            ..Default::default()
        }];
        let splits = vec![PaySplit {
            split_num: 1,
            proc_num: 1,
            split_amount: 30.0,
            date_pay: date(2026, 3, 4),
            ..Default::default()
        }];
        let table = patient_portion_uncollected_report(
            &patients,
            &procedures,
            &codes,
            &adjustments,
            &splits,
            &[],
            &[],
            date(2026, 1, 1),
            date(2026, 12, 31),
            false,
        );
        // 100 fee - 20 adjustment - 30 paid = 50 still owed. The TP row and the
        // previous-year row are excluded by the completed/status filters.
        assert_eq!(table.rows.len(), 1);
        assert!(row_matches(
            &table,
            &[
                "03/02/2026", "Lovelace, Ada", "Prophy", "100.00", "100.00",
                "-20.00", "30.00", "50.00"
            ]
        ));
    }

    #[test]
    fn patient_portion_uncollected_subtracts_insurance_estimate_only_when_enabled() {
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let mut proc = procedure(1, 1, 1, date(2026, 3, 2), 100.0);
        proc.unit_qty = 1.0;
        let procedures = vec![proc];
        let claim_procs = vec![
            ClaimProc {
                claim_proc_num: 1,
                proc_num: 1,
                status: 0, // NotReceived
                ins_pay_est: 25.0,
                ..Default::default()
            },
            ClaimProc {
                claim_proc_num: 2,
                proc_num: 1,
                status: 1, // Received
                ins_pay_amt: 10.0,
                write_off: 5.0,
                ..Default::default()
            },
        ];
        let without = patient_portion_uncollected_report(
            &patients,
            &procedures,
            &[],
            &[],
            &[],
            &claim_procs,
            &[],
            date(2026, 1, 1),
            date(2026, 12, 31),
            false,
        );
        assert!(row_matches(
            &without,
            &[
                "03/02/2026", "Lovelace, Ada", "Code 1", "100.00", "100.00",
                "0.00", "0.00", "100.00"
            ]
        ));
        let with = patient_portion_uncollected_report(
            &patients,
            &procedures,
            &[],
            &[],
            &[],
            &claim_procs,
            &[],
            date(2026, 1, 1),
            date(2026, 12, 31),
            true,
        );
        // InsEst = 25 estimate + 10 received + 5 write-off = 40.
        assert!(row_matches(
            &with,
            &[
                "03/02/2026", "Lovelace, Ada", "Code 1", "100.00", "60.00",
                "0.00", "0.00", "60.00"
            ]
        ));
    }

    #[test]
    fn patient_portion_uncollected_ignores_hidden_unearned_splits() {
        use od_domain::definitions::DefinitionCategory;
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let mut proc = procedure(1, 1, 1, date(2026, 3, 2), 100.0);
        proc.unit_qty = 1.0;
        let procedures = vec![proc];
        let definitions = vec![Definition {
            def_num: 5,
            category: DefinitionCategory::PAY_SPLIT_UNEARNED_TYPE,
            item_name: "Prepayment".into(),
            item_value: "hidden".into(),
            ..Default::default()
        }];
        let splits = vec![
            PaySplit {
                split_num: 1,
                proc_num: 1,
                split_amount: 100.0,
                unearned_type: 5,
                pay_plan_charge_num: 0,
                pay_plan_debit_type: Default::default(),
                ..Default::default()
            },
            PaySplit {
                split_num: 2,
                proc_num: 1,
                split_amount: 10.0,
                ..Default::default()
            },
        ];
        let table = patient_portion_uncollected_report(
            &patients,
            &procedures,
            &[],
            &[],
            &splits,
            &[],
            &definitions,
            date(2026, 1, 1),
            date(2026, 12, 31),
            false,
        );
        // Only the 10.00 ordinary split reduces the balance; the unearned
        // prepayment is excluded just as upstream's two subqueries exclude it.
        assert!(row_matches(
            &table,
            &[
                "03/02/2026", "Lovelace, Ada", "Code 1", "100.00", "100.00",
                "0.00", "10.00", "90.00"
            ]
        ));
    }

    fn tp(num: Id, pat: Id, code_num: Id, proc_date: NaiveDate, fee: f64) -> Procedure {
        Procedure {
            proc_num: num,
            pat_num: pat,
            code_num,
            proc_date,
            proc_status: ProcedureStatus::TreatmentPlanned,
            fee,
            unit_qty: 1.0,
            ..Default::default()
        }
    }

    #[test]
    fn treatment_finder_groups_planned_fees_and_excludes_future_appointments() {
        let mut grace_with_email = patient(2, "Grace", "Hopper");
        grace_with_email.email = "grace@example.com".into();
        let patients = vec![patient(1, "Ada", "Lovelace"), grace_with_email];
        let mut completed = tp(9, 1, 1, date(2026, 3, 1), 900.0);
        completed.proc_status = ProcedureStatus::Complete;
        let procedures = vec![
            tp(1, 1, 1, date(2026, 3, 2), 100.0),
            tp(2, 1, 1, date(2026, 4, 2), 50.0),
            tp(3, 2, 1, date(2026, 5, 2), 200.0),
            tp(4, 1, 1, date(2025, 12, 31), 999.0),
            completed,
        ];
        let appointments = vec![Appointment {
            apt_num: 1,
            pat_num: 2,
            apt_datetime: date(2026, 11, 30).and_hms_opt(9, 0, 0).unwrap(),
            apt_status: AppointmentStatus::Scheduled,
            ..Default::default()
        }];
        let from = date(2026, 1, 1);
        let to = date(2026, 12, 31);
        let today = date(2026, 11, 22);

        let default = treatment_finder_report(
            &patients,
            &procedures,
            &[],
            &appointments,
            &[],
            &[],
            &[],
            from,
            to,
            today,
            &TreatmentFinderReportOptions::default(),
            false,
        );
        assert_eq!(default.headers, vec!["LName", "FName", "Contact", "Treat Plan"]);
        assert_eq!(default.rows.len(), 1, "Grace has a future appointment");
        assert!(row_matches(&default, &["Lovelace", "Ada", "Mail", "150.00"]));

        let with_appts = TreatmentFinderReportOptions {
            pats_with_appts: true,
            ..Default::default()
        };
        let table = treatment_finder_report(
            &patients,
            &procedures,
            &[],
            &appointments,
            &[],
            &[],
            &[],
            from,
            to,
            today,
            &with_appts,
            false,
        );
        assert_eq!(table.rows.len(), 2);
        assert!(row_matches(&table, &["Hopper", "Grace", "grace@example.com", "200.00"]));
    }

    #[test]
    fn treatment_finder_honours_code_billing_provider_and_clinic_filters() {
        let mut ada = patient(1, "Ada", "Lovelace");
        ada.billing_type = 3;
        ada.clinic_num = 2;
        ada.primary_provider_num = 7;
        ada.home_phone = "011-222".into();
        let patients = vec![ada];
        let codes = vec![
            code(1, "D1110", "Prophy"),
            code(2, "D2740", "Crown"),
        ];
        let mut crown = tp(2, 1, 2, date(2026, 3, 2), 500.0);
        crown.provider_num = 9;
        let procedures = vec![tp(1, 1, 1, date(2026, 3, 2), 100.0), crown];
        let from = date(2026, 1, 1);
        let to = date(2026, 12, 31);
        let today = date(2026, 11, 22);
        let build = |options: &TreatmentFinderReportOptions| {
            treatment_finder_report(
                &patients,
                &procedures,
                &codes,
                &[],
                &[],
                &[],
                &[],
                from,
                to,
                today,
                options,
                false,
            )
        };

        let all = build(&TreatmentFinderReportOptions::default());
        assert!(row_matches(&all, &["Lovelace", "Ada", "Hm: 011-222", "600.00"]));
        let code_only = build(&TreatmentFinderReportOptions {
            code1: "D2000".into(),
            code2: "D2999".into(),
            ..Default::default()
        });
        assert!(row_matches(&code_only, &["Lovelace", "Ada", "Hm: 011-222", "500.00"]));
        let prov = build(&TreatmentFinderReportOptions {
            provider_num: 7,
            ..Default::default()
        });
        assert!(
            row_matches(&prov, &["Lovelace", "Ada", "Hm: 011-222", "600.00"]),
            "primary provider 7 matches both of Ada's procedures"
        );
        let treating = build(&TreatmentFinderReportOptions {
            provider_num: 9,
            use_treating_provider: true,
            ..Default::default()
        });
        assert!(row_matches(&treating, &["Lovelace", "Ada", "Hm: 011-222", "500.00"]));
        assert!(build(&TreatmentFinderReportOptions {
            provider_num: 9,
            ..Default::default()
        })
        .rows
        .is_empty(), "primary provider is 7, not 9");
        assert!(build(&TreatmentFinderReportOptions {
            billing_type_num: 99,
            ..Default::default()
        })
        .rows
        .is_empty());
        assert!(build(&TreatmentFinderReportOptions {
            clinic_num: 99,
            ..Default::default()
        })
        .rows
        .is_empty());
    }

    #[test]
    fn treatment_finder_insurance_columns_only_when_enabled() {
        let patients = vec![patient(1, "Ada", "Lovelace")];
        let procedures = vec![tp(1, 1, 1, date(2026, 3, 2), 100.0)];
        let claim_procs = vec![
            ClaimProc {
                claim_proc_num: 1,
                proc_num: 1,
                plan_num: 5,
                status: 0, // NotReceived
                ins_pay_est: 30.0,
                ..Default::default()
            },
            ClaimProc {
                claim_proc_num: 2,
                proc_num: 1,
                plan_num: 5,
                status: 1, // Received
                ins_pay_amt: 20.0,
                ..Default::default()
            },
        ];
        let plans = vec![InsPlan {
            plan_num: 5,
            carrier_num: 8,
            ..Default::default()
        }];
        let carriers = vec![Carrier {
            carrier_num: 8,
            carrier_name: "Highland Insurer".into(),
            ..Default::default()
        }];
        let without = treatment_finder_report(
            &patients,
            &procedures,
            &[],
            &[],
            &claim_procs,
            &plans,
            &carriers,
            date(2026, 1, 1),
            date(2026, 12, 31),
            date(2026, 11, 22),
            &TreatmentFinderReportOptions::default(),
            false,
        );
        assert_eq!(without.headers.len(), 4);
        assert!(row_matches(&without, &["Lovelace", "Ada", "Mail", "100.00"]));

        let with = treatment_finder_report(
            &patients,
            &procedures,
            &[],
            &[],
            &claim_procs,
            &plans,
            &carriers,
            date(2026, 1, 1),
            date(2026, 12, 31),
            date(2026, 11, 22),
            &TreatmentFinderReportOptions::default(),
            true,
        );
        assert_eq!(with.headers.len(), 9);
        // Amt Used 20, Amt Pend 30, Amt Rem clamped to 0 (benefits not modelled).
        assert!(row_matches(
            &with,
            &[
                "Lovelace", "Ada", "Mail", "100.00", "0.00", "20.00", "30.00", "0.00",
                "Highland Insurer"
            ]
        ));
    }
}
