//! Business rules shared by the desktop modules.

pub mod auth;
pub mod clinical;
pub mod definitions;
pub mod discount;
pub mod dynamic_payment_plan;
pub mod journal;
pub mod payment_plan;
pub mod period_close;
pub mod pdf;
pub mod recall;
pub mod registry;
pub mod reports;
pub mod reports_upstream;
pub mod reminders;
pub mod transport;
pub mod treatment_plan;

pub use clinical::{
    appointment_change_note, is_waitlist, parse_appointment_datetime, parse_appointment_length,
    patient_change_note, pattern_for_length, restore_appointment, schedule_from_waitlist,
    waitlist_appointments, waitlist_status_definitions, waitlist_status_name,
};
pub use definitions::{
    compact_definition_orders, compact_display_field_orders, display_fields_for,
    next_definition_order, sorted_definitions, validate_definition, validate_display_fields,
};
pub use discount::{
    annual_bucket, annual_max_range, discount_amount, proc_fee_total, quantity, DiscountInput,
    DiscountResult, FrequencyCodeSets,
};
pub use dynamic_payment_plan::{
    calc_periods_for_frequency, calculate_period_payment, chargeable_entries, completed_amount,
    expected_charges, issuance_block, issue_charges_due, is_stale_resolved_charge,
    principal_amount, production_entries, reconciliation, reconciliation_detail,
    resolve_stale_issued_charges, stale_charge_evidence, stale_resolution_reason,
    with_amount_remaining,
    DynamicPlanReconciliation, ProductionEntry, StaleIssuedCharge, StaleResolutionResult,
    MAX_CATCH_UP_PERIODS,
};
pub use payment_plan::{
    charge_generation_block, generate_plan_charges, next_period_date, period_rate,
    MAX_PLAN_CHARGES,
};
pub use journal::{
    adjustment_journal_memo, adjustment_journal_memo_with_note, adjustment_journal_reference,
    build_adjustment_journal, build_charge_journal, build_charge_posting_plan,
    build_journal_batches, build_payment_journal, build_posting_batch_plan,
    build_posting_batch_plan_with_closed, is_adjustment_posted, is_payment_posted,
    is_procedure_posted,
    journal_transaction_for_adjustment, journal_transaction_for_payment,
    journal_transaction_for_procedure, journal_posting_audit_report, ledger_tie_out_report,
    next_journal_transaction_num, payment_journal_memo,
    payment_journal_memo_with_note, payment_journal_reference, procedure_journal_memo,
    procedure_journal_memo_with_note, procedure_journal_reference, suggest_charge_ledger_accounts,
    suggest_ledger_accounts, suggest_payment_ledger_accounts, validate_balanced_journal,
    ChargePostingPlan, PostingAccountDefaults, PostingBatchItem, PostingSource, PostingSourceKind,
};
pub use period_close::{
    CLOSED_PERIODS_PREF, ClosedPeriod, ClosedPeriods, PeriodCloseAction, PeriodCloseActionFilter,
    PeriodCloseEvent, PeriodCloseHistoryFilter, close_audit_text, month_bounds,
    period_close_events, period_close_history_report, period_close_history_report_filtered,
    reopen_audit_text, tie_out_ready,
};
pub use pdf::{render_invoice_pdf, render_statement_pdf};
pub use registry::{build_invoice_document, validate_clinic_card_issue};
pub use od_domain::reports::{
    ActivePatientReportOptions, AdjustmentSheetReportOptions, AgingAgeFilter, AgingOptions,
    AgingFamilyGrouping, AppointmentDateBasis, AppointmentReportOptions, BrokenAppointmentMode,
    BrokenAppointmentReportOptions, BrokenProcedureOption, ConfirmationReportOptions,
    CustomAgingInclude, CustomAgingNegative, CustomAgingReportOptions, CustomAgingWriteoffDate,
    FinanceChargeReportOptions, NewPatientReportOptions, PatientsReportOptions, PaySheetGroupBy,
    PaySheetReportOptions, PlanTypeFilter, PayPlanReportOptions, ProcedureCodesReportOptions, ProcedureSheetMode,
    ProcedureSheetReportOptions, ProdIncMode, ProdIncReportOptions, RecallFieldsReportOptions,
};
pub use reports::{
    active_patient_report, adj_sheet_report, aging_report, appointments_report, asap_report,
    birthday_report, broken_appointments_report, collections_bucket_totals,
    collections_worklist, confirmation_report, custom_aging_report, filter_collections_rows,
    finance_charge_report,
    new_patient_report, pay_plan_charge_applications_report, pay_plan_detail_report, pay_sheet_report, payment_plan_report,
    patients_report, prescriptions_report, procedure_codes_report, prod_inc_report, procedure_sheet_report,
    recall_fields_report, recall_report, referral_analysis_report, referrals_report,
    render_report_csv, render_report_html, treatment_plan_report, CollectionsAgingBucket,
    CollectionsBucketTotals, CollectionsRow, ReportRow, ReportTable,
};
pub use reports_upstream::{
    balance_sheet_report, carrier_label_addresses, dental_sealant_measure_report, discount_plan_report,
    dpp_overcharged_report, general_ledger_report, hidden_pay_splits_report, laser_labels_report,
    patient_label_addresses, patient_portion_uncollected_report, ph_raw_population_report,
    ph_raw_screen_report, treatment_finder_report,
    presented_treatment_production_report, print_preview_report, proc_note_report,
    proc_overpaid_report, profit_loss_report, production_goal_report, provider_payroll_report,
    receivables_breakdown_report, routing_slips_report, service_date_view_report,
    statement_report, treat_plan_presentation_statistics_report, unearned_income_report,
    DiscountPlanSubscription, LabelAddress, PublicHealthRow,
};
pub use reminders::{due_messages, manual_message, render_template, resolve_transport, ReminderContext};
pub use transport::{deliver, deliver as deliver_message, delivery_preview, installed_transports, planned_note, Transport};
pub use recall::{
    add_interval, asap_appointments, format_interval, interval_from_parts, interval_parts,
    recall_due_date, recall_due_state, RecallDueState,
};
pub use treatment_plan::{
    default_heading, new_treat_plan, set_status, snapshot_plan, treat_plan_total,
    validate_treat_plan,
};

use chrono::Datelike;
use od_domain::*;

/// Appointment numbers that overlap `candidate` in the same operatory or with
/// the same provider. Cancelled and unscheduled appointments are ignored,
/// matching the way the Open Dental schedule treats blocked time.
pub fn schedule_conflicts(candidate: &Appointment, existing: &[Appointment]) -> Vec<Id> {
    let start = candidate.apt_datetime;
    let end = start + chrono::Duration::minutes(candidate.apt_length as i64);
    existing
        .iter()
        .filter(|a| {
            a.apt_num != candidate.apt_num
                && matches!(
                    a.apt_status,
                    AppointmentStatus::Scheduled | AppointmentStatus::Planned
                )
                && (a.operatory_num == candidate.operatory_num
                    || (candidate.provider_num != 0 && a.provider_num == candidate.provider_num))
                && a.apt_datetime < end
                && (a.apt_datetime + chrono::Duration::minutes(a.apt_length as i64)) > start
        })
        .map(|a| a.apt_num)
        .collect()
}

/// Outstanding balance for a patient: completed procedure fees minus payments.
pub fn patient_balance(procedures: &[Procedure], payments: &[Payment]) -> f64 {
    let charges: f64 = procedures
        .iter()
        .filter(|p| p.proc_status.is_completed())
        .map(|p| p.fee)
        .sum();
    let paid: f64 = payments.iter().map(|p| p.pay_amount).sum();
    (charges - paid).max(0.0)
}

