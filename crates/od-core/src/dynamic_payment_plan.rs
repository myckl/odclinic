//! Dynamic payment-plan links and automatic charge issuance.
//!
//! Mirrors the 24_3 C# path `PayPlanProductionEntry.GetProductionForLinks` →
//! `GetWithAmountRemaining` → `PayPlanEdit.GetListExpectedCharges` →
//! `IssueChargesDueForDynamicPaymentPlans`. A dynamic plan's principal is not a
//! stored amount: it comes from the production (procedures and adjustments)
//! linked through `payplanlink`. Charges are issued a period at a time as those
//! periods come due, which is what stops linked production from being billed
//! twice — the plan charges *are* the schedule for that production.
//!
//! Deliberate local adaptations for the insurance-free, local-first port:
//!   * Procedure patient portion is `fee + linked adjustments - prior
//!     paysplits` (no claimprocs, sales tax or discount-plan fields are
//!     modelled, and insurance is off by default).
//!   * `PayPlanLink.SecDateTEntry` is not modelled, so entries are ordered by
//!     production date, then link type, then key — the same tie-breakers OD
//!     uses after its completed/incomplete split.
//!   * Interest accrues on principal not yet charged. Prepayments / hidden
//!     unearned transfers (`CalculatePrincipalRemaining`'s buckets and
//!     `ApplyPrepaymentsToCharges`) are out of scope.

use chrono::NaiveDate;
use od_domain::{
    Adjustment, DynamicPayPlanTPOption, Id, PayPlan, PayPlanCharge, PayPlanChargeType,
    PayPlanFrequency, PayPlanLink, Procedure, ProcedureStatus,
};

use crate::payment_plan::{next_period_date, period_rate, round2, MAX_PLAN_CHARGES};

/// The maximum number of periods the issuer will catch up in one run
/// (`payPeriodsMissable` in OD's `IssueChargesDueForDynamicPaymentPlans`).
pub const MAX_CATCH_UP_PERIODS: i64 = 100;

/// One linked production item, with the amount still waiting to be turned into
/// plan charges. Corresponds to OD's `PayPlanProductionEntry`.
#[derive(Clone, Debug)]
pub struct ProductionEntry {
    pub link_type: i8,
    pub pri_key: Id,
    pub provider_num: Id,
    pub production_date: NaiveDate,
    /// Fee/adjustment less prior paysplits, before any override.
    pub amount_original: f64,
    /// Non-zero override from `payplanlink.AmountOverride`.
    pub amount_override: f64,
    /// Amount not yet covered by an existing charge.
    pub amount_remaining: f64,
    pub is_procedure_completed: bool,
}

impl ProductionEntry {
    /// OD `PayPlanProductionEntry.GetAmountAttached()`.
    pub fn attached_amount(&self) -> f64 {
        if self.amount_override != 0.0 {
            self.amount_override
        } else {
            self.amount_original
        }
    }
}

/// Build production entries from a plan's links (OD `GetProductionForLinks`).
/// Prior paysplits already credited directly to a procedure (or adjustment)
/// reduce its amount, so money paid before the plan was created is not financed
/// again. Incomplete TP procedures are kept and flagged so callers can decide
/// whether to charge them according to the plan's TP option.
pub fn production_entries(
    links: &[PayPlanLink],
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[od_domain::PaySplit],
) -> Vec<ProductionEntry> {
    let mut entries = Vec::new();
    for link in links {
        if link.link_type == PayPlanLink::PROCEDURE {
            let Some(proc) = procedures.iter().find(|p| p.proc_num == link.fkey) else {
                continue;
            };
            // OD `ClaimProcs.GetPatPortion` minus splits attached before the
            // plan. With insurance off, the patient portion is simply the fee
            // plus explicit adjustments on that procedure.
            let proc_adjustments: f64 = adjustments
                .iter()
                .filter(|a| a.proc_num == proc.proc_num)
                .map(|a| a.adj_amount)
                .sum();
            let prior_paid: f64 = splits
                .iter()
                .filter(|sp| {
                    sp.proc_num == proc.proc_num && sp.pay_plan_num == 0 && sp.unearned_type == 0
                })
                .map(|sp| sp.split_amount)
                .sum();
            let amount_original = proc.fee + proc_adjustments - prior_paid;
            let attached = if link.amount_override != 0.0 {
                link.amount_override
            } else {
                amount_original
            };
            entries.push(ProductionEntry {
                link_type: link.link_type,
                pri_key: link.fkey,
                provider_num: proc.provider_num,
                production_date: proc.proc_date,
                amount_original,
                amount_override: link.amount_override,
                amount_remaining: attached,
                is_procedure_completed: proc.proc_status == ProcedureStatus::Complete,
            });
        } else if link.link_type == PayPlanLink::ADJUSTMENT {
            let Some(adj) = adjustments.iter().find(|a| a.adj_num == link.fkey) else {
                continue;
            };
            let prior_paid: f64 = splits
                .iter()
                .filter(|sp| sp.adj_num == adj.adj_num && sp.pay_plan_num == 0 && sp.unearned_type == 0)
                .map(|sp| sp.split_amount)
                .sum();
            let amount_original = adj.adj_amount - prior_paid;
            let attached = if link.amount_override != 0.0 {
                link.amount_override
            } else {
                amount_original
            };
            entries.push(ProductionEntry {
                link_type: link.link_type,
                pri_key: link.fkey,
                provider_num: adj.provider_num,
                production_date: adj.adj_date,
                amount_original,
                amount_override: link.amount_override,
                amount_remaining: attached,
                is_procedure_completed: true, // adjustments are always chargeable
            });
        }
    }
    sort_entries(&mut entries);
    entries
}

