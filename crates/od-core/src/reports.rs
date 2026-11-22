//! Report builders for the local Reports slice and the first upstream tranche.
//!
//! The four simple builders read the same OD tables as the Appointments,
//! Account and Treatment Plan modules. The upstream builders port the column
//! sets and calculations of `FormRpAging`, `FormRpPayPlans`, `FormRpBirthday`
//! and `FormRpNewPatients` from the public 24_3 source. Keeping row construction
//! here makes the numbers testable without a GUI or a database, and gives every
//! report one shape that the UI can show, sort and export as CSV or HTML.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{Datelike, NaiveDate, NaiveDateTime};
use od_domain::definitions::{Definition, DefinitionCategory};
use od_domain::reports::{
    ActivePatientReportOptions, AdjustmentSheetReportOptions, AgingAgeFilter, AgingFamilyGrouping,
    AgingOptions, AppointmentDateBasis, AppointmentReportOptions, BrokenAppointmentMode,
    BrokenAppointmentReportOptions, BrokenProcedureOption, ConfirmationReportOptions,
    CustomAgingInclude, CustomAgingNegative, CustomAgingReportOptions, CustomAgingWriteoffDate,
    FinanceChargeReportOptions, NewPatientReportOptions, PaySheetGroupBy, PaySheetReportOptions,
    PlanTypeFilter, PayPlanReportOptions, PrescriptionReportOptions, ProcedureCodesReportOptions,
    ProcedureSheetMode, ProcedureSheetReportOptions, ProdIncMode, ProdIncReportOptions,
    PatientsReportOptions, RecallFieldsReportOptions, ReferralAnalysisReportOptions,
    ReferralsReportOptions, ReportFilter, ReportFilterCondition,
};
use od_domain::*;

use crate::{
    asap_appointments, recall_due_date, recall_due_state,
    treat_plan_total, RecallDueState,
};

/// One display row for the simple three-column reports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportRow {
    /// Usually the patient or provider name.
    pub label: String,
    /// The headline number or state.
    pub value: String,
    /// Optional context: dates, note, plan status, record number.
    pub detail: String,
}

impl ReportRow {
    pub fn new(label: impl Into<String>, value: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            detail: detail.into(),
        }
    }
}

/// A rendered report with a stable column layout. The simple reports use
/// `Item / Value / Detail`; the upstream ports keep OD's own column names so an
/// export looks like the report it was ported from.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReportTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl ReportTable {
    pub fn new(headers: &[&str]) -> Self {
        Self {
            headers: headers.iter().map(|header| header.to_string()).collect(),
            rows: Vec::new(),
        }
    }

    pub fn push(&mut self, values: &[&str]) {
        self.rows.push(values.iter().map(|value| value.to_string()).collect());
    }

    /// Wrap the simple three-column rows so every report follows one path.
    pub fn from_rows(rows: Vec<ReportRow>) -> Self {
        let mut table = ReportTable::new(&["Item", "Value", "Detail"]);
        for row in rows {
            table.push(&[&row.label, &row.value, &row.detail]);
        }
        table
    }

    /// Numeric-aware comparison: `100.00` sorts after `9.00`, while labels sort
    /// alphabetically. This is the same rule OD's complex report grid uses for
    /// `FieldValueType.Number` versus `FieldValueType.String` columns.
    fn compare(left: &str, right: &str) -> std::cmp::Ordering {
        match (left.trim().parse::<f64>(), right.trim().parse::<f64>()) {
            (Ok(x), Ok(y)) => x.total_cmp(&y),
            _ => left.cmp(right),
        }
    }

    /// Sort in place by one column. Blank total/footer rows are kept at the end
    /// so a sort never hides the grand total above the detail rows.
    pub fn sort_by_column(&mut self, column: usize, descending: bool) {
        if column >= self.headers.len() {
            return;
        }
        self.rows.sort_by(|a, b| {
            let left = a.get(column).map(String::as_str).unwrap_or("");
            let right = b.get(column).map(String::as_str).unwrap_or("");
            // Keep total/footer rows at the bottom in either direction.
            if left.trim().is_empty() && !right.trim().is_empty() {
                return std::cmp::Ordering::Greater;
            }
            if !left.trim().is_empty() && right.trim().is_empty() {
                return std::cmp::Ordering::Less;
            }
            let order = Self::compare(left, right);
            if descending {
                order.reverse()
            } else {
                order
            }
        });
    }
}

fn patient_name(patients: &[Patient], pat_num: Id) -> String {
    patients
        .iter()
        .find(|p| p.pat_num == pat_num)
        .map(|p| p.display_name())
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

/// OD's recall due list, sorted overdue first. `DateDue` (the user-editable
/// field) wins over OD's calculated `DateDueCalc`, exactly as in the
/// Appointments → Recalls tab.
pub fn recall_report(recalls: &[Recall], patients: &[Patient], today: NaiveDate) -> Vec<ReportRow> {
    let mut rows: Vec<(RecallDueState, NaiveDate, ReportRow)> = recalls
        .iter()
        .map(|recall| {
            let state = recall_due_state(recall, today);
            let due = recall_due_date(recall).unwrap_or(NaiveDate::MAX);
            let detail = match recall_due_date(recall) {
                Some(date) => date.to_string(),
                None => "no date".into(),
            };
            let detail = if recall.note.trim().is_empty() {
                detail
            } else {
                format!("{detail} — {}", recall.note.trim())
            };
            (
                state,
                due,
                ReportRow::new(
                    patient_name(patients, recall.pat_num),
                    state.label(),
                    detail,
                ),
            )
        })
        .collect();
    rows.sort_by_key(|(state, due, row)| (state.sort_rank(), *due, row.label.clone()));
    rows.into_iter().map(|(_, _, row)| row).collect()
}

/// ASAP appointments in a date range, oldest first. Derived from
/// `appointment.Priority = 1`, the same query the Appointments → ASAP tab uses.
pub fn asap_report(
    appointments: &[Appointment],
    patients: &[Patient],
    from: NaiveDateTime,
    to: NaiveDateTime,
) -> Vec<ReportRow> {
    let mut rows: Vec<(NaiveDateTime, ReportRow)> = asap_appointments(appointments)
        .into_iter()
        .filter(|apt| apt.apt_datetime >= from && apt.apt_datetime <= to)
        .map(|apt| {
            (
                apt.apt_datetime,
                ReportRow::new(
                    patient_name(patients, apt.pat_num),
                    format!(
                        "{} {}",
                        apt.apt_datetime.format("%Y-%m-%d %H:%M"),
                        apt.apt_status.label()
                    ),
                    apt.notes.clone(),
                ),
            )
        })
        .collect();
    rows.sort_by_key(|(when, _)| *when);
    rows.into_iter().map(|(_, row)| row).collect()
}

/// Per-payment-plan balances as of a date, plus a total row.
///
/// Charges due come from OD `payplancharge` (`principal + interest`, with credit
/// charges reducing the balance) and receipts from `paysplit.PayPlanNum`. This
/// is the same calculation the Account module shows, so the report cannot
/// disagree with the ledger.
pub fn payment_plan_report(
    plans: &[PayPlan],
    charges: &[PayPlanCharge],
    splits: &[PaySplit],
    patients: &[Patient],
    as_of: NaiveDate,
) -> Vec<ReportRow> {
    let mut entries: Vec<(f64, ReportRow)> = plans
        .iter()
        .map(|plan| {
            let breakdown = crate::pay_plan_balance_breakdown(plan, charges, splits, as_of);
            let balance = breakdown.total_balance - breakdown.unapplied_credit;
            let due_now_signed = breakdown.due_now - if breakdown.due_now <= 0.005 { breakdown.unapplied_credit } else { 0.0 };
            let charge_count = charges
                .iter()
                .filter(|c| c.pay_plan_num == plan.pay_plan_num && c.charge_date <= as_of)
                .count();
            let kind = if plan.is_dynamic { "dynamic" } else { "static" };
            let detail = format!(
                "Plan #{} · {} · {} · {} charge(s) due to date · due now {:.2} · future {:.2} · unapplied {:.2} · {}",
                plan.pay_plan_num,
                kind,
                if plan.is_closed { "closed" } else { "open" },
                charge_count,
                due_now_signed,
                breakdown.future_due,
                breakdown.unapplied_credit,
                plan.pay_plan_date
            );
            (
                balance,
                ReportRow::new(
                    patient_name(patients, plan.pat_num),
                    format!("{balance:.2}"),
                    detail,
                ),
            )
        })
        .collect();
    entries.sort_by(|a, b| b.0.total_cmp(&a.0));
    let total: f64 = entries.iter().map(|(balance, _)| *balance).sum();
    let mut total_due = 0.0_f64;
    let mut total_future = 0.0_f64;
    let mut total_unapplied = 0.0_f64;
    for p in plans {
        let b = crate::pay_plan_balance_breakdown(p, charges, splits, as_of);
        total_due += b.due_now - if b.due_now <= 0.005 { b.unapplied_credit } else { 0.0 };
        total_future += b.future_due;
        total_unapplied += b.unapplied_credit;
    }
    let mut rows: Vec<ReportRow> = entries.into_iter().map(|(_, row)| row).collect();
    rows.push(ReportRow::new(
        "Total outstanding",
        format!("{total:.2}"),
        format!(
            "{} plan(s) · due now {:.2} · future {:.2} · unapplied {:.2}",
            plans.len(), total_due, total_future, total_unapplied
        ),
    ));
    rows
}

/// Treatment-plan summary grouped by plan, plus a total row. Active/Inactive
/// plans read their lines through `treatplanattach`; Saved plans use the
/// immutable `proctp` snapshot. The store already resolves both into
/// `TreatPlanLine`, so the report can stay storage-agnostic.
pub fn treatment_plan_report(
    plans: &[TreatPlan],
    lines: &[TreatPlanLine],
    patients: &[Patient],
) -> Vec<ReportRow> {
    let mut rows: Vec<ReportRow> = plans
        .iter()
        .map(|plan| {
            let own_lines: Vec<TreatPlanLine> = lines
                .iter()
                .filter(|line| line.treat_plan_num == plan.treat_plan_num)
                .cloned()
                .collect();
            let total = treat_plan_total(&own_lines);
            ReportRow::new(
                patient_name(patients, plan.pat_num),
                format!("{total:.2}"),
                format!(
                    "{} · {} · {} line(s)",
                    plan.heading,
                    plan.status.label(),
                    own_lines.len()
                ),
            )
        })
        .collect();
    rows.sort_by(|a, b| a.label.cmp(&b.label).then(a.detail.cmp(&b.detail)));
    let total: f64 = plans
        .iter()
        .map(|plan| {
            treat_plan_total(
                &lines
                    .iter()
                    .filter(|line| line.treat_plan_num == plan.treat_plan_num)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        })
        .sum();
    rows.push(ReportRow::new(
        "Total planned",
        format!("{total:.2}"),
        format!("{} plan(s)", plans.len()),
    ));
    rows
}

// ---------------------------------------------------------------------------
// Upstream tranche 1 — ported from the public 24_3 C# source
// ---------------------------------------------------------------------------

/// `FormRpAging` / `Ledgers.GetAgingQueryString`: patient balances split into
/// 0–30, 31–60, 61–90 and over-90 buckets. Like OD, credits are applied to the
/// oldest bucket first, so a payment clears old debt before new debt. Charges
/// after the as-of date are excluded, which makes the report reproducible for
/// a past date rather than only "today". The `options` argument mirrors the
/// form's "Age of Account", "Include/Only Patients With", "Age write-off
/// estimates" and "Show last payment date" controls.
pub fn aging_report(
    patients: &[Patient],
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[PaySplit],
    claim_procs: &[ClaimProc],
    as_of: NaiveDate,
    insurance_enabled: bool,
    options: AgingOptions,
) -> ReportTable {
    let mut headers = vec![
        "Patient",
        "0-30 Days",
        "31-60 Days",
        "61-90 Days",
        "> 90 Days",
        "Total",
    ];
    // Write-offs and insurance estimate columns are insurance concepts; keep
    // them hidden for the default Ethiopia-style configuration.
    let show_writeoffs = insurance_enabled && options.age_writeoffs;
    if show_writeoffs {
        headers.push("-W/O Change");
    }
    if insurance_enabled {
        headers.extend(["-Ins Est", "=Patient"]);
    }
    if options.show_last_pay {
        headers.push("Last Pay Date");
    }
    let mut table = ReportTable::new(&headers);

    let day30 = as_of - chrono::Duration::days(30);
    let day60 = as_of - chrono::Duration::days(60);
    let day90 = as_of - chrono::Duration::days(90);

    let mut totals = [0.0_f64; 4];
    let mut grand_total = 0.0;
    let mut writeoff_total = 0.0;
    let mut ins_estimate_total = 0.0;
    let mut patient_total = 0.0;
    for patient in patients {
        let mut charges: Vec<(NaiveDate, f64)> = procedures
            .iter()
            .filter(|p| {
                p.pat_num == patient.pat_num && p.proc_status.is_completed() && p.proc_date <= as_of
            })
            .map(|p| (p.proc_date, p.fee))
            .collect();
        charges.extend(
            adjustments
                .iter()
                .filter(|a| a.pat_num == patient.pat_num && a.adj_date <= as_of)
                .map(|a| (a.adj_date, a.adj_amount)),
        );

        let patient_splits: Vec<&PaySplit> = splits
            .iter()
            .filter(|s| s.pat_num == patient.pat_num && s.date_pay <= as_of)
            .collect();
        let mut credits: f64 = patient_splits.iter().map(|s| s.split_amount).sum();
        let mut ins_estimate = 0.0;
        let mut write_off = 0.0;
        if insurance_enabled {
            // A claimproc links to a procedure, not directly to a patient, so
            // resolve the patient's procedure numbers first.
            let own_proc_nums: Vec<Id> = procedures
                .iter()
                .filter(|p| p.pat_num == patient.pat_num)
                .map(|p| p.proc_num)
                .collect();
            for cp in claim_procs
                .iter()
                .filter(|cp| own_proc_nums.contains(&cp.proc_num))
            {
                write_off += cp.write_off;
                match cp.status {
                    0 => ins_estimate += cp.ins_pay_est + cp.write_off,
                    _ => credits += cp.ins_pay_amt + cp.write_off,
                }
            }
        }

        let total: f64 = charges.iter().map(|(_, amount)| *amount).sum::<f64>() - credits;
        // OD hides zero-balance patients by default. Negative balances need
        // the "Negative balances" checkbox; "Only negative" flips the filter.
        if total.abs() < 0.005 {
            continue;
        }
        if total < -0.005 && !options.include_negative {
            continue;
        }
        if total > 0.005 && options.only_negative {
            continue;
        }

        // Index 0 = >90 days (oldest), 3 = 0–30 days (newest).
        let mut buckets = [0.0_f64; 4];
        for (date, amount) in charges {
            let index = if date < day90 {
                0
            } else if date < day60 {
                1
            } else if date < day30 {
                2
            } else {
                3
            };
            buckets[index] += amount;
        }
        // Apply credits from the oldest bucket forward, matching the CASE
        // expression in Ledgers.GetAgingQueryString.
        let mut remaining = credits;
        for bucket in buckets.iter_mut() {
            if remaining <= 0.0 {
                break;
            }
            let used = remaining.min(*bucket);
            *bucket -= used;
            remaining -= used;
        }
        // A credit balance stays in the newest bucket, as OD shows it.
        if remaining > 0.0 {
            buckets[3] -= remaining;
        }

        // "Age of Account" filters on the oldest outstanding bucket.
        let oldest_bucket = buckets
            .iter()
            .enumerate()
            .find(|(_, amount)| **amount > 0.005)
            .map(|(index, _)| index);
        let include_by_age = match options.age {
            AgingAgeFilter::Any => true,
            AgingAgeFilter::Over30 => oldest_bucket.is_some_and(|index| index <= 2),
            AgingAgeFilter::Over60 => oldest_bucket.is_some_and(|index| index <= 1),
            AgingAgeFilter::Over90 => oldest_bucket == Some(0),
        };
        if !include_by_age {
            continue;
        }

        let last_pay = patient_splits
            .iter()
            .map(|s| s.date_pay)
            .max()
            .map(|date| date.to_string())
            .unwrap_or_default();
        let mut values: Vec<String> = vec![
            patient.display_name(),
            format!("{:.2}", buckets[3]),
            format!("{:.2}", buckets[2]),
            format!("{:.2}", buckets[1]),
            format!("{:.2}", buckets[0]),
            format!("{total:.2}"),
        ];
        if show_writeoffs {
            values.push(format!("{write_off:.2}"));
            writeoff_total += write_off;
        }
        if insurance_enabled {
            values.push(format!("{ins_estimate:.2}"));
            values.push(format!("{:.2}", total - ins_estimate));
            ins_estimate_total += ins_estimate;
            patient_total += total - ins_estimate;
        }
        if options.show_last_pay {
            values.push(last_pay);
        }
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());

        for (index, bucket) in buckets.iter().enumerate() {
            totals[index] += bucket;
        }
        grand_total += total;
    }

    let mut footer: Vec<String> = vec![
        "Total".into(),
        format!("{:.2}", totals[3]),
        format!("{:.2}", totals[2]),
        format!("{:.2}", totals[1]),
        format!("{:.2}", totals[0]),
        format!("{grand_total:.2}"),
    ];
    if show_writeoffs {
        footer.push(format!("{writeoff_total:.2}"));
    }
    if insurance_enabled {
        footer.push(format!("{ins_estimate_total:.2}"));
        footer.push(format!("{patient_total:.2}"));
    }
    if options.show_last_pay {
        footer.push(String::new());
    }
    table.push(&footer.iter().map(String::as_str).collect::<Vec<_>>());
    table
}

/// `FormRpCustomAging` / `RpCustomAging.GetAgingList`: the selectable-transaction
/// version of patient aging. Each source table can be switched on or off,
/// credits may be aged in their own bucket (`Age Credits`) or applied
/// oldest-first, and rows are filtered by Age of Account, negative balance,
/// provider, clinic, billing type and patient status exactly as upstream.
///
/// Family grouping aggregates every member's transactions under the guarantor,
/// as `SELECT p.Guarantor PatNum ... GROUP BY p.Guarantor` does upstream. The
/// four insurance transaction types follow `RpCustomAging`'s UNION queries:
/// `InsPay` (status 1/4/5/7, `-InsPayAmt`), `InsEst` (status 0, `-InsPayEst`),
/// `Writeoff` (status 1/4/5/7, `-WriteOff`) and `WriteoffEst` (status 0 or a
/// received claim dated after the as-of date). Because the port has no
/// `claimproc.DateCP`, the insurance payment date is `DateSuppReceived` with
/// the procedure date as fallback. All four are gated by the Insurance &
/// claims feature toggle. "Pay Plan Charges" are the plan's debit charges;
/// "Pay Plan Production" are credit charges. Payments attached to a plan only
/// enter when both plan transaction types are selected, mirroring upstream's
/// guard against unmatched plan credits.
#[allow(clippy::too_many_arguments)]
pub fn custom_aging_report(
    patients: &[Patient],
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[PaySplit],
    plan_charges: &[PayPlanCharge],
    claim_procs: &[ClaimProc],
    claim_snapshots: &[ClaimSnapshot],
    as_of: NaiveDate,
    insurance_enabled: bool,
    options: CustomAgingReportOptions,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Patient",
        "0-30 Days",
        "31-60 Days",
        "61-90 Days",
        "> 90 Days",
        "Total",
    ]);
    // Upstream refuses to refresh with no transaction type selected.
    if !options.include.any() {
        return table;
    }

    #[derive(Clone, Copy)]
    enum Kind {
        Charge,
        Credit,
    }
    #[derive(Clone, Copy)]
    struct Tran {
        date: NaiveDate,
        amount: f64,
        kind: Kind,
    }
    struct Row {
        name: String,
        last: String,
        first: String,
        buckets: [f64; 4],
        total: f64,
    }

    let day30 = as_of - chrono::Duration::days(30);
    let day60 = as_of - chrono::Duration::days(60);
    let day90 = as_of - chrono::Duration::days(90);
    let bucket_index = |date: NaiveDate| -> usize {
        if date < day90 {
            0 // > 90 days (oldest)
        } else if date < day60 {
            1
        } else if date < day30 {
            2
        } else {
            3 // 0–30 days (newest)
        }
    };
    // Upstream rounds each bucket to three decimals in SQL before filtering.
    let round3 = |amount: f64| (amount * 1000.0).round() / 1000.0;
    let signed = |amount: f64| -> Kind {
        if amount >= 0.0 {
            Kind::Charge
        } else {
            Kind::Credit
        }
    };

    let include: CustomAgingInclude = options.include;
    let both_plan_sides = include.pay_plan_charges && include.pay_plan_production;
    let insurance = insurance_enabled
        && (include.insurance_payments
            || include.insurance_estimates
            || include.write_offs
            || include.write_off_estimates);

    // Upstream groups by patient.PatNum (Individual) or patient.Guarantor
    // (Family) after joining the transactions back to the patient table.
    let mut groups: BTreeMap<Id, Vec<&Patient>> = BTreeMap::new();
    for patient in patients {
        let key = match options.family_grouping {
            AgingFamilyGrouping::Individual => patient.pat_num,
            AgingFamilyGrouping::Family if patient.guarantor != 0 => patient.guarantor,
            AgingFamilyGrouping::Family => patient.pat_num,
        };
        groups.entry(key).or_default().push(patient);
    }

    let mut rows: Vec<Row> = Vec::new();
    for (group_num, members) in groups {
        // Provider, clinic, billing-type and status filters apply to the
        // guarantor row after grouping, matching upstream's final WHERE.
        let Some(representative) = members
            .iter()
            .find(|p| p.pat_num == group_num)
            .or_else(|| members.first())
            .copied()
        else {
            continue;
        };
        if !options.all_providers
            && !options.provider_nums.contains(&representative.primary_provider_num)
        {
            continue;
        }
        if !options.all_clinics && !options.clinic_nums.contains(&representative.clinic_num) {
            continue;
        }
        if !options.bill_type_nums.is_empty()
            && !options.bill_type_nums.contains(&representative.billing_type)
        {
            continue;
        }
        if options.exclude_inactive && representative.pat_status == 2 {
            continue;
        }
        if options.exclude_archive && representative.pat_status == 3 {
            continue;
        }
        if options.exclude_bad_address && representative.zip.trim().is_empty() {
            continue;
        }

        let mut transactions: Vec<Tran> = Vec::new();
        for member in &members {
            let pat_num = member.pat_num;
        if include.procedure_fees {
            transactions.extend(procedures.iter().filter_map(|proc| {
                if proc.pat_num != pat_num
                    || !proc.proc_status.is_completed()
                    || proc.proc_date > as_of
                {
                    return None;
                }
                let amount = proc.fee * (proc.unit_qty + proc.base_units);
                if amount == 0.0 {
                    return None;
                }
                Some(Tran {
                    date: proc.proc_date,
                    amount,
                    kind: signed(amount),
                })
            }));
        }
        if include.adjustments {
            transactions.extend(adjustments.iter().filter_map(|adj| {
                if adj.pat_num != pat_num || adj.adj_date > as_of || adj.adj_amount == 0.0 {
                    return None;
                }
                Some(Tran {
                    date: adj.adj_date,
                    amount: adj.adj_amount,
                    kind: signed(adj.adj_amount),
                })
            }));
        }
        if include.pay_plan_charges || include.pay_plan_production {
            for charge in plan_charges
                .iter()
                .filter(|charge| charge.pat_num == pat_num && charge.charge_date <= as_of)
            {
                match charge.charge_type {
                    PayPlanChargeType::Debit if include.pay_plan_charges => {
                        let amount = charge.principal + charge.interest;
                        if amount == 0.0 {
                            continue;
                        }
                        transactions.push(Tran {
                            date: charge.charge_date,
                            amount,
                            kind: signed(amount),
                        });
                    }
                    PayPlanChargeType::Credit if include.pay_plan_production => {
                        transactions.push(Tran {
                            date: charge.charge_date,
                            amount: -charge.principal.abs(),
                            kind: Kind::Credit,
                        });
                    }
                    _ => {}
                }
            }
        }
        if include.patient_payments {
            transactions.extend(splits.iter().filter_map(|split| {
                if split.pat_num != pat_num || split.date_pay > as_of {
                    return None;
                }
                if !both_plan_sides && split.pay_plan_num != 0 {
                    return None;
                }
                if split.split_amount == 0.0 {
                    return None;
                }
                Some(Tran {
                    date: split.date_pay,
                    // Upstream forces every PatPay row into the credit side,
                    // even when a refund split has a negative SplitAmt.
                    amount: -split.split_amount.abs(),
                    kind: Kind::Credit,
                })
            }));
        }
        if insurance {
            // Upstream's paid statuses are Received(1), Supplemental(4),
            // CapClaim(5) and CapComplete(7); NotReceived is 0.
            let paid = |status: i8| matches!(status, 1 | 4 | 5 | 7);
            let member_procs: BTreeMap<Id, NaiveDate> = procedures
                .iter()
                .filter(|p| p.pat_num == pat_num)
                .map(|p| (p.proc_num, p.proc_date))
                .collect();
            for cp in claim_procs
                .iter()
                .filter(|cp| member_procs.contains_key(&cp.proc_num))
            {
                let proc_date = member_procs.get(&cp.proc_num).copied();
                // The port has no DateCP; DateSuppReceived is its insurance
                // payment date, falling back to the procedure date.
                let cp_date = cp.date_supp_received.or(proc_date);
                if include.insurance_payments
                    && paid(cp.status)
                    && cp.ins_pay_amt != 0.0
                    && cp.plan_num == 0
                {
                    if let Some(date) = cp_date.filter(|d| *d <= as_of) {
                        transactions.push(Tran {
                            date,
                            amount: -cp.ins_pay_amt,
                            kind: Kind::Credit,
                        });
                    }
                }
                if include.insurance_estimates && cp.status == 0 && cp.ins_pay_est != 0.0 {
                    if let Some(date) = cp_date.filter(|d| *d <= as_of) {
                        transactions.push(Tran {
                            date,
                            amount: -cp.ins_pay_est,
                            kind: Kind::Credit,
                        });
                    }
                }
                if include.write_offs && paid(cp.status) && cp.write_off != 0.0 {
                    let snapshot = claim_snapshots
                        .iter()
                        .find(|s| s.claim_proc_num == cp.claim_proc_num);
                    match options.writeoff_date {
                        CustomAgingWriteoffDate::ProcedureDate => {
                            if let Some(date) = proc_date.filter(|d| *d <= as_of) {
                                transactions.push(Tran {
                                    date,
                                    amount: -cp.write_off,
                                    kind: Kind::Credit,
                                });
                            }
                        }
                        CustomAgingWriteoffDate::ClaimPayDate => {
                            // Upstream's two UNION branches: the snapshot
                            // write-off (or the live one with no snapshot)
                            // ages on ProcDate; the delta to the live
                            // write-off ages on DateCP only when a snapshot
                            // exists. A snapshot Writeoff of -1 nulls to 0.
                            let snap_wo = snapshot.map(|s| {
                                if s.write_off == -1.0 {
                                    0.0
                                } else {
                                    s.write_off
                                }
                            });
                            let first = snap_wo.map(|value| -value).unwrap_or(-cp.write_off);
                            if first != 0.0 {
                                if let Some(date) = proc_date.filter(|d| *d <= as_of) {
                                    transactions.push(Tran {
                                        date,
                                        amount: first,
                                        kind: Kind::Credit,
                                    });
                                }
                            }
                            if let Some(delta) = snap_wo.map(|value| value - cp.write_off) {
                                if delta != 0.0 {
                                    if let Some(date) =
                                        cp.date_supp_received.filter(|d| *d <= as_of)
                                    {
                                        transactions.push(Tran {
                                            date,
                                            amount: delta,
                                            kind: signed(delta),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                if include.write_off_estimates && cp.write_off != 0.0 {
                    let received_after =
                        cp.status == 1 && cp.date_supp_received.is_some_and(|d| d > as_of);
                    if cp.status == 0 || received_after {
                        if let Some(date) = proc_date.filter(|d| *d <= as_of) {
                            transactions.push(Tran {
                                date,
                                amount: -cp.write_off,
                                kind: Kind::Credit,
                            });
                        }
                    }
                }
            }
        }
        }

        let mut buckets = [0.0_f64; 4];
        let mut total = 0.0_f64;
        let mut total_credits = 0.0_f64;
        for tran in &transactions {
            total += tran.amount;
            let index = bucket_index(tran.date);
            match tran.kind {
                Kind::Charge => buckets[index] += tran.amount,
                Kind::Credit => {
                    total_credits += -tran.amount;
                    if options.age_credits {
                        buckets[index] += tran.amount;
                    }
                }
            }
        }
        if !options.age_credits {
            // Default upstream behaviour: consume credits from the oldest
            // bucket forward. Credits left over after every charge is cleared
            // leave all buckets at zero while the total stays negative.
            let mut remaining = total_credits;
            for bucket in buckets.iter_mut() {
                if remaining <= 0.0 {
                    break;
                }
                let used = remaining.min(*bucket);
                *bucket -= used;
                remaining -= used;
            }
        }
        for bucket in buckets.iter_mut() {
            *bucket = round3(*bucket);
        }
        let total = round3(total);

        let has_bucket = |indexes: &[usize]| indexes.iter().any(|index| buckets[*index] != 0.0);
        let age_included = match options.age {
            AgingAgeFilter::Any => has_bucket(&[0, 1, 2, 3]),
            AgingAgeFilter::Over30 => has_bucket(&[0, 1, 2]),
            AgingAgeFilter::Over60 => has_bucket(&[0, 1]),
            AgingAgeFilter::Over90 => buckets[0] != 0.0,
        };
        if !age_included {
            continue;
        }
        match options.negative {
            CustomAgingNegative::Exclude if total <= 0.0 => continue,
            CustomAgingNegative::Include if total == 0.0 => continue,
            CustomAgingNegative::ShowOnly if total >= 0.0 => continue,
            _ => {}
        }

        rows.push(Row {
            name: format!("{}, {}", representative.l_name, representative.f_name),
            last: representative.l_name.clone(),
            first: representative.f_name.clone(),
            buckets,
            total,
        });
    }

    rows.sort_by(|left, right| (&left.last, &left.first).cmp(&(&right.last, &right.first)));
    let mut totals = [0.0_f64; 4];
    let mut grand_total = 0.0;
    for row in &rows {
        let values: Vec<String> = vec![
            row.name.clone(),
            format!("{:.2}", row.buckets[3]),
            format!("{:.2}", row.buckets[2]),
            format!("{:.2}", row.buckets[1]),
            format!("{:.2}", row.buckets[0]),
            format!("{:.2}", row.total),
        ];
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
        for (index, bucket) in row.buckets.iter().enumerate() {
            totals[index] += bucket;
        }
        grand_total += row.total;
    }

    let footer: Vec<String> = vec![
        "Totals:".into(),
        format!("{:.2}", totals[3]),
        format!("{:.2}", totals[2]),
        format!("{:.2}", totals[1]),
        format!("{:.2}", totals[0]),
        format!("{:.2}", grand_total),
    ];
    table.push(&footer.iter().map(String::as_str).collect::<Vec<_>>());
    table
}

/// Auditable detail: how payments were applied to plan charges.
pub fn pay_plan_charge_applications_report(
    plans: &[PayPlan],
    charges: &[PayPlanCharge],
    splits: &[PaySplit],
    patients: &[Patient],
    payments: &[Payment],
    from: NaiveDate,
    to: NaiveDate,
    only_refunds_transfers: bool,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Date",
        "Patient",
        "Plan",
        "Charge #",
        "Charge Due",
        "Debit Type",
        "Amount Applied",
        "Pay #",
        "Unapplied Plan Amt",
        "Note",
    ]);
    let mut rows: Vec<(NaiveDate, Vec<String>)> = Vec::new();
    for split in splits
        .iter()
        .filter(|s| s.pay_plan_num != 0 && s.date_pay >= from && s.date_pay <= to)
    {
        let is_refund_or_transfer = split.split_amount < -0.005
            || (split.pay_plan_charge_num == 0 && split.split_amount < -0.005);
        if only_refunds_transfers && !is_refund_or_transfer {
            continue;
        }
        let plan = plans.iter().find(|p| p.pay_plan_num == split.pay_plan_num);
        let Some(plan) = plan else { continue; };
        let charge = charges.iter().find(|c| c.pay_plan_charge_num == split.pay_plan_charge_num);
        let charge_due = charge.map(|c| c.charge_date.format("%Y-%m-%d").to_string()).unwrap_or_default();
        let debit_type = if split.pay_plan_charge_num == 0 {
            if split.split_amount < 0.0 { "Transfer/Resolve".to_string() } else { "Unapplied".to_string() }
        } else {
            split.pay_plan_debit_type.label().to_string()
        };
        let unapplied = if split.pay_plan_charge_num == 0 { split.split_amount } else { 0.0 };
        let note = payments
            .iter()
            .find(|p| p.pay_num == split.pay_num)
            .map(|p| p.note.replace('\n', " "))
            .unwrap_or_default();
        let direction = if split.split_amount < -0.005 {
            format!("Plan #{} -> account/unallocated", split.pay_plan_num)
        } else if split.pay_plan_charge_num == 0 {
            format!("Account/unallocated -> Plan #{}", split.pay_plan_num)
        } else {
            format!("Account/unallocated -> Plan #{} charge #{}", split.pay_plan_num, split.pay_plan_charge_num)
        };
        let note_with_direction = if note.is_empty() { direction.clone() } else { format!("{note} [{direction}]") };
        rows.push((split.date_pay, vec![
            split.date_pay.format("%Y-%m-%d").to_string(),
            patient_name(patients, plan.pat_num),
            format!("#{} {}", plan.pay_plan_num, if plan.is_dynamic { "dynamic" } else { "static" }),
            if split.pay_plan_charge_num == 0 { String::new() } else { split.pay_plan_charge_num.to_string() },
            charge_due,
            debit_type,
            format!("{:.2}", split.split_amount),
            split.pay_num.to_string(),
            format!("{:.2}", unapplied),
            note_with_direction,
        ]));
    }
    rows.sort_by_key(|(d, _)| *d);
    for (_, row) in rows {
        table.push(&row.iter().map(String::as_str).collect::<Vec<_>>());
    }
    table
}

/// `FormRpPayPlans` / `RpPayPlan.GetPayPlanTable`: the full payment-plan A/R
/// columns instead of the single balance in `payment_plan_report`. `Princ` is
/// the plan's total principal, `Accum Int` the interest charged by the as-of
/// date, `Due Now` the charges already due, and `Bal Not Due` OD's v2 column
/// for the remainder. The `options` argument mirrors the form's "Payment Plan
/// Types", "Hide Completed", "Show Family Balance" and date-range controls.
#[allow(clippy::too_many_arguments)]
pub fn pay_plan_detail_report(
    plans: &[PayPlan],
    charges: &[PayPlanCharge],
    splits: &[PaySplit],
    links: &[PayPlanLink],
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    patients: &[Patient],
    providers: &[Provider],
    family_balances: &[(Id, f64)],
    from: NaiveDate,
    as_of: NaiveDate,
    insurance_enabled: bool,
    options: PayPlanReportOptions,
) -> ReportTable {
    let mut headers = vec!["Provider", "Guarantor"];
    if insurance_enabled {
        headers.push("Ins");
    }
    headers.extend([
        "Princ",
        "Accum Int",
        "Paid",
        "Balance",
        "Due Now",
        "Bal Not Due",
        "Unapplied Credit",
    ]);
    if options.show_family_balance {
        headers.push("Fam Balance");
    }
    let mut table = ReportTable::new(&headers);

    // Column layout: Provider, Guarantor, [Ins], six amounts, [Fam Balance].
    // This mirrors the order used when the headers were built above.
    let ins_offset = usize::from(insurance_enabled);
    let mut numeric_columns: Vec<usize> = (0..7).map(|index| 2 + ins_offset + index).collect();
    if options.show_family_balance {
        numeric_columns.push(9 + ins_offset);
    }
    let mut totals = vec![0.0_f64; numeric_columns.len()];
    let mut shown = 0usize;

    let mut ordered: Vec<&PayPlan> = plans
        .iter()
        .filter(|plan| {
            if options.hide_completed && plan.is_closed {
                return false;
            }
            if options.limit_date_range && (plan.pay_plan_date < from || plan.pay_plan_date > as_of) {
                return false;
            }
            let is_insurance = plan.insurance_plan_num != 0;
            if !insurance_enabled {
                // Ethiopia-style configuration: patient plans only.
                return !is_insurance;
            }
            match options.plan_type {
                PlanTypeFilter::Both => true,
                PlanTypeFilter::Patient => !is_insurance,
                PlanTypeFilter::Insurance => is_insurance,
            }
        })
        .collect();
    ordered.sort_by_key(|plan| {
        (
            patient_name(patients, plan.guarantor),
            patient_name(patients, plan.pat_num),
        )
    });

    for plan in ordered {
        let own_charges: Vec<&PayPlanCharge> = charges
            .iter()
            .filter(|charge| charge.pay_plan_num == plan.pay_plan_num)
            .collect();
        let debits: Vec<&PayPlanCharge> = own_charges
            .iter()
            .copied()
            .filter(|charge| charge.charge_type == PayPlanChargeType::Debit)
            .collect();

        // OD sums debit principal for a static plan; a dynamic plan takes its
        // principal from linked production.
        let princ = if plan.is_dynamic {
            let own_links: Vec<PayPlanLink> = links
                .iter()
                .filter(|l| l.pay_plan_num == plan.pay_plan_num)
                .cloned()
                .collect();
            let entries = crate::production_entries(&own_links, procedures, adjustments, splits);
            let linked = crate::principal_amount(&entries);
            if linked > 0.0 {
                linked
            } else if plan.completed_amt > 0.0 {
                plan.completed_amt
            } else {
                debits.iter().map(|charge| charge.principal).sum::<f64>()
            }
        } else {
            debits.iter().map(|charge| charge.principal).sum::<f64>()
        };
        let interest: f64 = debits
            .iter()
            .filter(|charge| charge.charge_date <= as_of)
            .map(|charge| charge.interest)
            .sum();
        let paid: f64 = splits
            .iter()
            .filter(|split| split.pay_plan_num == plan.pay_plan_num)
            .map(|split| split.split_amount)
            .sum();
        let breakdown = crate::pay_plan_balance_breakdown(plan, charges, splits, as_of);
        let due = breakdown.due_now;
        let unapplied_credit = breakdown.unapplied_credit;
        let not_due = breakdown.future_due;
        let balance = breakdown.total_balance;
        let family_balance = family_balances
            .iter()
            .find(|(pat_num, _)| *pat_num == plan.guarantor)
            .map(|(_, balance)| *balance)
            .unwrap_or(0.0);
        let provider = debits
            .iter()
            .find(|charge| charge.provider_num != 0)
            .map(|charge| charge.provider_num)
            .unwrap_or(0);

        let mut values: Vec<String> = vec![
            provider_name(providers, provider),
            patient_name(patients, plan.guarantor),
        ];
        if insurance_enabled {
            values.push(if plan.insurance_plan_num == 0 {
                String::new()
            } else {
                format!("Ins #{}", plan.insurance_plan_num)
            });
        }
        values.extend([
            format!("{princ:.2}"),
            format!("{interest:.2}"),
            format!("{paid:.2}"),
            format!("{balance:.2}"),
            format!("{due:.2}"),
            format!("{not_due:.2}"),
            format!("{unapplied_credit:.2}"),
        ]);
        if options.show_family_balance {
            values.push(format!("{family_balance:.2}"));
        }
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());

        for (total_index, column) in numeric_columns.iter().enumerate() {
            if let Ok(number) = values[*column].parse::<f64>() {
                totals[total_index] += number;
            }
        }
        shown += 1;
    }

    let mut footer = vec![String::new(); headers.len()];
    footer[0] = format!("Total ({shown} plan(s))");
    for (total_index, column) in numeric_columns.iter().enumerate() {
        footer[*column] = format!("{:.2}", totals[total_index]);
    }
    table.push(&footer.iter().map(String::as_str).collect::<Vec<_>>());
    table
}

/// `FormRpBirthday`: patients whose birthday falls inside the selected range.
/// OD compares month and day, so the year of birth is ignored and a range that
/// wraps the new year (from > to) still works. The age is calculated as of the
/// report date, as in the original's `Age` column.
fn month_day_ordinal(month: u32, day: u32) -> u32 {
    const MONTH_STARTS: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    MONTH_STARTS[(month - 1) as usize] + day
}

pub fn birthday_report(
    patients: &[Patient],
    from: NaiveDate,
    to: NaiveDate,
    as_of: NaiveDate,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "LName", "FName", "Preferred", "Address", "Add2", "City", "State", "Zip", "Birthdate",
        "Age",
    ]);
    let from_ordinal = month_day_ordinal(from.month(), from.day());
    let to_ordinal = month_day_ordinal(to.month(), to.day());
    let mut entries: Vec<(u32, String, Patient)> = patients
        .iter()
        .filter_map(|patient| {
            let birth = patient.birth_date?;
            let ordinal = month_day_ordinal(birth.month(), birth.day());
            // Rotate the calendar so a range wrapping the new year stays in
            // chronological order from the selected start date.
            let offset = (ordinal + 365 - from_ordinal) % 365;
            let in_range = if from_ordinal <= to_ordinal {
                from_ordinal <= ordinal && ordinal <= to_ordinal
            } else {
                ordinal >= from_ordinal || ordinal <= to_ordinal
            };
            in_range.then(|| (offset, patient.display_name(), patient.clone()))
        })
        .collect();
    entries.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    for (_, _, patient) in entries {
        let birth = patient.birth_date.unwrap();
        let mut age = as_of.year() - birth.year();
        if (as_of.month(), as_of.day()) < (birth.month(), birth.day()) {
            age -= 1;
        }
        let values = [
            patient.l_name.clone(),
            patient.f_name.clone(),
            String::new(), // Preferred is not stored in this port.
            patient.address.clone(),
            String::new(), // Address2 is not stored in this port.
            patient.city.clone(),
            patient.state.clone(),
            patient.zip.clone(),
            birth.to_string(),
            age.to_string(),
        ];
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }
    if table.rows.is_empty() {
        table.push(&["No birthdays in range", "", "", "", "", "", "", "", "", ""]);
    }
    table
}

/// `FormRpNewPatients` / `RpNewPatients.GetNewPatients`: a patient is "new"
/// when their first completed procedure falls in the range, not when their
/// record was created. `Production Fee` sums all completed procedures for that
/// patient, exactly as the upstream query does. Referral and preferred name
/// are kept as empty columns because this port does not store those fields.
pub fn new_patient_report(
    patients: &[Patient],
    procedures: &[Procedure],
    from: NaiveDate,
    to: NaiveDate,
    options: NewPatientReportOptions,
) -> ReportTable {
    let mut headers = vec!["#", "Date", "LName", "FName", "Referral", "Production Fee"];
    if options.include_address {
        headers.extend(["Pref'd", "Address", "Add2", "City", "ST", "Zip"]);
    }
    let mut table = ReportTable::new(&headers);

    let mut entries: Vec<(NaiveDate, String, String, String, f64, Patient)> = patients
        .iter()
        .filter_map(|patient| {
            let own: Vec<&Procedure> = procedures
                .iter()
                .filter(|p| p.pat_num == patient.pat_num && p.proc_status.is_completed())
                .collect();
            let first = own.iter().map(|p| p.proc_date).min()?;
            if first < from || first > to {
                return None;
            }
            let production: f64 = own.iter().map(|p| p.fee).sum();
            if options.exclude_no_production && production <= 0.0 {
                return None;
            }
            Some((
                first,
                patient.l_name.clone(),
                patient.f_name.clone(),
                patient.display_name(),
                production,
                patient.clone(),
            ))
        })
        .collect();
    entries.sort_by(|a, b| (a.0, &a.1, &a.2).cmp(&(b.0, &b.1, &b.2)));

    for (number, (date, _, _, _, production, patient)) in entries.iter().enumerate() {
        let mut values = vec![
            (number + 1).to_string(),
            date.to_string(),
            patient.l_name.clone(),
            patient.f_name.clone(),
            String::new(), // Referral is not stored in this port.
            format!("{production:.2}"),
        ];
        if options.include_address {
            values.extend([
                String::new(), // Preferred name is not stored in this port.
                patient.address.clone(),
                String::new(), // Address2 is not stored in this port.
                patient.city.clone(),
                patient.state.clone(),
                patient.zip.clone(),
            ]);
        }
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }
    let footer = format!("Total ({} patient(s))", entries.len());
    let mut footer_values = vec![footer];
    footer_values.resize(headers.len(), String::new());
    table.push(&footer_values.iter().map(String::as_str).collect::<Vec<_>>());
    table
}

// ---------------------------------------------------------------------------
// Upstream tranche 2 — insurance-free FormRp* ports
// ---------------------------------------------------------------------------

/// Full years between a birth date and an as-of date, matching OD's
/// `Patients.DateToAge` used by the Age display field.
fn age_on(birth: NaiveDate, as_of: NaiveDate) -> String {
    let mut age = as_of.year() - birth.year();
    if (as_of.month(), as_of.day()) < (birth.month(), birth.day()) {
        age -= 1;
    }
    age.to_string()
}

fn provider_abbr(providers: &[Provider], provider_num: Id) -> String {
    providers
        .iter()
        .find(|p| p.provider_num == provider_num)
        .map(|p| p.abbreviation.clone())
        .unwrap_or_default()
}

/// `FormRpAppointments` / `RpAppointments.GetAppointmentTable`: one row per
/// real booking in the date range. OD always excludes Planned (6) and
/// UnschedList (3); PtNote/PtNoteCompleted (7/8) need the option. The upstream
/// `BETWEEN dateFrom AND dateTo.AddDays(1)` upper edge is kept so the last day
/// is included in full. "Work Ph." stays blank because this port's `Patient`
/// does not store `WkPhone`, and "Description" uses the appointment note as the
/// closest available stand-in for OD's `ProcDescript` snapshot.
pub fn appointments_report(
    appointments: &[Appointment],
    patients: &[Patient],
    from: NaiveDate,
    to: NaiveDate,
    as_of: NaiveDate,
    options: AppointmentReportOptions,
) -> ReportTable {
    let created = options.date_basis == AppointmentDateBasis::Created;
    let mut headers: Vec<&str> = if created {
        vec!["DateCreated", "AptDate"]
    } else {
        vec!["Date"]
    };
    headers.extend([
        "PatNum",
        "Patient",
        "Age",
        "Time",
        "Length",
        "Description",
        "Home Ph.",
        "Work Ph.",
        "Cell Ph.",
    ]);
    let mut table = ReportTable::new(&headers);
    let exclusive_end = to + chrono::Duration::days(1);

    let mut entries: Vec<(NaiveDate, String, Vec<String>)> = appointments
        .iter()
        .filter(|apt| {
            let status = apt.apt_status.as_od();
            if status == 3 || status == 6 {
                return false;
            }
            if !options.show_note_appointments && (status == 7 || status == 8) {
                return false;
            }
            let date = apt.apt_datetime.date();
            date >= from && date < exclusive_end
        })
        .filter_map(|apt| {
            let patient = patients.iter().find(|p| p.pat_num == apt.pat_num)?;
            let apt_date = apt.apt_datetime.date();
            // SecDateTEntry is not stored in this port, so the created-date
            // mode falls back to the appointment date and says so in the README.
            let minutes = if apt.pattern.is_empty() {
                apt.apt_length
            } else {
                apt.pattern.chars().count() as u32 * 5
            };
            let mut values = Vec::new();
            if created {
                values.push(apt_date.to_string());
                values.push(apt_date.to_string());
            } else {
                values.push(apt_date.to_string());
            }
            values.extend([
                apt.pat_num.to_string(),
                patient.display_name(),
                patient
                    .birth_date
                    .map(|birth| age_on(birth, as_of))
                    .unwrap_or_default(),
                apt.apt_datetime.format("%I:%M %p").to_string(),
                minutes.to_string(),
                apt.notes.clone(),
                patient.home_phone.clone(),
                String::new(),
                patient.wireless_phone.clone(),
            ]);
            Some((apt_date, patient.display_name(), values))
        })
        .collect();
    entries.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    for (_, _, values) in entries {
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }
    table
}

/// `FormRpActivePatients` / `RpActivePatients.GetActivePatientTable`: one row
/// per patient with a completed procedure in the range. Upstream also filters
/// by billing type, patient status and clinic; those fields are not on this
/// port's `Patient`, so they are omitted rather than represented by fake
/// controls. "Address2" and "Carrier" are blank for the same reason (no
/// address line 2, and insurance is off in the default configuration).
pub fn active_patient_report(
    patients: &[Patient],
    procedures: &[Procedure],
    providers: &[Provider],
    from: NaiveDate,
    to: NaiveDate,
    options: ActivePatientReportOptions,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Name", "Provider", "Address", "Address2", "City", "State", "Zip", "Carrier",
    ]);
    let mut entries: Vec<(String, String, Vec<String>)> = patients
        .iter()
        .filter(|patient| {
            if !options.all_providers && patient.primary_provider_num != options.provider_num {
                return false;
            }
            procedures.iter().any(|proc| {
                proc.pat_num == patient.pat_num
                    && proc.proc_status.is_completed()
                    && proc.proc_date >= from
                    && proc.proc_date <= to
            })
        })
        .map(|patient| {
            let abbr = provider_abbr(providers, patient.primary_provider_num);
            let values = vec![
                patient.display_name(),
                abbr.clone(),
                patient.address.clone(),
                String::new(),
                patient.city.clone(),
                patient.state.clone(),
                patient.zip.clone(),
                String::new(),
            ];
            (abbr, patient.display_name(), values)
        })
        .collect();
    entries.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    for (_, _, values) in entries {
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }
    // Upstream puts a "Patient Count:" group-summary under the Carrier column.
    let mut footer = vec![String::new(); 8];
    footer[7] = format!("Patient Count: {}", table.rows.len());
    table.push(&footer.iter().map(String::as_str).collect::<Vec<_>>());
    table
}