/// A patient's account broken into the transaction classes used by Open Dental
/// `Ledgers.GetAgingQueryString` (24.3 source, `Db Multi Table/Ledgers.cs`).
/// The shipped 25.3 assembly still contains the `Ledgers`, `Adjustment`,
/// `PaySplit`, `Claim` and `ClaimStatus` types (REA evidence
/// `ev_fc318f31f8ebbc27ada18860f5417e2658fb971dae37779767261af3a8e27e41`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AccountSummary {
    /// Completed procedures, `ProcFee * (UnitQty + BaseUnits)` in OD.
    pub charges: f64,
    /// Signed adjustment amounts; positive increases the balance.
    pub adjustments: f64,
    /// Patient payments from `paysplit`, always a credit.
    pub patient_payments: f64,
    /// Insurance payments and write-offs on received claimprocs, a credit.
    pub insurance_paid: f64,
    /// Insurance estimate on unreceived claimprocs, shown separately by OD.
    pub insurance_estimate: f64,
    /// Amount the patient owes. Negative means a credit balance.
    pub balance: f64,
}

/// Compute the account summary using OD's transaction classes:
/// `completed procedures + adjustments - paysplits - received insurance`.
/// Insurance estimates are reported but deliberately not subtracted from the
/// balance, matching the `InsEst` column of the OD aging query.
pub fn account_summary(
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[PaySplit],
    claim_procs: &[ClaimProc],
) -> AccountSummary {
    let charges = procedures
        .iter()
        .filter(|p| p.proc_status.is_completed())
        .map(|p| p.fee)
        .sum();
    let adjustments = adjustments.iter().map(|a| a.adj_amount).sum();
    let patient_payments = splits.iter().map(|p| p.split_amount).sum();

    let mut insurance_paid = 0.0;
    let mut insurance_estimate = 0.0;
    for cp in claim_procs {
        if cp.status != 0 {
            // OD: `CASE WHEN cp.Status != 0 THEN -cp.InsPayAmt - cp.WriteOff`.
            insurance_paid += cp.ins_pay_amt + cp.write_off;
        } else {
            // NotReceived claimprocs contribute only to OD's InsPayEst/InsWoEst.
            insurance_estimate += cp.ins_pay_est + cp.write_off;
        }
    }

    AccountSummary {
        charges,
        adjustments,
        patient_payments,
        insurance_paid,
        insurance_estimate,
        balance: charges + adjustments - patient_payments - insurance_paid,
    }
}

/// Patient-only balance for practices that do not use insurance (e.g. Ethiopia,
/// where payments and payment plans are used instead). Claimprocs are ignored
/// and the insurance fields are zeroed so they never reach the UI.
pub fn account_summary_without_insurance(
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[PaySplit],
) -> AccountSummary {
    let mut summary = account_summary(procedures, adjustments, splits, &[]);
    summary.insurance_paid = 0.0;
    summary.insurance_estimate = 0.0;
    summary.balance = summary.charges + summary.adjustments - summary.patient_payments;
    summary
}

/// Check that a payment's allocations sum to its header amount, as OD's
/// `paysplit` model requires. The GUI calls this before writing so a split
/// cannot silently drop money.
pub fn validate_payment_allocations(
    payment: &Payment,
    allocations: &[PaymentAllocation],
) -> Result<(), String> {
    payment.validate_allocations(allocations)
}

/// Allocate a payment amount across due payment-plan charges (OD-style).
/// Applies to oldest due debits first, interest before principal on each charge.
/// Returns plan-charge allocations summing to `amount`; any remainder is left as
/// an unallocated plan-level split if no due charge can take it.
pub fn allocate_plan_payment(
    plan_num: Id,
    amount: f64,
    charges: &[PayPlanCharge],
    existing_splits: &[PaySplit],
    as_of: chrono::NaiveDate,
) -> Vec<PaymentAllocation> {
    if !amount.is_finite() || amount <= 0.0 {
        return vec![];
    }
    let mut remaining = (amount * 100.0).round() / 100.0;
    let mut due: Vec<&PayPlanCharge> = charges
        .iter()
        .filter(|c| c.pay_plan_num == plan_num && c.charge_type == PayPlanChargeType::Debit && c.charge_date <= as_of)
        .collect();
    due.sort_by_key(|c| (c.charge_date, c.pay_plan_charge_num));

    let mut out = Vec::new();
    for charge in due {
        if remaining <= 0.005 {
            break;
        }
        let already_paid: f64 = existing_splits
            .iter()
            .filter(|s| s.pay_plan_charge_num == charge.pay_plan_charge_num)
            .map(|s| s.split_amount)
            .sum();
        let interest_due = (charge.interest - existing_splits.iter().filter(|s| s.pay_plan_charge_num == charge.pay_plan_charge_num && s.pay_plan_debit_type == PayPlanDebitType::Interest).map(|s| s.split_amount).sum::<f64>()).max(0.0);
        let principal_due = (charge.principal - existing_splits.iter().filter(|s| s.pay_plan_charge_num == charge.pay_plan_charge_num && s.pay_plan_debit_type == PayPlanDebitType::Principal).map(|s| s.split_amount).sum::<f64>()).max(0.0);
        // If split has no debit type, fall back to total remaining on charge.
        let typed_paid = existing_splits.iter().filter(|s| s.pay_plan_charge_num == charge.pay_plan_charge_num && s.pay_plan_debit_type != PayPlanDebitType::Unknown).map(|s| s.split_amount).sum::<f64>();
        let untyped_paid = (already_paid - typed_paid).max(0.0);
        let total_due = (charge.principal + charge.interest - already_paid).max(0.0);
        if total_due <= 0.005 {
            continue;
        }
        // Apply untyped paid proportionally by reducing interest first then principal.
        let mut int_rem = interest_due;
        let mut prin_rem = principal_due;
        if untyped_paid > 0.005 {
            let take_int = untyped_paid.min(int_rem);
            int_rem = (int_rem - take_int).max(0.0);
            let take_prin = (untyped_paid - take_int).min(prin_rem);
            prin_rem = (prin_rem - take_prin).max(0.0);
        }
        let _ = total_due; // computed for clarity

        let take_int = remaining.min(int_rem);
        if take_int > 0.005 {
            let amt = (take_int * 100.0).round() / 100.0;
            out.push(PaymentAllocation {
                target: PaymentTarget::PayPlanCharge { plan_num, charge_num: charge.pay_plan_charge_num },
                amount: amt,
                pay_plan_debit_type: PayPlanDebitType::Interest,
                ..Default::default()
            });
            remaining = ((remaining - amt) * 100.0).round() / 100.0;
        }
        let take_prin = remaining.min(prin_rem);
        if take_prin > 0.005 {
            let amt = (take_prin * 100.0).round() / 100.0;
            out.push(PaymentAllocation {
                target: PaymentTarget::PayPlanCharge { plan_num, charge_num: charge.pay_plan_charge_num },
                amount: amt,
                pay_plan_debit_type: PayPlanDebitType::Principal,
                ..Default::default()
            });
            remaining = ((remaining - amt) * 100.0).round() / 100.0;
        }
    }
    if remaining > 0.005 {
        out.push(PaymentAllocation {
            target: PaymentTarget::PayPlan(plan_num),
            amount: remaining,
            ..Default::default()
        });
    }
    out
}

/// Amount still owing on a payment plan as of `as_of`.
///
/// Mirrors OD's account view: charges due on or before `as_of` affect the due-now
/// amount, and paysplits with `PayPlanNum` are credits. Payments are applied in
/// payment date order (`date_pay`, then `split_num`) so backdated payments do not
/// erase later-due charges that were not yet due at the payment date.
/// `DownPayment` is not subtracted here because OD records it as a paysplit.
/// Credit charges (production moved onto the plan) reduce the amount due.
pub fn pay_plan_balance(
    plan: &PayPlan,
    charges: &[PayPlanCharge],
    splits: &[PaySplit],
    as_of: chrono::NaiveDate,
) -> f64 {
    let b = pay_plan_balance_breakdown(plan, charges, splits, as_of);
    b.total_balance - b.unapplied_credit
}