/// OD `PayPlanEdit.OrderDynamicPayPlanProductionEntries`: incomplete TP
/// procedures last, then production date, then production type, then key.
fn sort_entries(entries: &mut [ProductionEntry]) {
    entries.sort_by(|a, b| {
        let a_tp = a.link_type == PayPlanLink::PROCEDURE && !a.is_procedure_completed;
        let b_tp = b.link_type == PayPlanLink::PROCEDURE && !b.is_procedure_completed;
        a_tp.cmp(&b_tp)
            .then_with(|| a.production_date.cmp(&b.production_date))
            .then_with(|| b.link_type.cmp(&a.link_type))
            .then_with(|| a.pri_key.cmp(&b.pri_key))
    });
}

/// OD `PayPlanProductionEntry.GetWithAmountRemaining`: subtract the principal
/// of existing charges from the linked item they reference, and drop items that
/// are fully charged.
pub fn with_amount_remaining(
    entries: &[ProductionEntry],
    existing_charges: &[PayPlanCharge],
) -> Vec<ProductionEntry> {
    let mut out = entries.to_vec();
    for entry in &mut out {
        if entry.amount_remaining == 0.0 {
            continue;
        }
        for charge in existing_charges
            .iter()
            .filter(|c| c.link_type == entry.link_type && c.fkey == entry.pri_key)
        {
            entry.amount_remaining -= charge.principal.min(entry.amount_remaining);
            if entry.amount_remaining == 0.0 {
                break;
            }
        }
    }
    out.retain(|e| e.amount_remaining.abs() > 1e-9);
    out
}

/// Entries that may be charged under the plan's TP option. With
/// `AwaitComplete`, a TP procedure contributes nothing until it is complete.
pub fn chargeable_entries(
    entries: &[ProductionEntry],
    tp_option: DynamicPayPlanTPOption,
) -> Vec<ProductionEntry> {
    entries
        .iter()
        .filter(|e| {
            tp_option == DynamicPayPlanTPOption::TreatAsComplete
                || e.link_type != PayPlanLink::PROCEDURE
                || e.is_procedure_completed
        })
        .cloned()
        .collect()
}

/// OD `GetPayPlanTerms`-equivalent principal: sum of attached amounts.
pub fn principal_amount(entries: &[ProductionEntry]) -> f64 {
    entries.iter().map(|e| e.attached_amount()).sum()
}

/// OD `PayPlanProductionEntry.GetDynamicPayPlanCompletedAmount`: the amount
/// that may have been issued so far.
pub fn completed_amount(entries: &[ProductionEntry], tp_option: DynamicPayPlanTPOption) -> f64 {
    principal_amount(&chargeable_entries(entries, tp_option))
}

/// OD `PayPlanEdit.CalculatePeriodPayment`: with a payment count, amortise and
/// round the period payment *up* to the nearest cent so the last charge is the
/// small one. With no count, the stored `pay_amt` is the period payment.
pub fn calculate_period_payment(plan: &PayPlan, principal: f64) -> f64 {
    if plan.pay_amt > 0.0 {
        return plan.pay_amt;
    }
    if plan.number_of_payments <= 0 {
        return 0.0;
    }
    let count = plan.number_of_payments as f64;
    let rate = period_rate(plan.apr, plan.charge_frequency);
    let exact = if rate == 0.0 {
        principal / count
    } else {
        principal * rate / (1.0 - (1.0 + rate).powf(-count))
    };
    (exact * 100.0).ceil() / 100.0
}

/// Number of period steps from `first` to `today` (OD `CalcPeriodsForFrequency`).
/// Used only by tests/callers that want the current period index; issuance
/// iterates from period 0 and skips dates already charged.
pub fn calc_periods_for_frequency(
    first: NaiveDate,
    frequency: PayPlanFrequency,
    today: NaiveDate,
) -> i64 {
    let mut period = 0_i64;
    while next_period_date(first, period, frequency) <= today {
        period += 1;
        if period >= MAX_PLAN_CHARGES as i64 {
            break;
        }
    }
    period.saturating_sub(1).max(0)
}

fn is_down_payment(charge: &PayPlanCharge) -> bool {
    charge.note.to_lowercase().contains("down payment")
}

/// True when a charge row was zeroed and annotated by
/// [`resolve_stale_issued_charges`] rather than deleted. The financial history
/// row is intentionally kept for audit; callers decide whether to display it.
pub fn is_stale_resolved_charge(charge: &PayPlanCharge) -> bool {
    (charge.principal + charge.interest).abs() < 0.005 && charge.note.contains("[Stale resolved")
}