/// One daily transaction row for `prod_inc_report`: a date, a sort name and
/// the already-formatted column values.
type ProdIncEntry = (NaiveDate, String, Vec<String>);

fn prod_inc_daily(
    patients: &[Patient],
    providers: &[Provider],
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[PaySplit],
    claim_procs: &[ClaimProc],
    definitions: &[Definition],
    from: NaiveDate,
    to: NaiveDate,
    insurance_enabled: bool,
    include_unearned: bool,
) -> ReportTable {
    let mut headers = vec![
        "Date",
        "Patient Name",
        "Description",
        "Prov",
        "Production",
        "Adjust",
    ];
    if insurance_enabled {
        headers.push("Write-off");
    }
    let pt_income_col = headers.len();
    headers.push("Pt Income");
    let ins_income_col = if insurance_enabled {
        headers.push("Ins Income");
        Some(headers.len() - 1)
    } else {
        None
    };
    let writeoff_col = if insurance_enabled { Some(6) } else { None };
    let mut table = ReportTable::new(&headers);
    let mut entries: Vec<ProdIncEntry> = Vec::new();
    let mut totals = vec![0.0_f64; headers.len()];

    let push = |entries: &mut Vec<ProdIncEntry>,
                totals: &mut Vec<f64>,
                date: NaiveDate,
                pat_num: Id,
                description: String,
                provider_num: Id,
                amounts: &[(usize, f64)]| {
        let mut values = vec![String::new(); headers.len()];
        values[0] = date.to_string();
        values[1] = patient_name(patients, pat_num);
        values[2] = description;
        values[3] = provider_abbr(providers, provider_num);
        for (column, amount) in amounts {
            values[*column] = format!("{amount:.2}");
            totals[*column] += *amount;
        }
        entries.push((date, values[1].clone(), values));
    };

    for proc in procedures.iter().filter(|p| {
        p.proc_status.is_completed() && p.proc_date >= from && p.proc_date <= to
    }) {
        let description = if proc.description_override.is_empty() {
            format!("Procedure {}", proc.code_num)
        } else {
            proc.description_override.clone()
        };
        push(
            &mut entries,
            &mut totals,
            proc.proc_date,
            proc.pat_num,
            description,
            proc.provider_num,
            &[(4, proc.fee)],
        );
    }
    for adj in adjustments
        .iter()
        .filter(|a| a.adj_date >= from && a.adj_date <= to)
    {
        let description = if adj.note.is_empty() {
            "Adjustment".to_string()
        } else {
            adj.note.clone()
        };
        push(
            &mut entries,
            &mut totals,
            adj.adj_date,
            adj.pat_num,
            description,
            adj.provider_num,
            &[(5, adj.adj_amount)],
        );
    }
    let hidden_unearned = hidden_unearned_def_nums(definitions);
    // OD's daily query excludes hidden unearned definitions and, when
    // "Include Unearned" is off, every UnearnedType != 0 split. Included
    // unearned splits fold into the single Pt Income detail column.
    for split in splits.iter().filter(|s| {
        s.date_pay >= from
            && s.date_pay <= to
            && (s.unearned_type == 0 || !hidden_unearned.contains(&s.unearned_type))
            && (s.unearned_type == 0 || include_unearned)
    }) {
        push(
            &mut entries,
            &mut totals,
            split.date_pay,
            split.pat_num,
            "Payment".to_string(),
            split.provider_num,
            &[(pt_income_col, split.split_amount)],
        );
    }
    if insurance_enabled {
        // ClaimProc has no date in this port, so write-offs and insurance
        // receipts are attributed to their linked procedure's date, matching
        // the upstream ProcDate write-off mode.
        for cp in claim_procs.iter().filter(|cp| cp.status == 1 || cp.status == 4) {
            let Some(proc) = procedures.iter().find(|p| p.proc_num == cp.proc_num) else {
                continue;
            };
            if proc.proc_date < from || proc.proc_date > to {
                continue;
            }
            let mut amounts = Vec::new();
            if let Some(column) = writeoff_col {
                amounts.push((column, -cp.write_off));
            }
            if let Some(column) = ins_income_col {
                amounts.push((column, cp.ins_pay_amt));
            }
            push(
                &mut entries,
                &mut totals,
                proc.proc_date,
                proc.pat_num,
                "Insurance".to_string(),
                proc.provider_num,
                &amounts,
            );
        }
    }

    entries.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    for (_, _, values) in entries {
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }

    let production = 4;
    let total_production = totals[production]
        + totals[5]
        + writeoff_col.map(|col| totals[col]).unwrap_or(0.0);
    let total_income = totals[pt_income_col]
        + ins_income_col.map(|col| totals[col]).unwrap_or(0.0);
    let mut prod_footer = vec![String::new(); headers.len()];
    prod_footer[0] = if writeoff_col.is_some() {
        "Total Production (Production + Adjustments - Write-offs)".to_string()
    } else {
        "Total Production (Production + Adjustments)".to_string()
    };
    prod_footer[production] = format!("{total_production:.2}");
    table.push(&prod_footer.iter().map(String::as_str).collect::<Vec<_>>());
    let mut income_footer = vec![String::new(); headers.len()];
    income_footer[0] = if ins_income_col.is_some() {
        "Total Income (Pt Income + Ins Income)".to_string()
    } else {
        "Total Income (Pt Income)".to_string()
    };
    income_footer[pt_income_col] = format!("{total_income:.2}");
    table.push(&income_footer.iter().map(String::as_str).collect::<Vec<_>>());
    table
}