/// Breakdown of payment-plan amounts for clear billing visibility.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PayPlanBalanceBreakdown {
    /// Charges/credits due on or before `as_of`, minus payments effectively applied by `as_of`.
    pub due_now: f64,
    /// Charges due after `as_of` (future schedule), not yet due.
    pub future_due: f64,
    /// Payments applied beyond total plan charges as of `as_of`; credit/overpaid on plan.
    pub unapplied_credit: f64,
    /// Net plan balance as of `as_of`: max(due_now + future_due - unapplied_credit, 0).
    pub total_balance: f64,
}

fn round_money(amount: f64) -> f64 {
    (amount * 100.0).round() / 100.0
}

/// Compute due-now / future-due / unapplied-credit for a payment plan.
///
/// Payments are applied chronologically. A payment can only reduce due-now up to
/// charges that were due on/before that payment date; any remaining amount reduces
/// future-due. This prevents backdated payments from incorrectly wiping later-due
/// charges in "due now" and keeps partial-period reconciliation stable.
pub fn pay_plan_balance_breakdown(
    plan: &PayPlan,
    charges: &[PayPlanCharge],
    splits: &[PaySplit],
    as_of: chrono::NaiveDate,
) -> PayPlanBalanceBreakdown {
    let mut due_now_charges = 0.0_f64;
    let mut future_due_charges = 0.0_f64;
    for c in charges.iter().filter(|c| c.pay_plan_num == plan.pay_plan_num) {
        let amt = match c.charge_type {
            PayPlanChargeType::Debit => c.principal + c.interest,
            PayPlanChargeType::Credit => -c.principal,
        };
        if c.charge_date <= as_of {
            due_now_charges += amt;
        } else {
            future_due_charges += amt;
        }
    }

    let mut plan_splits: Vec<&PaySplit> = splits
        .iter()
        .filter(|s| {
            s.pay_plan_num == plan.pay_plan_num
                && (s.date_pay == chrono::NaiveDate::default() || s.date_pay.year() <= 1 || s.date_pay <= as_of)
        })
        .collect();
    plan_splits.sort_by_key(|s| (s.date_pay, s.split_num));

    let mut due_now_paid = 0.0_f64;
    let mut future_paid = 0.0_f64;
    for split in plan_splits {
        let mut amt = split.split_amount;
        if amt.abs() <= 0.000_005 {
            continue;
        }
        if amt > 0.0 {
            // Default/zero dates (legacy/test rows) are treated as as_of.
            // Backdated payments can satisfy charges that were due on/before payment date,
            // then charges that became due by as_of, then future charges.
            let effective_date = if split.date_pay == chrono::NaiveDate::default() || split.date_pay.year() <= 1 { as_of } else { split.date_pay };
            let due_at_payment: f64 = charges
                .iter()
                .filter(|c| c.pay_plan_num == plan.pay_plan_num && c.charge_date <= effective_date.min(as_of))
                .map(|c| match c.charge_type {
                    PayPlanChargeType::Debit => c.principal + c.interest,
                    PayPlanChargeType::Credit => -c.principal,
                })
                .sum();
            let due_now_as_of: f64 = charges
                .iter()
                .filter(|c| c.pay_plan_num == plan.pay_plan_num && c.charge_date <= as_of)
                .map(|c| match c.charge_type {
                    PayPlanChargeType::Debit => c.principal + c.interest,
                    PayPlanChargeType::Credit => -c.principal,
                })
                .sum();
            let can_apply_due_at_payment = (due_at_payment - due_now_paid).max(0.0);
            let apply_due_at_payment = amt.min(can_apply_due_at_payment);
            due_now_paid += apply_due_at_payment;
            amt -= apply_due_at_payment;

            if amt > 0.000_005 {
                let can_apply_due_now_as_of = (due_now_as_of - due_now_paid).max(0.0);
                let apply_due_now_as_of = amt.min(can_apply_due_now_as_of);
                due_now_paid += apply_due_now_as_of;
                amt -= apply_due_now_as_of;
            }
            if amt > 0.000_005 {
                future_paid += amt;
            }
        } else {
            // Negative split/refund increases balance; apply to due-now first.
            let refund = -amt;
            let restore_future = refund.min(future_paid.max(0.0));
            future_paid -= restore_future;
            let remaining_refund = refund - restore_future;
            if remaining_refund > 0.000_005 {
                due_now_paid -= remaining_refund;
            }
        }
    }

    let raw_due_now = due_now_charges - due_now_paid;
    let raw_future = future_due_charges - future_paid;
    let due_now = raw_due_now.max(0.0);
    let future_due = raw_future.max(0.0);
    let unapplied_credit = (-raw_due_now).max(0.0) + (-raw_future).max(0.0);
    let total_balance = (due_now + future_due).max(0.0);

    PayPlanBalanceBreakdown {
        due_now: round_money(due_now),
        future_due: round_money(future_due),
        unapplied_credit: round_money(unapplied_credit),
        total_balance: round_money(total_balance),
    }
}

/// Amount currently due on a payment plan as of `as_of`: debits/credits due
/// minus payments already applied to the plan. Negative when overpaid.
pub fn pay_plan_due_now(
    plan: &PayPlan,
    charges: &[PayPlanCharge],
    splits: &[PaySplit],
    as_of: chrono::NaiveDate,
) -> f64 {
    let b = pay_plan_balance_breakdown(plan, charges, splits, as_of);
    b.due_now - b.unapplied_credit
}

/// Reason a payment plan cannot be closed cleanly. Closing with remaining
/// balance or unapplied credit would hide real billing state.
pub fn pay_plan_close_block_reason(
    plan: &PayPlan,
    charges: &[PayPlanCharge],
    splits: &[PaySplit],
    as_of: chrono::NaiveDate,
) -> Option<String> {
    let b = pay_plan_balance_breakdown(plan, charges, splits, as_of);
    if b.due_now > 0.005 {
        return Some(format!(
            "Payment plan has {:.2} due now; collect/apply payment or adjust before closing.",
            b.due_now
        ));
    }
    if b.future_due > 0.005 {
        return Some(format!(
            "Payment plan has {:.2} future due; settle or adjust remaining scheduled charges before closing.",
            b.future_due
        ));
    }
    if b.unapplied_credit > 0.005 {
        return Some(format!(
            "Payment plan has {:.2} unapplied/overpaid credit; resolve it (transfer/refund/adjustment) before closing.",
            b.unapplied_credit
        ));
    }
    let _ = plan;
    None
}

/// Build a balanced zero-amount transfer that moves unapplied plan credit to
/// unallocated patient account (or to a procedure/plan target chosen by caller).
/// This is an explicit resolution action; it does not hide the credit silently.
pub fn build_unapplied_plan_credit_transfer(
    plan: &PayPlan,
    amount: f64,
    as_of: chrono::NaiveDate,
    to_target: PaymentTarget,
) -> Result<(Payment, Vec<PaymentAllocation>), String> {
    if !amount.is_finite() || amount <= 0.005 {
        return Err("No unapplied plan credit to transfer".into());
    }
    let target_label = match to_target {
        PaymentTarget::Procedure(proc_num) => format!("procedure #{}", proc_num),
        PaymentTarget::PayPlan(pn) => format!("plan #{}", pn),
        PaymentTarget::PayPlanCharge { plan_num, charge_num } => format!("plan #{} charge #{}", plan_num, charge_num),
        PaymentTarget::Unallocated => "patient account (unallocated)".to_string(),
    };
    let payment = Payment {
        pay_num: 0,
        pat_num: plan.pat_num,
        pay_date: as_of,
        pay_amount: 0.0,
        pay_type: 0,
        check_num: String::new(),
        note: format!("Transfer unapplied credit from plan #{} to {}", plan.pay_plan_num, target_label),
    };
    let allocations = vec![
        PaymentAllocation {
            target: PaymentTarget::PayPlan(plan.pay_plan_num),
            amount: -amount,
            ..Default::default()
        },
        PaymentAllocation {
            target: to_target,
            amount,
            ..Default::default()
        },
    ];
    payment.validate_allocations(&allocations)?;
    Ok((payment, allocations))
}