fn has_charge_on(existing: &[PayPlanCharge], date: NaiveDate) -> bool {
    existing
        .iter()
        .any(|c| c.charge_date == date && !is_down_payment(c))
}

/// Shared charge builder. `only_due` stops at `today` (issuance); otherwise the
/// whole schedule is produced (preview). `skip_existing_dates` suppresses a
/// second charge for a period that is already in the database.
fn build_charges(
    plan: &PayPlan,
    all_entries: &[ProductionEntry],
    existing_charges: &[PayPlanCharge],
    today: NaiveDate,
    only_due: bool,
    skip_existing_dates: bool,
) -> Vec<PayPlanCharge> {
    if !plan.is_dynamic || plan.is_closed || plan.is_locked {
        return Vec::new();
    }
    let chargeable = chargeable_entries(all_entries, plan.dynamic_pay_plan_tp_option);
    let mut entries = with_amount_remaining(&chargeable, existing_charges);
    let mut remaining: f64 = entries.iter().map(|e| e.amount_remaining).sum();
    if remaining <= 1e-9 {
        return Vec::new();
    }

    let period_payment = calculate_period_payment(plan, remaining + existing_principal(existing_charges));
    if period_payment <= 0.0 {
        return Vec::new();
    }
    let rate = period_rate(plan.apr, plan.charge_frequency);
    let first = plan.date_pay_plan_start.unwrap_or(plan.pay_plan_date);
    let interest_start = plan.date_interest_start.unwrap_or(first);

    let mut out = Vec::new();
    // OD issues the down payment first, dated today, only when the plan has no
    // charges yet. It is not tied to a linked production item.
    if existing_charges.is_empty() && plan.down_payment > 0.0 {
        let down = plan.down_payment.min(remaining);
        out.push(PayPlanCharge {
            pay_plan_num: plan.pay_plan_num,
            guarantor: plan.guarantor,
            pat_num: plan.pat_num,
            charge_date: today,
            principal: round2(down),
            interest: 0.0,
            note: "Down payment".to_string(),
            charge_type: PayPlanChargeType::Debit,
            ..Default::default()
        });
        remaining -= down;
    }

    let mut period = 0_i64;
    while remaining > 1e-9 && out.len() < MAX_PLAN_CHARGES {
        let date = next_period_date(first, period, plan.charge_frequency);
        if only_due && date > today {
            break;
        }
        if skip_existing_dates && has_charge_on(existing_charges, date) {
            period += 1;
            continue;
        }
        let interest = if date >= interest_start {
            round2(remaining * rate)
        } else {
            0.0
        };
        let due = (period_payment - interest).min(remaining);
        if due <= 1e-9 {
            break;
        }
        let mut left = due;
        let mut first_charge_of_period = true;
        for entry in &mut entries {
            if left <= 1e-9 {
                break;
            }
            if entry.amount_remaining <= 1e-9 {
                continue;
            }
            let take = left.min(entry.amount_remaining);
            out.push(PayPlanCharge {
                pay_plan_num: plan.pay_plan_num,
                guarantor: plan.guarantor,
                pat_num: plan.pat_num,
                charge_date: date,
                principal: round2(take),
                // OD applies interest to the first charge of a period only.
                interest: if first_charge_of_period {
                    round2(interest)
                } else {
                    0.0
                },
                note: format!("Plan charge {} of {}", period + 1, plan.number_of_payments.max(0)),
                provider_num: entry.provider_num,
                charge_type: PayPlanChargeType::Debit,
                fkey: entry.pri_key,
                link_type: entry.link_type,
                ..Default::default()
            });
            first_charge_of_period = false;
            entry.amount_remaining -= take;
            left -= take;
        }
        remaining -= due - left;
        period += 1;
    }
    out
}

fn existing_principal(existing: &[PayPlanCharge]) -> f64 {
    existing
        .iter()
        .filter(|c| !is_down_payment(c) && c.charge_type == PayPlanChargeType::Debit)
        .map(|c| c.principal)
        .sum()
}

/// Full expected schedule for display/preview. Does not include charges whose
/// period already exists in the database, and stops at the 2,000-charge ceiling.
pub fn expected_charges(
    plan: &PayPlan,
    links: &[PayPlanLink],
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[od_domain::PaySplit],
    existing_charges: &[PayPlanCharge],
    today: NaiveDate,
) -> Vec<PayPlanCharge> {
    let plan_links: Vec<PayPlanLink> = links
        .iter()
        .filter(|l| l.pay_plan_num == plan.pay_plan_num)
        .cloned()
        .collect();
    let entries = production_entries(&plan_links, procedures, adjustments, splits);
    build_charges(
        plan,
        &entries,
        existing_charges,
        today,
        false,
        true,
    )
}

/// Issue every charge that is due on or before `today` and has not already been
/// issued. Idempotent: running it twice produces no new rows. This is the
/// in-process equivalent of OD's service-driven
/// `IssueChargesDueForDynamicPaymentPlans`, called when a plan or its linked
/// production changes and from a "Issue charges due" action.
pub fn issue_charges_due(
    plan: &PayPlan,
    links: &[PayPlanLink],
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[od_domain::PaySplit],
    existing_charges: &[PayPlanCharge],
    today: NaiveDate,
) -> Vec<PayPlanCharge> {
    let plan_links: Vec<PayPlanLink> = links
        .iter()
        .filter(|l| l.pay_plan_num == plan.pay_plan_num)
        .cloned()
        .collect();
    let entries = production_entries(&plan_links, procedures, adjustments, splits);
    let mut charges = build_charges(
        plan,
        &entries,
        existing_charges,
        today,
        true,
        true,
    );
    // A catch-up run should never bury the account in a huge back-log.
    charges.truncate(MAX_CATCH_UP_PERIODS as usize);
    charges
}