fn prod_inc_provider(
    providers: &[Provider],
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[PaySplit],
    claim_procs: &[ClaimProc],
    definitions: &[Definition],
    from: NaiveDate,
    to: NaiveDate,
    insurance_enabled: bool,
    include_unearned: bool,
) -> ReportTable {
    let hidden_unearned = hidden_unearned_def_nums(definitions);
    #[derive(Default)]
    struct Totals {
        production: f64,
        adjust: f64,
        writeoff: f64,
        pt_income: f64,
        ins_income: f64,
    }

    let mut by_provider: std::collections::BTreeMap<Id, Totals> = std::collections::BTreeMap::new();
    for proc in procedures.iter().filter(|p| {
        p.proc_status.is_completed() && p.proc_date >= from && p.proc_date <= to
    }) {
        by_provider
            .entry(proc.provider_num)
            .or_default()
            .production += proc.fee;
    }
    for adj in adjustments
        .iter()
        .filter(|a| a.adj_date >= from && a.adj_date <= to)
    {
        by_provider.entry(adj.provider_num).or_default().adjust += adj.adj_amount;
    }
    for split in splits.iter().filter(|s| {
        s.date_pay >= from
            && s.date_pay <= to
            && (s.unearned_type == 0 || !hidden_unearned.contains(&s.unearned_type))
            && (s.unearned_type == 0 || include_unearned)
    }) {
        by_provider
            .entry(split.provider_num)
            .or_default()
            .pt_income += split.split_amount;
    }
    if insurance_enabled {
        for cp in claim_procs.iter().filter(|cp| cp.status == 1 || cp.status == 4) {
            let Some(proc) = procedures.iter().find(|p| p.proc_num == cp.proc_num) else {
                continue;
            };
            if proc.proc_date < from || proc.proc_date > to {
                continue;
            }
            let totals = by_provider.entry(proc.provider_num).or_default();
            totals.writeoff -= cp.write_off;
            totals.ins_income += cp.ins_pay_amt;
        }
    }

    let mut headers = vec!["Provider", "Production", "Adjustments"];
    if insurance_enabled {
        headers.push("Write-off");
    }
    let tot_prod_col = headers.len();
    headers.push("Tot Prod");
    let pt_income_col = headers.len();
    headers.push("Pt Income");
    let ins_income_col = if insurance_enabled {
        headers.push("Ins Income");
        Some(headers.len() - 1)
    } else {
        None
    };
    let total_income_col = headers.len();
    headers.push("Total Income");
    let writeoff_col = if insurance_enabled { Some(3) } else { None };
    let mut table = ReportTable::new(&headers);
    let mut grand = Totals::default();

    let mut rows: Vec<(String, Vec<String>)> = by_provider
        .into_iter()
        .filter_map(|(provider_num, totals)| {
            let label = if provider_num == 0 {
                "Unassigned".to_string()
            } else {
                let abbr = provider_abbr(providers, provider_num);
                if abbr.is_empty() {
                    format!("Provider #{provider_num}")
                } else {
                    abbr
                }
            };
            let has_data = totals.production != 0.0
                || totals.adjust != 0.0
                || totals.writeoff != 0.0
                || totals.pt_income != 0.0
                || totals.ins_income != 0.0;
            if !has_data {
                return None;
            }
            let tot_prod = totals.production + totals.adjust + totals.writeoff;
            let total_income = totals.pt_income + totals.ins_income;
            let mut values = vec![String::new(); headers.len()];
            values[0] = label.clone();
            values[1] = format!("{:.2}", totals.production);
            values[2] = format!("{:.2}", totals.adjust);
            if let Some(column) = writeoff_col {
                values[column] = format!("{:.2}", totals.writeoff);
            }
            values[tot_prod_col] = format!("{tot_prod:.2}");
            values[pt_income_col] = format!("{:.2}", totals.pt_income);
            if let Some(column) = ins_income_col {
                values[column] = format!("{:.2}", totals.ins_income);
            }
            values[total_income_col] = format!("{total_income:.2}");
            grand.production += totals.production;
            grand.adjust += totals.adjust;
            grand.writeoff += totals.writeoff;
            grand.pt_income += totals.pt_income;
            grand.ins_income += totals.ins_income;
            Some((label, values))
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let count = rows.len();
    for (_, values) in rows {
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }

    let mut footer = vec![String::new(); headers.len()];
    footer[0] = format!("Total ({count} provider(s))");
    footer[1] = format!("{:.2}", grand.production);
    footer[2] = format!("{:.2}", grand.adjust);
    if let Some(column) = writeoff_col {
        footer[column] = format!("{:.2}", grand.writeoff);
    }
    footer[tot_prod_col] = format!(
        "{:.2}",
        grand.production + grand.adjust + grand.writeoff
    );
    footer[pt_income_col] = format!("{:.2}", grand.pt_income);
    if let Some(column) = ins_income_col {
        footer[column] = format!("{:.2}", grand.ins_income);
    }
    footer[total_income_col] = format!("{:.2}", grand.pt_income + grand.ins_income);
    table.push(&footer.iter().map(String::as_str).collect::<Vec<_>>());
    table
}

/// Aggregated amounts for one day (Monthly) or one month (Annual).
#[derive(Clone, Copy, Default)]
struct ProdIncBucket {
    production: f64,
    sched: f64,
    adjust: f64,
    writeoff: f64,
    pt_income: f64,
    unearned_income: f64,
    ins_income: f64,
}

impl ProdIncBucket {
    fn has_data(self, include_sched: bool) -> bool {
        self.production != 0.0
            || (include_sched && self.sched != 0.0)
            || self.adjust != 0.0
            || self.writeoff != 0.0
            || self.pt_income != 0.0
            || self.unearned_income != 0.0
            || self.ins_income != 0.0
    }

    fn tot_prod(self, include_sched: bool) -> f64 {
        self.production
            + self.adjust
            + self.writeoff
            + if include_sched { self.sched } else { 0.0 }
    }

    fn total_pt_income(self) -> f64 {
        self.pt_income + self.unearned_income
    }

    fn total_income(self) -> f64 {
        self.total_pt_income() + self.ins_income
    }
}

/// OD `DefCat.PaySplitUnearnedType` definitions whose `ItemValue` is nonblank
/// are hidden from Production & Income (upstream `GetHiddenUnearnedDefNums`).
fn hidden_unearned_def_nums(definitions: &[Definition]) -> Vec<Id> {
    definitions
        .iter()
        .filter(|d| {
            d.category == DefinitionCategory::PAY_SPLIT_UNEARNED_TYPE && !d.item_value.is_empty()
        })
        .map(|d| d.def_num)
        .collect()
}

/// Upstream's Monthly `Sched` column: every scheduled appointment in range
/// prices its treatment-planned procedures (joined by `procedurelog.AptNum`),
/// minus estimated write-offs. The appointment date, not `ProcDate`, buckets
/// the amount, and appointments with no TP procedure contribute zero.
fn prod_inc_sched_by_date(
    appointments: &[Appointment],
    procedures: &[Procedure],
    claim_procs: &[ClaimProc],
    from: NaiveDate,
    to: NaiveDate,
) -> std::collections::BTreeMap<NaiveDate, f64> {
    let mut by_date: std::collections::BTreeMap<NaiveDate, f64> =
        std::collections::BTreeMap::new();
    for appt in appointments
        .iter()
        .filter(|a| a.apt_status == AppointmentStatus::Scheduled)
    {
        let date = appt.apt_datetime.date();
        if date < from || date > to {
            continue;
        }
        let mut amount = 0.0;
        for proc in procedures.iter().filter(|p| {
            p.apt_num == appt.apt_num && p.proc_status == ProcedureStatus::TreatmentPlanned
        }) {
            // The port's production convention uses the stored ProcFee, as in
            // the Daily/Annual tables; OD multiplies it by UnitQty+BaseUnits.
            let mut writeoff = 0.0;
            for cp in claim_procs
                .iter()
                .filter(|cp| cp.proc_num == proc.proc_num && matches!(cp.status, 6 | 8))
            {
                // OD's default ReportPandIschedProdSubtractsWO preference is
                // off: Estimate (6) claimprocs contribute zero write-off.
                if cp.status == 6 {
                    continue;
                }
                let estimate = if cp.write_off_est_override != -1.0 {
                    cp.write_off_est_override
                } else {
                    cp.write_off_est
                };
                if estimate != -1.0 {
                    writeoff += estimate;
                }
            }
            amount += proc.fee - writeoff;
        }
        *by_date.entry(date).or_default() += amount;
    }
    by_date
}

/// Fold the shared OD tables into per-day or per-month buckets. The caller's
/// key closures also apply the date range, so only in-range transactions are
/// included. Scheduled production is handled separately by
/// `prod_inc_sched_by_date` because upstream buckets it by appointment date.
fn prod_inc_buckets<K, FP, FA, FS>(
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[PaySplit],
    claim_procs: &[ClaimProc],
    hidden_unearned: &[Id],
    include_unearned: bool,
    insurance_enabled: bool,
    proc_key: FP,
    adj_key: FA,
    split_key: FS,
) -> std::collections::BTreeMap<K, ProdIncBucket>
where
    K: Ord,
    FP: Fn(&Procedure) -> Option<K>,
    FA: Fn(&Adjustment) -> Option<K>,
    FS: Fn(&PaySplit) -> Option<K>,
{
    let mut buckets: std::collections::BTreeMap<K, ProdIncBucket> =
        std::collections::BTreeMap::new();
    for proc in procedures {
        let Some(key) = proc_key(proc) else { continue };
        if proc.proc_status == ProcedureStatus::Complete {
            buckets.entry(key).or_default().production += proc.fee;
        }
    }
    for adj in adjustments {
        if let Some(key) = adj_key(adj) {
            buckets.entry(key).or_default().adjust += adj.adj_amount;
        }
    }
    for split in splits {
        // OD excludes PaySplitUnearnedType definitions with a nonblank
        // ItemValue, and excludes every unearned split when the "Include
        // Unearned" checkbox is off (`UnearnedType=0` in its WHERE clause).
        if split.unearned_type != 0
            && (hidden_unearned.contains(&split.unearned_type) || !include_unearned)
        {
            continue;
        }
        if let Some(key) = split_key(split) {
            let bucket = buckets.entry(key).or_default();
            if split.unearned_type != 0 {
                bucket.unearned_income += split.split_amount;
            } else {
                bucket.pt_income += split.split_amount;
            }
        }
    }
    if insurance_enabled {
        // ClaimProc has no date in this port, so insurance amounts follow
        // their procedure's date (OD's "procedure date" write-off mode).
        for cp in claim_procs.iter().filter(|cp| cp.status == 1 || cp.status == 4) {
            let Some(proc) = procedures.iter().find(|p| p.proc_num == cp.proc_num) else {
                continue;
            };
            if let Some(key) = proc_key(proc) {
                let bucket = buckets.entry(key).or_default();
                bucket.writeoff -= cp.write_off;
                bucket.ins_income += cp.ins_pay_amt;
            }
        }
    }
    buckets
}

/// `FormRpProdInc` → Monthly: one row per day with transactions, with OD's
/// `Date/Day/Production/Sched/Adj/Tot Prod/Pt Income/Unearned Pt Income/
/// Ins Income/Tot Income/Total Pt Income` columns. `Sched` comes from scheduled
/// appointments joined to treatment-planned procedures by `AptNum`, and
/// `Unearned Pt Income` from `paysplit.UnearnedType != 0`, both as upstream.
fn prod_inc_monthly(
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[PaySplit],
    claim_procs: &[ClaimProc],
    appointments: &[Appointment],
    definitions: &[Definition],
    from: NaiveDate,
    to: NaiveDate,
    insurance_enabled: bool,
    include_unearned: bool,
) -> ReportTable {
    let mut headers = vec!["Date", "Day", "Production", "Sched", "Adj"];
    if insurance_enabled {
        headers.push("Write-off");
    }
    let tot_prod_col = headers.len();
    headers.push("Tot Prod");
    let pt_income_col = headers.len();
    headers.push("Pt Income");
    let unearned_col = if include_unearned {
        headers.push("Unearned Pt Income");
        Some(headers.len() - 1)
    } else {
        None
    };
    let ins_income_col = if insurance_enabled {
        headers.push("Ins Income");
        Some(headers.len() - 1)
    } else {
        None
    };
    let total_income_col = headers.len();
    headers.push("Tot Income");
    let total_pt_col = headers.len();
    headers.push("Total Pt Income");
    let writeoff_col = if insurance_enabled { Some(5) } else { None };

    let hidden = hidden_unearned_def_nums(definitions);
    let mut buckets = prod_inc_buckets(
        procedures,
        adjustments,
        splits,
        claim_procs,
        &hidden,
        include_unearned,
        insurance_enabled,
        |p| (from..=to).contains(&p.proc_date).then_some(p.proc_date),
        |a| (from..=to).contains(&a.adj_date).then_some(a.adj_date),
        |s| (from..=to).contains(&s.date_pay).then_some(s.date_pay),
    );
    for (date, sched) in prod_inc_sched_by_date(appointments, procedures, claim_procs, from, to) {
        buckets.entry(date).or_default().sched += sched;
    }

    let mut table = ReportTable::new(&headers);
    let mut grand = ProdIncBucket::default();
    for (date, bucket) in buckets {
        if !bucket.has_data(true) {
            continue;
        }
        let mut values = vec![String::new(); headers.len()];
        values[0] = date.to_string();
        values[1] = date.format("%A").to_string();
        values[2] = format!("{:.2}", bucket.production);
        values[3] = format!("{:.2}", bucket.sched);
        values[4] = format!("{:.2}", bucket.adjust);
        if let Some(column) = writeoff_col {
            values[column] = format!("{:.2}", bucket.writeoff);
        }
        values[tot_prod_col] = format!("{:.2}", bucket.tot_prod(true));
        values[pt_income_col] = format!("{:.2}", bucket.pt_income);
        if let Some(column) = unearned_col {
            values[column] = format!("{:.2}", bucket.unearned_income);
        }
        if let Some(column) = ins_income_col {
            values[column] = format!("{:.2}", bucket.ins_income);
        }
        values[total_income_col] = format!("{:.2}", bucket.total_income());
        values[total_pt_col] = format!("{:.2}", bucket.total_pt_income());
        grand.production += bucket.production;
        grand.sched += bucket.sched;
        grand.adjust += bucket.adjust;
        grand.writeoff += bucket.writeoff;
        grand.pt_income += bucket.pt_income;
        grand.unearned_income += bucket.unearned_income;
        grand.ins_income += bucket.ins_income;
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }

    let mut prod_footer = vec![String::new(); headers.len()];
    prod_footer[0] = if insurance_enabled {
        "Total Production (Production + Scheduled + Adjustments - Write-offs):".into()
    } else {
        "Total Production (Production + Scheduled + Adjustments):".into()
    };
    prod_footer[tot_prod_col] = format!("{:.2}", grand.tot_prod(true));
    table.push(&prod_footer.iter().map(String::as_str).collect::<Vec<_>>());
    let mut pt_footer = vec![String::new(); headers.len()];
    pt_footer[0] = "Total Pt Income (Pt Income):".into();
    pt_footer[pt_income_col] = format!("{:.2}", grand.pt_income);
    table.push(&pt_footer.iter().map(String::as_str).collect::<Vec<_>>());
    if let Some(column) = unearned_col {
        let mut unearned_footer = vec![String::new(); headers.len()];
        unearned_footer[0] = "Total Unearned Pt Income (Unearned Pt Income):".into();
        unearned_footer[column] = format!("{:.2}", grand.unearned_income);
        table.push(&unearned_footer.iter().map(String::as_str).collect::<Vec<_>>());
    }
    let mut income_footer = vec![String::new(); headers.len()];
    income_footer[0] = if insurance_enabled {
        "Total Income (Total Pt Income + Ins Income):".into()
    } else {
        "Total Income (Total Pt Income):".into()
    };
    income_footer[total_income_col] = format!("{:.2}", grand.total_income());
    table.push(&income_footer.iter().map(String::as_str).collect::<Vec<_>>());
    let mut total_pt_footer = vec![String::new(); headers.len()];
    total_pt_footer[0] = "Total Pt Income (Total Pt Income):".into();
    total_pt_footer[total_pt_col] = format!("{:.2}", grand.total_pt_income());
    table.push(&total_pt_footer.iter().map(String::as_str).collect::<Vec<_>>());
    table
}

/// `FormRpProdInc` → Annual: one row per month with transactions, labelled
/// `MMM yyyy` as upstream does. Monthly's Scheduled column does not exist in
/// the annual form, so TP procedures are not counted here.
fn prod_inc_annual(
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[PaySplit],
    claim_procs: &[ClaimProc],
    definitions: &[Definition],
    from: NaiveDate,
    to: NaiveDate,
    insurance_enabled: bool,
    include_unearned: bool,
) -> ReportTable {
    let mut headers = vec!["Month", "Production", "Adjustments"];
    if insurance_enabled {
        headers.push("Write-off");
    }
    let tot_prod_col = headers.len();
    headers.push("Tot Prod");
    let pt_income_col = headers.len();
    headers.push("Pt Income");
    let unearned_col = if include_unearned {
        headers.push("Unearned Pt Income");
        Some(headers.len() - 1)
    } else {
        None
    };
    let ins_income_col = if insurance_enabled {
        headers.push("Ins Income");
        Some(headers.len() - 1)
    } else {
        None
    };
    let total_income_col = headers.len();
    headers.push("Total Income");
    let total_pt_col = headers.len();
    headers.push("Total Pt Income");
    let writeoff_col = if insurance_enabled { Some(3) } else { None };
    let in_range = |year: i32, month: u32| {
        (year, month) >= (from.year(), from.month()) && (year, month) <= (to.year(), to.month())
    };

    let hidden = hidden_unearned_def_nums(definitions);

    let buckets = prod_inc_buckets(
        procedures,
        adjustments,
        splits,
        claim_procs,
        &hidden,
        include_unearned,
        insurance_enabled,
        |p| {
            in_range(p.proc_date.year(), p.proc_date.month())
                .then_some((p.proc_date.year(), p.proc_date.month()))
        },
        |a| {
            in_range(a.adj_date.year(), a.adj_date.month())
                .then_some((a.adj_date.year(), a.adj_date.month()))
        },
        |s| {
            in_range(s.date_pay.year(), s.date_pay.month())
                .then_some((s.date_pay.year(), s.date_pay.month()))
        },
    );

    let mut table = ReportTable::new(&headers);
    let mut grand = ProdIncBucket::default();
    for ((year, month), bucket) in buckets {
        if !bucket.has_data(false) {
            continue;
        }
        let mut values = vec![String::new(); headers.len()];
        values[0] = NaiveDate::from_ymd_opt(year, month, 1)
            .map(|date| date.format("%b %Y").to_string())
            .unwrap_or_default();
        values[1] = format!("{:.2}", bucket.production);
        values[2] = format!("{:.2}", bucket.adjust);
        if let Some(column) = writeoff_col {
            values[column] = format!("{:.2}", bucket.writeoff);
        }
        values[tot_prod_col] = format!("{:.2}", bucket.tot_prod(false));
        values[pt_income_col] = format!("{:.2}", bucket.pt_income);
        if let Some(column) = unearned_col {
            values[column] = format!("{:.2}", bucket.unearned_income);
        }
        if let Some(column) = ins_income_col {
            values[column] = format!("{:.2}", bucket.ins_income);
        }
        values[total_income_col] = format!("{:.2}", bucket.total_income());
        values[total_pt_col] = format!("{:.2}", bucket.total_pt_income());
        grand.production += bucket.production;
        grand.adjust += bucket.adjust;
        grand.writeoff += bucket.writeoff;
        grand.pt_income += bucket.pt_income;
        grand.unearned_income += bucket.unearned_income;
        grand.ins_income += bucket.ins_income;
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }

    let mut prod_footer = vec![String::new(); headers.len()];
    prod_footer[0] = if insurance_enabled {
        "Total Production (Production + Adjustments - Write-offs):".into()
    } else {
        "Total Production (Production + Adjustments):".into()
    };
    prod_footer[tot_prod_col] = format!("{:.2}", grand.tot_prod(false));
    table.push(&prod_footer.iter().map(String::as_str).collect::<Vec<_>>());
    let mut pt_footer = vec![String::new(); headers.len()];
    pt_footer[0] = "Total Pt Income (Pt Income):".into();
    pt_footer[pt_income_col] = format!("{:.2}", grand.pt_income);
    table.push(&pt_footer.iter().map(String::as_str).collect::<Vec<_>>());
    if let Some(column) = unearned_col {
        let mut unearned_footer = vec![String::new(); headers.len()];
        unearned_footer[0] = "Total Unearned Pt Income (Unearned Pt Income):".into();
        unearned_footer[column] = format!("{:.2}", grand.unearned_income);
        table.push(&unearned_footer.iter().map(String::as_str).collect::<Vec<_>>());
    }
    let mut income_footer = vec![String::new(); headers.len()];
    income_footer[0] = if insurance_enabled {
        "Total Income (Total Pt Income + Ins Income):".into()
    } else {
        "Total Income (Total Pt Income):".into()
    };
    income_footer[total_income_col] = format!("{:.2}", grand.total_income());
    table.push(&income_footer.iter().map(String::as_str).collect::<Vec<_>>());
    let mut total_pt_footer = vec![String::new(); headers.len()];
    total_pt_footer[0] = "Total Pt Income (Total Pt Income):".into();
    total_pt_footer[total_pt_col] = format!("{:.2}", grand.total_pt_income());
    table.push(&total_pt_footer.iter().map(String::as_str).collect::<Vec<_>>());
    table
}

/// `FormRpProdInc` core. Daily emits one row per transaction; Monthly
/// aggregates by day (using scheduled appointments for `Sched`), Annual by
/// month and Provider by provider. Insurance income and write-off columns
/// appear only when the Insurance & claims toggle is on. Monthly and Annual
/// expose upstream's separate "Unearned Pt Income" column, and all modes
/// exclude hidden `PaySplitUnearnedType` definitions as upstream does.
#[allow(clippy::too_many_arguments)]
pub fn prod_inc_report(
    patients: &[Patient],
    providers: &[Provider],
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[PaySplit],
    claim_procs: &[ClaimProc],
    appointments: &[Appointment],
    definitions: &[Definition],
    from: NaiveDate,
    to: NaiveDate,
    insurance_enabled: bool,
    options: ProdIncReportOptions,
) -> ReportTable {
    match options.mode {
        ProdIncMode::Daily => prod_inc_daily(
            patients,
            providers,
            procedures,
            adjustments,
            splits,
            claim_procs,
            definitions,
            from,
            to,
            insurance_enabled,
            options.include_unearned,
        ),
        ProdIncMode::Monthly => prod_inc_monthly(
            procedures,
            adjustments,
            splits,
            claim_procs,
            appointments,
            definitions,
            from,
            to,
            insurance_enabled,
            options.include_unearned,
        ),
        ProdIncMode::Annual => prod_inc_annual(
            procedures,
            adjustments,
            splits,
            claim_procs,
            definitions,
            from,
            to,
            insurance_enabled,
            options.include_unearned,
        ),
        ProdIncMode::Provider => prod_inc_provider(
            providers,
            procedures,
            adjustments,
            splits,
            claim_procs,
            definitions,
            from,
            to,
            insurance_enabled,
            options.include_unearned,
        ),
    }
}

/// One rendered payment row before section headings and totals are added.
struct PaySheetRow {
    type_num: Id,
    type_name: String,
    date: NaiveDate,
    patient: String,
    provider: String,
    check_num: String,
    amount: f64,
}

/// `FormRpPaySheet` → patient-payment half of `RpPaySheet.GetPatTable`.
///
/// Upstream groups by `payment.PayNum`, payment date, check number, payment
/// type and (optionally) the split's provider and patient. Provider comes from
/// `paysplit.ProvNum`, never from the payment header; this is how OD attributes
/// one check split across several providers. Amounts are the sum of
/// `paysplit.SplitAmt` and are positive (money received).
///
/// Not ported, because this port's `Payment`/`PaySplit` do not store the
/// columns: clinics, CareCredit/PayConnect merchant fees, online-payment
/// splitting (`ProcessStatus`) and unearned prepayment filtering
/// (`UnearnedType`). The insurance-payment query needs claim-payment headers,
/// carrier and check numbers this port also lacks, so only patient payments are
/// emitted; the global Insurance & claims toggle therefore does not change
/// this report's columns.
pub fn pay_sheet_report(
    patients: &[Patient],
    providers: &[Provider],
    payments: &[Payment],
    splits: &[PaySplit],
    definitions: &[Definition],
    from: NaiveDate,
    to: NaiveDate,
    options: PaySheetReportOptions,
) -> ReportTable {
    let mut table = ReportTable::new(&["Date", "Paying Patient", "Provider", "Check#", "Amount"]);

    // Upstream Defs.GetNameWithHidden(): DefNum 0 is the synthetic
    // "Income Transfer" type, hidden defs get " (hidden)", missing -> "Undefined".
    let payment_type_name = |def_num: Id| -> String {
        if def_num == 0 {
            return "Income Transfer".into();
        }
        definitions
            .iter()
            .find(|d| d.def_num == def_num && d.category == DefinitionCategory::PAYMENT_TYPES)
            .map(|d| {
                if d.is_hidden {
                    format!("{} (hidden)", d.item_name)
                } else {
                    d.item_name.clone()
                }
            })
            .unwrap_or_else(|| "Undefined".into())
    };

    let mut splits_by_pay: HashMap<Id, Vec<&PaySplit>> = HashMap::new();
    for split in splits {
        splits_by_pay.entry(split.pay_num).or_default().push(split);
    }

    let mut detail: Vec<PaySheetRow> = Vec::new();
    for payment in payments.iter().filter(|p| p.pay_date >= from && p.pay_date <= to) {
        let type_num = payment.pay_type as Id;
        if !options.all_payment_types && type_num != options.payment_type_num {
            continue;
        }
        let type_name = payment_type_name(type_num);
        let paying_patient = patient_name(patients, payment.pat_num);
        let has_splits = splits_by_pay.contains_key(&payment.pay_num);

        // Grouping key: (patient if "Patient", provider if "show splits").
        // Each value holds the summed amount and the distinct providers used.
        let mut groups: BTreeMap<(Id, Id), (f64, Vec<Id>)> = BTreeMap::new();
        if let Some(payment_splits) = splits_by_pay.get(&payment.pay_num) {
            for split in payment_splits.iter().copied() {
                if !options.all_providers && split.provider_num != options.provider_num {
                    continue;
                }
                let patient_key = if options.group_by == PaySheetGroupBy::Patient {
                    split.pat_num
                } else {
                    0
                };
                let provider_key = if options.show_splits_by_provider {
                    split.provider_num
                } else {
                    0
                };
                let entry = groups.entry((patient_key, provider_key)).or_default();
                entry.0 += split.split_amount;
                if !entry.1.contains(&split.provider_num) {
                    entry.1.push(split.provider_num);
                }
            }
        }

        // A payment with no splits at all still produces a 0.00 row, matching
        // upstream's LEFT JOIN. A payment whose splits were filtered out by
        // provider produces nothing, matching upstream's WHERE clause.
        if groups.is_empty() && !has_splits && options.all_providers {
            groups.insert((0, 0), (0.0, Vec::new()));
        }

        for ((patient_key, _), (amount, mut provider_nums)) in groups {
            let patient = if options.group_by == PaySheetGroupBy::Patient {
                patient_name(patients, patient_key)
            } else {
                paying_patient.clone()
            };
            provider_nums.sort_unstable();
            provider_nums.dedup();
            let provider = if provider_nums.is_empty() {
                String::new()
            } else if options.show_splits_by_provider {
                let num = provider_nums[0];
                if num == 0 {
                    "No Provider".into()
                } else {
                    provider_abbr(providers, num)
                }
            } else {
                provider_nums
                    .iter()
                    .map(|&num| {
                        if num == 0 {
                            "No Provider".into()
                        } else {
                            provider_abbr(providers, num)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            detail.push(PaySheetRow {
                type_num,
                type_name: type_name.clone(),
                date: payment.pay_date,
                patient,
                provider,
                check_num: payment.check_num.clone(),
                amount,
            });
        }
    }

    // Upstream ORDER BY payment.PayType, payment.PayDate, lfname.
    detail.sort_by(|a, b| {
        a.type_num
            .cmp(&b.type_num)
            .then_with(|| a.date.cmp(&b.date))
            .then_with(|| a.patient.cmp(&b.patient))
    });

    let mut grand_total = 0.0;
    let mut current_type: Option<Id> = None;
    for row in detail {
        if current_type != Some(row.type_num) {
            current_type = Some(row.type_num);
            table.push(&[
                &format!("{} — Patient Payments", row.type_name),
                "",
                "",
                "",
                "",
            ]);
        }
        grand_total += row.amount;
        table.push(&[
            &row.date.to_string(),
            &row.patient,
            &row.provider,
            &row.check_num,
            &format!("{:.2}", row.amount),
        ]);
    }

    table.push(&[
        "Total Patient Payments:",
        "",
        "",
        "",
        &format!("{:.2}", grand_total),
    ]);
    table
}

/// `FormRpAdjSheet` / `RpAdjSheet.GetAdjTable`: one row per adjustment in the
/// inclusive `AdjDate` range. Columns and joins mirror OD's query exactly:
///
/// ```text
/// SELECT AdjDate, CONCAT(LName,', ',FName,', ',MiddleI), provider.Abbr,
///        definition.ItemName, AdjNote, AdjAmt
/// FROM adjustment
/// INNER JOIN definition ON definition.DefNum=adjustment.AdjType
/// INNER JOIN patient ON patient.PatNum=adjustment.PatNum
/// LEFT JOIN provider ON provider.ProvNum=adjustment.ProvNum
/// WHERE AdjDate BETWEEN start AND end [AND ProvNum IN(..)] [AND AdjType IN(..)]
/// ORDER BY AdjDate
/// ```
///
/// Port notes: clinics are omitted (this port has none); the type join uses
/// *all* `DefCat.AdjTypes` definitions, hidden ones included — the "All" option
/// therefore shows hidden types even though the GUI picker lists visible ones
/// only; `AdjAmt` is printed verbatim (no abs/summary/footer); and the inner
/// joins drop adjustments whose patient or type cannot be resolved while a
/// missing provider prints blank (OD's LEFT JOIN). The signed amount is shown
/// with two decimals like the other money columns in this module.
pub fn adj_sheet_report(
    patients: &[Patient],
    providers: &[Provider],
    adjustments: &[Adjustment],
    definitions: &[Definition],
    from: NaiveDate,
    to: NaiveDate,
    options: AdjustmentSheetReportOptions,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Date",
        "Patient Name",
        "Prov",
        "AdjustmentType",
        "Note",
        "Amount",
    ]);

    // INNER JOIN definition ON definition.DefNum=adjustment.AdjType. The join
    // covers every AdjTypes definition, hidden included; OD does not filter
    // hidden types out of the data, only out of the picker.
    let type_names: HashMap<Id, &str> = definitions
        .iter()
        .filter(|d| d.category == DefinitionCategory::ADJ_TYPES)
        .map(|d| (d.def_num, d.item_name.as_str()))
        .collect();

    let mut rows: Vec<(NaiveDate, [String; 6])> = Vec::new();
    for adj in adjustments
        .iter()
        .filter(|a| a.adj_date >= from && a.adj_date <= to)
    {
        if !options.all_providers && adj.provider_num != options.provider_num {
            continue;
        }
        if !options.all_types && adj.adj_type != options.type_num {
            continue;
        }
        // INNER JOIN patient + definition: drop unresolved records.
        let Some(type_name) = type_names.get(&adj.adj_type) else {
            continue;
        };
        let Some(patient) = patients.iter().find(|p| p.pat_num == adj.pat_num) else {
            continue;
        };
        // LEFT JOIN provider: an unresolvable provider prints a blank Abbr.
        let provider = provider_abbr(providers, adj.provider_num);
        rows.push((
            adj.adj_date,
            [
                adj.adj_date.to_string(),
                patient.display_name(),
                provider,
                (*type_name).to_string(),
                adj.note.clone(),
                format!("{:.2}", adj.adj_amount),
            ],
        ));
    }

    // ORDER BY adjustment.AdjDate. Stable sort keeps input order for ties.
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, values) in rows {
        let refs: Vec<&str> = values.iter().map(String::as_str).collect();
        table.push(&refs);
    }
    table
}

/// `FormRpFinanceCharge` / `RpFinanceCharge.GetFinanceChargeTable`: one row per
/// finance-charge adjustment in the inclusive `AdjDate` range. OD's query is
///
/// ```text
/// SELECT patient.PatNum, CONCAT(LName,', ',FName,' ',MiddleI),
///        patient.Preferred, adjustment.AdjAmt
/// FROM patient
/// INNER JOIN adjustment ON patient.PatNum=adjustment.PatNum
///   AND adjustment.AdjDate BETWEEN start AND end
///   AND adjustment.AdjType = financeChargeAdjType
/// [AND patient.PriProv IN(..)] [AND patient.BillingType IN(..)]
/// ORDER BY patient.LName, patient.FName, AdjAmt DESC
/// ```
///
/// Port notes: the practice-wide `FinanceChargeAdjustmentType` preference is
/// a picker here, and `0` means "all adjustment types" so the report is useful
/// before a dedicated type exists. The provider filter matches the patient's
/// primary provider (`PriProv`), not the adjustment's `ProvNum`. `Preferred` is
/// blank because this port's `Patient` has no nickname field, and the billing
/// type filter is omitted because it is not stored on the patient record. The
/// signed `AdjAmt` is printed verbatim with two decimals, as upstream's number
/// column does.
pub fn finance_charge_report(
    patients: &[Patient],
    adjustments: &[Adjustment],
    from: NaiveDate,
    to: NaiveDate,
    options: FinanceChargeReportOptions,
) -> ReportTable {
    let mut table = ReportTable::new(&["PatNum", "Patient Name", "Preferred Name", "Amount"]);

    // (LName, FName, signed amount, display row) for upstream's ORDER BY.
    let mut rows: Vec<(String, String, f64, [String; 4])> = Vec::new();
    for adj in adjustments
        .iter()
        .filter(|a| a.adj_date >= from && a.adj_date <= to)
    {
        if options.adjustment_type_num != 0 && adj.adj_type != options.adjustment_type_num {
            continue;
        }
        // INNER JOIN patient: unresolved patients are dropped.
        let Some(patient) = patients.iter().find(|p| p.pat_num == adj.pat_num) else {
            continue;
        };
        if !options.all_providers && patient.primary_provider_num != options.provider_num {
            continue;
        }
        let name = format!(
            "{}, {} {}",
            patient.l_name, patient.f_name, patient.middle_i
        )
        .trim()
        .to_string();
        rows.push((
            patient.l_name.clone(),
            patient.f_name.clone(),
            adj.adj_amount,
            [
                adj.pat_num.to_string(),
                name,
                String::new(), // patient.Preferred is not stored by this port
                format!("{:.2}", adj.adj_amount),
            ],
        ));
    }

    // ORDER BY patient.LName, patient.FName, AdjAmt DESC.
    rows.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| b.2.total_cmp(&a.2))
    });
    for (_, _, _, values) in rows {
        let refs: Vec<&str> = values.iter().map(String::as_str).collect();
        table.push(&refs);
    }
    table
}