/// One line on a patient statement, mirroring the Account ledger.
#[derive(Clone, Debug)]
pub struct StatementLine {
    pub date: chrono::NaiveDate,
    pub kind: String,
    pub description: String,
    pub charge: f64,
    pub credit: f64,
}

/// Print-ready content for one patient refund/transfer receipt.
#[derive(Clone, Debug)]
pub struct RefundTransferReceipt {
    pub pat_num: Id,
    pub patient_name: String,
    pub payment_num: Id,
    pub pay_date: chrono::NaiveDate,
    pub generated_on: chrono::NaiveDate,
    pub note: String,
    pub recorded_by: String,
    pub lines: Vec<RefundTransferReceiptLine>,
    pub total_moved: f64,
}

/// One line on a refund/transfer receipt.
#[derive(Clone, Debug)]
pub struct RefundTransferReceiptLine {
    pub date: chrono::NaiveDate,
    pub direction: String,
    pub source: String,
    pub target: String,
    pub amount: f64,
    pub payment_num: Id,
    pub plan_num: Id,
    pub charge_num: Id,
    pub note: String,
}

/// Print-ready content for one patient statement.
#[derive(Clone, Debug)]
pub struct StatementDocument {
    pub pat_num: Id,
    pub patient_name: String,
    pub generated_on: chrono::NaiveDate,
    pub lines: Vec<StatementLine>,
    pub charges: f64,
    pub adjustments: f64,
    pub payments: f64,
    pub balance: f64,
    pub plan_balances: Vec<(String, f64)>,
    pub note: String,
}

/// Build a patient-only statement from the same data the Account view uses.
/// Only completed procedures become charge lines so the line total ties to
/// `summary.balance`; insurance columns are deliberately absent.
pub fn build_statement_document(
    pat_num: Id,
    patient_name: &str,
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[PaySplit],
    plans: &[PayPlan],
    plan_charges: &[PayPlanCharge],
    summary: &AccountSummary,
    as_of: chrono::NaiveDate,
    note: &str,
) -> StatementDocument {
    let mut lines: Vec<StatementLine> = procedures
        .iter()
        .filter(|p| p.proc_status.is_completed())
        .map(|p| StatementLine {
            date: p.proc_date,
            kind: "Procedure".into(),
            description: format!("Code #{} tooth {}", p.code_num, p.tooth_num),
            charge: p.fee,
            credit: 0.0,
        })
        .collect();
    lines.extend(adjustments.iter().map(|a| StatementLine {
        date: a.adj_date,
        kind: "Adjustment".into(),
        description: if a.note.is_empty() {
            format!("Adjustment type {}", a.adj_type)
        } else {
            a.note.clone()
        },
        charge: a.adj_amount.max(0.0),
        credit: (-a.adj_amount).max(0.0),
    }));
    lines.extend(splits.iter().map(|sp| StatementLine {
        date: sp.date_pay,
        kind: "Payment".into(),
        description: if sp.proc_num == 0 {
            "Unallocated payment".into()
        } else {
            format!("Toward procedure #{}", sp.proc_num)
        },
        charge: 0.0,
        credit: sp.split_amount,
    }));
    lines.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.kind.cmp(&b.kind)));

    let plan_balances = plans
        .iter()
        .map(|plan| {
            let label = if plan.note.is_empty() {
                format!("Plan #{}", plan.pay_plan_num)
            } else {
                plan.note.clone()
            };
            (label, pay_plan_balance(plan, plan_charges, splits, as_of))
        })
        .collect();

    StatementDocument {
        pat_num,
        patient_name: patient_name.to_string(),
        generated_on: as_of,
        lines,
        charges: summary.charges,
        adjustments: summary.adjustments,
        payments: summary.patient_payments,
        balance: summary.balance,
        plan_balances,
        note: note.to_string(),
    }
}

fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn target_label(target: PaymentTarget) -> String {
    match target {
        PaymentTarget::Procedure(proc_num) => format!("Procedure #{}", proc_num),
        PaymentTarget::PayPlan(plan_num) => format!("Plan #{}", plan_num),
        PaymentTarget::PayPlanCharge { plan_num, charge_num } => {
            format!("Plan #{} charge #{}", plan_num, charge_num)
        }
        PaymentTarget::Unallocated => "Patient account (unallocated)".to_string(),
    }
}

/// Build a print-ready refund/transfer receipt from a recorded payment and its splits.
pub fn build_refund_transfer_receipt(
    pat_num: Id,
    patient_name: &str,
    payment: &Payment,
    splits: &[PaySplit],
    recorded_by: &str,
    generated_on: chrono::NaiveDate,
) -> Option<RefundTransferReceipt> {
    let mut receipt_splits: Vec<&PaySplit> = splits
        .iter()
        .filter(|s| s.pay_num == payment.pay_num && s.split_amount < -0.005)
        .collect();
    receipt_splits.sort_by_key(|s| (s.date_pay, s.split_num));
    if receipt_splits.is_empty() {
        return None;
    }

    let mut lines = Vec::new();
    let mut total_moved = 0.0;
    for split in &receipt_splits {
        let amount = (-split.split_amount).max(0.0);
        total_moved += amount;
        let counterpart = splits.iter().find(|o| {
            o.pay_num == split.pay_num
                && o.split_num != split.split_num
                && o.split_amount > 0.005
        });
        let target = counterpart
            .map(|o| {
                if o.proc_num != 0 {
                    PaymentTarget::Procedure(o.proc_num)
                } else if o.pay_plan_num != 0 && o.pay_plan_num != split.pay_plan_num {
                    PaymentTarget::PayPlan(o.pay_plan_num)
                } else if o.pay_plan_charge_num != 0 {
                    PaymentTarget::PayPlanCharge {
                        plan_num: o.pay_plan_num,
                        charge_num: o.pay_plan_charge_num,
                    }
                } else {
                    PaymentTarget::Unallocated
                }
            })
            .unwrap_or(PaymentTarget::Unallocated);
        let source = if split.pay_plan_charge_num != 0 {
            format!("Plan #{} charge #{}", split.pay_plan_num, split.pay_plan_charge_num)
        } else if split.pay_plan_num != 0 {
            format!("Plan #{}", split.pay_plan_num)
        } else if split.proc_num != 0 {
            format!("Procedure #{}", split.proc_num)
        } else {
            "Patient account (unallocated)".to_string()
        };
        let direction = format!("{} -> {}", source, target_label(target));
        lines.push(RefundTransferReceiptLine {
            date: split.date_pay,
            direction,
            source,
            target: target_label(target),
            amount,
            payment_num: split.pay_num,
            plan_num: split.pay_plan_num,
            charge_num: split.pay_plan_charge_num,
            note: payment.note.clone(),
        });
    }

    Some(RefundTransferReceipt {
        pat_num,
        patient_name: patient_name.to_string(),
        payment_num: payment.pay_num,
        pay_date: payment.pay_date,
        generated_on,
        note: payment.note.clone(),
        recorded_by: recorded_by.to_string(),
        lines,
        total_moved,
    })
}