/// Reconciliation snapshot for a dynamic plan as of `as_of`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DynamicPlanReconciliation {
    pub issued_count: usize,
    pub issued_total: f64,
    pub expected_due_count: usize,
    pub expected_due_total: f64,
    pub expected_full_count: usize,
    pub expected_full_total: f64,
    pub missing_due_count: usize,
    pub missing_due_total: f64,
    /// Issued debits (not down payments) that no longer match the current
    /// expected full schedule by (date,fkey,link_type) or whose amount changed.
    /// These are stale/over-issued after production/link/term changes.
    pub stale_issued_count: usize,
    pub stale_issued_total: f64,
}

/// A single issued charge flagged as stale/over-issued.
#[derive(Clone, Debug)]
pub struct StaleIssuedCharge {
    pub charge: PayPlanCharge,
    /// Expected amount for same slot if any; None means orphan/no matching slot.
    pub expected_amount: Option<f64>,
    pub has_splits: bool,
}

/// Compare issued dynamic-plan charges against expected schedule through `as_of`.
/// `missing_due_*` counts expected-due charges/amount not yet issued as of `as_of`.
/// `stale_issued_*` counts issued debits that no longer match current expected schedule.
pub fn reconciliation(
    plan: &PayPlan,
    links: &[PayPlanLink],
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[od_domain::PaySplit],
    existing_charges: &[PayPlanCharge],
    as_of: NaiveDate,
) -> DynamicPlanReconciliation {
    let (all_own, expected_due, expected_full, stale) =
        reconciliation_detail(plan, links, procedures, adjustments, splits, existing_charges, as_of);
    let own_active: Vec<&PayPlanCharge> = all_own.iter().filter(|c| !is_stale_resolved_charge(c)).collect();
    let issued: Vec<&PayPlanCharge> = own_active.iter().copied().filter(|c| c.charge_date <= as_of).collect();
    let issued_total: f64 = issued.iter().map(|c| c.principal + c.interest).sum();
    let expected_due_total: f64 = expected_due.iter().map(|c| c.principal + c.interest).sum();
    let expected_full_total: f64 = expected_full.iter().map(|c| c.principal + c.interest).sum();

    let mut missing_due_count = 0usize;
    let mut missing_due_total = 0.0f64;
    for exp in &expected_due {
        let covered = all_own.iter().any(|c| c.charge_date == exp.charge_date && c.fkey == exp.fkey && c.link_type == exp.link_type);
        if !covered {
            missing_due_count += 1;
            missing_due_total += exp.principal + exp.interest;
        }
    }
    let stale_issued_total: f64 = stale.iter().map(|s| s.charge.principal + s.charge.interest).sum();

    DynamicPlanReconciliation {
        issued_count: issued.len(),
        issued_total: round2(issued_total),
        expected_due_count: expected_due.len(),
        expected_due_total: round2(expected_due_total),
        expected_full_count: expected_full.len(),
        expected_full_total: round2(expected_full_total),
        missing_due_count,
        missing_due_total: round2(missing_due_total),
        stale_issued_count: stale.len(),
        stale_issued_total: round2(stale_issued_total),
    }
}

/// Detailed reconciliation including stale/over-issued charge list.
pub fn reconciliation_detail(
    plan: &PayPlan,
    links: &[PayPlanLink],
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[od_domain::PaySplit],
    existing_charges: &[PayPlanCharge],
    as_of: NaiveDate,
) -> (
    Vec<PayPlanCharge>,
    Vec<PayPlanCharge>,
    Vec<PayPlanCharge>,
    Vec<StaleIssuedCharge>,
) {
    let plan_links: Vec<PayPlanLink> = links
        .iter()
        .filter(|l| l.pay_plan_num == plan.pay_plan_num)
        .cloned()
        .collect();
    let entries = production_entries(&plan_links, procedures, adjustments, splits);
    let all_own: Vec<PayPlanCharge> = existing_charges
        .iter()
        .filter(|c| c.pay_plan_num == plan.pay_plan_num && c.charge_type == PayPlanChargeType::Debit && !is_down_payment(c))
        .cloned()
        .collect();
    let own_existing: Vec<PayPlanCharge> = all_own.iter().filter(|c| !is_stale_resolved_charge(c)).cloned().collect();
    let expected_due = build_charges(plan, &entries, &[], as_of, true, false);
    let expected_full = build_charges(plan, &entries, &[], as_of, false, false);

    let mut stale = Vec::new();
    for c in &own_existing {
        let amt = round2(c.principal + c.interest);
        let matched = expected_full.iter().find(|e| e.charge_date == c.charge_date && e.fkey == c.fkey && e.link_type == c.link_type);
        let is_stale = match matched {
            None => true,
            Some(e) => (round2(e.principal + e.interest) - amt).abs() > 0.01,
        };
        if is_stale {
            // Conservative: any split explicitly targeting this charge means it has splits.
            let has_splits = splits.iter().any(|s| s.pay_plan_charge_num == c.pay_plan_charge_num && c.pay_plan_charge_num != 0);
            stale.push(StaleIssuedCharge {
                charge: c.clone(),
                expected_amount: matched.map(|e| round2(e.principal + e.interest)),
                has_splits,
            });
        }
    }
    stale.sort_by_key(|s| (s.charge.charge_date, s.charge.pay_plan_charge_num));
    (own_existing, expected_due, expected_full, stale)
}