/// `FormRpBrokenAppointments` / `RpBrokenAppointments`: three reports sharing a
/// date range and provider filter, selected by the form's radio buttons.
///
/// * `ByAppointmentStatus` (`ByApptStatusQuery`): appointments with
///   `AptStatus = Broken(5)` and `DATE(AptDateTime)` in range. Columns are
///   `AptDate / Patient / Doctor / Hygienist / IsHyg`; doctor and hygienist are
///   `LEFT JOIN`ed providers.
/// * `ByAdjustments` (`ByAdjustmentsQuery`): adjustments in range of a
///   broken-appointment type. Columns are `Date / Provider / Patient / Amount /
///   Note`; provider is `INNER JOIN`ed.
/// * `ByProcedures` (`ByProceduresQuery`): completed procedures whose CDT code
///   is D9986 (missed), D9987 (cancelled) or both. Columns are
///   `Date / Provider / [Code] / Patient / Fee`; provider is `INNER JOIN`ed and
///   the `Code` column appears only in the `Both` option, as upstream does.
///
/// Each mode ends with upstream's group-summary rows, which this port renders
/// as a final row with the label in column 0 and the total in the summarized
/// column (`Fee`, `Amount` or `IsHyg` respectively).
///
/// Port notes: clinics are omitted; this port has no `ProvHyg`, `IsHygieneApt`
/// or `SecDateTEntry`, so the Hygienist and IsHyg columns are blank/False. The
/// upstream adjustment-type picker is a single type here (`0` = all types),
/// and the provider filter uses the mode's own provider field with no
/// hygienist alternative.
pub fn broken_appointments_report(
    patients: &[Patient],
    providers: &[Provider],
    appointments: &[Appointment],
    adjustments: &[Adjustment],
    procedures: &[Procedure],
    procedure_codes: &[ProcedureCode],
    from: NaiveDate,
    to: NaiveDate,
    options: BrokenAppointmentReportOptions,
) -> ReportTable {
    let provider_selected = |num: Id| !options.all_providers && num == options.provider_num;
    // Upstream INNER JOINs provider in the adjustment/procedure modes: a row
    // whose provider cannot be resolved is dropped. The status mode LEFT JOINs.
    let inner_provider = |num: Id| providers.iter().any(|p| p.provider_num == num);

    match options.mode {
        BrokenAppointmentMode::ByAppointmentStatus => {
            let mut table = ReportTable::new(&["AptDate", "Patient", "Doctor", "Hygienist", "IsHyg"]);
            let mut entries: Vec<(NaiveDate, String, [String; 5])> = Vec::new();
            for apt in appointments.iter().filter(|a| {
                a.apt_status == AppointmentStatus::Broken
                    && a.apt_datetime.date() >= from
                    && a.apt_datetime.date() <= to
                    && (options.all_providers || provider_selected(a.provider_num))
            }) {
                let Some(patient) = patients.iter().find(|p| p.pat_num == apt.pat_num) else {
                    continue; // INNER JOIN patient
                };
                let name = format!("{}, {}", patient.l_name, patient.f_name);
                entries.push((
                    apt.apt_datetime.date(),
                    name.clone(),
                    [
                        apt.apt_datetime.format("%m/%d/%Y %H:%M:%S").to_string(),
                        name,
                        provider_abbr(providers, apt.provider_num), // doctor, LEFT JOIN
                        String::new(),                             // hygienist: ProvHyg not stored
                        "False".to_string(),                       // IsHygieneApt not stored
                    ],
                ));
            }
            // ORDER BY AptDateTime, patient.LName, patient.FName.
            entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
            let count = entries.len();
            for (_, _, values) in &entries {
                table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
            }
            table.push(&[
                "Total Broken Appointments:", "", "", "", &count.to_string(),
            ]);
            table
        }
        BrokenAppointmentMode::ByAdjustments => {
            let mut table = ReportTable::new(&["Date", "Provider", "Patient", "Amount", "Note"]);
            let mut entries: Vec<(NaiveDate, String, [String; 5])> = Vec::new();
            let mut total = 0.0;
            for adj in adjustments.iter().filter(|a| {
                a.adj_date >= from
                    && a.adj_date <= to
                    && (options.adjustment_type_num == 0
                        || a.adj_type == options.adjustment_type_num)
                    && (options.all_providers || provider_selected(a.provider_num))
                    && inner_provider(a.provider_num)
            }) {
                let Some(patient) = patients.iter().find(|p| p.pat_num == adj.pat_num) else {
                    continue; // INNER JOIN patient
                };
                let name = format!("{}, {}", patient.l_name, patient.f_name);
                total += adj.adj_amount;
                entries.push((
                    adj.adj_date,
                    name.clone(),
                    [
                        adj.adj_date.to_string(),
                        provider_abbr(providers, adj.provider_num),
                        name,
                        format!("{:.2}", adj.adj_amount),
                        adj.note.clone(),
                    ],
                ));
            }
            // ORDER BY AdjDate, patient.LName, patient.FName.
            entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
            let count = entries.len();
            for (_, _, values) in &entries {
                table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
            }
            table.push(&[
                "Total Broken Appointment Adjustment Amount:",
                "",
                "",
                &format!("{total:.2}"),
                "",
            ]);
            table.push(&["Total Broken Appointments:", "", "", &count.to_string(), ""]);
            table
        }
        BrokenAppointmentMode::ByProcedures => {
            let wanted_codes: &[&str] = match options.procedure_option {
                BrokenProcedureOption::Missed => &["D9986"],
                BrokenProcedureOption::Cancelled => &["D9987"],
                BrokenProcedureOption::Both => &["D9986", "D9987"],
            };
            let matching_code_nums: std::collections::HashSet<Id> = procedure_codes
                .iter()
                .filter(|c| wanted_codes.contains(&c.code.as_str()))
                .map(|c| c.code_num)
                .collect();
            let mut headers = vec!["Date", "Provider"];
            if options.procedure_option == BrokenProcedureOption::Both {
                headers.push("Code");
            }
            headers.extend(["Patient", "Fee"]);
            let mut table = ReportTable::new(&headers);
            let mut entries: Vec<(NaiveDate, String, Vec<String>)> = Vec::new();
            let mut total = 0.0;
            for proc in procedures.iter().filter(|p| {
                p.proc_date >= from
                    && p.proc_date <= to
                    && p.proc_status == ProcedureStatus::Complete
                    && matching_code_nums.contains(&p.code_num)
                    && (options.all_providers || provider_selected(p.provider_num))
                    && inner_provider(p.provider_num)
            }) {
                let Some(patient) = patients.iter().find(|p| p.pat_num == proc.pat_num) else {
                    continue; // INNER JOIN patient
                };
                let code = procedure_codes
                    .iter()
                    .find(|c| c.code_num == proc.code_num)
                    .map(|c| c.code.clone())
                    .unwrap_or_default();
                let name = format!("{}, {}", patient.l_name, patient.f_name);
                total += proc.fee;
                let mut values = vec![
                    proc.proc_date.to_string(),
                    provider_abbr(providers, proc.provider_num),
                ];
                if options.procedure_option == BrokenProcedureOption::Both {
                    values.push(code);
                }
                values.extend([name.clone(), format!("{:.2}", proc.fee)]);
                entries.push((proc.proc_date, name, values));
            }
            // ORDER BY ProcDate, patient.LName, patient.FName.
            entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
            let count = entries.len();
            for (_, _, values) in &entries {
                table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
            }
            // Both summaries display in the Fee column (upstream's "Fee").
            let fee_index = headers.len() - 1;
            let mut fees_row = vec![String::new(); headers.len()];
            fees_row[0] = "Total Broken Appointment Fees:".into();
            fees_row[fee_index] = format!("{total:.2}");
            table.push(&fees_row.iter().map(String::as_str).collect::<Vec<_>>());
            let mut count_row = vec![String::new(); headers.len()];
            count_row[0] = "Total Broken Appointments:".into();
            count_row[fee_index] = count.to_string();
            table.push(&count_row.iter().map(String::as_str).collect::<Vec<_>>());
            table
        }
    }
}

/// Resolve a `FormRpConfirm`/`FormRpRecall` patient field name to its value.
/// Upstream projects `patient.<Field>` verbatim; this port leaves fields it does
/// not store (Preferred, Address2, WkPhone, billing type, notes, insurance
/// carriers, student/school data, balances, guarantor, secondary provider,
/// image folder) blank rather than inventing data. `Salutation` uses the
/// patient's title, the closest stored equivalent.
fn patient_report_field(field: &str, patient: &Patient) -> String {
    match field {
        "LName" => patient.l_name.clone(),
        "FName" => patient.f_name.clone(),
        "MiddleI" => patient.middle_i.clone(),
        "Salutation" => patient.title.clone(),
        "Address" => patient.address.clone(),
        "City" => patient.city.clone(),
        "State" => patient.state.clone(),
        "Zip" => patient.zip.clone(),
        "HmPhone" => patient.home_phone.clone(),
        "WirelessPhone" => patient.wireless_phone.clone(),
        "Birthdate" => patient
            .birth_date
            .map(|d| d.to_string())
            .unwrap_or_default(),
        "PatNum" => patient.pat_num.to_string(),
        // Upstream selects raw `patient.Gender` / `patient.PriProv`, i.e. the
        // numeric OD values, not display labels.
        "Gender" => patient.gender.as_od().to_string(),
        "PriProv" => patient.primary_provider_num.to_string(),
        "Email" => patient.email.clone(),
        _ => String::new(),
    }
}

/// `FormRpConfirm`: a field-selection projection over appointments. Upstream
/// takes its row scope from the confirmation list's selected `AptNum`s; this
/// port uses appointments inside the report date range. Columns are exactly the
/// selected field names, in the form's two-list order, and unmodelled fields
/// are blank. The standard fields are selected by default.
pub fn confirmation_report(
    patients: &[Patient],
    appointments: &[Appointment],
    options: &ConfirmationReportOptions,
) -> ReportTable {
    let fields: Vec<&str> = options.fields.iter().map(String::as_str).collect();
    let mut table = ReportTable::new(&fields);
    for apt in appointments {
        let Some(patient) = patients.iter().find(|p| p.pat_num == apt.pat_num) else {
            continue; // INNER JOIN patient
        };
        let values: Vec<String> = fields
            .iter()
            .map(|field| {
                if *field == "AptDateTime" {
                    apt.apt_datetime.format("%m/%d/%Y %H:%M:%S").to_string()
                } else {
                    patient_report_field(field, patient)
                }
            })
            .collect();
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }
    table
}

/// `FormRpRecall`: the field-selection projection over recall rows (distinct
/// from the local Recall & ASAP due list). Upstream scopes rows to the recall
/// list's selected `RecallNum`s; this port uses recalls due in the date range.
/// `RecallStatus` prints the definition number, `DateDue` the editable due date
/// and `RecallInterval` the upstream `ROUND(RecallInterval/65536)` month value.
pub fn recall_fields_report(
    patients: &[Patient],
    recalls: &[Recall],
    from: NaiveDate,
    to: NaiveDate,
    options: &RecallFieldsReportOptions,
) -> ReportTable {
    // Upstream's SELECT emits the alias MonthInterval for the RecallInterval
    // field; keep that header so exported columns match.
    let headers: Vec<String> = options
        .fields
        .iter()
        .map(|f| {
            if f == "RecallInterval" {
                "MonthInterval".to_string()
            } else {
                f.clone()
            }
        })
        .collect();
    let fields: Vec<&str> = options.fields.iter().map(String::as_str).collect();
    let mut table = ReportTable::new(&headers.iter().map(String::as_str).collect::<Vec<_>>());
    for recall in recalls.iter().filter(|r| {
        r.date_due
            .map(|d| d >= from && d <= to)
            .unwrap_or(false)
    }) {
        let Some(patient) = patients.iter().find(|p| p.pat_num == recall.pat_num) else {
            continue; // INNER JOIN patient
        };
        let values: Vec<String> = fields
            .iter()
            .map(|field| match *field {
                "DateDue" => recall.date_due.map(|d| d.to_string()).unwrap_or_default(),
                "RecallStatus" => recall.recall_status.to_string(),
                "RecallInterval" => ((recall.recall_interval as f64 / 65536.0).round() as i64).to_string(),
                _ => patient_report_field(field, patient),
            })
            .collect();
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }
    table
}

/// `FormRpProcSheet` / `RpProcSheet.GetIndividualTable` and `GetGroupedTable`.
///
/// Both layouts cover completed procedures in the inclusive date range, joined
/// INNER to patient and procedure code and LEFT to provider. Upstream also
/// LEFT JOINs `claimproc` (capitation write-offs) and multiplies the fee by
/// `UnitQty + BaseUnits`; this port has neither claims nor unit quantities, so
/// `Fee` is the stored `procedurelog.ProcFee` verbatim. Clinics are omitted.
/// The `Area` column is blank because tooth surfaces (`procedurelog.Surf`) are
/// not stored; `Tooth` is always shown because this is a dental practice port.
pub fn procedure_sheet_report(
    patients: &[Patient],
    providers: &[Provider],
    procedures: &[Procedure],
    procedure_codes: &[ProcedureCode],
    definitions: &[Definition],
    from: NaiveDate,
    to: NaiveDate,
    options: &ProcedureSheetReportOptions,
) -> ReportTable {
    // Upstream's LIKE '%CODE%' is case-insensitive via UPPER on both sides.
    let code_filter = options.code_filter.trim().to_ascii_uppercase();
    let mut rows: Vec<(&Procedure, &ProcedureCode, &Patient)> = procedures
        .iter()
        .filter(|p| p.proc_status == ProcedureStatus::Complete)
        .filter(|p| p.proc_date >= from && p.proc_date <= to)
        .filter(|p| options.all_providers || p.provider_num == options.provider_num)
        .filter_map(|p| {
            let code = procedure_codes.iter().find(|c| c.code_num == p.code_num)?;
            if !code_filter.is_empty() && !code.code.to_ascii_uppercase().contains(&code_filter) {
                return None;
            }
            let patient = patients.iter().find(|pat| pat.pat_num == p.pat_num)?;
            Some((p, code, patient))
        })
        .collect();

    match options.mode {
        ProcedureSheetMode::Individual => {
            let mut table = ReportTable::new(&[
                "Date", "Patient Name", "Code", "Tooth", "Area", "Description", "Prov", "Fee",
            ]);
            // ORDER BY ProcDate, LName + ', ' + FName + ' ' + MiddleI, ProcCode, ToothNum.
            rows.sort_by(|a, b| {
                let ka = (
                    a.0.proc_date,
                    &a.2.l_name,
                    &a.2.f_name,
                    &a.2.middle_i,
                    &a.1.code,
                    &a.0.tooth_num,
                );
                let kb = (
                    b.0.proc_date,
                    &b.2.l_name,
                    &b.2.f_name,
                    &b.2.middle_i,
                    &b.1.code,
                    &b.0.tooth_num,
                );
                ka.cmp(&kb)
            });
            for (proc, code, patient) in rows {
                let patient_name = format!(
                    "{}, {} {}",
                    patient.l_name, patient.f_name, patient.middle_i
                );
                let values = [
                    proc.proc_date.to_string(),
                    patient_name,
                    code.code.clone(),
                    proc.tooth_num.clone(),
                    String::new(), // Area: procedurelog.Surf is not stored.
                    code.code_name.clone(),
                    provider_abbr(providers, proc.provider_num),
                    format!("{:.2}", proc.fee),
                ];
                table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
            }
            table
        }
        ProcedureSheetMode::GroupedByCode => {
            let mut table = ReportTable::new(&[
                "Category", "Code", "Description", "Quantity", "Average Fee", "Total Fees",
            ]);
            struct Group {
                category_order: i32,
                category_name: String,
                code: String,
                description: String,
                quantity: usize,
                total: f64,
            }
            let mut groups: BTreeMap<Id, Group> = BTreeMap::new();
            for (proc, code, _) in rows {
                // Upstream's grouped query INNER JOINs definition on
                // procedurecode.ProcCat; uncategorised codes are dropped.
                let Some(category) = definitions.iter().find(|d| {
                    d.category == DefinitionCategory::PROC_CODE_CATS
                        && d.def_num == code.category_num
                }) else {
                    continue;
                };
                let entry = groups.entry(code.code_num).or_insert_with(|| Group {
                    category_order: category.item_order,
                    category_name: category.item_name.clone(),
                    code: code.code.clone(),
                    description: code.code_name.clone(),
                    quantity: 0,
                    total: 0.0,
                });
                entry.quantity += 1;
                entry.total += proc.fee;
            }
            let mut groups: Vec<Group> = groups.into_values().collect();
            // ORDER BY procs.ItemOrder, procs.ProcCode.
            groups.sort_by(|a, b| {
                (a.category_order, &a.code).cmp(&(b.category_order, &b.code))
            });
            for group in groups {
                let average = (group.total / group.quantity as f64 * 100.0).round() / 100.0;
                let values = [
                    group.category_name,
                    group.code,
                    group.description,
                    group.quantity.to_string(),
                    format!("{average:.2}"),
                    format!("{:.2}", group.total),
                ];
                table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
            }
            table
        }
    }
}

/// `FormRpProcCodes` / `RpProcCodes.GetData`: every CDT code on a fee schedule.
///
/// Upstream reads fees from `fee` filtered by `FeeSched`, `ClinicNum` and
/// `ProvNum`, matches them to codes by `ProcCode`, and treats `Amount == -1` as
/// "blank" (suppressed unless blank fees are requested). It orders the category
/// layout by `procedurecode.ProcCat` (the definition FK) then `ProcCode`, and the
/// plain layout by `ProcCode`. The Category column's `SuppressIfDuplicate`
/// display flag is a print-layout concern, so this tabular port keeps the name
/// on every row; the fee uses a plain two-decimal format rather than .NET's
/// culture-specific `"n"` thousands separator. Clinics are always the default
/// clinic (0) because this port has none.
pub fn procedure_codes_report(
    procedure_codes: &[ProcedureCode],
    fees: &[Fee],
    definitions: &[Definition],
    options: &ProcedureCodesReportOptions,
) -> ReportTable {
    let mut table = if options.categories {
        ReportTable::new(&["Category", "Code", "Desc", "Abbr", "Fee"])
    } else {
        ReportTable::new(&["Code", "Desc", "Abbr", "Fee"])
    };

    // OD matches the fee query rows to codes by ProcCode text. Matching by the
    // CodeNum FK here is equivalent for a well-formed OD database.
    let fee_by_code: HashMap<Id, f64> = fees.iter().map(|f| (f.code_num, f.amount)).collect();

    let mut codes: Vec<&ProcedureCode> = procedure_codes.iter().collect();
    if options.categories {
        codes.sort_by(|a, b| a.category_num.cmp(&b.category_num).then(a.code.cmp(&b.code)));
    } else {
        codes.sort_by(|a, b| a.code.cmp(&b.code));
    }

    let category_names: HashMap<Id, &str> = definitions
        .iter()
        .filter(|d| d.category == DefinitionCategory::PROC_CODE_CATS)
        .map(|d| (d.def_num, d.item_name.as_str()))
        .collect();

    for code in codes {
        // Amount == -1 is OD's sentinel for a deliberately blank fee. It is
        // treated exactly like a missing fee row: keep the row only when blank
        // fees were requested.
        let fee = fee_by_code.get(&code.code_num).copied();
        let has_fee = fee.is_some_and(|amount| amount != -1.0);
        if !has_fee && !options.include_blanks {
            continue;
        }
        let fee_text = match fee {
            Some(amount) if amount != -1.0 => format!("{amount:.2}"),
            _ => String::new(),
        };
        let values: Vec<String> = if options.categories {
            vec![
                category_names
                    .get(&code.category_num)
                    .map(|name| (*name).to_string())
                    .unwrap_or_default(),
                code.code.clone(),
                code.code_name.clone(),
                code.abbreviation.clone(),
                fee_text,
            ]
        } else {
            vec![
                code.code.clone(),
                code.code_name.clone(),
                code.abbreviation.clone(),
                fee_text,
            ]
        };
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }
    table
}

/// OD's Prescriptions report (`FormRpPrescriptions` / `RpPrescriptions`).
///
/// Upstream runs `SELECT LName + ', ' + FName + ' ' + MiddleI, RxDate, Drug,
/// Sig, Disp, provider.abbr FROM patient, rxpat, provider` and filters by either
/// a patient-surname prefix or a drug-name prefix. The INNER JOINs mean a
/// prescription with no matching patient or provider is omitted, which this
/// builder mirrors. The report has no date filter; the shared report range is
/// deliberately ignored.
pub fn prescriptions_report(
    patients: &[Patient],
    providers: &[Provider],
    prescriptions: &[Prescription],
    options: &PrescriptionReportOptions,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Patient Name",
        "Date",
        "Drug Name",
        "Directions",
        "Dispense",
        "Prov Name",
    ]);
    let prefix = options.input_text.trim().to_lowercase();

    // Keep only rows whose patient and provider both exist, matching the two
    // INNER JOINs of the upstream query.
    let mut rows: Vec<(&Prescription, &Patient, &Provider)> = prescriptions
        .iter()
        .filter_map(|rx| {
            let patient = patients.iter().find(|p| p.pat_num == rx.pat_num)?;
            let provider = providers.iter().find(|p| p.provider_num == rx.prov_num)?;
            if options.search_patient {
                if !patient.l_name.to_lowercase().starts_with(&prefix) {
                    return None;
                }
            } else if !rx.drug.to_lowercase().starts_with(&prefix) {
                return None;
            }
            Some((rx, patient, provider))
        })
        .collect();

    if options.search_patient {
        // ORDER BY patient.LName, patient.FName, rxpat.RxDate
        rows.sort_by(|a, b| {
            a.1.l_name
                .cmp(&b.1.l_name)
                .then(a.1.f_name.cmp(&b.1.f_name))
                .then(a.0.rx_date.cmp(&b.0.rx_date))
        });
    } else {
        // ORDER BY patient.LName, rxpat.Drug, rxpat.RxDate
        rows.sort_by(|a, b| {
            a.1.l_name
                .cmp(&b.1.l_name)
                .then(a.0.drug.cmp(&b.0.drug))
                .then(a.0.rx_date.cmp(&b.0.rx_date))
        });
    }

    for (rx, patient, provider) in rows {
        let patient_name = format!("{}, {} {}", patient.l_name, patient.f_name, patient.middle_i);
        let date_text = if rx.rx_date.year() <= 1 {
            String::new()
        } else {
            rx.rx_date.to_string()
        };
        let values = [
            patient_name,
            date_text,
            rx.drug.clone(),
            rx.sig.clone(),
            rx.disp.clone(),
            provider.abbreviation.clone(),
        ];
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }
    table
}

/// `FormRpReferralAnalysis` / `RpReferralAnalysis.GetReferralTable`: referring
/// sources ranked by the number of distinct patients they sent and the
/// production those patients generated.
///
/// Upstream's SQL has three layers, all reproduced here over in-memory data:
///   1. distinct (ReferralNum, PatNum) pairs from `refattach` where RefType=1;
///   2. per-patient production from completed, non-D9986/D9987 procedures,
///      multiplied by `UnitQty + BaseUnits`;
///   3. the outer GROUP BY referral with COUNT(DISTINCT patient) and SUM(fee).
/// The "new patients only" variant drops the date filter from the WHERE and
/// instead applies `HAVING MIN(ProcDate) BETWEEN from AND to`, so a patient with
/// older work is excluded entirely even though only in-range fees are summed.
/// Specialty is resolved from `DefCat.ProviderSpecialties` when addresses show.
pub fn referral_analysis_report(
    procedures: &[Procedure],
    procedure_codes: &[ProcedureCode],
    referrals: &[Referral],
    refattaches: &[RefAttach],
    definitions: &[Definition],
    from: NaiveDate,
    to: NaiveDate,
    options: &ReferralAnalysisReportOptions,
) -> ReportTable {
    let mut headers = vec!["Last Name", "First Name", "Count", "Production"];
    if options.include_address {
        headers.extend_from_slice(&[
            "Title", "Address", "Add2", "City", "State", "Zip", "Specialty",
        ]);
    }
    let mut table = ReportTable::new(&headers);

    // INNER JOIN procedurecode ... AND ProcCode NOT IN ('D9986','D9987').
    // A procedure with no code row is dropped by that same INNER JOIN.
    let mut coded = BTreeSet::new();
    for code in procedure_codes {
        if code.code != "D9986" && code.code != "D9987" {
            coded.insert(code.code_num);
        }
    }
    let provider_ok = |proc: &Procedure| {
        options.all_providers || options.provider_nums.contains(&proc.provider_num)
    };

    // The inner SELECT ... GROUP BY PatNum. `first_proc_date` ignores the date
    // range because upstream's new-patient HAVING reads MIN over every
    // qualifying completed procedure, not only those inside the range.
    let mut production: BTreeMap<Id, f64> = BTreeMap::new();
    let mut first_proc_date: BTreeMap<Id, NaiveDate> = BTreeMap::new();
    for proc in procedures {
        if proc.proc_status != ProcedureStatus::Complete
            || !provider_ok(proc)
            || !coded.contains(&proc.code_num)
        {
            continue;
        }
        first_proc_date
            .entry(proc.pat_num)
            .and_modify(|first| {
                if proc.proc_date < *first {
                    *first = proc.proc_date;
                }
            })
            .or_insert(proc.proc_date);
        // In both variants only in-range fees are summed: the normal query
        // filters the WHERE, the new-patient query wraps the sum in CASE WHEN.
        if proc.proc_date >= from && proc.proc_date <= to {
            *production.entry(proc.pat_num).or_insert(0.0) +=
                proc.fee * (proc.unit_qty + proc.base_units);
        }
    }

    // ON procs.PatNum = attach.PatNum plus the new-patient HAVING clause.
    let mut attached: BTreeMap<Id, BTreeSet<Id>> = BTreeMap::new();
    for attach in refattaches {
        if attach.ref_type != 1 || !production.contains_key(&attach.pat_num) {
            continue; // 1 = ReferralType.RefFrom
        }
        if options.only_new_patients {
            let Some(first) = first_proc_date.get(&attach.pat_num) else {
                continue;
            };
            if *first < from || *first > to {
                continue;
            }
        }
        attached
            .entry(attach.referral_num)
            .or_default()
            .insert(attach.pat_num);
    }

    // The two INNER JOINs remove referrals with no qualifying patients.
    let mut rows: Vec<(&Referral, usize, f64)> = referrals
        .iter()
        .filter_map(|referral| {
            let pats = attached.get(&referral.referral_num)?;
            let amount = pats.iter().map(|pat| production[pat]).sum();
            Some((referral, pats.len(), amount))
        })
        .collect();
    // ORDER BY HowMany DESC; the name is a deterministic tiebreaker because
    // upstream leaves equal counts to the database.
    rows.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(a.0.l_name.cmp(&b.0.l_name))
            .then(a.0.f_name.cmp(&b.0.f_name))
    });

    for (referral, count, amount) in rows {
        let mut values = vec![
            referral.l_name.clone(),
            referral.f_name.clone(),
            count.to_string(),
            format!("{amount:.2}"),
        ];
        if options.include_address {
            let specialty = definitions
                .iter()
                .find(|d| {
                    d.category == DefinitionCategory::PROVIDER_SPECIALTIES
                        && d.def_num == referral.specialty_num
                })
                .map(|d| d.item_name.clone())
                .unwrap_or_default();
            values.extend_from_slice(&[
                referral.title.clone(),
                referral.address.clone(),
                referral.address2.clone(),
                referral.city.clone(),
                referral.state.clone(),
                referral.zip.clone(),
                specialty,
            ]);
        }
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }
    table
}

/// One raw `referral` column, rendered exactly as upstream's `SELECT` does:
/// booleans as `0`/`1`, `Specialty` as the numeric definition FK (not its name),
/// and the OD column names mapped to the form's labels (`ST` → state,
/// `EMail` → Email). Unknown fields render blank.
fn referral_field_value(referral: &Referral, field: &str) -> String {
    match field {
        "Address" => referral.address.clone(),
        "Address2" => referral.address2.clone(),
        "City" => referral.city.clone(),
        "Email" => referral.email.clone(),
        "FName" => referral.f_name.clone(),
        "IsHidden" => referral.is_hidden.to_string(),
        "LName" => referral.l_name.clone(),
        "MName" => referral.m_name.clone(),
        "Note" => referral.note.clone(),
        "NotPerson" => referral.not_person.to_string(),
        "Phone2" => referral.phone2.clone(),
        "ReferralNum" => referral.referral_num.to_string(),
        "Specialty" => referral.specialty_num.to_string(),
        "SSN" => referral.ssn.clone(),
        "ST" => referral.state.clone(),
        "Telephone" => referral.telephone.clone(),
        "Title" => referral.title.clone(),
        "UsingTIN" => referral.using_tin.to_string(),
        "Zip" => referral.zip.clone(),
        _ => String::new(),
    }
}

/// Case-insensitive SQL `LIKE` matcher with `%` (any run) and `_` (one char).
/// Upstream wraps the text box input as `LIKE '%value%'`, so wildcards typed by
/// the user stay active, exactly as they are in the generated SQL.
fn sql_like(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let text: Vec<char> = text.to_ascii_lowercase().chars().collect();
    let mut prev = vec![false; text.len() + 1];
    prev[0] = true;
    for &ch in &pattern {
        let mut cur = vec![false; text.len() + 1];
        if ch == '%' {
            cur[0] = prev[0];
            for j in 1..=text.len() {
                cur[j] = prev[j] || cur[j - 1];
            }
        } else {
            for j in 1..=text.len() {
                cur[j] = prev[j - 1] && (ch == '_' || ch == text[j - 1]);
            }
        }
        prev = cur;
    }
    prev[text.len()]
}

/// OD compares its numeric columns as numbers and its varchar columns as
/// case-insensitive strings, so the numeric-vs-string decision is made from the
/// field name rather than from the value's shape (a zip code such as "1000"
/// must stay a string). Both upstream query-builder reports share this rule;
/// each passes the set of numeric fields it can select.
fn report_filter_matches(actual: &str, filter: &ReportFilter, numeric_fields: &[&str]) -> bool {
    if filter.condition == ReportFilterCondition::Like {
        return sql_like(&format!("%{}%", filter.value), actual);
    }
    if numeric_fields.contains(&filter.field.as_str()) {
        if let (Ok(a), Ok(expected)) = (actual.parse::<f64>(), filter.value.parse::<f64>()) {
            return match filter.condition {
                ReportFilterCondition::Eq => a == expected,
                ReportFilterCondition::Gt => a > expected,
                ReportFilterCondition::Lt => a < expected,
                ReportFilterCondition::Gte => a >= expected,
                ReportFilterCondition::Lte => a <= expected,
                ReportFilterCondition::Ne => a != expected,
                ReportFilterCondition::Like => unreachable!(),
            };
        }
    }
    let a = actual.to_ascii_lowercase();
    let expected = filter.value.to_ascii_lowercase();
    match filter.condition {
        ReportFilterCondition::Eq => a == expected,
        ReportFilterCondition::Gt => a > expected,
        ReportFilterCondition::Lt => a < expected,
        ReportFilterCondition::Gte => a >= expected,
        ReportFilterCondition::Lte => a <= expected,
        ReportFilterCondition::Ne => a != expected,
        ReportFilterCondition::Like => unreachable!(),
    }
}