/// Render a refund/transfer receipt as a self-contained HTML page.
pub fn render_refund_transfer_receipt_html(doc: &RefundTransferReceipt) -> String {
    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\n");
    html.push_str("<title>Refund/Transfer Receipt - ");
    html.push_str(&escape_html(&doc.patient_name));
    html.push_str("</title>\n<style>\n");
    html.push_str("body{font-family:Arial,sans-serif;margin:32px;color:#222}\n");
    html.push_str("h1{font-size:20px;margin:0 0 4px}\n");
    html.push_str("h2{font-size:14px;margin:24px 0 8px}\n");
    html.push_str(".muted{color:#666;font-size:12px}\n");
    html.push_str("table{border-collapse:collapse;width:100%;font-size:13px}\n");
    html.push_str("th,td{border-bottom:1px solid #ddd;padding:6px 8px;text-align:left;vertical-align:top}\n");
    html.push_str("th{background:#f4f4f4}\n");
    html.push_str(".num{text-align:right;font-variant-numeric:tabular-nums}\n");
    html.push_str(".total{font-weight:bold;font-size:15px}\n");
    html.push_str("@media print{button{display:none}}\n");
    html.push_str("</style></head>\n<body>\n");
    html.push_str("<button onclick=\"window.print()\">Print receipt</button>\n");
    html.push_str("<h1>Refund / Transfer Receipt - ");
    html.push_str(&escape_html(&doc.patient_name));
    html.push_str("</h1>\n");
    html.push_str(&format!(
        "<div class=\"muted\">Patient #{} &middot; Payment #{} &middot; Date {} &middot; Generated {}</div>\n",
        doc.pat_num, doc.payment_num, doc.pay_date, doc.generated_on
    ));
    if !doc.recorded_by.trim().is_empty() {
        html.push_str(&format!("<div class=\"muted\">Recorded by: {}</div>\n", escape_html(&doc.recorded_by)));
    }
    if !doc.note.trim().is_empty() {
        html.push_str("<h2>Reason / note</h2><p>");
        html.push_str(&escape_html(&doc.note));
        html.push_str("</p>\n");
    }
    html.push_str("<h2>Details</h2>\n<table><thead><tr><th>Date</th><th>Direction</th><th>Source</th><th>Target</th><th class=\"num\">Amount</th><th>Payment</th><th>Plan</th><th>Charge</th></tr></thead><tbody>\n");
    for line in &doc.lines {
        html.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"num\">{:.2}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            line.date,
            escape_html(&line.direction),
            escape_html(&line.source),
            escape_html(&line.target),
            line.amount,
            if line.payment_num == 0 { String::new() } else { line.payment_num.to_string() },
            if line.plan_num == 0 { String::new() } else { line.plan_num.to_string() },
            if line.charge_num == 0 { String::new() } else { line.charge_num.to_string() },
        ));
    }
    html.push_str("</tbody></table>\n");
    html.push_str(&format!("<p class=\"total\">Total moved/refunded: {:.2}</p>\n", doc.total_moved));
    html.push_str("</body></html>\n");
    html
}

/// Render a refund/transfer receipt as PDF bytes.
pub fn render_refund_transfer_receipt_pdf(doc: &RefundTransferReceipt) -> Vec<u8> {
    crate::pdf::render_refund_transfer_receipt_pdf(doc)
}

/// Render a statement as a self-contained HTML page. A print button calls
/// `window.print()`, so opening the file in a browser gives a print preview.
pub fn render_statement_html(doc: &StatementDocument) -> String {
    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\n");
    html.push_str("<title>Statement - ");
    html.push_str(&escape_html(&doc.patient_name));
    html.push_str("</title>\n<style>\n");
    html.push_str("body{font-family:Arial,sans-serif;margin:32px;color:#222}\n");
    html.push_str("h1{font-size:20px;margin:0 0 4px}\n");
    html.push_str("h2{font-size:14px;margin:24px 0 8px}\n");
    html.push_str(".muted{color:#666;font-size:12px}\n");
    html.push_str("table{border-collapse:collapse;width:100%;font-size:13px}\n");
    html.push_str("th,td{border-bottom:1px solid #ddd;padding:6px 8px;text-align:left}\n");
    html.push_str("th{background:#f4f4f4}\n");
    html.push_str(".num{text-align:right;font-variant-numeric:tabular-nums}\n");
    html.push_str(".total{font-weight:bold;font-size:15px}\n");
    html.push_str("@media print{button{display:none}}\n");
    html.push_str("</style></head>\n<body>\n");
    html.push_str("<button onclick=\"window.print()\">Print statement</button>\n");
    html.push_str("<h1>Statement - ");
    html.push_str(&escape_html(&doc.patient_name));
    html.push_str("</h1>\n");
    html.push_str(&format!(
        "<div class=\"muted\">Patient #{} &middot; Generated {}</div>\n",
        doc.pat_num, doc.generated_on
    ));
    html.push_str("<h2>Account activity</h2>\n<table><thead><tr><th>Date</th><th>Type</th><th>Description</th><th class=\"num\">Charge</th><th class=\"num\">Credit</th></tr></thead><tbody>\n");
    for line in &doc.lines {
        html.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"num\">{:.2}</td><td class=\"num\">{:.2}</td></tr>\n",
            line.date,
            escape_html(&line.kind),
            escape_html(&line.description),
            line.charge,
            line.credit
        ));
    }
    html.push_str("</tbody></table>\n");
    html.push_str(&format!(
        "<p>Charges {:.2} &middot; Adjustments {:.2} &middot; Payments {:.2}</p>\n",
        doc.charges, doc.adjustments, doc.payments
    ));
    html.push_str(&format!(
        "<p class=\"total\">Balance due: {:.2}</p>\n",
        doc.balance
    ));
    if !doc.plan_balances.is_empty() {
        html.push_str("<h2>Payment plans</h2>\n<table><thead><tr><th>Plan</th><th class=\"num\">Owing</th></tr></thead><tbody>\n");
        for (label, balance) in &doc.plan_balances {
            html.push_str(&format!(
                "<tr><td>{}</td><td class=\"num\">{:.2}</td></tr>\n",
                escape_html(label),
                balance
            ));
        }
        html.push_str("</tbody></table>\n");
    }
    if !doc.note.trim().is_empty() {
        html.push_str("<h2>Note</h2><p>");
        html.push_str(&escape_html(&doc.note));
        html.push_str("</p>\n");
    }
    html.push_str("</body></html>\n");
    html
}