/// Staff-facing evidence for each stale charge, derived from the same
/// reconciliation detail that gates resolution. Shown before a staff member
/// confirms "Resolve stale charges (safe)"; it never changes stored rows.
pub fn stale_charge_evidence(stale: &[StaleIssuedCharge]) -> Vec<String> {
    stale
        .iter()
        .map(|s| {
            let issued = round2(s.charge.principal + s.charge.interest);
            let expected = match s.expected_amount {
                Some(amount) => format!("{:.2}", round2(amount)),
                None => "no matching slot".to_string(),
            };
            let splits = if s.has_splits {
                "applied splits — blocked"
            } else {
                "no splits — safe to resolve"
            };
            format!(
                "charge #{} dated {}: issued {:.2}, expected {}, {}",
                s.charge.pay_plan_charge_num, s.charge.charge_date, issued, expected, splits
            )
        })
        .collect()
}

/// Explicit staff resolution for stale/over-issued dynamic-plan debits.
/// Safe behavior: only charges with no applied splits are resolved by zeroing
/// principal/interest and annotating the note; financial history row is kept.
/// Charges with applied splits are reported as blocked.
#[derive(Clone, Debug, Default)]
pub struct StaleResolutionResult {
    pub resolved: Vec<PayPlanCharge>,
    pub blocked_with_splits: Vec<Id>,
}

pub fn resolve_stale_issued_charges(
    stale: &[StaleIssuedCharge],
    as_of: NaiveDate,
    reason: &str,
) -> StaleResolutionResult {
    let mut out = StaleResolutionResult::default();
    let reason = reason.trim();
    let reason = if reason.is_empty() { "production/terms changed" } else { reason };
    for s in stale {
        if s.has_splits {
            out.blocked_with_splits.push(s.charge.pay_plan_charge_num);
            continue;
        }
        let mut c = s.charge.clone();
        c.principal = 0.0;
        c.interest = 0.0;
        let tag = format!("[Stale resolved {as_of}: {reason}]");
        c.note = if c.note.trim().is_empty() {
            tag
        } else if c.note.contains("[Stale resolved") {
            c.note.clone()
        } else {
            format!("{} {tag}", c.note.trim())
        };
        out.resolved.push(c);
    }
    out
}