/// `FormRpReferrals`: an upstream field-selection report over OD's `referral`
/// table. Upstream builds a literal `SELECT <fields> FROM referral WHERE ...`
/// and shows the raw values in a query grid. This port keeps the same 19-field
/// vocabulary, the seven conditions, AND chaining of prerequisites, and the
/// boolean/numeric/string comparison rules of MySQL. Upstream has no ORDER BY;
/// the MyISAM table scan is approximated with `ReferralNum` order.
pub fn referrals_report(referrals: &[Referral], options: &ReferralsReportOptions) -> ReportTable {
    let headers: Vec<&str> = options.fields.iter().map(String::as_str).collect();
    let mut table = ReportTable::new(&headers);
    let mut rows: Vec<&Referral> = referrals.iter().collect();
    rows.sort_by_key(|referral| referral.referral_num);
    for referral in rows {
        if !options
            .filters
            .iter()
            .all(|filter| {
                report_filter_matches(
                    &referral_field_value(referral, &filter.field),
                    filter,
                    &["ReferralNum", "Specialty", "IsHidden", "NotPerson", "UsingTIN"],
                )
            })
        {
            continue;
        }
        let values: Vec<String> = options
            .fields
            .iter()
            .map(|field| referral_field_value(referral, field))
            .collect();
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }
    table
}

/// One `patient` column rendered exactly as upstream's generated SELECT would:
/// the enum and definition columns as their raw numbers, dates as ISO strings
/// and missing derived values as blank. First/Last Complete Proc use upstream's
/// correlated `MIN`/`MAX(ProcDate)` subqueries with `ProcStatus = Complete`.
fn patient_field_value(
    patient: &Patient,
    field: &str,
    first_complete: Option<NaiveDate>,
    last_complete: Option<NaiveDate>,
    recall_status: Option<Id>,
) -> String {
    match field {
        "PatNum" => patient.pat_num.to_string(),
        "LName" => patient.l_name.clone(),
        "FName" => patient.f_name.clone(),
        "MiddleI" => patient.middle_i.clone(),
        "Title" => patient.title.clone(),
        "Birthdate" => patient
            .birth_date
            .map(|date| date.format("%Y-%m-%d").to_string())
            .unwrap_or_default(),
        "Gender" => match patient.gender {
            Gender::Male => "0",
            Gender::Female => "1",
            Gender::Unknown => "2",
            Gender::Other => "3",
        }
        .to_string(),
        "HmPhone" => patient.home_phone.clone(),
        "WirelessPhone" => patient.wireless_phone.clone(),
        "Email" => patient.email.clone(),
        "Address" => patient.address.clone(),
        "City" => patient.city.clone(),
        "State" => patient.state.clone(),
        "Zip" => patient.zip.clone(),
        "PriProv" => patient.primary_provider_num.to_string(),
        "ClinicNum" => patient.clinic_num.to_string(),
        "DateTStamp" => patient
            .date_created
            .map(|stamp| stamp.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_default(),
        "First Complete Proc" => first_complete
            .map(|date| date.format("%Y-%m-%d").to_string())
            .unwrap_or_default(),
        "Last Complete Proc" => last_complete
            .map(|date| date.format("%Y-%m-%d").to_string())
            .unwrap_or_default(),
        "RecallStatus" => recall_status.map(|num| num.to_string()).unwrap_or_default(),
        _ => String::new(),
    }
}

/// `FormRpPatients`: upstream's raw SQL query builder over the `patient` table.
/// The user picks any subset of columns and stacks WHERE prerequisites joined
/// with AND; the generated SQL has no ORDER BY, so the port orders by surname,
/// forename and `PatNum` for a stable export. Patients without a recall or a
/// completed procedure are kept with blank derived values, whereas upstream's
/// comma joins would drop them — the port avoids that row multiplication.
pub fn patients_report(
    patients: &[Patient],
    procedures: &[Procedure],
    recalls: &[Recall],
    options: &PatientsReportOptions,
) -> ReportTable {
    let headers: Vec<&str> = options.fields.iter().map(String::as_str).collect();
    let mut table = ReportTable::new(&headers);

    // Index the two correlated lookups once, so every patient renders as one
    // row no matter how many procedures or recalls reference them.
    let mut proc_dates: HashMap<Id, (Option<NaiveDate>, Option<NaiveDate>)> = HashMap::new();
    for proc in procedures {
        if proc.proc_status != ProcedureStatus::Complete {
            continue;
        }
        let entry = proc_dates.entry(proc.pat_num).or_insert((None, None));
        if entry.0.map_or(true, |date| proc.proc_date < date) {
            entry.0 = Some(proc.proc_date);
        }
        if entry.1.map_or(true, |date| proc.proc_date > date) {
            entry.1 = Some(proc.proc_date);
        }
    }
    let mut recall_statuses: HashMap<Id, Id> = HashMap::new();
    for recall in recalls {
        recall_statuses.insert(recall.pat_num, recall.recall_status);
    }

    let mut rows: Vec<&Patient> = patients.iter().collect();
    rows.sort_by(|left, right| {
        left.l_name
            .to_ascii_lowercase()
            .cmp(&right.l_name.to_ascii_lowercase())
            .then(left.f_name.to_ascii_lowercase().cmp(&right.f_name.to_ascii_lowercase()))
            .then(left.pat_num.cmp(&right.pat_num))
    });
    for patient in rows {
        let dates = proc_dates.get(&patient.pat_num);
        let first_complete = dates.and_then(|dates| dates.0);
        let last_complete = dates.and_then(|dates| dates.1);
        let recall_status = recall_statuses.get(&patient.pat_num).copied();
        let passes = options.filters.iter().all(|filter| {
            let actual = patient_field_value(
                patient,
                &filter.field,
                first_complete,
                last_complete,
                recall_status,
            );
            report_filter_matches(
                &actual,
                filter,
                &["PatNum", "Gender", "PriProv", "ClinicNum", "RecallStatus"],
            )
        });
        if !passes {
            continue;
        }
        let values: Vec<String> = options
            .fields
            .iter()
            .map(|field| {
                patient_field_value(
                    patient,
                    field,
                    first_complete,
                    last_complete,
                    recall_status,
                )
            })
            .collect();
        table.push(&values.iter().map(String::as_str).collect::<Vec<_>>());
    }
    table
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Minimal RFC-4180 CSV, safe to open in Excel or LibreOffice. `practice` is
/// repeated on every row so the file is self-identifying after sorting/filters.
/// The column header comes from the table, so upstream reports keep OD's names.
pub fn render_report_csv(title: &str, practice: &str, table: &ReportTable) -> String {
    let mut out = String::from("Practice,Report,");
    for (index, header) in table.headers.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&csv_field(header));
    }
    out.push_str("\r\n");
    for row in &table.rows {
        out.push_str(&csv_field(practice));
        out.push(',');
        out.push_str(&csv_field(title));
        for value in row {
            out.push(',');
            out.push_str(&csv_field(value));
        }
        out.push_str("\r\n");
    }
    out
}