/// Total production by provider for a date range — the basis of the
/// production reports in the Reports module.
pub fn production_by_provider(
    procedures: &[Procedure],
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> Vec<(Id, f64)> {
    let mut totals: Vec<(Id, f64)> = Vec::new();
    for proc in procedures
        .iter()
        .filter(|p| p.proc_status.is_completed() && p.proc_date >= from && p.proc_date <= to)
    {
        if let Some(entry) = totals.iter_mut().find(|(id, _)| *id == proc.provider_num) {
            entry.1 += proc.fee;
        } else {
            totals.push((proc.provider_num, proc.fee));
        }
    }
    totals.sort_by(|a, b| b.1.total_cmp(&a.1));
    totals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payment() -> Payment {
        Payment {
            pay_amount: 100.0,
            ..Default::default()
        }
    }

    #[test]
    fn refund_transfer_receipt_builds_html_and_pdf() {
        let payment = Payment {
            pay_num: 77,
            pat_num: 1,
            pay_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap(),
            pay_amount: 0.0,
            note: "Plan credit moved to account".into(),
            ..Default::default()
        };
        let splits = vec![
            PaySplit {
                split_num: 1,
                pay_num: 77,
                pat_num: 1,
                pay_plan_num: 9,
                split_amount: -25.0,
                date_pay: chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap(),
                ..Default::default()
            },
            PaySplit {
                split_num: 2,
                pay_num: 77,
                pat_num: 1,
                proc_num: 0,
                pay_plan_num: 0,
                split_amount: 25.0,
                date_pay: chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap(),
                ..Default::default()
            },
        ];
        let doc = build_refund_transfer_receipt(
            1,
            "Ada A",
            &payment,
            &splits,
            "admin",
            chrono::NaiveDate::from_ymd_opt(2026, 1, 6).unwrap(),
        )
        .expect("receipt");
        assert_eq!(doc.payment_num, 77);
        assert_eq!(doc.lines.len(), 1);
        assert!((doc.total_moved - 25.0).abs() < 0.01);
        assert!(doc.lines[0].direction.contains("Plan #9"));
        assert!(doc.lines[0].target.contains("Patient account"));
        let html = render_refund_transfer_receipt_html(&doc);
        assert!(html.contains("Refund / Transfer Receipt"));
        assert!(html.contains("Plan credit moved to account"));
        assert!(html.contains("Total moved/refunded: 25.00"));
        let pdf = render_refund_transfer_receipt_pdf(&doc);
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(pdf.ends_with(b"%%EOF\n"));
    }

    #[test]
    fn payment_allocations_must_sum_to_the_payment() {
        let payment = payment();
        let allocations = [
            PaymentAllocation {
                target: PaymentTarget::Procedure(7),
                amount: 60.0,
                ..Default::default()
            },
            PaymentAllocation {
                target: PaymentTarget::PayPlan(9),
                amount: 40.0,
                ..Default::default()
            },
        ];
        assert!(validate_payment_allocations(&payment, &allocations).is_ok());

        let splits = payment.splits(&allocations).unwrap();
        assert_eq!(splits.len(), 2);
        assert_eq!(splits[0].proc_num, 7);
        assert_eq!(splits[0].pay_plan_num, 0);
        assert_eq!(splits[1].proc_num, 0);
        assert_eq!(splits[1].pay_plan_num, 9);
        let total: f64 = splits.iter().map(|s| s.split_amount).sum();
        assert_eq!(total, 100.0);
    }

    #[test]
    fn payment_allocation_validation_rejects_bad_totals_and_empty_rows() {
        let payment = payment();
        assert!(validate_payment_allocations(&payment, &[]).is_err());
        assert!(validate_payment_allocations(
            &payment,
            &[PaymentAllocation {
                amount: 99.0,
                ..Default::default()
            }]
        )
        .is_err());
        assert!(validate_payment_allocations(
            &payment,
            &[PaymentAllocation {
                amount: 0.0,
                ..Default::default()
            }]
        )
        .is_err());
        assert!(validate_payment_allocations(
            &payment,
            &[PaymentAllocation {
                amount: f64::NAN,
                ..Default::default()
            }]
        )
        .is_err());
    }

    #[test]
    fn statement_document_ties_to_the_account_balance() {
        let procedures = [
            Procedure {
                fee: 100.0,
                proc_status: ProcedureStatus::Complete,
                ..Default::default()
            },
            Procedure {
                fee: 500.0,
                proc_status: ProcedureStatus::TreatmentPlanned,
                ..Default::default()
            },
        ];
        let adjustments = [Adjustment {
            adj_amount: -20.0,
            ..Default::default()
        }];
        let splits = [PaySplit {
            split_amount: 30.0,
            ..Default::default()
        }];
        let summary = account_summary_without_insurance(&procedures, &adjustments, &splits);
        let doc = build_statement_document(
            5,
            "Test Patient",
            &procedures,
            &adjustments,
            &splits,
            &[],
            &[],
            &summary,
            chrono::NaiveDate::from_ymd_opt(2026, 11, 21).unwrap(),
            "Please pay at reception",
        );

        // Only the completed procedure is a charge line; the planned one is
        // excluded so the lines tie to the balance the Account view shows.
        assert_eq!(doc.lines.len(), 3);
        assert_eq!(doc.charges, 100.0);
        assert_eq!(doc.adjustments, -20.0);
        assert_eq!(doc.payments, 30.0);
        assert_eq!(doc.balance, 50.0);
    }

    #[test]
    fn statement_html_renders_content_and_escapes_text() {
        let doc = StatementDocument {
            pat_num: 5,
            patient_name: "Ada <O'Brien> & Co".into(),
            generated_on: chrono::NaiveDate::from_ymd_opt(2026, 11, 21).unwrap(),
            lines: vec![StatementLine {
                date: chrono::NaiveDate::from_ymd_opt(2026, 11, 20).unwrap(),
                kind: "Procedure".into(),
                description: "Cleaning".into(),
                charge: 100.0,
                credit: 0.0,
            }],
            charges: 100.0,
            adjustments: 0.0,
            payments: 25.0,
            balance: 75.0,
            plan_balances: vec![("Plan #1".into(), 40.0)],
            note: "Pay <soon>".into(),
        };
        let html = render_statement_html(&doc);
        assert!(html.contains("Ada &lt;O&#39;Brien&gt; &amp; Co"));
        assert!(html.contains("Balance due: 75.00"));
        assert!(html.contains("Plan #1"));
        assert!(html.contains("Pay &lt;soon&gt;"));
        assert!(html.contains("window.print()"));
    }

    fn proc(fee: f64, status: ProcedureStatus) -> Procedure {
        Procedure {
            fee,
            proc_status: status,
            ..Default::default()
        }
    }

    fn adj(amount: f64) -> Adjustment {
        Adjustment {
            adj_amount: amount,
            adj_date: chrono::NaiveDate::from_ymd_opt(2026, 11, 21).unwrap(),
            ..Default::default()
        }
    }

    fn split(amount: f64) -> PaySplit {
        PaySplit {
            split_amount: amount,
            ..Default::default()
        }
    }

    #[test]
    fn account_summary_combines_od_transaction_classes() {
        let procedures = vec![
            proc(100.0, ProcedureStatus::Complete),
            proc(50.0, ProcedureStatus::Complete),
            proc(999.0, ProcedureStatus::TreatmentPlanned),
        ];
        let adjustments = vec![adj(-20.0), adj(5.0)];
        let splits = vec![split(30.0), split(10.0)];
        let claim_procs = vec![ClaimProc {
            status: 1,
            ins_pay_amt: 40.0,
            write_off: 10.0,
            ..Default::default()
        }];

        let summary = account_summary(&procedures, &adjustments, &splits, &claim_procs);
        assert_eq!(summary.charges, 150.0);
        assert_eq!(summary.adjustments, -15.0);
        assert_eq!(summary.patient_payments, 40.0);
        assert_eq!(summary.insurance_paid, 50.0);
        assert_eq!(summary.insurance_estimate, 0.0);
        assert_eq!(summary.balance, 45.0);
    }

    #[test]
    fn insurance_disabled_ignores_claimprocs_entirely() {
        let claim_procs = vec![ClaimProc {
            status: 1,
            ins_pay_amt: 90.0,
            write_off: 10.0,
            ins_pay_est: 25.0,
            ..Default::default()
        }];
        let with_insurance = account_summary(
            &[proc(200.0, ProcedureStatus::Complete)],
            &[adj(-10.0)],
            &[split(20.0)],
            &claim_procs,
        );
        let patient_only = account_summary_without_insurance(
            &[proc(200.0, ProcedureStatus::Complete)],
            &[adj(-10.0)],
            &[split(20.0)],
        );

        assert_eq!(with_insurance.balance, 70.0);
        assert_eq!(with_insurance.insurance_paid, 100.0);
        assert_eq!(patient_only.balance, 170.0);
        assert_eq!(patient_only.insurance_paid, 0.0);
        assert_eq!(patient_only.insurance_estimate, 0.0);
        assert_eq!(patient_only.charges, with_insurance.charges);
        assert_eq!(patient_only.patient_payments, with_insurance.patient_payments);
    }

    fn plan() -> PayPlan {
        PayPlan {
            pay_plan_num: 9,
            pat_num: 4,
            guarantor: 4,
            pay_plan_date: chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            date_pay_plan_start: Some(chrono::NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()),
            completed_amt: 600.0,
            number_of_payments: 4,
            charge_frequency: PayPlanFrequency::Monthly,
            ..Default::default()
        }
    }

    #[test]
    fn plan_charges_are_even_and_sum_to_completed_amount() {
        let charges = generate_plan_charges(&plan(), chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        assert_eq!(charges.len(), 4);
        assert_eq!(charges[0].charge_date, chrono::NaiveDate::from_ymd_opt(2026, 10, 15).unwrap());
        assert_eq!(charges[1].charge_date, chrono::NaiveDate::from_ymd_opt(2026, 11, 15).unwrap());
        assert_eq!(charges[0].principal, 150.0);
        let total: f64 = charges.iter().map(|c| c.principal).sum();
        assert_eq!(total, 600.0);
        assert!(charges.iter().all(|c| c.interest == 0.0));
        assert!(charges.iter().all(|c| c.pay_plan_num == 9));
    }

    #[test]
    fn plan_balance_ages_charges_and_credits_paysplits() {
        let plan = plan();
        let charges = generate_plan_charges(&plan, chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        let splits = vec![PaySplit {
            pay_plan_num: 9,
            split_amount: 50.0,
            date_pay: chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            ..Default::default()
        }];
        let before = chrono::NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        let after_first = chrono::NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();

        // Before first charge due: payment is unapplied/early credit against future schedule.
        let b_before = pay_plan_balance_breakdown(&plan, &charges, &splits, before);
        assert!((b_before.due_now - 0.0).abs() < 0.01);
        assert!((b_before.future_due - 550.0).abs() < 0.01);
        assert!((b_before.unapplied_credit - 0.0).abs() < 0.01);
        assert!((pay_plan_balance(&plan, &charges, &splits, before) - 550.0).abs() < 0.01);

        let b_after = pay_plan_balance_breakdown(&plan, &charges, &splits, after_first);
        assert!((b_after.due_now - 100.0).abs() < 0.01);
        assert!((pay_plan_balance(&plan, &charges, &splits, after_first) - 550.0).abs() < 0.01);
    }

    #[test]
    fn allocate_plan_payment_applies_interest_then_principal() {
        let charges = vec![PayPlanCharge {
            pay_plan_charge_num: 1,
            pay_plan_num: 9,
            charge_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            principal: 100.0,
            interest: 10.0,
            charge_type: PayPlanChargeType::Debit,
            ..Default::default()
        }];
        let allocs = allocate_plan_payment(9, 50.0, &charges, &[], chrono::NaiveDate::from_ymd_opt(2026, 2, 1).unwrap());
        assert_eq!(allocs.len(), 2);
        assert_eq!(allocs[0].pay_plan_debit_type, PayPlanDebitType::Interest);
        assert!((allocs[0].amount - 10.0).abs() < 0.01);
        assert_eq!(allocs[1].pay_plan_debit_type, PayPlanDebitType::Principal);
        assert!((allocs[1].amount - 40.0).abs() < 0.01);
    }

    #[test]
    fn plan_credit_charges_reduce_the_balance() {
        let plan = plan();
        let charges = vec![PayPlanCharge {
            pay_plan_num: 9,
            charge_date: chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            principal: 600.0,
            charge_type: PayPlanChargeType::Credit,
            ..Default::default()
        }];
        assert_eq!(
            pay_plan_balance(&plan, &charges, &[], chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap()),
            -600.0
        );
    }

    #[test]
    fn insurance_estimate_is_reported_but_not_subtracted() {
        let claim_procs = vec![ClaimProc {
            status: 0,
            ins_pay_est: 70.0,
            write_off: 30.0,
            ..Default::default()
        }];
        let summary = account_summary(&[proc(200.0, ProcedureStatus::Complete)], &[], &[], &claim_procs);
        assert_eq!(summary.insurance_estimate, 100.0);
        assert_eq!(summary.insurance_paid, 0.0);
        assert_eq!(summary.balance, 200.0);
    }

    #[test]
    fn overpayment_produces_a_credit_balance() {
        let summary = account_summary(&[proc(100.0, ProcedureStatus::Complete)], &[], &[split(150.0)], &[]);
        assert_eq!(summary.balance, -50.0);
    }
}

    #[test]
    fn pay_plan_balance_breakdown_separates_due_future_and_credit() {
        let plan = PayPlan { pay_plan_num: 1, ..Default::default() };
        let charges = vec![
            PayPlanCharge { pay_plan_num: 1, charge_date: chrono::NaiveDate::from_ymd_opt(2026,1,1).unwrap(), principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() },
            PayPlanCharge { pay_plan_num: 1, charge_date: chrono::NaiveDate::from_ymd_opt(2026,3,1).unwrap(), principal: 50.0, charge_type: PayPlanChargeType::Debit, ..Default::default() },
        ];
        let splits = vec![PaySplit { pay_plan_num: 1, split_amount: 120.0, date_pay: chrono::NaiveDate::from_ymd_opt(2026,2,1).unwrap(), ..Default::default() }];
        let b = pay_plan_balance_breakdown(&plan, &charges, &splits, chrono::NaiveDate::from_ymd_opt(2026,2,1).unwrap());
        assert!((b.due_now - 0.0).abs() < 0.01);
        assert!((b.future_due - 30.0).abs() < 0.01);
        assert!((b.unapplied_credit - 0.0).abs() < 0.01);
        assert!((b.total_balance - 30.0).abs() < 0.01);
    }

    #[test]
    fn backdated_payment_does_not_hide_later_due_now() {
        let plan = PayPlan { pay_plan_num: 1, ..Default::default() };
        let charges = vec![
            PayPlanCharge { pay_plan_num: 1, charge_date: chrono::NaiveDate::from_ymd_opt(2026,1,1).unwrap(), principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() },
            PayPlanCharge { pay_plan_num: 1, charge_date: chrono::NaiveDate::from_ymd_opt(2026,2,1).unwrap(), principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() },
        ];
        let splits = vec![PaySplit { pay_plan_num: 1, split_amount: 100.0, date_pay: chrono::NaiveDate::from_ymd_opt(2026,1,5).unwrap(), split_num: 2, ..Default::default() }];
        let b = pay_plan_balance_breakdown(&plan, &charges, &splits, chrono::NaiveDate::from_ymd_opt(2026,2,15).unwrap());
        assert!((b.due_now - 100.0).abs() < 0.01);
        assert!((b.future_due - 0.0).abs() < 0.01);
        assert!((b.unapplied_credit - 0.0).abs() < 0.01);
        assert!((b.total_balance - 100.0).abs() < 0.01);
    }

    #[test]
    fn adjustment_credit_after_charges_reduces_due_now() {
        let plan = PayPlan { pay_plan_num: 1, ..Default::default() };
        let charges = vec![
            PayPlanCharge { pay_plan_num: 1, charge_date: chrono::NaiveDate::from_ymd_opt(2026,1,1).unwrap(), principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() },
            PayPlanCharge { pay_plan_num: 1, charge_date: chrono::NaiveDate::from_ymd_opt(2026,1,15).unwrap(), principal: 20.0, charge_type: PayPlanChargeType::Credit, ..Default::default() },
        ];
        let splits = vec![PaySplit { pay_plan_num: 1, split_amount: 50.0, date_pay: chrono::NaiveDate::from_ymd_opt(2026,1,20).unwrap(), ..Default::default() }];
        let b = pay_plan_balance_breakdown(&plan, &charges, &splits, chrono::NaiveDate::from_ymd_opt(2026,1,31).unwrap());
        assert!((b.due_now - 30.0).abs() < 0.01);
        assert!((b.total_balance - 30.0).abs() < 0.01);
    }

    #[test]
    fn partial_period_reconciliation_mid_period_as_of() {
        let plan = PayPlan { pay_plan_num: 1, ..Default::default() };
        let charges = vec![
            PayPlanCharge { pay_plan_num: 1, charge_date: chrono::NaiveDate::from_ymd_opt(2026,1,1).unwrap(), principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() },
            PayPlanCharge { pay_plan_num: 1, charge_date: chrono::NaiveDate::from_ymd_opt(2026,2,1).unwrap(), principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() },
            PayPlanCharge { pay_plan_num: 1, charge_date: chrono::NaiveDate::from_ymd_opt(2026,3,1).unwrap(), principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() },
        ];
        // Payment on Feb 15 applies to due-now through Feb 15 (Jan+Feb), not Mar.
        let splits = vec![PaySplit { pay_plan_num: 1, split_amount: 150.0, date_pay: chrono::NaiveDate::from_ymd_opt(2026,2,15).unwrap(), ..Default::default() }];
        let mid_feb = chrono::NaiveDate::from_ymd_opt(2026,2,15).unwrap();
        let b = pay_plan_balance_breakdown(&plan, &charges, &splits, mid_feb);
        assert!((b.due_now - 50.0).abs() < 0.01);
        assert!((b.future_due - 100.0).abs() < 0.01);
        assert!((b.total_balance - 150.0).abs() < 0.01);
    }

    #[test]
    fn pay_plan_charge_applications_report_lists_charge_and_unapplied() {
        use crate::pay_plan_charge_applications_report;
        let plans = vec![PayPlan { pay_plan_num: 1, pat_num: 1, ..Default::default() }];
        let charges = vec![PayPlanCharge { pay_plan_charge_num: 10, pay_plan_num: 1, charge_date: chrono::NaiveDate::from_ymd_opt(2026,1,1).unwrap(), principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() }];
        let splits = vec![
            PaySplit { pay_plan_num: 1, pay_plan_charge_num: 10, split_amount: 40.0, date_pay: chrono::NaiveDate::from_ymd_opt(2026,1,2).unwrap(), pay_plan_debit_type: PayPlanDebitType::Principal, ..Default::default() },
            PaySplit { pay_plan_num: 1, pay_plan_charge_num: 0, split_amount: 5.0, date_pay: chrono::NaiveDate::from_ymd_opt(2026,1,3).unwrap(), ..Default::default() },
        ];
        let t = pay_plan_charge_applications_report(&plans, &charges, &splits, &[Patient { pat_num: 1, f_name: "Ada".into(), l_name: "A".into(), ..Default::default() }], &[], chrono::NaiveDate::from_ymd_opt(2026,1,1).unwrap(), chrono::NaiveDate::from_ymd_opt(2026,12,31).unwrap(), false);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0][3], "10");
        assert_eq!(t.rows[1][5], "Unapplied");
    }

    #[test]
    fn pay_plan_close_block_reason_reports_due_future_and_credit() {
        let plan = PayPlan { pay_plan_num: 1, ..Default::default() };
        let as_of = chrono::NaiveDate::from_ymd_opt(2026, 2, 1).unwrap();
        let charges = vec![
            PayPlanCharge { pay_plan_num: 1, charge_date: chrono::NaiveDate::from_ymd_opt(2026,1,1).unwrap(), principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() },
            PayPlanCharge { pay_plan_num: 1, charge_date: chrono::NaiveDate::from_ymd_opt(2026,3,1).unwrap(), principal: 50.0, charge_type: PayPlanChargeType::Debit, ..Default::default() },
        ];
        let splits_due = vec![PaySplit { pay_plan_num: 1, split_amount: 20.0, ..Default::default() }];
        assert!(pay_plan_close_block_reason(&plan, &charges, &splits_due, as_of).unwrap().contains("due now"));
        let splits_future = vec![PaySplit { pay_plan_num: 1, split_amount: 100.0, ..Default::default() }];
        assert!(pay_plan_close_block_reason(&plan, &charges, &splits_future, as_of).unwrap().contains("future due"));
        let charges_paid = vec![PayPlanCharge { pay_plan_num: 1, charge_date: chrono::NaiveDate::from_ymd_opt(2026,1,1).unwrap(), principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() }];
        let splits_credit = vec![PaySplit { pay_plan_num: 1, split_amount: 120.0, ..Default::default() }];
        assert!(pay_plan_close_block_reason(&plan, &charges_paid, &splits_credit, as_of).unwrap().contains("unapplied/overpaid credit"));
        let settled_charges = vec![PayPlanCharge { pay_plan_num: 1, charge_date: chrono::NaiveDate::from_ymd_opt(2026,1,1).unwrap(), principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() }];
        let settled_splits = vec![PaySplit { pay_plan_num: 1, split_amount: 100.0, ..Default::default() }];
        assert_eq!(pay_plan_close_block_reason(&plan, &settled_charges, &settled_splits, as_of), None);
    }

    #[test]
    fn build_unapplied_plan_credit_transfer_creates_balanced_zero_payment() {
        let plan = PayPlan { pay_plan_num: 7, pat_num: 3, ..Default::default() };
        let (pay, allocs) = build_unapplied_plan_credit_transfer(&plan, 25.0, chrono::NaiveDate::from_ymd_opt(2026,1,1).unwrap(), PaymentTarget::Unallocated).unwrap();
        assert_eq!(pay.pay_amount, 0.0);
        assert_eq!(allocs.len(), 2);
        assert_eq!(allocs[0].amount, -25.0);
        assert_eq!(allocs[1].amount, 25.0);
        pay.validate_allocations(&allocs).unwrap();
    }

    #[test]
    fn pay_plan_charge_applications_report_includes_note() {
        use crate::pay_plan_charge_applications_report;
        let plans = vec![PayPlan { pay_plan_num: 1, pat_num: 1, ..Default::default() }];
        let charges = vec![PayPlanCharge { pay_plan_charge_num: 10, pay_plan_num: 1, charge_date: chrono::NaiveDate::from_ymd_opt(2026,1,1).unwrap(), principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() }];
        let splits = vec![PaySplit { pay_num: 5, pay_plan_num: 1, pay_plan_charge_num: 0, split_amount: -10.0, date_pay: chrono::NaiveDate::from_ymd_opt(2026,1,3).unwrap(), ..Default::default() }];
        let payments = vec![Payment { pay_num: 5, pat_num: 1, pay_date: chrono::NaiveDate::from_ymd_opt(2026,1,3).unwrap(), pay_amount: 0.0, note: "Resolve credit".into(), ..Default::default() }];
        let t = pay_plan_charge_applications_report(&plans, &charges, &splits, &[Patient { pat_num: 1, f_name: "Ada".into(), l_name: "A".into(), ..Default::default() }], &payments, chrono::NaiveDate::from_ymd_opt(2026,1,1).unwrap(), chrono::NaiveDate::from_ymd_opt(2026,12,31).unwrap(), false);
        assert_eq!(t.headers.last().unwrap(), "Note");
        assert_eq!(t.rows[0][5], "Transfer/Resolve");
        assert!(t.rows[0][9].contains("Resolve credit"));
        assert!(t.rows[0][9].contains("Plan #1 -> account/unallocated"));
        let filtered = pay_plan_charge_applications_report(&plans, &charges, &splits, &[Patient { pat_num: 1, f_name: "Ada".into(), l_name: "A".into(), ..Default::default() }], &payments, chrono::NaiveDate::from_ymd_opt(2026,1,1).unwrap(), chrono::NaiveDate::from_ymd_opt(2026,12,31).unwrap(), true);
        assert_eq!(filtered.rows.len(), 1);
    }