/// Why a dynamic plan cannot issue charges right now. Returned to the UI so the
/// user sees the exact reason rather than a silent no-op.
pub fn issuance_block(plan: &PayPlan, links: &[PayPlanLink]) -> Option<&'static str> {
    if !plan.is_dynamic {
        return Some("This is not a dynamic payment plan.");
    }
    if plan.is_closed {
        return Some("This payment plan is closed.");
    }
    if plan.is_locked {
        return Some("This payment plan is locked.");
    }
    if !links
        .iter()
        .any(|l| l.pay_plan_num == plan.pay_plan_num && l.link_type != PayPlanLink::NONE)
    {
        return Some("Attach at least one procedure or adjustment to the plan first.");
    }
    if plan.pay_amt <= 0.0 && plan.number_of_payments <= 0 {
        return Some("Set a payment amount or a payment count before issuing charges.");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_domain::PaymentSchedule;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn plan(tp_option: DynamicPayPlanTPOption) -> PayPlan {
        PayPlan {
            pay_plan_num: 9,
            pat_num: 4,
            guarantor: 4,
            pay_plan_date: date(2026, 1, 1),
            date_pay_plan_start: Some(date(2026, 1, 1)),
            apr: 0.0,
            pay_amt: 100.0,
            number_of_payments: 0,
            charge_frequency: PayPlanFrequency::Monthly,
            pay_schedule: PaymentSchedule::Monthly,
            is_dynamic: true,
            dynamic_pay_plan_tp_option: tp_option,
            ..Default::default()
        }
    }

    fn proc(num: Id, fee: f64, status: ProcedureStatus, day: u32) -> Procedure {
        Procedure {
            proc_num: num,
            pat_num: 4,
            fee,
            proc_status: status,
            proc_date: date(2026, 1, day),
            provider_num: 7,
            ..Default::default()
        }
    }

    fn link(num: Id, link_type: i8, fkey: Id) -> PayPlanLink {
        PayPlanLink {
            pay_plan_link_num: num,
            pay_plan_num: 9,
            link_type,
            fkey,
            ..Default::default()
        }
    }

    #[test]
    fn production_entries_use_fee_plus_adjustments_less_prior_splits() {
        let links = [link(1, PayPlanLink::PROCEDURE, 100)];
        let procedures = [proc(100, 400.0, ProcedureStatus::Complete, 2)];
        let adjustments = [Adjustment {
            adj_num: 5,
            proc_num: 100,
            adj_amount: -25.0,
            ..Default::default()
        }];
        let splits = [od_domain::PaySplit {
            proc_num: 100,
            split_amount: 50.0,
            pay_plan_num: 0,
            ..Default::default()
        }];
        let entries = production_entries(&links, &procedures, &adjustments, &splits);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].amount_original, 325.0); // 400 - 25 - 50
        assert_eq!(entries[0].attached_amount(), 325.0);
        assert!(entries[0].is_procedure_completed);
        assert_eq!(entries[0].provider_num, 7);
    }

    #[test]
    fn amount_override_replaces_computed_portion() {
        let links = [PayPlanLink {
            amount_override: 200.0,
            ..link(1, PayPlanLink::PROCEDURE, 100)
        }];
        let procedures = [proc(100, 400.0, ProcedureStatus::Complete, 2)];
        let entries = production_entries(&links, &procedures, &[], &[]);
        assert_eq!(entries[0].amount_original, 400.0);
        assert_eq!(entries[0].attached_amount(), 200.0);
        assert_eq!(entries[0].amount_remaining, 200.0);
    }

    #[test]
    fn await_complete_excludes_treatment_planned_procedures() {
        let links = [
            link(1, PayPlanLink::PROCEDURE, 100), // complete
            link(2, PayPlanLink::PROCEDURE, 101), // TP
        ];
        let procedures = [
            proc(100, 400.0, ProcedureStatus::Complete, 2),
            proc(101, 300.0, ProcedureStatus::TreatmentPlanned, 3),
        ];
        let all = production_entries(&links, &procedures, &[], &[]);
        assert_eq!(all.len(), 2);
        assert_eq!(completed_amount(&all, DynamicPayPlanTPOption::AwaitComplete), 400.0);
        assert_eq!(
            completed_amount(&all, DynamicPayPlanTPOption::TreatAsComplete),
            700.0
        );
        let chargeable = chargeable_entries(&all, DynamicPayPlanTPOption::AwaitComplete);
        assert_eq!(chargeable.len(), 1);
        assert_eq!(chargeable[0].pri_key, 100);
        // Sorting puts the incomplete TP procedure last.
        assert_eq!(all[1].pri_key, 101);
    }

    #[test]
    fn expected_charges_assign_each_link_its_own_fkey() {
        let links = [
            link(1, PayPlanLink::PROCEDURE, 100),
            link(2, PayPlanLink::ADJUSTMENT, 5),
        ];
        let procedures = [proc(100, 150.0, ProcedureStatus::Complete, 2)];
        let adjustments = [Adjustment {
            adj_num: 5,
            adj_date: date(2026, 1, 3),
            adj_amount: 50.0,
            ..Default::default()
        }];
        let mut p = plan(DynamicPayPlanTPOption::None);
        p.pay_amt = 200.0; // one period pays off both linked items
        let charges = expected_charges(
            &p,
            &links,
            &procedures,
            &adjustments,
            &[],
            &[],
            date(2026, 1, 1),
        );
        assert_eq!(charges.len(), 2); // one per linked production item
        assert_eq!(charges[0].principal, 150.0);
        assert_eq!(charges[0].fkey, 100);
        assert_eq!(charges[0].link_type, PayPlanLink::PROCEDURE);
        assert_eq!(charges[1].principal, 50.0);
        assert_eq!(charges[1].fkey, 5);
        assert_eq!(charges[1].link_type, PayPlanLink::ADJUSTMENT);
        let total: f64 = charges.iter().map(|c| c.principal).sum();
        assert_eq!(total, 200.0);
    }

    #[test]
    fn issuance_is_period_by_period_and_idempotent() {
        let links = [link(1, PayPlanLink::PROCEDURE, 100)];
        let procedures = [proc(100, 200.0, ProcedureStatus::Complete, 2)];
        let p = plan(DynamicPayPlanTPOption::None);
        // Only charges due on or before 1 Feb (two periods: Jan and Feb).
        let due = issue_charges_due(
            &p,
            &links,
            &procedures,
            &[],
            &[],
            &[],
            date(2026, 2, 1),
        );
        assert_eq!(due.len(), 2);
        assert_eq!(due[0].charge_date, date(2026, 1, 1));
        assert_eq!(due[1].charge_date, date(2026, 2, 1));
        // Running again with those charges present yields nothing new.
        let again = issue_charges_due(
            &p,
            &links,
            &procedures,
            &[],
            &[],
            &due,
            date(2026, 2, 1),
        );
        assert!(again.is_empty());
    }

    #[test]
    fn completing_a_linked_tp_procedure_unlocks_more_charges() {
        let links = [link(1, PayPlanLink::PROCEDURE, 100)];
        let tp = [proc(100, 200.0, ProcedureStatus::TreatmentPlanned, 2)];
        let p = plan(DynamicPayPlanTPOption::AwaitComplete);
        assert!(issue_charges_due(&p, &links, &tp, &[], &[], &[], date(2026, 3, 1)).is_empty());

        let complete = [proc(100, 200.0, ProcedureStatus::Complete, 2)];
        let due = issue_charges_due(
            &p,
            &links,
            &complete,
            &[],
            &[],
            &[],
            date(2026, 3, 1),
        );
        assert_eq!(due.len(), 2);
        let total: f64 = due.iter().map(|c| c.principal).sum();
        assert_eq!(total, 200.0);
    }

    #[test]
    fn interest_uses_period_rate_and_first_charge_only() {
        let links = [
            link(1, PayPlanLink::PROCEDURE, 100),
            link(2, PayPlanLink::PROCEDURE, 101),
        ];
        let procedures = [
            proc(100, 120.0, ProcedureStatus::Complete, 2),
            proc(101, 120.0, ProcedureStatus::Complete, 3),
        ];
        let mut p = plan(DynamicPayPlanTPOption::None);
        p.apr = 12.0; // 1% per month on 240 → 2.40
        p.pay_amt = 240.0;
        let charges = expected_charges(&p, &links, &procedures, &[], &[], &[], date(2026, 1, 1));
        // First period splits across two links: interest only on the first.
        assert_eq!(charges[0].interest, 2.40);
        assert_eq!(charges[1].interest, 0.0);
        assert_eq!(round2(charges[0].principal + charges[1].principal), 237.60);
    }

    #[test]
    fn calculate_period_payment_amortises_when_count_set() {
        let mut p = plan(DynamicPayPlanTPOption::None);
        p.pay_amt = 0.0;
        p.number_of_payments = 5;
        p.apr = 12.0;
        // 1000 over 5 at 1% per period ≈ 206.04 rounded up.
        assert_eq!(calculate_period_payment(&p, 1000.0), 206.04);
        p.apr = 0.0;
        assert_eq!(calculate_period_payment(&p, 1000.0), 200.0);
    }

    #[test]
    fn issuance_block_reports_configuration_problems() {
        let links = [link(1, PayPlanLink::PROCEDURE, 100)];
        let mut p = plan(DynamicPayPlanTPOption::None);
        assert!(issuance_block(&p, &links).is_none());
        p.is_dynamic = false;
        assert!(issuance_block(&p, &links).is_some());
        p.is_dynamic = true;
        assert!(issuance_block(&p, &[]).is_some());
        p.pay_amt = 0.0;
        p.number_of_payments = 0;
        assert!(issuance_block(&p, &links).is_some());
    }

    #[test]
    fn mid_period_as_of_does_not_issue_next_period_charge() {
        let links = [link(1, PayPlanLink::PROCEDURE, 100)];
        let procedures = [proc(100, 300.0, ProcedureStatus::Complete, 2)];
        let p = plan(DynamicPayPlanTPOption::None); // monthly 100, starts 2026-01-01
        let mid_jan = date(2026, 1, 15);
        let due = issue_charges_due(&p, &links, &procedures, &[], &[], &[], mid_jan);
        assert_eq!(due.len(), 1, "only Jan 1 period is due by Jan 15");
        assert_eq!(due[0].charge_date, date(2026, 1, 1));
        let due_feb1 = issue_charges_due(&p, &links, &procedures, &[], &[], &due, date(2026, 2, 1));
        assert_eq!(due_feb1.len(), 1);
        assert_eq!(due_feb1[0].charge_date, date(2026, 2, 1));
    }

    #[test]
    fn reconciliation_reports_issued_and_expected_through_as_of() {
        let links = [link(1, PayPlanLink::PROCEDURE, 100)];
        let procedures = [proc(100, 300.0, ProcedureStatus::Complete, 2)];
        let p = plan(DynamicPayPlanTPOption::None); // monthly 100, 3 periods
        let as_of = date(2026, 2, 15); // mid period: Jan+Feb due, Mar not due
        let issued = issue_charges_due(&p, &links, &procedures, &[], &[], &[], as_of);
        assert_eq!(issued.len(), 2);
        let rec = reconciliation(&p, &links, &procedures, &[], &[], &issued, as_of);
        assert_eq!(rec.issued_count, 2);
        assert_eq!(round2(rec.issued_total), 200.0);
        assert_eq!(rec.expected_due_count, 2);
        assert_eq!(round2(rec.expected_due_total), 200.0);
        assert_eq!(rec.expected_full_count, 3);
        assert_eq!(round2(rec.expected_full_total), 300.0);
        assert_eq!(rec.missing_due_count, 0);
        assert!(round2(rec.missing_due_total).abs() < 0.01);
        assert_eq!(rec.stale_issued_count, 0);
    }

    #[test]
    fn reconciliation_flags_stale_issued_after_production_reduction() {
        let links = [link(1, PayPlanLink::PROCEDURE, 100)];
        let procedures = [proc(100, 300.0, ProcedureStatus::Complete, 2)];
        let mut p = plan(DynamicPayPlanTPOption::None);
        p.pay_amt = 100.0;
        let as_of = date(2026, 3, 1);
        let mut issued = issue_charges_due(&p, &links, &procedures, &[], &[], &[], as_of);
        for (i, c) in issued.iter_mut().enumerate() { c.pay_plan_charge_num = (i as Id) + 1; }
        assert_eq!(issued.len(), 3);
        // Production reduced below already-issued amount; expected full becomes 2x100.
        let reduced = [proc(100, 200.0, ProcedureStatus::Complete, 2)];
        let rec = reconciliation(&p, &links, &reduced, &[], &[], &issued, as_of);
        assert!(rec.stale_issued_count >= 1, "stale={}", rec.stale_issued_count);
        assert!(rec.stale_issued_total > 0.0);
        let (_own, _due, _full, stale) = reconciliation_detail(&p, &links, &reduced, &[], &[], &issued, as_of);
        assert!(stale.iter().any(|s| s.charge.pay_plan_charge_num == 3));
    }

    #[test]
    fn reconciliation_flags_stale_amount_change() {
        let links = [link(1, PayPlanLink::PROCEDURE, 100)];
        let procedures = [proc(100, 300.0, ProcedureStatus::Complete, 2)];
        let mut p = plan(DynamicPayPlanTPOption::None);
        p.pay_amt = 100.0;
        let as_of = date(2026, 2, 1);
        let mut issued = issue_charges_due(&p, &links, &procedures, &[], &[], &[], as_of);
        for (i, c) in issued.iter_mut().enumerate() { c.pay_plan_charge_num = (i as Id) + 1; }
        let mut changed = p.clone();
        changed.pay_amt = 150.0; // term change makes old issued amounts stale vs expected
        let rec = reconciliation(&changed, &links, &procedures, &[], &[], &issued, as_of);
        assert!(rec.stale_issued_count >= 1);
    }

    #[test]
    fn resolve_stale_issued_charges_blocks_splits_and_zeroes_safe_rows() {
        let links = [link(1, PayPlanLink::PROCEDURE, 100)];
        let procedures = [proc(100, 300.0, ProcedureStatus::Complete, 2)];
        let mut p = plan(DynamicPayPlanTPOption::None);
        p.pay_amt = 100.0;
        let as_of = date(2026, 3, 1);
        let mut issued = issue_charges_due(&p, &links, &procedures, &[], &[], &[], as_of);
        for (i, c) in issued.iter_mut().enumerate() { c.pay_plan_charge_num = (i as Id) + 1; }
        let reduced = [proc(100, 100.0, ProcedureStatus::Complete, 2)];
        let (_own, _due, _full, stale) = reconciliation_detail(&p, &links, &reduced, &[], &[], &issued, as_of);
        assert!(!stale.is_empty());
        let splits = vec![od_domain::PaySplit {
            split_num: 1,
            pay_num: 10,
            pat_num: 2,
            pay_plan_num: 1,
            pay_plan_charge_num: 2,
            pay_plan_debit_type: od_domain::PayPlanDebitType::Principal,
            split_amount: 100.0,
            date_pay: date(2026, 1, 10),
            ..Default::default()
        }];
        let stale_with_split: Vec<_> = stale
            .into_iter()
            .map(|mut s| { s.has_splits = splits.iter().any(|sp| sp.pay_plan_charge_num == s.charge.pay_plan_charge_num); s })
            .collect();
        let res = resolve_stale_issued_charges(&stale_with_split, as_of, "production reduced");
        assert!(res.blocked_with_splits.contains(&2));
        assert!(res.resolved.iter().all(|c| c.pay_plan_charge_num != 2));
        for c in &res.resolved {
            assert_eq!(round2(c.principal + c.interest), 0.0);
            assert!(c.note.contains("Stale resolved"));
        }
    }

    #[test]
    fn stale_charge_evidence_shows_issued_expected_and_split_blocking() {
        let stale = vec![
            StaleIssuedCharge {
                charge: PayPlanCharge {
                    pay_plan_charge_num: 12,
                    charge_date: date(2026, 3, 1),
                    principal: 100.0,
                    ..Default::default()
                },
                expected_amount: Some(60.0),
                has_splits: false,
            },
            StaleIssuedCharge {
                charge: PayPlanCharge {
                    pay_plan_charge_num: 13,
                    charge_date: date(2026, 4, 1),
                    principal: 75.0,
                    ..Default::default()
                },
                expected_amount: None,
                has_splits: true,
            },
        ];
        let lines = stale_charge_evidence(&stale);
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            "charge #12 dated 2026-03-01: issued 100.00, expected 60.00, no splits — safe to resolve"
        );
        assert_eq!(
            lines[1],
            "charge #13 dated 2026-04-01: issued 75.00, expected no matching slot, applied splits — blocked"
        );
    }

    #[test]
    fn stale_resolved_charge_marker_is_recognized() {
        let marker = "[Stale resolved 2026-03-01: production reduced]";
        let mut charge = PayPlanCharge {
            note: marker.into(),
            ..Default::default()
        };
        assert!(is_stale_resolved_charge(&charge));

        // The marker alone is not enough: a live charge may legitimately quote
        // the resolution in its note while still carrying a balance.
        charge.principal = 50.0;
        assert!(!is_stale_resolved_charge(&charge));

        // Nor is a zeroed charge without the annotation a resolved row.
        charge.principal = 0.0;
        charge.note.clear();
        assert!(!is_stale_resolved_charge(&charge));
    }
}