/// A compact printable HTML table, matching the statement output's style.
pub fn render_report_html(title: &str, practice: &str, table: &ReportTable) -> String {
    let mut out = String::from("<!doctype html><html><head><meta charset=\"utf-8\">");
    out.push_str("<title>");
    out.push_str(&crate::escape_html(title));
    out.push_str("</title>");
    out.push_str("<style>body{font-family:Segoe UI,Arial,sans-serif;margin:2rem;}");
    out.push_str("table{border-collapse:collapse;width:100%;}");
    out.push_str("th,td{border:1px solid #ccc;padding:.35rem .5rem;text-align:left;}");
    out.push_str(".practice{color:#555;margin-top:-1rem;}");
    out.push_str("</style></head><body><h1>");
    out.push_str(&crate::escape_html(title));
    out.push_str("</h1>");
    if !practice.trim().is_empty() {
        out.push_str("<p class=\"practice\">");
        out.push_str(&crate::escape_html(practice));
        out.push_str("</p>");
    }
    out.push_str("<table><thead><tr>");
    for header in &table.headers {
        out.push_str("<th>");
        out.push_str(&crate::escape_html(header));
        out.push_str("</th>");
    }
    out.push_str("</tr></thead><tbody>");
    for row in &table.rows {
        out.push_str("<tr>");
        for value in row {
            out.push_str("<td>");
            out.push_str(&crate::escape_html(value));
            out.push_str("</td>");
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table></body></html>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use od_domain::reports::{ReferralFilter, PATIENT_REPORT_FIELDS};

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn patient(num: Id, last: &str) -> Patient {
        Patient {
            pat_num: num,
            l_name: last.into(),
            f_name: "Ada".into(),
            ..Default::default()
        }
    }

    #[test]
    fn recall_report_sorts_overdue_first_and_prefers_editable_due_date() {
        let patients = [patient(1, "Alvarez"), patient(2, "Bennett")];
        let today = day(2026, 11, 21);
        let recalls = [
            Recall {
                pat_num: 1,
                date_due_calc: Some(day(2026, 11, 20)),
                date_due: Some(day(2026, 12, 1)),
                ..Default::default()
            },
            Recall {
                pat_num: 2,
                date_due: Some(day(2026, 11, 10)),
                note: "Call before lunch".into(),
                ..Default::default()
            },
        ];
        let rows = recall_report(&recalls, &patients, today);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].label.contains("Bennett"), "overdue patient first");
        assert!(rows[0].value.contains("11d overdue"));
        assert!(rows[0].detail.contains("Call before lunch"));
        assert!(rows[1].value.contains("Due in 10d"), "DateDue wins over DateDueCalc");
    }

    #[test]
    fn asap_report_uses_priority_and_date_range() {
        let patients = [patient(1, "Alvarez"), patient(2, "Bennett")];
        let appt = |num, pat, hour, priority| Appointment {
            apt_num: num,
            pat_num: pat,
            apt_datetime: day(2026, 11, 21).and_hms_opt(hour, 0, 0).unwrap(),
            priority,
            ..Default::default()
        };
        let rows = asap_report(
            &[
                appt(1, 1, 9, RecallPriority::Asap),
                appt(2, 2, 8, RecallPriority::Asap),
                appt(3, 1, 7, RecallPriority::Normal),
            ],
            &patients,
            day(2026, 11, 21).and_hms_opt(0, 0, 0).unwrap(),
            day(2026, 11, 21).and_hms_opt(23, 59, 59).unwrap(),
        );
        assert_eq!(rows.len(), 2, "normal priority is excluded");
        assert!(rows[0].label.contains("Bennett"), "earliest ASAP first");
        assert!(rows[0].value.contains("08:00"));
        let none = asap_report(
            &[appt(1, 1, 9, RecallPriority::Asap)],
            &patients,
            day(2026, 11, 22).and_hms_opt(0, 0, 0).unwrap(),
            day(2026, 11, 22).and_hms_opt(23, 59, 59).unwrap(),
        );
        assert!(none.is_empty(), "outside the date range");
    }

    #[test]
    fn payment_plan_report_matches_account_balances_and_totals() {
        let patients = [patient(1, "Alvarez")];
        let plan = PayPlan {
            pay_plan_num: 5,
            pat_num: 1,
            pay_plan_date: day(2026, 1, 1),
            ..Default::default()
        };
        let charges = [
            PayPlanCharge {
                pay_plan_num: 5,
                charge_date: day(2026, 11, 1),
                charge_type: PayPlanChargeType::Debit,
                principal: 100.0,
                interest: 5.0,
                ..Default::default()
            },
            PayPlanCharge {
                pay_plan_num: 5,
                charge_date: day(2026, 11, 2),
                charge_type: PayPlanChargeType::Credit,
                principal: 20.0,
                ..Default::default()
            },
        ];
        let splits = [PaySplit {
            pay_plan_num: 5,
            split_amount: 30.0,
            date_pay: day(2026, 11, 1),
            ..Default::default()
        }];
        let rows = payment_plan_report(
            &[plan],
            &charges,
            &splits,
            &patients,
            day(2026, 11, 21),
        );
        assert_eq!(rows.len(), 2, "patient row plus total row");
        assert_eq!(rows[0].value, "55.00", "105 charged − 20 credit − 30 paid");
        assert!(rows[0].detail.contains("2 charge(s)"));
        assert_eq!(rows[1].label, "Total outstanding");
        assert_eq!(rows[1].value, "55.00");
        assert!(rows[1].detail.contains("due now 55.00"));
    }

    #[test]
    fn payment_plan_report_includes_dynamic_due_now() {
        let patients = [patient(1, "Alvarez")];
        let plan = PayPlan {
            pay_plan_num: 7,
            pat_num: 1,
            pay_plan_date: day(2026, 1, 1),
            is_dynamic: true,
            ..Default::default()
        };
        let charges = [
            PayPlanCharge {
                pay_plan_num: 7,
                charge_date: day(2026, 1, 1),
                charge_type: PayPlanChargeType::Debit,
                principal: 100.0,
                ..Default::default()
            },
            PayPlanCharge {
                pay_plan_num: 7,
                charge_date: day(2026, 3, 1),
                charge_type: PayPlanChargeType::Debit,
                principal: 100.0,
                ..Default::default()
            },
        ];
        let splits = [PaySplit {
            pay_plan_num: 7,
            split_amount: 40.0,
            date_pay: day(2026, 1, 15),
            ..Default::default()
        }];
        let rows = payment_plan_report(&[plan], &charges, &splits, &patients, day(2026, 2, 1));
        assert_eq!(rows[0].value, "160.00");
        assert!(rows[0].detail.contains("dynamic"));
        assert!(rows[0].detail.contains("due now 60.00"));
        assert!(rows[0].detail.contains("future 100.00"));
    }

    #[test]
    fn payment_plan_report_partial_period_reconciliation() {
        let patients = [patient(1, "Alvarez")];
        let plan = PayPlan { pay_plan_num: 8, pat_num: 1, pay_plan_date: day(2026, 1, 1), ..Default::default() };
        let charges = [
            PayPlanCharge { pay_plan_num: 8, charge_date: day(2026, 1, 1), charge_type: PayPlanChargeType::Debit, principal: 100.0, ..Default::default() },
            PayPlanCharge { pay_plan_num: 8, charge_date: day(2026, 2, 1), charge_type: PayPlanChargeType::Debit, principal: 100.0, ..Default::default() },
            PayPlanCharge { pay_plan_num: 8, charge_date: day(2026, 3, 1), charge_type: PayPlanChargeType::Debit, principal: 100.0, ..Default::default() },
        ];
        let splits = [PaySplit { pay_plan_num: 8, split_amount: 150.0, date_pay: day(2026, 2, 15), ..Default::default() }];
        let as_of_mid_period = day(2026, 2, 15);
        let rows = payment_plan_report(&[plan], &charges, &splits, &patients, as_of_mid_period);
        assert_eq!(rows[0].value, "150.00");
        assert!(rows[0].detail.contains("due now 50.00"));
        assert!(rows[0].detail.contains("future 100.00"));
    }

    #[test]
    fn treatment_plan_report_sums_lines_and_shows_status() {
        let patients = [patient(1, "Alvarez")];
        let plan = TreatPlan {
            treat_plan_num: 9,
            pat_num: 1,
            heading: "Crowns".into(),
            status: TreatPlanStatus::Active,
            ..Default::default()
        };
        let lines = [
            TreatPlanLine {
                treat_plan_num: 9,
                fee: 400.0,
                ..Default::default()
            },
            TreatPlanLine {
                treat_plan_num: 9,
                fee: 250.0,
                ..Default::default()
            },
            TreatPlanLine {
                treat_plan_num: 99,
                fee: 999.0,
                ..Default::default()
            },
        ];
        let rows = treatment_plan_report(&[plan], &lines, &patients);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].value, "650.00");
        assert!(rows[0].detail.contains("Active"));
        assert!(rows[0].detail.contains("2 line(s)"));
        assert_eq!(rows[1].value, "650.00", "unrelated plan lines are excluded");
    }

    #[test]
    fn aging_report_applies_credits_to_oldest_bucket() {
        let patients = [patient(1, "Alvarez")];
        let as_of = day(2026, 11, 21);
        let procedures = [
            Procedure {
                pat_num: 1,
                proc_date: day(2026, 8, 1), // > 90 days
                proc_status: ProcedureStatus::Complete,
                fee: 100.0,
                ..Default::default()
            },
            Procedure {
                pat_num: 1,
                proc_date: day(2026, 11, 10), // 0–30 days
                proc_status: ProcedureStatus::Complete,
                fee: 50.0,
                ..Default::default()
            },
        ];
        let splits = [PaySplit {
            pat_num: 1,
            split_amount: 80.0,
            date_pay: day(2026, 11, 15),
            ..Default::default()
        }];
        let table = aging_report(
            &patients,
            &procedures,
            &[],
            &splits,
            &[],
            as_of,
            false,
            AgingOptions {
                show_last_pay: true,
                ..Default::default()
            },
        );
        assert_eq!(table.headers, [
            "Patient", "0-30 Days", "31-60 Days", "61-90 Days", "> 90 Days", "Total", "Last Pay Date"
        ]);
        assert_eq!(table.rows[0][1], "50.00", "new charge untouched");
        assert_eq!(table.rows[0][5], "70.00", "100 + 50 − 80");
        assert_eq!(table.rows[0][4], "20.00", "payment clears the oldest bucket first");
        assert_eq!(table.rows[0][6], "2026-11-15");
    }

    #[test]
    fn aging_report_hides_insurance_until_enabled() {
        let patients = [patient(1, "Alvarez")];
        let procedures = [Procedure {
            pat_num: 1,
            proc_date: day(2026, 11, 1),
            proc_status: ProcedureStatus::Complete,
            fee: 40.0,
            ..Default::default()
        }];
        let plain = aging_report(
            &patients,
            &procedures,
            &[],
            &[],
            &[],
            day(2026, 11, 21),
            false,
            AgingOptions::default(),
        );
        let insured = aging_report(
            &patients,
            &procedures,
            &[],
            &[],
            &[],
            day(2026, 11, 21),
            true,
            AgingOptions::default(),
        );
        assert!(!plain.headers.iter().any(|h| h.contains("Ins")));
        assert!(insured.headers.iter().any(|h| h == "-Ins Est"));
    }

    #[test]
    fn aging_options_filter_rows_and_columns() {
        let patients = [patient(1, "Alvarez")];
        let today = day(2026, 11, 21);
        let recent = [Procedure {
            pat_num: 1,
            proc_date: day(2026, 11, 10),
            proc_status: ProcedureStatus::Complete,
            fee: 100.0,
            ..Default::default()
        }];
        let old = [Procedure {
            pat_num: 1,
            proc_date: day(2026, 5, 10),
            proc_status: ProcedureStatus::Complete,
            fee: 100.0,
            ..Default::default()
        }];
        let opts = AgingOptions::default();
        assert_eq!(
            aging_report(&patients, &recent, &[], &[], &[], today, false, opts).rows.len(),
            2,
            "detail row plus total for any balance"
        );
        assert_eq!(
            aging_report(
                &patients,
                &recent,
                &[],
                &[],
                &[],
                today,
                false,
                AgingOptions {
                    age: AgingAgeFilter::Over90,
                    ..opts
                }
            )
            .rows
            .len(),
            1,
            "recent charge is not over 90 days"
        );
        assert_eq!(
            aging_report(
                &patients,
                &old,
                &[],
                &[],
                &[],
                today,
                false,
                AgingOptions {
                    age: AgingAgeFilter::Over90,
                    ..opts
                }
            )
            .rows
            .len(),
            2,
            "old charge is over 90 days"
        );

        let credit = [Adjustment {
            pat_num: 1,
            adj_date: day(2026, 11, 1),
            adj_amount: -150.0,
            ..Default::default()
        }];
        let hidden = aging_report(&patients, &recent, &credit, &[], &[], today, false, opts);
        assert_eq!(hidden.rows.len(), 1, "negative balances are hidden by default");
        let included = aging_report(
            &patients,
            &recent,
            &credit,
            &[],
            &[],
            today,
            false,
            AgingOptions {
                include_negative: true,
                ..opts
            },
        );
        assert_eq!(included.rows.len(), 2, "checkbox includes the credit balance");
        let reversed = aging_report(
            &patients,
            &recent,
            &[],
            &[],
            &[],
            today,
            false,
            AgingOptions {
                only_negative: true,
                ..opts
            },
        );
        assert_eq!(reversed.rows.len(), 1, "only-negative drops a positive balance");
    }

    #[test]
    fn custom_aging_report_ages_selected_transactions_and_totals() {
        let patients = [Patient {
            zip: "1000".into(),
            ..patient(1, "Alvarez")
        }];
        let procedures = [
            Procedure {
                pat_num: 1,
                proc_date: day(2026, 8, 1), // > 90 days
                proc_status: ProcedureStatus::Complete,
                fee: 100.0,
                unit_qty: 1.0,
                ..Default::default()
            },
            Procedure {
                pat_num: 1,
                proc_date: day(2026, 11, 10), // 0–30 days
                proc_status: ProcedureStatus::Complete,
                fee: 50.0,
                unit_qty: 1.0,
                ..Default::default()
            },
            Procedure {
                pat_num: 1,
                proc_date: day(2026, 11, 12),
                proc_status: ProcedureStatus::TreatmentPlanned,
                fee: 999.0,
                unit_qty: 1.0,
                ..Default::default()
            },
        ];
        let adjustments = [Adjustment {
            pat_num: 1,
            adj_date: day(2026, 11, 15),
            adj_amount: -30.0,
            ..Default::default()
        }];
        let splits = [PaySplit {
            pat_num: 1,
            split_amount: 20.0,
            date_pay: day(2026, 11, 16),
            ..Default::default()
        }];
        let plan_charges = [PayPlanCharge {
            pat_num: 1,
            charge_date: day(2026, 10, 20), // 31–60 days
            charge_type: PayPlanChargeType::Credit,
            principal: 40.0,
            ..Default::default()
        }];
        let table = custom_aging_report(
            &patients,
            &procedures,
            &adjustments,
            &splits,
            &plan_charges,
            &[],
            &[],
            day(2026, 11, 21),
            false,
            CustomAgingReportOptions::default(),
        );
        assert_eq!(table.headers, [
            "Patient", "0-30 Days", "31-60 Days", "61-90 Days", "> 90 Days", "Total"
        ]);
        assert_eq!(table.rows.len(), 2, "one detail row plus Totals");
        assert_eq!(table.rows[0][0], "Alvarez, Ada");
        assert_eq!(table.rows[0][1], "50.00", "recent fee untouched");
        assert_eq!(table.rows[0][2], "0.00", "plan production is a credit");
        assert_eq!(table.rows[0][4], "10.00", "all credits clear the oldest bucket first");
        assert_eq!(table.rows[0][5], "60.00", "150 fees − 30 adj − 20 pay − 40 production");
        assert_eq!(table.rows[1], [
            "Totals:", "50.00", "0.00", "0.00", "10.00", "60.00"
        ]);
    }

    #[test]
    fn custom_aging_report_age_credits_and_negative_filters() {
        let patients = [patient(1, "Alvarez")];
        let procedures = [Procedure {
            pat_num: 1,
            proc_date: day(2026, 11, 10),
            proc_status: ProcedureStatus::Complete,
            fee: -100.0,
            unit_qty: 1.0,
            ..Default::default()
        }];
        let as_of = day(2026, 11, 21);
        let build = |options| {
            custom_aging_report(
                &patients, &procedures, &[], &[], &[], &[], &[], as_of, false, options,
            )
        };
        let hidden = build(CustomAgingReportOptions::default());
        assert_eq!(hidden.rows.len(), 1, "negative balance hidden by default");

        let aged = build(CustomAgingReportOptions {
            age_credits: true,
            negative: CustomAgingNegative::ShowOnly,
            ..Default::default()
        });
        assert_eq!(aged.rows.len(), 2);
        assert_eq!(aged.rows[0][1], "-100.00", "Age Credits keeps credits in their own bucket");
        assert_eq!(aged.rows[0][5], "-100.00");

        let oldest_first = build(CustomAgingReportOptions {
            negative: CustomAgingNegative::ShowOnly,
            ..Default::default()
        });
        assert_eq!(
            oldest_first.rows.len(),
            1,
            "without Age Credits a credit-only account has no nonzero bucket, so upstream's Age of Account filter removes it"
        );

        let over30 = build(CustomAgingReportOptions {
            age: AgingAgeFilter::Over30,
            age_credits: true,
            negative: CustomAgingNegative::Include,
            ..Default::default()
        });
        assert_eq!(over30.rows.len(), 1, "a recent-only balance is not over 30 days");
    }

    #[test]
    fn custom_aging_report_respects_transaction_and_patient_filters() {
        let patients = [Patient {
            pat_num: 1,
            l_name: "Alvarez".into(),
            f_name: "Ada".into(),
            primary_provider_num: 7,
            clinic_num: 2,
            zip: "1000".into(),
            ..Default::default()
        }];
        let procedures = [Procedure {
            pat_num: 1,
            proc_date: day(2026, 11, 10),
            proc_status: ProcedureStatus::Complete,
            fee: 100.0,
            unit_qty: 1.0,
            ..Default::default()
        }];
        let adjustments = [Adjustment {
            pat_num: 1,
            adj_date: day(2026, 11, 11),
            adj_amount: -20.0,
            ..Default::default()
        }];
        let splits = [PaySplit {
            pat_num: 1,
            split_amount: 10.0,
            date_pay: day(2026, 11, 12),
            pay_plan_num: 5,
            ..Default::default()
        }];
        let plan_charges = [
            PayPlanCharge {
                pat_num: 1,
                charge_date: day(2026, 11, 13),
                charge_type: PayPlanChargeType::Debit,
                principal: 30.0,
                ..Default::default()
            },
            PayPlanCharge {
                pat_num: 1,
                charge_date: day(2026, 11, 14),
                charge_type: PayPlanChargeType::Credit,
                principal: 5.0,
                ..Default::default()
            },
        ];
        let as_of = day(2026, 11, 21);
        let build = |options| {
            custom_aging_report(
                &patients, &procedures, &adjustments, &splits, &plan_charges, &[], &[], as_of,
                false, options,
            )
        };

        let only_fees = build(CustomAgingReportOptions {
            include: CustomAgingInclude {
                procedure_fees: true,
                adjustments: false,
                pay_plan_charges: false,
                pay_plan_production: false,
                patient_payments: false,
                ..CustomAgingInclude::default()
            },
            ..Default::default()
        });
        assert_eq!(only_fees.rows[0][5], "100.00", "unselected transaction types are ignored");

        let both_plan_sides = build(CustomAgingReportOptions {
            include: CustomAgingInclude {
                pay_plan_charges: true,
                pay_plan_production: true,
                ..CustomAgingInclude::default()
            },
            ..Default::default()
        });
        assert_eq!(
            both_plan_sides.rows[0][5], "95.00",
            "debit 30 + production −5 + plan payment −10 join the fee and adjustment"
        );

        let no_types = build(CustomAgingReportOptions {
            include: CustomAgingInclude {
                procedure_fees: false,
                adjustments: false,
                pay_plan_charges: false,
                pay_plan_production: false,
                patient_payments: false,
                ..CustomAgingInclude::default()
            },
            ..Default::default()
        });
        assert!(no_types.rows.is_empty(), "upstream refuses to run with no types");

        let wrong_provider = build(CustomAgingReportOptions {
            all_providers: false,
            provider_nums: vec![9],
            ..Default::default()
        });
        assert_eq!(wrong_provider.rows.len(), 1, "provider filter removes the patient");
        let right_provider = build(CustomAgingReportOptions {
            all_providers: false,
            provider_nums: vec![7],
            ..Default::default()
        });
        assert_eq!(right_provider.rows.len(), 2);

        let wrong_clinic = build(CustomAgingReportOptions {
            all_clinics: false,
            clinic_nums: vec![9],
            ..Default::default()
        });
        assert_eq!(wrong_clinic.rows.len(), 1, "clinic filter removes the patient");
        let bad_address = build(CustomAgingReportOptions {
            exclude_bad_address: true,
            ..Default::default()
        });
        assert_eq!(bad_address.rows.len(), 2, "a zip is present, so the patient stays");
        let no_zip = custom_aging_report(
            &[Patient {
                zip: String::new(),
                ..patients[0].clone()
            }],
            &procedures,
            &adjustments,
            &splits,
            &plan_charges,
            &[],
            &[],
            as_of,
            false,
            CustomAgingReportOptions {
                exclude_bad_address: true,
                ..Default::default()
            },
        );
        assert_eq!(no_zip.rows.len(), 1, "bad addresses are excluded when requested");
    }

    #[test]
    fn pay_plan_detail_report_uses_upstream_column_formulas() {
        let patients = [patient(1, "Alvarez")];
        let providers = [Provider {
            provider_num: 7,
            l_name: "Smith".into(),
            f_name: "Jo".into(),
            ..Default::default()
        }];
        let plan = PayPlan {
            pay_plan_num: 5,
            pat_num: 1,
            guarantor: 1,
            pay_plan_date: day(2026, 10, 1),
            ..Default::default()
        };
        let charges = [
            PayPlanCharge {
                pay_plan_num: 5,
                charge_date: day(2026, 10, 1),
                charge_type: PayPlanChargeType::Debit,
                principal: 300.0,
                interest: 12.0,
                provider_num: 7,
                ..Default::default()
            },
            PayPlanCharge {
                pay_plan_num: 5,
                charge_date: day(2026, 12, 1), // future charge
                charge_type: PayPlanChargeType::Debit,
                principal: 100.0,
                ..Default::default()
            },
        ];
        let splits = [PaySplit {
            pay_plan_num: 5,
            split_amount: 62.0,
            ..Default::default()
        }];
        let table = pay_plan_detail_report(
            &[plan],
            &charges,
            &splits,
            &[],
            &[],
            &[],
            &patients,
            &providers,
            &[(1, 500.0)],
            day(2026, 1, 1),
            day(2026, 11, 21),
            false,
            PayPlanReportOptions {
                show_family_balance: true,
                ..Default::default()
            },
        );
        assert_eq!(table.headers[0], "Provider");
        assert!(table.headers.contains(&"Bal Not Due".to_string()));
        let row = &table.rows[0];
        assert_eq!(row[0], "Smith, Jo");
        assert_eq!(row[1], "Alvarez, Ada");
        assert_eq!(row[2], "400.00", "principal includes the future charge");
        assert_eq!(row[3], "12.00", "only interest due by the as-of date");
        assert_eq!(row[4], "62.00");
        assert_eq!(row[5], "350.00", "400 + 12 − 62");
        assert_eq!(row[6], "250.00", "312 − 62 due now");
        assert_eq!(row[7], "100.00", "balance − due now");
        assert_eq!(row[8], "0.00", "no unapplied credit");
        assert_eq!(row[9], "500.00", "family balance");
        assert!(table.rows[1][0].starts_with("Total (1 plan"));
    }

    #[test]
    fn pay_plan_detail_report_uses_dynamic_linked_principal() {
        let patients = [patient(1, "Alvarez")];
        let providers = [Provider { provider_num: 7, l_name: "Smith".into(), f_name: "Jo".into(), ..Default::default() }];
        let plan = PayPlan {
            pay_plan_num: 8,
            pat_num: 1,
            guarantor: 1,
            pay_plan_date: day(2026, 1, 1),
            is_dynamic: true,
            ..Default::default()
        };
        let links = [PayPlanLink { pay_plan_link_num: 1, pay_plan_num: 8, link_type: PayPlanLink::ADJUSTMENT, fkey: 10, ..Default::default() }];
        let adjustments = [Adjustment { adj_num: 10, pat_num: 1, provider_num: 7, adj_amount: 300.0, adj_date: day(2026, 1, 2), ..Default::default() }];
        let charges = [PayPlanCharge {
            pay_plan_num: 8, charge_date: day(2026, 1, 1), charge_type: PayPlanChargeType::Debit,
            principal: 100.0, interest: 0.0, provider_num: 7, fkey: 10, link_type: PayPlanLink::ADJUSTMENT, ..Default::default()
        }];
        let table = pay_plan_detail_report(
            &[plan], &charges, &[], &links, &[], &adjustments, &patients, &providers, &[(1, 0.0)],
            day(2026, 1, 1), day(2026, 2, 1), false, PayPlanReportOptions::default(),
        );
        assert_eq!(table.rows[0][2], "300.00", "dynamic principal comes from linked production");
        assert_eq!(table.rows[0][6], "100.00", "due now from issued charges");
    }

    #[test]
    fn birthday_and_new_patient_reports_filter_and_sort() {
        let mut birthday = patient(1, "Alvarez");
        birthday.birth_date = Some(day(1980, 12, 25));
        let mut other = patient(2, "Bennett");
        other.birth_date = Some(day(1990, 1, 5));
        let patients = [birthday.clone(), other.clone()];
        let table = birthday_report(
            &patients,
            day(2026, 12, 1),
            day(2027, 1, 31),
            day(2026, 11, 21),
        );
        assert_eq!(table.headers.len(), 10, "upstream birthday column set");
        assert_eq!(table.rows.len(), 2, "range wraps the new year");
        assert_eq!(table.rows[0][0], "Alvarez");
        assert_eq!(table.rows[0][9], "45", "age as of the report date");

        let chen = patient(3, "Chen");
        let procedures = [Procedure {
            pat_num: 3,
            proc_date: day(2026, 11, 15),
            proc_status: ProcedureStatus::Complete,
            fee: 250.0,
            ..Default::default()
        }];
        let options = NewPatientReportOptions::default();
        let table = new_patient_report(
            &[chen.clone(), birthday.clone()],
            &procedures,
            day(2026, 11, 1),
            day(2026, 11, 30),
            options,
        );
        assert_eq!(table.headers, ["#", "Date", "LName", "FName", "Referral", "Production Fee"]);
        assert_eq!(table.rows.len(), 2, "detail row plus total");
        assert_eq!(table.rows[0][2], "Chen");
        assert_eq!(table.rows[0][5], "250.00");
        assert!(table.rows[1][0].starts_with("Total (1 patient"));

        let with_address = new_patient_report(
            &[chen],
            &procedures,
            day(2026, 11, 1),
            day(2026, 11, 30),
            NewPatientReportOptions {
                include_address: true,
                exclude_no_production: true,
            },
        );
        assert_eq!(with_address.headers.len(), 12, "address columns are optional");

        let free = [Procedure {
            pat_num: 3,
            proc_date: day(2026, 11, 15),
            proc_status: ProcedureStatus::Complete,
            fee: 0.0,
            ..Default::default()
        }];
        let excluded = new_patient_report(
            &[patient(3, "Chen")],
            &free,
            day(2026, 11, 1),
            day(2026, 11, 30),
            NewPatientReportOptions {
                exclude_no_production: true,
                ..Default::default()
            },
        );
        assert_eq!(excluded.rows.len(), 1, "no production means no detail row");
    }

    #[test]
    fn report_table_sorts_numerically_and_exports_headers() {
        let mut table = ReportTable::new(&["Item", "Value", "Detail"]);
        table.push(&["Nine", "9.00", "a"]);
        table.push(&["Hundred", "100.00", "b"]);
        table.push(&["Total", "", ""]);
        table.sort_by_column(1, false);
        assert_eq!(table.rows[0][0], "Nine", "9 before 100 numerically");
        table.sort_by_column(0, true);
        assert_eq!(table.rows[0][0], "Total", "text sorts reverse alphabetically");

        let rows = vec![ReportRow::new(
            "Alvarez, Ada",
            "12.00",
            "Said \"yes\"\nagain",
        )];
        let table = ReportTable::from_rows(rows);
        let csv = render_report_csv("Recall & ASAP", "Highland Dental", &table);
        assert!(csv.starts_with("Practice,Report,Item,Value,Detail\r\n"));
        assert!(csv.contains("\"Alvarez, Ada\""));
        assert!(csv.contains("\"Said \"\"yes\"\"\nagain\""));
        assert!(csv.contains("Highland Dental"));
        let html = render_report_html("Recall & ASAP", "Highland Dental", &table);
        assert!(html.contains("Recall &amp; ASAP"));
        assert!(html.contains("Highland Dental"));
        assert!(html.contains("Alvarez, Ada"));
    }

    fn provider(num: Id, abbr: &str) -> Provider {
        Provider {
            provider_num: num,
            abbreviation: abbr.into(),
            ..Default::default()
        }
    }

    #[test]
    fn appointments_report_filters_statuses_and_dates() {
        let mut alvarez = patient(1, "Alvarez");
        alvarez.birth_date = Some(day(1980, 11, 1));
        alvarez.home_phone = "011-234".into();
        alvarez.wireless_phone = "091-999".into();
        let appointments = [
            Appointment {
                pat_num: 1,
                apt_datetime: day(2026, 11, 15).and_hms_opt(10, 0, 0).unwrap(),
                apt_status: AppointmentStatus::Scheduled,
                pattern: "XXXX".into(),
                notes: "Cleaning".into(),
                ..Default::default()
            },
            Appointment {
                apt_num: 2,
                pat_num: 1,
                apt_datetime: day(2026, 11, 15).and_hms_opt(12, 0, 0).unwrap(),
                apt_status: AppointmentStatus::Planned,
                ..Default::default()
            },
            Appointment {
                apt_num: 3,
                pat_num: 1,
                apt_datetime: day(2026, 11, 16).and_hms_opt(9, 0, 0).unwrap(),
                apt_status: AppointmentStatus::PtNote,
                notes: "Called back".into(),
                ..Default::default()
            },
            Appointment {
                apt_num: 4,
                pat_num: 1,
                apt_datetime: day(2026, 12, 5).and_hms_opt(9, 0, 0).unwrap(),
                apt_status: AppointmentStatus::Complete,
                ..Default::default()
            },
        ];
        let from = day(2026, 11, 1);
        let to = day(2026, 11, 30);
        let opts = AppointmentReportOptions::default();
        let table = appointments_report(&appointments, &[alvarez.clone()], from, to, day(2026, 11, 21), opts);
        assert_eq!(table.headers.len(), 10);
        assert_eq!(table.headers[0], "Date");
        assert_eq!(table.rows.len(), 1, "planned, note and out-of-range rows excluded");
        assert_eq!(table.rows[0][0], "2026-11-15");
        assert_eq!(table.rows[0][1], "1", "PatNum");
        assert!(table.rows[0][2].contains("Alvarez"), "patient name");
        assert_eq!(table.rows[0][3], "46", "age as of the report date");
        assert_eq!(table.rows[0][4], "10:00 AM");
        assert_eq!(table.rows[0][5], "20", "4 pattern increments × 5 minutes");
        assert_eq!(table.rows[0][6], "Cleaning");
        assert_eq!(table.rows[0][7], "011-234");
        assert_eq!(table.rows[0][8], "", "work phone is not stored");

        let with_notes = appointments_report(
            &appointments,
            &[alvarez.clone()],
            from,
            to,
            day(2026, 11, 21),
            AppointmentReportOptions {
                show_note_appointments: true,
                ..opts
            },
        );
        assert_eq!(with_notes.rows.len(), 2, "note appointments are opt-in");

        let created = appointments_report(
            &appointments,
            &[alvarez],
            from,
            to,
            day(2026, 11, 21),
            AppointmentReportOptions {
                date_basis: AppointmentDateBasis::Created,
                ..opts
            },
        );
        assert_eq!(created.headers.len(), 11);
        assert_eq!(created.headers[0], "DateCreated");
        assert_eq!(created.headers[1], "AptDate");
    }

    #[test]
    fn active_patient_report_requires_completed_procedure_in_range() {
        let patients = [patient(1, "Alvarez"), patient(2, "Bennett")];
        let providers = [provider(7, "SM")];
        let mut alvarez = patients[0].clone();
        alvarez.primary_provider_num = 7;
        alvarez.city = "Addis Ababa".into();
        let mut procedures = vec![Procedure {
            pat_num: 1,
            proc_date: day(2026, 11, 10),
            proc_status: ProcedureStatus::Complete,
            ..Default::default()
        }];
        procedures.push(Procedure {
            proc_num: 2,
            pat_num: 1,
            proc_date: day(2026, 11, 12),
            proc_status: ProcedureStatus::TreatmentPlanned,
            ..Default::default()
        });
        procedures.push(Procedure {
            proc_num: 3,
            pat_num: 2,
            proc_date: day(2025, 1, 1),
            proc_status: ProcedureStatus::Complete,
            ..Default::default()
        });
        let table = active_patient_report(
            &[alvarez.clone(), patients[1].clone()],
            &procedures,
            &providers,
            day(2026, 11, 1),
            day(2026, 11, 30),
            ActivePatientReportOptions::default(),
        );
        assert_eq!(table.headers.len(), 8);
        assert_eq!(table.headers[0], "Name");
        assert_eq!(table.headers[7], "Carrier");
        assert_eq!(table.rows.len(), 2, "detail row plus patient-count footer");
        assert!(table.rows[0][0].contains("Alvarez"));
        assert_eq!(table.rows[0][1], "SM");
        assert_eq!(table.rows[0][4], "Addis Ababa");
        assert_eq!(table.rows[1][7], "Patient Count: 1");

        let filtered = active_patient_report(
            &[alvarez],
            &procedures,
            &providers,
            day(2026, 11, 1),
            day(2026, 11, 30),
            ActivePatientReportOptions {
                all_providers: false,
                provider_num: 99,
            },
        );
        assert_eq!(filtered.rows.len(), 1, "no patients for provider 99");
        assert_eq!(filtered.rows[0][7], "Patient Count: 0");
    }

    fn prod_inc_fixture() -> (Vec<Patient>, Vec<Provider>, Vec<Procedure>, Vec<Adjustment>, Vec<PaySplit>, Vec<ClaimProc>) {
        let patients = vec![patient(1, "Alvarez")];
        let providers = vec![provider(7, "SM")];
        let procedures = vec![
            Procedure {
                proc_num: 1,
                pat_num: 1,
                proc_date: day(2026, 11, 10),
                proc_status: ProcedureStatus::Complete,
                code_num: 101,
                provider_num: 7,
                fee: 100.0,
                ..Default::default()
            },
            Procedure {
                proc_num: 2,
                pat_num: 1,
                proc_date: day(2026, 12, 1),
                proc_status: ProcedureStatus::Complete,
                provider_num: 7,
                fee: 50.0,
                ..Default::default()
            },
        ];
        let adjustments = vec![Adjustment {
            pat_num: 1,
            adj_date: day(2026, 11, 11),
            adj_amount: -10.0,
            provider_num: 7,
            ..Default::default()
        }];
        let splits = vec![PaySplit {
            pat_num: 1,
            split_amount: 60.0,
            date_pay: day(2026, 11, 12),
            provider_num: 7,
            ..Default::default()
        }];
        let claim_procs = vec![ClaimProc {
            proc_num: 1,
            status: 1,
            write_off: 20.0,
            ins_pay_amt: 30.0,
            ..Default::default()
        }];
        (patients, providers, procedures, adjustments, splits, claim_procs)
    }

    #[test]
    fn prod_inc_daily_and_provider_modes_use_patient_only_formulas() {
        let (patients, providers, procedures, adjustments, splits, claim_procs) = prod_inc_fixture();
        let from = day(2026, 11, 1);
        let to = day(2026, 11, 30);
        let daily = prod_inc_report(
            &patients, &providers, &procedures, &adjustments, &splits, &claim_procs,
            &[], &[], from, to, false, ProdIncReportOptions::default(),
        );
        assert_eq!(daily.headers.len(), 7, "no insurance columns by default");
        assert_eq!(daily.headers[6], "Pt Income");
        assert_eq!(daily.rows.len(), 5, "3 transactions plus 2 total rows");
        assert_eq!(daily.rows[0][4], "100.00", "production from completed procedure");
        assert_eq!(daily.rows[1][5], "-10.00", "adjustment in its own column");
        assert_eq!(daily.rows[2][6], "60.00", "patient payment in its own column");
        assert_eq!(daily.rows[3][0], "Total Production (Production + Adjustments)");
        assert_eq!(daily.rows[3][4], "90.00");
        assert_eq!(daily.rows[4][0], "Total Income (Pt Income)");
        assert_eq!(daily.rows[4][6], "60.00");

        let provider_table = prod_inc_report(
            &patients, &providers, &procedures, &adjustments, &splits, &claim_procs,
            &[], &[], from, to, false,
            ProdIncReportOptions { mode: ProdIncMode::Provider, ..Default::default() },
        );
        assert_eq!(provider_table.headers.len(), 6);
        assert_eq!(provider_table.rows[0][0], "SM");
        assert_eq!(provider_table.rows[0][1], "100.00");
        assert_eq!(provider_table.rows[0][2], "-10.00");
        assert_eq!(provider_table.rows[0][3], "90.00", "Tot Prod");
        assert_eq!(provider_table.rows[0][4], "60.00");
        assert_eq!(provider_table.rows[0][5], "60.00", "Total Income");
        assert!(provider_table.rows[1][0].starts_with("Total (1 provider"));
    }

    #[test]
    fn prod_inc_insurance_columns_and_totals_are_gated() {
        let (patients, providers, procedures, adjustments, splits, claim_procs) = prod_inc_fixture();
        let from = day(2026, 11, 1);
        let to = day(2026, 11, 30);
        let daily = prod_inc_report(
            &patients, &providers, &procedures, &adjustments, &splits, &claim_procs,
            &[], &[], from, to, true, ProdIncReportOptions::default(),
        );
        assert_eq!(daily.headers.len(), 9);
        assert_eq!(daily.headers[6], "Write-off");
        assert_eq!(daily.headers[8], "Ins Income");
        assert_eq!(daily.rows.len(), 6, "4 transactions plus 2 total rows");
        assert_eq!(daily.rows[1][6], "-20.00", "claim write-off is negative");
        assert_eq!(daily.rows[1][8], "30.00", "insurance receipt on its detail row");
        assert_eq!(daily.rows[4][4], "70.00", "100 − 10 − 20 write-off");
        assert_eq!(daily.rows[5][7], "90.00", "total income = 60 patient + 30 insurance");

        let provider_table = prod_inc_report(
            &patients, &providers, &procedures, &adjustments, &splits, &claim_procs,
            &[], &[], from, to, true,
            ProdIncReportOptions { mode: ProdIncMode::Provider, ..Default::default() },
        );
        assert_eq!(provider_table.headers.len(), 8);
        assert_eq!(provider_table.rows[0][3], "-20.00");
        assert_eq!(provider_table.rows[0][4], "70.00");
        assert_eq!(provider_table.rows[0][6], "30.00");
        assert_eq!(provider_table.rows[0][7], "90.00");
    }

    #[test]
    fn prod_inc_monthly_and_annual_aggregate_by_day_and_month() {
        let (patients, providers, mut procedures, adjustments, splits, claim_procs) =
            prod_inc_fixture();
        procedures.push(Procedure {
            proc_num: 3,
            pat_num: 1,
            proc_date: day(2026, 11, 13),
            apt_num: 5,
            proc_status: ProcedureStatus::TreatmentPlanned,
            fee: 200.0,
            ..Default::default()
        });
        // OD buckets Sched by the appointment date, not ProcDate. Giving the
        // appointment a different date proves the join goes via AptNum and is
        // not just a TP-procedure sum.
        let appointments = vec![Appointment {
            apt_num: 5,
            pat_num: 1,
            apt_datetime: day(2026, 11, 14).and_hms_opt(9, 0, 0).unwrap(),
            apt_status: AppointmentStatus::Scheduled,
            ..Default::default()
        }];
        let from = day(2026, 11, 1);
        let to = day(2026, 11, 30);

        let monthly = prod_inc_report(
            &patients, &providers, &procedures, &adjustments, &splits, &claim_procs,
            &appointments, &[], from, to, false,
            ProdIncReportOptions { mode: ProdIncMode::Monthly, ..Default::default() },
        );
        assert_eq!(monthly.headers.len(), 10);
        assert_eq!(monthly.headers[0], "Date");
        assert_eq!(monthly.headers[3], "Sched");
        assert_eq!(monthly.headers[7], "Unearned Pt Income");
        assert_eq!(monthly.headers[9], "Total Pt Income");
        assert_eq!(monthly.rows.len(), 9, "4 days with data plus 5 totals");
        assert_eq!(monthly.rows[0][0], "2026-11-10");
        assert_eq!(monthly.rows[0][1], "Tuesday");
        assert_eq!(monthly.rows[0][2], "100.00");
        assert_eq!(monthly.rows[1][4], "-10.00", "adjustment on 11-11");
        assert_eq!(monthly.rows[2][6], "60.00", "patient payment on 11-12");
        assert_eq!(monthly.rows[3][0], "2026-11-14");
        assert_eq!(monthly.rows[3][3], "200.00", "TP fee is Sched on the appointment date");
        assert_eq!(monthly.rows[3][5], "200.00");
        assert_eq!(monthly.rows[4][0], "Total Production (Production + Scheduled + Adjustments):");
        assert_eq!(monthly.rows[4][5], "290.00", "100 + 200 sched − 10 adjust");
        assert_eq!(monthly.rows[5][6], "60.00");
        assert_eq!(monthly.rows[6][0], "Total Unearned Pt Income (Unearned Pt Income):");
        assert_eq!(monthly.rows[7][8], "60.00");
        assert_eq!(monthly.rows[8][9], "60.00");

        let annual = prod_inc_report(
            &patients, &providers, &procedures, &adjustments, &splits, &claim_procs,
            &appointments, &[], from, to, false,
            ProdIncReportOptions { mode: ProdIncMode::Annual, ..Default::default() },
        );
        assert_eq!(annual.headers.len(), 8);
        assert_eq!(annual.headers[0], "Month");
        assert_eq!(annual.headers[5], "Unearned Pt Income");
        assert_eq!(annual.rows.len(), 6, "one month plus 5 totals");
        assert_eq!(annual.rows[0][0], "Nov 2026");
        assert_eq!(annual.rows[0][1], "100.00");
        assert_eq!(annual.rows[0][2], "-10.00");
        assert_eq!(annual.rows[0][3], "90.00", "annual has no Sched term");
        assert_eq!(annual.rows[0][4], "60.00");
        assert_eq!(annual.rows[0][6], "60.00");
        assert_eq!(annual.rows[1][0], "Total Production (Production + Adjustments):");
        assert_eq!(annual.rows[1][3], "90.00");
        assert_eq!(annual.rows[5][7], "60.00");
    }

    #[test]
    fn prod_inc_monthly_insurance_columns_are_gated() {
        let (patients, providers, procedures, adjustments, splits, claim_procs) = prod_inc_fixture();
        let from = day(2026, 11, 1);
        let to = day(2026, 11, 30);
        let monthly = prod_inc_report(
            &patients, &providers, &procedures, &adjustments, &splits, &claim_procs,
            &[], &[], from, to, true,
            ProdIncReportOptions { mode: ProdIncMode::Monthly, ..Default::default() },
        );
        assert_eq!(monthly.headers.len(), 12);
        assert_eq!(monthly.headers[5], "Write-off");
        assert_eq!(monthly.headers[8], "Unearned Pt Income");
        assert_eq!(monthly.headers[9], "Ins Income");
        assert_eq!(monthly.rows[0][5], "-20.00");
        assert_eq!(monthly.rows[0][6], "80.00", "100 production − 20 write-off");
        assert_eq!(monthly.rows[0][9], "30.00");
        assert_eq!(monthly.rows[0][10], "30.00");
        assert_eq!(monthly.rows[3][6], "70.00", "100 − 10 − 20");
        assert_eq!(monthly.rows[6][10], "90.00", "60 patient + 30 insurance");

        let annual = prod_inc_report(
            &patients, &providers, &procedures, &adjustments, &splits, &claim_procs,
            &[], &[], from, to, true,
            ProdIncReportOptions { mode: ProdIncMode::Annual, ..Default::default() },
        );
        assert_eq!(annual.headers.len(), 10);
        assert_eq!(annual.headers[3], "Write-off");
        assert_eq!(annual.headers[6], "Unearned Pt Income");
        assert_eq!(annual.rows[0][3], "-20.00");
        assert_eq!(annual.rows[0][4], "70.00");
        assert_eq!(annual.rows[0][8], "90.00");
    }

    #[test]
    fn prod_inc_excludes_hidden_and_disabled_unearned_splits() {
        let patients = vec![patient(1, "Alvarez")];
        let providers = vec![provider(7, "SM")];
        let procedures = vec![Procedure {
            proc_num: 1,
            pat_num: 1,
            proc_date: day(2026, 11, 10),
            proc_status: ProcedureStatus::Complete,
            fee: 100.0,
            provider_num: 7,
            ..Default::default()
        }];
        let splits = vec![
            PaySplit {
                pat_num: 1,
                split_amount: 60.0,
                date_pay: day(2026, 11, 12),
                provider_num: 7,
                ..Default::default()
            }, // ordinary Pt Income
            PaySplit {
                pat_num: 1,
                split_amount: 40.0,
                date_pay: day(2026, 11, 13),
                provider_num: 7,
                unearned_type: 5,
                pay_plan_charge_num: 0,
                pay_plan_debit_type: Default::default(),
                ..Default::default()
            }, // visible unearned type
            PaySplit {
                pat_num: 1,
                split_amount: 30.0,
                date_pay: day(2026, 11, 14),
                provider_num: 7,
                unearned_type: 6,
                pay_plan_charge_num: 0,
                pay_plan_debit_type: Default::default(),
                ..Default::default()
            }, // hidden unearned type
        ];
        let definitions = vec![
            Definition {
                def_num: 5,
                category: DefinitionCategory::PAY_SPLIT_UNEARNED_TYPE,
                item_value: String::new(),
                ..Default::default()
            },
            Definition {
                def_num: 6,
                category: DefinitionCategory::PAY_SPLIT_UNEARNED_TYPE,
                item_value: "hidden".into(),
                ..Default::default()
            },
        ];
        let from = day(2026, 11, 1);
        let to = day(2026, 11, 30);

        let included = prod_inc_report(
            &patients, &providers, &procedures, &[], &splits, &[], &[], &definitions, from, to,
            false,
            ProdIncReportOptions {
                mode: ProdIncMode::Monthly,
                ..Default::default()
            },
        );
        assert_eq!(included.headers[7], "Unearned Pt Income");
        assert_eq!(included.rows[1][6], "60.00", "ordinary payment only in Pt Income");
        assert_eq!(included.rows[2][7], "40.00", "visible unearned in its own column");
        assert!(
            included.rows.iter().all(|r| r[0] != "2026-11-14"),
            "a hidden unearned split must not create a row"
        );
        assert_eq!(included.rows[4][6], "60.00", "Pt Income total excludes unearned");
        assert_eq!(included.rows[5][7], "40.00", "Unearned total excludes hidden type");
        assert_eq!(included.rows[7][9], "100.00", "Total Pt Income = 60 + 40");

        let excluded = prod_inc_report(
            &patients, &providers, &procedures, &[], &splits, &[], &[], &definitions, from, to,
            false,
            ProdIncReportOptions {
                mode: ProdIncMode::Monthly,
                include_unearned: false,
                ..Default::default()
            },
        );
        assert!(
            !excluded.headers.iter().any(|h| h == "Unearned Pt Income"),
            "the unearned column disappears when Include Unearned is off"
        );
        assert_eq!(excluded.rows[1][6], "60.00");
        assert!(
            excluded.rows.iter().all(|r| r[0] != "2026-11-13" && r[0] != "2026-11-14"),
            "all unearned splits are excluded when Include Unearned is off"
        );
        assert_eq!(excluded.rows.last().unwrap()[8], "60.00");

        // Daily detail folds included unearned into the single Pt Income
        // column, matching upstream's `UnearnedIncome + PayAmt` line.
        let daily = prod_inc_report(
            &patients, &providers, &procedures, &[], &splits, &[], &[], &definitions, from, to,
            false, ProdIncReportOptions::default(),
        );
        let daily_pt = daily.rows[1..]
            .iter()
            .filter(|r| r[6] != "0.00" && r[6] != String::new())
            .count();
        assert!(daily_pt >= 2, "both the normal and visible unearned split appear");
        assert_eq!(daily.rows.last().unwrap()[6], "100.00", "60 + 40 folded into Pt Income");
    }

    #[test]
    fn custom_aging_report_ages_insurance_transaction_types() {
        let patients = [Patient {
            zip: "1000".into(),
            ..patient(1, "Alvarez")
        }];
        let procedures = [Procedure {
            proc_num: 10,
            pat_num: 1,
            proc_date: day(2026, 10, 20), // 31–60 days from 2026-11-21
            proc_status: ProcedureStatus::Complete,
            fee: 100.0,
            ..Default::default()
        }];
        let claim_procs = [
            ClaimProc {
                claim_proc_num: 100,
                proc_num: 10,
                status: 1, // Received
                ins_pay_amt: 50.0,
                write_off: 20.0,
                date_supp_received: Some(day(2026, 11, 15)), // 0–30 days
                ..Default::default()
            },
            ClaimProc {
                claim_proc_num: 101,
                proc_num: 10,
                status: 0, // NotReceived
                ins_pay_est: 30.0,
                write_off: 10.0,
                ..Default::default()
            },
        ];
        let build = |include: CustomAgingInclude| {
            custom_aging_report(
                &patients,
                &procedures,
                &[],
                &[],
                &[],
                &claim_procs,
                &[],
                day(2026, 11, 21),
                true,
                CustomAgingReportOptions {
                    include,
                    age_credits: true, // keep each credit in its own bucket
                    negative: CustomAgingNegative::Include,
                    ..Default::default()
                },
            )
        };

        let none = CustomAgingInclude {
            procedure_fees: false,
            adjustments: false,
            pay_plan_production: false,
            patient_payments: false,
            ..Default::default()
        };
        // All four insurance transactions on together. InsPay is dated by
        // DateSuppReceived (0–30 days); the other three fall back to ProcDate
        // (31–60 days) because this port has no claimproc.DateCP.
        let table = build(CustomAgingInclude {
            insurance_payments: true,
            insurance_estimates: true,
            write_offs: true,
            write_off_estimates: true,
            ..none
        });
        assert_eq!(table.headers, [
            "Patient", "0-30 Days", "31-60 Days", "61-90 Days", "> 90 Days", "Total"
        ]);
        assert_eq!(table.rows[0][0], "Alvarez, Ada");
        assert_eq!(table.rows[0][1], "-50.00", "InsPay -50 at 0–30 days");
        assert_eq!(table.rows[0][2], "-60.00", "InsEst -30 + WriteOff -20 + WriteoffEst -10 at 31–60 days");
        assert_eq!(table.rows[0][5], "-110.00");

        // With the feature toggle off, insurance types contribute nothing even
        // if their checkboxes are on, so no row survives.
        let gated = custom_aging_report(
            &patients, &procedures, &[], &[], &[], &claim_procs, &[], day(2026, 11, 21), false,
            CustomAgingReportOptions {
                include: CustomAgingInclude {
                    insurance_payments: true,
                    insurance_estimates: true,
                    write_offs: true,
                    write_off_estimates: true,
                    ..none
                },
                age_credits: true,
                ..Default::default()
            },
        );
        assert_eq!(gated.rows.len(), 1, "only the Totals row remains");
    }

    #[test]
    fn custom_aging_report_writeoff_date_mode_uses_claim_snapshot() {
        let patients = [Patient {
            zip: "1000".into(),
            ..patient(1, "Alvarez")
        }];
        let procedures = [Procedure {
            proc_num: 10,
            pat_num: 1,
            proc_date: day(2026, 10, 20), // 31–60 days
            proc_status: ProcedureStatus::Complete,
            fee: 100.0,
            ..Default::default()
        }];
        let claim_procs = [ClaimProc {
            claim_proc_num: 100,
            proc_num: 10,
            status: 1,
            write_off: 20.0,
            date_supp_received: Some(day(2026, 11, 15)), // 0–30 days
            ..Default::default()
        }];
        let include = CustomAgingInclude {
            procedure_fees: false,
            adjustments: false,
            pay_plan_production: false,
            patient_payments: false,
            write_offs: true,
            ..Default::default()
        };
        let build = |snapshots: &[ClaimSnapshot], date: CustomAgingWriteoffDate| {
            custom_aging_report(
                &patients, &procedures, &[], &[], &[], &claim_procs, snapshots,
                day(2026, 11, 21), true,
                CustomAgingReportOptions {
                    include,
                    age_credits: true,
                    negative: CustomAgingNegative::Include,
                    writeoff_date: date,
                    ..Default::default()
                },
            )
        };

        // ProcedureDate: the whole write-off ages on the procedure date.
        let proc_date = build(&[], CustomAgingWriteoffDate::ProcedureDate);
        assert_eq!(proc_date.rows[0][1], "0.00");
        assert_eq!(proc_date.rows[0][2], "-20.00");

        // ClaimPayDate without a snapshot: live write-off on ProcDate.
        let no_snapshot = build(&[], CustomAgingWriteoffDate::ClaimPayDate);
        assert_eq!(no_snapshot.rows[0][1], "0.00");
        assert_eq!(no_snapshot.rows[0][2], "-20.00");

        // With a snapshot of 15: -15 ages on ProcDate and the -5 delta ages
        // on DateSuppReceived.
        let snapshots = [ClaimSnapshot {
            claim_proc_num: 100,
            proc_num: 10,
            write_off: 15.0,
            ..Default::default()
        }];
        let with_snapshot = build(&snapshots, CustomAgingWriteoffDate::ClaimPayDate);
        assert_eq!(with_snapshot.rows[0][1], "-5.00", "delta on payment date");
        assert_eq!(with_snapshot.rows[0][2], "-15.00", "snapshot write-off on proc date");
        assert_eq!(with_snapshot.rows[0][5], "-20.00");

        // A snapshot write-off of -1 means "no stored estimate" (nulls to 0):
        // the whole -20 delta then ages on the payment date.
        let null_snapshot = [ClaimSnapshot {
            claim_proc_num: 100,
            proc_num: 10,
            write_off: -1.0,
            ..Default::default()
        }];
        let nulled = build(&null_snapshot, CustomAgingWriteoffDate::ClaimPayDate);
        assert_eq!(nulled.rows[0][1], "-20.00");
        assert_eq!(nulled.rows[0][2], "0.00");
    }

    #[test]
    fn custom_aging_report_family_grouping_and_filters() {
        let patients = vec![
            Patient {
                pat_num: 1,
                f_name: "Ada".into(),
                guarantor: 1,
                billing_type: 10,
                zip: "1000".into(),
                ..patient(1, "Alvarez")
            },
            Patient {
                pat_num: 2,
                f_name: "Ben".into(),
                guarantor: 1,
                billing_type: 10,
                zip: "1000".into(),
                ..patient(2, "Alvarez")
            },
            Patient {
                pat_num: 3,
                f_name: "Cara".into(),
                guarantor: 3,
                billing_type: 20,
                pat_status: 2, // inactive
                zip: "1000".into(),
                ..patient(3, "Bennett")
            },
            Patient {
                pat_num: 4,
                f_name: "Dan".into(),
                guarantor: 4,
                billing_type: 20,
                pat_status: 3, // archived
                zip: "1000".into(),
                ..patient(4, "Chen")
            },
        ];
        let procedures: Vec<Procedure> = [1, 2, 3, 4]
            .into_iter()
            .map(|pat_num| Procedure {
                pat_num,
                proc_date: day(2026, 11, 10),
                proc_status: ProcedureStatus::Complete,
                fee: match pat_num {
                    1 => 100.0,
                    2 => 50.0,
                    3 => 200.0,
                    _ => 300.0,
                },
                unit_qty: 1.0,
                ..Default::default()
            })
            .collect();
        let build = |options: CustomAgingReportOptions| {
            custom_aging_report(
                &patients, &procedures, &[], &[], &[], &[], &[], day(2026, 11, 21), false, options,
            )
        };

        let individual = build(CustomAgingReportOptions::default());
        assert_eq!(individual.rows.len(), 5, "four patients plus Totals");
        assert_eq!(individual.rows[0][0], "Alvarez, Ada");
        assert_eq!(individual.rows[0][5], "100.00");

        let family = build(CustomAgingReportOptions {
            family_grouping: AgingFamilyGrouping::Family,
            ..Default::default()
        });
        assert_eq!(family.rows.len(), 4, "two family members merge into one row");
        assert_eq!(family.rows[0][0], "Alvarez, Ada", "guarantor represents the family");
        assert_eq!(family.rows[0][5], "150.00", "100 + 50 from both members");

        let billed = build(CustomAgingReportOptions {
            family_grouping: AgingFamilyGrouping::Family,
            bill_type_nums: vec![10],
            ..Default::default()
        });
        assert_eq!(billed.rows.len(), 2, "only billing type 10's family + Totals");
        assert_eq!(billed.rows[0][5], "150.00");

        let active = build(CustomAgingReportOptions {
            family_grouping: AgingFamilyGrouping::Family,
            exclude_inactive: true,
            ..Default::default()
        });
        assert_eq!(active.rows.len(), 3, "inactive Bennett family dropped");
        assert!(active.rows.iter().all(|r| r[0] != "Bennett, Cara"));

        let not_archived = build(CustomAgingReportOptions {
            family_grouping: AgingFamilyGrouping::Family,
            exclude_archive: true,
            ..Default::default()
        });
        assert_eq!(not_archived.rows.len(), 3, "archived Chen dropped");
        assert!(not_archived.rows.iter().all(|r| r[0] != "Chen, Dan"));
    }

    fn pay_sheet_fixture() -> (
        Vec<Patient>,
        Vec<Provider>,
        Vec<Payment>,
        Vec<PaySplit>,
        Vec<Definition>,
    ) {
        let patients = vec![patient(1, "Alvarez"), patient(2, "Bennett")];
        let providers = vec![
            Provider {
                provider_num: 1,
                abbreviation: "AH".into(),
                l_name: "Harrison".into(),
                f_name: "Ada".into(),
                ..Default::default()
            },
            Provider {
                provider_num: 2,
                abbreviation: "BK".into(),
                l_name: "Khan".into(),
                f_name: "Bea".into(),
                ..Default::default()
            },
        ];
        let payments = vec![
            Payment {
                pay_num: 100,
                pat_num: 1,
                pay_date: day(2026, 11, 5),
                pay_type: 10,
                check_num: "CHK1".into(),
                ..Default::default()
            },
            Payment {
                pay_num: 101,
                pat_num: 2,
                pay_date: day(2026, 11, 3),
                pay_type: 11,
                ..Default::default()
            },
            Payment {
                pay_num: 102,
                pat_num: 2,
                pay_date: day(2026, 11, 20),
                pay_type: 10,
                check_num: "LATE".into(),
                ..Default::default()
            },
        ];
        let splits = vec![
            PaySplit {
                split_num: 1,
                pay_num: 100,
                pat_num: 1,
                split_amount: 60.0,
                date_pay: day(2026, 11, 5),
                provider_num: 1,
                ..Default::default()
            },
            PaySplit {
                split_num: 2,
                pay_num: 100,
                pat_num: 1,
                split_amount: 40.0,
                date_pay: day(2026, 11, 5),
                provider_num: 2,
                ..Default::default()
            },
            PaySplit {
                split_num: 3,
                pay_num: 101,
                pat_num: 2,
                split_amount: 90.0,
                date_pay: day(2026, 11, 3),
                provider_num: 1,
                ..Default::default()
            },
            PaySplit {
                split_num: 4,
                pay_num: 102,
                pat_num: 2,
                split_amount: 999.0,
                date_pay: day(2026, 11, 20),
                provider_num: 1,
                ..Default::default()
            },
        ];
        let definitions = vec![
            Definition {
                def_num: 10,
                category: DefinitionCategory::PAYMENT_TYPES,
                item_name: "Cash".into(),
                ..Default::default()
            },
            Definition {
                def_num: 11,
                category: DefinitionCategory::PAYMENT_TYPES,
                item_name: "Card".into(),
                is_hidden: true,
                ..Default::default()
            },
        ];
        (patients, providers, payments, splits, definitions)
    }

    #[test]
    fn pay_sheet_report_groups_by_type_and_split_provider() {
        let (patients, providers, payments, splits, definitions) = pay_sheet_fixture();
        let table = pay_sheet_report(
            &patients,
            &providers,
            &payments,
            &splits,
            &definitions,
            day(2026, 11, 1),
            day(2026, 11, 15),
            PaySheetReportOptions::default(),
        );
        assert_eq!(table.headers, ["Date", "Paying Patient", "Provider", "Check#", "Amount"]);
        assert_eq!(table.rows.len(), 6, "2 headings + 3 detail rows + total");
        assert_eq!(table.rows[0][0], "Cash — Patient Payments");
        assert_eq!(table.rows[1][0], "2026-11-05");
        assert_eq!(table.rows[1][1], "Alvarez, Ada");
        assert_eq!(table.rows[1][2], "AH");
        assert_eq!(table.rows[1][3], "CHK1");
        assert_eq!(table.rows[1][4], "60.00");
        assert_eq!(table.rows[2][2], "BK", "second split carries its own provider");
        assert_eq!(table.rows[2][4], "40.00");
        assert_eq!(table.rows[3][0], "Card (hidden) — Patient Payments");
        assert_eq!(table.rows[4][4], "90.00");
        assert_eq!(table.rows[5][0], "Total Patient Payments:");
        assert_eq!(table.rows[5][4], "190.00", "late payment is excluded");
        assert!(table.rows.iter().all(|row| row.len() == 5));
    }

    #[test]
    fn pay_sheet_report_honours_provider_type_and_patient_filters() {
        let (patients, providers, payments, splits, definitions) = pay_sheet_fixture();
        let table = pay_sheet_report(
            &patients,
            &providers,
            &payments,
            &splits,
            &definitions,
            day(2026, 11, 1),
            day(2026, 11, 30),
            PaySheetReportOptions {
                all_providers: false,
                provider_num: 2,
                group_by: PaySheetGroupBy::Patient,
                show_splits_by_provider: false,
                all_payment_types: false,
                payment_type_num: 10,
            },
        );
        assert_eq!(table.rows.len(), 3, "one heading + one row + total");
        assert_eq!(table.rows[0][0], "Cash — Patient Payments");
        assert_eq!(table.rows[1][2], "BK");
        assert_eq!(table.rows[1][4], "40.00", "only provider 2's split");
        assert_eq!(table.rows[2][4], "40.00");
    }

    #[test]
    fn pay_sheet_report_combines_providers_and_resolves_missing_types() {
        let (patients, providers, mut payments, splits, definitions) = pay_sheet_fixture();
        payments[0].pay_type = 99;
        let table = pay_sheet_report(
            &patients,
            &providers,
            &payments,
            &splits,
            &definitions,
            day(2026, 11, 1),
            day(2026, 11, 15),
            PaySheetReportOptions {
                show_splits_by_provider: false,
                ..Default::default()
            },
        );
        let undefined = table
            .rows
            .iter()
            .position(|row| row[0] == "Undefined — Patient Payments")
            .expect("missing definition type is reported as Undefined");
        assert_eq!(table.rows[undefined + 1][2], "AH, BK", "providers joined when split rows are off");
        assert_eq!(table.rows[undefined + 1][4], "100.00", "both splits of the payment combined");
        assert_eq!(table.rows.last().unwrap()[4], "190.00");
    }

    fn adj_def(num: Id, name: &str, hidden: bool) -> Definition {
        Definition {
            def_num: num,
            category: DefinitionCategory::ADJ_TYPES,
            item_name: name.into(),
            is_hidden: hidden,
            ..Default::default()
        }
    }

    fn adj(
        num: Id,
        pat: Id,
        date: NaiveDate,
        amount: f64,
        prov: Id,
        type_num: Id,
        note: &str,
    ) -> Adjustment {
        Adjustment {
            adj_num: num,
            adj_date: date,
            adj_amount: amount,
            pat_num: pat,
            adj_type: type_num,
            provider_num: prov,
            note: note.into(),
            ..Default::default()
        }
    }

    fn adj_sheet_fixture() -> (Vec<Patient>, Vec<Provider>, Vec<Adjustment>, Vec<Definition>) {
        let patients = [patient(1, "Alvarez"), patient(2, "Bennett")];
        let providers = [
            Provider {
                provider_num: 1,
                abbreviation: "AH".into(),
                ..Default::default()
            },
            Provider {
                provider_num: 2,
                abbreviation: "BK".into(),
                ..Default::default()
            },
        ];
        let adjustments = [
            adj(1, 1, day(2026, 11, 3), 25.0, 1, 10, "late fee"),
            adj(2, 2, day(2026, 11, 1), -12.5, 2, 11, "courtesy"),
            adj(3, 1, day(2026, 11, 20), 5.0, 1, 10, "outside range"),
        ];
        let definitions = [
            adj_def(10, "Adjustment", false),
            adj_def(11, "Discount", true),
        ];
        (patients.to_vec(), providers.to_vec(), adjustments.to_vec(), definitions.to_vec())
    }

    #[test]
    fn adj_sheet_report_ports_columns_order_and_verbatim_amounts() {
        let (patients, providers, adjustments, definitions) = adj_sheet_fixture();
        let table = adj_sheet_report(
            &patients,
            &providers,
            &adjustments,
            &definitions,
            day(2026, 11, 1),
            day(2026, 11, 15),
            AdjustmentSheetReportOptions::default(),
        );
        assert_eq!(
            table.headers,
            ["Date", "Patient Name", "Prov", "AdjustmentType", "Note", "Amount"]
        );
        assert_eq!(table.rows.len(), 2, "hidden type included, late adjustment excluded");
        // ORDER BY AdjDate: the 11-01 courtesy adjustment comes first.
        assert_eq!(table.rows[0][0], "2026-11-01");
        assert_eq!(table.rows[0][1], "Bennett, Ada");
        assert_eq!(table.rows[0][2], "BK");
        assert_eq!(table.rows[0][3], "Discount", "hidden defs keep the plain ItemName");
        assert_eq!(table.rows[0][4], "courtesy");
        assert_eq!(table.rows[0][5], "-12.50", "signed AdjAmt printed verbatim");
        assert_eq!(table.rows[1][0], "2026-11-03");
        assert_eq!(table.rows[1][5], "25.00");
        assert!(
            table.rows.iter().all(|row| row.len() == 6),
            "flat rows only, no total/footer"
        );
        assert!(table.rows.iter().all(|row| !row[0].contains("Total")));
    }

    #[test]
    fn adj_sheet_report_honours_provider_and_type_filters() {
        let (patients, providers, adjustments, definitions) = adj_sheet_fixture();
        let table = adj_sheet_report(
            &patients,
            &providers,
            &adjustments,
            &definitions,
            day(2026, 11, 1),
            day(2026, 11, 30),
            AdjustmentSheetReportOptions {
                all_providers: false,
                provider_num: 2,
                all_types: false,
                type_num: 11,
            },
        );
        assert_eq!(table.rows.len(), 1, "only prov 2 + hidden type 11");
        assert_eq!(table.rows[0][4], "courtesy");
        assert_eq!(table.rows[0][5], "-12.50");
    }

    #[test]
    fn adj_sheet_report_inner_joins_patient_type_and_left_joins_provider() {
        let (patients, providers, _, definitions) = adj_sheet_fixture();
        let adjustments = [
            adj(1, 1, day(2026, 11, 2), 10.0, 99, 10, "no provider"),
            adj(2, 99, day(2026, 11, 3), 10.0, 1, 10, "no patient"),
            adj(3, 1, day(2026, 11, 4), 10.0, 1, 99, "no type"),
        ];
        let table = adj_sheet_report(
            &patients,
            &providers,
            &adjustments,
            &definitions,
            day(2026, 11, 1),
            day(2026, 11, 30),
            AdjustmentSheetReportOptions::default(),
        );
        assert_eq!(table.rows.len(), 1, "unresolved patient/type dropped by inner join");
        assert_eq!(table.rows[0][2], "", "unresolved provider prints a blank Abbr");
        assert_eq!(table.rows[0][4], "no provider");
    }

    #[test]
    fn finance_charge_report_ports_columns_sort_and_verbatim_amounts() {
        let patients = [
            Patient {
                pat_num: 1,
                l_name: "Alvarez".into(),
                f_name: "Ada".into(),
                middle_i: "M".into(),
                primary_provider_num: 1,
                ..Default::default()
            },
            Patient {
                pat_num: 2,
                l_name: "Bennett".into(),
                f_name: "Bo".into(),
                primary_provider_num: 2,
                ..Default::default()
            },
        ];
        let adjustments = [
            adj(1, 1, day(2026, 11, 1), 20.0, 99, 10, "small"),
            adj(2, 1, day(2026, 11, 2), 35.5, 99, 10, "large"),
            adj(3, 2, day(2026, 11, 3), -5.0, 99, 11, "other type"),
            adj(4, 2, day(2026, 12, 1), 99.0, 99, 10, "out of range"),
        ];
        let table = finance_charge_report(
            &patients,
            &adjustments,
            day(2026, 11, 1),
            day(2026, 11, 30),
            FinanceChargeReportOptions::default(),
        );
        assert_eq!(table.headers, ["PatNum", "Patient Name", "Preferred Name", "Amount"]);
        assert_eq!(table.rows.len(), 3);
        // ORDER BY LName, FName, AdjAmt DESC: Alvarez's 35.50 before 20.00.
        assert_eq!(table.rows[0][0], "1");
        assert_eq!(table.rows[0][1], "Alvarez, Ada M");
        assert_eq!(table.rows[0][2], "", "Preferred is not stored by this port");
        assert_eq!(table.rows[0][3], "35.50");
        assert_eq!(table.rows[1][3], "20.00");
        assert_eq!(table.rows[2][0], "2", "Bennett follows Alvarez");
        assert_eq!(table.rows[2][3], "-5.00");
    }

    #[test]
    fn finance_charge_report_honours_type_and_primary_provider_filters() {
        let patients = [
            Patient {
                pat_num: 1,
                l_name: "Alvarez".into(),
                primary_provider_num: 1,
                ..Default::default()
            },
            Patient {
                pat_num: 2,
                l_name: "Bennett".into(),
                primary_provider_num: 2,
                ..Default::default()
            },
        ];
        let adjustments = [
            adj(1, 1, day(2026, 11, 1), 20.0, 2, 10, "alvarez type 10"),
            adj(2, 2, day(2026, 11, 2), 30.0, 2, 10, "bennett type 10"),
            adj(3, 2, day(2026, 11, 3), 40.0, 2, 11, "bennett type 11"),
        ];
        let by_type = finance_charge_report(
            &patients,
            &adjustments,
            day(2026, 11, 1),
            day(2026, 11, 30),
            FinanceChargeReportOptions {
                adjustment_type_num: 11,
                ..Default::default()
            },
        );
        assert_eq!(by_type.rows.len(), 1);
        assert_eq!(by_type.rows[0][0], "2");
        assert_eq!(by_type.rows[0][3], "40.00");

        // The provider filter is on patient.PriProv, not adjustment.ProvNum:
        // both adjustments have ProvNum 2 but only Bennett has PriProv 2.
        let by_provider = finance_charge_report(
            &patients,
            &adjustments,
            day(2026, 11, 1),
            day(2026, 11, 30),
            FinanceChargeReportOptions {
                all_providers: false,
                provider_num: 2,
                adjustment_type_num: 10,
            },
        );
        assert_eq!(by_provider.rows.len(), 1);
        assert_eq!(by_provider.rows[0][0], "2", "patient primary provider wins");
        assert_eq!(by_provider.rows[0][3], "30.00");
    }

    fn broken_patients() -> Vec<Patient> {
        vec![
            Patient {
                pat_num: 1,
                l_name: "Alvarez".into(),
                f_name: "Ada".into(),
                ..Default::default()
            },
            Patient {
                pat_num: 2,
                l_name: "Bennett".into(),
                f_name: "Bo".into(),
                ..Default::default()
            },
        ]
    }

    fn broken_providers() -> Vec<Provider> {
        vec![provider(1, "DR"), provider(2, "DH")]
    }

    #[test]
    fn broken_appointments_by_status_filters_broken_and_sums_count() {
        let apts = [
            Appointment {
                apt_num: 1,
                pat_num: 1,
                apt_datetime: day(2026, 11, 5).and_hms_opt(10, 30, 0).unwrap(),
                apt_status: AppointmentStatus::Broken,
                provider_num: 1,
                ..Default::default()
            },
            Appointment {
                apt_num: 2,
                pat_num: 2,
                apt_datetime: day(2026, 11, 6).and_hms_opt(9, 0, 0).unwrap(),
                apt_status: AppointmentStatus::Scheduled,
                provider_num: 2,
                ..Default::default()
            },
            Appointment {
                apt_num: 3,
                pat_num: 2,
                apt_datetime: day(2026, 12, 1).and_hms_opt(9, 0, 0).unwrap(),
                apt_status: AppointmentStatus::Broken,
                provider_num: 2,
                ..Default::default()
            },
        ];
        let table = broken_appointments_report(
            &broken_patients(),
            &broken_providers(),
            &apts,
            &[],
            &[],
            &[],
            day(2026, 11, 1),
            day(2026, 11, 30),
            BrokenAppointmentReportOptions::default(),
        );
        assert_eq!(table.headers, ["AptDate", "Patient", "Doctor", "Hygienist", "IsHyg"]);
        assert_eq!(table.rows.len(), 2, "one detail row plus the count footer");
        assert_eq!(table.rows[0][0], "11/05/2026 10:30:00");
        assert_eq!(table.rows[0][1], "Alvarez, Ada");
        assert_eq!(table.rows[0][2], "DR");
        assert_eq!(table.rows[0][3], "", "Hygienist is not stored");
        assert_eq!(table.rows[0][4], "False", "IsHyg is not stored");
        assert_eq!(table.rows[1][0], "Total Broken Appointments:");
        assert_eq!(table.rows[1][4], "1", "count footer in the IsHyg column");
    }

    #[test]
    fn broken_appointments_by_adjustments_filters_type_and_provider_and_sums() {
        let adjustments = [
            adj(1, 1, day(2026, 11, 2), 25.0, 1, 10, "no-show fee"),
            adj(2, 2, day(2026, 11, 3), 15.0, 2, 11, "other type"),
            adj(3, 1, day(2026, 11, 4), 10.0, 99, 10, "unresolved provider skipped"),
            adj(4, 1, day(2026, 12, 1), 99.0, 1, 10, "out of range"),
        ];
        let table = broken_appointments_report(
            &broken_patients(),
            &broken_providers(),
            &[],
            &adjustments,
            &[],
            &[],
            day(2026, 11, 1),
            day(2026, 11, 30),
            BrokenAppointmentReportOptions {
                mode: BrokenAppointmentMode::ByAdjustments,
                adjustment_type_num: 10,
                ..Default::default()
            },
        );
        assert_eq!(table.headers, ["Date", "Provider", "Patient", "Amount", "Note"]);
        assert_eq!(table.rows.len(), 3, "one detail + two summary rows");
        assert_eq!(table.rows[0][1], "DR");
        assert_eq!(table.rows[0][3], "25.00");
        assert_eq!(table.rows[0][4], "no-show fee");
        assert_eq!(table.rows[1][0], "Total Broken Appointment Adjustment Amount:");
        assert_eq!(table.rows[1][3], "25.00", "sum of the remaining type-10 rows");
        assert_eq!(table.rows[2][0], "Total Broken Appointments:");
        assert_eq!(table.rows[2][3], "1");
    }

    #[test]
    fn broken_appointments_by_procedures_resolves_codes_and_adds_code_column_when_both() {
        let patients = broken_patients();
        let providers = broken_providers();
        let codes = [
            ProcedureCode { code_num: 1, code: "D9986".into(), ..Default::default() },
            ProcedureCode { code_num: 2, code: "D9987".into(), ..Default::default() },
        ];
        let procs = [
            Procedure { proc_num: 1, pat_num: 1, code_num: 1, proc_date: day(2026, 11, 2), proc_status: ProcedureStatus::Complete, provider_num: 1, fee: 40.0, ..Default::default() },
            Procedure { proc_num: 2, pat_num: 2, code_num: 2, proc_date: day(2026, 11, 3), proc_status: ProcedureStatus::Complete, provider_num: 2, fee: 50.0, ..Default::default() },
            Procedure { proc_num: 3, pat_num: 2, code_num: 2, proc_date: day(2026, 11, 4), proc_status: ProcedureStatus::TreatmentPlanned, provider_num: 2, fee: 60.0, ..Default::default() },
        ];
        let both = broken_appointments_report(
            &patients,
            &providers,
            &[],
            &[],
            &procs,
            &codes,
            day(2026, 11, 1),
            day(2026, 11, 30),
            BrokenAppointmentReportOptions {
                mode: BrokenAppointmentMode::ByProcedures,
                procedure_option: BrokenProcedureOption::Both,
                ..Default::default()
            },
        );
        assert_eq!(both.headers, ["Date", "Provider", "Code", "Patient", "Fee"]);
        assert_eq!(both.rows.len(), 4, "two details + two summary rows; TP dropped");
        assert_eq!(both.rows[0][2], "D9986");
        assert_eq!(both.rows[0][4], "40.00");
        assert_eq!(both.rows[1][2], "D9987");
        assert_eq!(both.rows[2][0], "Total Broken Appointment Fees:");
        assert_eq!(both.rows[2][4], "90.00");
        assert_eq!(both.rows[3][0], "Total Broken Appointments:");
        assert_eq!(both.rows[3][4], "2");

        let missed_only = broken_appointments_report(
            &patients,
            &providers,
            &[],
            &[],
            &procs,
            &codes,
            day(2026, 11, 1),
            day(2026, 11, 30),
            BrokenAppointmentReportOptions {
                mode: BrokenAppointmentMode::ByProcedures,
                ..Default::default()
            },
        );
        assert_eq!(missed_only.headers, ["Date", "Provider", "Patient", "Fee"], "no Code column");
        assert_eq!(missed_only.rows.len(), 3, "one D9986 detail + two footer rows");
        assert_eq!(missed_only.rows[0][3], "40.00");
    }

    #[test]
    fn confirmation_report_projects_selected_fields_in_upstream_order() {
        let mut patient = patient(1, "Alvarez");
        patient.middle_i = "M".into();
        patient.title = "Mr".into();
        patient.address = "12 High St".into();
        patient.city = "Addis Ababa".into();
        patient.state = "AA".into();
        patient.zip = "1000".into();
        patient.home_phone = "011-234".into();
        patient.wireless_phone = "091-999".into();
        patient.email = "ada@example.test".into();
        patient.birth_date = Some(day(1985, 4, 12));
        patient.gender = Gender::Female;
        patient.primary_provider_num = 1;
        let apt = Appointment {
            pat_num: 1,
            apt_datetime: day(2026, 11, 15).and_hms_opt(9, 45, 0).unwrap(),
            ..Default::default()
        };

        let standard = confirmation_report(
            &[patient.clone()],
            &[apt.clone()],
            &ConfirmationReportOptions::default(),
        );
        assert_eq!(standard.headers.len(), 15, "all 15 standard fields");
        assert_eq!(standard.headers[0], "LName");
        assert_eq!(standard.headers[13], "Birthdate");
        assert_eq!(standard.headers[14], "AptDateTime");
        assert_eq!(standard.rows.len(), 1);
        assert_eq!(standard.rows[0][0], "Alvarez");
        assert_eq!(standard.rows[0][2], "M");
        assert_eq!(standard.rows[0][3], "", "Preferred is not stored");
        assert_eq!(standard.rows[0][4], "Mr", "Salutation uses the patient title");
        assert_eq!(standard.rows[0][5], "12 High St");
        assert_eq!(standard.rows[0][10], "011-234");
        assert_eq!(standard.rows[0][11], "", "WkPhone is not stored");
        assert_eq!(standard.rows[0][12], "091-999");
        assert_eq!(standard.rows[0][14], "11/15/2026 09:45:00");

        let custom = confirmation_report(
            &[patient],
            &[apt],
            &ConfirmationReportOptions {
                fields: vec!["PatNum".into(), "Gender".into(), "PriProv".into(), "PriCarrier".into()],
            },
        );
        assert_eq!(custom.headers, ["PatNum", "Gender", "PriProv", "PriCarrier"]);
        assert_eq!(custom.rows[0][0], "1");
        assert_eq!(custom.rows[0][1], "1", "Gender is the raw OD value (Female=1)");
        assert_eq!(custom.rows[0][2], "1", "PriProv is the raw provider FK");
        assert_eq!(custom.rows[0][3], "", "insurance fields are blank");
    }

    #[test]
    fn recall_fields_report_uses_due_dates_and_month_interval_formula() {
        let patients = [patient(1, "Alvarez"), patient(2, "Bennett")];
        let recalls = [
            Recall {
                recall_num: 1,
                pat_num: 1,
                date_due: Some(day(2026, 11, 10)),
                recall_interval: 6 * 65536, // 6 months
                recall_status: 4,
                ..Default::default()
            },
            Recall {
                recall_num: 2,
                pat_num: 2,
                date_due: Some(day(2026, 12, 1)),
                recall_interval: 65536 + 32_768, // ROUND(1.5) = 2
                ..Default::default()
            },
            Recall {
                recall_num: 3,
                pat_num: 2,
                date_due: Some(day(2027, 1, 1)),
                ..Default::default()
            },
        ];
        let table = recall_fields_report(
            &patients,
            &recalls,
            day(2026, 11, 1),
            day(2026, 11, 30),
            &RecallFieldsReportOptions {
                fields: vec![
                    "LName".into(),
                    "DateDue".into(),
                    "RecallStatus".into(),
                    "RecallInterval".into(),
                    "Preferred".into(),
                ],
            },
        );
        assert_eq!(table.headers, ["LName", "DateDue", "RecallStatus", "MonthInterval", "Preferred"]);
        assert_eq!(table.rows.len(), 1, "only the November due recall is included");
        assert_eq!(table.rows[0][0], "Alvarez");
        assert_eq!(table.rows[0][1], "2026-11-10");
        assert_eq!(table.rows[0][2], "4");
        assert_eq!(table.rows[0][3], "6");
        assert_eq!(table.rows[0][4], "");
    }

    fn proc_sheet_fixture() -> (Vec<Patient>, Vec<Provider>, Vec<Procedure>, Vec<ProcedureCode>, Vec<Definition>) {
        let patients = vec![
            Patient {
                pat_num: 1,
                l_name: "Alvarez".into(),
                f_name: "Ada".into(),
                middle_i: "M".into(),
                ..Default::default()
            },
            Patient {
                pat_num: 2,
                l_name: "Bennett".into(),
                f_name: "Bo".into(),
                ..Default::default()
            },
        ];
        let providers = vec![
            Provider {
                provider_num: 1,
                abbreviation: "DR".into(),
                ..Default::default()
            },
            Provider {
                provider_num: 2,
                abbreviation: "BS".into(),
                ..Default::default()
            },
        ];
        let procedure_codes = vec![
            ProcedureCode {
                code_num: 10,
                code: "D1110".into(),
                code_name: "Prophylaxis - adult".into(),
                category_num: 100,
                ..Default::default()
            },
            ProcedureCode {
                code_num: 11,
                code: "D2140".into(),
                code_name: "Amalgam - one surface".into(),
                category_num: 101,
                ..Default::default()
            },
            ProcedureCode {
                code_num: 12,
                code: "D9999".into(),
                code_name: "Uncategorised".into(),
                category_num: 0,
                ..Default::default()
            },
        ];
        let definitions = vec![
            Definition {
                def_num: 100,
                category: DefinitionCategory::PROC_CODE_CATS,
                item_order: 2,
                item_name: "Restorative".into(),
                ..Default::default()
            },
            Definition {
                def_num: 101,
                category: DefinitionCategory::PROC_CODE_CATS,
                item_order: 1,
                item_name: "Diagnostic".into(),
                ..Default::default()
            },
        ];
        let procedures = vec![
            Procedure {
                proc_num: 1,
                pat_num: 1,
                code_num: 10,
                proc_date: day(2026, 11, 2),
                proc_status: ProcedureStatus::Complete,
                tooth_num: "".into(),
                provider_num: 1,
                fee: 95.0,
                ..Default::default()
            },
            Procedure {
                proc_num: 2,
                pat_num: 2,
                code_num: 11,
                proc_date: day(2026, 11, 1),
                proc_status: ProcedureStatus::Complete,
                tooth_num: "30".into(),
                provider_num: 2,
                fee: 120.0,
                ..Default::default()
            },
            Procedure {
                proc_num: 3,
                pat_num: 1,
                code_num: 10,
                proc_date: day(2026, 11, 3),
                proc_status: ProcedureStatus::TreatmentPlanned,
                provider_num: 1,
                fee: 95.0,
                ..Default::default()
            },
            Procedure {
                proc_num: 4,
                pat_num: 2,
                code_num: 10,
                proc_date: day(2026, 12, 1),
                proc_status: ProcedureStatus::Complete,
                provider_num: 1,
                fee: 95.0,
                ..Default::default()
            },
        ];
        (patients, providers, procedures, procedure_codes, definitions)
    }

    #[test]
    fn procedure_sheet_individual_ports_columns_sort_and_filters() {
        let (patients, providers, procedures, codes, defs) = proc_sheet_fixture();
        let table = procedure_sheet_report(
            &patients,
            &providers,
            &procedures,
            &codes,
            &defs,
            day(2026, 11, 1),
            day(2026, 11, 30),
            &ProcedureSheetReportOptions::default(),
        );
        assert_eq!(
            table.headers,
            ["Date", "Patient Name", "Code", "Tooth", "Area", "Description", "Prov", "Fee"]
        );
        // Only completed procedures in the range: proc 1 and 2. TP (3) and
        // out-of-range (4) are excluded.
        assert_eq!(table.rows.len(), 2);
        // ORDER BY ProcDate first: Bennett's 11/01 before Alvarez's 11/02.
        assert_eq!(table.rows[0][0], "2026-11-01");
        assert_eq!(table.rows[0][1], "Bennett, Bo ");
        assert_eq!(table.rows[0][2], "D2140");
        assert_eq!(table.rows[0][3], "30");
        assert_eq!(table.rows[0][4], "", "Area (Surf) is not stored");
        assert_eq!(table.rows[0][6], "BS");
        assert_eq!(table.rows[0][7], "120.00");
        assert_eq!(table.rows[1][1], "Alvarez, Ada M");
        assert_eq!(table.rows[1][6], "DR");
        assert_eq!(table.rows[1][7], "95.00");

        // Provider filter: only provider 2.
        let by_provider = procedure_sheet_report(
            &patients,
            &providers,
            &procedures,
            &codes,
            &defs,
            day(2026, 11, 1),
            day(2026, 11, 30),
            &ProcedureSheetReportOptions {
                all_providers: false,
                provider_num: 2,
                ..Default::default()
            },
        );
        assert_eq!(by_provider.rows.len(), 1);
        assert_eq!(by_provider.rows[0][2], "D2140");

        // Code substring filter, case-insensitive.
        let by_code = procedure_sheet_report(
            &patients,
            &providers,
            &procedures,
            &codes,
            &defs,
            day(2026, 11, 1),
            day(2026, 11, 30),
            &ProcedureSheetReportOptions {
                code_filter: "d1110".into(),
                ..Default::default()
            },
        );
        assert_eq!(by_code.rows.len(), 1);
        assert_eq!(by_code.rows[0][2], "D1110");
    }

    #[test]
    fn procedure_sheet_grouped_uses_categories_quantities_and_averages() {
        let (patients, providers, procedures, codes, defs) = proc_sheet_fixture();
        // Add a second D1110 so the grouped mode has quantity 2 and an average.
        let mut procedures = procedures;
        procedures.push(Procedure {
            proc_num: 5,
            pat_num: 1,
            code_num: 10,
            proc_date: day(2026, 11, 4),
            proc_status: ProcedureStatus::Complete,
            provider_num: 1,
            fee: 105.0,
            ..Default::default()
        });
        let table = procedure_sheet_report(
            &patients,
            &providers,
            &procedures,
            &codes,
            &defs,
            day(2026, 11, 1),
            day(2026, 11, 30),
            &ProcedureSheetReportOptions {
                mode: ProcedureSheetMode::GroupedByCode,
                ..Default::default()
            },
        );
        assert_eq!(
            table.headers,
            ["Category", "Code", "Description", "Quantity", "Average Fee", "Total Fees"]
        );
        // ORDER BY category.ItemOrder: Diagnostic (1) before Restorative (2).
        // D9999 has no category definition and is dropped by the INNER JOIN.
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0][0], "Diagnostic");
        assert_eq!(table.rows[0][1], "D2140");
        assert_eq!(table.rows[0][3], "1");
        assert_eq!(table.rows[0][4], "120.00");
        assert_eq!(table.rows[0][5], "120.00");
        assert_eq!(table.rows[1][0], "Restorative");
        assert_eq!(table.rows[1][1], "D1110");
        assert_eq!(table.rows[1][3], "2");
        assert_eq!(table.rows[1][4], "100.00", "average of 95 and 105");
        assert_eq!(table.rows[1][5], "200.00");
    }

    fn proc_codes_fixture() -> (Vec<ProcedureCode>, Vec<Fee>, Vec<Definition>) {
        let codes = vec![
            ProcedureCode {
                code_num: 1,
                code: "D2140".into(),
                code_name: "Amalgam - one surface".into(),
                abbreviation: "Amalg 1s".into(),
                category_num: 101,
                ..Default::default()
            },
            ProcedureCode {
                code_num: 2,
                code: "D1110".into(),
                code_name: "Prophylaxis - adult".into(),
                abbreviation: "Adult prophy".into(),
                category_num: 100,
                ..Default::default()
            },
            ProcedureCode {
                code_num: 3,
                code: "D9999".into(),
                code_name: "No fee code".into(),
                abbreviation: "No fee".into(),
                category_num: 0,
                ..Default::default()
            },
            ProcedureCode {
                code_num: 4,
                code: "D8888".into(),
                code_name: "Explicit blank fee".into(),
                abbreviation: "Blank".into(),
                category_num: 0,
                ..Default::default()
            },
        ];
        let fees = vec![
            Fee { fee_num: 1, code_num: 1, schedule_num: 53, amount: 120.0 },
            Fee { fee_num: 2, code_num: 2, schedule_num: 53, amount: 95.5 },
            Fee { fee_num: 3, code_num: 4, schedule_num: 53, amount: -1.0 },
        ];
        let definitions = vec![
            Definition {
                def_num: 100,
                category: DefinitionCategory::PROC_CODE_CATS,
                item_name: "Preventive".into(),
                ..Default::default()
            },
            Definition {
                def_num: 101,
                category: DefinitionCategory::PROC_CODE_CATS,
                item_name: "Restorative".into(),
                ..Default::default()
            },
        ];
        (codes, fees, definitions)
    }

    #[test]
    fn procedure_codes_report_ports_code_layout_and_blank_fee_rules() {
        let (codes, fees, defs) = proc_codes_fixture();
        let table = procedure_codes_report(&codes, &fees, &defs, &ProcedureCodesReportOptions::default());
        assert_eq!(table.headers, ["Code", "Desc", "Abbr", "Fee"]);
        // Ordered by ProcCode; missing fee and OD's -1 sentinel are suppressed.
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0][0], "D1110");
        assert_eq!(table.rows[0][3], "95.50");
        assert_eq!(table.rows[1][0], "D2140");
        assert_eq!(table.rows[1][3], "120.00");

        let with_blanks = procedure_codes_report(
            &codes,
            &fees,
            &defs,
            &ProcedureCodesReportOptions {
                include_blanks: true,
                ..Default::default()
            },
        );
        assert_eq!(with_blanks.rows.len(), 4);
        // D8888 has an explicit -1 fee; D9999 has no fee row at all.
        assert_eq!(with_blanks.rows[0][0], "D1110");
        assert_eq!(with_blanks.rows[1][0], "D2140");
        assert_eq!(with_blanks.rows[2][0], "D8888");
        assert_eq!(with_blanks.rows[2][3], "", "OD's -1 sentinel is blank");
        assert_eq!(with_blanks.rows[3][0], "D9999");
        assert_eq!(with_blanks.rows[3][3], "", "missing fee is blank");
    }

    #[test]
    fn procedure_codes_report_categories_order_by_cat_fk_and_name_uncategorised() {
        let (codes, fees, defs) = proc_codes_fixture();
        let table = procedure_codes_report(
            &codes,
            &fees,
            &defs,
            &ProcedureCodesReportOptions {
                categories: true,
                include_blanks: true,
                ..Default::default()
            },
        );
        assert_eq!(table.headers, ["Category", "Code", "Desc", "Abbr", "Fee"]);
        // Upstream orders by ProcCat (definition FK) ascending, NOT ItemOrder:
        // category 0 (uncategorised) first, then 100, then 101. Within a
        // category it orders by ProcCode.
        assert_eq!(table.rows.len(), 4);
        assert_eq!(table.rows[0][0], "", "no definition for category 0");
        assert_eq!(table.rows[0][1], "D8888");
        assert_eq!(table.rows[1][1], "D9999");
        assert_eq!(table.rows[2][0], "Preventive");
        assert_eq!(table.rows[2][1], "D1110");
        assert_eq!(table.rows[3][0], "Restorative");
        assert_eq!(table.rows[3][1], "D2140");
    }

    fn prescriptions_fixture() -> (Vec<Patient>, Vec<Provider>, Vec<Prescription>) {
        let patients = vec![
            Patient {
                pat_num: 1,
                l_name: "Alvarez".into(),
                f_name: "Marta".into(),
                ..Default::default()
            },
            Patient {
                pat_num: 2,
                l_name: "Bennett".into(),
                f_name: "James".into(),
                middle_i: "Q".into(),
                ..Default::default()
            },
        ];
        let providers = vec![
            Provider {
                provider_num: 1,
                abbreviation: "AH".into(),
                ..Default::default()
            },
            Provider {
                provider_num: 2,
                abbreviation: "DN".into(),
                ..Default::default()
            },
        ];
        let prescriptions = vec![
            Prescription {
                rx_num: 1,
                pat_num: 1,
                rx_date: day(2026, 3, 3),
                drug: "Amoxicillin 500mg".into(),
                sig: "Take one capsule TID".into(),
                disp: "21".into(),
                prov_num: 1,
            },
            Prescription {
                rx_num: 2,
                pat_num: 2,
                rx_date: day(2026, 1, 10),
                drug: "Ibuprofen 400mg".into(),
                sig: "As needed for pain".into(),
                disp: "30".into(),
                prov_num: 2,
            },
            Prescription {
                rx_num: 3,
                pat_num: 1,
                rx_date: day(2026, 2, 20),
                drug: "Amoxicillin 250mg".into(),
                sig: "Take two capsules BID".into(),
                disp: "20".into(),
                prov_num: 2,
            },
            // A prescription with no matching provider is dropped (INNER JOIN).
            Prescription {
                rx_num: 4,
                pat_num: 1,
                rx_date: day(2026, 4, 1),
                drug: "Orphan".into(),
                prov_num: 99,
                ..Default::default()
            },
        ];
        (patients, providers, prescriptions)
    }

    #[test]
    fn prescriptions_report_patient_mode_orders_and_formats_like_upstream() {
        let (patients, providers, prescriptions) = prescriptions_fixture();
        let table = prescriptions_report(
            &patients,
            &providers,
            &prescriptions,
            &PrescriptionReportOptions::default(),
        );
        assert_eq!(
            table.headers,
            ["Patient Name", "Date", "Drug Name", "Directions", "Dispense", "Prov Name"]
        );
        // Order: patient LName, FName, then RxDate. The orphan row is skipped.
        assert_eq!(table.rows.len(), 3);
        assert_eq!(table.rows[0][0], "Alvarez, Marta ");
        assert_eq!(table.rows[0][1], "2026-02-20");
        assert_eq!(table.rows[0][2], "Amoxicillin 250mg");
        assert_eq!(table.rows[0][5], "DN");
        assert_eq!(table.rows[1][1], "2026-03-03");
        assert_eq!(table.rows[1][5], "AH");
        assert_eq!(table.rows[2][0], "Bennett, James Q");
        assert_eq!(table.rows[2][3], "As needed for pain");
    }

    #[test]
    fn prescriptions_report_drug_mode_filters_prefix_and_orders_by_drug() {
        let (patients, providers, prescriptions) = prescriptions_fixture();
        let table = prescriptions_report(
            &patients,
            &providers,
            &prescriptions,
            &PrescriptionReportOptions {
                search_patient: false,
                input_text: "amox".into(),
            },
        );
        // Case-insensitive LIKE 'amox%' matches only the two Amoxicillin rows.
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0][2], "Amoxicillin 250mg");
        assert_eq!(table.rows[1][2], "Amoxicillin 500mg");

        let by_patient = prescriptions_report(
            &patients,
            &providers,
            &prescriptions,
            &PrescriptionReportOptions {
                search_patient: true,
                input_text: "BEN".into(),
            },
        );
        assert_eq!(by_patient.rows.len(), 1);
        assert_eq!(by_patient.rows[0][0], "Bennett, James Q");
    }

    #[test]
    fn prescriptions_report_blanks_od_empty_date() {
        let (patients, providers, _) = prescriptions_fixture();
        let prescriptions = [Prescription {
            rx_num: 5,
            pat_num: 1,
            rx_date: day(1, 1, 1),
            drug: "No date".into(),
            prov_num: 1,
            ..Default::default()
        }];
        let table = prescriptions_report(
            &patients,
            &providers,
            &prescriptions,
            &PrescriptionReportOptions::default(),
        );
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0][1], "");
    }

    fn referral_fixture() -> (
        Vec<Procedure>,
        Vec<ProcedureCode>,
        Vec<Referral>,
        Vec<RefAttach>,
        Vec<Definition>,
    ) {
        let procedures = vec![
            Procedure {
                proc_num: 1,
                pat_num: 1,
                code_num: 1,
                proc_date: day(2026, 6, 5),
                proc_status: ProcedureStatus::Complete,
                provider_num: 1,
                fee: 100.0,
                unit_qty: 1.0,
                ..Default::default()
            },
            // 50 x (2 + 1) = 150, proving the upstream unit formula.
            Procedure {
                proc_num: 2,
                pat_num: 1,
                code_num: 2,
                proc_date: day(2026, 6, 15),
                proc_status: ProcedureStatus::Complete,
                provider_num: 2,
                fee: 50.0,
                unit_qty: 2.0,
                base_units: 1.0,
                ..Default::default()
            },
            // Old work: sums nothing in-range, but sets MIN(ProcDate) for pat 1.
            Procedure {
                proc_num: 3,
                pat_num: 1,
                code_num: 1,
                proc_date: day(2025, 1, 5),
                proc_status: ProcedureStatus::Complete,
                provider_num: 1,
                fee: 999.0,
                unit_qty: 1.0,
                ..Default::default()
            },
            Procedure {
                proc_num: 4,
                pat_num: 2,
                code_num: 1,
                proc_date: day(2026, 6, 10),
                proc_status: ProcedureStatus::Complete,
                provider_num: 1,
                fee: 70.0,
                unit_qty: 1.0,
                ..Default::default()
            },
            // D9986 is dropped by the procedurecode INNER JOIN.
            Procedure {
                proc_num: 5,
                pat_num: 2,
                code_num: 3,
                proc_date: day(2026, 6, 11),
                proc_status: ProcedureStatus::Complete,
                provider_num: 1,
                fee: 500.0,
                unit_qty: 1.0,
                ..Default::default()
            },
            // A planned procedure never contributes to production.
            Procedure {
                proc_num: 6,
                pat_num: 3,
                code_num: 1,
                proc_date: day(2026, 6, 12),
                proc_status: ProcedureStatus::TreatmentPlanned,
                provider_num: 1,
                fee: 400.0,
                unit_qty: 1.0,
                ..Default::default()
            },
        ];
        let procedure_codes = vec![
            ProcedureCode { code_num: 1, code: "D1110".into(), ..Default::default() },
            ProcedureCode { code_num: 2, code: "D2140".into(), ..Default::default() },
            ProcedureCode { code_num: 3, code: "D9986".into(), ..Default::default() },
        ];
        let referrals = vec![
            Referral {
                referral_num: 1,
                l_name: "Ada".into(),
                f_name: "Source".into(),
                title: "Dr".into(),
                address: "1 Main".into(),
                address2: "Suite 2".into(),
                city: "Addis".into(),
                state: "AA".into(),
                zip: "1000".into(),
                specialty_num: 9,
                ..Default::default()
            },
            Referral {
                referral_num: 2,
                l_name: "Bek".into(),
                f_name: "Source".into(),
                ..Default::default()
            },
            // No refattach at all: removed by the two INNER JOINs.
            Referral {
                referral_num: 3,
                l_name: "Cid".into(),
                f_name: "None".into(),
                ..Default::default()
            },
        ];
        let refattaches = vec![
            RefAttach { ref_attach_num: 1, referral_num: 1, pat_num: 1, ref_type: 1 },
            RefAttach { ref_attach_num: 2, referral_num: 1, pat_num: 2, ref_type: 1 },
            RefAttach { ref_attach_num: 3, referral_num: 2, pat_num: 1, ref_type: 1 },
            // RefTo and RefCustom are never counted.
            RefAttach { ref_attach_num: 4, referral_num: 2, pat_num: 2, ref_type: 0 },
            RefAttach { ref_attach_num: 5, referral_num: 2, pat_num: 2, ref_type: 2 },
            // RefFrom but patient 3 has only a planned procedure.
            RefAttach { ref_attach_num: 6, referral_num: 2, pat_num: 3, ref_type: 1 },
        ];
        let definitions = vec![Definition {
            def_num: 9,
            category: DefinitionCategory::PROVIDER_SPECIALTIES,
            item_name: "Endodontics".into(),
            ..Default::default()
        }];
        (
            procedures,
            procedure_codes,
            referrals,
            refattaches,
            definitions,
        )
    }

    #[test]
    fn referral_analysis_report_groups_counts_and_uses_unit_formula() {
        let (procedures, codes, referrals, attaches, definitions) = referral_fixture();
        let table = referral_analysis_report(
            &procedures,
            &codes,
            &referrals,
            &attaches,
            &definitions,
            day(2026, 6, 1),
            day(2026, 6, 30),
            &ReferralAnalysisReportOptions::default(),
        );
        assert_eq!(table.headers, ["Last Name", "First Name", "Count", "Production"]);
        assert_eq!(table.rows.len(), 2, "a referral with no RefFrom patients is dropped");
        assert_eq!(table.rows[0][0], "Ada");
        assert_eq!(table.rows[0][2], "2");
        // 100 x 1 + 50 x (2 + 1) for patient 1, plus 70 x 1 for patient 2.
        assert_eq!(table.rows[0][3], "320.00");
        assert_eq!(table.rows[1][0], "Bek");
        assert_eq!(table.rows[1][2], "1");
        assert_eq!(table.rows[1][3], "250.00");
    }

    #[test]
    fn referral_analysis_report_address_columns_resolve_specialty() {
        let (procedures, codes, referrals, attaches, definitions) = referral_fixture();
        let table = referral_analysis_report(
            &procedures,
            &codes,
            &referrals,
            &attaches,
            &definitions,
            day(2026, 6, 1),
            day(2026, 6, 30),
            &ReferralAnalysisReportOptions {
                include_address: true,
                ..Default::default()
            },
        );
        assert_eq!(
            table.headers,
            [
                "Last Name", "First Name", "Count", "Production", "Title", "Address", "Add2",
                "City", "State", "Zip", "Specialty"
            ]
        );
        assert_eq!(
            table.rows[0],
            ["Ada", "Source", "2", "320.00", "Dr", "1 Main", "Suite 2", "Addis", "AA", "1000", "Endodontics"]
        );
        assert_eq!(table.rows[1][10], "", "unknown specialty renders blank");
    }

    #[test]
    fn referral_analysis_report_only_new_patients_uses_min_proc_date() {
        let (procedures, codes, referrals, attaches, definitions) = referral_fixture();
        let table = referral_analysis_report(
            &procedures,
            &codes,
            &referrals,
            &attaches,
            &definitions,
            day(2026, 6, 1),
            day(2026, 6, 30),
            &ReferralAnalysisReportOptions {
                only_new_patients: true,
                ..Default::default()
            },
        );
        // Patient 1 first appeared in 2025, so HAVING MIN(ProcDate) removes it
        // even though it has in-range production. Patient 2 first appeared in
        // June 2026 and keeps only its in-range 70.00.
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0][0], "Ada");
        assert_eq!(table.rows[0][2], "1");
        assert_eq!(table.rows[0][3], "70.00");
    }

    #[test]
    fn referral_analysis_report_filters_by_selected_providers() {
        let (procedures, codes, referrals, attaches, definitions) = referral_fixture();
        let table = referral_analysis_report(
            &procedures,
            &codes,
            &referrals,
            &attaches,
            &definitions,
            day(2026, 6, 1),
            day(2026, 6, 30),
            &ReferralAnalysisReportOptions {
                all_providers: false,
                provider_nums: vec![2],
                ..Default::default()
            },
        );
        // Only procedure 2 survives (provider 2): 50 x (2 + 1) = 150.00.
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0][0], "Ada");
        assert_eq!(table.rows[0][3], "150.00");
        assert_eq!(table.rows[1][0], "Bek");
        assert_eq!(table.rows[1][3], "150.00");
    }

    fn referrals_fixture() -> Vec<Referral> {
        vec![
            Referral {
                referral_num: 2,
                l_name: "Tesfaye".into(),
                f_name: "Daniel".into(),
                m_name: "K".into(),
                title: "Dr".into(),
                address: "8 Bole Road".into(),
                city: "Addis Ababa".into(),
                state: "AA".into(),
                zip: "1001".into(),
                telephone: "+251 11 765 4321".into(),
                email: "daniel@example.com".into(),
                note: "Referred by a colleague".into(),
                specialty_num: 9,
                using_tin: 1,
                is_hidden: 0,
                not_person: 0,
                ..Default::default()
            },
            Referral {
                referral_num: 1,
                l_name: "Girma".into(),
                f_name: "Aster".into(),
                city: "Addis Ababa".into(),
                state: "AA".into(),
                zip: "1000".into(),
                specialty_num: 17,
                is_hidden: 1,
                not_person: 1,
                ..Default::default()
            },
        ]
    }

    #[test]
    fn referrals_report_emits_raw_selected_fields_in_referral_num_order() {
        let options = ReferralsReportOptions {
            fields: vec![
                "LName".into(),
                "FName".into(),
                "Specialty".into(),
                "IsHidden".into(),
                "ST".into(),
            ],
            ..Default::default()
        };
        let table = referrals_report(&referrals_fixture(), &options);
        assert_eq!(table.headers, ["LName", "FName", "Specialty", "IsHidden", "ST"]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0], ["Girma", "Aster", "17", "1", "AA"]);
        assert_eq!(table.rows[1], ["Tesfaye", "Daniel", "9", "0", "AA"]);
    }

    #[test]
    fn referrals_report_like_is_case_insensitive_and_supports_wildcards() {
        let options = ReferralsReportOptions {
            fields: vec!["LName".into(), "MName".into(), "Note".into()],
            filters: vec![ReferralFilter {
                field: "LName".into(),
                condition: ReportFilterCondition::Like,
                value: "tes".into(),
            }],
            ..Default::default()
        };
        let table = referrals_report(&referrals_fixture(), &options);
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0][0], "Tesfaye");
        assert_eq!(table.rows[0][1], "K", "MName is exposed even when not a default field");
        assert_eq!(table.rows[0][2], "Referred by a colleague");

        // A typed `%` stays a SQL wildcard: upstream turns "%ir%" into
        // LIKE '%%ir%%', which matches only Girma.
        let options = ReferralsReportOptions {
            fields: vec!["LName".into()],
            filters: vec![ReferralFilter {
                field: "LName".into(),
                condition: ReportFilterCondition::Like,
                value: "%ir%".into(),
            }],
            ..Default::default()
        };
        let table = referrals_report(&referrals_fixture(), &options);
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0][0], "Girma");
    }

    #[test]
    fn referrals_report_ands_filters_and_compares_numeric_fields_numerically() {
        let options = ReferralsReportOptions {
            fields: vec!["LName".into(), "Specialty".into(), "IsHidden".into()],
            filters: vec![
                ReferralFilter {
                    field: "Specialty".into(),
                    condition: ReportFilterCondition::Gte,
                    value: "10".into(),
                },
                ReferralFilter {
                    field: "IsHidden".into(),
                    condition: ReportFilterCondition::Eq,
                    value: "1".into(),
                },
            ],
            ..Default::default()
        };
        let table = referrals_report(&referrals_fixture(), &options);
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0], ["Girma", "17", "1"]);
    }

    #[test]
    fn referrals_report_string_fields_compare_lexicographically() {
        let options = ReferralsReportOptions {
            fields: vec!["LName".into(), "Zip".into()],
            filters: vec![ReferralFilter {
                field: "Zip".into(),
                condition: ReportFilterCondition::Lt,
                value: "999".into(),
            }],
            ..Default::default()
        };
        let table = referrals_report(&referrals_fixture(), &options);
        // "1000" < "999" as a string, though it is larger as a number.
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0][1], "1000");
        assert_eq!(table.rows[1][1], "1001");
    }

    #[test]
    fn referrals_report_ne_and_all_nineteen_field_labels_are_available() {
        let options = ReferralsReportOptions {
            fields: od_domain::reports::REFERRAL_REPORT_FIELDS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            filters: vec![ReferralFilter {
                field: "LName".into(),
                condition: ReportFilterCondition::Ne,
                value: "Girma".into(),
            }],
            ..Default::default()
        };
        let table = referrals_report(&referrals_fixture(), &options);
        assert_eq!(table.headers.len(), 19);
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0][0], "8 Bole Road", "Address is the first field");
        assert!(table.rows.iter().all(|row| row.len() == 19));
    }

    fn patients_fixture() -> Vec<Patient> {
        vec![
            Patient {
                pat_num: 2,
                l_name: "Bennett".into(),
                f_name: "Lily".into(),
                birth_date: Some(day(1985, 3, 4)),
                gender: Gender::Female,
                home_phone: "222".into(),
                email: "lily@example.com".into(),
                ..Default::default()
            },
            Patient {
                pat_num: 1,
                l_name: "Alvarez".into(),
                f_name: "Marta".into(),
                birth_date: Some(day(1978, 7, 12)),
                gender: Gender::Female,
                home_phone: "111".into(),
                email: "marta@example.com".into(),
                ..Default::default()
            },
        ]
    }

    fn patient_procedures_fixture() -> Vec<Procedure> {
        vec![
            Procedure {
                proc_num: 1,
                pat_num: 1,
                proc_date: day(2024, 1, 10),
                proc_status: ProcedureStatus::Complete,
                ..Default::default()
            },
            Procedure {
                proc_num: 2,
                pat_num: 1,
                proc_date: day(2025, 6, 20),
                proc_status: ProcedureStatus::Complete,
                ..Default::default()
            },
            Procedure {
                proc_num: 3,
                pat_num: 2,
                proc_date: day(2023, 5, 5),
                proc_status: ProcedureStatus::Complete,
                ..Default::default()
            },
            Procedure {
                proc_num: 4,
                pat_num: 2,
                proc_date: day(2026, 1, 1),
                proc_status: ProcedureStatus::TreatmentPlanned,
                ..Default::default()
            },
        ]
    }

    fn patient_recalls_fixture() -> Vec<Recall> {
        vec![Recall {
            pat_num: 1,
            recall_status: 17,
            ..Default::default()
        }]
    }

    #[test]
    fn patients_report_renders_selected_fields_and_upstream_derived_columns() {
        let options = PatientsReportOptions {
            fields: [
                "PatNum",
                "LName",
                "Gender",
                "Birthdate",
                "HmPhone",
                "First Complete Proc",
                "Last Complete Proc",
                "RecallStatus",
            ]
            .iter()
            .map(|field| field.to_string())
            .collect(),
            filters: Vec::new(),
        };
        let table = patients_report(
            &patients_fixture(),
            &patient_procedures_fixture(),
            &patient_recalls_fixture(),
            &options,
        );
        assert_eq!(table.headers.len(), 8);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0][1], "Alvarez", "surname sort, not input order");
        assert_eq!(table.rows[0][2], "1", "Gender is OD's numeric Female");
        assert_eq!(table.rows[0][3], "1978-07-12");
        assert_eq!(table.rows[0][5], "2024-01-10", "MIN(ProcDate) of completed");
        assert_eq!(table.rows[0][6], "2025-06-20", "MAX(ProcDate) of completed");
        assert_eq!(table.rows[0][7], "17", "recall status definition number");
        assert_eq!(table.rows[1][1], "Bennett");
        assert_eq!(table.rows[1][5], "2023-05-05");
        assert_eq!(table.rows[1][6], "2023-05-05", "planned procedure ignored");
        assert_eq!(table.rows[1][7], "", "no recall row renders blank");
    }

    #[test]
    fn patients_report_ands_filters_and_compares_numeric_fields_numerically() {
        let options = PatientsReportOptions {
            fields: vec!["LName".into(), "PatNum".into()],
            filters: vec![
                ReportFilter {
                    field: "PatNum".into(),
                    condition: ReportFilterCondition::Eq,
                    value: "02".into(),
                },
                ReportFilter {
                    field: "LName".into(),
                    condition: ReportFilterCondition::Like,
                    value: "enn".into(),
                },
            ],
        };
        let table = patients_report(
            &patients_fixture(),
            &patient_procedures_fixture(),
            &patient_recalls_fixture(),
            &options,
        );
        assert_eq!(table.rows.len(), 1, "numeric '02' matches PatNum 2");
        assert_eq!(table.rows[0][0], "Bennett");
    }

    #[test]
    fn patients_report_string_conditions_and_empty_field_selection() {
        let options = PatientsReportOptions {
            fields: PATIENT_REPORT_FIELDS
                .iter()
                .map(|field| field.to_string())
                .collect(),
            filters: vec![ReportFilter {
                field: "LName".into(),
                condition: ReportFilterCondition::Lt,
                value: "Bennett".into(),
            }],
        };
        let table = patients_report(
            &patients_fixture(),
            &patient_procedures_fixture(),
            &patient_recalls_fixture(),
            &options,
        );
        assert_eq!(table.headers.len(), PATIENT_REPORT_FIELDS.len());
        assert_eq!(table.rows.len(), 1, "string '<' is case-insensitive");
        assert_eq!(table.rows[0][1], "Alvarez");
        assert!(table.rows.iter().all(|row| row.len() == PATIENT_REPORT_FIELDS.len()));

        let empty = patients_report(
            &patients_fixture(),
            &[],
            &[],
            &PatientsReportOptions {
                fields: Vec::new(),
                filters: Vec::new(),
            },
        );
        assert!(empty.headers.is_empty());
        assert!(empty.rows.iter().all(|row| row.is_empty()));
    }
}
