//! Main desktop application window.
//!
//! Navigation mirrors Open Dental's `FormOpenDental`: a top menu strip with
//! Setup and Reports, and a left module bar. Each module hosts a grid-style
//! view similar to the original `GridOD` controls.

mod definitions;
mod eservices;

use definitions::DefinitionsTab;
use eservices::EServicesTab;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{Datelike, Duration, Local, NaiveDate};
use egui::{Color32, Ui};
use od_core::{
    account_summary, account_summary_without_insurance, active_patient_report, adj_sheet_report,
    aging_report, appointments_report, appointment_change_note, asap_report, balance_sheet_report,
    adjustment_journal_memo_with_note, birthday_report, broken_appointments_report,
    collections_bucket_totals, collections_worklist, filter_collections_rows,
    CollectionsAgingBucket, CollectionsRow,
    build_adjustment_journal, build_invoice_document, build_journal_batches,
    build_payment_journal, build_posting_batch_plan_with_closed,
    build_refund_transfer_receipt, build_statement_document,
    compact_display_field_orders, confirmation_report, custom_aging_report,
    carrier_label_addresses, dental_sealant_measure_report, display_fields_for, discount_plan_report,
    discount_amount, annual_bucket, DiscountInput, FrequencyCodeSets,
    dpp_overcharged_report, finance_charge_report, general_ledger_report, hidden_pay_splits_report,
    close_audit_text, is_adjustment_posted, is_payment_posted, is_waitlist,
    journal_posting_audit_report, journal_transaction_for_adjustment,
    journal_transaction_for_payment, laser_labels_report, ledger_tie_out_report, month_bounds,
    new_patient_report, period_close_history_report_filtered, PeriodCloseActionFilter,
    PeriodCloseHistoryFilter, reopen_audit_text, tie_out_ready,
    next_definition_order, next_journal_transaction_num, patient_label_addresses,
    patient_portion_uncollected_report, pay_plan_charge_applications_report, pay_plan_detail_report, pay_sheet_report, patient_balance,
    patient_change_note, treatment_finder_report,
    parse_appointment_datetime, parse_appointment_length, pattern_for_length, payment_plan_report,
    patients_report, ph_raw_population_report, ph_raw_screen_report,
    presented_treatment_production_report, prescriptions_report, print_preview_report,
    proc_note_report, proc_overpaid_report, procedure_codes_report, prod_inc_report,
    procedure_sheet_report, profit_loss_report, production_goal_report, production_by_provider,
    provider_payroll_report, recall_due_date, recall_due_state, recall_fields_report, recall_report,
    receivables_breakdown_report, referral_analysis_report, referrals_report, render_invoice_pdf,
    render_refund_transfer_receipt_html, render_refund_transfer_receipt_pdf,
    render_report_csv, render_report_html, render_statement_html, render_statement_pdf,
    restore_appointment, routing_slips_report, schedule_from_waitlist, format_interval,
    interval_from_parts, interval_parts, schedule_conflicts, service_date_view_report,
    payment_journal_memo_with_note, suggest_ledger_accounts, suggest_payment_ledger_accounts,
    validate_balanced_journal,
    sorted_definitions, statement_report, treatment_plan_report,
    treat_plan_presentation_statistics_report, unearned_income_report,
    validate_clinic_card_issue, validate_definition, validate_display_fields,
    validate_payment_allocations, waitlist_status_definitions, waitlist_status_name,
    AppointmentDateBasis, CLOSED_PERIODS_PREF, ClosedPeriods, DiscountPlanSubscription,
    LabelAddress, PostingAccountDefaults, PostingSourceKind, PaySheetGroupBy, ProdIncMode,
    PublicHealthRow, RecallDueState, ReportRow, ReportTable,
};
use od_db::{Backend, DemoStore, MySqlStore, Store};
use od_domain::definitions::{
    default_display_fields, Definition, DefinitionCategory, DisplayField, DisplayFieldCategory,
};
use od_domain::eservices::*;
use od_domain::reports_ext::{LaserInsMode, LaserLabelTab, ProcNoteGroup, UnearnedIncomeTab};
use od_domain::reports::{
    AgingAgeFilter, AgingFamilyGrouping, BrokenAppointmentMode, BrokenProcedureOption,
    CollectionsAgingBand, CustomAgingNegative, CustomAgingWriteoffDate, PeriodCloseHistoryView,
    PlanTypeFilter,
    ProcedureSheetMode, ReferralFilter, ReportDateRange, ReportFilter, ReportFilterCondition,
    ReportPreferences, ReportSortState, EXPORT_KEY_ACCOUNT_AUDIT, EXPORT_KEY_ACCOUNT_LEDGER,
    EXPORT_KEY_PAY_PLAN_AUDIT, EXPORT_KEY_SECURITY_LOG, CONFIRMATION_OTHER_FIELDS,
    CONFIRMATION_STANDARD_FIELDS, PATIENT_REPORT_FIELDS, RECALL_OTHER_FIELDS, RECALL_STANDARD_FIELDS,
    REFERRAL_REPORT_FIELDS,
};
use od_domain::*;

/// One allocation row in the Record payment dialog.
#[derive(Clone, Debug)]
struct PaymentRow {
    target: PaymentTarget,
    amount_text: String,
}

/// Open a local file with the system's default handler (browser or PDF viewer).
fn open_in_browser(target: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let launched = std::process::Command::new("rundll32")
        .arg("url.dll,FileProtocolHandler")
        .arg(target)
        .spawn()
        .map(|_| ());
    #[cfg(not(target_os = "windows"))]
    let launched = std::process::Command::new("xdg-open")
        .arg(target)
        .spawn()
        .map(|_| ());
    launched.map_err(|e| format!("Could not open file: {e}"))
}

// ---------------------------------------------------------------------------
// Data gathering for the second upstream report tranche.
//
// These are free functions rather than closures inside `reports_ui` so they do
// not hold an immutable borrow of `self` across the grid-rendering code (which
// mutates sorting state). OD splits can reference patients other than the one
// who paid, so the per-patient reads are de-duplicated by primary key.
// ---------------------------------------------------------------------------

/// Row action offered by the per-patient Collections worklist.
#[derive(Clone, Copy, Debug, PartialEq)]
enum CollectionsAction {
    OpenAccount,
    RecordPayment,
    Statement,
}

/// Sort the worklist by the same visible values the exported grid uses, so the
/// action buttons stay aligned with the row the staff member clicked.
fn sort_collections_rows(rows: &mut [CollectionsRow], column: usize, descending: bool) {
    if column >= 6 {
        return;
    }
    rows.sort_by(|a, b| {
        let left = a.as_table_row();
        let right = b.as_table_row();
        match (left[column].parse::<f64>(), right[column].parse::<f64>()) {
            (Ok(x), Ok(y)) => x.total_cmp(&y),
            _ => left[column].cmp(&right[column]),
        }
    });
    if descending {
        rows.reverse();
    }
}

/// Translate the persisted band token into the live filter. `All` is both the
/// explicit choice and the fallback for users with no saved band.
fn collections_band_from_storage(band: CollectionsAgingBand) -> Option<CollectionsAgingBucket> {
    match band {
        CollectionsAgingBand::All => None,
        CollectionsAgingBand::Current => Some(CollectionsAgingBucket::Current),
        CollectionsAgingBand::Days31To60 => Some(CollectionsAgingBucket::Days31To60),
        CollectionsAgingBand::Days61To90 => Some(CollectionsAgingBucket::Days61To90),
        CollectionsAgingBand::Over90 => Some(CollectionsAgingBucket::Over90),
    }
}

fn collections_band_to_storage(selection: Option<CollectionsAgingBucket>) -> CollectionsAgingBand {
    match selection {
        None => CollectionsAgingBand::All,
        Some(CollectionsAgingBucket::Current) => CollectionsAgingBand::Current,
        Some(CollectionsAgingBucket::Days31To60) => CollectionsAgingBand::Days31To60,
        Some(CollectionsAgingBucket::Days61To90) => CollectionsAgingBand::Days61To90,
        Some(CollectionsAgingBucket::Over90) => CollectionsAgingBand::Over90,
    }
}

/// Export title for the Collections worklist. The title is repeated on every
/// CSV row and becomes the HTML heading, so naming the active aging band there
/// records which triage slice a saved file came from. Other reports keep their
/// plain title.
fn collections_export_title(filter: Option<CollectionsAgingBucket>) -> String {
    let scope = match filter {
        Some(bucket) => format!("{} band", bucket.label()),
        None => "All bands".to_string(),
    };
    format!("{} ({})", ReportKind::Collections.label(), scope)
}

fn gather_procedures(store: &dyn Store, patients: &[Patient]) -> Vec<Procedure> {
    let mut out = Vec::new();
    for patient in patients {
        out.extend(store.list_procedures(patient.pat_num).unwrap_or_default());
    }
    out
}

fn gather_splits(store: &dyn Store, patients: &[Patient]) -> Vec<PaySplit> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for patient in patients {
        for split in store.list_pay_splits(patient.pat_num).unwrap_or_default() {
            if seen.insert(split.split_num) {
                out.push(split);
            }
        }
    }
    out
}

fn posting_account_label(accounts: &[Account], num: Id) -> String {
    accounts
        .iter()
        .find(|a| a.account_num == num)
        .map(|a| format!("{} (#{})", a.description, a.account_num))
        .unwrap_or_else(|| format!("Select an account (#{num})"))
}

fn posting_account_pair_ui(
    ui: &mut Ui,
    debit_salt: &'static str,
    credit_salt: &'static str,
    caption: &str,
    accounts: &[Account],
    pair: &mut (Id, Id),
) {
    ui.label(caption);
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(debit_salt)
            .selected_text(posting_account_label(accounts, pair.0))
            .show_ui(ui, |ui| {
                for account in accounts {
                    ui.selectable_value(
                        &mut pair.0,
                        account.account_num,
                        posting_account_label(accounts, account.account_num),
                    );
                }
            });
        ui.label("→");
        egui::ComboBox::from_id_salt(credit_salt)
            .selected_text(posting_account_label(accounts, pair.1))
            .show_ui(ui, |ui| {
                for account in accounts {
                    ui.selectable_value(
                        &mut pair.1,
                        account.account_num,
                        posting_account_label(accounts, account.account_num),
                    );
                }
            });
    });
    ui.end_row();
}

fn gather_claim_procs(store: &dyn Store, patients: &[Patient]) -> Vec<ClaimProc> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for patient in patients {
        for proc in store.list_claim_procs(patient.pat_num).unwrap_or_default() {
            if seen.insert(proc.claim_proc_num) {
                out.push(proc);
            }
        }
    }
    out
}

fn gather_adjustments(store: &dyn Store, patients: &[Patient]) -> Vec<Adjustment> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for patient in patients {
        for adj in store.list_adjustments(patient.pat_num).unwrap_or_default() {
            if seen.insert(adj.adj_num) {
                out.push(adj);
            }
        }
    }
    out
}

fn gather_pay_plans(store: &dyn Store, patients: &[Patient]) -> Vec<PayPlan> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for patient in patients {
        for plan in store.list_pay_plans(patient.pat_num).unwrap_or_default() {
            if seen.insert(plan.pay_plan_num) {
                out.push(plan);
            }
        }
    }
    out
}

fn gather_pay_plan_charges(store: &dyn Store, patients: &[Patient]) -> Vec<PayPlanCharge> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for patient in patients {
        for charge in store.list_pay_plan_charges(patient.pat_num).unwrap_or_default() {
            if seen.insert(charge.pay_plan_charge_num) {
                out.push(charge);
            }
        }
    }
    out
}

/// Charge rows to show in the Account plan history. Stale-resolved rows are
/// zeroed/annotated history kept for audit; hiding them is an explicit staff
/// choice and never changes the stored rows or reconciliation.
fn visible_plan_charge_rows<'a>(
    charges: &'a [PayPlanCharge],
    plan_num: Id,
    show_stale_resolved: bool,
) -> Vec<&'a PayPlanCharge> {
    charges
        .iter()
        .filter(|charge| charge.pay_plan_num == plan_num)
        .filter(|charge| show_stale_resolved || !od_core::is_stale_resolved_charge(charge))
        .collect()
}

fn gather_treat_plans(store: &dyn Store, patients: &[Patient]) -> Vec<TreatPlan> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for patient in patients {
        for plan in store.list_treat_plans(patient.pat_num).unwrap_or_default() {
            if seen.insert(plan.treat_plan_num) {
                out.push(plan);
            }
        }
    }
    out
}

fn gather_treat_plan_lines(store: &dyn Store, plans: &[TreatPlan]) -> Vec<TreatPlanLine> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for plan in plans {
        for line in store.list_treat_plan_lines(plan.treat_plan_num).unwrap_or_default() {
            if seen.insert(line.line_num) {
                out.push(line);
            }
        }
    }
    out
}

fn gather_statements(store: &dyn Store, patients: &[Patient]) -> Vec<Statement> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for patient in patients {
        for statement in store.list_statements(patient.pat_num).unwrap_or_default() {
            if seen.insert(statement.statement_num) {
                out.push(statement);
            }
        }
    }
    out
}

/// Parse an optional OD date field from the subscription editor. Blank means
/// open ended (OD stores `0001-01-01`); a nonblank bad value is a user error.
fn parse_discount_date(text: &str, label: &str) -> Result<Option<NaiveDate>, String> {
    let text = text.trim();
    if text.is_empty() {
        Ok(None)
    } else {
        NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| format!("{label} must be a date in YYYY-MM-DD form"))
    }
}

/// Shape OD's `discountplan`/`discountplansub`/`feesched`/`definition` join into
/// the rows consumed by `FormRpDiscountPlan`.
fn gather_discount_subscriptions(
    store: &dyn Store,
    patients: &[Patient],
) -> Vec<DiscountPlanSubscription> {
    let plans = store.list_discount_plans().unwrap_or_default();
    let subs = store.list_discount_plan_subs().unwrap_or_default();
    let fee_schedules = store.list_fee_schedules().unwrap_or_default();
    let definitions = store.list_definitions().unwrap_or_default();
    subs.iter()
        .filter_map(|sub| {
            let plan = plans
                .iter()
                .find(|p| p.discount_plan_num == sub.discount_plan_num)?;
            let patient = patients.iter().find(|p| p.pat_num == sub.pat_num)?;
            Some(DiscountPlanSubscription {
                description: plan.description.clone(),
                fee_schedule: fee_schedules
                    .iter()
                    .find(|f| f.fee_sched_num == plan.fee_sched_num)
                    .map(|f| f.description.clone())
                    .unwrap_or_default(),
                adj_type: definitions
                    .iter()
                    .find(|d| d.def_num == plan.def_num)
                    .map(|d| d.item_name.clone())
                    .unwrap_or_default(),
                date_effective: sub.date_effective,
                date_term: sub.date_term,
                patient: patient.display_name(),
            })
        })
        .collect()
}

fn format_date(value: Option<NaiveDate>) -> String {
    value
        .map(|d| d.format("%m/%d/%Y").to_string())
        .unwrap_or_default()
}

fn yes_no(value: bool) -> &'static str {
    if value { "Yes" } else { "No" }
}

/// `FormRpPHRawPop`: one row per patient with a completed procedure in the
/// range. `NumberBroken` counts broken appointments (OD uses specific broken-
/// appointment adjustment types, which this local-first port does not seed).
fn gather_public_health_population(
    store: &dyn Store,
    patients: &[Patient],
    appointments: &[Appointment],
    procedures: &[Procedure],
    from: NaiveDate,
    to: NaiveDate,
) -> Vec<PublicHealthRow> {
    let providers = store.list_providers().unwrap_or_default();
    let sites = store.list_sites().unwrap_or_default();
    let counties = store.list_counties().unwrap_or_default();
    let races = store.list_patient_races().unwrap_or_default();
    patients
        .iter()
        .filter_map(|patient| {
            let first_proc = procedures
                .iter()
                .filter(|p| {
                    p.pat_num == patient.pat_num
                        && p.proc_status == ProcedureStatus::Complete
                        && p.proc_date >= from
                        && p.proc_date <= to
                })
                .min_by_key(|p| p.proc_date);
            let broken = appointments
                .iter()
                .filter(|a| {
                    a.pat_num == patient.pat_num
                        && a.apt_status == AppointmentStatus::Broken
                        && a.apt_datetime.date() >= from
                        && a.apt_datetime.date() <= to
                })
                .count();
            if first_proc.is_none() && broken == 0 {
                return None;
            }
            let site = sites.iter().find(|s| s.site_num == patient.site_num);
            let county_code = counties
                .iter()
                .find(|c| c.county_name == patient.county)
                .map(|c| c.county_code.clone())
                .unwrap_or_default();
            let race_values: Vec<String> = races
                .iter()
                .filter(|r| r.pat_num == patient.pat_num)
                .map(|r| r.race.to_string())
                .collect();
            let prov_name = first_proc
                .and_then(|p| providers.iter().find(|v| v.provider_num == p.provider_num))
                .map(|v| format!("{}, {}", v.l_name, v.f_name))
                .unwrap_or_default();
            Some(PublicHealthRow {
                values: vec![
                    patient.pat_num.to_string(),
                    format_date(first_proc.map(|p| p.proc_date)),
                    prov_name,
                    patient.county.clone(),
                    county_code,
                    site.map(|s| s.description.clone()).unwrap_or_default(),
                    site.map(|s| s.note.clone()).unwrap_or_default(),
                    patient.grade_level.to_string(),
                    format_date(patient.birth_date),
                    race_values.join(","),
                    Gender::from_od(patient.gender as i8).label().to_string(),
                    patient.urgency.to_string(),
                    patient.billing_type.to_string(),
                    yes_no(patient.planned_is_done).to_string(),
                    broken.to_string(),
                ],
            })
        })
        .collect()
}

/// `FormRpPHRawScreen`: one row per screening record in the date range.
fn gather_public_health_screens(
    store: &dyn Store,
    from: NaiveDate,
    to: NaiveDate,
    dental: bool,
) -> Vec<PublicHealthRow> {
    let groups = store.list_screen_groups().unwrap_or_default();
    let screens = store.list_screens().unwrap_or_default();
    let sites = store.list_sites().unwrap_or_default();
    let counties = store.list_counties().unwrap_or_default();
    screens
        .iter()
        .filter_map(|screen| {
            let group = groups
                .iter()
                .find(|g| g.screen_group_num == screen.screen_group_num)?;
            let sg_date = group.sg_date?;
            if sg_date < from || sg_date > to {
                return None;
            }
            let site = sites.iter().find(|s| s.description == group.grade_school);
            let county_code = counties
                .iter()
                .find(|c| c.county_name == group.county)
                .map(|c| c.county_code.clone())
                .unwrap_or_default();
            let mut values = vec![
                format_date(Some(sg_date)),
                group.prov_name.clone(),
                group.county.clone(),
                county_code,
                site.map(|s| s.description.clone()).unwrap_or_default(),
                site.map(|s| s.note.clone()).unwrap_or_default(),
                group.place_service.to_string(),
                screen.grade_level.to_string(),
                screen.age.to_string(),
                format_date(screen.birth_date),
                screen.race_old.to_string(),
                Gender::from_od(screen.gender as i8).label().to_string(),
                screen.urgency.to_string(),
            ];
            if dental {
                values.extend_from_slice(&[
                    yes_no(screen.has_caries).to_string(),
                    yes_no(screen.early_child_caries).to_string(),
                    yes_no(screen.caries_experience).to_string(),
                    yes_no(screen.existing_sealants).to_string(),
                    yes_no(screen.needs_sealants).to_string(),
                    yes_no(screen.missing_all_teeth).to_string(),
                ]);
            }
            values.push(screen.comments.clone());
            Some(PublicHealthRow { values })
        })
        .collect()
}

/// Write a print-ready statement to a temp file and open it in the system's
/// default browser, where the page's Print button starts the print dialog.
fn open_statement_in_browser(html: &str, pat_num: Id) -> Result<String, String> {
    let path = std::env::temp_dir().join(format!("odclinic-statement-{pat_num}.html"));
    std::fs::write(&path, html).map_err(|e| format!("Could not write statement file: {e}"))?;
    let target = path.to_string_lossy().into_owned();
    open_in_browser(&target)?;
    Ok(target)
}

/// Build the A5 invoice preview, applying the clinic's tax rate to every line.
fn invoice_for(
    pat_num: Id,
    patient_name: &str,
    procedures: &[Procedure],
    splits: &[PaySplit],
    tax_text: &str,
    note: &str,
) -> InvoiceDocument {
    let mut doc = build_invoice_document(
        pat_num,
        patient_name,
        procedures,
        splits,
        Local::now().date_naive(),
        note,
    );
    let tax_rate = tax_text.trim().parse::<f64>().unwrap_or(0.0).max(0.0);
    for line in &mut doc.lines {
        line.tax_rate = tax_rate;
    }
    doc
}

/// A sensible default location for a patient's A5 invoice PDF.
fn default_invoice_pdf_path(pat_num: Id) -> String {
    std::env::var_os("USERPROFILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Documents")
        .join(format!("odclinic-invoice-a5-{pat_num}.pdf"))
        .to_string_lossy()
        .into_owned()
}

fn default_refund_transfer_receipt_pdf_path(pat_num: Id, pay_num: Id) -> String {
    std::env::var_os("USERPROFILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Documents")
        .join(format!("odclinic-refund-transfer-receipt-{pat_num}-{pay_num}.pdf"))
        .to_string_lossy()
        .into_owned()
}

/// A sensible default location for an exported report.
fn default_report_path(kind: ReportKind, extension: &str) -> String {
    std::env::var_os("USERPROFILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Documents")
        .join(format!(
            "odclinic-{}-{}.{}",
            kind.file_stem(),
            chrono::Local::now().format("%Y-%m-%d"),
            extension
        ))
        .to_string_lossy()
        .into_owned()
}

/// A persisted export path for the signed-in user, blank when neither they nor
/// the practice have set one.
fn persisted_export_path(prefs: &ReportPreferences, user_key: &str, key: &str) -> String {
    prefs
        .effective_export_path(user_key, key)
        .map(str::to_string)
        .unwrap_or_default()
}

/// Dedicated exports that carry a patient number use one filing location per
/// account. The unscoped key is honoured as a fallback for paths saved before
/// per-patient keys existed.
fn patient_export_key(base_key: &str, pat_num: Id) -> String {
    format!("{base_key}.{pat_num}")
}

fn patient_export_path(
    prefs: &ReportPreferences,
    user_key: &str,
    base_key: &str,
    pat_num: Id,
    fallback: String,
) -> String {
    prefs
        .effective_export_path(user_key, &patient_export_key(base_key, pat_num))
        .or_else(|| prefs.effective_export_path(user_key, base_key))
        .map(str::to_string)
        .unwrap_or(fallback)
}

/// The security-log export is practice-wide unless a patient filter is active.
fn security_log_export_key(patient_num: Option<Id>) -> String {
    match patient_num {
        Some(num) => patient_export_key(EXPORT_KEY_SECURITY_LOG, num),
        None => EXPORT_KEY_SECURITY_LOG.to_string(),
    }
}

fn default_account_ledger_export_path(pat_num: Id) -> String {
    std::env::current_dir()
        .ok()
        .unwrap_or_default()
        .join(format!("account-ledger-patient-{pat_num}.csv"))
        .to_string_lossy()
        .into_owned()
}

/// Ensure the user's export path carries the extension for the chosen format.
/// Windows treats a leading/trailing space and a stray dot literally, so trim
/// both before comparing the extension case-insensitively.
fn report_export_path(path: &str, extension: &str) -> std::path::PathBuf {
    let trimmed = path.trim();
    let path = std::path::PathBuf::from(trimmed);
    let wanted = format!(".{extension}");
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if format!(".{ext}").eq_ignore_ascii_case(&wanted) => path,
        _ => path.with_extension(extension),
    }
}

/// Case-insensitive security-log search across details, permission and user.
fn security_log_matches(event: &SecurityEvent, filter: &str) -> bool {
    let needle = filter.trim().to_lowercase();
    needle.is_empty()
        || event.log_text.to_lowercase().contains(&needle)
        || event.perm_type.label().to_lowercase().contains(&needle)
        || event.user_num.to_string().contains(&needle)
}

/// Optional patient/account filter for the audit log. Blank or non-numeric
/// values leave the log unscoped; `0` is never a valid patient number.
fn security_log_patient_filter(text: &str) -> Option<Id> {
    text.trim().parse::<Id>().ok().filter(|num| *num > 0)
}

/// A sensible default path for an exported audit view. Including the patient
/// number makes the file easy to attach to that patient's/account's record.
fn default_security_log_export_path(patient_num: Option<Id>) -> String {
    let file = match patient_num {
        Some(num) => format!("security-log-patient-{num}.csv"),
        None => "security-log.csv".to_string(),
    };
    std::env::current_dir()
        .unwrap_or_default()
        .join(file)
        .to_string_lossy()
        .into_owned()
}

/// Security-log rows as an auditable export table. The patient number is kept
/// as its own column so a stale-resolution row can be filed against the right
/// patient/account record outside this app.
fn security_log_export_table(log: &[&SecurityEvent]) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Log #",
        "Time",
        "User",
        "Type",
        "Patient",
        "Details",
    ]);
    for event in log {
        let log_num = event.security_log_num.to_string();
        let time = event.log_datetime.format("%Y-%m-%d %H:%M:%S").to_string();
        let user = event.user_num.to_string();
        let patient = if event.pat_num == 0 {
            String::new()
        } else {
            event.pat_num.to_string()
        };
        table.push(&[
            &log_num,
            &time,
            &user,
            event.perm_type.label(),
            &patient,
            &event.log_text,
        ]);
    }
    table
}

#[derive(Clone)]
struct LedgerRow {
    date: NaiveDate,
    kind: String,
    description: String,
    charge: f64,
    credit: f64,
    plan_num: Id,
    transfer_target: Option<PaymentTarget>,
    pay_num: Id,
    /// Set on adjustment rows so the ledger can offer "Post to ledger".
    adj_num: Id,
    note: String,
}

/// The period-close action being confirmed in the audited dialog.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PeriodAction {
    Close,
    Reopen,
}

/// The patient-account record being posted into the double-entry journal.
/// Adjustments and payments share one window, one balance guard and one audit
/// path; only the source-specific builders and reference tokens differ.
#[derive(Clone)]
enum JournalPostSource {
    Adjustment(Adjustment),
    Payment(Payment),
}

impl JournalPostSource {
    fn source_num(&self) -> Id {
        match self {
            Self::Adjustment(adj) => adj.adj_num,
            Self::Payment(pay) => pay.pay_num,
        }
    }

    fn pat_num(&self) -> Id {
        match self {
            Self::Adjustment(adj) => adj.pat_num,
            Self::Payment(pay) => pay.pat_num,
        }
    }

    fn date(&self) -> chrono::NaiveDate {
        match self {
            Self::Adjustment(adj) => adj.adj_date,
            Self::Payment(pay) => pay.pay_date,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Adjustment(_) => "Adjustment",
            Self::Payment(_) => "Payment",
        }
    }

    fn note(&self) -> &str {
        match self {
            Self::Adjustment(adj) => &adj.note,
            Self::Payment(pay) => &pay.note,
        }
    }

    fn amount(&self) -> f64 {
        match self {
            Self::Adjustment(adj) => adj.adj_amount.abs(),
            Self::Payment(pay) => pay.pay_amount.abs(),
        }
    }

    fn heading(&self) -> String {
        match self {
            Self::Adjustment(adj) => format!(
                "Adjustment #{} · patient #{} · {} · {:.2}",
                adj.adj_num, adj.pat_num, adj.adj_date, adj.adj_amount
            ),
            Self::Payment(pay) => format!(
                "Payment #{} · patient #{} · {} · {:.2}{}",
                pay.pay_num,
                pay.pat_num,
                pay.pay_date,
                pay.pay_amount,
                if pay.check_num.trim().is_empty() {
                    String::new()
                } else {
                    format!(" · cheque {}", pay.check_num)
                }
            ),
        }
    }

    fn is_posted(&self, journal: &[JournalEntry]) -> bool {
        match self {
            Self::Adjustment(adj) => is_adjustment_posted(journal, adj.adj_num, adj.pat_num),
            Self::Payment(pay) => is_payment_posted(journal, pay.pay_num, pay.pat_num),
        }
    }

    fn memo(&self, note: &str) -> String {
        match self {
            Self::Adjustment(adj) => adjustment_journal_memo_with_note(adj, note),
            Self::Payment(pay) => payment_journal_memo_with_note(pay, note),
        }
    }

    fn default_accounts(&self, accounts: &[Account]) -> (Id, Id) {
        match self {
            Self::Adjustment(adj) => suggest_ledger_accounts(accounts, adj.adj_amount),
            Self::Payment(pay) => suggest_payment_ledger_accounts(accounts, pay.pay_amount),
        }
        .unwrap_or((0, 0))
    }

    fn build(
        &self,
        debit_account: Id,
        credit_account: Id,
        transaction_num: Id,
    ) -> Result<[JournalEntry; 2], String> {
        match self {
            Self::Adjustment(adj) => {
                build_adjustment_journal(adj, debit_account, credit_account, transaction_num)
            }
            Self::Payment(pay) => {
                build_payment_journal(pay, debit_account, credit_account, transaction_num)
            }
        }
    }
}

fn build_account_ledger_rows(
    procedures: &[Procedure],
    adjustments: &[Adjustment],
    splits: &[PaySplit],
    payments: &[Payment],
    include_adjustments: bool,
) -> Vec<LedgerRow> {
    let mut rows: Vec<LedgerRow> = procedures
        .iter()
        .map(|p| LedgerRow {
            date: p.proc_date,
            kind: "Proc".into(),
            description: format!("Code #{} tooth {}", p.code_num, p.tooth_num),
            charge: p.fee,
            credit: 0.0,
            plan_num: 0,
            transfer_target: None,
            pay_num: 0,
            adj_num: 0,
            note: String::new(),
        })
        .collect();
    if include_adjustments {
        rows.extend(adjustments.iter().map(|a| LedgerRow {
            date: a.adj_date,
            kind: "Adj".into(),
            description: if a.note.is_empty() {
                format!("Adjustment type {}", a.adj_type)
            } else {
                a.note.clone()
            },
            charge: a.adj_amount.max(0.0),
            credit: (-a.adj_amount).max(0.0),
            plan_num: 0,
            transfer_target: None,
            pay_num: 0,
            adj_num: a.adj_num,
            note: a.note.clone(),
        }));
    }
    rows.extend(splits.iter().map(|sp| {
        let is_refund_transfer = sp.split_amount < -0.005;
        let transfer_target = if is_refund_transfer && sp.pay_plan_num != 0 {
            let counterpart = splits.iter().find(|o| {
                o.pay_num == sp.pay_num
                    && o.split_num != sp.split_num
                    && o.split_amount > 0.005
            });
            Some(
                counterpart
                    .map(|o| {
                        if o.proc_num != 0 {
                            PaymentTarget::Procedure(o.proc_num)
                        } else if o.pay_plan_num != 0 && o.pay_plan_num != sp.pay_plan_num {
                            PaymentTarget::PayPlan(o.pay_plan_num)
                        } else {
                            PaymentTarget::Unallocated
                        }
                    })
                    .unwrap_or(PaymentTarget::Unallocated),
            )
        } else {
            None
        };
        LedgerRow {
            date: sp.date_pay,
            kind: if is_refund_transfer {
                "Refund/Transfer".into()
            } else {
                "Payment".into()
            },
            description: if sp.pay_plan_charge_num != 0 {
                format!(
                    "Plan #{} charge #{} ({})",
                    sp.pay_plan_num,
                    sp.pay_plan_charge_num,
                    sp.pay_plan_debit_type.label()
                )
            } else if sp.pay_plan_num != 0 {
                if sp.split_amount < 0.0 {
                    let target = match transfer_target.unwrap_or(PaymentTarget::Unallocated) {
                        PaymentTarget::Procedure(pn) => format!("procedure #{}", pn),
                        PaymentTarget::PayPlan(pn) => format!("plan #{}", pn),
                        PaymentTarget::PayPlanCharge { plan_num, charge_num } => {
                            format!("plan #{} charge #{}", plan_num, charge_num)
                        }
                        PaymentTarget::Unallocated => "patient account (unallocated)".to_string(),
                    };
                    format!("Payment plan #{} (credit moved/refund -> {})", sp.pay_plan_num, target)
                } else {
                    format!("Payment plan #{} (unapplied to charge)", sp.pay_plan_num)
                }
            } else if sp.proc_num == 0 {
                if sp.split_amount < 0.0 {
                    "Unallocated refund/transfer".into()
                } else {
                    "Unallocated payment".into()
                }
            } else {
                format!("Toward procedure #{}", sp.proc_num)
            },
            charge: (-sp.split_amount).max(0.0),
            credit: sp.split_amount.max(0.0),
            plan_num: sp.pay_plan_num,
            transfer_target,
            pay_num: sp.pay_num,
            adj_num: 0,
            note: payments.iter().find(|p| p.pay_num == sp.pay_num).map(|p| p.note.clone()).unwrap_or_default(),
        }
    }));
    rows
}

fn filter_account_ledger_rows(
    rows: &[LedgerRow],
    refunds_transfers_only: bool,
    plan_filter: Id,
    target_filter: PaymentTarget,
    from_date: Option<NaiveDate>,
    to_date: Option<NaiveDate>,
) -> Vec<LedgerRow> {
    rows.iter()
        .cloned()
        .filter(|r| {
            if refunds_transfers_only && r.kind != "Refund/Transfer" {
                return false;
            }
            if plan_filter != 0 && r.plan_num != plan_filter {
                return false;
            }
            if let Some(from) = from_date {
                if r.date < from {
                    return false;
                }
            }
            if let Some(to) = to_date {
                if r.date > to {
                    return false;
                }
            }
            if !matches!(target_filter, PaymentTarget::Unallocated) {
                return r.kind == "Refund/Transfer" && r.transfer_target == Some(target_filter);
            }
            true
        })
        .collect()
}

fn account_ledger_export_table(rows: &[LedgerRow]) -> ReportTable {
    let mut table = ReportTable::new(&["Date", "Type", "Description", "Charge", "Credit", "Payment #", "Note"]);
    for r in rows {
        table.push(&[
            &r.date.to_string(),
            &r.kind,
            &r.description,
            &format!("{:.2}", r.charge),
            &format!("{:.2}", r.credit),
            &if r.pay_num == 0 { String::new() } else { r.pay_num.to_string() },
            &r.note,
        ]);
    }
    table
}

/// A sensible default filename for a patient-scoped account audit bundle.
/// Including the patient number makes the file ready to file against that
/// patient's/account's record.
fn default_account_audit_export_path(pat_num: Id) -> String {
    std::env::current_dir()
        .unwrap_or_default()
        .join(format!("account-audit-patient-{pat_num}.csv"))
        .to_string_lossy()
        .into_owned()
}

/// One patient's stale-resolution history and matching security-log rows in a
/// single table, so staff can file one audit file against the account instead
/// of associating two separate exports. Details always carry `reason: …` so
/// the same text is searchable in both sections and in CSV/HTML.
fn account_audit_export_table(
    pat_num: Id,
    patient_name: &str,
    plans: &[PayPlan],
    charges: &[PayPlanCharge],
    log: &[SecurityEvent],
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Section",
        "When",
        "Patient",
        "Plan #",
        "Source #",
        "User",
        "Details",
    ]);
    let patient = format!("{} (#{pat_num})", patient_name);
    for charge in charges
        .iter()
        .filter(|charge| charge.pat_num == pat_num && od_core::is_stale_resolved_charge(charge))
    {
        let reason = od_core::stale_resolution_reason(&charge.note).unwrap_or("(not recorded)");
        let plan_note = plans
            .iter()
            .find(|plan| plan.pay_plan_num == charge.pay_plan_num)
            .map(|plan| plan.note.as_str())
            .filter(|note| !note.is_empty())
            .unwrap_or("—");
        let details = format!(
            "reason: {reason} · charge #{} · plan note: {} · {}",
            charge.pay_plan_charge_num, plan_note, charge.note
        );
        table.push(&[
            "Resolved charge",
            &charge.charge_date.to_string(),
            &patient,
            &charge.pay_plan_num.to_string(),
            &charge.pay_plan_charge_num.to_string(),
            "",
            &details,
        ]);
    }
    for event in log.iter().filter(|event| event.pat_num == pat_num) {
        table.push(&[
            "Security log",
            &event.log_datetime.format("%Y-%m-%d %H:%M:%S").to_string(),
            &patient,
            "",
            &event.security_log_num.to_string(),
            &event.user_num.to_string(),
            &event.log_text,
        ]);
    }
    table
}

/// A sensible default filename for the payment-plan stale-resolved audit
/// export. The name makes the file easy to file alongside the security-log
/// extract for the same reconciliation.
fn default_pay_plan_audit_export_path() -> String {
    std::env::current_dir()
        .unwrap_or_default()
        .join(format!(
            "pay-plan-stale-resolved-{}.csv",
            chrono::Local::now().format("%Y-%m-%d")
        ))
        .to_string_lossy()
        .into_owned()
}

/// Keep only plan rows with a positive `Resolved` count, dropping the report's
/// total footer. This makes the exported file a genuine stale-resolved audit
/// subset even if the staff member did not tick the report filter checkbox.
fn stale_resolved_export_table(table: &ReportTable) -> ReportTable {
    let Some(resolved_index) = table.headers.iter().position(|header| header == "Resolved") else {
        return table.clone();
    };
    ReportTable {
        headers: table.headers.clone(),
        rows: table
            .rows
            .iter()
            .filter(|row| {
                // The report footer also carries the resolved total; it is a
                // summary, not a plan row, so never export it as an audit row.
                if row
                    .first()
                    .map_or(false, |value| value.starts_with("Total ("))
                {
                    return false;
                }
                row.get(resolved_index)
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0)
                    > 0
            })
            .cloned()
            .collect(),
    }
}

/// A sensible default location for a patient's statement PDF.
fn default_statement_pdf_path(pat_num: Id) -> String {
    std::env::var_os("USERPROFILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Documents")
        .join(format!("odclinic-statement-{pat_num}.pdf"))
        .to_string_lossy()
        .into_owned()
}

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1320.0, 860.0]),
        ..Default::default()
    };
    eframe::run_native(
        "odclinic — Open Dental Rust port",
        options,
        Box::new(|_cc| Ok(Box::new(OdApp::new()))),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ApptTab {
    Schedule,
    Recalls,
    Asap,
    Waitlist,
}

impl ApptTab {
    const ALL: [ApptTab; 4] = [
        ApptTab::Schedule,
        ApptTab::Recalls,
        ApptTab::Asap,
        ApptTab::Waitlist,
    ];

    fn label(self) -> &'static str {
        match self {
            ApptTab::Schedule => "Schedule",
            ApptTab::Recalls => "Recalls",
            ApptTab::Asap => "ASAP list",
            ApptTab::Waitlist => "Waitlist",
        }
    }

    fn enabled(self, features: &FeatureSettings) -> bool {
        match self {
            ApptTab::Schedule => true,
            ApptTab::Recalls => features.enabled(FeatureKey::RecallsEnabled),
            ApptTab::Asap => features.enabled(FeatureKey::AsapListEnabled),
            ApptTab::Waitlist => features.enabled(FeatureKey::WaitlistEnabled),
        }
    }
}

/// Tabs inside the patient registry window. The registry is reached from the
/// treatment and family views, not from a separate card-issuing module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RegistryTab {
    Overview,
    Clinical,
    Card,
    Invoice,
}

impl RegistryTab {
    const ALL: [RegistryTab; 4] = [
        RegistryTab::Overview,
        RegistryTab::Clinical,
        RegistryTab::Card,
        RegistryTab::Invoice,
    ];

    fn label(self) -> &'static str {
        match self {
            RegistryTab::Overview => "Overview",
            RegistryTab::Clinical => "Treatments & conditions",
            RegistryTab::Card => "Clinic card",
            RegistryTab::Invoice => "A5 invoice",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReportKind {
    PatientList,
    Production,
    Collections,
    Schedule,
    RecallAsap,
    PaymentPlans,
    TreatmentPlans,
    Aging,
    PayPlanDetail,
    PayPlanChargeApplications,
    Birthday,
    NewPatients,
    Appointments,
    ActivePatients,
    ProdInc,
    PaySheet,
    AdjSheet,
    FinanceCharge,
    BrokenAppointments,
    Confirmation,
    RecallFields,
    DailyProcedures,
    ProcedureCodes,
    Prescriptions,
    ReferralAnalysis,
    Referrals,
    CustomAging,
    Patients,
    BalanceSheet,
    GeneralLedger,
    ProfitLoss,
    JournalPostingAudit,
    LedgerTieOut,
    PeriodCloseHistory,
    HiddenPaySplits,
    UnearnedIncome,
    ProcOverpaid,
    ReceivablesBreakdown,
    ServiceDateView,
    ProcNote,
    DentalSealant,
    DiscountPlan,
    DppOvercharged,
    PatientPortionUncollected,
    TreatmentFinder,
    PhRawPop,
    PhRawScreen,
    Routing,
    LaserLabels,
    PrintPreview,
    Statement,
    ProdGoal,
    ProviderPayroll,
    PresentedTreatment,
    TpPresentationStats,
}

impl ReportKind {
    const ALL: [ReportKind; 55] = [
        ReportKind::PatientList,
        ReportKind::Production,
        ReportKind::Collections,
        ReportKind::Schedule,
        ReportKind::RecallAsap,
        ReportKind::PaymentPlans,
        ReportKind::TreatmentPlans,
        ReportKind::Aging,
        ReportKind::PayPlanDetail,
        ReportKind::PayPlanChargeApplications,
        ReportKind::Birthday,
        ReportKind::NewPatients,
        ReportKind::Appointments,
        ReportKind::ActivePatients,
        ReportKind::ProdInc,
        ReportKind::PaySheet,
        ReportKind::AdjSheet,
        ReportKind::FinanceCharge,
        ReportKind::BrokenAppointments,
        ReportKind::Confirmation,
        ReportKind::RecallFields,
        ReportKind::DailyProcedures,
        ReportKind::ProcedureCodes,
        ReportKind::Prescriptions,
        ReportKind::ReferralAnalysis,
        ReportKind::Referrals,
        ReportKind::CustomAging,
        ReportKind::Patients,
        ReportKind::BalanceSheet,
        ReportKind::GeneralLedger,
        ReportKind::ProfitLoss,
        ReportKind::JournalPostingAudit,
        ReportKind::LedgerTieOut,
        ReportKind::PeriodCloseHistory,
        ReportKind::HiddenPaySplits,
        ReportKind::UnearnedIncome,
        ReportKind::ProcOverpaid,
        ReportKind::ReceivablesBreakdown,
        ReportKind::ServiceDateView,
        ReportKind::ProcNote,
        ReportKind::DentalSealant,
        ReportKind::DiscountPlan,
        ReportKind::DppOvercharged,
        ReportKind::PatientPortionUncollected,
        ReportKind::TreatmentFinder,
        ReportKind::PhRawPop,
        ReportKind::PhRawScreen,
        ReportKind::Routing,
        ReportKind::LaserLabels,
        ReportKind::PrintPreview,
        ReportKind::Statement,
        ReportKind::ProdGoal,
        ReportKind::ProviderPayroll,
        ReportKind::PresentedTreatment,
        ReportKind::TpPresentationStats,
    ];
    fn label(self) -> &'static str {
        match self {
            ReportKind::PatientList => "Patient list",
            ReportKind::Production => "Production by provider",
            ReportKind::Collections => "Collections",
            ReportKind::Schedule => "Scheduled appointments",
            ReportKind::RecallAsap => "Recall & ASAP due list",
            ReportKind::PaymentPlans => "Payment-plan balances",
            ReportKind::TreatmentPlans => "Treatment-plan summary",
            ReportKind::Aging => "Patient aging (FormRpAging)",
            ReportKind::PayPlanDetail => "Payment plans (FormRpPayPlans)",
            ReportKind::PayPlanChargeApplications => "Payment plan charge applications",
            ReportKind::Birthday => "Birthdays (FormRpBirthday)",
            ReportKind::NewPatients => "New patients (FormRpNewPatients)",
            ReportKind::Appointments => "Appointments (FormRpAppointments)",
            ReportKind::ActivePatients => "Active patients (FormRpActivePatients)",
            ReportKind::ProdInc => "Production & income (FormRpProdInc)",
            ReportKind::PaySheet => "Daily payments (FormRpPaySheet)",
            ReportKind::AdjSheet => "Daily adjustments (FormRpAdjSheet)",
            ReportKind::FinanceCharge => "Finance charges (FormRpFinanceCharge)",
            ReportKind::BrokenAppointments => "Broken appointments (FormRpBrokenAppointments)",
            ReportKind::Confirmation => "Confirmations (FormRpConfirm)",
            ReportKind::RecallFields => "Recall field selection (FormRpRecall)",
            ReportKind::DailyProcedures => "Daily procedures (FormRpProcSheet)",
            ReportKind::ProcedureCodes => "Procedure codes (FormRpProcCodes)",
            ReportKind::Prescriptions => "Prescriptions (FormRpPrescriptions)",
            ReportKind::ReferralAnalysis => "Referral analysis (FormRpReferralAnalysis)",
            ReportKind::Referrals => "Referrals (FormRpReferrals)",
            ReportKind::CustomAging => "Custom aging (FormRpCustomAging)",
            ReportKind::Patients => "Patients (FormRpPatients)",
            ReportKind::BalanceSheet => "Balance sheet (FormRpAccountingBalanceSheet)",
            ReportKind::GeneralLedger => "General ledger (FormRpAccountingGenLedg)",
            ReportKind::ProfitLoss => "Profit & loss (FormRpAccountingProfitLoss)",
            ReportKind::JournalPostingAudit => "Journal posting audit",
            ReportKind::LedgerTieOut => "Ledger tie-out",
            ReportKind::PeriodCloseHistory => "Period close history",
            ReportKind::HiddenPaySplits => "Hidden pay splits (FormRpHiddenPaySplits)",
            ReportKind::UnearnedIncome => "Unearned income (FormRpUnearnedIncome)",
            ReportKind::ProcOverpaid => "Overpaid procedures (FormRpProcOverpaid)",
            ReportKind::ReceivablesBreakdown => "Receivables breakdown (FormRpReceivablesBreakdown)",
            ReportKind::ServiceDateView => "Service-date view (FormRpServiceDateView)",
            ReportKind::ProcNote => "Incomplete procedure notes (FormRpProcNote)",
            ReportKind::DentalSealant => "Dental sealant measure (FormRpDentalSealantMeasure)",
            ReportKind::DiscountPlan => "Discount plans (FormRpDiscountPlan)",
            ReportKind::DppOvercharged => "Overcharged payment plans (FormRpDPPOvercharged)",
            ReportKind::PatientPortionUncollected => {
                "Patient portion uncollected (FormRpPatPortionUncollected)"
            }
            ReportKind::TreatmentFinder => "Treatment finder (FormRpTreatmentFinder)",
            ReportKind::PhRawPop => "Raw population extract (FormRpPHRawPop)",
            ReportKind::PhRawScreen => "Raw screening extract (FormRpPHRawScreen)",
            ReportKind::Routing => "Routing slips (FormRpRouting)",
            ReportKind::LaserLabels => "Laser labels (FormRpLaserLabels)",
            ReportKind::PrintPreview => "Print preview (FormRpPrintPreview)",
            ReportKind::Statement => "Statements (FormRpStatement)",
            ReportKind::ProdGoal => "Production goal (FormRpProdGoal)",
            ReportKind::ProviderPayroll => "Provider payroll (FormRpProviderPayroll)",
            ReportKind::PresentedTreatment => "Presented treatment production (FormRpPresentedTreatmentProduction)",
            ReportKind::TpPresentationStats => "Plan presentation statistics (FormRpTreatPlanPresentationStatistics)",
        }
    }

    fn feature_key(self) -> FeatureKey {
        match self {
            ReportKind::PatientList => FeatureKey::ReportPatientList,
            ReportKind::Production => FeatureKey::ReportProduction,
            ReportKind::Collections => FeatureKey::ReportCollections,
            ReportKind::Schedule => FeatureKey::ReportSchedule,
            ReportKind::RecallAsap => FeatureKey::ReportRecallAsap,
            ReportKind::PaymentPlans => FeatureKey::ReportPaymentPlans,
            ReportKind::TreatmentPlans => FeatureKey::ReportTreatmentPlans,
            ReportKind::Aging => FeatureKey::ReportAging,
            ReportKind::PayPlanDetail => FeatureKey::ReportPayPlanDetail,
            ReportKind::PayPlanChargeApplications => FeatureKey::ReportPayPlanChargeApplications,
            ReportKind::Birthday => FeatureKey::ReportBirthday,
            ReportKind::NewPatients => FeatureKey::ReportNewPatients,
            ReportKind::Appointments => FeatureKey::ReportAppointments,
            ReportKind::ActivePatients => FeatureKey::ReportActivePatients,
            ReportKind::ProdInc => FeatureKey::ReportProdInc,
            ReportKind::PaySheet => FeatureKey::ReportPaySheet,
            ReportKind::AdjSheet => FeatureKey::ReportAdjSheet,
            ReportKind::FinanceCharge => FeatureKey::ReportFinanceCharge,
            ReportKind::BrokenAppointments => FeatureKey::ReportBrokenAppointments,
            ReportKind::Confirmation => FeatureKey::ReportConfirmations,
            ReportKind::RecallFields => FeatureKey::ReportRecallFields,
            ReportKind::DailyProcedures => FeatureKey::ReportDailyProcedures,
            ReportKind::ProcedureCodes => FeatureKey::ReportProcedureCodes,
            ReportKind::Prescriptions => FeatureKey::ReportPrescriptions,
            ReportKind::ReferralAnalysis => FeatureKey::ReportReferralAnalysis,
            ReportKind::Referrals => FeatureKey::ReportReferrals,
            ReportKind::CustomAging => FeatureKey::ReportCustomAging,
            ReportKind::Patients => FeatureKey::ReportPatients,
            ReportKind::BalanceSheet => FeatureKey::ReportBalanceSheet,
            ReportKind::GeneralLedger => FeatureKey::ReportGeneralLedger,
            ReportKind::ProfitLoss => FeatureKey::ReportProfitLoss,
            ReportKind::JournalPostingAudit => FeatureKey::LedgerJournalPosting,
            ReportKind::LedgerTieOut => FeatureKey::LedgerJournalPosting,
            ReportKind::PeriodCloseHistory => FeatureKey::LedgerJournalPosting,
            ReportKind::HiddenPaySplits => FeatureKey::ReportHiddenPaySplits,
            ReportKind::UnearnedIncome => FeatureKey::ReportUnearnedIncome,
            ReportKind::ProcOverpaid => FeatureKey::ReportProcOverpaid,
            ReportKind::ReceivablesBreakdown => FeatureKey::ReportReceivablesBreakdown,
            ReportKind::ServiceDateView => FeatureKey::ReportServiceDateView,
            ReportKind::ProcNote => FeatureKey::ReportProcNote,
            ReportKind::DentalSealant => FeatureKey::ReportDentalSealant,
            ReportKind::DiscountPlan => FeatureKey::ReportDiscountPlan,
            ReportKind::DppOvercharged => FeatureKey::ReportDppOvercharged,
            ReportKind::PatientPortionUncollected => FeatureKey::ReportPatientPortionUncollected,
            ReportKind::TreatmentFinder => FeatureKey::ReportTreatmentFinder,
            ReportKind::PhRawPop => FeatureKey::ReportPhRawPop,
            ReportKind::PhRawScreen => FeatureKey::ReportPhRawScreen,
            ReportKind::Routing => FeatureKey::ReportRouting,
            ReportKind::LaserLabels => FeatureKey::ReportLaserLabels,
            ReportKind::PrintPreview => FeatureKey::ReportPrintPreview,
            ReportKind::Statement => FeatureKey::ReportStatement,
            ReportKind::ProdGoal => FeatureKey::ReportProdGoal,
            ReportKind::ProviderPayroll => FeatureKey::ReportProviderPayroll,
            ReportKind::PresentedTreatment => FeatureKey::ReportPresentedTreatment,
            ReportKind::TpPresentationStats => FeatureKey::ReportTpPresentationStats,
        }
    }

    /// Balance/summary reports use the range end as an "as of" date and do not
    /// filter source rows by the start date.
    fn uses_date_range(self) -> bool {
        matches!(
            self,
            ReportKind::Production
                | ReportKind::Collections
                | ReportKind::Schedule
                | ReportKind::Birthday
                | ReportKind::NewPatients
                | ReportKind::Appointments
                | ReportKind::ActivePatients
                | ReportKind::ProdInc
                | ReportKind::PaySheet
                | ReportKind::AdjSheet
                | ReportKind::FinanceCharge
                | ReportKind::BrokenAppointments
                | ReportKind::Confirmation
                | ReportKind::RecallFields
                | ReportKind::DailyProcedures
                | ReportKind::ReferralAnalysis
                | ReportKind::BalanceSheet
                | ReportKind::GeneralLedger
                | ReportKind::ProfitLoss
                | ReportKind::JournalPostingAudit
                | ReportKind::LedgerTieOut
                | ReportKind::HiddenPaySplits
                | ReportKind::UnearnedIncome
                | ReportKind::ProcOverpaid
                | ReportKind::ReceivablesBreakdown
                | ReportKind::ServiceDateView
                | ReportKind::ProcNote
                | ReportKind::DentalSealant
                | ReportKind::DppOvercharged
                | ReportKind::PatientPortionUncollected
                | ReportKind::TreatmentFinder
                | ReportKind::PayPlanChargeApplications
                | ReportKind::Routing
                | ReportKind::LaserLabels
                | ReportKind::ProdGoal
                | ReportKind::ProviderPayroll
                | ReportKind::PresentedTreatment
                | ReportKind::TpPresentationStats
        )
    }

    fn from_file_stem(stem: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.file_stem() == stem)
    }

    fn file_stem(self) -> &'static str {
        match self {
            ReportKind::PatientList => "patient-list",
            ReportKind::Production => "production",
            ReportKind::Collections => "collections",
            ReportKind::Schedule => "schedule",
            ReportKind::RecallAsap => "recall-asap",
            ReportKind::PaymentPlans => "payment-plans",
            ReportKind::TreatmentPlans => "treatment-plans",
            ReportKind::Aging => "patient-aging",
            ReportKind::PayPlanDetail => "payment-plan-detail",
            ReportKind::PayPlanChargeApplications => "payment-plan-charge-applications",
            ReportKind::Birthday => "birthdays",
            ReportKind::NewPatients => "new-patients",
            ReportKind::Appointments => "appointments",
            ReportKind::ActivePatients => "active-patients",
            ReportKind::ProdInc => "production-income",
            ReportKind::PaySheet => "daily-payments",
            ReportKind::AdjSheet => "daily-adjustments",
            ReportKind::FinanceCharge => "finance-charge",
            ReportKind::BrokenAppointments => "broken-appointments",
            ReportKind::Confirmation => "confirmations",
            ReportKind::RecallFields => "recall-fields",
            ReportKind::DailyProcedures => "daily-procedures",
            ReportKind::ProcedureCodes => "procedure-codes",
            ReportKind::Prescriptions => "prescriptions",
            ReportKind::ReferralAnalysis => "referral-analysis",
            ReportKind::Referrals => "referrals",
            ReportKind::CustomAging => "custom-aging",
            ReportKind::Patients => "patients",
            ReportKind::BalanceSheet => "balance-sheet",
            ReportKind::GeneralLedger => "general-ledger",
            ReportKind::ProfitLoss => "profit-loss",
            ReportKind::JournalPostingAudit => "journal-posting-audit",
            ReportKind::LedgerTieOut => "ledger-tie-out",
            ReportKind::PeriodCloseHistory => "period-close-history",
            ReportKind::HiddenPaySplits => "hidden-pay-splits",
            ReportKind::UnearnedIncome => "unearned-income",
            ReportKind::ProcOverpaid => "overpaid-procedures",
            ReportKind::ReceivablesBreakdown => "receivables-breakdown",
            ReportKind::ServiceDateView => "service-date-view",
            ReportKind::ProcNote => "incomplete-procedure-notes",
            ReportKind::DentalSealant => "dental-sealant-measure",
            ReportKind::DiscountPlan => "discount-plans",
            ReportKind::DppOvercharged => "overcharged-payment-plans",
            ReportKind::PatientPortionUncollected => "patient-portion-uncollected",
            ReportKind::TreatmentFinder => "treatment-finder",
            ReportKind::PhRawPop => "raw-population",
            ReportKind::PhRawScreen => "raw-screening",
            ReportKind::Routing => "routing-slips",
            ReportKind::LaserLabels => "laser-labels",
            ReportKind::PrintPreview => "print-preview",
            ReportKind::Statement => "statements",
            ReportKind::ProdGoal => "production-goal",
            ReportKind::ProviderPayroll => "provider-payroll",
            ReportKind::PresentedTreatment => "presented-treatment-production",
            ReportKind::TpPresentationStats => "plan-presentation-statistics",
        }
    }
}

/// Two-list field picker mirroring the `FormRpConfirm` / `FormRpRecall`
/// listSelect controls. Returns whether the selection changed. The selection is
/// always re-sorted into the upstream canonical order (standard fields first),
/// so exported column order is stable regardless of click order.
fn field_selection_ui(
    ui: &mut Ui,
    selected: &mut Vec<String>,
    standard: &[&str],
    other: &[&str],
) -> bool {
    let canonical: Vec<&str> = standard.iter().chain(other.iter()).copied().collect();
    let mut changed = false;
    for (label, fields) in [("Standard fields", standard), ("Other fields", other)] {
        ui.collapsing(label, |ui| {
            for &field in fields {
                let mut checked = selected.iter().any(|f| f == field);
                if ui.checkbox(&mut checked, field).changed() {
                    changed = true;
                    if checked {
                        selected.push(field.to_string());
                    } else {
                        selected.retain(|f| f != field);
                    }
                }
            }
        });
    }
    if changed {
        selected.sort_by_key(|f| canonical.iter().position(|c| *c == f).unwrap_or(usize::MAX));
    }
    changed
}

struct OdApp {
    store: Arc<dyn Store>,
    db_label: String,
    features: FeatureSettings,
    module: Module,
    show_settings: bool,

    patients: Vec<Patient>,
    providers: Vec<Provider>,
    operatories: Vec<Operatory>,
    appointments: Vec<Appointment>,
    appt_tab: ApptTab,
    recalls: Vec<Recall>,
    asap_appointments: Vec<Appointment>,
    /// OD's unscheduled list (`AptStatus = 3`), shown as the clinic's waitlist.
    waitlist: Vec<Appointment>,
    /// OD `histappointment` rows for the appointment whose History button was used.
    apt_history: Vec<AppointmentChange>,
    apt_history_open: bool,
    apt_history_num: Option<Id>,
    recall_edit: Option<Recall>,
    recall_edit_open: bool,
    recall_due_text: String,
    recall_years_text: String,
    recall_months_text: String,
    recall_weeks_text: String,
    recall_days_text: String,
    recall_status_text: String,

    search: String,
    selected_patient: Option<Id>,
    schedule_day: NaiveDate,

    patient_edit: Option<Patient>,
    patient_edit_open: bool,
    birth_text: String,
    /// Reason recorded with a patient correction snapshot.
    patient_reason_text: String,
    /// Port-local patient correction snapshots for the selected patient.
    patient_corrections: Vec<PatientCorrection>,
    patient_history_open: bool,
    patient_history_pat_num: Option<Id>,

    // Integrated patient registry (reached from Treatment Plan and Family).
    registry_open: bool,
    registry_patient: Option<Id>,
    registry_tab: RegistryTab,
    registry_conditions: Vec<Disease>,
    registry_treatments: Vec<RegistryTreatment>,
    registry_note: Option<PatientNote>,
    registry_cards: Vec<ClinicCard>,
    registry_card_reason: ClinicCardReason,
    registry_card_auth: String,
    registry_card_note: String,
    registry_card_ack: bool,
    registry_invoice_tax_text: String,
    registry_invoice_note: String,
    registry_invoice_pdf_path: String,
    registry_invoice_pdf_patient: Id,

    apt_edit: Option<Appointment>,
    apt_edit_open: bool,
    apt_text: String,
    apt_len_text: String,

    // Account/billing entry state.
    adj_edit: Option<Adjustment>,
    adj_edit_open: bool,
    adj_date_text: String,
    adj_amount_text: String,
    adj_note_text: String,
    /// Explicit "post to ledger" path into the double-entry journal, for both
    /// adjustments and patient payments.
    journal_post_edit: Option<JournalPostSource>,
    journal_post_open: bool,
    journal_debit_account: Id,
    journal_credit_account: Id,
    journal_transaction_num: Id,
    journal_memo_text: String,
    /// Period-close batch posting of charges, payments and adjustments in the
    /// report range. The window previews the whole plan, then writes every
    /// balanced pair in one DB transaction.
    post_all_open: bool,
    post_all_defaults: PostingAccountDefaults,
    /// Signed-off accounting months, persisted in the OD preference table and
    /// enforced by every journal posting window until explicitly reopened.
    closed_periods: ClosedPeriods,
    period_action_open: bool,
    period_action: Option<(PeriodAction, chrono::NaiveDate)>,
    period_action_reason: String,
    payment_edit: Option<Payment>,
    payment_edit_open: bool,
    payment_date_text: String,
    payment_amount_text: String,
    payment_rows: Vec<PaymentRow>,
    /// Where "Save PDF" writes the current patient's statement.
    statement_pdf_path: String,
    statement_pdf_patient: Id,
    account_show_refunds_transfers_only: bool,
    /// Stale-resolved plan charge rows stay visible by default for audit.
    account_show_resolved_plan_charges: bool,
    account_transfer_plan_filter: Id,
    account_transfer_target_filter: PaymentTarget,
    account_transfer_from_text: String,
    account_transfer_to_text: String,
    account_ledger_export_path: String,
    /// Patient whose ledger path is currently loaded, so switching accounts
    /// reloads that account's own filing location.
    account_ledger_export_patient: Id,
    /// Path for the patient-scoped audit bundle (reasons + security log).
    account_audit_export_path: String,
    /// Patient whose audit-bundle path is currently loaded.
    account_audit_export_patient: Id,
    plan_refund_transfer_target: PaymentTarget,

    // Payment plan state.
    plan_recon_as_of_text: String,
    /// Staff-entered reason for each plan's stale-charge resolution, keyed by
    /// `PayPlanNum`; required before "Resolve stale charges (safe)" is allowed.
    stale_resolution_reasons: HashMap<Id, String>,
    plan_edit: Option<PayPlan>,
    plan_edit_open: bool,
    plan_amount_text: String,
    plan_count_text: String,
    plan_start_text: String,
    plan_apr_text: String,
    plan_down_payment_text: String,
    plan_interest_start_text: String,
    plan_note_text: String,
    plan_frequency: PayPlanFrequency,
    plan_is_dynamic: bool,
    plan_tp_option: DynamicPayPlanTPOption,
    plan_is_closed: bool,
    plan_is_locked: bool,
    plan_link_target: Id,
    plan_link_type: i8,
    plan_charge_edit: Option<PayPlanCharge>,
    plan_charge_edit_open: bool,
    plan_charge_date_text: String,
    plan_charge_amount_text: String,

    // Procedure entry/completion state (Chart). Completing a procedure is the
    // trigger OD uses to price it from a discount plan and post an adjustment.
    proc_edit: Option<Procedure>,
    proc_edit_open: bool,
    proc_date_text: String,
    proc_code_num: Id,
    proc_fee_text: String,
    proc_tooth_text: String,
    proc_units_text: String,

    // Discount plan state (Setup → Discount plans). Plans price procedures;
    // subscriptions attach a patient to a plan over an effective/term range.
    discount_plans: Vec<DiscountPlan>,
    discount_plan_subs: Vec<DiscountPlanSub>,
    discount_plan_edit: Option<DiscountPlan>,
    discount_plan_edit_open: bool,
    discount_sub_edit: Option<DiscountPlanSub>,
    discount_sub_edit_open: bool,
    discount_sub_pat_num: Id,
    discount_sub_effective_text: String,
    discount_sub_term_text: String,
    /// Description filter for the Discount Plan report.
    discount_report_filter: String,

    // Treatment plan state. OD creates Inactive plans and promotes them to
    // Active; Saved plans are immutable proctp snapshots.
    treat_plan_edit: Option<TreatPlan>,
    treat_plan_edit_open: bool,
    treat_plan_heading_text: String,
    treat_plan_note_text: String,
    treat_plan_date_text: String,
    treat_plan_status: TreatPlanStatus,
    treat_plan_line_edit: Option<TreatPlanLine>,
    treat_plan_line_edit_open: bool,
    treat_plan_line_plan_num: Id,
    treat_plan_line_proc_num: Id,
    treat_plan_line_code_text: String,
    treat_plan_line_desc_text: String,
    treat_plan_line_tooth_text: String,
    treat_plan_line_fee_text: String,

    report: ReportKind,
    /// Rows behind the interactive Collections worklist. The exported report
    /// grid only carries strings, so actions read the patient numbers here.
    collections_rows: Vec<CollectionsRow>,
    /// Active aging band in the Collections triage strip. `None` shows all
    /// bands; filtering only removes whole patient rows.
    collections_bucket_filter: Option<CollectionsAgingBucket>,
    report_from: NaiveDate,
    report_to: NaiveDate,
    /// Editable ISO copies of the reporting window. Invalid text is kept on
    /// screen until corrected, while the parsed dates drive the reports.
    report_from_text: String,
    report_to_text: String,
    report_export_path: String,
    report_export_kind: Option<ReportKind>,
    /// Report whose saved sort was last loaded into the live sort fields.
    report_sort_kind: Option<ReportKind>,
    /// Dedicated path for the payment-plan stale-resolved audit export, kept
    /// separate from the general report export so both files survive.
    pay_plan_audit_export_path: String,
    /// Practice name printed on exports and persisted in reports.txt.
    report_prefs: ReportPreferences,
    /// Click-to-sort state for the report grid; restored per user and report,
    /// with the column index validated against the current table's headers.
    report_sort_column: usize,
    report_sort_desc: bool,
    /// Filters for the read-only Period close history report. Blank fields and
    /// `All` show the full register; exports always follow the visible rows.
    period_close_history_user_filter: String,
    period_close_history_month_filter: String,
    period_close_history_action_filter: PeriodCloseActionFilter,
    /// Draft WHERE prerequisite editor for the FormRpReferrals options panel.
    referrals_filter_field: String,
    referrals_filter_condition: ReportFilterCondition,
    referrals_filter_value: String,
    referrals_selected_filter: Option<usize>,
    /// Draft WHERE prerequisite editor for the FormRpPatients options panel.
    patients_filter_field: String,
    patients_filter_condition: ReportFilterCondition,
    patients_filter_value: String,
    patients_selected_filter: Option<usize>,

    selected_tooth: Option<String>,
    notice: Option<String>,
    error: Option<String>,

    // Security state. `session` is None while the login screen is showing.
    session: Option<Session>,
    users: Vec<Userod>,
    group_permissions: Vec<GroupPermission>,
    group_attaches: Vec<UserGroupAttach>,
    security_log: Vec<SecurityEvent>,
    security_log_filter: String,
    /// Optional patient/account number, so an exported audit view can be
    /// scoped to one patient and filed against that record.
    security_log_patient_text: String,
    security_log_export_path: String,
    /// Patient filter currently applied to the persisted security-log path
    /// (`0` means the practice-wide, unscoped path).
    security_log_export_patient: Id,
    login_name: String,
    login_pass: String,
    login_error: Option<String>,

    // eServices state. All rule evaluation and delivery is local; nothing here
    // talks to an Open Dental HQ server.
    eservices_config: EServicesConfig,
    reminder_rules: Vec<ReminderRule>,
    patient_contacts: Vec<PatientContact>,
    outbox: Vec<OutboxMessage>,
    sent_keys: HashSet<(Id, Id)>,
    eservices_tab: EServicesTab,
    send_message_open: bool,
    manual_patient: Option<Id>,
    manual_transport: TransportKind,
    manual_text: String,
    rule_edit: Option<ReminderRule>,
    rule_edit_open: bool,
    rule_minutes_text: String,
    rule_dnd_text: String,
    rule_sms_template: String,
    rule_email_subject: String,
    rule_email_body: String,
    rule_comm_type: CommType,
    rule_type: ReminderType,

    // Definitions and display fields (OD Setup → Definitions / Display Fields).
    definitions_tab: DefinitionsTab,
    definitions: Vec<Definition>,
    definition_category: DefinitionCategory,
    definitions_show_hidden: bool,
    definition_edit: Option<Definition>,
    definition_edit_open: bool,
    definition_name_text: String,
    definition_value_text: String,
    definition_color_text: String,
    display_fields: Vec<DisplayField>,
    display_field_category: DisplayFieldCategory,
    display_field_edit: Option<DisplayField>,
    display_field_edit_open: bool,
    display_field_name_text: String,
    display_field_label_text: String,
    display_field_width_text: String,
}

impl OdApp {
    fn new() -> Self {
        let features = FeatureSettings::load();
        let startup_user_key = od_domain::period_close_history_user_key(None);
        let (store, db_label) = if features.enabled(FeatureKey::PreferDemoData) {
            (
                Arc::new(Backend::Demo(DemoStore::seeded())) as Arc<dyn Store>,
                "Demo data (forced in Settings)".to_string(),
            )
        } else {
            match std::env::var("ODCLINIC_DB_URL") {
                Ok(url) => match MySqlStore::new(&url) {
                    Ok(store) => (
                        Arc::new(Backend::MySql(store)) as Arc<dyn Store>,
                        "MySQL (ODCLINIC_DB_URL)".to_string(),
                    ),
                    Err(err) => (
                        Arc::new(Backend::Demo(DemoStore::seeded())) as Arc<dyn Store>,
                        format!("Demo data — MySQL failed: {err}"),
                    ),
                },
                Err(_) => (
                    Arc::new(Backend::Demo(DemoStore::seeded())) as Arc<dyn Store>,
                    "Demo data (set ODCLINIC_DB_URL for MySQL)".to_string(),
                ),
            }
        };
        let today = chrono::Local::now().date_naive();
        let login_required = features.enabled(FeatureKey::RequireLogin);
        let users = store.list_users().unwrap_or_default();
        let group_permissions = store.list_group_permissions().unwrap_or_default();
        let group_attaches = store.list_user_group_attaches().unwrap_or_default();
        let security_log = store.list_security_log(50).unwrap_or_default();
        let report_prefs = ReportPreferences::load();
        let closed_periods = ClosedPeriods::from_prefs(&store.list_prefs().unwrap_or_default());
        let mut app = Self {
            store,
            db_label,
            features,
            module: Module::Appointments,
            show_settings: false,
            patients: Vec::new(),
            providers: Vec::new(),
            operatories: Vec::new(),
            appointments: Vec::new(),
            appt_tab: ApptTab::Schedule,
            recalls: Vec::new(),
            asap_appointments: Vec::new(),
            waitlist: Vec::new(),
            apt_history: Vec::new(),
            apt_history_open: false,
            apt_history_num: None,
            recall_edit: None,
            recall_edit_open: false,
            recall_due_text: String::new(),
            recall_years_text: String::new(),
            recall_months_text: String::new(),
            recall_weeks_text: String::new(),
            recall_days_text: String::new(),
            recall_status_text: String::new(),
            search: String::new(),
            selected_patient: None,
            schedule_day: today,
            patient_edit: None,
            patient_edit_open: false,
            birth_text: String::new(),
            patient_reason_text: String::new(),
            patient_corrections: Vec::new(),
            patient_history_open: false,
            patient_history_pat_num: None,
            registry_open: false,
            registry_patient: None,
            registry_tab: RegistryTab::Overview,
            registry_conditions: Vec::new(),
            registry_treatments: Vec::new(),
            registry_note: None,
            registry_cards: Vec::new(),
            registry_card_reason: ClinicCardReason::CorporateAccount,
            registry_card_auth: String::new(),
            registry_card_note: String::new(),
            registry_card_ack: false,
            registry_invoice_tax_text: String::new(),
            registry_invoice_note: String::new(),
            registry_invoice_pdf_path: String::new(),
            registry_invoice_pdf_patient: 0,
            apt_edit: None,
            apt_edit_open: false,
            apt_text: String::new(),
            apt_len_text: String::new(),
            adj_edit: None,
            adj_edit_open: false,
            adj_date_text: String::new(),
            adj_amount_text: String::new(),
            adj_note_text: String::new(),
            journal_post_edit: None,
            journal_post_open: false,
            journal_debit_account: 0,
            journal_credit_account: 0,
            journal_transaction_num: 0,
            journal_memo_text: String::new(),
            post_all_open: false,
            post_all_defaults: PostingAccountDefaults {
                charge: (0, 0),
                payment: (0, 0),
                positive_adjustment: (0, 0),
                negative_adjustment: (0, 0),
            },
            closed_periods,
            period_action_open: false,
            period_action: None,
            period_action_reason: String::new(),
            payment_edit: None,
            payment_edit_open: false,
            payment_date_text: String::new(),
            payment_amount_text: String::new(),
            payment_rows: Vec::new(),
            statement_pdf_path: String::new(),
            statement_pdf_patient: 0,
            account_show_refunds_transfers_only: false,
            account_show_resolved_plan_charges: true,
            account_transfer_plan_filter: 0,
            account_transfer_target_filter: PaymentTarget::Unallocated,
            account_transfer_from_text: String::new(),
            account_transfer_to_text: String::new(),
            account_ledger_export_path: String::new(),
            account_ledger_export_patient: 0,
            account_audit_export_path: String::new(),
            account_audit_export_patient: 0,
            plan_refund_transfer_target: PaymentTarget::Unallocated,
            plan_recon_as_of_text: chrono::Local::now().format("%Y-%m-%d").to_string(),
            stale_resolution_reasons: HashMap::new(),
            plan_edit: None,
            plan_edit_open: false,
            plan_amount_text: String::new(),
            plan_count_text: String::new(),
            plan_start_text: String::new(),
            plan_apr_text: String::new(),
            plan_down_payment_text: String::new(),
            plan_interest_start_text: String::new(),
            plan_note_text: String::new(),
            plan_frequency: PayPlanFrequency::Monthly,
            plan_is_dynamic: false,
            plan_tp_option: DynamicPayPlanTPOption::None,
            plan_is_closed: false,
            plan_is_locked: false,
            plan_link_target: 0,
            plan_link_type: PayPlanLink::PROCEDURE,
            plan_charge_edit: None,
            plan_charge_edit_open: false,
            plan_charge_date_text: String::new(),
            plan_charge_amount_text: String::new(),
            proc_edit: None,
            proc_edit_open: false,
            proc_date_text: today.format("%Y-%m-%d").to_string(),
            proc_code_num: 0,
            proc_fee_text: String::new(),
            proc_tooth_text: String::new(),
            proc_units_text: "1".into(),
            discount_plans: Vec::new(),
            discount_plan_subs: Vec::new(),
            discount_plan_edit: None,
            discount_plan_edit_open: false,
            discount_sub_edit: None,
            discount_sub_edit_open: false,
            discount_sub_pat_num: 0,
            discount_sub_effective_text: String::new(),
            discount_sub_term_text: String::new(),
            discount_report_filter: String::new(),
            treat_plan_edit: None,
            treat_plan_edit_open: false,
            treat_plan_heading_text: String::new(),
            treat_plan_note_text: String::new(),
            treat_plan_date_text: String::new(),
            treat_plan_status: TreatPlanStatus::Inactive,
            treat_plan_line_edit: None,
            treat_plan_line_edit_open: false,
            treat_plan_line_plan_num: 0,
            treat_plan_line_proc_num: 0,
            treat_plan_line_code_text: String::new(),
            treat_plan_line_desc_text: String::new(),
            treat_plan_line_tooth_text: String::new(),
            treat_plan_line_fee_text: String::new(),
            report: ReportKind::Production,
            collections_rows: Vec::new(),
            collections_bucket_filter: None,
            report_from: today - Duration::days(30),
            report_to: today,
            report_from_text: (today - Duration::days(30)).format("%Y-%m-%d").to_string(),
            report_to_text: today.format("%Y-%m-%d").to_string(),
            report_export_path: String::new(),
            report_export_kind: None,
            report_sort_kind: None,
            pay_plan_audit_export_path: persisted_export_path(
                &report_prefs,
                &startup_user_key,
                EXPORT_KEY_PAY_PLAN_AUDIT,
            ),
            report_prefs,
            report_sort_column: 0,
            report_sort_desc: false,
            period_close_history_user_filter: String::new(),
            period_close_history_month_filter: String::new(),
            period_close_history_action_filter: PeriodCloseActionFilter::All,
            referrals_filter_field: "LName".into(),
            referrals_filter_condition: ReportFilterCondition::Like,
            referrals_filter_value: String::new(),
            referrals_selected_filter: None,
            patients_filter_field: "LName".into(),
            patients_filter_condition: ReportFilterCondition::Like,
            patients_filter_value: String::new(),
            patients_selected_filter: None,
            selected_tooth: None,
            notice: None,
            error: None,
            session: if login_required { None } else { Some(Session::local()) },
            users,
            group_permissions,
            group_attaches,
            security_log,
            security_log_filter: String::new(),
            security_log_patient_text: String::new(),
            security_log_export_path: String::new(),
            security_log_export_patient: 0,
            login_name: String::new(),
            login_pass: String::new(),
            login_error: None,
            eservices_config: EServicesConfig::load(),
            reminder_rules: Vec::new(),
            patient_contacts: Vec::new(),
            outbox: Vec::new(),
            sent_keys: HashSet::new(),
            eservices_tab: EServicesTab::Channels,
            send_message_open: false,
            manual_patient: None,
            manual_transport: TransportKind::Smtp,
            manual_text: String::new(),
            rule_edit: None,
            rule_edit_open: false,
            rule_minutes_text: String::new(),
            rule_dnd_text: String::new(),
            rule_sms_template: String::new(),
            rule_email_subject: String::new(),
            rule_email_body: String::new(),
            rule_comm_type: CommType::Preferred,
            rule_type: ReminderType::Reminder,
            definitions_tab: DefinitionsTab::Definitions,
            definitions: Vec::new(),
            definition_category: DefinitionCategory::PAYMENT_TYPES,
            definitions_show_hidden: false,
            definition_edit: None,
            definition_edit_open: false,
            definition_name_text: String::new(),
            definition_value_text: String::new(),
            definition_color_text: "0".into(),
            display_fields: Vec::new(),
            display_field_category: DisplayFieldCategory::PATIENT_SELECT,
            display_field_edit: None,
            display_field_edit_open: false,
            display_field_name_text: String::new(),
            display_field_label_text: String::new(),
            display_field_width_text: "80".into(),
        };
        app.load_period_close_history_filters();
        app.load_report_user_view();
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        self.closed_periods = ClosedPeriods::from_prefs(&self.store.list_prefs().unwrap_or_default());
        match self.store.list_patients() {
            Ok(rows) => self.patients = rows,
            Err(e) => self.error = Some(e),
        }
        if let Ok(rows) = self.store.list_providers() {
            self.providers = rows;
        }
        if let Ok(rows) = self.store.list_operatories() {
            self.operatories = rows;
        }
        let from = self.schedule_day.and_hms_opt(0, 0, 0).unwrap_or_default();
        let to = from + Duration::days(1);
        match self.store.list_appointments(from, to) {
            Ok(rows) => self.appointments = rows,
            Err(e) => self.error = Some(e),
        }
        if let Ok(rows) = self.store.list_recalls() {
            self.recalls = rows;
        }
        if let Ok(rows) = self.store.list_asap_appointments() {
            self.asap_appointments = rows;
        }
        if let Ok(rows) = self.store.list_reminder_rules() {
            self.reminder_rules = rows;
        }
        if let Ok(rows) = self.store.list_patient_contacts() {
            self.patient_contacts = rows;
        }
        if let Ok(rows) = self.store.list_outbox(200) {
            self.outbox = rows;
        }
        if let Ok(rows) = self.store.list_reminder_sent() {
            self.sent_keys = rows.into_iter().collect();
        }
        if let Ok(rows) = self.store.list_definitions() {
            self.definitions = rows;
        }
        if let Ok(rows) = self.store.list_discount_plans() {
            self.discount_plans = rows;
        }
        if let Ok(rows) = self.store.list_discount_plan_subs() {
            self.discount_plan_subs = rows;
        }
        if let Ok(rows) = self.store.list_display_fields() {
            self.display_fields = rows;
        }
        if let Ok(rows) = self.store.list_waitlist_appointments() {
            self.waitlist = rows;
        }
        if let Some(pat_num) = self.selected_patient {
            if let Ok(rows) = self.store.list_patient_corrections(pat_num) {
                self.patient_corrections = rows;
            }
        }
        if let Some(apt_num) = self.apt_history_num {
            if let Ok(rows) = self.store.list_appointment_changes(apt_num) {
                self.apt_history = rows;
            }
        }
        if !self.appt_tab.enabled(&self.features) {
            self.appt_tab = ApptTab::Schedule;
        }
    }

    // ------------------------------------------------------------ Export paths

    /// Persist a dedicated export path for the signed-in user, so each user's
    /// filing location survives a restart or login switch. Skips redundant
    /// writes; a failed save is surfaced without hiding a successful file write
    /// that may already have happened.
    fn remember_export_path(&mut self, key: &str, path: &str) {
        let user_key = self.period_close_history_user_key();
        if self.report_prefs.user_export_path(&user_key, key) == Some(path) {
            return;
        }
        self.report_prefs.set_user_export_path(user_key, key, path);
        if let Err(err) = self.save_report_prefs() {
            self.error = Some(format!("Could not save export path preference: {err}"));
        }
    }

    /// Stable `reports.txt` key for the current user's history view.
    fn period_close_history_user_key(&self) -> String {
        od_domain::period_close_history_user_key(self.session.as_ref())
    }

    /// Restore the filters saved for the user who is now signed in. Missing or
    /// unreadable preferences leave the unfiltered register on screen.
    fn load_period_close_history_filters(&mut self) {
        let key = self.period_close_history_user_key();
        match self.report_prefs.period_close_history_view(&key) {
            Some(view) => {
                self.period_close_history_user_filter = view.user.clone();
                self.period_close_history_month_filter = view.month.clone();
                self.period_close_history_action_filter =
                    PeriodCloseActionFilter::from_storage(&view.action);
            }
            None => {
                self.period_close_history_user_filter.clear();
                self.period_close_history_month_filter.clear();
                self.period_close_history_action_filter = PeriodCloseActionFilter::All;
            }
        }
    }

    /// Save the currently visible filters for this user. Edits are persisted as
    /// they are made, so switching users or restarting never loses the view.
    fn persist_period_close_history_filters(&mut self) {
        let key = self.period_close_history_user_key();
        let view = PeriodCloseHistoryView {
            user: self.period_close_history_user_filter.clone(),
            month: self.period_close_history_month_filter.clone(),
            action: self.period_close_history_action_filter.storage_key().to_string(),
        };
        if self.report_prefs.period_close_history_view(&key) == Some(&view) {
            return;
        }
        self.report_prefs.set_period_close_history_view(key, view);
        if let Err(err) = self.save_report_prefs() {
            self.error = Some(format!("Could not save history filters: {err}"));
        }
    }

    /// Snapshot the active user's report options, then write `reports.txt`.
    /// Every option edit and preference write goes through here, so one user's
    /// choices cannot overwrite another user's saved set.
    fn save_report_prefs(&mut self) -> Result<(), String> {
        let key = self.period_close_history_user_key();
        self.report_prefs.snapshot_user_options(&key);
        self.report_prefs.save()
    }

    /// Restore the signed-in user's report options and reporting window. Called
    /// on startup and after each login/logout switch.
    fn load_report_user_view(&mut self) {
        let key = self.period_close_history_user_key();
        self.report_prefs.restore_user_options(&key);
        if let Some(range) = self.report_prefs.report_date_range(&key) {
            if let (Ok(from), Ok(to)) = (
                NaiveDate::parse_from_str(&range.from, "%Y-%m-%d"),
                NaiveDate::parse_from_str(&range.to, "%Y-%m-%d"),
            ) {
                self.show_report_dates(from, to);
            }
        }
        if let Some(stem) = self.report_prefs.selected_report_kind(&key).map(str::to_string) {
            if let Some(kind) = ReportKind::from_file_stem(&stem)
                .filter(|kind| self.features.enabled(kind.feature_key()))
            {
                self.report = kind;
            }
        }
        self.report_export_path = self
            .report_prefs
            .effective_export_path(&key, self.report.file_stem())
            .map(str::to_string)
            .unwrap_or_else(|| default_report_path(self.report, "csv"));
        self.report_export_kind = Some(self.report);
        self.load_report_sort(self.report);
        self.collections_bucket_filter =
            collections_band_from_storage(self.report_prefs.collections_bucket(&key));
    }

    /// Restore the saved column sort for one report. Called on startup, after a
    /// login switch, and when the Reports picker changes.
    fn load_report_sort(&mut self, kind: ReportKind) {
        let key = self.period_close_history_user_key();
        let sort = self.report_prefs.report_sort(&key, kind.file_stem());
        self.report_sort_column = sort.column;
        self.report_sort_desc = sort.descending;
        self.report_sort_kind = Some(kind);
    }

    /// Persist the live sort order for one report so it survives a restart or a
    /// switch back from another user.
    fn persist_report_sort(&mut self, report_key: &str) {
        let user_key = self.period_close_history_user_key();
        let sort = ReportSortState {
            column: self.report_sort_column,
            descending: self.report_sort_desc,
        };
        if self.report_prefs.report_sort(&user_key, report_key) == sort {
            return;
        }
        self.report_prefs.set_report_sort(user_key, report_key, sort);
        if let Err(err) = self.save_report_prefs() {
            self.error = Some(format!("Could not save report sort preference: {err}"));
        }
    }

    /// Stage the live Collections band for one user. Returns whether anything
    /// changed, so a login switch can save the outgoing view without forcing a
    /// redundant write to `reports.txt`.
    fn remember_collections_bucket(&mut self, user_key: &str) -> bool {
        let band = collections_band_to_storage(self.collections_bucket_filter);
        if self.report_prefs.collections_bucket(user_key) == band {
            return false;
        }
        self.report_prefs.set_collections_bucket(user_key, band);
        true
    }

    /// Persist the selected band so a restart restores the same triage view.
    fn persist_collections_bucket(&mut self) {
        let user_key = self.period_close_history_user_key();
        if !self.remember_collections_bucket(&user_key) {
            return;
        }
        if let Err(err) = self.save_report_prefs() {
            self.error = Some(format!("Could not save Collections aging band preference: {err}"));
        }
    }

    /// Replace both the live dates and their editable text. Loading a saved
    /// range uses this without writing, so an invalid saved value is not copied
    /// back into `reports.txt`.
    fn show_report_dates(&mut self, from: NaiveDate, to: NaiveDate) {
        self.report_from = from;
        self.report_to = to;
        self.report_from_text = from.format("%Y-%m-%d").to_string();
        self.report_to_text = to.format("%Y-%m-%d").to_string();
    }

    /// Apply a user-chosen range and save it for the signed-in user.
    fn apply_report_dates(&mut self, from: NaiveDate, to: NaiveDate) {
        if from > to {
            return;
        }
        self.show_report_dates(from, to);
        self.persist_report_date_range();
    }

    /// Parse the visible date fields without writing anything. Range reports use
    /// both fields; "as of" reports only show the end date, so an earlier end
    /// clamps the hidden start to keep the saved window ordered.
    fn parsed_report_dates(&self, uses_date_range: bool) -> Option<(NaiveDate, NaiveDate)> {
        let to = NaiveDate::parse_from_str(self.report_to_text.trim(), "%Y-%m-%d").ok()?;
        let from = if uses_date_range {
            NaiveDate::parse_from_str(self.report_from_text.trim(), "%Y-%m-%d").ok()?
        } else {
            self.report_from.min(to)
        };
        (from <= to).then_some((from, to))
    }

    /// Turn the editable text fields into the live dates when the visible fields
    /// are valid. Partial or reversed input stays on screen with a hint until
    /// corrected.
    fn commit_report_date_texts(&mut self, uses_date_range: bool) {
        if let Some((from, to)) = self.parsed_report_dates(uses_date_range) {
            self.apply_report_dates(from, to);
        }
    }

    fn report_date_text_valid(&self, uses_date_range: bool) -> bool {
        self.parsed_report_dates(uses_date_range).is_some()
    }

    /// Remember the selected report for the signed-in user. A saved report whose
    /// feature is turned off is not overwritten by the temporary fallback.
    fn persist_selected_report_kind(&mut self) {
        let key = self.period_close_history_user_key();
        let stem = self.report.file_stem();
        if self.report_prefs.selected_report_kind(&key) == Some(stem) {
            return;
        }
        self.report_prefs.set_selected_report_kind(key, stem);
        if let Err(err) = self.save_report_prefs() {
            self.error = Some(format!("Could not save selected report: {err}"));
        }
    }

    /// Persist the currently visible reporting window for this user. Missing or
    /// unreadable saved dates leave the current view untouched.
    fn persist_report_date_range(&mut self) {
        let key = self.period_close_history_user_key();
        let range = ReportDateRange {
            from: self.report_from.format("%Y-%m-%d").to_string(),
            to: self.report_to.format("%Y-%m-%d").to_string(),
        };
        if self.report_prefs.report_date_range(&key) == Some(&range) {
            return;
        }
        self.report_prefs.set_report_date_range(key, range);
        if let Err(err) = self.save_report_prefs() {
            self.error = Some(format!("Could not save report date range: {err}"));
        }
    }

    /// The session has already changed: persist the outgoing user's options
    /// before loading the incoming user's view, then write both snapshots.
    fn switch_report_user_view(&mut self, previous_user_key: &str) {
        self.report_prefs.snapshot_user_options(previous_user_key);
        self.remember_collections_bucket(previous_user_key);
        self.load_report_user_view();
        if let Err(err) = self.save_report_prefs() {
            self.error = Some(format!("Could not save report preferences: {err}"));
        }
    }

    // ---------------------------------------------------------------- Security

    fn login_required(&self) -> bool {
        self.features.enabled(FeatureKey::RequireLogin)
    }

    fn enforce_permissions(&self) -> bool {
        self.features.enabled(FeatureKey::EnforcePermissions)
    }

    /// Combine the Open Dental permission with the user-facing feature toggle.
    fn can(&self, permission: Permission) -> bool {
        match &self.session {
            Some(session) => od_core::auth::can(permission, session, self.enforce_permissions()),
            None => false,
        }
    }

    fn can_module(&self, module: Module) -> bool {
        self.features.module_enabled(module) && self.can(Permission::for_module(module))
    }

    /// Record a successful, auditable action in the OD `securitylog` table.
    fn audit(&mut self, permission: Permission, pat_num: Id, log_text: String) {
        let event = SecurityEvent {
            security_log_num: 0,
            perm_type: permission,
            user_num: self.session.as_ref().map(|s| s.user_num).unwrap_or(0),
            log_datetime: Local::now().naive_local(),
            log_text,
            pat_num,
        };
        if let Err(err) = self.store.append_security_log(&event) {
            self.error = Some(format!("Could not write audit log: {err}"));
        } else if self.features.enabled(FeatureKey::ShowAuditLog) {
            self.security_log = self.store.list_security_log(50).unwrap_or_default();
        }
    }

    fn audit_if(
        &mut self,
        key: FeatureKey,
        permission: Permission,
        pat_num: Id,
        log_text: String,
    ) {
        if self.features.enabled(key) {
            self.audit(permission, pat_num, log_text);
        }
    }

    fn reconcile_dynamic_plans_due_portfolio(
        &mut self,
        plans: &[PayPlan],
        plan_links: &[PayPlanLink],
        procedures: &[Procedure],
        adjustments: &[Adjustment],
        splits: &[PaySplit],
        plan_charges: &[PayPlanCharge],
        as_of: chrono::NaiveDate,
    ) {
        let mut total_issued = 0usize;
        let mut plans_touched = 0usize;
        let mut errors: Vec<String> = Vec::new();
        for p in plans {
            if !p.is_dynamic {
                continue;
            }
            let p_links: Vec<PayPlanLink> = plan_links.iter().filter(|l| l.pay_plan_num == p.pay_plan_num).cloned().collect();
            if let Some(reason) = od_core::issuance_block(p, &p_links) {
                errors.push(format!("Plan #{}: {}", p.pay_plan_num, reason));
                continue;
            }
            let existing: Vec<PayPlanCharge> = plan_charges.iter().filter(|c| c.pay_plan_num == p.pay_plan_num).cloned().collect();
            let due = od_core::issue_charges_due(p, &p_links, procedures, adjustments, splits, &existing, as_of);
            if due.is_empty() {
                continue;
            }
            let mut plan_error = None;
            let mut issued = 0usize;
            for charge in due {
                match self.store.upsert_pay_plan_charge(&charge) {
                    Ok(_) => issued += 1,
                    Err(e) => { plan_error = Some(e); break; }
                }
            }
            if let Some(e) = plan_error {
                errors.push(format!("Plan #{}: {}", p.pay_plan_num, e));
            }
            if issued > 0 {
                plans_touched += 1;
                total_issued += issued;
                self.audit_if(
                    FeatureKey::AuditBillingChanges,
                    Permission::BILLING,
                    p.pat_num,
                    format!("Issued {} dynamic charge(s) for plan {} (portfolio reconcile as of {})", issued, p.pay_plan_num, as_of),
                );
            }
        }
        if errors.is_empty() {
            self.error = None;
            self.notice = Some(if total_issued == 0 {
                "No dynamic charges due to issue".into()
            } else {
                format!("Issued {total_issued} charge(s) across {plans_touched} plan(s)")
            });
        } else {
            self.notice = if total_issued > 0 { Some(format!("Issued {total_issued} charge(s) across {plans_touched} plan(s); some plans had errors")) } else { None };
            self.error = Some(errors.join("; "));
        }
    }

    fn resolve_stale_dynamic_plan_charges(
        &mut self,
        plan: &PayPlan,
        own_links: &[PayPlanLink],
        procedures: &[Procedure],
        adjustments: &[Adjustment],
        splits: &[PaySplit],
        plan_charges: &[PayPlanCharge],
        reason: &str,
        as_of: chrono::NaiveDate,
    ) {
        let reason = reason.trim();
        if reason.is_empty() {
            self.notice = None;
            self.error = Some("Enter a resolution reason before resolving stale charges.".into());
            return;
        }
        let (_own, _due, _full, stale) = od_core::reconciliation_detail(plan, own_links, procedures, adjustments, splits, plan_charges, as_of);
        if stale.is_empty() {
            self.notice = Some("No stale dynamic charges to resolve".into());
            return;
        }
        let res = od_core::resolve_stale_issued_charges(&stale, as_of, reason);
        let mut resolved = 0usize;
        let mut errors = Vec::new();
        for c in &res.resolved {
            match self.store.upsert_pay_plan_charge(c) {
                Ok(_) => resolved += 1,
                Err(e) => errors.push(format!("#{}: {e}", c.pay_plan_charge_num)),
            }
        }
        if resolved > 0 {
            self.audit_if(
                FeatureKey::AuditBillingChanges,
                Permission::BILLING,
                plan.pat_num,
                format!("Resolved {resolved} stale dynamic charge(s) for plan {} (as of {}, reason: {}, blocked {})", plan.pay_plan_num, as_of, reason, res.blocked_with_splits.len()),
            );
        }
        if errors.is_empty() {
            self.error = None;
            let blocked = res.blocked_with_splits.len();
            self.notice = Some(if blocked > 0 {
                format!("Resolved {resolved} stale charge(s); {blocked} blocked because splits are applied")
            } else {
                format!("Resolved {resolved} stale charge(s)")
            });
        } else {
            self.notice = if resolved > 0 { Some(format!("Resolved {resolved} stale charge(s); some updates failed")) } else { None };
            self.error = Some(errors.join("; "));
        }
    }

    fn try_login(&mut self) {
        let name = self.login_name.trim();
        let Some(user) = self
            .users
            .iter()
            .find(|u| u.user_name.eq_ignore_ascii_case(name))
            .cloned()
        else {
            self.login_error = Some(format!("Unknown user: {name}"));
            return;
        };
        if !od_core::auth::verify_login(&user, &self.login_pass) {
            self.login_error = Some("Incorrect password".into());
            return;
        }
        let permissions =
            od_core::auth::resolve_permissions(&user, &self.group_attaches, &self.group_permissions);
        let user_name = user.user_name.clone();
        let user_num = user.user_num;
        let previous_user_key = self.period_close_history_user_key();
        self.persist_report_date_range();
        self.session = Some(Session {
            user_num,
            user_name: user_name.clone(),
            permissions,
        });
        self.load_period_close_history_filters();
        self.switch_report_user_view(&previous_user_key);
        self.login_error = None;
        self.login_pass.clear();
        self.show_settings = false;
        self.audit_if(
            FeatureKey::AuditLogins,
            Permission::USER_LOG_ON_OFF,
            0,
            format!("User: {user_name} has logged on."),
        );
    }

    fn logout(&mut self) {
        if let Some(session) = &self.session {
            let text = format!("User: {} has logged off.", session.user_name);
            self.audit_if(FeatureKey::AuditLogins, Permission::USER_LOG_ON_OFF, 0, text);
        }
        let previous_user_key = self.period_close_history_user_key();
        self.persist_report_date_range();
        self.session = None;
        self.load_period_close_history_filters();
        self.switch_report_user_view(&previous_user_key);
    }

    fn login_ui(&mut self, ui: &mut Ui) {
        ui.heading("odclinic — Log in");
        ui.label("Authenticate against the Open Dental userod table, or use the built-in demo users.");
        if let Some(err) = self.login_error.clone() {
            ui.colored_label(Color32::from_rgb(200, 60, 60), err);
        }
        ui.add_space(8.0);
        egui::Grid::new("login_grid")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label("User name");
                ui.add(
                    egui::TextEdit::singleline(&mut self.login_name)
                        .desired_width(220.0)
                        .hint_text("userod.UserName"),
                );
                ui.end_row();
                ui.label("Password");
                ui.add(
                    egui::TextEdit::singleline(&mut self.login_pass)
                        .password(true)
                        .desired_width(220.0),
                );
                ui.end_row();
            });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Log in").clicked() {
                self.try_login();
            }
            if ui.button("Demo credentials").clicked() {
                self.login_name = "admin".into();
                self.login_pass = "admin".into();
                self.login_error = None;
            }
            if ui.button("Settings").clicked() {
                self.show_settings = true;
            }
        });
        ui.add_space(8.0);
        ui.weak("Settings stays reachable here, so the login requirement can be turned off.");
        ui.weak("Demo users: admin / admin (full access), aharrison / harrison (clinical).");
    }

    fn render(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // Keep the session in step with the live RequireLogin switch so the
        // login screen's Settings escape hatch can unlock or lock the app
        // without a restart. A local session has UserNum 0.
        if self.login_required() {
            if self.session.as_ref().is_some_and(|s| s.user_num == 0) {
                self.session = None;
            }
        } else if self.session.is_none() {
            self.session = Some(Session::local());
        }
        if self.login_required() && self.session.is_none() {
            egui::CentralPanel::default().show(ui, |ui| {
                if self.show_settings {
                    self.settings_ui(ui);
                } else {
                    self.login_ui(ui);
                }
            });
            return;
        }

        self.top_bar(ui);
        self.side_bar(ui);

        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(err) = self.error.clone() {
                ui.colored_label(Color32::from_rgb(200, 60, 60), format!("Error: {err}"));
                ui.separator();
            }
            if let Some(msg) = self.notice.clone() {
                ui.colored_label(Color32::from_rgb(20, 120, 40), msg);
                ui.separator();
            }
            if self.show_settings {
                self.settings_ui(ui);
            } else if Module::ALL.iter().all(|m| !self.can_module(*m)) {
                ui.label("No modules are available. Check Settings or your permissions.");
            } else {
                if !self.can_module(self.module) {
                    self.module = Module::ALL
                        .into_iter()
                        .find(|m| self.can_module(*m))
                        .unwrap_or(Module::Appointments);
                }
                match self.module {
                    Module::Appointments => self.appointments_ui(ui),
                    Module::Family => self.family_ui(ui),
                    Module::Account => self.account_ui(ui),
                    Module::TreatmentPlan => self.treatment_ui(ui),
                    Module::Chart => self.chart_ui(ui),
                    Module::Reports => self.reports_ui(ui),
                    Module::Setup => self.setup_ui(ui),
                }
            }
        });

        self.patient_window(&ctx);
        self.patient_history_window(&ctx);
        self.registry_window(&ctx);
        self.appointment_window(&ctx);
        self.appointment_history_window(&ctx);
        self.recall_window(&ctx);
        self.adjustment_window(&ctx);
        self.journal_post_window(&ctx);
        self.post_all_window(&ctx);
        self.period_action_window(&ctx);
        self.payment_window(&ctx);
        self.plan_window(&ctx);
        self.plan_charge_window(&ctx);
        self.discount_plan_window(&ctx);
        self.discount_sub_window(&ctx);
        self.procedure_window(&ctx);
        self.treat_plan_window(&ctx);
        self.treat_plan_line_window(&ctx);
        self.send_message_window(&ctx);
        self.rule_window(&ctx);
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        egui::Panel::top("menu").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("odclinic");
                ui.separator();
                for module in Module::ALL {
                    if !self.can_module(module) {
                        continue;
                    }
                    if ui.button(module.label()).clicked() {
                        self.module = module;
                        self.show_settings = false;
                        self.refresh();
                    }
                }
                ui.separator();
                if ui.button("Settings").clicked() {
                    self.show_settings = true;
                }
                if ui.button("Refresh").clicked() {
                    self.refresh();
                    self.notice = Some("Refreshed".into());
                }
                let mut do_logout = false;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(session) = &self.session {
                        ui.label(format!("Signed in: {}", session.user_name));
                        if self.login_required() && ui.button("Log out").clicked() {
                            do_logout = true;
                        }
                    }
                    if self.features.enabled(FeatureKey::ShowDatabaseLabel) {
                        ui.label(&self.db_label);
                    }
                });
                if do_logout {
                    self.logout();
                }
            });
        });
    }

    fn side_bar(&mut self, ui: &mut Ui) {
        egui::Panel::left("module_bar")
            .resizable(false)
            .default_size(112.0)
            .show(ui, |ui| {
                ui.add_space(6.0);
                for module in Module::ALL {
                    if !self.can_module(module) {
                        continue;
                    }
                    let selected = !self.show_settings && self.module == module;
                    if ui.selectable_label(selected, module.label()).clicked() {
                        self.module = module;
                        self.show_settings = false;
                        self.refresh();
                    }
                }
                ui.add_space(12.0);
                ui.separator();
                if ui.selectable_label(self.show_settings, "Settings").clicked() {
                    self.show_settings = true;
                }
                ui.add_space(12.0);
                ui.separator();
                if self.features.enabled(FeatureKey::ShowImagingNote) {
                    ui.label(egui::RichText::new("Imaging omitted").weak().small());
                    ui.label(egui::RichText::new("by scope choice").weak().small());
                }
            });
    }

    // ---------------------------------------------------------------- Appointments

    fn appointments_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for tab in ApptTab::ALL {
                if !tab.enabled(&self.features) {
                    continue;
                }
                if ui.selectable_label(self.appt_tab == tab, tab.label()).clicked() {
                    self.appt_tab = tab;
                    self.refresh();
                }
            }
        });
        ui.separator();
        match self.appt_tab {
            ApptTab::Schedule => self.schedule_ui(ui),
            ApptTab::Recalls => self.recalls_ui(ui),
            ApptTab::Asap => self.asap_ui(ui),
            ApptTab::Waitlist => self.waitlist_ui(ui),
        }
    }

    fn schedule_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("< Previous day").clicked() {
                self.schedule_day -= Duration::days(1);
                self.refresh();
            }
            ui.strong(
                self.schedule_day
                    .format("%A, %B %d, %Y")
                    .to_string(),
            );
            if ui.button("Next day >").clicked() {
                self.schedule_day += Duration::days(1);
                self.refresh();
            }
            ui.separator();
            if self.features.enabled(FeatureKey::AppointmentCreate)
                && self.can(Permission::APPOINTMENT_CREATE)
                && ui.button("+ New appointment").clicked()
            {
                let mut apt = Appointment::default();
                apt.apt_datetime = self.schedule_day.and_hms_opt(9, 0, 0).unwrap_or_default();
                apt.apt_length = 30;
                apt.apt_status = AppointmentStatus::Scheduled;
                apt.operatory_num = self.operatories.first().map(|o| o.operatory_num).unwrap_or(0);
                apt.provider_num = self.providers.first().map(|p| p.provider_num).unwrap_or(0);
                apt.clinic_num = 1;
                apt.pattern = "XXXXXX".into();
                self.apt_edit = Some(apt);
                self.apt_text = self
                    .schedule_day
                    .and_hms_opt(9, 0, 0)
                    .unwrap_or_default()
                    .format("%Y-%m-%d %H:%M")
                    .to_string();
                self.apt_len_text = "30".into();
                self.apt_edit_open = true;
            }
            ui.separator();
            if self.features.enabled(FeatureKey::EServicesEnabled)
                && ui.button("Send message").clicked()
            {
                self.send_message_open = true;
            }
        });
        ui.separator();

        let patients = self.patients.clone();
        let operatories = self.operatories.clone();
        let appointments = self.appointments.clone();
        let day = self.schedule_day;
        let use_status_colors = self.features.enabled(FeatureKey::AppointmentStatusColors);
        let can_edit_appointments = self.features.enabled(FeatureKey::AppointmentEdit)
            && self.can(Permission::APPOINTMENT_EDIT);

        if operatories.is_empty() {
            ui.label("No operatories configured. Add rows to the operatory table.");
            return;
        }

        egui::Grid::new("appointment_schedule")
            .num_columns(operatories.len() + 1)
            .striped(true)
            .spacing([6.0, 2.0])
            .show(ui, |ui| {
                ui.strong("Time");
                for op in &operatories {
                    ui.strong(&op.description);
                }
                ui.end_row();

                for hour in 8..18 {
                    for half in [0u32, 30] {
                        ui.label(format!("{hour:02}:{half:02}"));
                        for op in &operatories {
                            let start = day.and_hms_opt(hour, half, 0).unwrap_or_default();
                            let end = start + Duration::minutes(30);
                            let found = appointments.iter().find(|a| {
                                a.operatory_num == op.operatory_num
                                    && a.apt_datetime >= start
                                    && a.apt_datetime < end
                            });
                            if let Some(apt) = found {
                                let label = format!(
                                    "{} · {} min",
                                    patient_name(&patients, apt.pat_num),
                                    apt.apt_length
                                );
                                let mut button = egui::Button::new(label).min_size([118.0, 18.0].into());
                                if use_status_colors {
                                    button = button.fill(status_color(apt.apt_status));
                                }
                                if ui.add(button).clicked() && can_edit_appointments {
                                    self.apt_edit = Some(apt.clone());
                                    self.apt_text = apt
                                        .apt_datetime
                                        .format("%Y-%m-%d %H:%M")
                                        .to_string();
                                    self.apt_len_text = apt.apt_length.to_string();
                                    self.apt_edit_open = true;
                                }
                            } else {
                                ui.label("—");
                            }
                        }
                        ui.end_row();
                    }
                }
            });
    }

    fn open_appointment_editor(&mut self, apt: Appointment) {
        self.apt_text = apt.apt_datetime.format("%Y-%m-%d %H:%M").to_string();
        self.apt_len_text = apt.apt_length.to_string();
        self.apt_edit = Some(apt);
        self.apt_edit_open = true;
    }

    fn open_recall_editor(&mut self, recall: Recall) {
        let (years, months, weeks, days) = interval_parts(recall.recall_interval);
        self.recall_due_text = recall_due_date(&recall).map(|d| d.to_string()).unwrap_or_default();
        self.recall_years_text = years.to_string();
        self.recall_months_text = months.to_string();
        self.recall_weeks_text = weeks.to_string();
        self.recall_days_text = days.to_string();
        self.recall_status_text = recall.recall_status.to_string();
        self.recall_edit = Some(recall);
        self.recall_edit_open = true;
    }

    fn save_recall(&mut self, recall: &Recall) {
        match self.store.upsert_recall(recall) {
            Ok(num) => {
                self.audit_if(
                    FeatureKey::AuditAppointmentChanges,
                    Permission::APPOINTMENT_EDIT,
                    recall.pat_num,
                    format!("Recall {num} saved (due {:?})", recall.date_due),
                );
                self.notice = Some("Recall saved".into());
                self.error = None;
                self.refresh();
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn recalls_ui(&mut self, ui: &mut Ui) {
        let today = Local::now().date_naive();
        let patients = self.patients.clone();
        let can_edit = self.can(Permission::APPOINTMENT_EDIT);
        let can_create = self.can(Permission::APPOINTMENT_CREATE);

        ui.horizontal(|ui| {
            if can_create && ui.button("+ New recall").clicked() {
                let pat_num = self
                    .selected_patient
                    .or_else(|| patients.first().map(|p| p.pat_num))
                    .unwrap_or(0);
                let recall = Recall {
                    pat_num,
                    date_due: Some(today),
                    date_due_calc: Some(today),
                    recall_interval: interval_from_parts(0, 6, 0, 0),
                    ..Default::default()
                };
                self.open_recall_editor(recall);
            }
            if ui.button("Refresh").clicked() {
                self.refresh();
            }
            ui.separator();
            if self.features.enabled(FeatureKey::EServicesEnabled)
                && ui.button("Send message").clicked()
            {
                self.send_message_open = true;
            }
        });
        ui.weak("OD recalls: DateDue is editable; DateDueCalc is calculated by Open Dental. Due rows sort first.");
        ui.separator();

        let mut rows = self.recalls.clone();
        rows.sort_by_key(|r| (recall_due_state(r, today).sort_rank(), r.pat_num));
        if rows.is_empty() {
            ui.label("No recalls yet. Add one to start a reminder list.");
            return;
        }

        egui::Grid::new("recall_grid")
            .num_columns(8)
            .striped(true)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.strong("Patient");
                ui.strong("Due");
                ui.strong("State");
                ui.strong("Interval");
                ui.strong("Priority");
                ui.strong("Status #");
                ui.strong("Note");
                ui.strong("Actions");
                ui.end_row();

                for recall in &rows {
                    let state = recall_due_state(recall, today);
                    let color = match state {
                        RecallDueState::Overdue(_) => Color32::from_rgb(190, 60, 60),
                        RecallDueState::DueToday => Color32::from_rgb(190, 120, 20),
                        RecallDueState::Disabled | RecallDueState::NoDate => Color32::GRAY,
                        _ => Color32::from_rgb(30, 110, 50),
                    };
                    ui.label(patient_name(&patients, recall.pat_num));
                    ui.label(
                        recall_due_date(recall)
                            .map(|d| d.to_string())
                            .unwrap_or_else(|| "-".into()),
                    );
                    ui.colored_label(color, state.label());
                    ui.label(format_interval(recall.recall_interval));
                    ui.label(recall.priority.label());
                    ui.label(if recall.recall_status == 0 {
                        "-".into()
                    } else {
                        recall.recall_status.to_string()
                    });
                    ui.label(&recall.note);
                    ui.horizontal(|ui| {
                        if can_edit && ui.button("Edit").clicked() {
                            self.open_recall_editor(recall.clone());
                        }
                        if can_create && ui.button("Schedule").clicked() {
                            let mut apt = Appointment {
                                pat_num: recall.pat_num,
                                apt_datetime: today.and_hms_opt(9, 0, 0).unwrap_or_default(),
                                apt_length: 30,
                                apt_status: AppointmentStatus::Scheduled,
                                operatory_num: self.operatories.first().map(|o| o.operatory_num).unwrap_or(0),
                                provider_num: self.providers.first().map(|p| p.provider_num).unwrap_or(0),
                                clinic_num: 1,
                                pattern: "XXXXXX".into(),
                                priority: recall.priority,
                                ..Default::default()
                            };
                            if let Some(previous) = recall.date_previous {
                                apt.apt_datetime = previous.and_hms_opt(9, 0, 0).unwrap_or(apt.apt_datetime);
                            }
                            self.open_appointment_editor(apt);
                        }
                    });
                    ui.end_row();
                }
            });
    }

    fn asap_ui(&mut self, ui: &mut Ui) {
        let patients = self.patients.clone();
        let providers = self.providers.clone();
        let operatories = self.operatories.clone();
        let can_edit = self.features.enabled(FeatureKey::AppointmentEdit)
            && self.can(Permission::APPOINTMENT_EDIT);
        let can_create = self.features.enabled(FeatureKey::AppointmentCreate)
            && self.can(Permission::APPOINTMENT_CREATE);

        ui.horizontal(|ui| {
            if can_create && ui.button("+ ASAP appointment").clicked() {
                let apt = Appointment {
                    apt_datetime: Local::now().date_naive().and_hms_opt(9, 0, 0).unwrap_or_default(),
                    apt_length: 30,
                    apt_status: AppointmentStatus::UnschedList,
                    operatory_num: self.operatories.first().map(|o| o.operatory_num).unwrap_or(0),
                    provider_num: self.providers.first().map(|p| p.provider_num).unwrap_or(0),
                    clinic_num: 1,
                    pattern: "XXXXXX".into(),
                    priority: RecallPriority::Asap,
                    ..Default::default()
                };
                self.open_appointment_editor(apt);
            }
            if ui.button("Refresh").clicked() {
                self.refresh();
            }
        });
        ui.weak("OD derives this list from appointment.Priority = 1. Clear the ASAP checkbox in the editor to remove a row.");
        ui.separator();

        let asap = self.asap_appointments.clone();
        if asap.is_empty() {
            ui.label("No ASAP appointments.");
            return;
        }

        egui::Grid::new("asap_grid")
            .num_columns(7)
            .striped(true)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.strong("Date / time");
                ui.strong("Patient");
                ui.strong("Status");
                ui.strong("Provider");
                ui.strong("Operatory");
                ui.strong("Notes");
                ui.strong("Actions");
                ui.end_row();

                for apt in &asap {
                    let provider = providers
                        .iter()
                        .find(|p| p.provider_num == apt.provider_num)
                        .map(|p| p.abbreviation.clone())
                        .unwrap_or_else(|| "-".into());
                    let operatory = operatories
                        .iter()
                        .find(|o| o.operatory_num == apt.operatory_num)
                        .map(|o| o.abbrev.clone())
                        .unwrap_or_else(|| "-".into());
                    ui.label(apt.apt_datetime.format("%Y-%m-%d %H:%M").to_string());
                    ui.label(patient_name(&patients, apt.pat_num));
                    ui.label(apt.apt_status.label());
                    ui.label(provider);
                    ui.label(operatory);
                    ui.label(&apt.notes);
                    ui.horizontal(|ui| {
                        if can_edit && ui.button("Edit").clicked() {
                            self.open_appointment_editor(apt.clone());
                        }
                    });
                    ui.end_row();
                }
            });
    }

    fn waitlist_ui(&mut self, ui: &mut Ui) {
        let patients = self.patients.clone();
        let definitions = self.definitions.clone();
        let rows = self.waitlist.clone();
        let can_edit = self.features.enabled(FeatureKey::AppointmentEdit)
            && self.can(Permission::APPOINTMENT_EDIT);
        let can_create = self.features.enabled(FeatureKey::AppointmentCreate)
            && self.can(Permission::APPOINTMENT_CREATE);
        let has_history = self.features.enabled(FeatureKey::AppointmentHistory);

        ui.horizontal(|ui| {
            if can_create && ui.button("+ Add to waitlist").clicked() {
                let status = waitlist_status_definitions(&definitions)
                    .first()
                    .map(|d| d.def_num)
                    .unwrap_or(0);
                let apt = Appointment {
                    apt_datetime: Local::now().date_naive().and_hms_opt(9, 0, 0).unwrap_or_default(),
                    apt_length: 30,
                    apt_status: AppointmentStatus::UnschedList,
                    waitlist_status: status,
                    operatory_num: self.operatories.first().map(|o| o.operatory_num).unwrap_or(0),
                    provider_num: self.providers.first().map(|p| p.provider_num).unwrap_or(0),
                    clinic_num: 1,
                    pattern: pattern_for_length(30),
                    ..Default::default()
                };
                self.open_appointment_editor(apt);
            }
            if ui.button("Refresh").clicked() {
                self.refresh();
            }
        });
        ui.weak(
            "OD's unscheduled list (AptStatus = 3). Removing a row audits with AppointmentEdit (27) and deletes the waitlist entry; no patient-facing cancellation or reconfirmation is sent. OD also offers to copy notes into an internal Commlog — this port omits that optional prompt. Use History to restore a mistaken removal.",
        );
        ui.separator();

        if rows.is_empty() {
            ui.label("No patients on the waitlist.");
            return;
        }

        egui::Grid::new("waitlist_grid")
            .num_columns(6)
            .striped(true)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.strong("Date / time");
                ui.strong("Patient");
                ui.strong("Waitlist status");
                ui.strong("Priority");
                ui.strong("Notes");
                ui.strong("Actions");
                ui.end_row();

                for apt in &rows {
                    ui.label(apt.apt_datetime.format("%Y-%m-%d %H:%M").to_string());
                    ui.label(patient_name(&patients, apt.pat_num));
                    ui.label(waitlist_status_name(&definitions, apt.waitlist_status));
                    ui.label(apt.priority.label());
                    ui.label(&apt.notes);
                    ui.horizontal(|ui| {
                        if can_edit && ui.button("Edit").clicked() {
                            self.open_appointment_editor(apt.clone());
                        }
                        if can_edit && ui.button("Schedule").clicked() {
                            let scheduled = schedule_from_waitlist(apt, apt.apt_datetime, apt.apt_length);
                            self.open_appointment_editor(scheduled);
                        }
                        if can_edit && ui.button("Remove").clicked() {
                            self.remove_from_waitlist(apt.clone());
                        }
                        if has_history && ui.button("History").clicked() {
                            self.open_appointment_history(apt.apt_num);
                        }
                    });
                    ui.end_row();
                }
            });
    }

    /// Mirrors OD `FormUnsched.Delete_Click()` (verified in 24.3 source and in
    /// the 25.3 IL): authorize with `AppointmentEdit` (27), delete the
    /// unscheduled appointment, and audit with the same permission. No
    /// patient-facing cancellation or reconfirmation is produced. OD also
    /// conditionally copies notes to an internal Commlog when the user opts
    /// in; this port deliberately omits that optional prompt.
    fn remove_from_waitlist(&mut self, apt: Appointment) {
        if !is_waitlist(&apt) {
            self.error = Some("Only unscheduled-list appointments can be removed here.".into());
            return;
        }
        if self.features.enabled(FeatureKey::AppointmentHistory) {
            let before = self
                .store
                .get_appointment(apt.apt_num)
                .ok()
                .flatten()
                .unwrap_or_else(|| apt.clone());
            let mut change = AppointmentChange::from_snapshot(&before, AppointmentChangeAction::Deleted);
            change.user_num = self.session.as_ref().map(|s| s.user_num).unwrap_or(0);
            if let Err(e) = self.store.append_appointment_change(&change) {
                self.error = Some(e);
                return;
            }
        }
        match self.store.delete_appointment(apt.apt_num) {
            Ok(()) => {
                self.audit_if(
                    FeatureKey::AuditAppointmentChanges,
                    Permission::APPOINTMENT_EDIT,
                    apt.pat_num,
                    format!(
                        "Removed {} from waitlist; unscheduled appointment {} deleted (no patient cancellation or reconfirmation)",
                        patient_name(&self.patients, apt.pat_num),
                        apt.apt_num
                    ),
                );
                self.notice = Some("Removed from waitlist. No patient cancellation or reconfirmation sent.".into());
                self.error = None;
                self.refresh();
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn open_appointment_history(&mut self, apt_num: Id) {
        self.apt_history_num = Some(apt_num);
        self.apt_history = self.store.list_appointment_changes(apt_num).unwrap_or_default();
        self.apt_history_open = true;
    }

    // ---------------------------------------------------------------- Family

    fn family_ui(&mut self, ui: &mut Ui) {
        let show_patient_search = self.features.enabled(FeatureKey::PatientSearch);
        let can_edit_patients = self.features.enabled(FeatureKey::PatientEdit)
            && self.can(Permission::PATIENT_EDIT);
        let can_create_patients = self.features.enabled(FeatureKey::PatientEdit)
            && self.can(Permission::PATIENT_CREATE);
        let can_delete_patients = self.features.enabled(FeatureKey::PatientDelete)
            && self.can(Permission::PATIENT_EDIT);
        ui.horizontal(|ui| {
            if show_patient_search {
                ui.label("Search:");
                ui.add(egui::TextEdit::singleline(&mut self.search).desired_width(220.0));
            }
            if can_create_patients && ui.button("+ New patient").clicked() {
                let mut patient = Patient::default();
                patient.gender = Gender::Unknown;
                patient.clinic_num = 1;
                self.patient_edit = Some(patient);
                self.birth_text.clear();
                self.patient_edit_open = true;
            }
        });
        ui.separator();

        let query = if show_patient_search { self.search.as_str() } else { "" };
        let filtered = self.store.find_patients(query).unwrap_or_default();
        let provider_names = self.providers.clone();

        egui::Grid::new("patient_grid")
            .num_columns(6)
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Num");
                ui.strong("Last name");
                ui.strong("First name");
                ui.strong("Birth date");
                ui.strong("Phone");
                ui.strong("Provider");
                ui.end_row();

                for p in &filtered {
                    ui.label(p.pat_num.to_string());
                    let selected = self.selected_patient == Some(p.pat_num);
                    if ui.selectable_label(selected, &p.l_name).clicked() {
                        self.selected_patient = Some(p.pat_num);
                    }
                    ui.label(&p.f_name);
                    ui.label(
                        p.birth_date
                            .map(|d| d.format("%Y-%m-%d").to_string())
                            .unwrap_or_default(),
                    );
                    ui.label(&p.home_phone);
                    ui.label(
                        provider_names
                            .iter()
                            .find(|provider| provider.provider_num == p.primary_provider_num)
                            .map(|provider| provider.abbreviation.clone())
                            .unwrap_or_default(),
                    );
                    ui.end_row();
                }
            });

        if let Some(num) = self.selected_patient {
            let patient = self.store.get_patient(num).unwrap_or_default();
            if let Some(patient) = patient {
                ui.separator();
                ui.heading(patient.display_name());
                ui.horizontal(|ui| {
                    if can_edit_patients && ui.button("Edit patient").clicked() {
                        self.birth_text = patient
                            .birth_date
                            .map(|d| d.format("%Y-%m-%d").to_string())
                            .unwrap_or_default();
                        self.patient_reason_text.clear();
                        self.patient_edit = Some(patient.clone());
                        self.patient_edit_open = true;
                    }
                    if self.features.enabled(FeatureKey::PatientCorrectionHistory)
                        && ui.button("Correction history").clicked()
                    {
                        self.open_patient_history(patient.pat_num);
                    }
                    if self.features.enabled(FeatureKey::PatientRegistry)
                        && ui.button("Patient registry").clicked()
                    {
                        self.open_registry(patient.pat_num);
                    }
                    if can_delete_patients && ui.button("Delete patient").clicked() {
                        let pat_num = patient.pat_num;
                        match self.store.delete_patient(pat_num) {
                            Ok(()) => {
                                self.selected_patient = None;
                                self.notice = Some("Patient deleted".into());
                                self.audit_if(
                                    FeatureKey::AuditPatientChanges,
                                    Permission::PATIENT_EDIT,
                                    pat_num,
                                    format!("Patient {pat_num} deleted"),
                                );
                                self.refresh();
                            }
                            Err(e) => self.error = Some(e),
                        }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("Chart #{}", patient.pat_num));
                    ui.separator();
                    ui.label(format!("DOB {}", patient.birth_date.map(|d| d.to_string()).unwrap_or_default()));
                    ui.separator();
                    ui.label(format!("Phone {}", patient.home_phone));
                    ui.separator();
                    ui.label(format!("Email {}", patient.email));
                });
                ui.label(&patient.address);
                ui.label(format!("{}, {} {}", patient.city, patient.state, patient.zip));
            }
        }
    }

    fn open_patient_history(&mut self, pat_num: Id) {
        self.patient_history_pat_num = Some(pat_num);
        self.patient_corrections = self.store.list_patient_corrections(pat_num).unwrap_or_default();
        self.patient_history_open = true;
    }

    // -------------------------------------------------------- Patient registry

    fn open_registry(&mut self, pat_num: Id) {
        self.registry_patient = Some(pat_num);
        self.registry_tab = RegistryTab::Overview;
        self.registry_card_auth.clear();
        self.registry_card_note.clear();
        self.registry_card_ack = false;
        self.registry_invoice_tax_text.clear();
        self.registry_invoice_note.clear();
        self.registry_invoice_pdf_path = default_invoice_pdf_path(pat_num);
        self.reload_registry(pat_num);
        self.registry_open = true;
    }

    fn reload_registry(&mut self, pat_num: Id) {
        self.registry_conditions = self.store.list_diseases(pat_num).unwrap_or_default();
        self.registry_treatments = self
            .store
            .list_registry_treatments(pat_num)
            .unwrap_or_default();
        self.registry_note = self.store.get_patient_note(pat_num).unwrap_or_default();
        self.registry_cards = self.store.list_clinic_cards(pat_num).unwrap_or_default();
    }

    fn registry_window(&mut self, ctx: &egui::Context) {
        if !self.registry_open || !self.features.enabled(FeatureKey::PatientRegistry) {
            return;
        }
        let Some(pat_num) = self.registry_patient else {
            return;
        };
        let patient = self.store.get_patient(pat_num).ok().flatten();
        let patient_name = patient
            .as_ref()
            .map(|p| p.display_name())
            .unwrap_or_else(|| format!("Patient #{pat_num}"));
        let procedures = self.store.list_procedures(pat_num).unwrap_or_default();
        let splits = self.store.list_pay_splits(pat_num).unwrap_or_default();
        let adjustments = self.store.list_adjustments(pat_num).unwrap_or_default();
        let insurance_enabled = self.features.enabled(FeatureKey::InsuranceEnabled);
        let claim_procs = if insurance_enabled {
            self.store.list_claim_procs(pat_num).unwrap_or_default()
        } else {
            Vec::new()
        };
        let summary = if insurance_enabled {
            account_summary(&procedures, &adjustments, &splits, &claim_procs)
        } else {
            account_summary_without_insurance(&procedures, &adjustments, &splits)
        };
        let today = Local::now().date_naive();
        let user_num = self.session.as_ref().map(|s| s.user_num).unwrap_or(0);
        let can_issue_card = self.features.enabled(FeatureKey::ClinicCardsEnabled)
            && self.can(Permission::PATIENT_EDIT);
        let can_invoice = self.features.enabled(FeatureKey::A5Invoice)
            && self.can(Permission::BILLING);

        // Work on local copies so widget drawing never holds a borrow of self.
        let mut tab = self.registry_tab;
        let mut reason = self.registry_card_reason;
        let mut auth = self.registry_card_auth.clone();
        let mut card_note = self.registry_card_note.clone();
        let mut ack = self.registry_card_ack;
        let mut tax_text = self.registry_invoice_tax_text.clone();
        let mut invoice_note = self.registry_invoice_note.clone();
        let mut pdf_path = self.registry_invoice_pdf_path.clone();
        let mut issue_card = false;
        let mut deactivate_card = None;
        let mut record_invoice = false;
        let mut write_pdf = false;
        let mut error: Option<String> = None;
        let mut notice: Option<String> = None;

        egui::Window::new("Patient registry")
            .open(&mut self.registry_open)
            .resizable(true)
            .default_width(700.0)
            .show(ctx, |ui| {
                ui.heading(&patient_name);
                ui.horizontal_wrapped(|ui| {
                    for candidate in RegistryTab::ALL {
                        if ui
                            .selectable_label(tab == candidate, candidate.label())
                            .clicked()
                        {
                            tab = candidate;
                        }
                    }
                });
                ui.separator();

                match tab {
                    RegistryTab::Overview => {
                        ui.horizontal(|ui| {
                            ui.strong(format!("Balance due: {:.2}", summary.balance));
                            ui.separator();
                            ui.label(format!(
                                "Charges {:.2} · Payments {:.2} · Adjustments {:.2}",
                                summary.charges, summary.patient_payments, summary.adjustments
                            ));
                        });
                        ui.label("All amounts use the Account ledger: completed procedures + adjustments − patient payments. Insurance is excluded unless enabled in Settings.");
                        ui.separator();
                        if let Some(patient) = &patient {
                            egui::Grid::new("registry_overview")
                                .num_columns(2)
                                .striped(true)
                                .show(ui, |ui| {
                                    ui.strong("Chart #");
                                    ui.label(patient.pat_num.to_string());
                                    ui.end_row();
                                    ui.strong("Phone");
                                    ui.label(&patient.home_phone);
                                    ui.end_row();
                                    ui.strong("Mobile");
                                    ui.label(&patient.wireless_phone);
                                    ui.end_row();
                                    ui.strong("Email");
                                    ui.label(&patient.email);
                                    ui.end_row();
                                    ui.strong("Address");
                                    ui.label(format!(
                                        "{}, {}, {} {}",
                                        patient.address, patient.city, patient.state, patient.zip
                                    ));
                                    ui.end_row();
                                });
                        }
                        ui.separator();
                        ui.label(format!(
                            "{} condition(s), {} treatment(s), {} clinic card(s) on this record.",
                            self.registry_conditions.len(),
                            self.registry_treatments.len(),
                            self.registry_cards.len()
                        ));
                    }
                    RegistryTab::Clinical => {
                        ui.heading("Existing conditions");
                        egui::Grid::new("registry_conditions")
                            .num_columns(4)
                            .striped(true)
                            .show(ui, |ui| {
                                ui.strong("Condition");
                                ui.strong("Status");
                                ui.strong("Start");
                                ui.strong("Note");
                                ui.end_row();
                                for disease in &self.registry_conditions {
                                    ui.label(&disease.disease_name);
                                    ui.label(disease.status.label());
                                    ui.label(disease.date_start.to_string());
                                    ui.label(&disease.note);
                                    ui.end_row();
                                }
                            });
                        if self.registry_conditions.is_empty() {
                            ui.label("No conditions recorded (OD disease).");
                        }
                        ui.separator();
                        ui.heading("Existing treatments");
                        egui::Grid::new("registry_treatments")
                            .num_columns(5)
                            .striped(true)
                            .show(ui, |ui| {
                                ui.strong("Date");
                                ui.strong("Treatment");
                                ui.strong("Tooth/site");
                                ui.strong("Fee");
                                ui.strong("Status");
                                ui.end_row();
                                for treatment in &self.registry_treatments {
                                    ui.label(treatment.proc_date.to_string());
                                    ui.label(&treatment.description);
                                    ui.label(&treatment.tooth_site);
                                    ui.label(format!("{:.2}", treatment.fee));
                                    ui.label(treatment.status_label());
                                    ui.end_row();
                                }
                            });
                        if self.registry_treatments.is_empty() {
                            ui.label("No treatments recorded (OD procedurelog).");
                        }
                        ui.separator();
                        ui.heading("Clinical notes");
                        if let Some(note) = &self.registry_note {
                            ui.label(format!("Medical: {}", note.medical));
                            ui.label(format!("Treatment: {}", note.treatment));
                            if !note.service.trim().is_empty() {
                                ui.label(format!("Service: {}", note.service));
                            }
                        } else {
                            ui.label("No patient note (OD patientnote).");
                        }
                    }
                    RegistryTab::Card => {
                        if !self.features.enabled(FeatureKey::ClinicCardsEnabled) {
                            ui.label("Clinic cards are disabled in Settings.");
                        } else {
                        ui.heading("Issued cards");
                        egui::Grid::new("registry_cards")
                            .num_columns(6)
                            .striped(true)
                            .show(ui, |ui| {
                                ui.strong("Number");
                                ui.strong("Issued");
                                ui.strong("Reason");
                                ui.strong("Authorization");
                                ui.strong("State");
                                ui.strong("");
                                ui.end_row();
                                for card in &self.registry_cards {
                                    ui.label(&card.card_number);
                                    ui.label(card.issued_at.to_string());
                                    ui.label(card.reason.label());
                                    ui.label(&card.authorization);
                                    ui.label(if card.is_active { "Active" } else { "Inactive" });
                                    if can_issue_card
                                        && card.is_active
                                        && ui.button("Deactivate").clicked()
                                    {
                                        deactivate_card = Some(card.card_num);
                                    }
                                    ui.end_row();
                                }
                            });
                        if self.registry_cards.is_empty() {
                            ui.label("No clinic cards issued for this patient.");
                        }
                        ui.separator();
                        ui.heading("Issue a card");
                        ui.label("A card is only issued when billing requires one. Issuing it never creates a statement, adjustment or payment.");
                        ui.horizontal(|ui| {
                            ui.label("Reason");
                            egui::ComboBox::from_id_salt("registry_card_reason")
                                .selected_text(reason.label())
                                .show_ui(ui, |ui| {
                                    for candidate in ClinicCardReason::ALL {
                                        ui.selectable_value(
                                            &mut reason,
                                            candidate,
                                            candidate.label(),
                                        );
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Auth code");
                            ui.add(
                                egui::TextEdit::singleline(&mut auth)
                                    .hint_text("billing authorisation"),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Note   ");
                            ui.add(egui::TextEdit::singleline(&mut card_note));
                        });
                        ui.checkbox(
                            &mut ack,
                            "I confirm this patient needs a card for billing",
                        );
                        if can_issue_card && ui.button("Issue clinic card").clicked() {
                            issue_card = true;
                        }
                        }
                    }
                    RegistryTab::Invoice => {
                        if !self.features.enabled(FeatureKey::A5Invoice) {
                            ui.label("A5 invoices are disabled in Settings.");
                        } else {
                        let doc = invoice_for(
                            pat_num,
                            &patient_name,
                            &procedures,
                            &splits,
                            &tax_text,
                            &invoice_note,
                        );
                        ui.horizontal(|ui| {
                            ui.label("Tax %");
                            ui.add(
                                egui::TextEdit::singleline(&mut tax_text).desired_width(70.0),
                            );
                            ui.label("Note");
                            ui.add(
                                egui::TextEdit::singleline(&mut invoice_note)
                                    .desired_width(240.0),
                            );
                        });
                        ui.separator();
                        egui::Grid::new("registry_invoice_lines")
                            .num_columns(6)
                            .striped(true)
                            .show(ui, |ui| {
                                ui.strong("Item");
                                ui.strong("Tooth/site");
                                ui.strong("Unit");
                                ui.strong("Disc");
                                ui.strong("Tax");
                                ui.strong("Amount");
                                ui.end_row();
                                for line in &doc.lines {
                                    ui.label(&line.description);
                                    ui.label(&line.tooth_site);
                                    ui.label(format!("{:.2}", line.unit_price));
                                    ui.label(format!("{:.2}", line.discount));
                                    ui.label(format!("{:.2}", line.tax()));
                                    ui.label(format!("{:.2}", line.total()));
                                    ui.end_row();
                                }
                            });
                        ui.separator();
                        ui.label(format!(
                            "Subtotal {:.2} · Discount {:.2} · Tax {:.2} · Amount paid {:.2}",
                            doc.subtotal(),
                            doc.discount_total(),
                            doc.tax_total(),
                            doc.amount_paid
                        ));
                        ui.strong(format!("Balance due: {:.2}", doc.balance()));
                        if doc.lines.is_empty() {
                            ui.label("No completed procedures to invoice.");
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("PDF");
                            ui.add(
                                egui::TextEdit::singleline(&mut pdf_path).desired_width(360.0),
                            );
                        });
                        ui.horizontal(|ui| {
                            if can_invoice && ui.button("Record A5 invoice").clicked() {
                                record_invoice = true;
                            }
                            if ui.button("Write A5 PDF").clicked() {
                                write_pdf = true;
                            }
                        });
                        }
                    }
                }
            });

        self.registry_tab = tab;
        self.registry_card_reason = reason;
        self.registry_card_auth = auth.clone();
        self.registry_card_note = card_note.clone();
        self.registry_card_ack = ack;
        self.registry_invoice_tax_text = tax_text.clone();
        self.registry_invoice_note = invoice_note.clone();
        self.registry_invoice_pdf_path = pdf_path.clone();

        if issue_card {
            match validate_clinic_card_issue(&auth, ack) {
                Err(err) => error = Some(err),
                Ok(()) => {
                    let next = self
                        .registry_cards
                        .iter()
                        .map(|card| card.card_num)
                        .max()
                        .unwrap_or(0)
                        + 1;
                    let card = ClinicCard {
                        pat_num,
                        card_number: format!("CC-{next:07}"),
                        issued_at: today,
                        issued_by: user_num,
                        reason,
                        authorization: auth.trim().to_string(),
                        note: card_note.trim().to_string(),
                        is_active: true,
                        ..Default::default()
                    };
                    match self.store.issue_clinic_card(&card) {
                        Ok(num) => {
                            self.audit(
                                Permission::PATIENT_EDIT,
                                pat_num,
                                format!(
                                    "Clinic card {} issued ({}), reason {}, no financial rows written",
                                    num, card.card_number, reason.as_str()
                                ),
                            );
                            self.reload_registry(pat_num);
                            self.registry_card_auth.clear();
                            self.registry_card_note.clear();
                            self.registry_card_ack = false;
                            notice = Some("Clinic card issued for billing".into());
                        }
                        Err(err) => error = Some(err),
                    }
                }
            }
        }

        if let Some(card_num) = deactivate_card {
            match self.store.deactivate_clinic_card(card_num) {
                Ok(()) => {
                    self.audit(
                        Permission::PATIENT_EDIT,
                        pat_num,
                        format!("Clinic card {card_num} deactivated (kept for history)"),
                    );
                    self.reload_registry(pat_num);
                    notice = Some("Clinic card deactivated".into());
                }
                Err(err) => error = Some(err),
            }
        }

        let doc = invoice_for(
            pat_num,
            &patient_name,
            &procedures,
            &splits,
            &tax_text,
            &invoice_note,
        );
        if record_invoice {
            let statement = Statement {
                pat_num,
                note: format!(
                    "A5 invoice {} — total {:.2}, paid {:.2}, balance {:.2}",
                    today,
                    doc.total(),
                    doc.amount_paid,
                    doc.balance()
                ),
                is_invoice: true,
                ..Default::default()
            };
            match self.store.insert_statement(&statement) {
                Ok(num) => {
                    self.audit(
                        Permission::BILLING,
                        pat_num,
                        format!("A5 invoice {num} recorded, balance {:.2}", doc.balance()),
                    );
                    notice = Some(format!("A5 invoice #{num} recorded"));
                }
                Err(err) => error = Some(err),
            }
        }

        if write_pdf {
            if pdf_path.trim().is_empty() {
                error = Some("Enter a PDF path.".into());
            } else {
                match std::fs::write(&pdf_path, render_invoice_pdf(&doc)) {
                    Ok(()) => {
                        notice = Some(format!("A5 invoice written to {pdf_path}"));
                        self.registry_invoice_pdf_patient = pat_num;
                    }
                    Err(err) => error = Some(format!("Could not write PDF: {err}")),
                }
            }
        }

        if let Some(message) = notice {
            self.notice = Some(message);
        }
        if let Some(message) = error {
            self.error = Some(message);
        }
    }

    // ---------------------------------------------------------------- Account

    fn account_ui(&mut self, ui: &mut Ui) {
        self.patient_picker(ui);
        ui.separator();
        let Some(num) = self.selected_patient else {
            ui.label("Select a patient in the Family module or above.");
            return;
        };

        // Read every OD transaction class up front so the UI can show them
        // together without holding a store borrow across widget drawing.
        let procedures = self.store.list_procedures(num).unwrap_or_default();
        let payments = self.store.list_payments(num).unwrap_or_default();
        let adjustments = self.store.list_adjustments(num).unwrap_or_default();
        let splits = self.store.list_pay_splits(num).unwrap_or_default();
        // Insurance is country-specific. When it is off we neither query nor
        // display claims, and the balance uses patient payments only.
        let insurance_enabled = self.features.enabled(FeatureKey::InsuranceEnabled);
        let (claims, claim_procs) = if insurance_enabled {
            (
                self.store.list_claims(num).unwrap_or_default(),
                self.store.list_claim_procs(num).unwrap_or_default(),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        let statements = self.store.list_statements(num).unwrap_or_default();
        let plans_enabled = self.features.enabled(FeatureKey::PaymentPlansEnabled);
        let (plans, plan_charges, plan_links) = if plans_enabled {
            (
                self.store.list_pay_plans(num).unwrap_or_default(),
                self.store.list_pay_plan_charges(num).unwrap_or_default(),
                self.store.list_pay_plan_links().unwrap_or_default(),
            )
        } else {
            (Vec::new(), Vec::new(), Vec::new())
        };
        let summary = if insurance_enabled {
            account_summary(&procedures, &adjustments, &splits, &claim_procs)
        } else {
            account_summary_without_insurance(&procedures, &adjustments, &splits)
        };

        if self.features.enabled(FeatureKey::AccountBalanceSummary) {
            ui.horizontal(|ui| {
                ui.strong(format!("Balance due: {:.2}", summary.balance));
                ui.separator();
                ui.label(format!(
                    "Charges {:.2} · Payments {:.2}",
                    summary.charges, summary.patient_payments
                ));
            });
            if self.features.enabled(FeatureKey::AccountDetailedBalance) {
                if insurance_enabled {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "Adjustments {:.2} · Insurance paid {:.2} · Insurance est {:.2}",
                            summary.adjustments, summary.insurance_paid, summary.insurance_estimate
                        ));
                    });
                } else {
                    ui.label(format!("Adjustments {:.2}", summary.adjustments));
                }
                ui.label(format!(
                    "{} payment row(s), {} paysplit(s). Simple charges − payments: {:.2}.",
                    payments.len(),
                    splits.len(),
                    patient_balance(&procedures, &payments)
                ));
                if insurance_enabled {
                    ui.label(
                        "OD aging: completed procedures + adjustments − paysplits − received insurance; estimates shown separately.",
                    )
                    .on_hover_text("Based on Ledgers.GetAgingQueryString in the 24.3 source; verified type set in 25.3 with REA.");
                } else {
                    ui.label("Patient balance: completed procedures + adjustments − paysplits. Insurance is off in Settings.")
                        .on_hover_text("Turn on “Insurance & claims” in Settings to include OD insurance payments, write-offs and estimates.");
                }
            }
            ui.separator();
        }

        let mut rows = build_account_ledger_rows(
            &procedures,
            &adjustments,
            &splits,
            &payments,
            self.features.enabled(FeatureKey::AccountShowAdjustments),
        );
        rows.sort_by_key(|r| r.date);

        ui.horizontal(|ui| {
            ui.checkbox(&mut self.account_show_refunds_transfers_only, "Show refunds/transfers only");
            ui.label("Filter applies to ledger and helps locate plan credit resolutions/refunds.");
        });
        ui.horizontal(|ui| {
            ui.label("Transfer plan # (0=all)");
            ui.add(egui::DragValue::new(&mut self.account_transfer_plan_filter).speed(1));
            let target_label = match self.account_transfer_target_filter {
                PaymentTarget::Unallocated => "Any/Unallocated target".into(),
                PaymentTarget::Procedure(pn) => format!("Procedure #{}", pn),
                PaymentTarget::PayPlan(pn) => format!("Plan #{}", pn),
                PaymentTarget::PayPlanCharge { plan_num, charge_num } => format!("Plan #{} charge #{}", plan_num, charge_num),
            };
            egui::ComboBox::from_id_salt("acct_transfer_target_filter")
                .selected_text(target_label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.account_transfer_target_filter, PaymentTarget::Unallocated, "Any/Unallocated target");
                    for p in &procedures {
                        ui.selectable_value(&mut self.account_transfer_target_filter, PaymentTarget::Procedure(p.proc_num), format!("Procedure #{}", p.proc_num));
                    }
                });
            ui.label("From");
            ui.add(egui::TextEdit::singleline(&mut self.account_transfer_from_text).desired_width(80.0).hint_text("YYYY-MM-DD"));
            ui.label("To");
            ui.add(egui::TextEdit::singleline(&mut self.account_transfer_to_text).desired_width(80.0).hint_text("YYYY-MM-DD"));
            if ui.small_button("Clear transfer filters").clicked() {
                self.account_show_refunds_transfers_only = false;
                self.account_transfer_plan_filter = 0;
                self.account_transfer_target_filter = PaymentTarget::Unallocated;
                self.account_transfer_from_text.clear();
                self.account_transfer_to_text.clear();
            }
        });

        let from_date = chrono::NaiveDate::parse_from_str(self.account_transfer_from_text.trim(), "%Y-%m-%d").ok();
        let to_date = chrono::NaiveDate::parse_from_str(self.account_transfer_to_text.trim(), "%Y-%m-%d").ok();
        let filtered_rows = filter_account_ledger_rows(
            &rows,
            self.account_show_refunds_transfers_only,
            self.account_transfer_plan_filter,
            self.account_transfer_target_filter,
            from_date,
            to_date,
        );

        if self.account_ledger_export_patient != num {
            self.account_ledger_export_patient = num;
            let path = patient_export_path(
                &self.report_prefs,
                &self.period_close_history_user_key(),
                EXPORT_KEY_ACCOUNT_LEDGER,
                num,
                default_account_ledger_export_path(num),
            );
            self.account_ledger_export_path = path.clone();
            self.remember_export_path(&patient_export_key(EXPORT_KEY_ACCOUNT_LEDGER, num), &path);
        }
        let mut ledger_path_changed: Option<String> = None;
        let mut ledger_saved: Option<String> = None;
        ui.horizontal(|ui| {
            ui.label("Export filtered ledger");
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.account_ledger_export_path)
                    .desired_width(320.0),
            );
            if response.changed() {
                ledger_path_changed = Some(self.account_ledger_export_path.clone());
            }
            let table = account_ledger_export_table(&filtered_rows);
            let csv = render_report_csv("Account ledger (filtered)", "", &table);
            let html = render_report_html("Account ledger (filtered)", "", &table);
            if ui.button("Save CSV").clicked() {
                let path = report_export_path(&self.account_ledger_export_path, "csv");
                match std::fs::write(&path, &csv) {
                    Ok(()) => {
                        let path_text = path.to_string_lossy().into_owned();
                        self.account_ledger_export_path = path_text.clone();
                        self.notice = Some(format!("Ledger written to {path_text}"));
                        self.error = None;
                        ledger_saved = Some(path_text);
                    }
                    Err(e) => self.error = Some(format!("Could not write ledger CSV: {e}")),
                }
            }
            if ui.button("Save HTML").clicked() {
                let path = report_export_path(&self.account_ledger_export_path, "html");
                match std::fs::write(&path, &html) {
                    Ok(()) => {
                        let path_text = path.to_string_lossy().into_owned();
                        self.account_ledger_export_path = path_text.clone();
                        self.notice = Some(format!("Ledger written to {path_text}"));
                        self.error = None;
                        ledger_saved = Some(path_text);
                    }
                    Err(e) => self.error = Some(format!("Could not write ledger HTML: {e}")),
                }
            }
            if ui.button("Copy CSV").clicked() {
                ui.ctx().copy_text(csv.clone());
                self.notice = Some("Filtered ledger CSV copied to clipboard".into());
                self.error = None;
            }
            if ui.button("Open export folder").clicked() {
                let path = report_export_path(&self.account_ledger_export_path, "csv");
                let folder = path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                match std::process::Command::new("explorer").arg(&folder).spawn() {
                    Ok(_) => { self.notice = Some(format!("Opened {}", folder.display())); self.error = None; }
                    Err(e) => self.error = Some(format!("Could not open export folder: {e}")),
                }
            }
        });
        if let Some(path) = ledger_path_changed.or(ledger_saved) {
            self.remember_export_path(&patient_export_key(EXPORT_KEY_ACCOUNT_LEDGER, num), &path);
        }

        let mut quick_refund_plan: Option<(Id, f64)> = None;
        let mut ledger_post: Option<JournalPostSource> = None;
        let posting_enabled = self.features.enabled(FeatureKey::LedgerJournalPosting)
            && self.can(Permission::ADJUSTMENT_CREATE);
        let ledger_journal: Vec<JournalEntry> = if posting_enabled {
            self.store.list_journal_entries().unwrap_or_default()
        } else {
            Vec::new()
        };
        // Shared control for the payment-backed rows (ordinary payments and
        // refunds). A payment with several splits may appear more than once;
        // once posted every row shows the same transaction instead of a button.
        let payment_post_control = |ui: &mut egui::Ui, pay_num: Id| -> Option<JournalPostSource> {
            if !posting_enabled {
                ui.weak("—").on_hover_text(
                    "Posting to the ledger is off in Settings, or the Adjustment Create permission is missing.",
                );
                return None;
            }
            let Some(payment) = payments.iter().find(|p| p.pay_num == pay_num) else {
                ui.weak("—");
                return None;
            };
            if is_payment_posted(&ledger_journal, payment.pay_num, payment.pat_num) {
                let txn = journal_transaction_for_payment(
                    &ledger_journal,
                    payment.pay_num,
                    payment.pat_num,
                )
                .unwrap_or(0);
                ui.weak(format!("Posted #{txn}")).on_hover_text(format!(
                    "Payment #{} is already in journal transaction #{}. Posting again would duplicate it.",
                    payment.pay_num, txn
                ));
                None
            } else if ui.small_button("Post to ledger").clicked() {
                Some(JournalPostSource::Payment(payment.clone()))
            } else {
                None
            }
        };
        egui::Grid::new("account_ledger")
            .num_columns(6)
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Date");
                ui.strong("Type");
                ui.strong("Description");
                ui.strong("Charge");
                ui.strong("Credit");
                ui.strong("Action");
                ui.end_row();
                for row in &filtered_rows {
                    ui.label(row.date.to_string());
                    ui.label(&row.kind);
                    ui.label(&row.description);
                    ui.label(format!("{:.2}", row.charge));
                    ui.label(format!("{:.2}", row.credit));
                    if row.kind == "Adj" {
                        if posting_enabled {
                            if let Some(adj) =
                                adjustments.iter().find(|a| a.adj_num == row.adj_num)
                            {
                                if is_adjustment_posted(
                                    &ledger_journal,
                                    adj.adj_num,
                                    adj.pat_num,
                                ) {
                                    let txn = journal_transaction_for_adjustment(
                                        &ledger_journal,
                                        adj.adj_num,
                                        adj.pat_num,
                                    )
                                    .unwrap_or(0);
                                    ui.weak(format!("Posted #{txn}")).on_hover_text(format!(
                                        "Adjustment #{} is already in journal transaction #{}. Posting again would duplicate it.",
                                        adj.adj_num, txn
                                    ));
                                } else if ui.small_button("Post to ledger").clicked() {
                                    ledger_post = Some(JournalPostSource::Adjustment(adj.clone()));
                                }
                            } else {
                                ui.weak("—");
                            }
                        } else {
                            ui.weak("—").on_hover_text(
                                "Posting to the ledger is off in Settings, or the Adjustment Create permission is missing.",
                            );
                        }
                    } else if row.kind == "Payment" {
                        if let Some(source) = payment_post_control(ui, row.pay_num) {
                            ledger_post = Some(source);
                        }
                    } else if row.kind == "Refund/Transfer" {
                        let target = match row.transfer_target.unwrap_or(PaymentTarget::Unallocated) {
                            PaymentTarget::Procedure(pn) => format!("procedure #{}", pn),
                            PaymentTarget::PayPlan(pn) => format!("plan #{}", pn),
                            PaymentTarget::PayPlanCharge { plan_num, charge_num } => format!("plan #{} charge #{}", plan_num, charge_num),
                            PaymentTarget::Unallocated => "patient account (unallocated)".to_string(),
                        };
                        ui.horizontal(|ui| {
                            ui.weak("audited").on_hover_text(format!("Transfer target: {}", target));
                            if row.pay_num != 0 {
                                if ui.small_button("Receipt").clicked() {
                                    if let Some(payment) = payments.iter().find(|p| p.pay_num == row.pay_num) {
                                        let patient_name = self.store.get_patient(num).ok().flatten().map(|p| p.display_name()).unwrap_or_else(|| format!("Patient #{num}"));
                                        let recorded_by = self.session.as_ref().map(|s| s.user_name.clone()).unwrap_or_else(|| "unknown".into());
                                        if let Some(doc) = build_refund_transfer_receipt(num, &patient_name, payment, &splits, &recorded_by, chrono::Local::now().date_naive()) {
                                            let html = render_refund_transfer_receipt_html(&doc);
                                            let path = std::env::temp_dir().join(format!("odclinic-refund-transfer-receipt-{}-{}.html", num, row.pay_num));
                                            match std::fs::write(&path, html) {
                                                Ok(()) => match open_in_browser(&path.to_string_lossy()) {
                                                    Ok(()) => { self.notice = Some(format!("Refund/transfer receipt opened ({})", path.display())); self.error = None; }
                                                    Err(e) => self.error = Some(format!("Receipt saved but could not open browser: {e}")),
                                                },
                                                Err(e) => self.error = Some(format!("Could not write refund/transfer receipt: {e}")),
                                            }
                                        } else {
                                            self.error = Some("No refund/transfer lines found for this payment".into());
                                        }
                                    }
                                }
                                if ui.small_button("PDF").clicked() {
                                    if let Some(payment) = payments.iter().find(|p| p.pay_num == row.pay_num) {
                                        let patient_name = self.store.get_patient(num).ok().flatten().map(|p| p.display_name()).unwrap_or_else(|| format!("Patient #{num}"));
                                        let recorded_by = self.session.as_ref().map(|s| s.user_name.clone()).unwrap_or_else(|| "unknown".into());
                                        if let Some(doc) = build_refund_transfer_receipt(num, &patient_name, payment, &splits, &recorded_by, chrono::Local::now().date_naive()) {
                                            let path = default_refund_transfer_receipt_pdf_path(num, row.pay_num);
                                            match std::fs::write(&path, render_refund_transfer_receipt_pdf(&doc)) {
                                                Ok(()) => { self.notice = Some(format!("Refund/transfer receipt PDF saved to {}", path)); self.error = None; }
                                                Err(e) => self.error = Some(format!("Could not save refund/transfer receipt PDF: {e}")),
                                            }
                                        } else {
                                            self.error = Some("No refund/transfer lines found for this payment".into());
                                        }
                                    }
                                }
                                if let Some(source) = payment_post_control(ui, row.pay_num) {
                                    ledger_post = Some(source);
                                }
                            }
                        });
                    } else if row.plan_num != 0 && row.credit > 0.005 {
                        if ui.small_button("Refund/transfer").clicked() {
                            quick_refund_plan = Some((row.plan_num, row.credit));
                        }
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });

        if let Some(source) = ledger_post {
            let accounts = self.store.list_accounts().unwrap_or_default();
            let (debit, credit) = source.default_accounts(&accounts);
            self.journal_post_edit = Some(source.clone());
            self.journal_debit_account = debit;
            self.journal_credit_account = credit;
            self.journal_transaction_num = next_journal_transaction_num(&ledger_journal);
            // Only the readable note is edited; the window re-attaches the
            // stable source reference before writing.
            self.journal_memo_text = source.note().to_string();
            self.journal_post_open = true;
            self.error = None;
        }

        let billing_edits = self.features.enabled(FeatureKey::AccountBillingEdits);
        if let Some((plan_num, amt)) = quick_refund_plan {
            if billing_edits && self.can(Permission::PAYMENT_CREATE) {
                self.payment_edit = Some(Payment {
                    pat_num: num,
                    pay_date: chrono::Local::now().date_naive(),
                    note: format!("Refund/transfer from plan #{} to patient account", plan_num),
                    ..Default::default()
                });
                self.payment_date_text = chrono::Local::now().format("%Y-%m-%d").to_string();
                self.payment_amount_text = "0.00".to_string();
                self.payment_rows = vec![
                    PaymentRow { target: PaymentTarget::PayPlan(plan_num), amount_text: format!("{:.2}", -amt) },
                    PaymentRow { target: PaymentTarget::Unallocated, amount_text: format!("{:.2}", amt) },
                ];
                self.payment_edit_open = true;
            } else {
                self.error = Some("Payment permission/feature required for refund/transfer".into());
            }
        }

        let billing_edits = self.features.enabled(FeatureKey::AccountBillingEdits);
        ui.separator();
        ui.horizontal(|ui| {
            if billing_edits && self.can(Permission::ADJUSTMENT_CREATE) {
                if ui.button("Record adjustment").clicked() {
                    self.adj_edit = Some(Adjustment {
                        adj_date: chrono::Local::now().date_naive(),
                        pat_num: num,
                        ..Default::default()
                    });
                    self.adj_date_text = chrono::Local::now().format("%Y-%m-%d").to_string();
                    self.adj_amount_text.clear();
                    self.adj_note_text.clear();
                    self.adj_edit_open = true;
                }
            }
            if billing_edits && self.can(Permission::PAYMENT_CREATE) {
                if ui.button("Record payment").clicked() {
                    self.payment_edit = Some(Payment {
                        pat_num: num,
                        pay_date: chrono::Local::now().date_naive(),
                        ..Default::default()
                    });
                    self.payment_date_text = chrono::Local::now().format("%Y-%m-%d").to_string();
                    self.payment_amount_text.clear();
                    self.payment_rows = vec![PaymentRow {
                        target: procedures
                            .first()
                            .map(|p| PaymentTarget::Procedure(p.proc_num))
                            .unwrap_or(PaymentTarget::Unallocated),
                        amount_text: String::new(),
                    }];
                    self.payment_edit_open = true;
                }
            }
        });
        if !billing_edits {
            ui.label("Enable “Record payments and adjustments” in Settings to add ledger entries.");
        }

        if plans_enabled {
            ui.separator();
            ui.horizontal(|ui| {
                ui.heading("Payment plans");
                if billing_edits && self.can(Permission::BILLING) {
                    if ui.button("New plan").clicked() {
                        self.plan_edit = Some(PayPlan {
                            pat_num: num,
                            guarantor: num,
                            pay_plan_date: chrono::Local::now().date_naive(),
                            charge_frequency: PayPlanFrequency::Monthly,
                            pay_schedule: PaymentSchedule::Monthly,
                            ..Default::default()
                        });
                        self.plan_amount_text.clear();
                        self.plan_count_text = "6".into();
                        self.plan_start_text = chrono::Local::now()
                            .format("%Y-%m-%d")
                            .to_string();
                        self.plan_apr_text = "0".into();
                        self.plan_down_payment_text = "0".into();
                        self.plan_interest_start_text.clear();
                        self.plan_note_text.clear();
                        self.plan_frequency = PayPlanFrequency::Monthly;
                        self.plan_is_dynamic = false;
                        self.plan_tp_option = DynamicPayPlanTPOption::None;
                        self.plan_is_closed = false;
                        self.plan_is_locked = false;
                        self.plan_edit_open = true;
                    }
                }
            });
            if plans.is_empty() {
                ui.label("No payment plans for this patient.");
            }
            let today = chrono::Local::now().date_naive();
            ui.horizontal(|ui| {
                ui.label("As of");
                ui.add(
                    egui::TextEdit::singleline(&mut self.plan_recon_as_of_text)
                        .desired_width(90.0)
                        .hint_text("YYYY-MM-DD"),
                );
                if ui.small_button("Today").clicked() {
                    self.plan_recon_as_of_text = today.format("%Y-%m-%d").to_string();
                }
            });
            let as_of = chrono::NaiveDate::parse_from_str(&self.plan_recon_as_of_text, "%Y-%m-%d")
                .unwrap_or(today);
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut self.account_show_resolved_plan_charges,
                    "Show stale-resolved charges in plan history",
                );
                let resolved = plan_charges
                    .iter()
                    .filter(|c| od_core::is_stale_resolved_charge(c))
                    .count();
                if resolved > 0 {
                    ui.small(format!("{resolved} resolved row(s) on file"));
                }
            });

            // Patient-scoped audit bundle: the recorded reasons on this
            // account's charges and the matching security-log rows in one file.
            ui.separator();
            let audit_patient = patient_name(&self.patients, num);
            if self.account_audit_export_patient != num {
                self.account_audit_export_patient = num;
                let path = patient_export_path(
                    &self.report_prefs,
                    &self.period_close_history_user_key(),
                    EXPORT_KEY_ACCOUNT_AUDIT,
                    num,
                    default_account_audit_export_path(num),
                );
                self.account_audit_export_path = path.clone();
                self.remember_export_path(&patient_export_key(EXPORT_KEY_ACCOUNT_AUDIT, num), &path);
            }
            let audit_includes_log = self.features.enabled(FeatureKey::ShowAuditLog)
                && self.can(Permission::AUDIT_TRAIL);
            let audit_events: Vec<SecurityEvent> = if audit_includes_log {
                self.security_log
                    .iter()
                    .filter(|event| event.pat_num == num)
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };
            let audit_log_count = audit_events.len();
            let resolved_rows = plan_charges
                .iter()
                .filter(|charge| charge.pat_num == num && od_core::is_stale_resolved_charge(charge))
                .count();
            let mut audit_table = account_audit_export_table(
                num,
                &audit_patient,
                &plans,
                &plan_charges,
                &audit_events,
            );
            if !audit_includes_log {
                audit_table.push(&[
                    "Security log",
                    "",
                    &format!("#{num}"),
                    "",
                    "",
                    "",
                    "Security-log rows omitted: enable the audit log feature and AUDIT_TRAIL permission.",
                ]);
            }
            let audit_title = format!("Account audit — {audit_patient} (patient #{num})");
            let audit_practice = self.report_prefs.practice_name.clone();
            let audit_csv = render_report_csv(&audit_title, &audit_practice, &audit_table);
            let audit_html = render_report_html(&audit_title, &audit_practice, &audit_table);
            let mut audit_path_changed: Option<String> = None;
            ui.horizontal(|ui| {
                ui.strong("Export account audit");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.account_audit_export_path)
                        .desired_width(300.0),
                );
                if response.changed() {
                    audit_path_changed = Some(self.account_audit_export_path.clone());
                }
                if ui.button("Refresh log").clicked() {
                    self.security_log = self.store.list_security_log(500).unwrap_or_default();
                }
            });
            ui.label(format!(
                "One file with this account's {resolved_rows} stale-resolved charge row(s) and {audit_log_count} matching security-log row(s), ready to file against the patient record."
            ));
            ui.collapsing("Preview bundled rows", |ui| {
                if audit_table.rows.is_empty() {
                    ui.weak("No stale-resolved charges or patient-scoped log rows on file.");
                } else {
                    egui::Grid::new(format!("account_audit_preview_{num}"))
                        .num_columns(4)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Section");
                            ui.strong("When");
                            ui.strong("Source #");
                            ui.strong("Details");
                            ui.end_row();
                            for row in &audit_table.rows {
                                ui.label(&row[0]);
                                ui.label(&row[1]);
                                ui.label(&row[4]);
                                ui.label(&row[6]);
                                ui.end_row();
                            }
                        });
                }
            });
            let mut audit_saved: Option<String> = None;
            let mut audit_file_saved: Option<String> = None;
            ui.horizontal(|ui| {
                if ui.button("Save CSV").clicked() {
                    let path = report_export_path(&self.account_audit_export_path, "csv");
                    match std::fs::write(&path, &audit_csv) {
                        Ok(()) => {
                            let path_text = path.to_string_lossy().into_owned();
                            self.account_audit_export_path = path_text.clone();
                            self.notice = Some(format!("Account audit written to {path_text}"));
                            self.error = None;
                            audit_file_saved = Some(path_text.clone());
                            audit_saved = Some(path_text);
                        }
                        Err(err) => self.error = Some(format!("Could not write account audit CSV: {err}")),
                    }
                }
                if ui.button("Save HTML").clicked() {
                    let path = report_export_path(&self.account_audit_export_path, "html");
                    match std::fs::write(&path, &audit_html) {
                        Ok(()) => {
                            let path_text = path.to_string_lossy().into_owned();
                            self.account_audit_export_path = path_text.clone();
                            self.notice = Some(format!("Account audit written to {path_text}"));
                            self.error = None;
                            audit_file_saved = Some(path_text.clone());
                            audit_saved = Some(path_text);
                        }
                        Err(err) => self.error = Some(format!("Could not write account audit HTML: {err}")),
                    }
                }
                if ui.button("Copy CSV").clicked() {
                    ui.ctx().copy_text(audit_csv.clone());
                    self.notice = Some("Account audit CSV copied to clipboard".into());
                    self.error = None;
                    audit_saved = Some("clipboard".into());
                }
                if ui.button("Open export folder").clicked() {
                    let path = report_export_path(&self.account_audit_export_path, "csv");
                    let folder = path.parent().map(|p| p.to_path_buf())
                        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                    match std::process::Command::new("explorer").arg(&folder).spawn() {
                        Ok(_) => {
                            self.notice = Some(format!("Opened {}", folder.display()));
                            self.error = None;
                        }
                        Err(err) => self.error = Some(format!("Could not open export folder: {err}")),
                    }
                }
            });
            if let Some(path) = audit_path_changed.or(audit_file_saved) {
                self.remember_export_path(&patient_export_key(EXPORT_KEY_ACCOUNT_AUDIT, num), &path);
            }
            if let Some(destination) = audit_saved {
                self.audit(
                    Permission::AUDIT_TRAIL,
                    num,
                    format!(
                        "Exported account audit for patient {num} ({resolved_rows} resolved charge(s), {audit_log_count} log row(s)) to {destination}"
                    ),
                );
            }

            let show_resolved = self.account_show_resolved_plan_charges;
            let mut dyn_count = 0usize;
            let mut dyn_issued_count = 0usize;
            let mut dyn_expected_due_count = 0usize;
            let mut dyn_issued_total = 0.0_f64;
            let mut dyn_expected_due_total = 0.0_f64;
            let mut dyn_missing_due_count = 0usize;
            let mut dyn_missing_due_total = 0.0_f64;
            let mut dyn_stale_count = 0usize;
            let mut dyn_stale_total = 0.0_f64;
            for plan in &plans {
                if plan.is_dynamic {
                    dyn_count += 1;
                    let own_links: Vec<PayPlanLink> = plan_links.iter().filter(|l| l.pay_plan_num == plan.pay_plan_num).cloned().collect();
                    let r = od_core::reconciliation(plan, &own_links, &procedures, &adjustments, &splits, &plan_charges, as_of);
                    dyn_issued_count += r.issued_count;
                    dyn_expected_due_count += r.expected_due_count;
                    dyn_issued_total += r.issued_total;
                    dyn_expected_due_total += r.expected_due_total;
                    dyn_missing_due_count += r.missing_due_count;
                    dyn_missing_due_total += r.missing_due_total;
                    dyn_stale_count += r.stale_issued_count;
                    dyn_stale_total += r.stale_issued_total;
                }
            }
            if dyn_count > 0 {
                let stale_suffix = if dyn_stale_count > 0 { format!(" · stale {dyn_stale_count} {dyn_stale_total:.2}") } else { String::new() };
                ui.label(format!(
                    "Dynamic portfolio (as of {as_of}): {dyn_count} plan(s) · issued {dyn_issued_count}/{dyn_expected_due_count} due {dyn_issued_total:.2}/{dyn_expected_due_total:.2} · missing due {dyn_missing_due_count} {dyn_missing_due_total:.2}{stale_suffix}"
                ));
                if dyn_missing_due_count > 0 || dyn_stale_count > 0 {
                    ui.horizontal(|ui| {
                        if dyn_missing_due_count > 0 {
                            ui.colored_label(egui::Color32::from_rgb(180, 90, 0), "Missing due charges detected.");
                        }
                        if dyn_stale_count > 0 {
                            ui.colored_label(egui::Color32::from_rgb(180, 90, 0), "Stale/over-issued charges detected.");
                        }
                        if dyn_missing_due_count > 0 && self.can(Permission::BILLING) && ui.button("Reconcile/issue all due").clicked() {
                            self.reconcile_dynamic_plans_due_portfolio(&plans, &plan_links, &procedures, &adjustments, &splits, &plan_charges, as_of);
                        }
                    });
                }
                ui.separator();
            }
            for plan in &plans {
                let own_links: Vec<PayPlanLink> = plan_links
                    .iter()
                    .filter(|l| l.pay_plan_num == plan.pay_plan_num)
                    .cloned()
                    .collect();
                let breakdown = od_core::pay_plan_balance_breakdown(plan, &plan_charges, &splits, as_of);
                let balance = od_core::pay_plan_balance(plan, &plan_charges, &splits, as_of);
                let due_now = breakdown.due_now - breakdown.unapplied_credit;
                let kind = if plan.is_dynamic { "dynamic" } else { "static" };
                let recon = if plan.is_dynamic {
                    Some(od_core::reconciliation(
                        plan,
                        &own_links,
                        &procedures,
                        &adjustments,
                        &splits,
                        &plan_charges,
                        as_of,
                    ))
                } else {
                    None
                };
                // Same detail used by resolution, surfaced before staff confirm.
                let stale_detail = if plan.is_dynamic {
                    od_core::reconciliation_detail(
                        plan,
                        &own_links,
                        &procedures,
                        &adjustments,
                        &splits,
                        &plan_charges,
                        as_of,
                    )
                    .3
                } else {
                    Vec::new()
                };
                ui.collapsing(
                    format!(
                        "{} — {:.2} owing, {:.2} due now ({}, {:.2}% APR)",
                        if plan.note.is_empty() {
                            format!("Plan #{}", plan.pay_plan_num)
                        } else {
                            plan.note.clone()
                        },
                        balance,
                        due_now,
                        kind,
                        plan.apr
                    ),
                    |ui| {
                        ui.label(format!(
                            "Started {} · {} · {}",
                            plan.date_pay_plan_start
                                .map(|d| d.to_string())
                                .unwrap_or_else(|| plan.pay_plan_date.to_string()),
                            plan.pay_schedule.label(),
                            if plan.is_closed { "closed" } else { "open" }
                        ));
                        ui.label(format!(
                            "Due now {:.2} · Future due {:.2} · Unapplied/overpaid credit {:.2}",
                            breakdown.due_now, breakdown.future_due, breakdown.unapplied_credit
                        ));
                        if let Some(r) = recon {
                            ui.label(format!(
                                "Dynamic reconciliation (as of {}): issued {}/{} due {:.2}/{:.2} · full {}/{} {:.2}/{:.2} · missing due {} {:.2} · stale {} {:.2}",
                                as_of,
                                r.issued_count,
                                r.expected_due_count,
                                r.issued_total,
                                r.expected_due_total,
                                r.issued_count,
                                r.expected_full_count,
                                r.issued_total,
                                r.expected_full_total,
                                r.missing_due_count,
                                r.missing_due_total,
                                r.stale_issued_count,
                                r.stale_issued_total,
                            ));
                            if r.missing_due_count > 0 {
                                ui.colored_label(egui::Color32::from_rgb(180, 90, 0), "Due charges are missing for this as-of date. Use Issue charges due.");
                            }
                            if r.stale_issued_count > 0 {
                                ui.colored_label(egui::Color32::from_rgb(180, 90, 0), "Stale/over-issued charges detected after production/term changes. Resolve explicitly; rows with applied splits are blocked.");
                                egui::CollapsingHeader::new(format!("Stale charge evidence ({})", stale_detail.len()))
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        for line in od_core::stale_charge_evidence(&stale_detail) {
                                            ui.label(line);
                                        }
                                    });
                                if self.can(Permission::BILLING) {
                                    let reason = self
                                        .stale_resolution_reasons
                                        .entry(plan.pay_plan_num)
                                        .or_default();
                                    ui.horizontal(|ui| {
                                        ui.label("Resolution reason");
                                        ui.add(
                                            egui::TextEdit::singleline(reason)
                                                .desired_width(280.0)
                                                .hint_text("Required; saved on charges and audit log"),
                                        );
                                    });
                                    let reason = reason.trim().to_string();
                                    if ui.small_button("Resolve stale charges (safe)").clicked() {
                                        self.resolve_stale_dynamic_plan_charges(
                                            plan,
                                            &own_links,
                                            &procedures,
                                            &adjustments,
                                            &splits,
                                            &plan_charges,
                                            &reason,
                                            as_of,
                                        );
                                    }
                                }
                            }
                        }
                        if breakdown.unapplied_credit > 0.005 && !plan.is_closed {
                            ui.horizontal(|ui| {
                                let target_label = match self.plan_refund_transfer_target {
                                    PaymentTarget::Procedure(proc_num) => format!("Procedure #{}", proc_num),
                                    PaymentTarget::PayPlan(pn) => format!("Plan #{}", pn),
                                    PaymentTarget::PayPlanCharge { plan_num, charge_num } => format!("Plan #{} charge #{}", plan_num, charge_num),
                                    PaymentTarget::Unallocated => "Patient account (unallocated)".into(),
                                };
                                egui::ComboBox::from_id_salt(("plan_refund_target", plan.pay_plan_num))
                                    .selected_text(target_label)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut self.plan_refund_transfer_target, PaymentTarget::Unallocated, "Patient account (unallocated)");
                                        for proc in &procedures {
                                            ui.selectable_value(
                                                &mut self.plan_refund_transfer_target,
                                                PaymentTarget::Procedure(proc.proc_num),
                                                format!("Procedure #{} ({}) {:.2}", proc.proc_num, proc.proc_date, proc.fee),
                                            );
                                        }
                                    });
                                if ui.small_button("Refund/transfer...").clicked() {
                                    let to_target = self.plan_refund_transfer_target;
                                    let target_note = match to_target {
                                        PaymentTarget::Procedure(proc_num) => format!("procedure #{}", proc_num),
                                        PaymentTarget::PayPlan(pn) => format!("plan #{}", pn),
                                        PaymentTarget::PayPlanCharge { plan_num, charge_num } => format!("plan #{} charge #{}", plan_num, charge_num),
                                        PaymentTarget::Unallocated => "patient account (unallocated)".to_string(),
                                    };
                                    self.payment_edit = Some(Payment {
                                        pat_num: plan.pat_num,
                                        pay_date: chrono::Local::now().date_naive(),
                                        note: format!("Refund/transfer unapplied credit from plan #{} to {}", plan.pay_plan_num, target_note),
                                        ..Default::default()
                                    });
                                    self.payment_date_text = chrono::Local::now().format("%Y-%m-%d").to_string();
                                    self.payment_amount_text = "0.00".to_string();
                                    self.payment_rows = vec![
                                        PaymentRow { target: PaymentTarget::PayPlan(plan.pay_plan_num), amount_text: format!("{:.2}", -breakdown.unapplied_credit) },
                                        PaymentRow { target: to_target, amount_text: format!("{:.2}", breakdown.unapplied_credit) },
                                    ];
                                    self.payment_edit_open = true;
                                }
                                if ui.small_button("Resolve unapplied credit to account").clicked() {
                                    let today = chrono::Local::now().date_naive();
                                    match od_core::build_unapplied_plan_credit_transfer(
                                        plan,
                                        breakdown.unapplied_credit,
                                        today,
                                        PaymentTarget::Unallocated,
                                    ) {
                                        Ok((payment, allocs)) => {
                                            match self.store.record_payment_splits(&payment, &allocs) {
                                                Ok(num) => {
                                                    self.audit_if(
                                                        FeatureKey::AuditBillingChanges,
                                                        Permission::BILLING,
                                                        plan.pat_num,
                                                        format!(
                                                            "Resolved unapplied credit {:.2} from plan #{} as transfer payment #{num}",
                                                            breakdown.unapplied_credit, plan.pay_plan_num
                                                        ),
                                                    );
                                                    self.notice = Some("Unapplied plan credit transferred to patient account".into());
                                                    self.error = None;
                                                }
                                                Err(e) => self.error = Some(e),
                                            }
                                        }
                                        Err(e) => self.error = Some(e),
                                    }
                                }
                                if ui.small_button("Resolve and close plan").clicked() {
                                    let today = chrono::Local::now().date_naive();
                                    match od_core::build_unapplied_plan_credit_transfer(
                                        plan,
                                        breakdown.unapplied_credit,
                                        today,
                                        PaymentTarget::Unallocated,
                                    ) {
                                        Ok((payment, allocs)) => {
                                            match self.store.record_payment_splits(&payment, &allocs) {
                                                Ok(num) => {
                                                    let mut closed = plan.clone();
                                                    closed.is_closed = true;
                                                    let charges = self.store.list_pay_plan_charges(plan.pat_num).unwrap_or_default();
                                                    let splits = self.store.list_pay_splits(plan.pat_num).unwrap_or_default();
                                                    if let Some(reason) = od_core::pay_plan_close_block_reason(&closed, &charges, &splits, today) {
                                                        self.error = Some(reason);
                                                    } else if let Err(e) = self.store.upsert_pay_plan(&closed) {
                                                        self.error = Some(e);
                                                    } else {
                                                        self.audit_if(
                                                            FeatureKey::AuditBillingChanges,
                                                            Permission::BILLING,
                                                            plan.pat_num,
                                                            format!(
                                                                "Resolved unapplied credit {:.2} and closed plan #{} via transfer #{num}",
                                                                breakdown.unapplied_credit, plan.pay_plan_num
                                                            ),
                                                        );
                                                        self.notice = Some("Unapplied credit resolved and plan closed".into());
                                                        self.error = None;
                                                    }
                                                }
                                                Err(e) => self.error = Some(e),
                                            }
                                        }
                                        Err(e) => self.error = Some(e),
                                    }
                                }
                            });
                        }
                        if plan.is_dynamic {
                            let entries = od_core::production_entries(&own_links, &procedures, &adjustments, &splits);
                            let principal = od_core::principal_amount(&entries);
                            let completed = od_core::completed_amount(&entries, plan.dynamic_pay_plan_tp_option);
                            let issued: f64 = plan_charges
                                .iter()
                                .filter(|c| c.pay_plan_num == plan.pay_plan_num && c.charge_type == PayPlanChargeType::Debit)
                                .map(|c| c.principal)
                                .sum();
                            ui.label(format!(
                                "TP option: {} · Pay {:.2} per period · {} payment(s)",
                                plan.dynamic_pay_plan_tp_option.label(),
                                plan.pay_amt,
                                plan.number_of_payments
                            ));
                            ui.label(format!(
                                "Linked principal {:.2} · Chargeable now {:.2} · Issued principal {:.2}",
                                principal, completed, issued
                            ));
                        }

                        if billing_edits && self.can(Permission::BILLING) {
                            ui.horizontal(|ui| {
                                if ui.button("Edit plan").clicked() {
                                    self.plan_edit = Some(plan.clone());
                                    self.plan_amount_text = if plan.is_dynamic {
                                        format!("{:.2}", plan.pay_amt)
                                    } else {
                                        format!("{:.2}", plan.completed_amt)
                                    };
                                    self.plan_count_text = plan.number_of_payments.to_string();
                                    self.plan_start_text = plan
                                        .date_pay_plan_start
                                        .map(|d| d.format("%Y-%m-%d").to_string())
                                        .unwrap_or_default();
                                    self.plan_apr_text = format!("{}", plan.apr);
                                    self.plan_down_payment_text = format!("{}", plan.down_payment);
                                    self.plan_interest_start_text = plan
                                        .date_interest_start
                                        .map(|d| d.format("%Y-%m-%d").to_string())
                                        .unwrap_or_default();
                                    self.plan_note_text = plan.note.clone();
                                    self.plan_frequency = plan.charge_frequency;
                                    self.plan_is_dynamic = plan.is_dynamic;
                                    self.plan_tp_option = plan.dynamic_pay_plan_tp_option;
                                    self.plan_is_closed = plan.is_closed;
                                    self.plan_is_locked = plan.is_locked;
                                    self.plan_edit_open = true;
                                }
                                if ui.button("Add charge").clicked() {
                                    self.plan_charge_edit = Some(PayPlanCharge {
                                        pay_plan_num: plan.pay_plan_num,
                                        guarantor: plan.guarantor,
                                        pat_num: plan.pat_num,
                                        charge_type: PayPlanChargeType::Debit,
                                        ..Default::default()
                                    });
                                    self.plan_charge_date_text =
                                        chrono::Local::now().format("%Y-%m-%d").to_string();
                                    self.plan_charge_amount_text =
                                        format!("{:.2}", plan.pay_amt);
                                    self.plan_charge_edit_open = true;
                                }
                                if plan.is_dynamic {
                                    if ui.button("Issue charges due").clicked() {
                                        if let Some(reason) =
                                            od_core::issuance_block(plan, &own_links)
                                        {
                                            self.error = Some(reason.to_string());
                                        } else {
                                            let due = od_core::issue_charges_due(
                                                plan,
                                                &own_links,
                                                &procedures,
                                                &adjustments,
                                                &splits,
                                                &plan_charges
                                                    .iter()
                                                    .filter(|c| c.pay_plan_num == plan.pay_plan_num)
                                                    .cloned()
                                                    .collect::<Vec<_>>(),
                                                today,
                                            );
                                            let mut charge_error = None;
                                            let count = due.len();
                                            for charge in due {
                                                if let Err(e) =
                                                    self.store.upsert_pay_plan_charge(&charge)
                                                {
                                                    charge_error = Some(e);
                                                    break;
                                                }
                                            }
                                            if let Some(e) = charge_error {
                                                self.error = Some(e);
                                            } else {
                                                self.audit_if(
                                                    FeatureKey::AuditBillingChanges,
                                                    Permission::BILLING,
                                                    plan.pat_num,
                                                    format!(
                                                        "Issued {} dynamic charge(s) for plan {}",
                                                        count, plan.pay_plan_num
                                                    ),
                                                );
                                                self.notice = Some(if count == 0 {
                                                    "No new charges due".into()
                                                } else {
                                                    format!("Issued {count} charge(s)")
                                                });
                                                self.error = None;
                                            }
                                        }
                                    }
                                } else if ui.button("Generate charges").clicked() {
                                    let plan_charges_own: Vec<PayPlanCharge> = plan_charges
                                        .iter()
                                        .filter(|c| c.pay_plan_num == plan.pay_plan_num)
                                        .cloned()
                                        .collect();
                                    if let Some(reason) =
                                        od_core::charge_generation_block(plan, &plan_charges_own)
                                    {
                                        self.error = Some(reason.to_string());
                                    } else {
                                        let generated = od_core::generate_plan_charges(plan, today);
                                        let generated_count = generated.len();
                                        let mut charge_error = None;
                                        for charge in generated {
                                            if let Err(e) =
                                                self.store.upsert_pay_plan_charge(&charge)
                                            {
                                                charge_error = Some(e);
                                                break;
                                            }
                                        }
                                        if let Some(e) = charge_error {
                                            self.error = Some(e);
                                        } else {
                                            self.audit_if(
                                                FeatureKey::AuditBillingChanges,
                                                Permission::BILLING,
                                                plan.pat_num,
                                                format!(
                                                    "Generated {} charge(s) for plan {}",
                                                    generated_count, plan.pay_plan_num
                                                ),
                                            );
                                            self.notice = Some("Charges generated".into());
                                            self.error = None;
                                        }
                                    }
                                }
                            });

                            if plan.is_dynamic {
                                ui.separator();
                                ui.strong("Linked production");
                                if own_links.is_empty() {
                                    ui.label("No linked production yet.");
                                } else {
                                    for lnk in &own_links {
                                        let desc = if lnk.link_type == PayPlanLink::PROCEDURE {
                                            procedures
                                                .iter()
                                                .find(|p| p.proc_num == lnk.fkey)
                                                .map(|p| format!("Proc #{} {:.2} {}", p.proc_num, p.fee, p.proc_date))
                                                .unwrap_or_else(|| format!("Proc #{}", lnk.fkey))
                                        } else if lnk.link_type == PayPlanLink::ADJUSTMENT {
                                            adjustments
                                                .iter()
                                                .find(|a| a.adj_num == lnk.fkey)
                                                .map(|a| format!("Adj #{} {:.2} {}", a.adj_num, a.adj_amount, a.adj_date))
                                                .unwrap_or_else(|| format!("Adj #{}", lnk.fkey))
                                        } else {
                                            format!("Link type {} #{}", lnk.link_type, lnk.fkey)
                                        };
                                        ui.horizontal(|ui| {
                                            ui.label(desc);
                                            if ui.small_button("Unlink").clicked() {
                                                if let Err(e) = self.store.delete_pay_plan_link(lnk.pay_plan_link_num) {
                                                    self.error = Some(e);
                                                } else {
                                                    self.audit_if(
                                                        FeatureKey::AuditBillingChanges,
                                                        Permission::BILLING,
                                                        plan.pat_num,
                                                        format!("Unlinked payplanlink {} from plan {}", lnk.pay_plan_link_num, plan.pay_plan_num),
                                                    );
                                                    self.notice = Some("Link removed".into());
                                                }
                                            }
                                        });
                                    }
                                }
                                ui.horizontal(|ui| {
                                    ui.label("Link");
                                    egui::ComboBox::from_id_salt(format!("plan_link_type_{}", plan.pay_plan_num))
                                        .selected_text(if self.plan_link_type == PayPlanLink::ADJUSTMENT { "Adjustment" } else { "Procedure" })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.plan_link_type, PayPlanLink::PROCEDURE, "Procedure");
                                            ui.selectable_value(&mut self.plan_link_type, PayPlanLink::ADJUSTMENT, "Adjustment");
                                        });
                                    let target_label = if self.plan_link_type == PayPlanLink::ADJUSTMENT {
                                        adjustments.iter().find(|a| a.adj_num == self.plan_link_target)
                                            .map(|a| format!("#{} {:.2}", a.adj_num, a.adj_amount))
                                            .unwrap_or_else(|| "Select".into())
                                    } else {
                                        procedures.iter().find(|p| p.proc_num == self.plan_link_target)
                                            .map(|p| format!("#{} {:.2}", p.proc_num, p.fee))
                                            .unwrap_or_else(|| "Select".into())
                                    };
                                    egui::ComboBox::from_id_salt(format!("plan_link_target_{}", plan.pay_plan_num))
                                        .selected_text(target_label)
                                        .show_ui(ui, |ui| {
                                            if self.plan_link_type == PayPlanLink::ADJUSTMENT {
                                                for a in &adjustments {
                                                    if a.pat_num != plan.pat_num { continue; }
                                                    ui.selectable_value(&mut self.plan_link_target, a.adj_num, format!("#{} {:.2} {}", a.adj_num, a.adj_amount, a.adj_date));
                                                }
                                            } else {
                                                for p in &procedures {
                                                    if p.pat_num != plan.pat_num { continue; }
                                                    ui.selectable_value(&mut self.plan_link_target, p.proc_num, format!("#{} {:.2} {} {:?}", p.proc_num, p.fee, p.proc_date, p.proc_status));
                                                }
                                            }
                                        });
                                    if ui.button("Add link").clicked() {
                                        if self.plan_link_target == 0 {
                                            self.error = Some("Select a procedure or adjustment to link.".into());
                                        } else if own_links.iter().any(|l| l.link_type == self.plan_link_type && l.fkey == self.plan_link_target) {
                                            self.error = Some("That production item is already linked to this plan.".into());
                                        } else {
                                            let link = PayPlanLink {
                                                pay_plan_num: plan.pay_plan_num,
                                                link_type: self.plan_link_type,
                                                fkey: self.plan_link_target,
                                                ..Default::default()
                                            };
                                            match self.store.upsert_pay_plan_link(&link) {
                                                Ok(_) => {
                                                    self.audit_if(
                                                        FeatureKey::AuditBillingChanges,
                                                        Permission::BILLING,
                                                        plan.pat_num,
                                                        format!("Linked {:?} {} to plan {}", self.plan_link_type, self.plan_link_target, plan.pay_plan_num),
                                                    );
                                                    self.notice = Some("Production linked".into());
                                                    self.error = None;
                                                    self.plan_link_target = 0;
                                                }
                                                Err(e) => self.error = Some(e),
                                            }
                                        }
                                    }
                                });
                            }
                        }

                        let own_charges =
                            visible_plan_charge_rows(&plan_charges, plan.pay_plan_num, show_resolved);
                        let hidden_resolved = plan_charges
                            .iter()
                            .filter(|c| c.pay_plan_num == plan.pay_plan_num && od_core::is_stale_resolved_charge(c))
                            .count();
                        egui::Grid::new(format!("plan_charges_{}", plan.pay_plan_num))
                            .num_columns(8)
                            .striped(true)
                            .show(ui, |ui| {
                                ui.strong("Due");
                                ui.strong("Type");
                                ui.strong("Principal");
                                ui.strong("Interest");
                                ui.strong("Paid");
                                ui.strong("Remaining");
                                ui.strong("Applications");
                                ui.strong("Status");
                                ui.end_row();
                                for charge in own_charges {
                                    let charge_splits: Vec<_> = splits
                                        .iter()
                                        .filter(|s| s.pay_plan_charge_num == charge.pay_plan_charge_num)
                                        .collect();
                                    let paid: f64 = charge_splits.iter().map(|s| s.split_amount).sum();
                                    let remaining = (charge.principal + charge.interest - paid).max(0.0);
                                    let apps = if charge_splits.is_empty() {
                                        "—".to_string()
                                    } else {
                                        charge_splits.iter().map(|s| format!("{} {:.2} ({})", s.date_pay.format("%m/%d"), s.split_amount, s.pay_plan_debit_type.label())).collect::<Vec<_>>().join("; ")
                                    };
                                    ui.label(charge.charge_date.to_string());
                                    ui.label(charge.charge_type.label());
                                    ui.label(format!("{:.2}", charge.principal));
                                    ui.label(format!("{:.2}", charge.interest));
                                    ui.label(format!("{:.2}", paid));
                                    ui.label(format!("{:.2}", remaining));
                                    ui.label(apps);
                                    if od_core::is_stale_resolved_charge(charge) {
                                        let hover = match od_core::stale_resolution_reason(&charge.note) {
                                            Some(reason) => format!("Resolution reason: {reason}\n\n{}", charge.note),
                                            None => charge.note.clone(),
                                        };
                                        ui.colored_label(egui::Color32::from_rgb(150, 90, 20), "Resolved (stale)")
                                            .on_hover_text(hover);
                                    } else {
                                        ui.label("—");
                                    }
                                    ui.end_row();
                                }
                            });
                        if !show_resolved && hidden_resolved > 0 {
                            ui.small(format!(
                                "{hidden_resolved} stale-resolved charge row(s) hidden; audit history is retained."
                            ));
                        }
                        ui.label("To pay this plan, use Record payment and choose the plan in the dialog.");
                    },
                );
            }
            ui.label("Static plans amortise from APR/start with 'Generate charges' (refuses duplicates). Dynamic plans issue due charges from linked production with 'Issue charges due' (idempotent).");
        }

        if insurance_enabled && self.features.enabled(FeatureKey::AccountShowClaims) {
            ui.separator();
            ui.heading("Insurance claims");
            if claims.is_empty() {
                ui.label("No claims for this patient.");
            }
            for claim in &claims {
                ui.collapsing(
                    format!(
                        "{} — {} ({})",
                        if claim.claim_identifier.is_empty() {
                            format!("Claim #{}", claim.claim_num)
                        } else {
                            claim.claim_identifier.clone()
                        },
                        claim.claim_status.label(),
                        claim.date_sent.map(|d| d.to_string()).unwrap_or_else(|| "not sent".into())
                    ),
                    |ui| {
                        ui.label(format!("Fee billed: {:.2}", claim.total_fee));
                        let lines: Vec<&ClaimProc> = claim_procs
                            .iter()
                            .filter(|cp| cp.claim_num == claim.claim_num)
                            .collect();
                        egui::Grid::new(format!("claim_{}", claim.claim_num))
                            .num_columns(4)
                            .striped(true)
                            .show(ui, |ui| {
                                ui.strong("Proc");
                                ui.strong("Billed");
                                ui.strong("Ins paid");
                                ui.strong("Write-off");
                                ui.end_row();
                                for cp in lines {
                                    ui.label(format!("#{}", cp.proc_num));
                                    ui.label(format!("{:.2}", cp.fee_billed));
                                    ui.label(format!("{:.2}", cp.ins_pay_amt));
                                    ui.label(format!("{:.2}", cp.write_off));
                                    ui.end_row();
                                }
                            });
                    },
                );
            }
        }

        if self.features.enabled(FeatureKey::AccountShowStatements) {
            ui.separator();
            ui.heading("Statements");
            ui.horizontal(|ui| {
                if billing_edits && self.can(Permission::BILLING) {
                    if ui.button("Generate statement").clicked() {
                        self.generate_statement_for(num);
                    }
                }
            });
            if self.statement_pdf_patient != num {
                self.statement_pdf_patient = num;
                self.statement_pdf_path = default_statement_pdf_path(num);
            }
            ui.horizontal(|ui| {
                ui.label("PDF");
                ui.add(
                    egui::TextEdit::singleline(&mut self.statement_pdf_path)
                        .desired_width(300.0)
                        .hint_text("path to .pdf"),
                );
                let save_pdf = ui
                    .button("Save PDF")
                    .on_hover_text("Standard PDF fonts; characters outside Latin-1 appear as '?'.");
                if billing_edits && self.can(Permission::BILLING) && save_pdf.clicked() {
                    let path = self.statement_pdf_path.trim().to_string();
                    if path.is_empty() {
                        self.error = Some("Choose a path for the PDF".into());
                    } else {
                        let patient_name = self
                            .store
                            .get_patient(num)
                            .ok()
                            .flatten()
                            .map(|p| p.display_name())
                            .unwrap_or_else(|| format!("Patient #{num}"));
                        let today = chrono::Local::now().date_naive();
                        let note = format!("Statement generated {}", today);
                        let doc = build_statement_document(
                            num,
                            &patient_name,
                            &procedures,
                            &adjustments,
                            &splits,
                            &plans,
                            &plan_charges,
                            &summary,
                            today,
                            &note,
                        );
                        let bytes = render_statement_pdf(&doc);
                        match std::fs::write(&path, &bytes) {
                            Ok(()) => {
                                let opened = open_in_browser(&path);
                                self.audit_if(
                                    FeatureKey::AuditBillingChanges,
                                    Permission::BILLING,
                                    num,
                                    format!(
                                        "Statement PDF saved for {patient_name}: balance {:.2}, {} line(s), {} byte(s), path {path}",
                                        doc.balance,
                                        doc.lines.len(),
                                        bytes.len()
                                    ),
                                );
                                self.notice = Some(match opened {
                                    Ok(()) => format!("PDF saved to {path}"),
                                    Err(e) => {
                                        format!("PDF saved to {path} (viewer did not open: {e})")
                                    }
                                });
                                self.error = None;
                            }
                            Err(e) => self.error = Some(format!("Could not save PDF: {e}")),
                        }
                    }
                }
            });
            if statements.is_empty() {
                ui.label("No statements for this patient.");
            }
            for statement in &statements {
                ui.horizontal(|ui| {
                    ui.label(
                        statement
                            .date_sent
                            .map(|d| d.to_string())
                            .unwrap_or_else(|| "unsent".into()),
                    );
                    if statement.is_invoice {
                        ui.colored_label(Color32::from_rgb(120, 180, 240), "invoice");
                    }
                    ui.label(&statement.note);
                });
            }
        }
    }

    // ---------------------------------------------------------------- Treatment plans

    fn treatment_ui(&mut self, ui: &mut Ui) {
        self.patient_picker(ui);
        ui.separator();
        let Some(num) = self.selected_patient else {
            ui.label("Select a patient first.");
            return;
        };
        if self.features.enabled(FeatureKey::PatientRegistry)
            && ui.button("Patient registry").clicked()
        {
            self.open_registry(num);
        }
        ui.separator();
        let can_edit = self.features.enabled(FeatureKey::TreatmentPlanEdit)
            && self.can(Permission::TREAT_PLAN_EDIT);
        let show_lines = self.features.enabled(FeatureKey::TreatmentPlanLines);
        let plans = self.store.list_treat_plans(num).unwrap_or_default();
        let providers = self.providers.clone();
        let default_note = self
            .store
            .list_prefs()
            .unwrap_or_default()
            .into_iter()
            .find(|p| p.pref_name.eq_ignore_ascii_case("TreatmentPlanNote"))
            .map(|p| p.pref_value)
            .unwrap_or_default();
        let today = Local::now().date_naive();

        ui.horizontal(|ui| {
            if can_edit && ui.button("New plan").clicked() {
                self.open_treat_plan_editor(od_core::new_treat_plan(
                    num,
                    TreatPlanStatus::Inactive,
                    &default_note,
                ));
            }
            ui.label("OD starts new plans Inactive; use Make active to promote one.");
        });
        ui.separator();

        for plan in &plans {
            let lines = self
                .store
                .list_treat_plan_lines(plan.treat_plan_num)
                .unwrap_or_default();
            let total = od_core::treat_plan_total(&lines);
            let heading = format!(
                "{} — {} ({}) · {:.2}",
                plan.heading,
                plan.date_saved
                    .map(|d| d.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "not saved".into()),
                treat_plan_status_label(plan),
                total
            );
            ui.collapsing(heading, |ui| {
                if !plan.note.trim().is_empty() {
                    ui.label(&plan.note);
                }
                let provider = providers
                    .iter()
                    .find(|p| p.provider_num == plan.provider_num)
                    .map(|p| p.display_name())
                    .unwrap_or_default();
                if !provider.is_empty() {
                    ui.label(format!("Provider: {provider}"));
                }
                ui.label(format!(
                    "OD storage: {}",
                    if plan.status == TreatPlanStatus::Saved {
                        "proctp snapshot"
                    } else {
                        "treatplanattach links"
                    }
                ));

                ui.horizontal(|ui| {
                    if can_edit && ui.button("Edit plan").clicked() {
                        self.open_treat_plan_editor(plan.clone());
                    }
                    if can_edit
                        && plan.status != TreatPlanStatus::Active
                        && ui.button("Make active").clicked()
                    {
                        self.change_plan_status(num, plan.treat_plan_num, TreatPlanStatus::Active);
                    }
                    if can_edit
                        && plan.status == TreatPlanStatus::Active
                        && ui.button("Make inactive").clicked()
                    {
                        self.change_plan_status(num, plan.treat_plan_num, TreatPlanStatus::Inactive);
                    }
                    if can_edit
                        && plan.status != TreatPlanStatus::Saved
                        && ui.button("Save as snapshot").clicked()
                    {
                        self.snapshot_treat_plan(plan, &lines, today);
                    }
                    if can_edit
                        && plan.status != TreatPlanStatus::Active
                        && ui.button("Delete plan").clicked()
                    {
                        match self.store.delete_treat_plan(plan.treat_plan_num) {
                            Ok(()) => {
                                self.audit_if(
                                    FeatureKey::AuditTreatmentPlans,
                                    Permission::TREAT_PLAN_EDIT,
                                    num,
                                    format!("Treatment plan {} deleted", plan.treat_plan_num),
                                );
                                self.notice = Some("Treatment plan deleted".into());
                            }
                            Err(err) => self.error = Some(err),
                        }
                    }
                });

                if show_lines {
                    ui.separator();
                    egui::Grid::new(format!("plan_lines_{}", plan.treat_plan_num))
                        .num_columns(6)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Code");
                            ui.strong("Tooth");
                            ui.strong("Description");
                            ui.strong("Fee");
                            ui.strong("Priority");
                            ui.strong("");
                            ui.end_row();
                            for line in &lines {
                                ui.label(&line.proc_code);
                                ui.label(&line.tooth_num);
                                ui.label(&line.description);
                                ui.label(format!("{:.2}", line.fee));
                                ui.label(line.priority.to_string());
                                ui.horizontal(|ui| {
                                    if can_edit && ui.button("Edit").clicked() {
                                        self.open_treat_plan_line_editor(
                                            plan.treat_plan_num,
                                            line.clone(),
                                        );
                                    }
                                    if can_edit && ui.button("Remove").clicked() {
                                        if let Err(err) = self.store.delete_treat_plan_line(
                                            plan,
                                            line.line_num,
                                        ) {
                                            self.error = Some(err);
                                        } else {
                                            self.audit_if(
                                                FeatureKey::AuditTreatmentPlans,
                                                Permission::TREAT_PLAN_EDIT,
                                                num,
                                                format!(
                                                    "Line {} removed from treatment plan {}",
                                                    line.line_num, plan.treat_plan_num
                                                ),
                                            );
                                        }
                                    }
                                });
                                ui.end_row();
                            }
                        });
                    if can_edit && ui.button("Add procedure line").clicked() {
                        self.open_treat_plan_line_editor(
                            plan.treat_plan_num,
                            TreatPlanLine {
                                treat_plan_num: plan.treat_plan_num,
                                pat_num: num,
                                ..Default::default()
                            },
                        );
                    }
                }
            });
        }
        if plans.is_empty() {
            ui.label("No treatment plans for this patient.");
        }
    }

    fn open_treat_plan_editor(&mut self, plan: TreatPlan) {
        self.treat_plan_heading_text = plan.heading.clone();
        self.treat_plan_note_text = plan.note.clone();
        self.treat_plan_date_text = plan
            .date_saved
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        self.treat_plan_status = plan.status;
        self.treat_plan_edit = Some(plan);
        self.treat_plan_edit_open = true;
    }

    fn open_treat_plan_line_editor(&mut self, plan_num: Id, line: TreatPlanLine) {
        self.treat_plan_line_plan_num = plan_num;
        self.treat_plan_line_proc_num = line.proc_num;
        self.treat_plan_line_code_text = line.proc_code.clone();
        self.treat_plan_line_desc_text = line.description.clone();
        self.treat_plan_line_tooth_text = line.tooth_num.clone();
        self.treat_plan_line_fee_text = if line.fee == 0.0 {
            String::new()
        } else {
            format!("{:.2}", line.fee)
        };
        self.treat_plan_line_edit = Some(line);
        self.treat_plan_line_edit_open = true;
    }

    fn change_plan_status(&mut self, pat_num: Id, plan_num: Id, status: TreatPlanStatus) {
        let plans = self.store.list_treat_plans(pat_num).unwrap_or_default();
        let mut failed = None;
        for changed in od_core::set_status(&plans, plan_num, status) {
            if let Err(err) = self.store.upsert_treat_plan(&changed) {
                failed = Some(err);
                break;
            }
        }
        if let Some(err) = failed {
            self.error = Some(err);
            return;
        }
        self.audit_if(
            FeatureKey::AuditTreatmentPlans,
            Permission::TREAT_PLAN_EDIT,
            pat_num,
            format!(
                "Treatment plan {plan_num} set to {}",
                status.label()
            ),
        );
        self.notice = Some(format!("Treatment plan is now {}", status.label()));
    }

    fn snapshot_treat_plan(&mut self, plan: &TreatPlan, lines: &[TreatPlanLine], today: NaiveDate) {
        let (saved, copied) = od_core::snapshot_plan(plan, lines, today);
        match self.store.upsert_treat_plan(&saved) {
            Ok(saved_num) => {
                let mut saved_plan = saved.clone();
                saved_plan.treat_plan_num = saved_num;
                let mut failed = None;
                for mut line in copied {
                    line.treat_plan_num = saved_num;
                    if let Err(err) = self.store.upsert_treat_plan_line(&saved_plan, &line) {
                        failed = Some(err);
                        break;
                    }
                }
                self.audit_if(
                    FeatureKey::AuditTreatmentPlans,
                    Permission::TREAT_PLAN_EDIT,
                    plan.pat_num,
                    format!("Saved snapshot {saved_num} created from plan {}", plan.treat_plan_num),
                );
                if let Some(err) = failed {
                    self.error = Some(err);
                } else {
                    self.notice = Some(format!("Saved snapshot #{saved_num} created"));
                }
            }
            Err(err) => self.error = Some(err),
        }
    }

    // ---------------------------------------------------------------- Chart

    fn chart_ui(&mut self, ui: &mut Ui) {
        self.patient_picker(ui);
        ui.separator();
        let Some(num) = self.selected_patient else {
            ui.label("Select a patient first.");
            return;
        };
        let procedures = self.store.list_procedures(num).unwrap_or_default();

        if self.features.enabled(FeatureKey::ProcedureCompletion) {
            ui.horizontal(|ui| {
                if ui.button("New procedure").clicked() {
                    self.open_new_procedure(num);
                }
                if self.features.enabled(FeatureKey::DiscountPlanAutoAdjust) {
                    ui.label(
                        "Completing a procedure prices it from the patient's active discount plan and posts the difference as an adjustment.",
                    );
                }
            });
        }

        if self.features.enabled(FeatureKey::ChartToothSelection) {
            ui.strong("Tooth chart (universal numbering)");
            for row in 0..2 {
                ui.horizontal(|ui| {
                    for tooth in 1..=16u8 {
                        let number = if row == 0 { tooth } else { 33 - tooth };
                        let label = number.to_string();
                        let selected = self.selected_tooth.as_deref() == Some(label.as_str());
                        if ui.selectable_label(selected, label).clicked() {
                            self.selected_tooth = Some(number.to_string());
                        }
                    }
                });
            }
            if let Some(tooth) = &self.selected_tooth {
                ui.label(format!("Selected tooth: {tooth}"));
            }
            ui.separator();
        }

        ui.strong("Procedure history");
        let show_actions = self.features.enabled(FeatureKey::ProcedureCompletion);
        egui::Grid::new("chart_procedures")
            .num_columns(if show_actions { 6 } else { 5 })
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Date");
                ui.strong("Code");
                ui.strong("Tooth");
                ui.strong("Status");
                ui.strong("Fee");
                if show_actions {
                    ui.strong("Action");
                }
                ui.end_row();
                for proc in &procedures {
                    ui.label(proc.proc_date.to_string());
                    ui.label(proc.code_num.to_string());
                    ui.label(&proc.tooth_num);
                    ui.label(proc.proc_status.label());
                    ui.label(format!("{:.2}", proc.fee));
                    if show_actions {
                        if ui.button("Complete").clicked() {
                            self.complete_procedure(proc);
                        }
                    }
                    ui.end_row();
                }
            });
        if procedures.is_empty() {
            ui.label("No completed or planned procedures recorded.");
        }
    }

    /// Open the new-procedure dialog with OD-like defaults from the selected
    /// CDT code, the patient's primary provider and the selected tooth.
    fn open_new_procedure(&mut self, pat_num: Id) {
        let today = Local::now().date_naive();
        let codes = self.store.list_procedure_codes().unwrap_or_default();
        let first = codes.first().cloned().unwrap_or_default();
        let provider = self
            .patients
            .iter()
            .find(|p| p.pat_num == pat_num)
            .map(|p| p.primary_provider_num)
            .unwrap_or(0);
        self.proc_edit = Some(Procedure {
            pat_num,
            proc_date: today,
            code_num: first.code_num,
            provider_num: provider,
            fee: first.fee,
            unit_qty: 1.0,
            tooth_num: self.selected_tooth.clone().unwrap_or_default(),
            ..Default::default()
        });
        self.proc_date_text = today.format("%Y-%m-%d").to_string();
        self.proc_code_num = first.code_num;
        self.proc_fee_text = format!("{:.2}", first.fee);
        self.proc_tooth_text = self.selected_tooth.clone().unwrap_or_default();
        self.proc_units_text = "1".into();
        self.proc_edit_open = true;
    }

    fn procedure_window(&mut self, ctx: &egui::Context) {
        let Some(mut proc) = self.proc_edit.take() else {
            return;
        };
        let codes = self.store.list_procedure_codes().unwrap_or_default();
        let selected = codes
            .iter()
            .find(|c| c.code_num == self.proc_code_num)
            .map(|c| c.code.clone())
            .unwrap_or_else(|| "(select)".into());
        let mut save = false;
        let mut cancel = false;
        egui::Window::new("Procedure — New")
            .open(&mut self.proc_edit_open)
            .resizable(true)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.label("Creates a planned procedure. Complete it from the Chart grid to apply discount-plan pricing.");
                ui.horizontal(|ui| {
                    ui.label("Date  ");
                    ui.add(egui::TextEdit::singleline(&mut self.proc_date_text).hint_text("YYYY-MM-DD"));
                });
                egui::ComboBox::from_label("Code")
                    .selected_text(selected.clone())
                    .show_ui(ui, |ui| {
                        for code in &codes {
                            let text = format!("{} — {}", code.code, code.code_name);
                            ui.selectable_value(&mut self.proc_code_num, code.code_num, text);
                        }
                    });
                ui.horizontal(|ui| {
                    ui.label("Fee   ");
                    ui.text_edit_singleline(&mut self.proc_fee_text);
                });
                ui.horizontal(|ui| {
                    ui.label("Tooth ");
                    ui.text_edit_singleline(&mut self.proc_tooth_text);
                });
                ui.horizontal(|ui| {
                    ui.label("Units ");
                    ui.text_edit_singleline(&mut self.proc_units_text);
                });
                ui.label("OD quantity = BaseUnits + UnitQty, never less than 1.");
                ui.separator();
                ui.horizontal(|ui| {
                    save = ui.button("Save").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if save {
            proc.proc_date =
                NaiveDate::parse_from_str(&self.proc_date_text, "%Y-%m-%d").unwrap_or(proc.proc_date);
            proc.code_num = self.proc_code_num;
            proc.fee = self.proc_fee_text.parse().unwrap_or(0.0);
            proc.unit_qty = self.proc_units_text.parse().unwrap_or(1.0);
            proc.tooth_num = self.proc_tooth_text.clone();
            let pat_num = proc.pat_num;
            match self.store.insert_procedure(&proc) {
                Ok(num) => {
                    self.audit_if(
                        FeatureKey::AuditBillingChanges,
                        Permission::CHART_MODULE,
                        pat_num,
                        format!("Procedure {num} created ({})", proc.code_num),
                    );
                    self.notice = Some(format!("Procedure {num} created"));
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
            self.proc_edit_open = false;
        } else if cancel {
            self.proc_edit_open = false;
        } else if self.proc_edit_open {
            self.proc_edit = Some(proc);
        }
    }

    /// Look up the reduced fee for a procedure the way OD does: first on the
    /// plan's own schedule for the procedure's provider, then the generic row
    /// on that schedule. The port has no provider-level fee schedule in its
    /// domain model, so remaining schedules are searched as a final fallback.
    fn discount_fee_for(&self, plan: &DiscountPlan, proc: &Procedure) -> Option<f64> {
        let schedules = self.store.list_fee_schedules().unwrap_or_default();
        let mut candidates = vec![
            (plan.fee_sched_num, proc.provider_num),
            (plan.fee_sched_num, 0),
        ];
        for schedule in &schedules {
            if schedule.fee_sched_num == plan.fee_sched_num {
                continue;
            }
            candidates.push((schedule.fee_sched_num, proc.provider_num));
            candidates.push((schedule.fee_sched_num, 0));
        }
        for (schedule_num, provider_num) in candidates {
            if let Ok(fees) = self.store.list_fees(schedule_num, provider_num) {
                if let Some(fee) = fees.into_iter().find(|f| f.code_num == proc.code_num) {
                    return Some(fee.amount);
                }
            }
        }
        None
    }

    /// Mirror `Procedures.FormProcEditUpdate` → `Adjustments.CreateAdjustmentForDiscountPlan`:
    /// price the procedure from the patient's active subscription, post the
    /// negative adjustment, store `DiscountPlanAmt`, then mark it complete.
    fn complete_procedure(&mut self, source: &Procedure) {
        if !self.features.enabled(FeatureKey::ProcedureCompletion) {
            return;
        }
        let proc = source.clone();
        let auto = self.features.enabled(FeatureKey::DiscountPlans)
            && self.features.enabled(FeatureKey::DiscountPlanAutoAdjust);
        if auto && !self.can(Permission::ADJUSTMENT_CREATE) {
            self.error = Some(
                "Completing a discounted procedure requires the Adjustment Create permission."
                    .into(),
            );
            return;
        }
        let today = Local::now().date_naive();
        let mut discount_amount_value = 0.0;
        let mut discount_note = String::new();
        let mut discount_def_num = 0;

        if auto {
            let subscription = self
                .discount_plan_subs
                .iter()
                .filter(|s| s.pat_num == proc.pat_num && s.is_valid_for_date(proc.proc_date))
                .min_by_key(|s| s.discount_sub_num)
                .cloned();
            match subscription {
                None => self.notice = Some("No active discount subscription for this patient.".into()),
                Some(sub) => {
                    let plan = self
                        .discount_plans
                        .iter()
                        .find(|p| p.discount_plan_num == sub.discount_plan_num)
                        .cloned();
                    match plan {
                        None => self.notice = Some("The patient's discount plan no longer exists.".into()),
                        Some(plan) => {
                            let (bucket_start, bucket_end) = annual_bucket(
                                proc.proc_date,
                                sub.date_effective,
                                sub.date_term,
                                today,
                            );
                            let history = self.store.list_procedures(proc.pat_num).unwrap_or_default();
                            let running_total: f64 = self
                                .store
                                .list_adjustments(proc.pat_num)
                                .unwrap_or_default()
                                .iter()
                                .filter(|a| {
                                    a.adj_amount < 0.0
                                        && a.adj_type == plan.def_num
                                        && a.proc_num > 0
                                        && a.proc_num != proc.proc_num
                                        && a.proc_date >= bucket_start
                                        && a.proc_date <= bucket_end
                                })
                                .map(|a| -a.adj_amount)
                                .sum();
                            let prefs = self.store.list_prefs().unwrap_or_default();
                            let codes = self.store.list_procedure_codes().unwrap_or_default();
                            let frequency_codes = FrequencyCodeSets::from_prefs(&prefs, &codes);
                            let result = discount_amount(&DiscountInput {
                                procedure: &proc,
                                plan: &plan,
                                subscription: &sub,
                                discount_fee: self.discount_fee_for(&plan, &proc),
                                running_total,
                                history: &history,
                                frequency_codes: &frequency_codes,
                                today,
                            });
                            discount_amount_value = result.amount;
                            discount_def_num = plan.def_num;
                            discount_note = format!("Discount plan: {}", plan.description);
                            if result.exceeds_frequency {
                                self.notice = Some(
                                    "Procedure completed. No discount: frequency limit already used."
                                        .into(),
                                );
                            } else if result.exceeds_annual_max {
                                self.notice = Some(format!(
                                    "Procedure completed. Annual maximum applied: {:.2}.",
                                    result.amount
                                ));
                            }
                        }
                    }
                }
            }
        }

        if discount_amount_value > 0.0 {
            let adjustment = Adjustment {
                adj_date: today,
                proc_date: proc.proc_date,
                adj_amount: -discount_amount_value,
                pat_num: proc.pat_num,
                adj_type: discount_def_num,
                provider_num: proc.provider_num,
                proc_num: proc.proc_num,
                note: discount_note,
                ..Default::default()
            };
            let pat_num = proc.pat_num;
            match self.store.upsert_adjustment(&adjustment) {
                Ok(num) => {
                    self.audit_if(
                        FeatureKey::AuditBillingChanges,
                        Permission::ADJUSTMENT_CREATE,
                        pat_num,
                        format!(
                            "Adjustment made for discount plan: {} ({:.2})",
                            num, adjustment.adj_amount
                        ),
                    );
                }
                Err(e) => {
                    self.error = Some(format!("Could not post discount adjustment: {e}"));
                    return;
                }
            }
        }

        match self
            .store
            .set_procedure_complete(proc.proc_num, discount_amount_value)
        {
            Ok(()) => {
                self.audit(
                    Permission::CHART_MODULE,
                    proc.pat_num,
                    format!(
                        "Procedure {} set complete (discount {:.2})",
                        proc.proc_num, discount_amount_value
                    ),
                );
                if discount_amount_value > 0.0 && self.notice.is_none() {
                    self.notice = Some(format!(
                        "Procedure completed; {:.2} discount adjustment posted.",
                        discount_amount_value
                    ));
                }
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    // ---------------------------------------------------------------- Reports

    /// Navigate a Collections row's individual patient into the Account view.
    fn open_patient_account(&mut self, pat_num: Id) -> bool {
        if !self.features.module_enabled(Module::Account) {
            self.error = Some("Account module is turned off in Settings".into());
            return false;
        }
        self.selected_patient = Some(pat_num);
        self.module = Module::Account;
        self.show_settings = false;
        self.error = None;
        true
    }

    /// Shared by the Account view and the Collections worklist so both paths
    /// create an identical patient-only statement and audit it the same way.
    fn generate_statement_for(&mut self, num: Id) {
        let today = Local::now().date_naive();
        let procedures = self.store.list_procedures(num).unwrap_or_default();
        let adjustments = self.store.list_adjustments(num).unwrap_or_default();
        let splits = self.store.list_pay_splits(num).unwrap_or_default();
        let plans = self.store.list_pay_plans(num).unwrap_or_default();
        let plan_charges = self.store.list_pay_plan_charges(num).unwrap_or_default();
        let summary = account_summary_without_insurance(&procedures, &adjustments, &splits);
        let patient_name = self
            .store
            .get_patient(num)
            .ok()
            .flatten()
            .map(|p| p.display_name())
            .unwrap_or_else(|| format!("Patient #{num}"));
        let note = format!("Statement generated {}", today);
        let doc = build_statement_document(
            num,
            &patient_name,
            &procedures,
            &adjustments,
            &splits,
            &plans,
            &plan_charges,
            &summary,
            today,
            &note,
        );
        let html = render_statement_html(&doc);
        let statement = Statement {
            pat_num: num,
            date_sent: Some(today),
            note: note.clone(),
            is_invoice: false,
            ..Default::default()
        };
        match self.store.insert_statement(&statement) {
            Ok(statement_num) => match open_statement_in_browser(&html, num) {
                Ok(path) => {
                    self.audit_if(
                        FeatureKey::AuditBillingChanges,
                        Permission::BILLING,
                        num,
                        format!(
                            "Statement {statement_num} generated for {patient_name}: balance {:.2}, {} line(s), opened {path}",
                            doc.balance,
                            doc.lines.len()
                        ),
                    );
                    self.notice = Some(format!(
                        "Statement {statement_num} generated and opened for printing"
                    ));
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            },
            Err(e) => self.error = Some(e),
        }
    }

    fn apply_collections_action(&mut self, pat_num: Id, action: CollectionsAction) {
        match action {
            CollectionsAction::OpenAccount => {
                if self.open_patient_account(pat_num) {
                    self.notice = Some(format!("Opened account for patient #{pat_num}"));
                }
            }
            CollectionsAction::RecordPayment => {
                if !self.open_patient_account(pat_num) {
                    return;
                }
                if !(self.features.enabled(FeatureKey::AccountBillingEdits)
                    && self.can(Permission::PAYMENT_CREATE))
                {
                    self.error =
                        Some("Payment permission/feature required to record a payment".into());
                    return;
                }
                let procedures = self.store.list_procedures(pat_num).unwrap_or_default();
                self.payment_edit = Some(Payment {
                    pat_num,
                    pay_date: Local::now().date_naive(),
                    ..Default::default()
                });
                self.payment_date_text = Local::now().format("%Y-%m-%d").to_string();
                self.payment_amount_text.clear();
                self.payment_rows = vec![PaymentRow {
                    target: procedures
                        .first()
                        .map(|p| PaymentTarget::Procedure(p.proc_num))
                        .unwrap_or(PaymentTarget::Unallocated),
                    amount_text: String::new(),
                }];
                self.payment_edit_open = true;
                self.error = None;
            }
            CollectionsAction::Statement => {
                if !self.features.enabled(FeatureKey::AccountShowStatements) {
                    self.error = Some("Statements are turned off in Settings".into());
                    return;
                }
                if !(self.features.enabled(FeatureKey::AccountBillingEdits)
                    && self.can(Permission::BILLING))
                {
                    self.error = Some("Billing permission/feature required for statements".into());
                    return;
                }
                self.generate_statement_for(pat_num);
            }
        }
    }

    fn reports_ui(&mut self, ui: &mut Ui) {
        let enabled_reports: Vec<ReportKind> = ReportKind::ALL
            .into_iter()
            .filter(|kind| self.features.enabled(kind.feature_key()))
            .collect();
        if enabled_reports.is_empty() {
            ui.label("All reports are turned off in Settings.");
            return;
        }
        let mut current = self.report;
        if !enabled_reports.contains(&current) {
            current = enabled_reports[0];
            self.report = current;
        }
        if self.report_export_kind != Some(current) {
            self.report_export_path = self
                .report_prefs
                .effective_export_path(&self.period_close_history_user_key(), current.file_stem())
                .map(str::to_string)
                .unwrap_or_else(|| default_report_path(current, "csv"));
            self.report_export_kind = Some(current);
        }
        if self.report_sort_kind != Some(current) {
            self.load_report_sort(current);
        }

        ui.horizontal(|ui| {
            egui::ComboBox::from_label("Report")
                .selected_text(current.label())
                .show_ui(ui, |ui| {
                    for kind in enabled_reports {
                        ui.selectable_value(&mut current, kind, kind.label());
                    }
                });
            ui.separator();
            let mut date_text_changed = false;
            if current.uses_date_range() {
                ui.label("From");
                date_text_changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut self.report_from_text)
                            .desired_width(100.0)
                            .hint_text("YYYY-MM-DD"),
                    )
                    .changed();
                ui.label("to");
                date_text_changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut self.report_to_text)
                            .desired_width(100.0)
                            .hint_text("YYYY-MM-DD"),
                    )
                    .changed();
                if ui.button("Last 30 days").clicked() {
                    let today = chrono::Local::now().date_naive();
                    self.apply_report_dates(today - Duration::days(30), today);
                }
                if current == ReportKind::Birthday && ui.button("Next Month").clicked() {
                    let today = chrono::Local::now().date_naive();
                    let this_first = today.with_day(1).unwrap_or(today);
                    let next_first = (this_first + Duration::days(32))
                        .with_day(1)
                        .unwrap_or(this_first);
                    let after_next = (next_first + Duration::days(32))
                        .with_day(1)
                        .unwrap_or(next_first);
                    self.apply_report_dates(next_first, after_next - Duration::days(1));
                }
            } else if current == ReportKind::PeriodCloseHistory {
                ui.label("Full audit history (all dates)");
            } else {
                ui.label("As of");
                date_text_changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut self.report_to_text)
                            .desired_width(100.0)
                            .hint_text("YYYY-MM-DD"),
                    )
                    .changed();
            }
            if date_text_changed {
                self.commit_report_date_texts(current.uses_date_range());
            }
        });
        if current != ReportKind::PeriodCloseHistory
            && !self.report_date_text_valid(current.uses_date_range())
        {
            let error_color = ui.visuals().error_fg_color;
            let hint = if current.uses_date_range() {
                "Dates must be YYYY-MM-DD, with the start on or before the end."
            } else {
                "End date must be YYYY-MM-DD."
            };
            ui.colored_label(error_color, hint);
        }
        if current != self.report {
            self.report = current;
            self.report_export_path = self
                .report_prefs
                .effective_export_path(&self.period_close_history_user_key(), current.file_stem())
                .map(str::to_string)
                .unwrap_or_else(|| default_report_path(current, "csv"));
            self.report_export_kind = Some(current);
            self.load_report_sort(current);
            self.persist_selected_report_kind();
        }
        if !current.uses_date_range() {
            if current == ReportKind::PeriodCloseHistory {
                ui.label("Every audited close and reopen is listed, regardless of the date range above.");
                self.period_close_history_filters(ui);
            } else {
                ui.label("Balance and summary reports use the range end date as 'as of'; the start date is ignored.");
            }
        }
        // Accounting reports read only `journalentry`, so charges, payments and
        // adjustments must be posted before the Balance Sheet and P&L are complete.
        if matches!(
            current,
            ReportKind::BalanceSheet
                | ReportKind::GeneralLedger
                | ReportKind::ProfitLoss
                | ReportKind::JournalPostingAudit
                | ReportKind::LedgerTieOut
        ) {
            ui.horizontal(|ui| {
                if self.features.enabled(FeatureKey::LedgerJournalPosting)
                    && self.can(Permission::ADJUSTMENT_CREATE)
                {
                    if ui.button("Post all unposted sources").clicked() {
                        let accounts = self.store.list_accounts().unwrap_or_default();
                        self.post_all_defaults = PostingAccountDefaults::from_chart(&accounts);
                        self.post_all_open = true;
                        self.error = None;
                    }
                    ui.weak("Posts every unposted charge, payment and adjustment in this report range.")
                        .on_hover_text("One balanced journal pair per source, all written atomically — then re-run the ledger tie-out to confirm the period.");
                } else {
                    ui.weak("Journal posting is off in Settings, or the Adjustment Create permission is missing.");
                }
            });
            let locked = self.closed_periods.in_range(self.report_from, self.report_to);
            if !locked.is_empty() {
                let labels = locked
                    .iter()
                    .map(|p| p.label())
                    .collect::<Vec<_>>()
                    .join(", ");
                ui.horizontal(|ui| {
                    ui.colored_label(
                        Color32::from_rgb(200, 130, 30),
                        format!("Closed period(s) in range: {labels} — posting to them is locked."),
                    )
                    .on_hover_text("Reopen the period in the Ledger tie-out report to post a late correction, then close it again with a new reason.");
                });
            }
        }
        if current == ReportKind::LedgerTieOut {
            self.period_close_controls(ui);
        }
        // The upstream reports expose the same option checkboxes/radios as the
        // original FormRp* dialogs. Changes are saved with the other report
        // preferences so a practice keeps its choices between runs.
        if matches!(
            current,
            ReportKind::Aging
                | ReportKind::CustomAging
                | ReportKind::PayPlanDetail
                | ReportKind::PayPlanChargeApplications
                | ReportKind::Birthday
                | ReportKind::NewPatients
                | ReportKind::Appointments
                | ReportKind::ActivePatients
                | ReportKind::ProdInc
                | ReportKind::PaySheet
                | ReportKind::AdjSheet
                | ReportKind::FinanceCharge
                | ReportKind::BrokenAppointments
                | ReportKind::Confirmation
                | ReportKind::RecallFields
                | ReportKind::DailyProcedures
                | ReportKind::ProcedureCodes
                | ReportKind::Prescriptions
                | ReportKind::ReferralAnalysis
                | ReportKind::Referrals
                | ReportKind::Patients
        ) {
            let mut options_changed = false;
            let insurance = self.features.enabled(FeatureKey::InsuranceEnabled);
            let provider_choices = self.providers.clone();
            let payment_type_choices = sorted_definitions(
                &self.definitions,
                Some(DefinitionCategory::PAYMENT_TYPES),
                true,
            );
            // The type picker lists only non-hidden AdjTypes, as upstream's
            // list box does. "All" still reports hidden types in the data.
            let adj_type_choices = sorted_definitions(
                &self.definitions,
                Some(DefinitionCategory::ADJ_TYPES),
                false,
            );
            // Finance-charge types may be hidden, so its picker shows all defs.
            let all_adj_type_choices = sorted_definitions(
                &self.definitions,
                Some(DefinitionCategory::ADJ_TYPES),
                true,
            );
            ui.collapsing("Report options (as in Open Dental)", |ui| {
                match current {
                    ReportKind::Aging => {
                        ui.horizontal(|ui| {
                            ui.label("Age of Account");
                            for age in AgingAgeFilter::ALL {
                                options_changed |= ui
                                    .radio_value(&mut self.report_prefs.aging.age, age, age.label())
                                    .clicked();
                            }
                        });
                        options_changed |= ui
                            .checkbox(
                                &mut self.report_prefs.aging.include_negative,
                                "Include patients with negative balances",
                            )
                            .changed();
                        options_changed |= ui
                            .checkbox(
                                &mut self.report_prefs.aging.only_negative,
                                "Only patients with negative balances",
                            )
                            .changed();
                        options_changed |= ui
                            .checkbox(
                                &mut self.report_prefs.aging.show_last_pay,
                                "Show last payment date",
                            )
                            .changed();
                        if insurance {
                            options_changed |= ui
                                .checkbox(
                                    &mut self.report_prefs.aging.age_writeoffs,
                                    "Age write-off estimates",
                                )
                                .changed();
                        }
                    }
                    ReportKind::PayPlanDetail => {
                        ui.collapsing("Payment plan options (FormRpPayPlans)", |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Payment Plan Types");
                                for kind in PlanTypeFilter::ALL {
                                    options_changed |= ui
                                        .radio_value(
                                            &mut self.report_prefs.pay_plans.plan_type,
                                            kind,
                                            kind.label(),
                                        )
                                        .clicked();
                                }
                            });
                            if !insurance {
                                ui.label("Insurance plans are hidden while Insurance & claims is off.");
                            }
                            options_changed |= ui
                                .checkbox(
                                    &mut self.report_prefs.pay_plans.hide_completed,
                                    "Hide completed payment plans",
                                )
                                .changed();
                            options_changed |= ui
                                .checkbox(
                                    &mut self.report_prefs.pay_plans.show_family_balance,
                                    "Show Family Balance",
                                )
                                .changed();
                            options_changed |= ui
                                .checkbox(
                                    &mut self.report_prefs.pay_plans.limit_date_range,
                                    "Limit to plans created in date range",
                                )
                                .changed();
                            options_changed |= ui
                                .checkbox(
                                    &mut self.report_prefs.pay_plans.show_stale_resolved,
                                    "Show stale-resolved count and reasons",
                                )
                                .changed();
                            let only_changed = ui
                                .checkbox(
                                    &mut self.report_prefs.pay_plans.only_stale_resolved,
                                    "Only plans with stale-resolved charges (audit view)",
                                )
                                .changed();
                            if only_changed && self.report_prefs.pay_plans.only_stale_resolved {
                                // The recorded reasons are the point of the
                                // audit view, so never hide them on this path.
                                self.report_prefs.pay_plans.show_stale_resolved = true;
                            }
                            options_changed |= only_changed;
                        });
                    }
                    ReportKind::PayPlanChargeApplications => {
                        options_changed |= ui
                            .checkbox(
                                &mut self
                                    .report_prefs
                                    .pay_plan_charge_applications
                                    .only_refunds_transfers,
                                "Only refunds/transfers (negative or resolved plan credit)",
                            )
                            .changed();
                        ui.label("Shows how payments were applied to plan charges (principal/interest), unapplied amounts, and refund/transfer notes.");
                    }
                    ReportKind::NewPatients => {
                        options_changed |= ui
                            .checkbox(
                                &mut self.report_prefs.new_patients.include_address,
                                "Include address information",
                            )
                            .changed();
                        options_changed |= ui
                            .checkbox(
                                &mut self.report_prefs.new_patients.exclude_no_production,
                                "Exclude patients with no production",
                            )
                            .changed();
                        ui.label("New patients are found by their first completed procedure in the range.");
                    }
                    ReportKind::Appointments => {
                        ui.horizontal(|ui| {
                            ui.label("Date basis");
                            for basis in AppointmentDateBasis::ALL {
                                options_changed |= ui
                                    .radio_value(
                                        &mut self.report_prefs.appointments.date_basis,
                                        basis,
                                        basis.label(),
                                    )
                                    .clicked();
                            }
                        });
                        options_changed |= ui
                            .checkbox(
                                &mut self.report_prefs.appointments.show_note_appointments,
                                "Show \"Note\" Appointments",
                            )
                            .changed();
                        if self.report_prefs.appointments.date_basis
                            == AppointmentDateBasis::Created
                        {
                            ui.label("This port falls back to the appointment date: creation timestamps are not stored.");
                        }
                    }
                    ReportKind::ActivePatients => {
                        options_changed |= ui
                            .checkbox(
                                &mut self.report_prefs.active_patients.all_providers,
                                "All providers",
                            )
                            .changed();
                        if !self.report_prefs.active_patients.all_providers {
                            let selected = provider_choices
                                .iter()
                                .find(|p| {
                                    p.provider_num
                                        == self.report_prefs.active_patients.provider_num
                                })
                                .map(|p| p.abbreviation.clone())
                                .unwrap_or_else(|| {
                                    format!("Provider #{}", self.report_prefs.active_patients.provider_num)
                                });
                            egui::ComboBox::from_label("Provider")
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    for provider in &provider_choices {
                                        options_changed |= ui
                                            .selectable_value(
                                                &mut self.report_prefs.active_patients.provider_num,
                                                provider.provider_num,
                                                format!(
                                                    "{} ({})",
                                                    provider.abbreviation,
                                                    provider.display_name()
                                                ),
                                            )
                                            .clicked();
                                    }
                                });
                        }
                        ui.label("One row per patient with a completed procedure in the range. Billing-type, patient-status and clinic filters are not stored by this port.");
                    }
                    ReportKind::ProdInc => {
                        ui.horizontal(|ui| {
                            ui.label("Report type");
                            for mode in ProdIncMode::ALL {
                                options_changed |= ui
                                    .radio_value(
                                        &mut self.report_prefs.prod_inc.mode,
                                        mode,
                                        mode.label(),
                                    )
                                    .clicked();
                            }
                        });
                        options_changed |= ui
                            .checkbox(
                                &mut self.report_prefs.prod_inc.include_unearned,
                                "Include Unearned Pt Income",
                            )
                            .changed();
                        if insurance {
                            ui.label("Insurance income and write-offs are shown because Insurance & claims is on.");
                        } else {
                            ui.label("Insurance income and write-off columns are hidden while Insurance & claims is off.");
                        }
                        ui.label("Daily lists each transaction; Monthly buckets Sched by scheduled appointment date and separates Unearned Pt Income; Annual aggregates by month; Provider totals per provider. Unearned types whose definition has a nonblank ItemValue are hidden, as in OD.");
                    }
                    ReportKind::PaySheet => {
                        ui.horizontal(|ui| {
                            ui.label("Group by");
                            for group in PaySheetGroupBy::ALL {
                                options_changed |= ui
                                    .radio_value(
                                        &mut self.report_prefs.pay_sheet.group_by,
                                        group,
                                        group.label(),
                                    )
                                    .clicked();
                            }
                        });
                        options_changed |= ui
                            .checkbox(
                                &mut self.report_prefs.pay_sheet.show_splits_by_provider,
                                "Show splits by provider separately",
                            )
                            .changed();
                        options_changed |= ui
                            .checkbox(
                                &mut self.report_prefs.pay_sheet.all_providers,
                                "All providers",
                            )
                            .changed();
                        if !self.report_prefs.pay_sheet.all_providers {
                            let selected = provider_choices
                                .iter()
                                .find(|p| {
                                    p.provider_num == self.report_prefs.pay_sheet.provider_num
                                })
                                .map(|p| p.abbreviation.clone())
                                .unwrap_or_else(|| {
                                    format!("Provider #{}", self.report_prefs.pay_sheet.provider_num)
                                });
                            egui::ComboBox::from_label("Provider")
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    for provider in &provider_choices {
                                        options_changed |= ui
                                            .selectable_value(
                                                &mut self.report_prefs.pay_sheet.provider_num,
                                                provider.provider_num,
                                                format!(
                                                    "{} ({})",
                                                    provider.abbreviation,
                                                    provider.display_name()
                                                ),
                                            )
                                            .clicked();
                                    }
                                });
                        }
                        options_changed |= ui
                            .checkbox(
                                &mut self.report_prefs.pay_sheet.all_payment_types,
                                "All patient payment types",
                            )
                            .changed();
                        if !self.report_prefs.pay_sheet.all_payment_types {
                            let selected = if self.report_prefs.pay_sheet.payment_type_num == 0 {
                                "Income Transfer".to_string()
                            } else {
                                payment_type_choices
                                    .iter()
                                    .find(|d| {
                                        d.def_num == self.report_prefs.pay_sheet.payment_type_num
                                    })
                                    .map(|d| d.item_name.clone())
                                    .unwrap_or_else(|| {
                                        format!(
                                            "Type #{}",
                                            self.report_prefs.pay_sheet.payment_type_num
                                        )
                                    })
                            };
                            egui::ComboBox::from_label("Payment type")
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    options_changed |= ui
                                        .selectable_value(
                                            &mut self.report_prefs.pay_sheet.payment_type_num,
                                            0,
                                            "Income Transfer",
                                        )
                                        .clicked();
                                    for def in &payment_type_choices {
                                        let label = if def.is_hidden {
                                            format!("{} (hidden)", def.item_name)
                                        } else {
                                            def.item_name.clone()
                                        };
                                        options_changed |= ui
                                            .selectable_value(
                                                &mut self.report_prefs.pay_sheet.payment_type_num,
                                                def.def_num,
                                                label,
                                            )
                                            .clicked();
                                    }
                                });
                        }
                        ui.label("Amounts come from paysplits, so one check split across providers appears on one row per provider. Clinics, merchant fees, online payments, unearned prepayments and the insurance-payment section need columns this port does not store.");
                    }
                    ReportKind::AdjSheet => {
                        options_changed |= ui
                            .checkbox(
                                &mut self.report_prefs.adj_sheet.all_providers,
                                "All providers",
                            )
                            .changed();
                        if !self.report_prefs.adj_sheet.all_providers {
                            let selected = provider_choices
                                .iter()
                                .find(|p| {
                                    p.provider_num == self.report_prefs.adj_sheet.provider_num
                                })
                                .map(|p| p.abbreviation.clone())
                                .unwrap_or_else(|| {
                                    format!("Provider #{}", self.report_prefs.adj_sheet.provider_num)
                                });
                            egui::ComboBox::from_label("Provider")
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    for provider in &provider_choices {
                                        options_changed |= ui
                                            .selectable_value(
                                                &mut self.report_prefs.adj_sheet.provider_num,
                                                provider.provider_num,
                                                format!(
                                                    "{} ({})",
                                                    provider.abbreviation,
                                                    provider.display_name()
                                                ),
                                            )
                                            .clicked();
                                    }
                                });
                        }
                        options_changed |= ui
                            .checkbox(&mut self.report_prefs.adj_sheet.all_types, "All adjustment types")
                            .changed();
                        if !self.report_prefs.adj_sheet.all_types {
                            let selected = adj_type_choices
                                .iter()
                                .find(|d| d.def_num == self.report_prefs.adj_sheet.type_num)
                                .map(|d| d.item_name.clone())
                                .unwrap_or_else(|| {
                                    format!("Type #{}", self.report_prefs.adj_sheet.type_num)
                                });
                            egui::ComboBox::from_label("Adjustment type")
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    for def in &adj_type_choices {
                                        options_changed |= ui
                                            .selectable_value(
                                                &mut self.report_prefs.adj_sheet.type_num,
                                                def.def_num,
                                                def.item_name.clone(),
                                            )
                                            .clicked();
                                    }
                                });
                        }
                        ui.label("One flat row per adjustment, sorted by date, with the stored signed amount. Clinics are omitted (this port has none). \"All\" includes hidden adjustment types even though the picker lists only visible ones.");
                    }
                    ReportKind::FinanceCharge => {
                        options_changed |= ui
                            .checkbox(
                                &mut self.report_prefs.finance_charge.all_providers,
                                "All providers",
                            )
                            .changed();
                        if !self.report_prefs.finance_charge.all_providers {
                            let selected = provider_choices
                                .iter()
                                .find(|p| {
                                    p.provider_num == self.report_prefs.finance_charge.provider_num
                                })
                                .map(|p| p.abbreviation.clone())
                                .unwrap_or_else(|| {
                                    format!(
                                        "Provider #{}",
                                        self.report_prefs.finance_charge.provider_num
                                    )
                                });
                            egui::ComboBox::from_label("Primary provider")
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    for provider in &provider_choices {
                                        options_changed |= ui
                                            .selectable_value(
                                                &mut self.report_prefs.finance_charge.provider_num,
                                                provider.provider_num,
                                                format!(
                                                    "{} ({})",
                                                    provider.abbreviation,
                                                    provider.display_name()
                                                ),
                                            )
                                            .clicked();
                                    }
                                });
                        }
                        let selected_type = if self.report_prefs.finance_charge.adjustment_type_num == 0
                        {
                            "All adjustment types".to_string()
                        } else {
                            all_adj_type_choices
                                .iter()
                                .find(|d| {
                                    d.def_num
                                        == self.report_prefs.finance_charge.adjustment_type_num
                                })
                                .map(|d| {
                                    if d.is_hidden {
                                        format!("{} (hidden)", d.item_name)
                                    } else {
                                        d.item_name.clone()
                                    }
                                })
                                .unwrap_or_else(|| {
                                    format!(
                                        "Type #{}",
                                        self.report_prefs.finance_charge.adjustment_type_num
                                    )
                                })
                        };
                        egui::ComboBox::from_label("Finance-charge adjustment type")
                            .selected_text(selected_type)
                            .show_ui(ui, |ui| {
                                options_changed |= ui
                                    .selectable_value(
                                        &mut self.report_prefs.finance_charge.adjustment_type_num,
                                        0,
                                        "All adjustment types",
                                    )
                                    .clicked();
                                for def in &all_adj_type_choices {
                                    let label = if def.is_hidden {
                                        format!("{} (hidden)", def.item_name)
                                    } else {
                                        def.item_name.clone()
                                    };
                                    options_changed |= ui
                                        .selectable_value(
                                            &mut self.report_prefs.finance_charge.adjustment_type_num,
                                            def.def_num,
                                            label,
                                        )
                                        .clicked();
                                }
                            });
                        ui.label("Finance-charge adjustments in the date range with patient number, name and signed amount, sorted by patient name then amount. OD reads the charge type from a practice preference; here \"All\" is the default so the report works before a dedicated type exists. The provider filter matches the patient's primary provider.");
                        ui.label("Preferred Name is blank and the billing-type filter is omitted: this port does not store patient nicknames or billing types.");
                    }
                    ReportKind::BrokenAppointments => {
                        let opts = &mut self.report_prefs.broken_appointments;
                        ui.label("Mode (as in FormRpBrokenAppointments):");
                        for (mode, label) in [
                            (BrokenAppointmentMode::ByAppointmentStatus, "By appointment status"),
                            (BrokenAppointmentMode::ByAdjustments, "By broken-appointment adjustment"),
                            (BrokenAppointmentMode::ByProcedures, "By ADA code (D9986/D9987)"),
                        ] {
                            options_changed |= ui.radio_value(&mut opts.mode, mode, label).clicked();
                        }
                        options_changed |= ui.checkbox(&mut opts.all_providers, "All providers").changed();
                        if !opts.all_providers {
                            let selected = provider_choices
                                .iter()
                                .find(|p| p.provider_num == opts.provider_num)
                                .map(|p| p.abbreviation.clone())
                                .unwrap_or_else(|| format!("Provider #{}", opts.provider_num));
                            egui::ComboBox::from_label("Provider")
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    for provider in &provider_choices {
                                        options_changed |= ui
                                            .selectable_value(
                                                &mut opts.provider_num,
                                                provider.provider_num,
                                                format!("{} ({})", provider.abbreviation, provider.display_name()),
                                            )
                                            .clicked();
                                    }
                                });
                        }
                        if opts.mode == BrokenAppointmentMode::ByProcedures {
                            ui.label("ADA code:");
                            for (option, label) in [
                                (BrokenProcedureOption::Missed, "Missed (D9986)"),
                                (BrokenProcedureOption::Cancelled, "Cancelled (D9987)"),
                                (BrokenProcedureOption::Both, "Both (adds a Code column)"),
                            ] {
                                options_changed |= ui.radio_value(&mut opts.procedure_option, option, label).clicked();
                            }
                        }
                        if opts.mode == BrokenAppointmentMode::ByAdjustments {
                            let selected_type = if opts.adjustment_type_num == 0 {
                                "All adjustment types".to_string()
                            } else {
                                all_adj_type_choices
                                    .iter()
                                    .find(|d| d.def_num == opts.adjustment_type_num)
                                    .map(|d| d.item_name.clone())
                                    .unwrap_or_else(|| format!("Type #{}", opts.adjustment_type_num))
                            };
                            egui::ComboBox::from_label("Adjustment type")
                                .selected_text(selected_type)
                                .show_ui(ui, |ui| {
                                    options_changed |= ui
                                        .selectable_value(&mut opts.adjustment_type_num, 0, "All adjustment types")
                                        .clicked();
                                    for def in &all_adj_type_choices {
                                        options_changed |= ui
                                            .selectable_value(&mut opts.adjustment_type_num, def.def_num, def.item_name.clone())
                                            .clicked();
                                    }
                                });
                        }
                        ui.label("Clinics are omitted (this port has none). The hygienist and IsHyg columns are blank/False because the port does not store ProvHyg or IsHygieneApt. Upstream reads the mode from practice preferences and offers a multi-select of positive adjustment types; here one type (or All) is selectable.");
                    }
                    ReportKind::Confirmation => {
                        let opts = &mut self.report_prefs.confirmation;
                        if field_selection_ui(
                            ui,
                            &mut opts.fields,
                            CONFIRMATION_STANDARD_FIELDS,
                            CONFIRMATION_OTHER_FIELDS,
                        ) {
                            options_changed = true;
                        }
                        ui.label("Mirrors FormRpConfirm: one column per selected field over appointments in the date range. Upstream scopes rows to the confirmation list's selected AptNums; this port uses the range. Unmodelled fields (Preferred, Address2, WkPhone, billing type, notes, insurance carriers, etc.) are blank.");
                    }
                    ReportKind::RecallFields => {
                        let opts = &mut self.report_prefs.recall_fields;
                        if field_selection_ui(
                            ui,
                            &mut opts.fields,
                            RECALL_STANDARD_FIELDS,
                            RECALL_OTHER_FIELDS,
                        ) {
                            options_changed = true;
                        }
                        ui.label("Mirrors FormRpRecall: one column per selected field over recalls due in the date range. RecallInterval is emitted as MonthInterval = ROUND(RecallInterval/65536). Unmodelled patient fields are blank.");
                    }
                    ReportKind::DailyProcedures => {
                        let opts = &mut self.report_prefs.procedure_sheet;
                        ui.horizontal(|ui| {
                            ui.label("Layout");
                            for (mode, label) in [
                                (ProcedureSheetMode::Individual, "Individual"),
                                (ProcedureSheetMode::GroupedByCode, "Grouped by code"),
                            ] {
                                options_changed |= ui.radio_value(&mut opts.mode, mode, label).clicked();
                            }
                        });
                        options_changed |= ui
                            .checkbox(&mut opts.all_providers, "All providers")
                            .changed();
                        if !opts.all_providers {
                            let selected = provider_choices
                                .iter()
                                .find(|p| p.provider_num == opts.provider_num)
                                .map(|p| p.abbreviation.clone())
                                .unwrap_or_else(|| format!("Provider #{}", opts.provider_num));
                            egui::ComboBox::from_label("Provider")
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    for provider in &provider_choices {
                                        options_changed |= ui
                                            .selectable_value(
                                                &mut opts.provider_num,
                                                provider.provider_num,
                                                format!("{} ({})", provider.abbreviation, provider.display_name()),
                                            )
                                            .clicked();
                                    }
                                });
                        }
                        options_changed |= ui
                            .text_edit_singleline(&mut opts.code_filter)
                            .changed();
                        ui.label("Procedure code filter (substring, case-insensitive). Mirrors FormRpProcSheet: Individual lists completed procedures with blank Area (tooth surfaces are not stored); Grouped totals by code with category, quantity, average fee and total fees. Clinics, capitation write-offs, tooth surfaces and unit quantities are omitted.");
                    }
                    ReportKind::ProcedureCodes => {
                        let opts = &mut self.report_prefs.procedure_codes;
                        let schedule_choices = self.store.list_fee_schedules().unwrap_or_default();
                        // Upstream selects the first fee schedule on load. Do
                        // the same once when no schedule has been chosen yet.
                        if opts.fee_schedule_num == 0 {
                            if let Some(first) = schedule_choices.first() {
                                opts.fee_schedule_num = first.fee_sched_num;
                                options_changed = true;
                            }
                        }
                        let selected = schedule_choices
                            .iter()
                            .find(|s| s.fee_sched_num == opts.fee_schedule_num)
                            .map(|s| s.description.clone())
                            .unwrap_or_else(|| format!("Fee schedule #{}", opts.fee_schedule_num));
                        egui::ComboBox::from_label("Fee schedule")
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                for schedule in &schedule_choices {
                                    options_changed |= ui
                                        .selectable_value(
                                            &mut opts.fee_schedule_num,
                                            schedule.fee_sched_num,
                                            schedule.description.clone(),
                                        )
                                        .clicked();
                                }
                            });
                        ui.horizontal(|ui| {
                            ui.label("Layout");
                            options_changed |= ui.radio_value(&mut opts.categories, false, "Code").clicked();
                            options_changed |= ui
                                .radio_value(&mut opts.categories, true, "Categories")
                                .clicked();
                        });
                        options_changed |= ui
                            .checkbox(&mut opts.include_blanks, "Show blank fees")
                            .changed();
                        let selected_provider = if opts.provider_num == 0 {
                            "Default".to_string()
                        } else {
                            provider_choices
                                .iter()
                                .find(|p| p.provider_num == opts.provider_num)
                                .map(|p| p.abbreviation.clone())
                                .unwrap_or_else(|| format!("Provider #{}", opts.provider_num))
                        };
                        egui::ComboBox::from_label("Provider")
                            .selected_text(selected_provider)
                            .show_ui(ui, |ui| {
                                options_changed |= ui
                                    .selectable_value(&mut opts.provider_num, 0, "Default")
                                    .clicked();
                                for provider in &provider_choices {
                                    options_changed |= ui
                                        .selectable_value(
                                            &mut opts.provider_num,
                                            provider.provider_num,
                                            format!("{} ({})", provider.abbreviation, provider.display_name()),
                                        )
                                        .clicked();
                                }
                            });
                        ui.label("Mirrors FormRpProcCodes: the Code layout lists every code with a fee on the chosen schedule; Categories adds the ProcCodeCats name first and orders by category FK then code. Upstream's -1 fee sentinel and missing fees are hidden unless 'Show blank fees' is checked. The clinic selector is fixed to Default because this port has none, and duplicate category names are kept in every row rather than suppressed by the print layout.");
                    }
                    ReportKind::Prescriptions => {
                        let opts = &mut self.report_prefs.prescriptions;
                        ui.horizontal(|ui| {
                            ui.label("Search by");
                            options_changed |= ui
                                .radio_value(&mut opts.search_patient, true, "Patient")
                                .clicked();
                            options_changed |= ui
                                .radio_value(&mut opts.search_patient, false, "Drug")
                                .clicked();
                        });
                        ui.horizontal(|ui| {
                            ui.label("Starts with");
                            options_changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut opts.input_text)
                                        .desired_width(220.0),
                                )
                                .changed();
                        });
                        ui.label("Mirrors FormRpPrescriptions: a case-insensitive prefix filter on the patient's last name or the drug name, with the same OD column set (Patient Name, Date, Drug Name, Directions, Dispense, Prov Name) and ordering. The shared date range is ignored because the original form has no date filter.");
                    }
                    ReportKind::ReferralAnalysis => {
                        let opts = &mut self.report_prefs.referral_analysis;
                        options_changed |= ui
                            .checkbox(&mut opts.include_address, "Include address information")
                            .changed();
                        options_changed |= ui
                            .checkbox(&mut opts.only_new_patients, "Only patients new in this range")
                            .changed();
                        options_changed |= ui.checkbox(&mut opts.all_providers, "All providers").changed();
                        if !opts.all_providers {
                            ui.label("Providers to include");
                            egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                                for provider in &provider_choices {
                                    let mut chosen = opts.provider_nums.contains(&provider.provider_num);
                                    if ui
                                        .checkbox(
                                            &mut chosen,
                                            format!(
                                                "{} ({})",
                                                provider.abbreviation,
                                                provider.display_name()
                                            ),
                                        )
                                        .changed()
                                    {
                                        if chosen {
                                            opts.provider_nums.push(provider.provider_num);
                                        } else {
                                            opts.provider_nums
                                                .retain(|num| *num != provider.provider_num);
                                        }
                                        options_changed = true;
                                    }
                                }
                            });
                        }
                        ui.label("Mirrors FormRpReferralAnalysis: referring sources ranked by distinct RefFrom patients and their completed-procedure production (ProcFee x (UnitQty + BaseUnits)). D9986/D9987 are excluded and the date range is inclusive. 'New patients' uses upstream's MIN(ProcDate) rule. Address adds Title/Address/Add2/City/State/Zip/Specialty from Provider Specialties definitions.");
                    }
                    ReportKind::Referrals => {
                        let OdApp {
                            report_prefs,
                            referrals_filter_field,
                            referrals_filter_condition,
                            referrals_filter_value,
                            referrals_selected_filter,
                            definitions,
                            ..
                        } = self;
                        let opts = &mut report_prefs.referrals;
                        ui.label("SELECT columns (the 19 fields of OD's referral table):");
                        ui.horizontal(|ui| {
                            if ui.button("All").clicked() {
                                opts.fields = REFERRAL_REPORT_FIELDS
                                    .iter()
                                    .map(|s| (*s).to_string())
                                    .collect();
                                options_changed = true;
                            }
                            if ui.button("None").clicked() {
                                opts.fields.clear();
                                options_changed = true;
                            }
                        });
                        egui::ScrollArea::vertical().max_height(130.0).show(ui, |ui| {
                            for field in REFERRAL_REPORT_FIELDS {
                                let mut chosen = opts.fields.iter().any(|f| f == field);
                                if ui.checkbox(&mut chosen, *field).changed() {
                                    if chosen && !opts.fields.iter().any(|f| f == field) {
                                        opts.fields.push((*field).to_string());
                                    } else if !chosen {
                                        opts.fields.retain(|f| f != field);
                                    }
                                    options_changed = true;
                                }
                            }
                        });
                        ui.separator();
                        ui.label("WHERE prerequisites (joined with AND, as upstream does):");
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_label("Field")
                                .selected_text(referrals_filter_field.as_str())
                                .show_ui(ui, |ui| {
                                    for field in REFERRAL_REPORT_FIELDS {
                                        options_changed |= ui
                                            .selectable_value(
                                                referrals_filter_field,
                                                (*field).to_string(),
                                                *field,
                                            )
                                            .clicked();
                                    }
                                });
                            egui::ComboBox::from_label("Condition")
                                .selected_text(referrals_filter_condition.as_str())
                                .show_ui(ui, |ui| {
                                    for condition in [
                                        ReportFilterCondition::Like,
                                        ReportFilterCondition::Eq,
                                        ReportFilterCondition::Gt,
                                        ReportFilterCondition::Lt,
                                        ReportFilterCondition::Gte,
                                        ReportFilterCondition::Lte,
                                        ReportFilterCondition::Ne,
                                    ] {
                                        options_changed |= ui
                                            .selectable_value(
                                                referrals_filter_condition,
                                                condition,
                                                condition.as_str(),
                                            )
                                            .clicked();
                                    }
                                });
                        });
                        match referrals_filter_field.as_str() {
                            "Specialty" => {
                                let choices = sorted_definitions(
                                    definitions,
                                    Some(DefinitionCategory::PROVIDER_SPECIALTIES),
                                    true,
                                );
                                let selected = choices
                                    .iter()
                                    .find(|d| d.def_num.to_string() == *referrals_filter_value)
                                    .map(|d| d.item_name.clone())
                                    .unwrap_or_else(|| "(select a specialty)".into());
                                egui::ComboBox::from_label("Value")
                                    .selected_text(selected)
                                    .show_ui(ui, |ui| {
                                        for def in &choices {
                                            options_changed |= ui
                                                .selectable_value(
                                                    referrals_filter_value,
                                                    def.def_num.to_string(),
                                                    def.item_name.clone(),
                                                )
                                                .clicked();
                                        }
                                    });
                            }
                            "IsHidden" | "NotPerson" | "UsingTIN" => {
                                let selected = if referrals_filter_value == "1" {
                                    "True"
                                } else {
                                    "False"
                                };
                                egui::ComboBox::from_label("Value")
                                    .selected_text(selected)
                                    .show_ui(ui, |ui| {
                                        options_changed |= ui
                                            .selectable_value(
                                                referrals_filter_value,
                                                "0".to_string(),
                                                "False",
                                            )
                                            .clicked();
                                        options_changed |= ui
                                            .selectable_value(
                                                referrals_filter_value,
                                                "1".to_string(),
                                                "True",
                                            )
                                            .clicked();
                                    });
                            }
                            _ => {
                                ui.horizontal(|ui| {
                                    ui.label("Value");
                                    options_changed |= ui.text_edit_singleline(referrals_filter_value).changed();
                                });
                            }
                        }
                        if ui.button("Add / update filter").clicked() {
                            let filter = ReferralFilter {
                                field: referrals_filter_field.clone(),
                                condition: *referrals_filter_condition,
                                value: referrals_filter_value.clone(),
                            };
                            if let Some(index) = *referrals_selected_filter {
                                if let Some(existing) = opts.filters.get_mut(index) {
                                    *existing = filter;
                                }
                            } else {
                                opts.filters.push(filter);
                            }
                            referrals_filter_value.clear();
                            *referrals_selected_filter = None;
                            options_changed = true;
                        }
                        if !opts.filters.is_empty() {
                            ui.separator();
                            egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| {
                                for (index, filter) in opts.filters.iter().enumerate() {
                                    let label = format!(
                                        "{} {} '{}'",
                                        filter.field,
                                        filter.condition.as_str(),
                                        filter.value
                                    );
                                    if ui
                                        .selectable_label(*referrals_selected_filter == Some(index), label)
                                        .clicked()
                                    {
                                        *referrals_selected_filter = Some(index);
                                        *referrals_filter_field = filter.field.clone();
                                        *referrals_filter_condition = filter.condition;
                                        *referrals_filter_value = filter.value.clone();
                                    }
                                }
                            });
                            if ui.button("Delete selected filter").clicked() {
                                if let Some(index) = *referrals_selected_filter {
                                    if index < opts.filters.len() {
                                        opts.filters.remove(index);
                                        options_changed = true;
                                    }
                                    *referrals_selected_filter = None;
                                }
                            }
                        }
                        ui.label("Mirrors FormRpReferrals: select any of the 19 raw referral columns and stack WHERE prerequisites with LIKE, =, >, <, >=, <= or <>. Specialty filters use Provider Specialties definitions and IsHidden/NotPerson/UsingTIN use False/True. The shared report date range is ignored because the original form has no date filter.");
                    }
                    ReportKind::Patients => {
                        let OdApp {
                            report_prefs,
                            patients_filter_field,
                            patients_filter_condition,
                            patients_filter_value,
                            patients_selected_filter,
                            ..
                        } = self;
                        let opts = &mut report_prefs.patients;
                        ui.label("SELECT columns (patient fields supported by this port):");
                        ui.horizontal(|ui| {
                            if ui.button("All").clicked() {
                                opts.fields = PATIENT_REPORT_FIELDS
                                    .iter()
                                    .map(|field| (*field).to_string())
                                    .collect();
                                options_changed = true;
                            }
                            if ui.button("None").clicked() {
                                opts.fields.clear();
                                options_changed = true;
                            }
                        });
                        egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                            for field in PATIENT_REPORT_FIELDS {
                                let mut chosen = opts.fields.iter().any(|f| f == field);
                                if ui.checkbox(&mut chosen, *field).changed() {
                                    if chosen && !opts.fields.iter().any(|f| f == field) {
                                        opts.fields.push((*field).to_string());
                                    } else if !chosen {
                                        opts.fields.retain(|f| f != field);
                                    }
                                    options_changed = true;
                                }
                            }
                        });
                        ui.separator();
                        ui.label("WHERE prerequisites (joined with AND, as upstream does):");
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_label("Field")
                                .selected_text(patients_filter_field.as_str())
                                .show_ui(ui, |ui| {
                                    for field in PATIENT_REPORT_FIELDS {
                                        options_changed |= ui
                                            .selectable_value(
                                                patients_filter_field,
                                                (*field).to_string(),
                                                *field,
                                            )
                                            .clicked();
                                    }
                                });
                            egui::ComboBox::from_label("Condition")
                                .selected_text(patients_filter_condition.as_str())
                                .show_ui(ui, |ui| {
                                    for condition in [
                                        ReportFilterCondition::Like,
                                        ReportFilterCondition::Eq,
                                        ReportFilterCondition::Gt,
                                        ReportFilterCondition::Lt,
                                        ReportFilterCondition::Gte,
                                        ReportFilterCondition::Lte,
                                        ReportFilterCondition::Ne,
                                    ] {
                                        options_changed |= ui
                                            .selectable_value(
                                                patients_filter_condition,
                                                condition,
                                                condition.as_str(),
                                            )
                                            .clicked();
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Value");
                            options_changed |= ui.text_edit_singleline(patients_filter_value).changed();
                        });
                        if ui.button("Add / update filter").clicked() {
                            let filter = ReportFilter {
                                field: patients_filter_field.clone(),
                                condition: *patients_filter_condition,
                                value: patients_filter_value.clone(),
                            };
                            if let Some(index) = *patients_selected_filter {
                                if let Some(existing) = opts.filters.get_mut(index) {
                                    *existing = filter;
                                }
                            } else {
                                opts.filters.push(filter);
                            }
                            patients_filter_value.clear();
                            *patients_selected_filter = None;
                            options_changed = true;
                        }
                        if !opts.filters.is_empty() {
                            ui.separator();
                            egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| {
                                for (index, filter) in opts.filters.iter().enumerate() {
                                    let label = format!(
                                        "{} {} '{}'",
                                        filter.field,
                                        filter.condition.as_str(),
                                        filter.value
                                    );
                                    if ui
                                        .selectable_label(*patients_selected_filter == Some(index), label)
                                        .clicked()
                                    {
                                        *patients_selected_filter = Some(index);
                                        *patients_filter_field = filter.field.clone();
                                        *patients_filter_condition = filter.condition;
                                        *patients_filter_value = filter.value.clone();
                                    }
                                }
                            });
                            if ui.button("Delete selected filter").clicked() {
                                if let Some(index) = *patients_selected_filter {
                                    if index < opts.filters.len() {
                                        opts.filters.remove(index);
                                        options_changed = true;
                                    }
                                    *patients_selected_filter = None;
                                }
                            }
                        }
                        ui.label("Mirrors FormRpPatients: pick any supported patient column and stack WHERE prerequisites with LIKE, =, >, <, >=, <= or <>. First/Last Complete Proc mirror upstream's MIN/MAX(ProcDate) subqueries for completed procedures. Insurance and aging-balance columns, carrier filters and fields absent from this port's patient model are not offered. The shared report date range is ignored because the original form has no date filter.");
                    }
                    ReportKind::CustomAging => {
                        let mut clinic_choices: Vec<Id> = self
                            .patients
                            .iter()
                            .map(|patient| patient.clinic_num)
                            .collect();
                        clinic_choices.sort_unstable();
                        clinic_choices.dedup();
                        let mut bill_type_choices: Vec<Id> = self
                            .definitions
                            .iter()
                            .filter(|def| def.category == DefinitionCategory::BILLING_TYPES)
                            .map(|def| def.def_num)
                            .collect();
                        bill_type_choices.sort_unstable();
                        bill_type_choices.dedup();
                        let opts = &mut self.report_prefs.custom_aging;
                        ui.label("Include the following transaction types:");
                        options_changed |= ui
                            .checkbox(&mut opts.include.procedure_fees, "Procedure Fees")
                            .changed();
                        options_changed |= ui
                            .checkbox(&mut opts.include.adjustments, "Adjustments")
                            .changed();
                        options_changed |= ui
                            .checkbox(&mut opts.include.pay_plan_charges, "Pay Plan Charges")
                            .changed();
                        options_changed |= ui
                            .checkbox(
                                &mut opts.include.pay_plan_production,
                                "Pay Plan Production (credits)",
                            )
                            .changed();
                        options_changed |= ui
                            .checkbox(&mut opts.include.patient_payments, "Patient Payments")
                            .changed();
                        if insurance {
                            options_changed |= ui
                                .checkbox(
                                    &mut opts.include.insurance_payments,
                                    "Insurance Payments",
                                )
                                .changed();
                            options_changed |= ui
                                .checkbox(
                                    &mut opts.include.insurance_estimates,
                                    "Insurance Estimates",
                                )
                                .changed();
                            options_changed |= ui
                                .checkbox(&mut opts.include.write_offs, "Write-offs")
                                .changed();
                            options_changed |= ui
                                .checkbox(
                                    &mut opts.include.write_off_estimates,
                                    "Write-off Estimates",
                                )
                                .changed();
                        }
                        if !opts.include.any() {
                            ui.label("Select at least one transaction type before running the report.");
                        }
                        ui.horizontal(|ui| {
                            ui.label("Group by");
                            for grouping in AgingFamilyGrouping::ALL {
                                options_changed |= ui
                                    .radio_value(
                                        &mut opts.family_grouping,
                                        grouping,
                                        grouping.label(),
                                    )
                                    .clicked();
                            }
                        });
                        if insurance && opts.include.write_offs {
                            ui.horizontal(|ui| {
                                ui.label("Age write-offs by");
                                for mode in CustomAgingWriteoffDate::ALL {
                                    options_changed |= ui
                                        .radio_value(&mut opts.writeoff_date, mode, mode.label())
                                        .clicked();
                                }
                            });
                        }
                        ui.label("Billing types (none selected = all):");
                        ui.horizontal_wrapped(|ui| {
                            for bill_type in &bill_type_choices {
                                let mut chosen = opts.bill_type_nums.contains(bill_type);
                                if ui
                                    .checkbox(&mut chosen, format!("Type #{bill_type}"))
                                    .changed()
                                {
                                    if chosen {
                                        opts.bill_type_nums.push(*bill_type);
                                    } else {
                                        opts.bill_type_nums.retain(|num| num != bill_type);
                                    }
                                    options_changed = true;
                                }
                            }
                        });
                        options_changed |= ui
                            .checkbox(&mut opts.exclude_inactive, "Exclude inactive patients")
                            .changed();
                        options_changed |= ui
                            .checkbox(&mut opts.exclude_archive, "Exclude archived patients")
                            .changed();
                        ui.horizontal(|ui| {
                            ui.label("Age of Account");
                            for age in AgingAgeFilter::ALL {
                                options_changed |= ui.radio_value(&mut opts.age, age, age.label()).clicked();
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Negative Balances");
                            for negative in CustomAgingNegative::ALL {
                                options_changed |= ui
                                    .radio_value(&mut opts.negative, negative, negative.label())
                                    .clicked();
                            }
                        });
                        options_changed |= ui
                            .checkbox(&mut opts.age_credits, "Age Credits (keep credits in their own bucket)")
                            .changed();
                        options_changed |= ui
                            .checkbox(&mut opts.exclude_bad_address, "Exclude bad addresses (blank Zip)")
                            .changed();
                        options_changed |= ui
                            .checkbox(&mut opts.all_providers, "All providers")
                            .changed();
                        if !opts.all_providers {
                            ui.label("Providers to include");
                            egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                                for provider in &provider_choices {
                                    let mut chosen = opts.provider_nums.contains(&provider.provider_num);
                                    if ui
                                        .checkbox(
                                            &mut chosen,
                                            format!(
                                                "{} ({})",
                                                provider.abbreviation,
                                                provider.display_name()
                                            ),
                                        )
                                        .changed()
                                    {
                                        if chosen {
                                            opts.provider_nums.push(provider.provider_num);
                                        } else {
                                            opts.provider_nums.retain(|num| *num != provider.provider_num);
                                        }
                                        options_changed = true;
                                    }
                                }
                            });
                        }
                        options_changed |= ui.checkbox(&mut opts.all_clinics, "All clinics").changed();
                        if !opts.all_clinics {
                            ui.horizontal_wrapped(|ui| {
                                for clinic in &clinic_choices {
                                    let mut chosen = opts.clinic_nums.contains(clinic);
                                    if ui.checkbox(&mut chosen, format!("Clinic #{clinic}")).changed() {
                                        if chosen {
                                            opts.clinic_nums.push(*clinic);
                                        } else {
                                            opts.clinic_nums.retain(|num| num != clinic);
                                        }
                                        options_changed = true;
                                    }
                                }
                            });
                        }
                        ui.label("Mirrors FormRpCustomAging: procedure fees use OD's ProcFee x (UnitQty + BaseUnits). Provider, clinic, billing-type and patient-status filters apply to the guarantor row after grouping. Family grouping aggregates all members under Guarantor. Insurance transactions use DateSuppReceived as the port has no claimproc.DateCP; claim-payment-date write-offs use claimsnapshot with -1 nulled to 0. Insurance options are hidden while Insurance & claims is off.");
                    }
                    ReportKind::BalanceSheet
                    | ReportKind::GeneralLedger
                    | ReportKind::ProfitLoss => {
                        let opts = &mut self.report_prefs.accounting;
                        if current == ReportKind::BalanceSheet {
                            options_changed |= ui
                                .checkbox(
                                    &mut opts.include_net_income,
                                    "Include current-year net income as equity",
                                )
                                .changed();
                        }
                        if current == ReportKind::GeneralLedger {
                            options_changed |= ui
                                .checkbox(
                                    &mut opts.show_opening_balances,
                                    "Show opening balances",
                                )
                                .changed();
                        }
                        ui.label("Mirrors the OD accounting engine: Asset/Expense are debit-positive and Liability/Equity/Income are credit-positive. Dates come from the shared report range (Balance Sheet uses its end date). Retained earnings are derived from prior-year income/expense, never stored.");
                    }
                    ReportKind::HiddenPaySplits => {
                        let opts = &mut self.report_prefs.hidden_pay_splits;
                        options_changed |= ui
                            .checkbox(&mut opts.exclude_refunds, "Exclude refunds (negative splits)")
                            .changed();
                        ui.label("Hidden types are definition category 29 rows with a non-empty ItemValue, matching OD — not Definition.IsHidden.");
                    }
                    ReportKind::UnearnedIncome => {
                        let opts = &mut self.report_prefs.unearned_income;
                        egui::ComboBox::from_label("Tab")
                            .selected_text(opts.tab.label())
                            .show_ui(ui, |ui| {
                                for tab in UnearnedIncomeTab::ALL {
                                    options_changed |= ui
                                        .selectable_value(&mut opts.tab, tab, tab.label())
                                        .changed();
                                }
                            });
                        if opts.tab == UnearnedIncomeTab::Allocation {
                            options_changed |= ui
                                .checkbox(
                                    &mut opts.exclude_zero_allocation,
                                    "Exclude zero allocations",
                                )
                                .changed();
                        }
                        if opts.tab == UnearnedIncomeTab::Net {
                            options_changed |= ui
                                .checkbox(&mut opts.exclude_zero_net, "Exclude zero net")
                                .changed();
                        }
                        if matches!(
                            opts.tab,
                            UnearnedIncomeTab::LineItem | UnearnedIncomeTab::Allocation
                        ) {
                            options_changed |= ui
                                .checkbox(&mut opts.show_provider, "Show provider")
                                .changed();
                        }
                    }
                    ReportKind::ProcOverpaid => {
                        let opts = &mut self.report_prefs.proc_overpaid;
                        options_changed |= ui
                            .checkbox(
                                &mut opts.exclude_without_insurance,
                                "Only overpayments involving insurance",
                            )
                            .changed();
                    }
                    ReportKind::ProcNote => {
                        let opts = &mut self.report_prefs.proc_note;
                        egui::ComboBox::from_label("Group by")
                            .selected_text(opts.group_by.label())
                            .show_ui(ui, |ui| {
                                for group in ProcNoteGroup::ALL {
                                    options_changed |= ui
                                        .selectable_value(&mut opts.group_by, group, group.label())
                                        .changed();
                                }
                            });
                        options_changed |= ui
                            .checkbox(&mut opts.include_no_notes, "Add patients with no note")
                            .changed();
                        options_changed |= ui
                            .checkbox(&mut opts.include_unsigned, "Add unsigned notes")
                            .changed();
                        options_changed |= ui
                            .checkbox(
                                &mut opts.show_excluded_codes,
                                "Show D9986 / D9987 broken codes",
                            )
                            .changed();
                    }
                    ReportKind::DentalSealant => {
                        ui.label(format!(
                            "Reporting year: {} (taken from the range end date).",
                            self.report_to.year()
                        ));
                    }
                    ReportKind::DiscountPlan => {
                        ui.horizontal(|ui| {
                            ui.label("Description contains");
                            ui.text_edit_singleline(&mut self.discount_report_filter);
                        });
                        ui.label("Plans and patient subscriptions are editable in Setup → Discount plans. Blank term dates mean open ended, matching OD.");
                    }
                    ReportKind::TreatmentFinder => {
                        {
                            let opts = &mut self.report_prefs.treatment_finder;
                            options_changed |= ui
                                .checkbox(
                                    &mut opts.pats_with_appts,
                                    "Include patients with a future appointment",
                                )
                                .changed();
                            options_changed |= ui
                                .checkbox(
                                    &mut opts.use_treating_provider,
                                    "Filter on treating provider (not the patient's primary)",
                                )
                                .changed();
                            ui.horizontal(|ui| {
                                ui.label("Provider (0 = all)");
                                options_changed |= ui
                                    .add(egui::DragValue::new(&mut opts.provider_num).speed(1))
                                    .changed();
                                ui.label("Billing type (0 = all)");
                                options_changed |= ui
                                    .add(egui::DragValue::new(&mut opts.billing_type_num).speed(1))
                                    .changed();
                                ui.label("Clinic (0 = all)");
                                options_changed |= ui
                                    .add(egui::DragValue::new(&mut opts.clinic_num).speed(1))
                                    .changed();
                            });
                            ui.horizontal(|ui| {
                                ui.label("Code from");
                                options_changed |= ui.text_edit_singleline(&mut opts.code1).changed();
                                ui.label("to");
                                options_changed |= ui.text_edit_singleline(&mut opts.code2).changed();
                            });
                            ui.horizontal(|ui| {
                                ui.label("Min benefit remaining (above amount)");
                                options_changed |= ui
                                    .add(egui::DragValue::new(&mut opts.above_amount).speed(1.0))
                                    .changed();
                            });
                        }
                        ui.label("Planned procedures in the date range feed the list; patients with a future appointment are left off by default. Contact detail comes from each patient's email or phone. Insurance columns appear only when Insurance & claims is enabled.");
                    }
                    ReportKind::PhRawPop | ReportKind::PhRawScreen => {
                        ui.label("Read-only OD support tables (screening records, sites and counties) feed these extracts; they cannot be edited here.");
                    }
                    ReportKind::ProdGoal => {
                        ui.label("The goal column comes from open provider schedule blocks × provider.HourlyProdGoalAmt; other columns follow upstream's formulas.");
                    }
                    ReportKind::LaserLabels => {
                        egui::ComboBox::from_label("Label source")
                            .selected_text(self.report_prefs.laser_labels.tab.label())
                            .show_ui(ui, |ui| {
                                for tab in LaserLabelTab::ALL {
                                    options_changed |= ui
                                        .selectable_value(
                                            &mut self.report_prefs.laser_labels.tab,
                                            tab,
                                            tab.label(),
                                        )
                                        .changed();
                                }
                            });
                        if self.report_prefs.laser_labels.tab == LaserLabelTab::Insurance {
                            egui::ComboBox::from_label("Carrier selection")
                                .selected_text(self.report_prefs.laser_labels.ins_mode.label())
                                .show_ui(ui, |ui| {
                                    for mode in LaserInsMode::ALL {
                                        options_changed |= ui
                                            .selectable_value(
                                                &mut self.report_prefs.laser_labels.ins_mode,
                                                mode,
                                                mode.label(),
                                            )
                                            .changed();
                                    }
                                });
                            ui.horizontal(|ui| {
                                ui.label("Start carrier");
                                options_changed |= ui
                                    .text_edit_singleline(
                                        &mut self.report_prefs.laser_labels.start_name,
                                    )
                                    .changed();
                                ui.label("address");
                                options_changed |= ui
                                    .text_edit_singleline(
                                        &mut self.report_prefs.laser_labels.start_addr,
                                    )
                                    .changed();
                            });
                            if self.report_prefs.laser_labels.ins_mode == LaserInsMode::Range {
                                ui.horizontal(|ui| {
                                    ui.label("End carrier  ");
                                    options_changed |= ui
                                        .text_edit_singleline(
                                            &mut self.report_prefs.laser_labels.end_name,
                                        )
                                        .changed();
                                    ui.label("address");
                                    options_changed |= ui
                                        .text_edit_singleline(
                                            &mut self.report_prefs.laser_labels.end_addr,
                                        )
                                        .changed();
                                });
                            } else {
                                ui.horizontal(|ui| {
                                    ui.label("Copies");
                                    options_changed |= ui
                                        .add(
                                            egui::DragValue::new(
                                                &mut self.report_prefs.laser_labels.ins_copies,
                                            )
                                            .range(1..=100),
                                        )
                                        .changed();
                                });
                            }
                        }
                    }
                    ReportKind::Routing | ReportKind::Statement => {
                        ui.label("OD lays this report out with absolute sheet positions; the port exports the data fields as a grid and uses the simplified PDF renderer for print.");
                    }
                    ReportKind::PrintPreview => {
                        ui.label("FormRpPrintPreview is a debug-only print host with no report query; this entry point exists only for upstream parity.");
                    }
                    _ => {}
                }
            });
            if options_changed {
                if let Err(err) = self.save_report_prefs() {
                    self.error = Some(format!("Could not save report options: {err}"));
                }
            }
        }
        ui.horizontal(|ui| {
            ui.label("Practice on exports");
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.report_prefs.practice_name).desired_width(260.0),
            );
            if response.changed() {
                if let Err(err) = self.save_report_prefs() {
                    self.error = Some(format!("Could not save report preferences: {err}"));
                }
            }
        });
        ui.separator();

        let mut rows: Vec<ReportRow> = Vec::new();
        let mut upstream_table: Option<ReportTable> = None;
        let patients = self.patients.clone();
        let providers = self.providers.clone();
        let from_dt = self.report_from.and_hms_opt(0, 0, 0).unwrap_or_default();
        let to_dt = (self.report_to + Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .unwrap_or_default();
        match current {
            ReportKind::PatientList => {
                for p in &patients {
                    rows.push(ReportRow::new(
                        p.pat_num.to_string(),
                        p.display_name(),
                        format!("{} {}", p.city, p.state).trim().to_string(),
                    ));
                }
            }
            ReportKind::Production => {
                let mut all_procs = Vec::new();
                for p in &patients {
                    all_procs.extend(self.store.list_procedures(p.pat_num).unwrap_or_default());
                }
                for (provider_num, amount) in
                    production_by_provider(&all_procs, self.report_from, self.report_to)
                {
                    let name = providers
                        .iter()
                        .find(|p| p.provider_num == provider_num)
                        .map(|p| p.display_name())
                        .unwrap_or_else(|| format!("Provider #{provider_num}"));
                    rows.push(ReportRow::new(
                        name,
                        format!("{amount:.2}"),
                        "Completed procedures in range",
                    ));
                }
                if rows.is_empty() {
                    rows.push(ReportRow::new(
                        "No production in range",
                        "0.00",
                        String::new(),
                    ));
                }
            }
            ReportKind::Collections => {
                // One row per individual patient with an outstanding balance.
                // The report's end date is the "as of" date, matching the other
                // balance reports; the start date is not used for aging.
                let procedures = gather_procedures(&*self.store, &patients);
                let adjustments = gather_adjustments(&*self.store, &patients);
                let splits = gather_splits(&*self.store, &patients);
                let plans = gather_pay_plans(&*self.store, &patients);
                let plan_charges = gather_pay_plan_charges(&*self.store, &patients);
                let worklist = collections_worklist(
                    &patients,
                    &procedures,
                    &adjustments,
                    &splits,
                    &plans,
                    &plan_charges,
                    self.report_to,
                );
                let mut table = ReportTable::new(&[
                    "Patient",
                    "Balance Due",
                    "Days Overdue",
                    "Last Payment",
                    "Plan Due Now",
                    "Phone",
                ]);
                for row in &worklist {
                    let values = row.as_table_row();
                    table.rows.push(values);
                }
                if worklist.is_empty() {
                    table.push(&["No overdue patient accounts", "", "", "", "", ""]);
                }
                self.collections_rows = worklist;
                upstream_table = Some(table);
            }
            ReportKind::Schedule => {
                let appts = self.store.list_appointments(from_dt, to_dt).unwrap_or_default();
                for apt in &appts {
                    rows.push(ReportRow::new(
                        patient_name(&patients, apt.pat_num),
                        format!(
                            "{} {}",
                            apt.apt_datetime.format("%Y-%m-%d %H:%M"),
                            apt.apt_status.label()
                        ),
                        apt.notes.clone(),
                    ));
                }
                if rows.is_empty() {
                    rows.push(ReportRow::new(
                        "No appointments in range",
                        String::new(),
                        String::new(),
                    ));
                }
            }
            ReportKind::RecallAsap => {
                let today = chrono::Local::now().date_naive();
                let mut recalls = recall_report(&self.recalls, &patients, today);
                for row in &mut recalls {
                    row.detail = format!("Recall · {}", row.detail);
                }
                let mut asap = asap_report(&self.appointments, &patients, from_dt, to_dt);
                for row in &mut asap {
                    row.detail = format!("ASAP · {}", row.detail);
                }
                rows.push(ReportRow::new(
                    "Recall / ASAP totals",
                    format!("{} recall(s), {} ASAP", recalls.len(), asap.len()),
                    format!("As of {today}"),
                ));
                rows.extend(recalls);
                rows.extend(asap);
            }
            ReportKind::PaymentPlans => {
                let mut plans = Vec::new();
                let mut charges = Vec::new();
                let mut seen_splits = HashSet::new();
                let mut splits = Vec::new();
                let mut seen_links = HashSet::new();
                let mut links = Vec::new();
                let mut procedures = Vec::new();
                let mut adjustments = Vec::new();
                for l in self.store.list_pay_plan_links().unwrap_or_default() {
                    if seen_links.insert(l.pay_plan_link_num) {
                        links.push(l);
                    }
                }
                for patient in &patients {
                    plans.extend(self.store.list_pay_plans(patient.pat_num).unwrap_or_default());
                    charges.extend(
                        self.store
                            .list_pay_plan_charges(patient.pat_num)
                            .unwrap_or_default(),
                    );
                    procedures.extend(self.store.list_procedures(patient.pat_num).unwrap_or_default());
                    adjustments.extend(self.store.list_adjustments(patient.pat_num).unwrap_or_default());
                    for split in self.store.list_pay_splits(patient.pat_num).unwrap_or_default() {
                        if seen_splits.insert(split.split_num) {
                            splits.push(split);
                        }
                    }
                }
                rows.extend(payment_plan_report(
                    &plans,
                    &charges,
                    &splits,
                    &patients,
                    &links,
                    &procedures,
                    &adjustments,
                    self.report_to,
                ));
            }
            ReportKind::TreatmentPlans => {
                let mut plans = Vec::new();
                let mut lines = Vec::new();
                for patient in &patients {
                    for plan in self.store.list_treat_plans(patient.pat_num).unwrap_or_default() {
                        lines.extend(
                            self.store
                                .list_treat_plan_lines(plan.treat_plan_num)
                                .unwrap_or_default(),
                        );
                        plans.push(plan);
                    }
                }
                rows.extend(treatment_plan_report(&plans, &lines, &patients));
            }
            ReportKind::Aging => {
                let insurance = self.features.enabled(FeatureKey::InsuranceEnabled);
                let mut procedures = Vec::new();
                let mut adjustments = Vec::new();
                let mut splits = Vec::new();
                let mut claim_procs = Vec::new();
                for patient in &patients {
                    procedures.extend(
                        self.store.list_procedures(patient.pat_num).unwrap_or_default(),
                    );
                    adjustments.extend(
                        self.store.list_adjustments(patient.pat_num).unwrap_or_default(),
                    );
                    splits.extend(
                        self.store.list_pay_splits(patient.pat_num).unwrap_or_default(),
                    );
                    if insurance {
                        claim_procs.extend(
                            self.store
                                .list_claim_procs(patient.pat_num)
                                .unwrap_or_default(),
                        );
                    }
                }
                upstream_table = Some(aging_report(
                    &patients,
                    &procedures,
                    &adjustments,
                    &splits,
                    &claim_procs,
                    self.report_to,
                    insurance,
                    self.report_prefs.aging,
                ));
            }
            ReportKind::CustomAging => {
                let insurance = self.features.enabled(FeatureKey::InsuranceEnabled);
                let mut procedures = Vec::new();
                let mut adjustments = Vec::new();
                let mut splits = Vec::new();
                let mut plan_charges = Vec::new();
                let mut claim_procs = Vec::new();
                let claim_snapshots = if insurance {
                    self.store.list_claim_snapshots().unwrap_or_default()
                } else {
                    Vec::new()
                };
                for patient in &patients {
                    procedures.extend(
                        self.store.list_procedures(patient.pat_num).unwrap_or_default(),
                    );
                    adjustments.extend(
                        self.store.list_adjustments(patient.pat_num).unwrap_or_default(),
                    );
                    splits.extend(
                        self.store.list_pay_splits(patient.pat_num).unwrap_or_default(),
                    );
                    plan_charges.extend(
                        self.store
                            .list_pay_plan_charges(patient.pat_num)
                            .unwrap_or_default(),
                    );
                    if insurance {
                        claim_procs.extend(
                            self.store
                                .list_claim_procs(patient.pat_num)
                                .unwrap_or_default(),
                        );
                    }
                }
                upstream_table = Some(custom_aging_report(
                    &patients,
                    &procedures,
                    &adjustments,
                    &splits,
                    &plan_charges,
                    &claim_procs,
                    &claim_snapshots,
                    self.report_to,
                    insurance,
                    self.report_prefs.custom_aging.clone(),
                ));
            }
            ReportKind::PayPlanDetail => {
                let insurance = self.features.enabled(FeatureKey::InsuranceEnabled);
                let mut plans = Vec::new();
                let mut charges = Vec::new();
                let mut seen_splits = HashSet::new();
                let mut splits = Vec::new();
                let mut family_balances = Vec::new();
                let mut all_procedures = Vec::new();
                let mut all_adjustments = Vec::new();
                let plan_links = self.store.list_pay_plan_links().unwrap_or_default();
                for patient in &patients {
                    plans.extend(self.store.list_pay_plans(patient.pat_num).unwrap_or_default());
                    charges.extend(
                        self.store
                            .list_pay_plan_charges(patient.pat_num)
                            .unwrap_or_default(),
                    );
                    let procedures = self
                        .store
                        .list_procedures(patient.pat_num)
                        .unwrap_or_default();
                    let adjustments = self
                        .store
                        .list_adjustments(patient.pat_num)
                        .unwrap_or_default();
                    let patient_splits = self
                        .store
                        .list_pay_splits(patient.pat_num)
                        .unwrap_or_default();
                    let family_balance = if insurance {
                        let claim_procs = self
                            .store
                            .list_claim_procs(patient.pat_num)
                            .unwrap_or_default();
                        account_summary(&procedures, &adjustments, &patient_splits, &claim_procs)
                            .balance
                    } else {
                        account_summary_without_insurance(
                            &procedures,
                            &adjustments,
                            &patient_splits,
                        )
                        .balance
                    };
                    family_balances.push((patient.pat_num, family_balance));
                    for split in patient_splits {
                        if seen_splits.insert(split.split_num) {
                            splits.push(split);
                        }
                    }
                    all_procedures.extend(procedures);
                    all_adjustments.extend(adjustments);
                }
                upstream_table = Some(pay_plan_detail_report(
                    &plans,
                    &charges,
                    &splits,
                    &plan_links,
                    &all_procedures,
                    &all_adjustments,
                    &patients,
                    &providers,
                    &family_balances,
                    self.report_from,
                    self.report_to,
                    insurance,
                    self.report_prefs.pay_plans,
                ));
            }
            ReportKind::PayPlanChargeApplications => {
                let mut plans = Vec::new();
                let mut charges = Vec::new();
                let mut seen_splits = HashSet::new();
                let mut splits = Vec::new();
                let mut payments = Vec::new();
                let mut seen_payments = HashSet::new();
                for patient in &patients {
                    plans.extend(self.store.list_pay_plans(patient.pat_num).unwrap_or_default());
                    charges.extend(
                        self.store
                            .list_pay_plan_charges(patient.pat_num)
                            .unwrap_or_default(),
                    );
                    for split in self
                        .store
                        .list_pay_splits(patient.pat_num)
                        .unwrap_or_default()
                    {
                        if seen_splits.insert(split.split_num) {
                            splits.push(split);
                        }
                    }
                    for payment in self.store.list_payments(patient.pat_num).unwrap_or_default() {
                        if seen_payments.insert(payment.pay_num) {
                            payments.push(payment);
                        }
                    }
                }
                upstream_table = Some(pay_plan_charge_applications_report(
                    &plans,
                    &charges,
                    &splits,
                    &patients,
                    &payments,
                    self.report_from,
                    self.report_to,
                    self.report_prefs
                        .pay_plan_charge_applications
                        .only_refunds_transfers,
                ));
            }
            ReportKind::Birthday => {
                upstream_table = Some(birthday_report(
                    &patients,
                    self.report_from,
                    self.report_to,
                    chrono::Local::now().date_naive(),
                ));
            }
            ReportKind::NewPatients => {
                let mut procedures = Vec::new();
                for patient in &patients {
                    procedures.extend(
                        self.store
                            .list_procedures(patient.pat_num)
                            .unwrap_or_default(),
                    );
                }
                upstream_table = Some(new_patient_report(
                    &patients,
                    &procedures,
                    self.report_from,
                    self.report_to,
                    self.report_prefs.new_patients,
                ));
            }
            ReportKind::Appointments => {
                let appts = self
                    .store
                    .list_appointments(from_dt, to_dt)
                    .unwrap_or_default();
                upstream_table = Some(appointments_report(
                    &appts,
                    &patients,
                    self.report_from,
                    self.report_to,
                    chrono::Local::now().date_naive(),
                    self.report_prefs.appointments,
                ));
            }
            ReportKind::ActivePatients => {
                let mut procedures = Vec::new();
                for patient in &patients {
                    procedures.extend(
                        self.store
                            .list_procedures(patient.pat_num)
                            .unwrap_or_default(),
                    );
                }
                upstream_table = Some(active_patient_report(
                    &patients,
                    &procedures,
                    &providers,
                    self.report_from,
                    self.report_to,
                    self.report_prefs.active_patients,
                ));
            }
            ReportKind::ProdInc => {
                let insurance = self.features.enabled(FeatureKey::InsuranceEnabled);
                let mut procedures = Vec::new();
                let mut adjustments = Vec::new();
                let mut splits = Vec::new();
                let mut claim_procs = Vec::new();
                let mut seen_splits = HashSet::new();
                let appointments = self
                    .store
                    .list_appointments(
                        self.report_from.and_hms_opt(0, 0, 0).unwrap_or_default(),
                        self.report_to.and_hms_opt(23, 59, 59).unwrap_or_default(),
                    )
                    .unwrap_or_default();
                let definitions = self.store.list_definitions().unwrap_or_default();
                for patient in &patients {
                    procedures.extend(
                        self.store
                            .list_procedures(patient.pat_num)
                            .unwrap_or_default(),
                    );
                    adjustments.extend(
                        self.store
                            .list_adjustments(patient.pat_num)
                            .unwrap_or_default(),
                    );
                    for split in self
                        .store
                        .list_pay_splits(patient.pat_num)
                        .unwrap_or_default()
                    {
                        if seen_splits.insert(split.split_num) {
                            splits.push(split);
                        }
                    }
                    if insurance {
                        claim_procs.extend(
                            self.store
                                .list_claim_procs(patient.pat_num)
                                .unwrap_or_default(),
                        );
                    }
                }
                upstream_table = Some(prod_inc_report(
                    &patients,
                    &providers,
                    &procedures,
                    &adjustments,
                    &splits,
                    &claim_procs,
                    &appointments,
                    &definitions,
                    self.report_from,
                    self.report_to,
                    insurance,
                    self.report_prefs.prod_inc,
                ));
            }
            ReportKind::PaySheet => {
                let mut payments = Vec::new();
                let mut splits = Vec::new();
                let mut seen_splits = HashSet::new();
                for patient in &patients {
                    payments.extend(
                        self.store
                            .list_payments(patient.pat_num)
                            .unwrap_or_default(),
                    );
                    for split in self
                        .store
                        .list_pay_splits(patient.pat_num)
                        .unwrap_or_default()
                    {
                        if seen_splits.insert(split.split_num) {
                            splits.push(split);
                        }
                    }
                }
                payments.sort_by(|a, b| a.pay_num.cmp(&b.pay_num));
                payments.dedup_by_key(|payment| payment.pay_num);
                upstream_table = Some(pay_sheet_report(
                    &patients,
                    &providers,
                    &payments,
                    &splits,
                    &self.definitions,
                    self.report_from,
                    self.report_to,
                    self.report_prefs.pay_sheet,
                ));
            }
            ReportKind::AdjSheet => {
                let mut adjustments = Vec::new();
                let mut seen_adjustments = HashSet::new();
                for patient in &patients {
                    for adjustment in self
                        .store
                        .list_adjustments(patient.pat_num)
                        .unwrap_or_default()
                    {
                        if seen_adjustments.insert(adjustment.adj_num) {
                            adjustments.push(adjustment);
                        }
                    }
                }
                adjustments.sort_by(|a, b| a.adj_num.cmp(&b.adj_num));
                upstream_table = Some(adj_sheet_report(
                    &patients,
                    &providers,
                    &adjustments,
                    &self.definitions,
                    self.report_from,
                    self.report_to,
                    self.report_prefs.adj_sheet,
                ));
            }
            ReportKind::FinanceCharge => {
                let mut adjustments = Vec::new();
                let mut seen_adjustments = HashSet::new();
                for patient in &patients {
                    for adjustment in self
                        .store
                        .list_adjustments(patient.pat_num)
                        .unwrap_or_default()
                    {
                        if seen_adjustments.insert(adjustment.adj_num) {
                            adjustments.push(adjustment);
                        }
                    }
                }
                adjustments.sort_by(|a, b| a.adj_num.cmp(&b.adj_num));
                upstream_table = Some(finance_charge_report(
                    &patients,
                    &adjustments,
                    self.report_from,
                    self.report_to,
                    self.report_prefs.finance_charge,
                ));
            }
            ReportKind::BrokenAppointments => {
                let appts = self
                    .store
                    .list_appointments(from_dt, to_dt)
                    .unwrap_or_default();
                let mut adjustments = Vec::new();
                let mut seen_adjustments = HashSet::new();
                let mut procedures = Vec::new();
                for patient in &patients {
                    for adjustment in self
                        .store
                        .list_adjustments(patient.pat_num)
                        .unwrap_or_default()
                    {
                        if seen_adjustments.insert(adjustment.adj_num) {
                            adjustments.push(adjustment);
                        }
                    }
                    procedures.extend(self.store.list_procedures(patient.pat_num).unwrap_or_default());
                }
                let procedure_codes = self.store.list_procedure_codes().unwrap_or_default();
                upstream_table = Some(broken_appointments_report(
                    &patients,
                    &providers,
                    &appts,
                    &adjustments,
                    &procedures,
                    &procedure_codes,
                    self.report_from,
                    self.report_to,
                    self.report_prefs.broken_appointments,
                ));
            }
            ReportKind::Confirmation => {
                let appts = self
                    .store
                    .list_appointments(from_dt, to_dt)
                    .unwrap_or_default();
                upstream_table = Some(confirmation_report(
                    &patients,
                    &appts,
                    &self.report_prefs.confirmation,
                ));
            }
            ReportKind::RecallFields => {
                upstream_table = Some(recall_fields_report(
                    &patients,
                    &self.recalls,
                    self.report_from,
                    self.report_to,
                    &self.report_prefs.recall_fields,
                ));
            }
            ReportKind::DailyProcedures => {
                let mut procedures = Vec::new();
                for patient in &patients {
                    procedures.extend(
                        self.store
                            .list_procedures(patient.pat_num)
                            .unwrap_or_default(),
                    );
                }
                let procedure_codes = self.store.list_procedure_codes().unwrap_or_default();
                upstream_table = Some(procedure_sheet_report(
                    &patients,
                    &providers,
                    &procedures,
                    &procedure_codes,
                    &self.definitions,
                    self.report_from,
                    self.report_to,
                    &self.report_prefs.procedure_sheet,
                ));
            }
            ReportKind::ProcedureCodes => {
                let procedure_codes = self.store.list_procedure_codes().unwrap_or_default();
                let mut opts = self.report_prefs.procedure_codes.clone();
                let fee_schedules = self.store.list_fee_schedules().unwrap_or_default();
                if opts.fee_schedule_num == 0 {
                    if let Some(first) = fee_schedules.first() {
                        opts.fee_schedule_num = first.fee_sched_num;
                    }
                }
                let fees = self
                    .store
                    .list_fees(opts.fee_schedule_num, opts.provider_num)
                    .unwrap_or_default();
                upstream_table = Some(procedure_codes_report(
                    &procedure_codes,
                    &fees,
                    &self.definitions,
                    &opts,
                ));
            }
            ReportKind::Prescriptions => {
                let prescriptions = self.store.list_prescriptions().unwrap_or_default();
                upstream_table = Some(prescriptions_report(
                    &patients,
                    &providers,
                    &prescriptions,
                    &self.report_prefs.prescriptions,
                ));
            }
            ReportKind::ReferralAnalysis => {
                let mut procedures = Vec::new();
                for patient in &patients {
                    procedures.extend(
                        self.store
                            .list_procedures(patient.pat_num)
                            .unwrap_or_default(),
                    );
                }
                let procedure_codes = self.store.list_procedure_codes().unwrap_or_default();
                let referrals = self.store.list_referrals().unwrap_or_default();
                let refattaches = self.store.list_refattaches().unwrap_or_default();
                upstream_table = Some(referral_analysis_report(
                    &procedures,
                    &procedure_codes,
                    &referrals,
                    &refattaches,
                    &self.definitions,
                    self.report_from,
                    self.report_to,
                    &self.report_prefs.referral_analysis,
                ));
            }
            ReportKind::Referrals => {
                let referrals = self.store.list_referrals().unwrap_or_default();
                upstream_table = Some(referrals_report(
                    &referrals,
                    &self.report_prefs.referrals,
                ));
            }
            ReportKind::Patients => {
                let mut all_procs = Vec::new();
                for patient in &patients {
                    all_procs.extend(self.store.list_procedures(patient.pat_num).unwrap_or_default());
                }
                upstream_table = Some(patients_report(
                    &patients,
                    &all_procs,
                    &self.recalls,
                    &self.report_prefs.patients,
                ));
            }
            ReportKind::BalanceSheet => {
                let accounts = self.store.list_accounts().unwrap_or_default();
                let journal = self.store.list_journal_entries().unwrap_or_default();
                upstream_table = Some(balance_sheet_report(
                    &accounts,
                    &journal,
                    self.report_to,
                    self.report_prefs.accounting.include_net_income,
                ));
            }
            ReportKind::GeneralLedger => {
                let accounts = self.store.list_accounts().unwrap_or_default();
                let journal = self.store.list_journal_entries().unwrap_or_default();
                upstream_table = Some(general_ledger_report(
                    &accounts,
                    &journal,
                    self.report_from,
                    self.report_to,
                    self.report_prefs.accounting.show_opening_balances,
                ));
            }
            ReportKind::ProfitLoss => {
                let accounts = self.store.list_accounts().unwrap_or_default();
                let journal = self.store.list_journal_entries().unwrap_or_default();
                upstream_table = Some(profit_loss_report(
                    &accounts,
                    &journal,
                    self.report_from,
                    self.report_to,
                ));
            }
            ReportKind::JournalPostingAudit => {
                let mut all_procs = Vec::new();
                let mut all_payments = Vec::new();
                let mut all_adjustments = Vec::new();
                for patient in &patients {
                    all_procs.extend(self.store.list_procedures(patient.pat_num).unwrap_or_default());
                    all_payments.extend(self.store.list_payments(patient.pat_num).unwrap_or_default());
                    all_adjustments.extend(self.store.list_adjustments(patient.pat_num).unwrap_or_default());
                }
                let journal = self.store.list_journal_entries().unwrap_or_default();
                upstream_table = Some(journal_posting_audit_report(
                    &all_procs,
                    &all_payments,
                    &all_adjustments,
                    &journal,
                    self.report_from,
                    self.report_to,
                ));
            }
            ReportKind::LedgerTieOut => {
                let accounts = self.store.list_accounts().unwrap_or_default();
                let journal = self.store.list_journal_entries().unwrap_or_default();
                let procedures = gather_procedures(&*self.store, &patients);
                let adjustments = gather_adjustments(&*self.store, &patients);
                let splits = gather_splits(&*self.store, &patients);
                let mut payments = Vec::new();
                for patient in &patients {
                    payments.extend(self.store.list_payments(patient.pat_num).unwrap_or_default());
                }
                upstream_table = Some(ledger_tie_out_report(
                    &procedures,
                    &payments,
                    &adjustments,
                    &splits,
                    &journal,
                    &accounts,
                    self.report_from,
                    self.report_to,
                ));
            }
            ReportKind::PeriodCloseHistory => {
                // Sign-offs span months, so read the full log rather than the
                // short Setup view. This report never writes to the store.
                let log = match self.store.list_security_log(1000) {
                    Ok(rows) => rows,
                    Err(_) => self.security_log.clone(),
                };
                let filter = PeriodCloseHistoryFilter {
                    user: self.period_close_history_user_filter.clone(),
                    month: self.period_close_history_month_filter.clone(),
                    action: self.period_close_history_action_filter,
                };
                upstream_table = Some(period_close_history_report_filtered(
                    &log,
                    &self.users,
                    &self.closed_periods,
                    &filter,
                ));
            }
            ReportKind::HiddenPaySplits => {
                let definitions = self.store.list_definitions().unwrap_or_default();
                let splits = gather_splits(&*self.store, &patients);
                upstream_table = Some(hidden_pay_splits_report(
                    &patients,
                    &providers,
                    &definitions,
                    &splits,
                    self.report_from,
                    self.report_to,
                    &self.report_prefs.hidden_pay_splits,
                ));
            }
            ReportKind::UnearnedIncome => {
                let definitions = self.store.list_definitions().unwrap_or_default();
                let procedures = gather_procedures(&*self.store, &patients);
                let splits = gather_splits(&*self.store, &patients);
                upstream_table = Some(unearned_income_report(
                    &patients,
                    &providers,
                    &definitions,
                    &procedures,
                    &splits,
                    self.report_from,
                    self.report_to,
                    &self.report_prefs.unearned_income,
                ));
            }
            ReportKind::ProcOverpaid => {
                let procedures = gather_procedures(&*self.store, &patients);
                let codes = self.store.list_procedure_codes().unwrap_or_default();
                let claim_procs = gather_claim_procs(&*self.store, &patients);
                let splits = gather_splits(&*self.store, &patients);
                upstream_table = Some(proc_overpaid_report(
                    &patients,
                    &procedures,
                    &codes,
                    &claim_procs,
                    &splits,
                    self.report_from,
                    self.report_to,
                    &self.report_prefs.proc_overpaid,
                ));
            }
            ReportKind::ReceivablesBreakdown => {
                let definitions = self.store.list_definitions().unwrap_or_default();
                let procedures = gather_procedures(&*self.store, &patients);
                let splits = gather_splits(&*self.store, &patients);
                let claim_procs = gather_claim_procs(&*self.store, &patients);
                upstream_table = Some(receivables_breakdown_report(
                    &patients,
                    &definitions,
                    &procedures,
                    &splits,
                    &claim_procs,
                    self.report_to,
                    self.features.enabled(FeatureKey::InsuranceEnabled),
                ));
            }
            ReportKind::ServiceDateView => {
                let procedures = gather_procedures(&*self.store, &patients);
                let splits = gather_splits(&*self.store, &patients);
                let claim_procs = gather_claim_procs(&*self.store, &patients);
                upstream_table = Some(service_date_view_report(
                    &patients,
                    &procedures,
                    &splits,
                    &claim_procs,
                    self.report_from,
                    self.report_to,
                ));
            }
            ReportKind::ProcNote => {
                let procedures = gather_procedures(&*self.store, &patients);
                let codes = self.store.list_procedure_codes().unwrap_or_default();
                let notes = self.store.list_proc_notes().unwrap_or_default();
                upstream_table = Some(proc_note_report(
                    &patients,
                    &providers,
                    &procedures,
                    &codes,
                    &notes,
                    self.report_from,
                    self.report_to,
                    &self.report_prefs.proc_note,
                ));
            }
            ReportKind::DentalSealant => {
                let procedures = gather_procedures(&*self.store, &patients);
                let codes = self.store.list_procedure_codes().unwrap_or_default();
                upstream_table = Some(dental_sealant_measure_report(
                    &patients,
                    &providers,
                    &procedures,
                    &codes,
                    self.report_to.year(),
                ));
            }
            ReportKind::DiscountPlan => {
                let subscriptions = gather_discount_subscriptions(&*self.store, &patients);
                upstream_table = Some(discount_plan_report(
                    &subscriptions,
                    &self.discount_report_filter,
                ));
            }
            ReportKind::DppOvercharged => {
                let procedures = gather_procedures(&*self.store, &patients);
                let codes = self.store.list_procedure_codes().unwrap_or_default();
                let adjustments = gather_adjustments(&*self.store, &patients);
                let claim_procs = gather_claim_procs(&*self.store, &patients);
                let links = self.store.list_pay_plan_links().unwrap_or_default();
                let plans = gather_pay_plans(&*self.store, &patients);
                let charges = gather_pay_plan_charges(&*self.store, &patients);
                let splits = gather_splits(&*self.store, &patients);
                upstream_table = Some(dpp_overcharged_report(
                    &patients,
                    &providers,
                    &procedures,
                    &codes,
                    &adjustments,
                    &claim_procs,
                    &links,
                    &plans,
                    &charges,
                    &splits,
                    self.report_from,
                    self.report_to,
                ));
            }
            ReportKind::PatientPortionUncollected => {
                let procedures = gather_procedures(&*self.store, &patients);
                let codes = self.store.list_procedure_codes().unwrap_or_default();
                let adjustments = gather_adjustments(&*self.store, &patients);
                let splits = gather_splits(&*self.store, &patients);
                let claim_procs = gather_claim_procs(&*self.store, &patients);
                let definitions = self.store.list_definitions().unwrap_or_default();
                upstream_table = Some(patient_portion_uncollected_report(
                    &patients,
                    &procedures,
                    &codes,
                    &adjustments,
                    &splits,
                    &claim_procs,
                    &definitions,
                    self.report_from,
                    self.report_to,
                    self.features.enabled(FeatureKey::InsuranceEnabled),
                ));
            }
            ReportKind::TreatmentFinder => {
                let procedures = gather_procedures(&*self.store, &patients);
                let codes = self.store.list_procedure_codes().unwrap_or_default();
                let claim_procs = gather_claim_procs(&*self.store, &patients);
                let carriers = self.store.list_carriers().unwrap_or_default();
                let today = Local::now().date_naive();
                let appt_from = today.and_hms_opt(0, 0, 0).unwrap_or_default();
                let appt_to = (today + Duration::days(3650))
                    .and_hms_opt(0, 0, 0)
                    .unwrap_or_default();
                let appointments = self
                    .store
                    .list_appointments(appt_from, appt_to)
                    .unwrap_or_default();
                // `insplan` is not exposed through Store, so OD's carrier join
                // is unavailable here; the insurance carrier column is blank.
                upstream_table = Some(treatment_finder_report(
                    &patients,
                    &procedures,
                    &codes,
                    &appointments,
                    &claim_procs,
                    &[],
                    &carriers,
                    self.report_from,
                    self.report_to,
                    today,
                    &self.report_prefs.treatment_finder.clone(),
                    self.features.enabled(FeatureKey::InsuranceEnabled),
                ));
            }
            ReportKind::PhRawPop => {
                let procedures = gather_procedures(&*self.store, &patients);
                let rows = gather_public_health_population(
                    &*self.store,
                    &patients,
                    &self.appointments,
                    &procedures,
                    self.report_from,
                    self.report_to,
                );
                upstream_table = Some(ph_raw_population_report(&rows));
            }
            ReportKind::PhRawScreen => {
                let rows = gather_public_health_screens(
                    &*self.store,
                    self.report_from,
                    self.report_to,
                    true,
                );
                upstream_table = Some(ph_raw_screen_report(&rows, true));
            }
            ReportKind::Routing => {
                let procedures = gather_procedures(&*self.store, &patients);
                upstream_table = Some(routing_slips_report(
                    &patients,
                    &providers,
                    &procedures,
                    &self.appointments,
                    self.report_from,
                ));
            }
            ReportKind::LaserLabels => {
                let labels: Vec<LabelAddress> = match self.report_prefs.laser_labels.tab {
                    LaserLabelTab::Insurance => carrier_label_addresses(
                        &self.store.list_carriers().unwrap_or_default(),
                        &self.report_prefs.laser_labels,
                    ),
                    LaserLabelTab::Birthday => patient_label_addresses(
                        &patients,
                        self.report_from.month(),
                        self.report_from.day(),
                    ),
                    LaserLabelTab::Custom | LaserLabelTab::Patient => patients
                        .iter()
                        .map(|p| LabelAddress {
                            line1: format!("{} {}", p.f_name, p.l_name),
                            line2: p.address.clone(),
                            city_state_zip: format!("{}, {}   {}", p.city, p.state, p.zip),
                            copies: 1,
                        })
                        .collect(),
                };
                upstream_table = Some(laser_labels_report(&labels));
            }
            ReportKind::PrintPreview => {
                upstream_table = Some(print_preview_report());
            }
            ReportKind::Statement => {
                let statements = gather_statements(&*self.store, &patients);
                let procedures = gather_procedures(&*self.store, &patients);
                let splits = gather_splits(&*self.store, &patients);
                let adjustments = gather_adjustments(&*self.store, &patients);
                upstream_table = Some(statement_report(
                    &patients,
                    &statements,
                    &procedures,
                    &splits,
                    &adjustments,
                ));
            }
            ReportKind::ProdGoal => {
                let procedures = gather_procedures(&*self.store, &patients);
                let adjustments = gather_adjustments(&*self.store, &patients);
                let claim_procs = gather_claim_procs(&*self.store, &patients);
                let schedules = self.store.list_schedules().unwrap_or_default();
                let schedule_ops = self.store.list_schedule_ops().unwrap_or_default();
                let operatories = self.store.list_operatories().unwrap_or_default();
                upstream_table = Some(production_goal_report(
                    &procedures,
                    &self.appointments,
                    &adjustments,
                    &claim_procs,
                    &schedules,
                    &schedule_ops,
                    &providers,
                    &operatories,
                    self.report_from,
                    self.report_to,
                ));
            }
            ReportKind::ProviderPayroll => {
                let procedures = gather_procedures(&*self.store, &patients);
                let claim_procs = gather_claim_procs(&*self.store, &patients);
                let snapshots = self.store.list_claim_snapshots().unwrap_or_default();
                let adjustments = gather_adjustments(&*self.store, &patients);
                let splits = gather_splits(&*self.store, &patients);
                upstream_table = Some(provider_payroll_report(
                    &patients,
                    &providers,
                    &procedures,
                    &claim_procs,
                    &snapshots,
                    &adjustments,
                    &splits,
                    self.report_from,
                    self.report_to,
                    false,
                ));
            }
            ReportKind::PresentedTreatment => {
                let procedures = gather_procedures(&*self.store, &patients);
                let plans = gather_treat_plans(&*self.store, &patients);
                let lines = gather_treat_plan_lines(&*self.store, &plans);
                let claim_procs = gather_claim_procs(&*self.store, &patients);
                let adjustments = gather_adjustments(&*self.store, &patients);
                upstream_table = Some(presented_treatment_production_report(
                    &patients,
                    &providers,
                    &procedures,
                    &plans,
                    &lines,
                    &claim_procs,
                    &adjustments,
                    self.report_from,
                    self.report_to,
                    false,
                ));
            }
            ReportKind::TpPresentationStats => {
                let plans = gather_treat_plans(&*self.store, &patients);
                let lines = gather_treat_plan_lines(&*self.store, &plans);
                let procedures = gather_procedures(&*self.store, &patients);
                let claim_procs = gather_claim_procs(&*self.store, &patients);
                let adjustments = gather_adjustments(&*self.store, &patients);
                upstream_table = Some(treat_plan_presentation_statistics_report(
                    &providers,
                    &plans,
                    &lines,
                    &procedures,
                    &claim_procs,
                    &adjustments,
                    &self.appointments,
                    self.report_from,
                    self.report_to,
                    true,
                ));
            }
        }
        let mut table = upstream_table.unwrap_or_else(|| ReportTable::from_rows(rows));
        if self.report_sort_column >= table.headers.len() {
            self.report_sort_column = 0;
            self.report_sort_desc = false;
            self.persist_report_sort(current.file_stem());
        }

        // Click-to-sort header row. Keeping it separate from the data grid
        // means the exported CSV/HTML is always in exactly the order shown.
        ui.horizontal_wrapped(|ui| {
            for (index, header) in table.headers.iter().enumerate() {
                let indicator = if self.report_sort_column == index {
                    if self.report_sort_desc {
                        " ▼"
                    } else {
                        " ▲"
                    }
                } else {
                    ""
                };
                if ui.button(format!("{header}{indicator}")).clicked() {
                    if self.report_sort_column == index {
                        self.report_sort_desc = !self.report_sort_desc;
                    } else {
                        self.report_sort_column = index;
                        self.report_sort_desc = false;
                    }
                    self.persist_report_sort(current.file_stem());
                }
            }
            ui.label("(click a column to sort; exports use this order)");
        });
        // Rows left after the aging-band filter. Kept local so the grid can
        // borrow it while the row actions are collected from `self`.
        let mut visible_collections_rows: Vec<CollectionsRow> = Vec::new();
        if current == ReportKind::Collections {
            // Keep the exported grid and the clickable rows in the same order.
            sort_collections_rows(
                &mut self.collections_rows,
                self.report_sort_column,
                self.report_sort_desc,
            );
            let bucket_totals = collections_bucket_totals(&self.collections_rows);
            ui.horizontal_wrapped(|ui| {
                ui.label("Aging:");
                if ui
                    .selectable_label(self.collections_bucket_filter.is_none(), "All")
                    .clicked()
                {
                    self.collections_bucket_filter = None;
                    self.persist_collections_bucket();
                }
                for bucket in CollectionsAgingBucket::ALL {
                    let totals = bucket_totals[bucket.index()];
                    let label = format!(
                        "{} ({} · {:.2})",
                        bucket.label(), totals.patients, totals.balance
                    );
                    if ui
                        .selectable_label(self.collections_bucket_filter == Some(bucket), label)
                        .clicked()
                    {
                        self.collections_bucket_filter = Some(bucket);
                        self.persist_collections_bucket();
                    }
                }
            });
            visible_collections_rows =
                filter_collections_rows(&self.collections_rows, self.collections_bucket_filter);
            let mut sorted = ReportTable {
                headers: table.headers.clone(),
                rows: Vec::new(),
            };
            for row in &visible_collections_rows {
                sorted.rows.push(row.as_table_row());
            }
            if sorted.rows.is_empty() {
                let empty = match self.collections_bucket_filter {
                    Some(bucket) => format!("No patient accounts in {} bucket", bucket.label()),
                    None => "No overdue patient accounts".to_string(),
                };
                sorted.rows.push(vec![
                    empty,
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ]);
            }
            table = sorted;
        } else {
            table.sort_by_column(self.report_sort_column, self.report_sort_desc);
        }

        let practice = self.report_prefs.practice_name.clone();
        let export_title = if current == ReportKind::Collections {
            collections_export_title(self.collections_bucket_filter)
        } else {
            current.label().to_string()
        };
        let csv = render_report_csv(&export_title, &practice, &table);
        let html = render_report_html(&export_title, &practice, &table);

        if current == ReportKind::Collections {
            let total_balance: f64 = visible_collections_rows.iter().map(|row| row.balance).sum();
            let scope = match self.collections_bucket_filter {
                Some(bucket) => format!(" · {} band", bucket.label()),
                None => String::new(),
            };
            ui.label(format!(
                "{} individual patient account(s) · total balance {:.2}{} · as of {}",
                visible_collections_rows.len(),
                total_balance,
                scope,
                self.report_to
            ));
        }

        // Collect clicks first so the grid can borrow the worklist immutably
        // while the actions are applied to `self` afterwards.
        let mut worklist_actions: Vec<(Id, CollectionsAction)> = Vec::new();
        egui::Grid::new("report_grid")
            .num_columns(table.headers.len() + usize::from(current == ReportKind::Collections))
            .striped(true)
            .show(ui, |ui| {
                for (index, row) in table.rows.iter().enumerate() {
                    for value in row {
                        ui.label(value);
                    }
                    if current == ReportKind::Collections {
                        if let Some(entry) = visible_collections_rows.get(index) {
                            ui.horizontal(|ui| {
                                if ui.button("Open account").clicked() {
                                    worklist_actions
                                        .push((entry.pat_num, CollectionsAction::OpenAccount));
                                }
                                if ui.button("Record payment").clicked() {
                                    worklist_actions
                                        .push((entry.pat_num, CollectionsAction::RecordPayment));
                                }
                                if ui.button("Statement").clicked() {
                                    worklist_actions
                                        .push((entry.pat_num, CollectionsAction::Statement));
                                }
                            });
                        } else {
                            ui.label("");
                        }
                    }
                    ui.end_row();
                }
            });
        for (pat_num, action) in worklist_actions {
            self.apply_collections_action(pat_num, action);
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Export");
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.report_export_path).desired_width(360.0),
            );
            if response.changed() {
                let user_key = self.period_close_history_user_key();
                self.report_prefs.set_user_export_path(
                    user_key,
                    current.file_stem(),
                    self.report_export_path.clone(),
                );
                if let Err(err) = self.save_report_prefs() {
                    self.error = Some(format!("Could not save report preferences: {err}"));
                }
            }
        });
        let save_csv = ui.button("Save CSV").clicked();
        let save_html = ui.button("Save HTML").clicked();

        if save_csv || save_html {
            let extension = if save_csv { "csv" } else { "html" };
            let path = report_export_path(&self.report_export_path, extension);
            let content = if save_csv { &csv } else { &html };
            let path_text = path.to_string_lossy().into_owned();
            match std::fs::write(&path, content) {
                Ok(()) => self.notice = Some(format!("Report written to {path_text}")),
                Err(err) => self.error = Some(format!("Could not write report: {err}")),
            }
            self.report_export_path = path_text.clone();
            let user_key = self.period_close_history_user_key();
            self.report_prefs
                .set_user_export_path(user_key, current.file_stem(), path_text);
            if let Err(err) = self.save_report_prefs() {
                self.error = Some(format!("Could not save report preferences: {err}"));
            }
        }

        if current == ReportKind::PayPlanDetail {
            let audit_table = stale_resolved_export_table(&table);
            let audit_count = audit_table.rows.len();
            let practice = self.report_prefs.practice_name.clone();
            let audit_csv =
                render_report_csv("Payment plans (stale resolved)", &practice, &audit_table);
            let audit_html =
                render_report_html("Payment plans (stale resolved)", &practice, &audit_table);
            if self.pay_plan_audit_export_path.is_empty() {
                self.pay_plan_audit_export_path = default_pay_plan_audit_export_path();
                self.remember_export_path(
                    EXPORT_KEY_PAY_PLAN_AUDIT,
                    &self.pay_plan_audit_export_path.clone(),
                );
            }
            let mut audit_export: Option<String> = None;
            let mut audit_path_changed: Option<String> = None;
            ui.separator();
            ui.heading("Filtered audit export");
            ui.label(
                "Saves only plans with stale-resolved charges, including the recorded reasons, \
                 for filing alongside the security-log extract.",
            );
            ui.horizontal(|ui| {
                ui.label("Export filtered plans");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.pay_plan_audit_export_path)
                        .desired_width(320.0),
                );
                if response.changed() {
                    audit_path_changed = Some(self.pay_plan_audit_export_path.clone());
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Export filtered plans (CSV)").clicked() {
                    let path = report_export_path(&self.pay_plan_audit_export_path, "csv");
                    match std::fs::write(&path, &audit_csv) {
                        Ok(()) => {
                            let path_text = path.to_string_lossy().into_owned();
                            self.pay_plan_audit_export_path = path_text.clone();
                            self.notice = Some(format!(
                                "Exported {audit_count} stale-resolved plan(s) to {path_text}"
                            ));
                            self.error = None;
                            audit_export = Some(path_text);
                        }
                        Err(err) => self.error =
                            Some(format!("Could not export stale-resolved plans CSV: {err}")),
                    }
                }
                if ui.button("Export filtered plans (HTML)").clicked() {
                    let path = report_export_path(&self.pay_plan_audit_export_path, "html");
                    match std::fs::write(&path, &audit_html) {
                        Ok(()) => {
                            let path_text = path.to_string_lossy().into_owned();
                            self.pay_plan_audit_export_path = path_text.clone();
                            self.notice = Some(format!(
                                "Exported {audit_count} stale-resolved plan(s) to {path_text}"
                            ));
                            self.error = None;
                            audit_export = Some(path_text);
                        }
                        Err(err) => self.error =
                            Some(format!("Could not export stale-resolved plans HTML: {err}")),
                    }
                }
            });
            if let Some(path) = audit_path_changed.or(audit_export.clone()) {
                self.remember_export_path(EXPORT_KEY_PAY_PLAN_AUDIT, &path);
            }
            if let Some(path_text) = audit_export {
                self.audit(
                    Permission::BILLING,
                    0,
                    format!("Exported {audit_count} stale-resolved plan(s) to {path_text}"),
                );
            }
        }
    }

    // ---------------------------------------------------------- Discount plans

    /// Setup → Discount plans: OD `discountplan` / `discountplansub` editors.
    /// Discount plans are the patient-facing alternative to insurance, so this
    /// editor stays available even when Insurance & claims is switched off.
    fn discount_plans_ui(&mut self, ui: &mut Ui) {
        if !self.features.enabled(FeatureKey::DiscountPlans) {
            return;
        }
        ui.separator();
        ui.heading("Discount plans");
        ui.label(
            "A plan prices completed procedures from its reduced fee schedule and posts the \
             difference as an adjustment of the chosen type. A subscription links one patient \
             to a plan over an effective/term range (OD discountplan / discountplansub).",
        );

        let can_edit = self.can(Permission::SETUP);
        let fee_schedules = self.store.list_fee_schedules().unwrap_or_default();
        let today = Local::now().date_naive();
        let mut edit_plan: Option<DiscountPlan> = None;
        let mut edit_sub: Option<DiscountPlanSub> = None;
        let mut end_sub: Option<(Id, Id)> = None; // (subscription, patient)
        let mut reopen_sub: Option<DiscountPlanSub> = None;

        ui.collapsing(format!("Plans ({})", self.discount_plans.len()), |ui| {
            if can_edit && ui.button("+ New plan").clicked() {
                edit_plan = Some(DiscountPlan::default());
            }
            egui::Grid::new("discount_plan_grid")
                .num_columns(6)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Description");
                    ui.strong("Fee schedule");
                    ui.strong("Adjustment type");
                    ui.strong("Annual max");
                    ui.strong("Hidden");
                    ui.strong("");
                    ui.end_row();
                    for plan in &self.discount_plans {
                        ui.label(&plan.description);
                        ui.label(
                            fee_schedules
                                .iter()
                                .find(|f| f.fee_sched_num == plan.fee_sched_num)
                                .map(|f| f.description.as_str())
                                .unwrap_or("—"),
                        );
                        ui.label(
                            self.definitions
                                .iter()
                                .find(|d| d.def_num == plan.def_num)
                                .map(|d| d.item_name.as_str())
                                .unwrap_or("—"),
                        );
                        ui.label(format!("{:.2}", plan.annual_max));
                        ui.label(if plan.is_hidden { "Yes" } else { "No" });
                        if can_edit && ui.button("Edit").clicked() {
                            edit_plan = Some(plan.clone());
                        }
                        ui.end_row();
                    }
                });
        });

        ui.collapsing(
            format!("Subscriptions ({})", self.discount_plan_subs.len()),
            |ui| {
                if can_edit && ui.button("+ New subscription").clicked() {
                    edit_sub = Some(DiscountPlanSub::default());
                }
                egui::Grid::new("discount_sub_grid")
                    .num_columns(8)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Patient");
                        ui.strong("Plan");
                        ui.strong("Effective");
                        ui.strong("Term");
                        ui.strong("Status");
                        ui.strong("Note");
                        ui.strong("");
                        ui.strong("");
                        ui.end_row();
                        for sub in &self.discount_plan_subs {
                            let patient_name = self
                                .patients
                                .iter()
                                .find(|p| p.pat_num == sub.pat_num)
                                .map(|p| format!("{}, {}", p.l_name, p.f_name))
                                .unwrap_or_else(|| format!("Patient #{}", sub.pat_num));
                            let plan_name = self
                                .discount_plans
                                .iter()
                                .find(|p| p.discount_plan_num == sub.discount_plan_num)
                                .map(|p| p.description.as_str())
                                .unwrap_or("—");
                            let term = if sub.is_open_ended() {
                                "Open".to_string()
                            } else {
                                sub.date_term
                                    .map(|d| d.format("%Y-%m-%d").to_string())
                                    .unwrap_or_default()
                            };
                            let status = if !sub.is_valid_for_date(today) {
                                if sub.date_effective.is_some_and(|d| today < d) {
                                    "Future"
                                } else {
                                    "Ended"
                                }
                            } else {
                                "Active"
                            };
                            ui.label(patient_name);
                            ui.label(plan_name);
                            ui.label(
                                sub.date_effective
                                    .map(|d| d.format("%Y-%m-%d").to_string())
                                    .unwrap_or_default(),
                            );
                            ui.label(term);
                            ui.label(status);
                            ui.label(&sub.note);
                            if can_edit && ui.button("Edit").clicked() {
                                edit_sub = Some(sub.clone());
                            } else {
                                ui.label("");
                            }
                            if can_edit {
                                if sub.is_open_ended() || sub.date_term.is_some_and(|d| d > today)
                                {
                                    if ui.button("End").clicked() {
                                        end_sub = Some((sub.discount_sub_num, sub.pat_num));
                                    }
                                } else if ui.button("Reopen").clicked() {
                                    let mut reopened = sub.clone();
                                    reopened.date_term = None;
                                    reopen_sub = Some(reopened);
                                }
                            } else {
                                ui.label("");
                            }
                            ui.end_row();
                        }
                    });
            },
        );

        if let Some(plan) = edit_plan {
            self.discount_plan_edit = Some(plan);
            self.discount_plan_edit_open = true;
        }
        if let Some(sub) = edit_sub {
            self.discount_sub_pat_num = sub.pat_num;
            self.discount_sub_effective_text = sub
                .date_effective
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            self.discount_sub_term_text = sub
                .date_term
                .filter(|_| !sub.is_open_ended())
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            self.discount_sub_edit = Some(sub);
            self.discount_sub_edit_open = true;
        }
        if let Some((num, pat_num)) = end_sub {
            match self.store.terminate_discount_plan_sub(num, today) {
                Ok(()) => {
                    self.audit_if(
                        FeatureKey::AuditBillingChanges,
                        Permission::SETUP,
                        pat_num,
                        format!(
                            "Discount subscription {num} ended on {}",
                            today.format("%Y-%m-%d")
                        ),
                    );
                    self.notice = Some(format!(
                        "Discount subscription {num} ended on {}",
                        today.format("%Y-%m-%d")
                    ));
                    self.error = None;
                    self.refresh();
                }
                Err(err) => self.error = Some(err),
            }
        }
        if let Some(sub) = reopen_sub {
            let num = sub.discount_sub_num;
            let pat_num = sub.pat_num;
            match self.store.upsert_discount_plan_sub(&sub) {
                Ok(_) => {
                    self.audit_if(
                        FeatureKey::AuditBillingChanges,
                        Permission::SETUP,
                        pat_num,
                        format!("Discount subscription {num} reopened"),
                    );
                    self.notice = Some(format!("Discount subscription {num} reopened"));
                    self.error = None;
                    self.refresh();
                }
                Err(err) => self.error = Some(err),
            }
        }
    }

    // ---------------------------------------------------------------- Setup

    fn setup_ui(&mut self, ui: &mut Ui) {
        ui.heading("Setup & security");
        self.eservices_ui(ui);
        ui.separator();
        self.definitions_ui(ui);
        self.discount_plans_ui(ui);
        ui.separator();
        ui.label("Preferences are loaded from the preference table (OD: Pref.ValueString).");

        if self.features.enabled(FeatureKey::ShowPreferences) {
            let prefs = self.store.list_prefs().unwrap_or_default();
            ui.collapsing(format!("Preferences ({})", prefs.len()), |ui| {
                egui::Grid::new("pref_grid")
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Name");
                        ui.strong("Value");
                        ui.end_row();
                        for pref in &prefs {
                            ui.label(&pref.pref_name);
                            ui.label(&pref.pref_value);
                            ui.end_row();
                        }
                    });
            });
        }

        if self.features.enabled(FeatureKey::ShowUsers)
            && self.can(Permission::SECURITY_ADMIN)
        {
            let users = self.store.list_users().unwrap_or_default();
            ui.collapsing(format!("Users ({})", users.len()), |ui| {
                egui::Grid::new("user_grid")
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Num");
                        ui.strong("User name");
                        ui.strong("Clinic");
                        ui.end_row();
                        for user in &users {
                            ui.label(user.user_num.to_string());
                            ui.label(&user.user_name);
                            ui.label(user.clinic_num.to_string());
                            ui.end_row();
                        }
                    });
            });
        }

        if self.features.enabled(FeatureKey::ShowPermissions) {
            let session = self.session.clone();
            let grants = self.group_permissions.clone();
            ui.collapsing("Effective permissions", |ui| {
                let Some(session) = session else {
                    ui.label("Not signed in.");
                    return;
                };
                ui.label(format!(
                    "{} (#{}) — {} grants",
                    session.user_name,
                    session.user_num,
                    session.permissions.len()
                ));
                if session.permissions.is_empty() {
                    ui.weak("No grouppermission rows found for this user.");
                }
                for permission in &session.permissions {
                    ui.label(format!("{} = {}", permission.label(), permission.as_od()));
                }
            });

            ui.collapsing("Permission codes", |ui| {
                ui.label("EnumPermType codes used by this port (from grouppermission):");
                egui::Grid::new("perm_code_grid")
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Code");
                        ui.strong("Permission");
                        ui.strong("Grants");
                        ui.end_row();
                        for permission in Permission::ALL {
                            let count = grants
                                .iter()
                                .filter(|g| g.perm_type == permission)
                                .count();
                            ui.label(permission.as_od().to_string());
                            ui.label(permission.label());
                            ui.label(count.to_string());
                            ui.end_row();
                        }
                    });
            });
        }

        if self.features.enabled(FeatureKey::ShowAuditLog)
            && self.can(Permission::AUDIT_TRAIL)
        {
            let mut refresh_log = false;
            let mut export_audit: Option<(Id, String)> = None;
            let mut log_path_changed: Option<String> = None;
            let mut log_file_saved: Option<String> = None;
            let patient_filter = security_log_patient_filter(&self.security_log_patient_text);
            let log_pat_num = patient_filter.unwrap_or(0);
            if self.security_log_export_patient != log_pat_num {
                self.security_log_export_patient = log_pat_num;
                let path = match patient_filter {
                    Some(num) => patient_export_path(
                        &self.report_prefs,
                        &self.period_close_history_user_key(),
                        EXPORT_KEY_SECURITY_LOG,
                        num,
                        default_security_log_export_path(Some(num)),
                    ),
                    None => {
                        let persisted = persisted_export_path(
                            &self.report_prefs,
                            &self.period_close_history_user_key(),
                            EXPORT_KEY_SECURITY_LOG,
                        );
                        if persisted.is_empty() {
                            default_security_log_export_path(None)
                        } else {
                            persisted
                        }
                    }
                };
                self.security_log_export_path = path.clone();
                self.remember_export_path(&security_log_export_key(patient_filter), &path);
            }
            let log: Vec<&SecurityEvent> = self
                .security_log
                .iter()
                .filter(|event| {
                    security_log_matches(event, &self.security_log_filter)
                        && patient_filter.map_or(true, |num| event.pat_num == num)
                })
                .collect();
            let export_count = log.len();
            let table = security_log_export_table(&log);
            let practice = self.report_prefs.practice_name.clone();
            let csv = render_report_csv("Security log (filtered)", &practice, &table);
            let html = render_report_html("Security log (filtered)", &practice, &table);
            ui.collapsing(
                format!(
                    "Security log ({}/{})",
                    log.len(),
                    self.security_log.len()
                ),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Filter");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.security_log_filter)
                                .desired_width(240.0)
                                .hint_text("reason, user, permission, text..."),
                        );
                        ui.label("Patient #");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.security_log_patient_text)
                                .desired_width(80.0)
                                .hint_text("optional"),
                        );
                        if ui.button("Refresh").clicked() {
                            refresh_log = true;
                        }
                    });
                    if log.is_empty() {
                        if self.security_log_filter.trim().is_empty()
                            && patient_filter.is_none()
                        {
                            ui.weak("No entries yet. Saving a patient or appointment writes one.");
                        } else {
                            ui.weak("No log entries match the filter.");
                        }
                    }
                    egui::Grid::new("security_log_grid")
                        .num_columns(5)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Time");
                            ui.strong("User");
                            ui.strong("Type");
                            ui.strong("Patient");
                            ui.strong("Details");
                            ui.end_row();
                            for event in &log {
                                ui.label(event.log_datetime.format("%Y-%m-%d %H:%M").to_string());
                                ui.label(event.user_num.to_string());
                                ui.label(event.perm_type.label());
                                ui.label(if event.pat_num == 0 {
                                    "—".to_string()
                                } else {
                                    event.pat_num.to_string()
                                });
                                ui.label(&event.log_text);
                                ui.end_row();
                            }
                        });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Export filtered log");
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.security_log_export_path)
                                .desired_width(320.0),
                        );
                        if response.changed() {
                            log_path_changed = Some(self.security_log_export_path.clone());
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Export filtered log (CSV)").clicked() {
                            let path = report_export_path(&self.security_log_export_path, "csv");
                            match std::fs::write(&path, &csv) {
                                Ok(()) => {
                                    let path_text = path.to_string_lossy().into_owned();
                                    self.security_log_export_path = path_text.clone();
                                    self.notice = Some(format!(
                                        "Exported {export_count} security log row(s) to {path_text}"
                                    ));
                                    self.error = None;
                                    log_file_saved = Some(path_text.clone());
                                    export_audit = Some((
                                        patient_filter.unwrap_or(0),
                                        format!(
                                            "Exported {export_count} security log row(s) to {path_text} (filter: {}, patient: {})",
                                            self.security_log_filter.trim(),
                                            self.security_log_patient_text.trim(),
                                        ),
                                    ));
                                }
                                Err(err) => self.error =
                                    Some(format!("Could not export security log CSV: {err}")),
                            }
                        }
                        if ui.button("Export filtered log (HTML)").clicked() {
                            let path = report_export_path(&self.security_log_export_path, "html");
                            match std::fs::write(&path, &html) {
                                Ok(()) => {
                                    let path_text = path.to_string_lossy().into_owned();
                                    self.security_log_export_path = path_text.clone();
                                    self.notice = Some(format!(
                                        "Exported {export_count} security log row(s) to {path_text}"
                                    ));
                                    self.error = None;
                                    log_file_saved = Some(path_text.clone());
                                    export_audit = Some((
                                        patient_filter.unwrap_or(0),
                                        format!(
                                            "Exported {export_count} security log row(s) to {path_text} (filter: {}, patient: {})",
                                            self.security_log_filter.trim(),
                                            self.security_log_patient_text.trim(),
                                        ),
                                    ));
                                }
                                Err(err) => self.error =
                                    Some(format!("Could not export security log HTML: {err}")),
                            }
                        }
                    });
                },
            );
            if let Some(path) = log_path_changed.or(log_file_saved) {
                self.remember_export_path(&security_log_export_key(patient_filter), &path);
            }
            if let Some((pat_num, text)) = export_audit {
                self.audit(Permission::AUDIT_TRAIL, pat_num, text);
            }
            if refresh_log {
                self.security_log = self.store.list_security_log(50).unwrap_or_default();
            }
        }
    }

    // ---------------------------------------------------------------- Settings

    fn settings_ui(&mut self, ui: &mut Ui) {
        ui.heading("Settings");
        ui.label("Turn features on or off. Changes save automatically.");
        ui.label("Settings are kept in your Windows AppData folder under odclinic.");
        ui.separator();

        let mut features = self.features.clone();
        let mut changed = false;
        let mut categories: Vec<&'static str> = Vec::new();
        for key in FeatureKey::ALL {
            if !categories.contains(&key.category()) {
                categories.push(key.category());
            }
        }

        for category in categories {
            ui.collapsing(category, |ui| {
                for key in FeatureKey::ALL.into_iter().filter(|k| k.category() == category) {
                    let mut value = features.enabled(key);
                    let response = ui
                        .checkbox(&mut value, key.label())
                        .on_hover_text(key.description());
                    if response.changed() {
                        features.set(key, value);
                        changed = true;
                    }
                }
            });
        }

        ui.separator();
        if ui.button("Restore defaults").clicked() {
            features = FeatureSettings::default();
            changed = true;
        }

        if changed {
            self.features = features;
            match self.features.save() {
                Ok(()) => self.notice = Some("Settings saved".into()),
                Err(e) => self.error = Some(e),
            }
        }
    }

    // ---------------------------------------------------------------- Shared widgets

    fn patient_picker(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Patient:");
            let current = self
                .selected_patient
                .and_then(|num| self.patients.iter().find(|p| p.pat_num == num))
                .map(|p| p.display_name())
                .unwrap_or_else(|| "(none selected)".into());
            egui::ComboBox::from_id_salt("patient_picker")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for patient in &self.patients {
                        let text = format!("{} — {}", patient.pat_num, patient.display_name());
                        if ui
                            .selectable_value(
                                &mut self.selected_patient,
                                Some(patient.pat_num),
                                text,
                            )
                            .clicked()
                        {
                            self.notice = None;
                        }
                    }
                });
            if self.features.module_enabled(Module::Family)
                && ui.button("Family module").clicked()
            {
                self.module = Module::Family;
                self.show_settings = false;
            }
        });
    }

    fn adjustment_window(&mut self, ctx: &egui::Context) {
        let Some(mut adj) = self.adj_edit.take() else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        egui::Window::new("Adjustment — New")
            .open(&mut self.adj_edit_open)
            .resizable(true)
            .default_width(380.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Date  ");
                    ui.add(egui::TextEdit::singleline(&mut self.adj_date_text).hint_text("YYYY-MM-DD"));
                });
                ui.horizontal(|ui| {
                    ui.label("Amount");
                    ui.text_edit_singleline(&mut self.adj_amount_text);
                });
                ui.label("Negative amounts reduce the balance (discount / write-off).");
                ui.horizontal(|ui| {
                    ui.label("Note  ");
                    ui.text_edit_singleline(&mut self.adj_note_text);
                });
                ui.separator();
                ui.horizontal(|ui| {
                    save = ui.button("Save").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if save {
            adj.adj_date = NaiveDate::parse_from_str(&self.adj_date_text, "%Y-%m-%d").unwrap_or(adj.adj_date);
            adj.adj_amount = self.adj_amount_text.parse().unwrap_or(0.0);
            adj.note = self.adj_note_text.clone();
            let pat_num = adj.pat_num;
            let amount = adj.adj_amount;
            match self.store.upsert_adjustment(&adj) {
                Ok(num) => {
                    self.audit_if(
                        FeatureKey::AuditBillingChanges,
                        Permission::ADJUSTMENT_CREATE,
                        pat_num,
                        format!("Adjustment {num} recorded: {amount:.2}"),
                    );
                    self.notice = Some("Adjustment recorded".into());
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
            self.adj_edit_open = false;
        } else if cancel {
            self.adj_edit_open = false;
        } else if self.adj_edit_open {
            self.adj_edit = Some(adj);
        }
    }

    /// Explicit, audited path from a recorded account adjustment into OD's
    /// double-entry journal. The two lines are previewed before anything is
    /// written, and the accounts default from the chart by the adjustment's sign.
    /// Explicit, audited path from an account adjustment or patient payment
    /// into OD's double-entry journal. The two lines are previewed before
    /// anything is written, and accounts default from the chart by direction.
    fn journal_post_window(&mut self, ctx: &egui::Context) {
        let Some(source) = self.journal_post_edit.clone() else {
            return;
        };
        let accounts = self.store.list_accounts().unwrap_or_default();
        let account_label = |num: Id| -> String {
            accounts
                .iter()
                .find(|a| a.account_num == num)
                .map(|a| {
                    format!(
                        "{} ({:?} #{})",
                        a.description, a.account_type, a.account_num
                    )
                })
                .unwrap_or_else(|| format!("Select an account (#{num})"))
        };

        // The reference token is rebuilt on every write, so editing the note
        // can never strip the link back to the source record.
        let memo = source.memo(&self.journal_memo_text);

        // Build the preview from the current selections so staff see exactly
        // which accounts, amounts and memo will be written before confirming.
        let mut preview_rows: Vec<[String; 5]> = Vec::new();
        let mut preview_error: Option<String> = None;
        let current_journal = self.store.list_journal_entries().unwrap_or_default();
        if source.is_posted(&current_journal) {
            preview_error = Some(format!(
                "{} #{} is already posted to the ledger. Posting again would duplicate it.",
                source.label(),
                source.source_num()
            ));
        }
        let lock_error = self.closed_periods.reject_posting(
            source.date(),
            &format!("{} #{}", source.label(), source.source_num()),
        );
        if preview_error.is_none() {
            preview_error = lock_error.err();
        }
        match source.build(
            self.journal_debit_account,
            self.journal_credit_account,
            self.journal_transaction_num,
        ) {
            Ok(entries) => {
                for entry in &entries {
                    preview_rows.push([
                        entry.date_displayed.to_string(),
                        account_label(entry.account_num),
                        format!("{:.2}", entry.debit_amt),
                        format!("{:.2}", entry.credit_amt),
                        memo.clone(),
                    ]);
                }
                let mut validated = entries;
                for entry in &mut validated {
                    entry.memo = memo.clone();
                }
                if let Err(err) = validate_balanced_journal(&validated) {
                    preview_error = Some(err);
                }
            }
            Err(err) => preview_error = Some(err),
        }

        let mut save = false;
        let mut cancel = false;
        egui::Window::new(format!("Post {} to ledger", source.label()))
            .open(&mut self.journal_post_open)
            .resizable(true)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.label(source.heading());
                if !source.note().trim().is_empty() {
                    ui.label(format!("Note: {}", source.note()));
                }
                ui.separator();
                ui.label(format!(
                    "Writes one balanced debit/credit pair with a shared transaction number. \
                     The patient balance is unchanged; the posting makes this {} visible \
                     to the Balance Sheet, General Ledger and Profit & Loss reports.",
                    source.label().to_lowercase()
                ));
                egui::Grid::new("journal_post_accounts")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Debit account");
                        egui::ComboBox::from_id_salt("journal_debit_account")
                            .selected_text(account_label(self.journal_debit_account))
                            .show_ui(ui, |ui| {
                                for account in &accounts {
                                    ui.selectable_value(
                                        &mut self.journal_debit_account,
                                        account.account_num,
                                        account_label(account.account_num),
                                    );
                                }
                            });
                        ui.end_row();
                        ui.label("Credit account");
                        egui::ComboBox::from_id_salt("journal_credit_account")
                            .selected_text(account_label(self.journal_credit_account))
                            .show_ui(ui, |ui| {
                                for account in &accounts {
                                    ui.selectable_value(
                                        &mut self.journal_credit_account,
                                        account.account_num,
                                        account_label(account.account_num),
                                    );
                                }
                            });
                        ui.end_row();
                        ui.label("Transaction #");
                        ui.label(self.journal_transaction_num.to_string());
                        ui.end_row();
                        ui.label("Memo note").on_hover_text(format!(
                            "The {} reference is added automatically and cannot be removed.",
                            source.label()
                        ));
                        ui.text_edit_singleline(&mut self.journal_memo_text);
                        ui.end_row();
                    });
                ui.separator();
                ui.strong("Preview — nothing is written until you confirm");
                egui::Grid::new("journal_post_preview")
                    .num_columns(5)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Date");
                        ui.strong("Account");
                        ui.strong("Debit");
                        ui.strong("Credit");
                        ui.strong("Memo");
                        ui.end_row();
                        for row in &preview_rows {
                            for cell in row {
                                ui.label(cell);
                            }
                            ui.end_row();
                        }
                    });
                if let Some(err) = &preview_error {
                    ui.colored_label(Color32::RED, err);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    save = ui
                        .button("Post to ledger")
                        .clicked()
                        && preview_error.is_none();
                    cancel = ui.button("Cancel").clicked();
                });
            });

        if cancel {
            self.journal_post_edit = None;
            self.journal_post_open = false;
            return;
        }
        if !self.journal_post_open {
            self.journal_post_edit = None;
            return;
        }
        if !save {
            return;
        }

        // Re-check with the freshest values (a selection change, a click or a
        // posting from another window can land on the same frame) before any
        // write. This is the last line of defence against a duplicate pair.
        if source.is_posted(&self.store.list_journal_entries().unwrap_or_default()) {
            self.error = Some(format!(
                "{} #{} is already posted to the ledger; nothing was written.",
                source.label(),
                source.source_num()
            ));
            return;
        }
        let lock_error = self.closed_periods.reject_posting(
            source.date(),
            &format!("{} #{}", source.label(), source.source_num()),
        );
        if let Err(err) = lock_error {
            self.error = Some(err);
            return;
        }
        let mut entries = match source.build(
            self.journal_debit_account,
            self.journal_credit_account,
            self.journal_transaction_num,
        ) {
            Ok(entries) => entries,
            Err(err) => {
                self.error = Some(err);
                return;
            }
        };
        for entry in &mut entries {
            entry.memo = memo.clone();
        }
        if let Err(err) = validate_balanced_journal(&entries) {
            self.error = Some(err);
            return;
        }
        let pat_num = source.pat_num();
        let source_num = source.source_num();
        let label = source.label();
        let amount = source.amount();
        let debit = self.journal_debit_account;
        let credit = self.journal_credit_account;
        match self.store.post_journal_transaction(&entries) {
            Ok(transaction_num) => {
                self.audit_if(
                    FeatureKey::AuditBillingChanges,
                    Permission::ADJUSTMENT_CREATE,
                    pat_num,
                    format!(
                        "{label} #{source_num} posted to journal transaction #{transaction_num}: debit account #{debit}, credit account #{credit}, amount {amount:.2}"
                    ),
                );
                self.notice = Some(format!(
                    "{label} posted to ledger as journal transaction #{transaction_num} — now visible in the accounting reports"
                ));
                self.error = None;
                self.journal_post_edit = None;
                self.journal_post_open = false;
            }
            Err(err) => self.error = Some(format!("Could not post {label} to ledger: {err}")),
        }
    }

    /// Period-close batch posting of every unposted source in the report range.
    /// The plan is rebuilt from fresh store data each frame and again on
    /// confirm, so a source cannot be posted twice even if another window
    /// changed the ledger while this one was open.
    fn post_all_window(&mut self, ctx: &egui::Context) {
        if !self.post_all_open {
            return;
        }
        let accounts = self.store.list_accounts().unwrap_or_default();
        let (range_from, range_to) = (self.report_from, self.report_to);
        let procedures = gather_procedures(&*self.store, &self.patients);
        let mut payments = Vec::new();
        for patient in &self.patients {
            payments.extend(self.store.list_payments(patient.pat_num).unwrap_or_default());
        }
        let adjustments = gather_adjustments(&*self.store, &self.patients);
        let plan = build_posting_batch_plan_with_closed(
            &procedures,
            &payments,
            &adjustments,
            &self.store.list_journal_entries().unwrap_or_default(),
            &self.post_all_defaults,
            &self.closed_periods,
            range_from,
            range_to,
        );
        let is_locked = |date: chrono::NaiveDate| {
            date >= range_from && date <= range_to && self.closed_periods.is_closed(date)
        };
        let blocked = procedures
            .iter()
            .filter(|p| p.proc_status.is_completed() && is_locked(p.proc_date))
            .count()
            + payments.iter().filter(|p| is_locked(p.pay_date)).count()
            + adjustments.iter().filter(|a| is_locked(a.adj_date)).count();
        let count = |kind: PostingSourceKind| {
            plan.iter()
                .filter(|item| item.source.kind() == kind)
                .count()
        };
        let accounts_ok = self.post_all_defaults.is_complete();

        let mut save = false;
        let mut cancel = false;
        egui::Window::new("Post all unposted sources")
            .open(&mut self.post_all_open)
            .resizable(true)
            .default_width(720.0)
            .show(ctx, |ui| {
                ui.label(format!(
                    "{} charge(s) • {} payment(s) • {} adjustment(s) ready to post in {range_from}–{range_to}.",
                    count(PostingSourceKind::Charge),
                    count(PostingSourceKind::Payment),
                    count(PostingSourceKind::Adjustment),
                ));
                if blocked > 0 {
                    ui.colored_label(
                        Color32::from_rgb(200, 130, 30),
                        format!("{blocked} source(s) in this range are in closed periods and were excluded — reopen the period to post them."),
                    );
                }
                if plan.is_empty() {
                    ui.colored_label(
                        Color32::GRAY,
                        "Nothing to post — every source in this range is already in the ledger, locked by a closed period, or has no amount.",
                    );
                }
                ui.separator();
                ui.label(
                    "One balanced journal pair per source, dated to the source. Charges debit A/R \
                     and credit income; collections debit cash and credit A/R; write-offs debit \
                     expense and credit A/R. Patient balances are unchanged.",
                );
                egui::Grid::new("post_all_accounts")
                    .num_columns(2)
                    .show(ui, |ui| {
                        posting_account_pair_ui(
                            ui,
                            "post_all_charge_debit",
                            "post_all_charge_credit",
                            "Charge debit (A/R) → credit (Income)",
                            &accounts,
                            &mut self.post_all_defaults.charge,
                        );
                        posting_account_pair_ui(
                            ui,
                            "post_all_payment_debit",
                            "post_all_payment_credit",
                            "Payment debit (Cash) → credit (A/R); refunds reverse",
                            &accounts,
                            &mut self.post_all_defaults.payment,
                        );
                        posting_account_pair_ui(
                            ui,
                            "post_all_pos_adj_debit",
                            "post_all_pos_adj_credit",
                            "Positive adjustment debit (A/R) → credit (Income)",
                            &accounts,
                            &mut self.post_all_defaults.positive_adjustment,
                        );
                        posting_account_pair_ui(
                            ui,
                            "post_all_neg_adj_debit",
                            "post_all_neg_adj_credit",
                            "Write-off debit (Expense) → credit (A/R)",
                            &accounts,
                            &mut self.post_all_defaults.negative_adjustment,
                        );
                    });
                ui.separator();
                ui.strong("Preview — nothing is written until you confirm");
                if plan.is_empty() {
                    ui.label("No sources to post.");
                } else {
                    egui::Grid::new("post_all_preview")
                        .num_columns(7)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Type");
                            ui.strong("Date");
                            ui.strong("Number");
                            ui.strong("Patient");
                            ui.strong("Amount");
                            ui.strong("Debit → Credit");
                            ui.strong("Txn");
                            ui.end_row();
                            for item in &plan {
                                ui.label(item.source.kind().label());
                                ui.label(item.source.date().to_string());
                                ui.label(item.source.number().to_string());
                                ui.label(format!("#{}", item.source.pat_num()));
                                ui.label(format!("{:.2}", item.source.amount()));
                                ui.label(format!(
                                    "#{} → #{}",
                                    item.debit_account, item.credit_account
                                ));
                                ui.label(item.transaction_num.to_string());
                                ui.end_row();
                            }
                        });
                }
                if !accounts_ok {
                    ui.colored_label(
                        Color32::RED,
                        "Each posting needs two different, real accounts.",
                    );
                }
                ui.separator();
                ui.horizontal(|ui| {
                    save = ui.button("Post all to ledger").clicked() && !plan.is_empty() && accounts_ok;
                    cancel = ui.button("Cancel").clicked();
                });
            });

        if cancel {
            self.post_all_open = false;
            return;
        }
        if !self.post_all_open || !save {
            return;
        }
        // Rebuild from fresh data so the plan cannot go stale between the
        // preview and the write: a source posted elsewhere in the meantime is
        // left out, and transaction numbers stay unique.
        let procedures = gather_procedures(&*self.store, &self.patients);
        let mut payments = Vec::new();
        for patient in &self.patients {
            payments.extend(self.store.list_payments(patient.pat_num).unwrap_or_default());
        }
        let adjustments = gather_adjustments(&*self.store, &self.patients);
        let plan = build_posting_batch_plan_with_closed(
            &procedures,
            &payments,
            &adjustments,
            &self.store.list_journal_entries().unwrap_or_default(),
            &self.post_all_defaults,
            &self.closed_periods,
            range_from,
            range_to,
        );
        if plan.is_empty() {
            self.notice = Some("No unposted sources remain (closed periods stay locked); nothing was written.".into());
            self.post_all_open = false;
            return;
        }
        let batches = match build_journal_batches(&plan) {
            Ok(batches) => batches,
            Err(err) => {
                self.error = Some(format!("Refusing to post an invalid batch: {err}"));
                return;
            }
        };
        let totals = |kind: PostingSourceKind| -> f64 {
            plan.iter()
                .filter(|item| item.source.kind() == kind)
                .map(|item| item.source.amount().abs())
                .sum()
        };
        let (charges, payments_n, adjustments_n) = (
            count(PostingSourceKind::Charge),
            count(PostingSourceKind::Payment),
            count(PostingSourceKind::Adjustment),
        );
        let first_txn = plan.first().map(|i| i.transaction_num).unwrap_or(0);
        let last_txn = plan.last().map(|i| i.transaction_num).unwrap_or(0);
        match self.store.post_journal_batch(&batches) {
            Ok(written) => {
                // One audit line per patient touched keeps every patient's
                // security log traceable to this practice-wide run.
                let mut audited = std::collections::BTreeSet::new();
                for item in &plan {
                    audited.insert(item.source.pat_num());
                }
                for pat_num in audited {
                    self.audit_if(
                        FeatureKey::AuditBillingChanges,
                        Permission::ADJUSTMENT_CREATE,
                        pat_num,
                        format!(
                            "Batch posted {} source(s) to journal transactions #{first_txn}–#{last_txn}: {} charge(s) totalling {:.2}, {} payment(s) totalling {:.2}, {} adjustment(s) totalling {:.2}",
                            plan.len(),
                            charges,
                            totals(PostingSourceKind::Charge),
                            payments_n,
                            totals(PostingSourceKind::Payment),
                            adjustments_n,
                            totals(PostingSourceKind::Adjustment),
                        ),
                    );
                }
                self.notice = Some(format!(
                    "Posted {} source(s) ({charges} charges, {payments_n} payments, {adjustments_n} adjustments) to the ledger as {written:?} — re-run the ledger tie-out to confirm.",
                    plan.len()
                ));
                self.error = None;
                self.post_all_open = false;
            }
            Err(err) => self.error = Some(format!("Could not post sources to ledger: {err}")),
        }
    }

    /// Rebuild the tie-out for one month and report any check that is not OK.
    /// Closing is only offered when this passes, so a signed-off month cannot
    /// conceal an unposted source or a chart-setup gap.
    fn tie_out_gate_for(&self, date: chrono::NaiveDate) -> Result<(), String> {
        let (from, to) = month_bounds(date);
        let accounts = self.store.list_accounts().unwrap_or_default();
        let journal = self.store.list_journal_entries().unwrap_or_default();
        let procedures = gather_procedures(&*self.store, &self.patients);
        let adjustments = gather_adjustments(&*self.store, &self.patients);
        let splits = gather_splits(&*self.store, &self.patients);
        let mut payments = Vec::new();
        for patient in &self.patients {
            payments.extend(self.store.list_payments(patient.pat_num).unwrap_or_default());
        }
        tie_out_ready(&ledger_tie_out_report(
            &procedures,
            &payments,
            &adjustments,
            &splits,
            &journal,
            &accounts,
            from,
            to,
        ))
    }

    /// Read-only narrowing for the Period close history report. The filtered
    /// table is the one rendered and exported, so the file matches the screen.
    /// Every change is saved for the current user in `reports.txt`.
    fn period_close_history_filters(&mut self, ui: &mut Ui) {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.label("Filter sign-offs:");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut self.period_close_history_user_filter)
                        .desired_width(130.0)
                        .hint_text("user"),
                )
                .changed();
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut self.period_close_history_month_filter)
                        .desired_width(100.0)
                        .hint_text("YYYY-MM"),
                )
                .changed();
            let action_before = self.period_close_history_action_filter;
            egui::ComboBox::from_label("Action")
                .selected_text(self.period_close_history_action_filter.label())
                .show_ui(ui, |ui| {
                    for option in PeriodCloseActionFilter::ALL {
                        ui.selectable_value(
                            &mut self.period_close_history_action_filter,
                            option,
                            option.label(),
                        );
                    }
                });
            changed |= action_before != self.period_close_history_action_filter;
            if ui.button("Clear filters").clicked() {
                self.period_close_history_user_filter.clear();
                self.period_close_history_month_filter.clear();
                self.period_close_history_action_filter = PeriodCloseActionFilter::All;
                changed = true;
            }
            ui.weak("Saved for this user; exports use the rows shown.");
        });
        if changed {
            self.persist_period_close_history_filters();
        }
    }

    /// Period-close controls shown with the Ledger tie-out report: close the
    /// month being reviewed, and reopen any earlier month that needs a late
    /// correction (both actions require Adjustment Create).
    fn period_close_controls(&mut self, ui: &mut Ui) {
        let target = self.report_to;
        let (first, last) = month_bounds(target);
        let allowed = self.features.enabled(FeatureKey::LedgerJournalPosting)
            && self.can(Permission::ADJUSTMENT_CREATE);
        let gate = if allowed {
            self.tie_out_gate_for(target)
        } else {
            Err("Journal posting is off in Settings, or the Adjustment Create permission is missing.".into())
        };
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong(format!("Period close · {first} – {last}"));
            let button = ui.add_enabled(
                gate.is_ok() && !self.period_action_open,
                egui::Button::new("Close period"),
            );
            if button.clicked() {
                self.period_action = Some((PeriodAction::Close, first));
                self.period_action_reason.clear();
                self.period_action_open = true;
                self.error = None;
            }
            button.on_hover_text(match &gate {
                Ok(()) => "Close this month once its tie-out reconciles. Requires a reason and the Adjustment Create permission.",
                Err(err) => err,
            });
        });
        if let Err(err) = &gate {
            ui.weak(format!("Cannot close yet: {err}"));
        }
        if !self.closed_periods.is_empty() {
            ui.weak("Closed periods — posting is locked until reopened:");
            let mut reopen: Option<chrono::NaiveDate> = None;
            for period in self.closed_periods.all() {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{} · {} · {}",
                        period.label(),
                        period.closed_at,
                        period.reason
                    ));
                    if ui
                        .add_enabled(
                            allowed && !self.period_action_open,
                            egui::Button::new("Reopen"),
                        )
                        .clicked()
                    {
                        reopen = Some(
                            chrono::NaiveDate::from_ymd_opt(period.year, period.month, 1)
                                .unwrap_or(target),
                        );
                    }
                });
            }
            if let Some(date) = reopen {
                self.period_action = Some((PeriodAction::Reopen, date));
                self.period_action_reason.clear();
                self.period_action_open = true;
                self.error = None;
            }
        }
    }

    /// Reasoned, audited close/reopen dialog. Preference write and in-memory
    /// state move together, so a failed save cannot unlock a period in the UI.
    fn period_action_window(&mut self, ctx: &egui::Context) {
        let Some((action, date)) = self.period_action else {
            return;
        };
        let (first, last) = month_bounds(date);
        let allowed = self.features.enabled(FeatureKey::LedgerJournalPosting)
            && self.can(Permission::ADJUSTMENT_CREATE);
        let gate = if action == PeriodAction::Close && allowed {
            self.tie_out_gate_for(date)
        } else {
            Ok(())
        };
        let reason_ready = !self.period_action_reason.trim().is_empty();
        let mut save = false;
        let mut cancel = false;
        let title = match action {
            PeriodAction::Close => "Close accounting period",
            PeriodAction::Reopen => "Reopen accounting period",
        };
        egui::Window::new(title)
            .open(&mut self.period_action_open)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.label(format!("Period {first} – {last}"));
                match action {
                    PeriodAction::Close => {
                        ui.label("Closing locks every journal posting dated in this month until it is reopened.");
                        if let Err(err) = &gate {
                            ui.colored_label(Color32::RED, err);
                        }
                    }
                    PeriodAction::Reopen => {
                        ui.label("Reopening allows new postings in this month. The reason is written to the security log.");
                    }
                }
                ui.label("Reason (written to the security log)");
                ui.text_edit_multiline(&mut self.period_action_reason);
                ui.separator();
                ui.horizontal(|ui| {
                    let label = match action {
                        PeriodAction::Close => "Close period",
                        PeriodAction::Reopen => "Reopen period",
                    };
                    save = ui
                        .add_enabled(
                            allowed && gate.is_ok() && reason_ready,
                            egui::Button::new(label),
                        )
                        .clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });

        if cancel {
            self.period_action_open = false;
            self.period_action = None;
            return;
        }
        if !self.period_action_open || !save {
            return;
        }
        if !allowed {
            self.error = Some("Journal posting is off, or the Adjustment Create permission is missing.".into());
            return;
        }
        let reason = self.period_action_reason.trim().to_string();
        let mut updated = self.closed_periods.clone();
        let outcome = match action {
            PeriodAction::Close => match gate {
                Ok(()) => match updated.close(date, &reason, Local::now().naive_local().to_string())
                {
                    Ok(period) => Ok(close_audit_text(&period.key(), &reason)),
                    Err(err) => Err(err),
                },
                Err(err) => Err(err),
            },
            PeriodAction::Reopen => match updated.reopen(date) {
                Ok(period) => Ok(reopen_audit_text(&period.key(), &period.reason, &reason)),
                Err(err) => Err(err),
            },
        };
        match outcome {
            Ok(log) => {
                if let Err(err) = self.store.upsert_pref(CLOSED_PERIODS_PREF, &updated.to_json()) {
                    self.error = Some(format!("Could not save the period close: {err}"));
                    return;
                }
                self.closed_periods = updated;
                // A gate change is always audited, independent of the optional
                // billing-change audit toggle.
                self.audit(Permission::ADJUSTMENT_CREATE, 0, log.clone());
                self.notice = Some(log);
                self.error = None;
                self.period_action_open = false;
                self.period_action = None;
            }
            Err(err) => self.error = Some(err),
        }
    }

    fn payment_window(&mut self, ctx: &egui::Context) {
        let Some(mut payment) = self.payment_edit.take() else {
            return;
        };
        let pat_num = payment.pat_num;
        let procedures = self.store.list_procedures(pat_num).unwrap_or_default();
        let plans = if self.features.enabled(FeatureKey::PaymentPlansEnabled) {
            self.store.list_pay_plans(pat_num).unwrap_or_default()
        } else {
            Vec::new()
        };

        let default_target = procedures
            .first()
            .map(|p| PaymentTarget::Procedure(p.proc_num))
            .unwrap_or(PaymentTarget::Unallocated);
        if self.payment_rows.is_empty() {
            self.payment_rows.push(PaymentRow {
                target: default_target,
                amount_text: String::new(),
            });
        }

        // Build the target list once per frame; a row whose target has
        // disappeared (e.g. a closed plan) falls back to Unallocated.
        let mut options: Vec<(PaymentTarget, String)> =
            vec![(PaymentTarget::Unallocated, "Unallocated".into())];
        for proc in &procedures {
            options.push((
                PaymentTarget::Procedure(proc.proc_num),
                format!("#{} — {} ({:.2})", proc.proc_num, proc.proc_date, proc.fee),
            ));
        }
        for plan in &plans {
            let label = if plan.note.is_empty() {
                format!("Plan #{}", plan.pay_plan_num)
            } else {
                plan.note.clone()
            };
            options.push((PaymentTarget::PayPlan(plan.pay_plan_num), label));
        }

        let mut save = false;
        let mut cancel = false;
        egui::Window::new("Payment — New")
            .open(&mut self.payment_edit_open)
            .resizable(true)
            .default_width(480.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Date  ");
                    ui.add(egui::TextEdit::singleline(&mut self.payment_date_text).hint_text("YYYY-MM-DD"));
                });
                ui.horizontal(|ui| {
                    ui.label("Amount");
                    ui.text_edit_singleline(&mut self.payment_amount_text);
                    if self.payment_amount_text.parse::<f64>().unwrap_or(0.0) < -0.005 {
                        ui.label("Refund/negative payment.");
                    }
                });
                let amount: f64 = self.payment_amount_text.parse().unwrap_or(0.0);

                let single_row = self.payment_rows.len() == 1;
                for row in &mut self.payment_rows {
                    if !options.iter().any(|(target, _)| *target == row.target) {
                        row.target = default_target;
                    }
                    // Keep the classic one-split workflow effortless: one
                    // empty row simply follows the payment amount.
                    if single_row && row.amount_text.is_empty() {
                        row.amount_text = self.payment_amount_text.clone();
                    }
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Refund plan...").clicked() {
                        let plan_num = plans.first().map(|p| p.pay_plan_num).unwrap_or(0);
                        self.payment_amount_text = (-amount.abs().max(0.0)).to_string();
                        self.payment_rows = vec![
                            PaymentRow { target: PaymentTarget::Unallocated, amount_text: format!("{:.2}", amount.abs().max(0.0)) },
                            PaymentRow { target: PaymentTarget::PayPlan(plan_num), amount_text: format!("{:.2}", -amount.abs().max(0.0)) },
                        ];
                        payment.note = format!("Refund from plan #{} to patient account", plan_num);
                    }
                    if ui.button("Transfer plan credit...").clicked() {
                        let plan_num = plans.first().map(|p| p.pay_plan_num).unwrap_or(0);
                        let amt = amount.abs().max(0.0);
                        self.payment_amount_text = "0.00".to_string();
                        self.payment_rows = vec![
                            PaymentRow { target: PaymentTarget::PayPlan(plan_num), amount_text: format!("{:.2}", -amt) },
                            PaymentRow { target: PaymentTarget::Unallocated, amount_text: format!("{:.2}", amt) },
                        ];
                        payment.note = format!("Transfer unapplied credit from plan #{} to patient account (unallocated)", plan_num);
                    }
                });
                ui.label("Allocate the payment across procedures, plans or unallocated cash:");
                let mut remove_index = None;
                let mut auto_apply_plan: Option<Id> = None;
                let mut clear_plan_allocs = false;
                for (index, row) in self.payment_rows.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        let selected = options
                            .iter()
                            .find(|(target, _)| *target == row.target)
                            .map(|(_, label)| label.clone())
                            .unwrap_or_else(|| match row.target {
                                PaymentTarget::PayPlanCharge { plan_num, charge_num } => {
                                    format!("Plan #{plan_num} charge #{charge_num}")
                                }
                                _ => "Unallocated".into(),
                            });
                        egui::ComboBox::from_id_salt(("payment_target", index))
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                for (target, label) in &options {
                                    ui.selectable_value(&mut row.target, *target, label);
                                }
                            });
                        ui.add(
                            egui::TextEdit::singleline(&mut row.amount_text)
                                .desired_width(80.0)
                                .hint_text("0.00"),
                        );
                        if let PaymentTarget::PayPlan(plan_num) = row.target {
                            if ui.small_button("Apply to due charges").clicked() {
                                auto_apply_plan = Some(plan_num);
                            }
                        }
                        if matches!(row.target, PaymentTarget::PayPlan(_) | PaymentTarget::PayPlanCharge { .. }) {
                            if ui.small_button("Clear plan allocations").clicked() {
                                clear_plan_allocs = true;
                            }
                        }
                        if ui.small_button("×").clicked() {
                            remove_index = Some(index);
                        }
                    });
                }
                if clear_plan_allocs {
                    self.payment_rows.retain(|r| !matches!(r.target, PaymentTarget::PayPlan(_) | PaymentTarget::PayPlanCharge { .. }));
                    if self.payment_rows.is_empty() {
                        self.payment_rows.push(PaymentRow { target: PaymentTarget::Unallocated, amount_text: String::new() });
                    }
                }
                if let Some(index) = remove_index {
                    if self.payment_rows.len() > 1 {
                        self.payment_rows.remove(index);
                    }
                }
                if let Some(plan_num) = auto_apply_plan {
                    let row_amount = self
                        .payment_rows
                        .iter()
                        .find(|r| matches!(r.target, PaymentTarget::PayPlan(pn) if pn == plan_num))
                        .and_then(|r| r.amount_text.parse::<f64>().ok())
                        .unwrap_or(amount);
                    let as_of = chrono::Local::now().date_naive();
                    let plan_charges = self.store.list_pay_plan_charges(pat_num).unwrap_or_default();
                    let plan_splits = self.store.list_pay_splits(pat_num).unwrap_or_default();
                    let allocs = od_core::allocate_plan_payment(plan_num, row_amount, &plan_charges, &plan_splits, as_of);
                    self.payment_rows.retain(|r| !matches!(r.target, PaymentTarget::PayPlan(pn) if pn == plan_num));
                    for a in allocs {
                        self.payment_rows.push(PaymentRow {
                            target: a.target,
                            amount_text: format!("{:.2}", a.amount),
                        });
                    }
                    if self.payment_rows.is_empty() {
                        self.payment_rows.push(PaymentRow { target: PaymentTarget::Unallocated, amount_text: String::new() });
                    }
                }

                ui.horizontal(|ui| {
                    if ui.button("Add split").clicked() {
                        self.payment_rows.push(PaymentRow {
                            target: PaymentTarget::Unallocated,
                            amount_text: String::new(),
                        });
                    }
                    if ui.button("Fill remaining").clicked() {
                        let allocated: f64 = self
                            .payment_rows
                            .iter()
                            .map(|r| r.amount_text.parse::<f64>().unwrap_or(0.0))
                            .sum();
                        let remaining = amount - allocated;
                        if let Some(row) = self.payment_rows.iter_mut().find(|r| {
                            r.amount_text.trim().is_empty()
                                || r.amount_text.parse::<f64>().unwrap_or(0.0) == 0.0
                        }) {
                            row.amount_text = format!("{remaining:.2}");
                        } else if remaining.abs() > 0.0 {
                            self.payment_rows.push(PaymentRow {
                                target: PaymentTarget::Unallocated,
                                amount_text: format!("{remaining:.2}"),
                            });
                        }
                    }
                });
                let allocated: f64 = self
                    .payment_rows
                    .iter()
                    .map(|r| r.amount_text.parse::<f64>().unwrap_or(0.0))
                    .sum();
                ui.label(format!(
                    "Allocated {allocated:.2} of {amount:.2} across {} split(s)",
                    self.payment_rows.len()
                ));
                let plan_charge_targets: Vec<_> = self.payment_rows.iter().filter(|r| matches!(r.target, PaymentTarget::PayPlanCharge { .. })).collect();
                if !plan_charge_targets.is_empty() {
                    let plan_unapplied: f64 = self.payment_rows.iter().filter(|r| matches!(r.target, PaymentTarget::PayPlan(_))).map(|r| r.amount_text.parse::<f64>().unwrap_or(0.0)).sum();
                    let msg = if plan_unapplied > 0.005 {
                        format!("Preview: {} charge-level split(s) (interest before principal); {:.2} remains unapplied to plan.", plan_charge_targets.len(), plan_unapplied)
                    } else {
                        format!("Preview: {} charge-level split(s) will be applied (interest before principal).", plan_charge_targets.len())
                    };
                    ui.label(msg);
                    egui::CollapsingHeader::new("Charge application preview")
                        .default_open(true)
                        .show(ui, |ui| {
                            egui::Grid::new("payment_charge_preview_grid")
                                .num_columns(4)
                                .striped(true)
                                .show(ui, |ui| {
                                    ui.strong("Plan");
                                    ui.strong("Charge");
                                    ui.strong("Debit");
                                    ui.strong("Amount");
                                    ui.end_row();
                                    for row in &self.payment_rows {
                                        if let PaymentTarget::PayPlanCharge { plan_num, charge_num } = row.target {
                                            let amt = row.amount_text.parse::<f64>().unwrap_or(0.0);
                                            let debit = "Allocated".to_string();
                                            ui.label(format!("#{}", plan_num));
                                            ui.label(format!("#{}", charge_num));
                                            ui.label(debit);
                                            ui.label(format!("{:.2}", amt));
                                            ui.end_row();
                                        }
                                    }
                                    if plan_unapplied > 0.005 {
                                        ui.label("—");
                                        ui.label("Unapplied");
                                        ui.label("Plan credit");
                                        ui.label(format!("{:.2}", plan_unapplied));
                                        ui.end_row();
                                    }
                                });
                        });
                }
                ui.horizontal(|ui| {
                    ui.label("Note");
                    ui.add(egui::TextEdit::singleline(&mut payment.note).desired_width(260.0).hint_text("Required for refunds/transfers"));
                });
                ui.label("Writes one OD payment row plus one paysplit row per allocation. Refunds/transfers require a note.");
                ui.separator();
                ui.horizontal(|ui| {
                    save = ui.button("Save").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });

        if save {
            payment.pay_date = NaiveDate::parse_from_str(&self.payment_date_text, "%Y-%m-%d")
                .unwrap_or(payment.pay_date);
            payment.pay_amount = self.payment_amount_text.parse().unwrap_or(0.0);
            let amount = payment.pay_amount;
            let allocations: Vec<PaymentAllocation> = self
                .payment_rows
                .iter()
                .map(|row| PaymentAllocation {
                    target: row.target,
                    amount: row.amount_text.parse().unwrap_or(0.0),
                    ..Default::default()
                })
                .collect();
            if let Err(error) = validate_payment_allocations(&payment, &allocations) {
                self.error = Some(error);
                self.payment_edit = Some(payment);
                self.payment_edit_open = true;
                return;
            }
            let split_count = allocations.len();
            match self.store.record_payment_splits(&payment, &allocations) {
                Ok(num) => {
                    let detail = allocations
                        .iter()
                        .map(|a| format!("{} {:.2}", a.target.label(), a.amount))
                        .collect::<Vec<_>>()
                        .join(", ");
                    self.audit_if(
                        FeatureKey::AuditBillingChanges,
                        Permission::PAYMENT_CREATE,
                        pat_num,
                        format!(
                            "Payment {num} recorded: {amount:.2} across {split_count} split(s) [{detail}]"
                        ),
                    );
                    self.notice = Some(format!("Payment and {split_count} paysplit(s) recorded"));
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
            self.payment_edit_open = false;
        } else if cancel {
            self.payment_edit_open = false;
        } else if self.payment_edit_open {
            self.payment_edit = Some(payment);
        }
    }

    fn plan_window(&mut self, ctx: &egui::Context) {
        let Some(mut plan) = self.plan_edit.take() else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        let frequency_label = self.plan_frequency.label().to_string();
        let tp_label = self.plan_tp_option.label().to_string();
        let is_new = plan.pay_plan_num == 0;
        let title = match (self.plan_is_dynamic, is_new) {
            (true, true) => "Dynamic payment plan — New",
            (true, false) => "Dynamic payment plan — Edit",
            (false, true) => "Payment plan — New",
            (false, false) => "Payment plan — Edit",
        };
        egui::Window::new(title)
            .open(&mut self.plan_edit_open)
            .resizable(true)
            .default_width(460.0)
            .show(ctx, |ui| {
                if is_new {
                    ui.checkbox(&mut self.plan_is_dynamic, "Dynamic plan (charge from linked production)");
                } else {
                    ui.label(if self.plan_is_dynamic { "Dynamic plan" } else { "Static plan" });
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.plan_is_closed, "Closed");
                        ui.checkbox(&mut self.plan_is_locked, "Locked");
                        ui.label("Close is blocked until due/future/unapplied amounts are resolved.");
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Amount ");
                    ui.text_edit_singleline(&mut self.plan_amount_text);
                    if self.plan_is_dynamic {
                        ui.label("(optional total cap)");
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Payments");
                    ui.text_edit_singleline(&mut self.plan_count_text);
                });
                ui.horizontal(|ui| {
                    ui.label("Note   ");
                    ui.text_edit_singleline(&mut self.plan_note_text);
                });
                ui.horizontal(|ui| {
                    ui.label("Start  ");
                    ui.add(egui::TextEdit::singleline(&mut self.plan_start_text).hint_text("YYYY-MM-DD"));
                });
                ui.horizontal(|ui| {
                    ui.label("APR %  ");
                    ui.text_edit_singleline(&mut self.plan_apr_text);
                    ui.label("Down");
                    ui.text_edit_singleline(&mut self.plan_down_payment_text);
                });
                ui.horizontal(|ui| {
                    ui.label("Interest from");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.plan_interest_start_text)
                            .hint_text("YYYY-MM-DD, blank = first payment"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Every  ");
                    egui::ComboBox::from_id_salt("plan_frequency")
                        .selected_text(&frequency_label)
                        .show_ui(ui, |ui| {
                            for freq in PayPlanFrequency::ALL {
                                ui.selectable_value(&mut self.plan_frequency, freq, freq.label());
                            }
                        });
                });
                if self.plan_is_dynamic {
                    ui.horizontal(|ui| {
                        ui.label("TP option");
                        egui::ComboBox::from_id_salt("plan_tp_option")
                            .selected_text(&tp_label)
                            .show_ui(ui, |ui| {
                                for opt in [
                                    DynamicPayPlanTPOption::None,
                                    DynamicPayPlanTPOption::AwaitComplete,
                                    DynamicPayPlanTPOption::TreatAsComplete,
                                ] {
                                    ui.selectable_value(&mut self.plan_tp_option, opt, opt.label());
                                }
                            });
                    });
                    ui.label("Dynamic plans issue charges from procedures/adjustments you link to the plan. Use 'Issue charges due' after linking production.");
                } else {
                    ui.label("APR is amortised from the interest-start date with OD's period rates. Leave APR at 0 for even, interest-free instalments.");
                }
                ui.separator();
                ui.horizontal(|ui| {
                    save = ui.button("Save").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if save {
            let amount: f64 = self.plan_amount_text.parse().unwrap_or(0.0);
            let count: i32 = self.plan_count_text.parse().unwrap_or(0);
            plan.number_of_payments = count;
            if self.plan_is_dynamic {
                plan.pay_amt = amount;
                if is_new {
                    plan.completed_amt = 0.0;
                }
            } else {
                plan.completed_amt = amount;
                plan.pay_amt = if count > 0 { amount / count as f64 } else { 0.0 };
            }
            plan.date_pay_plan_start =
                NaiveDate::parse_from_str(&self.plan_start_text, "%Y-%m-%d").ok();
            plan.apr = self.plan_apr_text.parse().unwrap_or(0.0);
            plan.down_payment = self.plan_down_payment_text.parse().unwrap_or(0.0);
            plan.date_interest_start =
                NaiveDate::parse_from_str(&self.plan_interest_start_text, "%Y-%m-%d").ok();
            plan.note = self.plan_note_text.clone();
            plan.charge_frequency = self.plan_frequency;
            plan.is_dynamic = self.plan_is_dynamic;
            plan.dynamic_pay_plan_tp_option = self.plan_tp_option;
            plan.is_closed = self.plan_is_closed;
            plan.is_locked = self.plan_is_locked;
            let pat_num = plan.pat_num;
            let was_new = plan.pay_plan_num == 0;
            if plan.is_closed && plan.pay_plan_num != 0 {
                let plan_charges = self.store.list_pay_plan_charges(pat_num).unwrap_or_default();
                let splits = self.store.list_pay_splits(pat_num).unwrap_or_default();
                let today = chrono::Local::now().date_naive();
                if let Some(reason) = od_core::pay_plan_close_block_reason(&plan, &plan_charges, &splits, today) {
                    self.error = Some(reason);
                    return;
                }
            }
            match self.store.upsert_pay_plan(&plan) {
                Ok(num) => {
                    plan.pay_plan_num = num;
                    if plan.is_dynamic {
                        self.audit_if(
                            FeatureKey::AuditBillingChanges,
                            Permission::BILLING,
                            pat_num,
                            format!(
                                "Dynamic payment plan {num} {}",
                                if was_new { "created" } else { "updated" }
                            ),
                        );
                        self.notice = Some(if was_new {
                            "Dynamic payment plan created. Link production, then issue charges due.".into()
                        } else {
                            "Payment plan updated".into()
                        });
                        self.error = None;
                    } else if was_new {
                        let generated =
                            od_core::generate_plan_charges(&plan, chrono::Local::now().date_naive());
                        let generated_count = generated.len();
                        let mut charge_error = None;
                        for charge in generated {
                            if let Err(e) = self.store.upsert_pay_plan_charge(&charge) {
                                charge_error = Some(e);
                                break;
                            }
                        }
                        self.audit_if(
                            FeatureKey::AuditBillingChanges,
                            Permission::BILLING,
                            pat_num,
                            format!("Payment plan {num} created with {generated_count} charge(s)"),
                        );
                        if let Some(e) = charge_error {
                            self.error = Some(e);
                        } else {
                            self.notice = Some("Payment plan and charges created".into());
                            self.error = None;
                        }
                    } else {
                        self.audit_if(
                            FeatureKey::AuditBillingChanges,
                            Permission::BILLING,
                            pat_num,
                            format!("Payment plan {num} updated"),
                        );
                        self.notice = Some("Payment plan updated".into());
                        self.error = None;
                    }
                }
                Err(e) => self.error = Some(e),
            }
            self.plan_edit_open = false;
        } else if cancel {
            self.plan_edit_open = false;
        } else if self.plan_edit_open {
            self.plan_edit = Some(plan);
        }
    }

    fn plan_charge_window(&mut self, ctx: &egui::Context) {
        let Some(mut charge) = self.plan_charge_edit.take() else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        egui::Window::new("Plan charge — New")
            .open(&mut self.plan_charge_edit_open)
            .resizable(true)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Due   ");
                    ui.add(egui::TextEdit::singleline(&mut self.plan_charge_date_text).hint_text("YYYY-MM-DD"));
                });
                ui.horizontal(|ui| {
                    ui.label("Amount");
                    ui.text_edit_singleline(&mut self.plan_charge_amount_text);
                });
                ui.label("A negative credit reduces the plan balance.");
                ui.separator();
                ui.horizontal(|ui| {
                    save = ui.button("Save").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if save {
            charge.charge_date =
                NaiveDate::parse_from_str(&self.plan_charge_date_text, "%Y-%m-%d")
                    .unwrap_or(charge.charge_date);
            charge.principal = self.plan_charge_amount_text.parse().unwrap_or(0.0);
            let pat_num = charge.pat_num;
            match self.store.upsert_pay_plan_charge(&charge) {
                Ok(num) => {
                    self.audit_if(
                        FeatureKey::AuditBillingChanges,
                        Permission::BILLING,
                        pat_num,
                        format!("Plan charge {num} recorded: {:.2}", charge.principal),
                    );
                    self.notice = Some("Plan charge recorded".into());
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
            self.plan_charge_edit_open = false;
        } else if cancel {
            self.plan_charge_edit_open = false;
        } else if self.plan_charge_edit_open {
            self.plan_charge_edit = Some(charge);
        }
    }

    /// OD `discountplan` editor. The reduced fee schedule and adjustment type
    /// are the two fields that affect procedure pricing and the generated
    /// adjustment; the rest are descriptive.
    fn discount_plan_window(&mut self, ctx: &egui::Context) {
        let Some(mut plan) = self.discount_plan_edit.take() else {
            return;
        };
        let is_new = plan.discount_plan_num == 0;
        let fee_schedules = self.store.list_fee_schedules().unwrap_or_default();
        let adj_types: Vec<Definition> = self
            .definitions
            .iter()
            .filter(|d| d.category == DefinitionCategory::ADJ_TYPES)
            .cloned()
            .collect();
        let mut save = false;
        let mut cancel = false;
        egui::Window::new(if is_new {
            "Discount plan — New"
        } else {
            "Discount plan — Edit"
        })
        .open(&mut self.discount_plan_edit_open)
        .resizable(true)
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Description");
                ui.text_edit_singleline(&mut plan.description);
            });
            egui::ComboBox::from_label("Fee schedule")
                    .selected_text(
                        fee_schedules
                            .iter()
                            .find(|f| f.fee_sched_num == plan.fee_sched_num)
                            .map(|f| f.description.as_str())
                            .unwrap_or("—"),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut plan.fee_sched_num, 0, "—");
                        for fee in &fee_schedules {
                            ui.selectable_value(
                                &mut plan.fee_sched_num,
                                fee.fee_sched_num,
                                &fee.description,
                            );
                        }
                    });
            egui::ComboBox::from_label("Adjustment type")
                    .selected_text(
                        adj_types
                            .iter()
                            .find(|d| d.def_num == plan.def_num)
                            .map(|d| d.item_name.as_str())
                            .unwrap_or("—"),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut plan.def_num, 0, "—");
                        for def in &adj_types {
                            ui.selectable_value(&mut plan.def_num, def.def_num, &def.item_name);
                        }
                    });
            ui.horizontal(|ui| {
                ui.label("Annual max");
                ui.add(egui::DragValue::new(&mut plan.annual_max).speed(1.0));
            });
            ui.checkbox(&mut plan.is_hidden, "Hidden from the plan list");
            ui.label("Plan note");
            ui.text_edit_multiline(&mut plan.note);
            ui.label(
                "The chosen fee schedule supplies the discounted fee; the difference from the \
                 usual fee is posted as an adjustment of the type above.",
            );
            ui.separator();
            ui.horizontal(|ui| {
                save = ui.button("Save").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if save {
            plan.description = plan.description.trim().to_string();
            if plan.description.is_empty() {
                self.error = Some("Discount plan description is required.".into());
                self.discount_plan_edit_open = true;
                self.discount_plan_edit = Some(plan);
                return;
            }
            match self.store.upsert_discount_plan(&plan) {
                Ok(num) => {
                    self.audit_if(
                        FeatureKey::AuditBillingChanges,
                        Permission::SETUP,
                        0,
                        format!("Discount plan {num} saved: {}", plan.description),
                    );
                    self.notice = Some(format!(
                        "Discount plan {} saved",
                        if is_new {
                            format!("#{num}")
                        } else {
                            num.to_string()
                        }
                    ));
                    self.error = None;
                    self.refresh();
                }
                Err(err) => self.error = Some(err),
            }
            self.discount_plan_edit_open = false;
        } else if cancel {
            self.discount_plan_edit_open = false;
        } else if self.discount_plan_edit_open {
            self.discount_plan_edit = Some(plan);
        }
    }

    /// OD `discountplansub` editor. Effective and term dates are inclusive; a
    /// blank term is OD's open-ended zero date.
    fn discount_sub_window(&mut self, ctx: &egui::Context) {
        let Some(mut sub) = self.discount_sub_edit.take() else {
            return;
        };
        let is_new = sub.discount_sub_num == 0;
        let plans = self.discount_plans.clone();
        let patients = self.patients.clone();
        let mut selected_plan = sub.discount_plan_num;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new(if is_new {
            "Discount subscription — New"
        } else {
            "Discount subscription — Edit"
        })
        .open(&mut self.discount_sub_edit_open)
        .resizable(true)
        .default_width(440.0)
        .show(ctx, |ui| {
            egui::ComboBox::from_label("Patient")
                    .selected_text(
                        patients
                            .iter()
                            .find(|p| p.pat_num == self.discount_sub_pat_num)
                            .map(|p| format!("{}, {}", p.l_name, p.f_name))
                            .unwrap_or_else(|| "Select a patient".into()),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.discount_sub_pat_num, 0, "—");
                        for patient in &patients {
                            ui.selectable_value(
                                &mut self.discount_sub_pat_num,
                                patient.pat_num,
                                format!(
                                    "{}, {} (#{})",
                                    patient.l_name, patient.f_name, patient.pat_num
                                ),
                            );
                        }
                    });
            egui::ComboBox::from_label("Plan")
                    .selected_text(
                        plans
                            .iter()
                            .find(|p| p.discount_plan_num == selected_plan)
                            .map(|p| p.description.as_str())
                            .unwrap_or("—"),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut selected_plan, 0, "—");
                        for plan in &plans {
                            ui.selectable_value(
                                &mut selected_plan,
                                plan.discount_plan_num,
                                &plan.description,
                            );
                        }
                    });
            ui.horizontal(|ui| {
                ui.label("Effective");
                ui.add(
                    egui::TextEdit::singleline(&mut self.discount_sub_effective_text)
                        .hint_text("YYYY-MM-DD"),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Term      ");
                ui.add(
                    egui::TextEdit::singleline(&mut self.discount_sub_term_text)
                        .hint_text("blank = open ended"),
                );
            });
            ui.label("Note");
            ui.text_edit_multiline(&mut sub.note);
            ui.label(
                "Both dates are inclusive. A blank term keeps discounting until you press End; \
                 Reopen clears the term again.",
            );
            ui.separator();
            ui.horizontal(|ui| {
                save = ui.button("Save").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if save {
            if self.discount_sub_pat_num == 0 {
                self.error = Some("Choose a patient for the subscription.".into());
                self.discount_sub_edit_open = true;
                self.discount_sub_edit = Some(sub);
                return;
            }
            if selected_plan == 0 {
                self.error = Some("Choose a discount plan.".into());
                self.discount_sub_edit_open = true;
                self.discount_sub_edit = Some(sub);
                return;
            }
            let effective = match parse_discount_date(&self.discount_sub_effective_text, "Effective date") {
                Ok(value) => value,
                Err(err) => {
                    self.error = Some(err);
                    self.discount_sub_edit_open = true;
                    self.discount_sub_edit = Some(sub);
                    return;
                }
            };
            let term = match parse_discount_date(&self.discount_sub_term_text, "Term date") {
                Ok(value) => value,
                Err(err) => {
                    self.error = Some(err);
                    self.discount_sub_edit_open = true;
                    self.discount_sub_edit = Some(sub);
                    return;
                }
            };
            if matches!((effective, term), (Some(start), Some(end)) if end < start) {
                self.error = Some("Term date must be on or after the effective date.".into());
                self.discount_sub_edit_open = true;
                self.discount_sub_edit = Some(sub);
                return;
            }
            sub.pat_num = self.discount_sub_pat_num;
            sub.discount_plan_num = selected_plan;
            sub.date_effective = effective;
            sub.date_term = term;
            let num = sub.discount_sub_num;
            let pat_num = sub.pat_num;
            match self.store.upsert_discount_plan_sub(&sub) {
                Ok(id) => {
                    self.audit_if(
                        FeatureKey::AuditBillingChanges,
                        Permission::SETUP,
                        pat_num,
                        format!("Discount subscription {id} saved"),
                    );
                    self.notice = Some(format!(
                        "Discount subscription {} saved",
                        if is_new {
                            format!("#{id}")
                        } else {
                            num.to_string()
                        }
                    ));
                    self.error = None;
                    self.refresh();
                }
                Err(err) => self.error = Some(err),
            }
            self.discount_sub_edit_open = false;
        } else if cancel {
            self.discount_sub_edit_open = false;
        } else if self.discount_sub_edit_open {
            self.discount_sub_edit = Some(sub);
        }
    }

    fn treat_plan_window(&mut self, ctx: &egui::Context) {
        let Some(mut plan) = self.treat_plan_edit.take() else {
            return;
        };
        let is_new = plan.treat_plan_num == 0;
        let mut save = false;
        let mut cancel = false;
        let status_label = self.treat_plan_status.label().to_string();
        egui::Window::new(if is_new {
            "Treatment plan — New"
        } else {
            "Treatment plan — Edit"
        })
        .open(&mut self.treat_plan_edit_open)
        .resizable(true)
        .default_width(440.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Heading ");
                ui.text_edit_singleline(&mut self.treat_plan_heading_text);
            });
            ui.horizontal(|ui| {
                ui.label("Date    ");
                ui.add(
                    egui::TextEdit::singleline(&mut self.treat_plan_date_text)
                        .hint_text("YYYY-MM-DD"),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Status  ");
                egui::ComboBox::from_id_salt("treat_plan_status")
                    .selected_text(&status_label)
                    .show_ui(ui, |ui| {
                        for status in TreatPlanStatus::ALL {
                            ui.selectable_value(
                                &mut self.treat_plan_status,
                                status,
                                status.label(),
                            );
                        }
                    });
            });
            ui.label("Note");
            ui.text_edit_multiline(&mut self.treat_plan_note_text);
            ui.label("OD keeps one Active plan per patient; promoting this plan demotes the others.");
            ui.separator();
            ui.horizontal(|ui| {
                save = ui.button("Save").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if save {
            plan.heading = self.treat_plan_heading_text.trim().to_string();
            plan.date_saved = NaiveDate::parse_from_str(&self.treat_plan_date_text, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0));
            plan.status = self.treat_plan_status;
            plan.note = self.treat_plan_note_text.clone();
            if let Err(err) = od_core::validate_treat_plan(&plan) {
                self.error = Some(err);
                self.treat_plan_edit_open = true;
                self.treat_plan_edit = Some(plan);
                return;
            }
            let pat_num = plan.pat_num;
            let plan_num = plan.treat_plan_num;
            let status = plan.status;
            match self.store.upsert_treat_plan(&plan) {
                Ok(num) => {
                    // Apply the one-Active-plan rule using the state before
                    // this write, so an existing Active plan is demoted.
                    let existing = self.store.list_treat_plans(pat_num).unwrap_or_default();
                    let mut failed = None;
                    for other in od_core::set_status(&existing, plan_num, status) {
                        if other.treat_plan_num == num {
                            continue;
                        }
                        if let Err(err) = self.store.upsert_treat_plan(&other) {
                            failed = Some(err);
                            break;
                        }
                    }
                    self.audit_if(
                        FeatureKey::AuditTreatmentPlans,
                        Permission::TREAT_PLAN_EDIT,
                        pat_num,
                        format!("Treatment plan {num} saved as {}", status.label()),
                    );
                    if let Some(err) = failed {
                        self.error = Some(err);
                    } else {
                        self.notice = Some(format!(
                            "Treatment plan {} saved",
                            if is_new { format!("#{num}") } else { format!("{num}") }
                        ));
                        self.error = None;
                    }
                }
                Err(err) => self.error = Some(err),
            }
            self.treat_plan_edit_open = false;
        } else if cancel {
            self.treat_plan_edit_open = false;
        } else if self.treat_plan_edit_open {
            self.treat_plan_edit = Some(plan);
        }
    }

    fn treat_plan_line_window(&mut self, ctx: &egui::Context) {
        let Some(mut line) = self.treat_plan_line_edit.take() else {
            return;
        };
        let Some(plan) = self
            .store
            .list_treat_plans(self.selected_patient.unwrap_or(0))
            .unwrap_or_default()
            .into_iter()
            .find(|p| p.treat_plan_num == self.treat_plan_line_plan_num)
        else {
            self.treat_plan_line_edit_open = false;
            return;
        };
        let saved = plan.status == TreatPlanStatus::Saved;
        let procedures = self.store.list_procedures(plan.pat_num).unwrap_or_default();
        let selected = if self.treat_plan_line_proc_num == 0 {
            "(select procedure)".to_string()
        } else {
            format!(
                "#{} — code #{}",
                self.treat_plan_line_proc_num,
                procedures
                    .iter()
                    .find(|p| p.proc_num == self.treat_plan_line_proc_num)
                    .map(|p| p.code_num)
                    .unwrap_or(0)
            )
        };
        let mut save = false;
        let mut cancel = false;
        egui::Window::new(if line.line_num == 0 {
            "Treatment plan line — New"
        } else {
            "Treatment plan line — Edit"
        })
        .open(&mut self.treat_plan_line_edit_open)
        .resizable(true)
        .default_width(400.0)
        .show(ctx, |ui| {
            egui::ComboBox::from_id_salt("treat_plan_line_proc")
                .selected_text(&selected)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_value(&mut self.treat_plan_line_proc_num, 0, "(none)")
                        .clicked()
                    {
                        self.treat_plan_line_proc_num = 0;
                    }
                    for proc in &procedures {
                        let text = format!("#{} — code #{}", proc.proc_num, proc.code_num);
                        if ui
                            .selectable_value(
                                &mut self.treat_plan_line_proc_num,
                                proc.proc_num,
                                text,
                            )
                            .clicked()
                        {
                            self.treat_plan_line_proc_num = proc.proc_num;
                            self.treat_plan_line_code_text = format!("#{}", proc.code_num);
                            self.treat_plan_line_desc_text = format!("Procedure #{}", proc.proc_num);
                            self.treat_plan_line_tooth_text = proc.tooth_num.clone();
                            self.treat_plan_line_fee_text = format!("{:.2}", proc.fee);
                        }
                    }
                });
            if saved {
                ui.horizontal(|ui| {
                    ui.label("Code");
                    ui.text_edit_singleline(&mut self.treat_plan_line_code_text);
                });
                ui.horizontal(|ui| {
                    ui.label("Desc");
                    ui.text_edit_singleline(&mut self.treat_plan_line_desc_text);
                });
                ui.horizontal(|ui| {
                    ui.label("Tooth");
                    ui.text_edit_singleline(&mut self.treat_plan_line_tooth_text);
                });
                ui.horizontal(|ui| {
                    ui.label("Fee  ");
                    ui.text_edit_singleline(&mut self.treat_plan_line_fee_text);
                });
            } else {
                ui.label(format!("Code: {}", line.proc_code));
                ui.label(format!("Description: {}", line.description));
                ui.label(format!("Tooth: {}", line.tooth_num));
                ui.label(format!("Fee: {:.2}", line.fee));
                ui.label("Active/Inactive plans store only the procedure link in OD; text comes from procedurelog.");
            }
            ui.separator();
            ui.horizontal(|ui| {
                save = ui.button("Save").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if save {
            line.proc_num = self.treat_plan_line_proc_num;
            line.proc_code = self.treat_plan_line_code_text.clone();
            line.description = self.treat_plan_line_desc_text.clone();
            line.tooth_num = self.treat_plan_line_tooth_text.clone();
            line.fee = self.treat_plan_line_fee_text.parse().unwrap_or(0.0);
            line.treat_plan_num = plan.treat_plan_num;
            line.pat_num = plan.pat_num;
            let pat_num = plan.pat_num;
            let line_num = line.line_num;
            match self.store.upsert_treat_plan_line(&plan, &line) {
                Ok(num) => {
                    self.audit_if(
                        FeatureKey::AuditTreatmentPlans,
                        Permission::TREAT_PLAN_EDIT,
                        pat_num,
                        format!(
                            "Treatment plan line {} saved on plan {} (procedure {})",
                            num,
                            plan.treat_plan_num,
                            line.proc_num
                        ),
                    );
                    self.notice = Some("Treatment plan line saved".into());
                    self.error = None;
                    if line_num == 0 {
                        line.line_num = num;
                    }
                }
                Err(err) => self.error = Some(err),
            }
            self.treat_plan_line_edit_open = false;
        } else if cancel {
            self.treat_plan_line_edit_open = false;
        } else if self.treat_plan_line_edit_open {
            self.treat_plan_line_edit = Some(line);
        }
    }

    fn patient_window(&mut self, ctx: &egui::Context) {
        let Some(mut patient) = self.patient_edit.take() else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        let providers = self.providers.clone();
        let is_existing = patient.pat_num != 0;
        let history_on = self.features.enabled(FeatureKey::PatientCorrectionHistory);

        egui::Window::new("Patient — Edit / Correct")
            .open(&mut self.patient_edit_open)
            .resizable(true)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("First");
                    ui.text_edit_singleline(&mut patient.f_name);
                });
                ui.horizontal(|ui| {
                    ui.label("Last ");
                    ui.text_edit_singleline(&mut patient.l_name);
                });
                ui.horizontal(|ui| {
                    ui.label("Middle");
                    ui.text_edit_singleline(&mut patient.middle_i);
                });
                ui.horizontal(|ui| {
                    ui.label("Title ");
                    ui.text_edit_singleline(&mut patient.title);
                });
                ui.horizontal(|ui| {
                    ui.label("Birth ");
                    ui.add(egui::TextEdit::singleline(&mut self.birth_text).hint_text("YYYY-MM-DD"));
                });
                ui.horizontal(|ui| {
                    ui.label("Phone ");
                    ui.text_edit_singleline(&mut patient.home_phone);
                });
                ui.horizontal(|ui| {
                    ui.label("Mobile");
                    ui.text_edit_singleline(&mut patient.wireless_phone);
                });
                ui.horizontal(|ui| {
                    ui.label("Email ");
                    ui.text_edit_singleline(&mut patient.email);
                });
                ui.horizontal(|ui| {
                    ui.label("Address");
                    ui.text_edit_singleline(&mut patient.address);
                });
                ui.horizontal(|ui| {
                    ui.label("City   ");
                    ui.text_edit_singleline(&mut patient.city);
                });
                ui.horizontal(|ui| {
                    ui.label("State  ");
                    ui.text_edit_singleline(&mut patient.state);
                });
                ui.horizontal(|ui| {
                    ui.label("Zip    ");
                    ui.text_edit_singleline(&mut patient.zip);
                });
                ui.horizontal(|ui| {
                    ui.label("Gender");
                    egui::ComboBox::from_id_salt("gender")
                        .selected_text(patient.gender.label())
                        .show_ui(ui, |ui| {
                            for gender in Gender::ALL {
                                ui.selectable_value(
                                    &mut patient.gender,
                                    gender,
                                    gender.label(),
                                );
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Prov  ");
                    let current = providers
                        .iter()
                        .find(|p| p.provider_num == patient.primary_provider_num)
                        .map(|p| p.abbreviation.clone())
                        .unwrap_or_else(|| "-".into());
                    egui::ComboBox::from_id_salt("patient_provider")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut patient.primary_provider_num, 0, "-");
                            for provider in &providers {
                                ui.selectable_value(
                                    &mut patient.primary_provider_num,
                                    provider.provider_num,
                                    format!("{} — {}, {}", provider.abbreviation, provider.l_name, provider.f_name),
                                );
                            }
                        });
                });
                if history_on && is_existing {
                    ui.separator();
                    ui.label("Correction reason (saved with the before-snapshot):");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.patient_reason_text)
                            .desired_rows(2)
                            .hint_text("e.g. migrated DOB was wrong"),
                    );
                }
                ui.separator();
                ui.horizontal(|ui| {
                    save = ui.button("Save").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });

        if save {
            if self.birth_text.trim().is_empty() {
                patient.birth_date = None;
            } else {
                match NaiveDate::parse_from_str(self.birth_text.trim(), "%Y-%m-%d") {
                    Ok(date) => patient.birth_date = Some(date),
                    Err(e) => {
                        self.error = Some(format!("Invalid birth date: {e}. Use YYYY-MM-DD."));
                        self.patient_edit = Some(patient);
                        self.patient_edit_open = true;
                        return;
                    }
                }
            }

            let before = if is_existing {
                self.store.get_patient(patient.pat_num).ok().flatten()
            } else {
                None
            };
            if history_on && is_existing {
                let mut correction = PatientCorrection {
                    pat_num: patient.pat_num,
                    reason: self.patient_reason_text.trim().to_string(),
                    before: before.clone().unwrap_or_else(|| patient.clone()),
                    ..Default::default()
                };
                correction.user_num = self.session.as_ref().map(|s| s.user_num).unwrap_or(0);
                if let Err(e) = self.store.append_patient_correction(&correction) {
                    self.error = Some(e);
                    self.patient_edit = Some(patient);
                    self.patient_edit_open = true;
                    return;
                }
            }

            match self.store.upsert_patient(&patient) {
                Ok(saved_num) => {
                    let (permission, action) = if !is_existing {
                        (Permission::PATIENT_CREATE, "created")
                    } else {
                        (Permission::PATIENT_EDIT, "corrected")
                    };
                    let note = match &before {
                        Some(before) => patient_change_note(before, &patient),
                        None => format!("Patient {saved_num} {action}"),
                    };
                    self.audit_if(FeatureKey::AuditPatientChanges, permission, saved_num, note);
                    self.notice = Some(if is_existing {
                        "Patient corrected; previous details saved for rollback".into()
                    } else {
                        "Patient saved".into()
                    });
                    self.error = None;
                    self.patient_reason_text.clear();
                    self.refresh();
                }
                Err(e) => self.error = Some(e),
            }
            self.patient_edit_open = false;
        } else if cancel {
            self.patient_edit_open = false;
        } else if self.patient_edit_open {
            self.patient_edit = Some(patient);
        }
    }

    fn recall_window(&mut self, ctx: &egui::Context) {
        let Some(mut recall) = self.recall_edit.take() else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        let mut asap = recall.priority == RecallPriority::Asap;
        let patients = self.patients.clone();

        egui::Window::new("Recall — Edit")
            .open(&mut self.recall_edit_open)
            .resizable(true)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Patient");
                    let current = patient_name(&patients, recall.pat_num);
                    egui::ComboBox::from_id_salt("recall_patient")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for patient in &patients {
                                ui.selectable_value(
                                    &mut recall.pat_num,
                                    patient.pat_num,
                                    patient.display_name(),
                                );
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Due date");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.recall_due_text)
                            .hint_text("YYYY-MM-DD (blank = none)"),
                    );
                });
                ui.label("Recall interval (OD packs these into one integer)");
                ui.horizontal(|ui| {
                    ui.label("Years");
                    ui.add(egui::TextEdit::singleline(&mut self.recall_years_text).desired_width(40.0));
                    ui.label("Months");
                    ui.add(egui::TextEdit::singleline(&mut self.recall_months_text).desired_width(40.0));
                    ui.label("Weeks");
                    ui.add(egui::TextEdit::singleline(&mut self.recall_weeks_text).desired_width(40.0));
                    ui.label("Days");
                    ui.add(egui::TextEdit::singleline(&mut self.recall_days_text).desired_width(40.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Status #");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.recall_status_text)
                            .desired_width(60.0)
                            .hint_text("0"),
                    );
                    ui.weak("definition.DefNum; definitions are a later slice");
                });
                ui.horizontal(|ui| {
                    ui.label("Note   ");
                    ui.text_edit_singleline(&mut recall.note);
                });
                ui.checkbox(&mut recall.is_disabled, "Disabled");
                ui.checkbox(&mut asap, "ASAP priority");
                ui.separator();
                ui.horizontal(|ui| {
                    save = ui.button("Save").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });

        if save {
            recall.date_due = NaiveDate::parse_from_str(&self.recall_due_text, "%Y-%m-%d").ok();
            recall.date_due_calc = recall.date_due;
            recall.recall_interval = interval_from_parts(
                self.recall_years_text.parse().unwrap_or(0),
                self.recall_months_text.parse().unwrap_or(0),
                self.recall_weeks_text.parse().unwrap_or(0),
                self.recall_days_text.parse().unwrap_or(0),
            );
            recall.recall_status = self.recall_status_text.parse().unwrap_or(0);
            recall.priority = if asap { RecallPriority::Asap } else { RecallPriority::Normal };
            self.save_recall(&recall);
            self.recall_edit_open = false;
        } else if cancel {
            self.recall_edit_open = false;
        } else if self.recall_edit_open {
            self.recall_edit = Some(recall);
        }
    }

    fn appointment_window(&mut self, ctx: &egui::Context) {
        let Some(mut apt) = self.apt_edit.take() else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        let mut delete = false;
        let mut asap = apt.priority == RecallPriority::Asap;
        let patients = self.patients.clone();
        let waitlist_defs: Vec<Definition> = waitlist_status_definitions(&self.definitions)
            .into_iter()
            .cloned()
            .collect();
        let is_existing = apt.apt_num != 0;
        let before = if is_existing {
            self.store.get_appointment(apt.apt_num).ok().flatten()
        } else {
            None
        };
        let history_on = self.features.enabled(FeatureKey::AppointmentHistory);
        let can_delete_appointment = self.features.enabled(FeatureKey::AppointmentDelete)
            && self.can(Permission::APPOINTMENT_DELETE);

        egui::Window::new("Appointment — Edit")
            .open(&mut self.apt_edit_open)
            .resizable(true)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("When  ");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.apt_text).hint_text("YYYY-MM-DD HH:MM"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Minutes");
                    ui.text_edit_singleline(&mut self.apt_len_text);
                });
                ui.horizontal(|ui| {
                    ui.label("Status");
                    egui::ComboBox::from_id_salt("apt_status")
                        .selected_text(apt.apt_status.label())
                        .show_ui(ui, |ui| {
                            for status in AppointmentStatus::ALL {
                                ui.selectable_value(&mut apt.apt_status, status, status.label());
                            }
                        });
                });
                if apt.apt_status == AppointmentStatus::UnschedList {
                    ui.horizontal(|ui| {
                        ui.label("Waitlist");
                        let selected = waitlist_status_name(&self.definitions, apt.waitlist_status);
                        egui::ComboBox::from_id_salt("waitlist_status")
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                if ui.selectable_value(&mut apt.waitlist_status, 0, "—").clicked() {
                                    // No change needed; selectable_value writes the value.
                                }
                                for definition in &waitlist_defs {
                                    ui.selectable_value(
                                        &mut apt.waitlist_status,
                                        definition.def_num,
                                        &definition.item_name,
                                    );
                                }
                            });
                    });
                    ui.weak("OD waitlist status = definition.DefNum in DefCat.RecallUnschedStatus (13).");
                }
                ui.horizontal(|ui| {
                    ui.label("Patient");
                    let current = patient_name(&patients, apt.pat_num);
                    egui::ComboBox::from_id_salt("apt_patient")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for patient in &patients {
                                ui.selectable_value(
                                    &mut apt.pat_num,
                                    patient.pat_num,
                                    patient.display_name(),
                                );
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Notes ");
                    ui.text_edit_singleline(&mut apt.notes);
                });
                ui.checkbox(&mut asap, "ASAP priority (appointment.Priority = 1)");
                ui.separator();
                if apt.apt_num != 0 && self.features.enabled(FeatureKey::AppointmentConflictWarnings) {
                    let others: Vec<Appointment> = self
                        .appointments
                        .iter()
                        .filter(|a| a.apt_num != apt.apt_num)
                        .cloned()
                        .collect();
                    let conflicts = schedule_conflicts(&apt, &others);
                    if conflicts.is_empty() {
                        ui.colored_label(Color32::from_rgb(20, 120, 40), "No schedule conflicts");
                    } else {
                        ui.colored_label(
                            Color32::from_rgb(190, 60, 60),
                            format!("Conflicts with appointment(s): {conflicts:?}"),
                        );
                    }
                }
                ui.horizontal(|ui| {
                    save = ui.button("Save").clicked();
                    cancel = ui.button("Cancel").clicked();
                    delete = apt.apt_num != 0
                        && can_delete_appointment
                        && ui.button("Delete").clicked();
                });
            });

        if delete {
            if history_on {
                let mut change = AppointmentChange::from_snapshot(
                    before.as_ref().unwrap_or(&apt),
                    AppointmentChangeAction::Deleted,
                );
                change.user_num = self.session.as_ref().map(|s| s.user_num).unwrap_or(0);
                if let Err(e) = self.store.append_appointment_change(&change) {
                    self.error = Some(e);
                    self.apt_edit = Some(apt);
                    self.apt_edit_open = true;
                    return;
                }
            }
            match self.store.delete_appointment(apt.apt_num) {
                Ok(()) => {
                    self.notice = Some("Appointment deleted".into());
                    self.audit_if(
                        FeatureKey::AuditAppointmentChanges,
                        Permission::APPOINTMENT_DELETE,
                        apt.pat_num,
                        format!("Appointment {} deleted", apt.apt_num),
                    );
                }
                Err(e) => self.error = Some(e),
            }
            self.apt_edit_open = false;
        } else if save {
            apt.apt_datetime = match parse_appointment_datetime(&self.apt_text) {
                Ok(when) => when,
                Err(e) => {
                    self.error = Some(e);
                    self.apt_edit = Some(apt);
                    self.apt_edit_open = true;
                    return;
                }
            };
            apt.apt_length = match parse_appointment_length(&self.apt_len_text) {
                Ok(length) => length,
                Err(e) => {
                    self.error = Some(e);
                    self.apt_edit = Some(apt);
                    self.apt_edit_open = true;
                    return;
                }
            };
            apt.pattern = pattern_for_length(apt.apt_length);
            apt.priority = if asap {
                RecallPriority::Asap
            } else {
                RecallPriority::Normal
            };
            if apt.apt_status != AppointmentStatus::UnschedList {
                apt.waitlist_status = 0;
            }
            if history_on && is_existing {
                let mut change = AppointmentChange::from_snapshot(
                    before.as_ref().unwrap_or(&apt),
                    AppointmentChangeAction::Changed,
                );
                change.user_num = self.session.as_ref().map(|s| s.user_num).unwrap_or(0);
                if let Err(e) = self.store.append_appointment_change(&change) {
                    self.error = Some(e);
                    self.apt_edit = Some(apt);
                    self.apt_edit_open = true;
                    return;
                }
            }
            match self.store.upsert_appointment(&apt) {
                Ok(saved_num) => {
                    if history_on && !is_existing {
                        let mut stored = apt.clone();
                        stored.apt_num = saved_num;
                        let mut change =
                            AppointmentChange::from_snapshot(&stored, AppointmentChangeAction::Created);
                        change.user_num = self.session.as_ref().map(|s| s.user_num).unwrap_or(0);
                        if let Err(e) = self.store.append_appointment_change(&change) {
                            self.error = Some(e);
                        }
                    }
                    let (permission, note) = if !is_existing {
                        (
                            Permission::APPOINTMENT_CREATE,
                            format!("Appointment {saved_num} created"),
                        )
                    } else {
                        (
                            Permission::APPOINTMENT_EDIT,
                            appointment_change_note(before.as_ref().unwrap_or(&apt), &apt),
                        )
                    };
                    self.audit_if(FeatureKey::AuditAppointmentChanges, permission, apt.pat_num, note);
                    self.notice = Some("Appointment saved".into());
                    self.error = None;
                    self.refresh();
                }
                Err(e) => self.error = Some(e),
            }
            self.apt_edit_open = false;
        } else if cancel {
            self.apt_edit_open = false;
        } else if self.apt_edit_open {
            self.apt_edit = Some(apt);
        }
    }

    fn patient_history_window(&mut self, ctx: &egui::Context) {
        if !self.patient_history_open {
            return;
        }
        let Some(pat_num) = self.patient_history_pat_num else {
            return;
        };
        let can_restore = self.features.enabled(FeatureKey::PatientCorrectionHistory)
            && self.can(Permission::PATIENT_EDIT);
        let rows = self.patient_corrections.clone();
        let mut restore: Option<PatientCorrection> = None;

        egui::Window::new("Patient — Correction history")
            .open(&mut self.patient_history_open)
            .resizable(true)
            .default_width(620.0)
            .show(ctx, |ui| {
                ui.label("Snapshots are saved before each edit. Restoring writes the earlier details back as a normal audited edit.");
                ui.separator();
                if rows.is_empty() {
                    ui.label("No corrections recorded for this patient.");
                    return;
                }
                egui::Grid::new("patient_correction_grid")
                    .num_columns(5)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("When");
                        ui.strong("Reason");
                        ui.strong("Previous name");
                        ui.strong("Previous DOB / phone");
                        ui.strong("Action");
                        ui.end_row();
                        for correction in &rows {
                            ui.label(correction.corrected_at.format("%Y-%m-%d %H:%M").to_string());
                            ui.label(if correction.reason.is_empty() {
                                "-"
                            } else {
                                &correction.reason
                            });
                            ui.label(correction.before.display_name());
                            ui.label(format!(
                                "{} / {}",
                                correction
                                    .before
                                    .birth_date
                                    .map(|d| d.to_string())
                                    .unwrap_or_default(),
                                correction.before.home_phone
                            ));
                            if can_restore && ui.button("Restore").clicked() {
                                restore = Some(correction.clone());
                            }
                            ui.end_row();
                        }
                    });
            });

        if let Some(correction) = restore {
            let current = self.store.get_patient(pat_num).ok().flatten();
            if let Some(current) = current {
                let mut rollback = PatientCorrection {
                    pat_num,
                    reason: format!("Rollback of correction {}", correction.correction_num),
                    before: current.clone(),
                    ..Default::default()
                };
                rollback.user_num = self.session.as_ref().map(|s| s.user_num).unwrap_or(0);
                if let Err(e) = self.store.append_patient_correction(&rollback) {
                    self.error = Some(e);
                    return;
                }
            }
            let mut restored = correction.before.clone();
            restored.pat_num = pat_num;
            match self.store.upsert_patient(&restored) {
                Ok(_) => {
                    self.audit_if(
                        FeatureKey::AuditPatientChanges,
                        Permission::PATIENT_EDIT,
                        pat_num,
                        format!("Patient {pat_num} rolled back to correction {}", correction.correction_num),
                    );
                    self.notice = Some("Patient details restored".into());
                    self.error = None;
                    self.refresh();
                    self.open_patient_history(pat_num);
                }
                Err(e) => self.error = Some(e),
            }
        }
    }

    fn appointment_history_window(&mut self, ctx: &egui::Context) {
        if !self.apt_history_open {
            return;
        }
        let Some(apt_num) = self.apt_history_num else {
            return;
        };
        let can_restore = self.features.enabled(FeatureKey::AppointmentHistory)
            && self.can(Permission::APPOINTMENT_EDIT);
        let definitions = self.definitions.clone();
        let rows = self.apt_history.clone();
        let mut restore: Option<AppointmentChange> = None;

        egui::Window::new("Appointment — History")
            .open(&mut self.apt_history_open)
            .resizable(true)
            .default_width(700.0)
            .show(ctx, |ui| {
                ui.label("OD histappointment snapshots. Restoring also works for an entry removed from the waitlist.");
                ui.separator();
                if rows.is_empty() {
                    ui.label("No history for this appointment.");
                    return;
                }
                egui::Grid::new("appointment_history_grid")
                    .num_columns(6)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("When");
                        ui.strong("Action");
                        ui.strong("Snapshot time");
                        ui.strong("Status");
                        ui.strong("Waitlist status");
                        ui.strong("Action");
                        ui.end_row();
                        for change in &rows {
                            ui.label(change.changed_at.format("%Y-%m-%d %H:%M").to_string());
                            ui.label(change.action.label());
                            ui.label(change.snapshot.apt_datetime.format("%Y-%m-%d %H:%M").to_string());
                            ui.label(change.snapshot.apt_status.label());
                            ui.label(waitlist_status_name(&definitions, change.snapshot.waitlist_status));
                            if can_restore && ui.button("Restore").clicked() {
                                restore = Some(change.clone());
                            }
                            ui.end_row();
                        }
                    });
            });

        if let Some(change) = restore {
            let current = self.store.get_appointment(apt_num).ok().flatten();
            if let Some(current) = current {
                let mut snapshot = AppointmentChange::from_snapshot(&current, AppointmentChangeAction::Changed);
                snapshot.user_num = self.session.as_ref().map(|s| s.user_num).unwrap_or(0);
                if let Err(e) = self.store.append_appointment_change(&snapshot) {
                    self.error = Some(e);
                    return;
                }
            }
            let restored = restore_appointment(&change);
            match self.store.upsert_appointment(&restored) {
                Ok(_) => {
                    self.audit_if(
                        FeatureKey::AuditAppointmentChanges,
                        Permission::APPOINTMENT_EDIT,
                        change.pat_num,
                        format!("Appointment {apt_num} restored from history {}", change.change_num),
                    );
                    self.notice = Some("Appointment restored".into());
                    self.error = None;
                    self.refresh();
                    self.open_appointment_history(apt_num);
                }
                Err(e) => self.error = Some(e),
            }
        }
    }
}

fn patient_name(patients: &[Patient], pat_num: Id) -> String {
    patients
        .iter()
        .find(|p| p.pat_num == pat_num)
        .map(|p| p.display_name())
        .unwrap_or_else(|| format!("Patient #{pat_num}"))
}

fn status_color(status: AppointmentStatus) -> Color32 {
    match status {
        AppointmentStatus::Scheduled => Color32::from_rgb(198, 222, 250),
        AppointmentStatus::Complete => Color32::from_rgb(200, 235, 204),
        AppointmentStatus::Broken => Color32::from_rgb(250, 205, 205),
        AppointmentStatus::Planned => Color32::from_rgb(228, 228, 228),
        AppointmentStatus::Unscheduled => Color32::from_rgb(255, 240, 200),
        AppointmentStatus::UnschedList => Color32::from_rgb(244, 240, 224),
        AppointmentStatus::Asap => Color32::from_rgb(255, 210, 160),
        AppointmentStatus::PtNote => Color32::from_rgb(255, 246, 190),
        AppointmentStatus::PtNoteCompleted => Color32::from_rgb(218, 238, 214),
    }
}

impl eframe::App for OdApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.render(ui);
    }
}

fn treat_plan_status_label(plan: &TreatPlan) -> &'static str {
    match plan.status {
        TreatPlanStatus::Active => "Active",
        TreatPlanStatus::Inactive => "Inactive",
        TreatPlanStatus::Saved => "Saved",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_core::{
        build_charge_posting_plan, build_posting_batch_plan, suggest_charge_ledger_accounts,
    };

    #[test]
    fn security_log_search_finds_stale_resolution_reason() {
        let event = SecurityEvent {
            security_log_num: 1,
            perm_type: Permission::BILLING,
            user_num: 7,
            log_datetime: chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap(),
            log_text: "Resolved 2 stale dynamic charge(s) for plan 8 (as of 2026-03-01, reason: fee corrected after chart review, blocked 0)".into(),
            pat_num: 1,
        };
        assert!(security_log_matches(&event, ""));
        assert!(security_log_matches(&event, "fee corrected"));
        assert!(security_log_matches(&event, "CHART REVIEW"));
        assert!(security_log_matches(&event, "billing"));
        assert!(security_log_matches(&event, "7"));
        assert!(!security_log_matches(&event, "unrelated reason"));

        assert_eq!(security_log_patient_filter(" 12 "), Some(12));
        assert_eq!(security_log_patient_filter("0"), None);
        assert_eq!(security_log_patient_filter("abc"), None);
    }

    #[test]
    fn security_log_export_keeps_reason_and_patient_for_audit_attachment() {
        let event = SecurityEvent {
            security_log_num: 42,
            perm_type: Permission::BILLING,
            user_num: 7,
            log_datetime: chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
                .unwrap()
                .and_hms_opt(9, 30, 0)
                .unwrap(),
            log_text: "Resolved 2 stale dynamic charge(s) for plan 8 (as of 2026-03-01, reason: fee corrected after chart review, blocked 0)".into(),
            pat_num: 123,
        };
        let table = security_log_export_table(&[&event]);
        assert_eq!(
            table.headers,
            vec!["Log #", "Time", "User", "Type", "Patient", "Details"]
        );
        assert_eq!(table.rows[0][4], "123");
        let csv = render_report_csv("Security log (filtered)", "Highland Dental", &table);
        assert!(csv.contains("Patient"));
        assert!(csv.contains("fee corrected after chart review"));
        assert!(csv.contains("123"));
        let html = render_report_html("Security log (filtered)", "Highland Dental", &table);
        assert!(html.contains("Patient"));
        assert!(html.contains("fee corrected after chart review"));
        assert!(html.contains("123"));
    }

    #[test]
    fn pay_plan_audit_export_keeps_only_stale_resolved_plans() {
        let mut table = ReportTable::new(&[
            "Provider",
            "Guarantor",
            "Balance",
            "Resolved",
            "Resolved reason",
        ]);
        table.push(&[
            "Smith, Jo",
            "Alvarez, Ada",
            "0.00",
            "0",
            "",
        ]);
        table.push(&[
            "Smith, Jo",
            "Bennett, Ada",
            "0.00",
            "2",
            "fee corrected after chart review; fee rechecked",
        ]);
        table.push(&[
            "Total (2 plan(s))",
            "",
            "0.00",
            "2",
            "",
        ]);

        let filtered = stale_resolved_export_table(&table);
        assert_eq!(filtered.headers, table.headers);
        assert_eq!(filtered.rows.len(), 1, "footer and clean plan are excluded");
        assert_eq!(filtered.rows[0][1], "Bennett, Ada");
        assert_eq!(
            filtered.rows[0][4],
            "fee corrected after chart review; fee rechecked"
        );

        let csv = render_report_csv("Payment plans (stale resolved)", "Highland Dental", &filtered);
        assert!(csv.contains("Resolved reason"));
        assert!(csv.contains("fee corrected after chart review"));
        assert!(!csv.contains("Alvarez"), "clean plan must not be exported");
        assert!(!csv.contains("Total ("), "footer must not be exported");
        let html =
            render_report_html("Payment plans (stale resolved)", "Highland Dental", &filtered);
        assert!(html.contains("fee corrected after chart review"));
        assert!(!html.contains("Alvarez"));
    }

    #[test]
    fn account_audit_export_bundles_reasons_and_patient_log_rows() {
        let when = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let plan = PayPlan {
            pay_plan_num: 8,
            pat_num: 12,
            guarantor: 12,
            note: "Crown plan".into(),
            ..Default::default()
        };
        let charges = [
            PayPlanCharge {
                pay_plan_charge_num: 44,
                pay_plan_num: 8,
                pat_num: 12,
                guarantor: 12,
                charge_date: when,
                charge_type: PayPlanChargeType::Debit,
                note: "[Stale resolved 2026-03-01: fee corrected after chart review]".into(),
                ..Default::default()
            },
            PayPlanCharge {
                pay_plan_charge_num: 45,
                pay_plan_num: 8,
                pat_num: 12,
                guarantor: 12,
                charge_date: when,
                charge_type: PayPlanChargeType::Debit,
                principal: 100.0,
                note: "ordinary charge".into(),
                ..Default::default()
            },
            PayPlanCharge {
                pay_plan_charge_num: 46,
                pay_plan_num: 9,
                pat_num: 99,
                guarantor: 99,
                charge_date: when,
                charge_type: PayPlanChargeType::Debit,
                note: "[Stale resolved 2026-03-01: another patient reason]".into(),
                ..Default::default()
            },
        ];
        let log = [
            SecurityEvent {
                security_log_num: 7,
                perm_type: Permission::BILLING,
                user_num: 3,
                log_datetime: when.and_hms_opt(10, 30, 0).unwrap(),
                log_text: "Resolved 1 stale dynamic charge(s) for plan 8 (as of 2026-03-01, reason: fee corrected after chart review, blocked 0)".into(),
                pat_num: 12,
            },
            SecurityEvent {
                security_log_num: 8,
                perm_type: Permission::BILLING,
                user_num: 3,
                log_datetime: when.and_hms_opt(11, 0, 0).unwrap(),
                log_text: "other patient audit row".into(),
                pat_num: 99,
            },
        ];

        let table = account_audit_export_table(12, "Alvarez, Ada", &[plan], &charges, &log);
        assert_eq!(
            table.headers,
            vec!["Section", "When", "Patient", "Plan #", "Source #", "User", "Details"]
        );
        assert_eq!(table.rows.len(), 2, "one resolved charge and one matching log row");
        assert_eq!(table.rows[0][0], "Resolved charge");
        assert_eq!(table.rows[0][4], "44");
        assert!(table.rows[0][6].contains("reason: fee corrected after chart review"));
        assert!(table.rows[0][6].contains("Crown plan"), "plan note adds context");
        assert_eq!(table.rows[1][0], "Security log");
        assert_eq!(table.rows[1][4], "7");
        assert_eq!(table.rows[1][5], "3");
        assert!(table.rows[1][6].contains("reason: fee corrected after chart review"));

        let title = "Account audit — Alvarez, Ada (patient #12)";
        let csv = render_report_csv(title, "Highland Dental", &table);
        assert!(csv.contains("Account audit"));
        assert!(csv.contains("patient #12"));
        assert!(csv.contains("fee corrected after chart review"));
        assert!(!csv.contains("another patient reason"));
        assert!(!csv.contains("other patient audit row"));
        let html = render_report_html(title, "Highland Dental", &table);
        assert!(html.contains("patient #12"));
        assert!(html.contains("fee corrected after chart review"));
        assert!(!html.contains("another patient reason"));
        assert!(!html.contains("other patient audit row"));

        assert!(
            default_account_audit_export_path(12).contains("account-audit-patient-12.csv"),
            "default filename must carry the patient number"
        );
    }

    /// Dedicated audit/export paths are read from the persisted preferences,
    /// with per-patient keys keeping two accounts' filing locations separate.
    #[test]
    fn dedicated_export_paths_load_from_preferences() {
        let mut prefs = ReportPreferences::default();
        prefs.set_export_path(EXPORT_KEY_ACCOUNT_AUDIT, "C:\\Audit\\practice.csv");
        prefs.set_user_export_path("local", EXPORT_KEY_ACCOUNT_AUDIT, "C:\\Audit\\account.csv");
        prefs.set_user_export_path(
            "local",
            patient_export_key(EXPORT_KEY_ACCOUNT_AUDIT, 12),
            "C:\\Audit\\patient-12.csv",
        );
        prefs.set_user_export_path("local", EXPORT_KEY_ACCOUNT_LEDGER, "C:\\Audit\\ledger.csv");
        prefs.set_user_export_path("local", EXPORT_KEY_PAY_PLAN_AUDIT, "C:\\Audit\\plans.csv");
        prefs.set_export_path(EXPORT_KEY_SECURITY_LOG, "C:\\Audit\\security.csv");
        let prefs = ReportPreferences::from_text(&prefs.to_text());

        assert_eq!(
            persisted_export_path(&prefs, "local", EXPORT_KEY_PAY_PLAN_AUDIT),
            "C:\\Audit\\plans.csv"
        );
        assert_eq!(
            persisted_export_path(&prefs, "local", EXPORT_KEY_ACCOUNT_LEDGER),
            "C:\\Audit\\ledger.csv"
        );
        assert_eq!(
            persisted_export_path(&prefs, "local", EXPORT_KEY_SECURITY_LOG),
            "C:\\Audit\\security.csv",
            "a user without their own path inherits the practice-wide default"
        );
        assert_eq!(
            persisted_export_path(&prefs, "user-1", EXPORT_KEY_ACCOUNT_AUDIT),
            "C:\\Audit\\practice.csv"
        );
        assert_eq!(
            patient_export_path(&prefs, "local", EXPORT_KEY_ACCOUNT_AUDIT, 12, "fallback".into()),
            "C:\\Audit\\patient-12.csv"
        );
        assert_eq!(
            patient_export_path(&prefs, "local", EXPORT_KEY_ACCOUNT_AUDIT, 99, "fallback".into()),
            "C:\\Audit\\account.csv",
            "an account without its own path inherits the user's default"
        );
        assert_eq!(
            patient_export_path(&ReportPreferences::default(), "local", EXPORT_KEY_ACCOUNT_AUDIT, 99, "fallback".into()),
            "fallback"
        );
        assert_eq!(
            security_log_export_key(Some(12)),
            patient_export_key(EXPORT_KEY_SECURITY_LOG, 12)
        );
        assert_eq!(security_log_export_key(None), EXPORT_KEY_SECURITY_LOG);
    }

    #[test]
    fn report_kind_file_stems_round_trip() {
        for kind in ReportKind::ALL {
            assert_eq!(ReportKind::from_file_stem(kind.file_stem()), Some(kind));
        }
        assert_eq!(ReportKind::from_file_stem("not-a-report"), None);
    }

    #[test]
    fn loading_user_view_restores_saved_report_kind() {
        let mut app = OdApp::new();
        let key = app.period_close_history_user_key();
        app.report = ReportKind::Production;
        app.report_prefs
            .set_selected_report_kind(key.clone(), ReportKind::Aging.file_stem());
        app.load_report_user_view();
        assert_eq!(app.report, ReportKind::Aging);
        assert_eq!(
            app.report_prefs.selected_report_kind(&key),
            Some(ReportKind::Aging.file_stem())
        );
    }

    #[test]
    fn export_paths_follow_the_signed_in_user() {
        let mut app = OdApp::new();
        // GUI verification writes real preferences to disk; start clean so this
        // test does not inherit a saved report kind from another session.
        // `OdApp::new()` already loaded that kind into `self.report`, so reset
        // both the preferences and the live selection.
        app.report_prefs = ReportPreferences::default();
        app.report = ReportKind::Production;
        let key = app.period_close_history_user_key();
        app.report_prefs.set_user_export_path(
            key.clone(),
            ReportKind::Production.file_stem(),
            "C:\\Reports\\mine.csv",
        );
        app.load_report_user_view();
        assert_eq!(app.report_export_path, "C:\\Reports\\mine.csv");

        app.report_prefs
            .set_user_export_path(key.clone(), "account-audit.12", "D:\\Audit\\12.csv");
        assert_eq!(
            app.report_prefs.effective_export_path(&key, "account-audit.12"),
            Some("D:\\Audit\\12.csv")
        );
    }

    #[test]
    fn report_sort_follows_the_signed_in_user_and_report() {
        let mut app = OdApp::new();
        let key = app.period_close_history_user_key();
        app.report_prefs.set_report_sort(
            key.clone(),
            ReportKind::Production.file_stem(),
            ReportSortState {
                column: 2,
                descending: true,
            },
        );
        app.report_prefs.set_report_sort(
            key,
            ReportKind::Collections.file_stem(),
            ReportSortState {
                column: 4,
                descending: false,
            },
        );

        app.load_report_sort(ReportKind::Production);
        assert_eq!(app.report_sort_column, 2);
        assert!(app.report_sort_desc);
        app.load_report_sort(ReportKind::Collections);
        assert_eq!(app.report_sort_column, 4);
        assert!(!app.report_sort_desc);
    }

    #[test]
    fn collections_band_storage_mapping_round_trips_every_choice() {
        for band in [
            CollectionsAgingBand::All,
            CollectionsAgingBand::Current,
            CollectionsAgingBand::Days31To60,
            CollectionsAgingBand::Days61To90,
            CollectionsAgingBand::Over90,
        ] {
            assert_eq!(
                collections_band_to_storage(collections_band_from_storage(band)),
                band
            );
        }
    }

    #[test]
    fn collections_export_title_records_the_active_aging_band() {
        assert_eq!(collections_export_title(None), "Collections (All bands)");
        for (bucket, expected) in [
            (
                CollectionsAgingBucket::Current,
                "Collections (0-30 days band)",
            ),
            (
                CollectionsAgingBucket::Days31To60,
                "Collections (31-60 days band)",
            ),
            (
                CollectionsAgingBucket::Days61To90,
                "Collections (61-90 days band)",
            ),
            (
                CollectionsAgingBucket::Over90,
                "Collections (90+ days band)",
            ),
        ] {
            assert_eq!(collections_export_title(Some(bucket)), expected);
        }

        // Both saved formats carry the band in their header, not just on screen.
        let table = ReportTable {
            headers: vec!["Patient".into(), "Balance".into()],
            rows: vec![vec!["Chen, Mei".into(), "99.00".into()]],
        };
        let title = collections_export_title(Some(CollectionsAgingBucket::Days61To90));
        let csv = render_report_csv(&title, "Highland Dental", &table);
        assert!(csv.contains("Collections (61-90 days band)"), "{csv}");
        let html = render_report_html(&title, "Highland Dental", &table);
        assert!(html.contains("Collections (61-90 days band)"), "{html}");
    }

    #[test]
    fn collections_aging_band_follows_the_signed_in_user() {
        let mut app = OdApp::new();
        // Ignore bands saved by an earlier run so a fresh user really has none.
        app.report_prefs = ReportPreferences::default();
        let local_key = app.period_close_history_user_key();
        app.collections_bucket_filter = Some(CollectionsAgingBucket::Over90);
        app.persist_collections_bucket();
        assert_eq!(
            app.report_prefs.collections_bucket(&local_key),
            CollectionsAgingBand::Over90
        );

        app.session = Some(Session {
            user_num: 7,
            user_name: "aharrison".into(),
            permissions: Permission::ALL.to_vec(),
        });
        let named_key = app.period_close_history_user_key();
        assert_eq!(named_key, "user-7");
        app.switch_report_user_view(&local_key);
        assert_eq!(
            app.collections_bucket_filter, None,
            "a user with no saved band starts unfiltered"
        );

        app.collections_bucket_filter = Some(CollectionsAgingBucket::Current);
        app.persist_collections_bucket();
        app.session = None;
        app.switch_report_user_view(&named_key);
        assert_eq!(
            app.collections_bucket_filter,
            Some(CollectionsAgingBucket::Over90),
            "the local user's band comes back after logout"
        );
        assert_eq!(
            app.report_prefs.collections_bucket(&named_key),
            CollectionsAgingBand::Current
        );
    }

    #[test]
    fn collections_aging_band_survives_preferences_restart() {
        let mut app = OdApp::new();
        app.report_prefs = ReportPreferences::default();
        app.session = Some(Session {
            user_num: 7,
            user_name: "aharrison".into(),
            permissions: Permission::ALL.to_vec(),
        });
        app.collections_bucket_filter = Some(CollectionsAgingBucket::Days61To90);
        app.persist_collections_bucket();

        let text = app.report_prefs.to_text();
        assert!(text.contains("collections_bucket.user-7=61-90"));
        app.report_prefs = ReportPreferences::from_text(&text);
        app.collections_bucket_filter = None;
        app.load_report_user_view();
        assert_eq!(
            app.collections_bucket_filter,
            Some(CollectionsAgingBucket::Days61To90)
        );
    }

    /// Editable report dates mirror the live range and reject partial or
    /// reversed input without changing the dates that drive the reports.
    #[test]
    fn report_date_text_fields_validate_and_mirror_dates() {
        let mut app = OdApp::new();
        let from = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 2, 20).unwrap();
        app.show_report_dates(from, to);
        assert_eq!(app.report_from_text, "2026-01-15");
        assert_eq!(app.report_to_text, "2026-02-20");
        assert!(app.report_date_text_valid(true));

        app.report_from_text = "2026-03-01".into();
        assert!(!app.report_date_text_valid(true), "reversed range is rejected");
        app.report_to_text = "not-a-date".into();
        assert!(!app.report_date_text_valid(true), "partial text is rejected");
        assert_eq!(app.report_from, from);
        assert_eq!(app.report_to, to);

        // "As of" reports hide the start field; a valid end still applies and
        // clamps the hidden start so the persisted window stays ordered.
        app.report_from_text = "not-a-date".into();
        app.report_to_text = "2026-01-10".into();
        assert!(app.report_date_text_valid(false));
        let (from, to) = app.parsed_report_dates(false).unwrap();
        assert_eq!(from, NaiveDate::from_ymd_opt(2026, 1, 10).unwrap());
        assert_eq!(to, NaiveDate::from_ymd_opt(2026, 1, 10).unwrap());
        app.show_report_dates(from, to);
        assert_eq!(app.report_from_text, "2026-01-10");
    }

    /// The account ledger keeps the source adjustment number so its
    /// "Post to ledger" action can open the right adjustment for posting.
    #[test]
    fn account_ledger_rows_expose_the_adjustment_number() {
        let adj = Adjustment {
            adj_num: 42,
            adj_date: chrono::NaiveDate::from_ymd_opt(2026, 11, 23).unwrap(),
            adj_amount: -15.0,
            note: "Courtesy".into(),
            ..Default::default()
        };
        let rows = build_account_ledger_rows(&[], &[adj.clone()], &[], &[], true);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "Adj");
        assert_eq!(rows[0].adj_num, 42);
        assert_eq!(rows[0].credit, 15.0);
        assert!(build_account_ledger_rows(&[], &[adj], &[], &[], false).is_empty());
    }

    /// Posting is a first-class, default-on toggle and writes both the journal
    /// pair and an auditable security-log entry through the real app state.
    #[test]
    fn gui_ledger_posting_writes_journal_pair_and_audit_entry() {
        let mut app = OdApp::new();
        assert!(
            app.features.enabled(FeatureKey::LedgerJournalPosting),
            "ledger posting starts on"
        );
        assert_eq!(
            FeatureKey::from_config_key("ledger_journal_posting"),
            Some(FeatureKey::LedgerJournalPosting)
        );
        let accounts = app.store.list_accounts().unwrap();
        let journal_before = app.store.list_journal_entries().unwrap();
        let adj = Adjustment {
            adj_num: 77,
            adj_date: chrono::NaiveDate::from_ymd_opt(2026, 11, 23).unwrap(),
            adj_amount: -9.5,
            pat_num: app.patients[0].pat_num,
            note: "GUI courtesy".into(),
            ..Default::default()
        };
        let (debit, credit) = suggest_ledger_accounts(&accounts, adj.adj_amount).unwrap();
        let transaction_num = next_journal_transaction_num(&journal_before);
        let mut entries = build_adjustment_journal(&adj, debit, credit, transaction_num).unwrap();
        for entry in &mut entries {
            entry.memo = "GUI posted memo".into();
        }
        validate_balanced_journal(&entries).unwrap();
        assert_eq!(
            app.store.post_journal_transaction(&entries).unwrap(),
            transaction_num
        );
        app.audit_if(
            FeatureKey::AuditBillingChanges,
            Permission::ADJUSTMENT_CREATE,
            adj.pat_num,
            format!(
                "Adjustment #{} posted to journal transaction #{}",
                adj.adj_num, transaction_num
            ),
        );

        let all_entries = app.store.list_journal_entries().unwrap();
        let posted: Vec<&JournalEntry> = all_entries
            .iter()
            .filter(|e| e.transaction_num == transaction_num)
            .collect();
        assert_eq!(posted.len(), 2);
        assert!(posted.iter().all(|e| e.memo == "GUI posted memo"));
        assert!(app
            .store
            .list_security_log(50)
            .unwrap()
            .iter()
            .any(|e| e.log_text.contains("posted to journal")));
    }

    /// Adjustments already in the journal must not offer a second posting, and
    /// a custom memo note must never strip the reference used for that check.
    #[test]
    fn gui_ledger_posting_refuses_a_duplicate_adjustment() {
        use od_core::adjustment_journal_reference;
        let mut app = OdApp::new();
        let accounts = app.store.list_accounts().unwrap();
        let adj = Adjustment {
            adj_num: 88,
            adj_date: chrono::NaiveDate::from_ymd_opt(2026, 11, 23).unwrap(),
            adj_amount: -6.25,
            pat_num: app.patients[0].pat_num,
            note: "First post".into(),
            ..Default::default()
        };
        let (debit, credit) = suggest_ledger_accounts(&accounts, adj.adj_amount).unwrap();
        let journal_before = app.store.list_journal_entries().unwrap();
        let transaction_num = next_journal_transaction_num(&journal_before);
        let mut entries = build_adjustment_journal(&adj, debit, credit, transaction_num).unwrap();
        let memo = adjustment_journal_memo_with_note(&adj, "edited by reception");
        assert!(memo.contains(&adjustment_journal_reference(88, adj.pat_num)));
        for entry in &mut entries {
            entry.memo = memo.clone();
        }
        assert!(!is_adjustment_posted(&journal_before, 88, adj.pat_num));
        app.store.post_journal_transaction(&entries).unwrap();

        let after = app.store.list_journal_entries().unwrap();
        assert!(is_adjustment_posted(&after, 88, adj.pat_num));
        assert_eq!(
            journal_transaction_for_adjustment(&after, 88, adj.pat_num),
            Some(transaction_num)
        );

        // The window's guard: a second attempt is rejected before any write.
        if is_adjustment_posted(
            &app.store.list_journal_entries().unwrap_or_default(),
            adj.adj_num,
            adj.pat_num,
        ) {
            app.error = Some(format!(
                "Adjustment #{} is already posted to the ledger; nothing was written.",
                adj.adj_num
            ));
        }
        assert!(app.error.is_some_and(|e| e.contains("already posted")));
        let after_second = app.store.list_journal_entries().unwrap();
        assert_eq!(after_second.len(), after.len(), "no duplicate lines written");
    }

    /// Payment-backed ledger rows carry the payment number so the Action
    /// column can offer one posting per payment, even with several splits.
    #[test]
    fn account_ledger_rows_expose_the_payment_number() {
        use od_domain::{PaySplit, Payment};
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
        let payment = Payment {
            pay_num: 71,
            pat_num: 1,
            pay_date: date,
            pay_amount: 40.0,
            pay_type: 0,
            check_num: "CHQ-71".into(),
            note: "Desk cash".into(),
        };
        let split = PaySplit {
            pay_num: 71,
            pat_num: 1,
            split_amount: 40.0,
            date_pay: date,
            ..Default::default()
        };
        let rows = build_account_ledger_rows(&[], &[], &[split], &[payment], true);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "Payment");
        assert_eq!(rows[0].pay_num, 71);
        assert_eq!(rows[0].credit, 40.0);
    }

    /// The shared source handles both record types: sign-based defaults,
    /// balanced pairs and a stable reference that survives note edits.
    #[test]
    fn journal_post_source_handles_adjustments_and_payments() {
        use od_core::payment_journal_reference;
        let app = OdApp::new();
        let accounts = app.store.list_accounts().unwrap();
        let pat_num = app.patients[0].pat_num;
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
        let adj_source = JournalPostSource::Adjustment(Adjustment {
            adj_num: 91,
            adj_date: date,
            adj_amount: 12.0,
            pat_num,
            note: "Charge".into(),
            ..Default::default()
        });
        let pay_source = JournalPostSource::Payment(Payment {
            pay_num: 92,
            pat_num,
            pay_date: date,
            pay_amount: -20.0,
            pay_type: 0,
            check_num: "CHQ-92".into(),
            note: "Refund".into(),
        });
        assert_eq!(adj_source.label(), "Adjustment");
        assert_eq!(pay_source.label(), "Payment");
        assert_eq!((adj_source.source_num(), pay_source.source_num()), (91, 92));
        assert_eq!((adj_source.pat_num(), pay_source.pat_num()), (pat_num, pat_num));
        let (adj_debit, adj_credit) = adj_source.default_accounts(&accounts);
        let (pay_debit, pay_credit) = pay_source.default_accounts(&accounts);
        assert_ne!(adj_debit, adj_credit);
        assert_ne!(pay_debit, pay_credit);
        let adj_entries = adj_source.build(adj_debit, adj_credit, 1).unwrap();
        let pay_entries = pay_source.build(pay_debit, pay_credit, 2).unwrap();
        validate_balanced_journal(&adj_entries).unwrap();
        validate_balanced_journal(&pay_entries).unwrap();
        assert!(adj_entries[0].memo.contains("[Adj #91"));
        assert!(pay_entries[0].memo.contains("[Pay #92"));
        assert_eq!(pay_entries[0].check_number, "CHQ-92");
        assert!(pay_source
            .memo("edited")
            .contains(&payment_journal_reference(92, pat_num)));
        assert!(pay_source.heading().contains("cheque CHQ-92"));
        assert!(!adj_source.is_posted(&pay_entries));
        assert!(!pay_source.is_posted(&adj_entries));
    }

    /// Posting a patient payment writes a balanced pair through the real app
    /// state, audits it, and refuses a second posting for the same payment.
    #[test]
    fn gui_payment_journal_posting_writes_pair_audit_and_guard() {
        let mut app = OdApp::new();
        let accounts = app.store.list_accounts().unwrap();
        let pat_num = app.patients[0].pat_num;
        let source = JournalPostSource::Payment(Payment {
            pay_num: 93,
            pat_num,
            pay_date: chrono::NaiveDate::from_ymd_opt(2026, 11, 24).unwrap(),
            pay_amount: 55.0,
            pay_type: 0,
            check_num: "CHQ-55".into(),
            note: "Desk cash".into(),
        });
        let (debit, credit) = source.default_accounts(&accounts);
        let journal_before = app.store.list_journal_entries().unwrap();
        let transaction_num = next_journal_transaction_num(&journal_before);
        let mut entries = source.build(debit, credit, transaction_num).unwrap();
        let memo = source.memo("till 2");
        for entry in &mut entries {
            entry.memo = memo.clone();
        }
        assert_eq!(
            app.store.post_journal_transaction(&entries).unwrap(),
            transaction_num
        );
        app.audit_if(
            FeatureKey::AuditBillingChanges,
            Permission::ADJUSTMENT_CREATE,
            pat_num,
            format!("Payment #93 posted to journal transaction #{transaction_num}"),
        );

        let all = app.store.list_journal_entries().unwrap();
        let posted: Vec<&JournalEntry> = all
            .iter()
            .filter(|e| e.transaction_num == transaction_num)
            .collect();
        assert_eq!(posted.len(), 2);
        assert!(posted
            .iter()
            .all(|e| e.check_number == "CHQ-55" && e.memo.contains("[Pay #93")));
        assert!(is_payment_posted(&all, 93, pat_num));
        assert!(app
            .store
            .list_security_log(50)
            .unwrap()
            .iter()
            .any(|e| e.log_text.contains("Payment #93 posted to journal")));

        // Once posted, the window's guard rejects a duplicate before any write.
        assert!(source.is_posted(&app.store.list_journal_entries().unwrap_or_default()));
        assert_eq!(
            app.store.list_journal_entries().unwrap().len(),
            all.len(),
            "no duplicate payment lines written"
        );
    }

    /// A batch run writes every balanced pair in one call, audits each touched
    /// patient, and cannot book the same charge twice on a second run.
    #[test]
    fn gui_charge_batch_writes_pairs_audits_and_guards_duplicates() {
        use od_core::is_procedure_posted;
        use od_domain::{Procedure, ProcedureStatus};
        let mut app = OdApp::new();
        let accounts = app.store.list_accounts().unwrap();
        let (debit, credit) = suggest_charge_ledger_accounts(&accounts).unwrap();
        let pat = app.patients[0].pat_num;
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
        let procedures: Vec<Procedure> = (0..3)
            .map(|i| Procedure {
                proc_num: 500 + i,
                pat_num: pat,
                proc_date: date,
                proc_status: ProcedureStatus::Complete,
                fee: 25.0 + i as f64,
                code_num: 3,
                description_override: format!("Test filling {i}"),
                ..Default::default()
            })
            .collect();
        let before = app.store.list_journal_entries().unwrap();
        let plan = build_charge_posting_plan(&procedures, &before, debit, credit);
        assert_eq!(plan.len(), 3);
        let batches: Vec<Vec<JournalEntry>> = plan.iter().map(|c| c.entries.to_vec()).collect();
        let txns = app.store.post_journal_batch(&batches).unwrap();
        assert_eq!(txns.len(), 3);

        let all = app.store.list_journal_entries().unwrap();
        assert_eq!(all.len(), before.len() + 6, "two lines per charge");
        for charge in &plan {
            let posted: Vec<&JournalEntry> = all
                .iter()
                .filter(|e| e.transaction_num == charge.transaction_num)
                .collect();
            assert_eq!(posted.len(), 2);
            assert!(posted.iter().any(|e| e.debit_amt == charge.procedure.fee
                && e.account_num == debit));
            assert!(posted.iter().any(|e| e.credit_amt == charge.procedure.fee
                && e.account_num == credit));
            assert!(is_procedure_posted(&all, charge.procedure.proc_num, pat));
        }
        app.audit_if(
            FeatureKey::AuditBillingChanges,
            Permission::ADJUSTMENT_CREATE,
            pat,
            "Batch posted 3 completed-procedure charge(s)".into(),
        );
        assert!(app
            .store
            .list_security_log(50)
            .unwrap()
            .iter()
            .any(|e| e.log_text.contains("Batch posted 3")));

        // A second run sees every procedure as posted and books nothing.
        let after = app.store.list_journal_entries().unwrap();
        assert!(build_charge_posting_plan(&procedures, &after, debit, credit).is_empty());
        assert_eq!(app.store.list_journal_entries().unwrap().len(), all.len());
    }

    /// One reviewed batch posts charges, payments and adjustments together in a
    /// single DB transaction, audits each touched patient, and is idempotent on
    /// a second run — so the ledger tie-out can be a one-step period-close gate.
    #[test]
    fn gui_post_all_batch_writes_every_source_and_guards_duplicates() {
        use od_core::{is_adjustment_posted, is_payment_posted, is_procedure_posted};
        use od_domain::{Adjustment, Payment, Procedure, ProcedureStatus};

        let mut app = OdApp::new();
        let defaults = PostingAccountDefaults::from_chart(&app.store.list_accounts().unwrap());
        assert!(defaults.is_complete());
        let pat = app.patients[0].pat_num;
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
        let from = chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let to = chrono::NaiveDate::from_ymd_opt(2026, 11, 30).unwrap();
        let procedures = vec![Procedure {
            proc_num: 600,
            pat_num: pat,
            proc_date: date,
            proc_status: ProcedureStatus::Complete,
            fee: 60.0,
            code_num: 3,
            description_override: "Test filling".into(),
            ..Default::default()
        }];
        let payments = vec![Payment {
            pay_num: 70,
            pat_num: pat,
            pay_date: date,
            pay_amount: 40.0,
            pay_type: 0,
            ..Default::default()
        }];
        let adjustments = vec![Adjustment {
            adj_num: 80,
            adj_date: date,
            adj_amount: -5.0,
            pat_num: pat,
            ..Default::default()
        }];
        let before = app.store.list_journal_entries().unwrap();
        let plan = build_posting_batch_plan(
            &procedures,
            &payments,
            &adjustments,
            &before,
            &defaults,
            from,
            to,
        );
        assert_eq!(plan.len(), 3);
        let batches = build_journal_batches(&plan).unwrap();
        assert_eq!(batches.len(), 3);
        let txns = app.store.post_journal_batch(&batches).unwrap();
        assert_eq!(txns.len(), 3);

        let all = app.store.list_journal_entries().unwrap();
        assert_eq!(all.len(), before.len() + 6, "two lines per source");
        for item in &plan {
            let posted: Vec<&JournalEntry> = all
                .iter()
                .filter(|e| e.transaction_num == item.transaction_num)
                .collect();
            assert_eq!(posted.len(), 2);
            assert!(posted.iter().any(|e| e.debit_amt == item.source.amount().abs()
                && e.account_num == item.debit_account));
            assert!(posted.iter().any(|e| e.credit_amt == item.source.amount().abs()
                && e.account_num == item.credit_account));
        }
        assert!(is_procedure_posted(&all, 600, pat));
        assert!(is_payment_posted(&all, 70, pat));
        assert!(is_adjustment_posted(&all, 80, pat));

        app.audit_if(
            FeatureKey::AuditBillingChanges,
            Permission::ADJUSTMENT_CREATE,
            pat,
            "Batch posted 3 source(s) to journal".into(),
        );
        assert!(app
            .store
            .list_security_log(50)
            .unwrap()
            .iter()
            .any(|e| e.log_text.contains("Batch posted 3 source(s)")));

        let after = app.store.list_journal_entries().unwrap();
        assert!(build_posting_batch_plan(
            &procedures,
            &payments,
            &adjustments,
            &after,
            &defaults,
            from,
            to,
        )
        .is_empty());
        assert_eq!(app.store.list_journal_entries().unwrap().len(), all.len());
    }

    /// A reconciled month can be closed once, persists in the preference table,
    /// is audited, and locks later sources until it is reopened (also audited).
    #[test]
    fn gui_period_close_persists_audits_and_locks_posting() {
        use od_domain::{Adjustment, Payment, Procedure, ProcedureStatus};

        let mut app = OdApp::new();
        let accounts = app.store.list_accounts().unwrap();
        let defaults = PostingAccountDefaults::from_chart(&accounts);
        let pat = app.patients[0].pat_num;
        let date = NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
        let from = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 11, 30).unwrap();
        let procedure = Procedure {
            proc_num: 700,
            pat_num: pat,
            proc_date: date,
            proc_status: ProcedureStatus::Complete,
            fee: 60.0,
            code_num: 3,
            ..Default::default()
        };
        let payment = Payment {
            pay_num: 71,
            pat_num: pat,
            pay_date: date,
            pay_amount: 40.0,
            pay_type: 0,
            ..Default::default()
        };
        let adjustment = Adjustment {
            adj_num: 81,
            adj_date: date,
            adj_amount: -5.0,
            pat_num: pat,
            ..Default::default()
        };
        let tie_out = |journal: &[JournalEntry]| {
            let mut payments = Vec::new();
            payments.push(payment.clone());
            ledger_tie_out_report(
                &[procedure.clone()],
                &payments,
                &[adjustment.clone()],
                &[],
                journal,
                &accounts,
                from,
                to,
            )
        };
        assert!(tie_out_ready(&tie_out(&app.store.list_journal_entries().unwrap())).is_err());

        let before = app.store.list_journal_entries().unwrap();
        let plan = build_posting_batch_plan(
            &[procedure.clone()],
            &[payment.clone()],
            &[adjustment.clone()],
            &before,
            &defaults,
            from,
            to,
        );
        let batches = build_journal_batches(&plan).unwrap();
        app.store.post_journal_batch(&batches).unwrap();
        let after = app.store.list_journal_entries().unwrap();
        assert!(tie_out_ready(&tie_out(&after)).is_ok());

        let mut closed = app.closed_periods.clone();
        closed.close(date, "Signed off after tie-out", "now".into()).unwrap();
        app.store
            .upsert_pref(CLOSED_PERIODS_PREF, &closed.to_json())
            .unwrap();
        app.closed_periods = closed;
        app.audit(
            Permission::ADJUSTMENT_CREATE,
            0,
            "Closed accounting period 2026-11: Signed off after tie-out".into(),
        );
        assert_eq!(
            ClosedPeriods::from_prefs(&app.store.list_prefs().unwrap()),
            app.closed_periods
        );
        assert!(app
            .store
            .list_security_log(50)
            .unwrap()
            .iter()
            .any(|e| e.log_text.contains("Closed accounting period 2026-11")));

        let late = Procedure {
            proc_num: 701,
            fee: 30.0,
            ..procedure.clone()
        };
        assert!(app.closed_periods.is_closed(late.proc_date));
        assert!(app
            .closed_periods
            .reject_posting(date, "Charge #701")
            .unwrap_err()
            .contains("2026-11"));
        assert_eq!(
            build_posting_batch_plan(
                &[late.clone()],
                &[],
                &[],
                &after,
                &defaults,
                from,
                to,
            )
            .len(),
            1
        );
        assert!(build_posting_batch_plan_with_closed(
            &[late],
            &[],
            &[],
            &after,
            &defaults,
            &app.closed_periods,
            from,
            to,
        )
        .is_empty());

        let mut reopened = app.closed_periods.clone();
        reopened.reopen(date).unwrap();
        app.store
            .upsert_pref(CLOSED_PERIODS_PREF, &reopened.to_json())
            .unwrap();
        app.closed_periods = reopened;
        app.audit(
            Permission::ADJUSTMENT_CREATE,
            0,
            "Reopened accounting period 2026-11 for a late correction".into(),
        );
        assert!(!app.closed_periods.is_closed(date));
        assert!(app
            .store
            .list_security_log(50)
            .unwrap()
            .iter()
            .any(|e| e.log_text.contains("Reopened accounting period 2026-11")));
    }

    /// A batch containing a malformed transaction group is refused up front,
    /// leaving the ledger untouched.
    #[test]
    fn gui_charge_batch_refuses_a_malformed_group_without_writing() {
        let app = OdApp::new();
        let before = app.store.list_journal_entries().unwrap();
        let bad = vec![
            vec![JournalEntry {
                transaction_num: 900,
                ..Default::default()
            }],
            vec![
                JournalEntry {
                    transaction_num: 901,
                    ..Default::default()
                },
                JournalEntry {
                    transaction_num: 902,
                    ..Default::default()
                },
            ],
        ];
        assert!(app.store.post_journal_batch(&bad).is_err());
        assert_eq!(app.store.list_journal_entries().unwrap().len(), before.len());
    }

    fn collections_test_row(
        pat_num: Id,
        patient: &str,
        days_overdue: i64,
        balance: f64,
    ) -> CollectionsRow {
        CollectionsRow {
            pat_num,
            patient: patient.into(),
            balance,
            days_overdue,
            last_payment: None,
            plan_due_now: 0.0,
            phone: String::new(),
        }
    }

    #[test]
    fn collections_worklist_demo_rows_have_clean_money_formatting() {
        let app = OdApp::new();
        let patients = &app.patients;
        let procedures = gather_procedures(&*app.store, patients);
        let adjustments = gather_adjustments(&*app.store, patients);
        let splits = gather_splits(&*app.store, patients);
        let plans = gather_pay_plans(&*app.store, patients);
        let plan_charges = gather_pay_plan_charges(&*app.store, patients);
        let rows = collections_worklist(
            patients,
            &procedures,
            &adjustments,
            &splits,
            &plans,
            &plan_charges,
            app.report_to,
        );
        assert!(!rows.is_empty(), "demo data should have overdue accounts");
        for row in &rows {
            // A paid-off plan must never render as "-0.00" in the worklist.
            assert!(
                row.plan_due_now >= 0.0,
                "{} has a negative plan due: {:?}",
                row.patient,
                row.plan_due_now
            );
            assert!(
                !row.as_table_row().iter().any(|cell| cell.starts_with("-0.00")),
                "{} has a negative-zero cell: {:?}",
                row.patient,
                row.as_table_row()
            );
        }
        let totals = collections_bucket_totals(&rows);
        assert_eq!(
            totals.iter().map(|t| t.patients).sum::<usize>(),
            rows.len(),
            "aging bands must cover every demo patient exactly once"
        );
        for bucket in CollectionsAgingBucket::ALL {
            let filtered = filter_collections_rows(&rows, Some(bucket));
            let balance: f64 = filtered.iter().map(|row| row.balance).sum();
            assert_eq!(filtered.len(), totals[bucket.index()].patients);
            assert!((balance - totals[bucket.index()].balance).abs() < 0.005);
        }
    }

    #[test]
    fn collections_worklist_sort_keeps_action_rows_aligned() {
        let mut rows = vec![
            CollectionsRow {
                pat_num: 1,
                patient: "Alvarez, Ada".into(),
                balance: 50.0,
                days_overdue: 30,
                last_payment: None,
                plan_due_now: 0.0,
                phone: String::new(),
            },
            CollectionsRow {
                pat_num: 2,
                patient: "Bennett, Ada".into(),
                balance: 200.0,
                days_overdue: 61,
                last_payment: None,
                plan_due_now: 0.0,
                phone: String::new(),
            },
        ];
        sort_collections_rows(&mut rows, 1, false);
        assert_eq!(rows[0].pat_num, 1, "50 sorts before 200 numerically");
        sort_collections_rows(&mut rows, 2, true);
        assert_eq!(rows[0].pat_num, 2, "61 sorts before 30 when descending");
        assert_eq!(rows[0].as_table_row()[2], "61", "visible row follows the action");
    }

    #[test]
    fn collections_aging_filter_keeps_export_and_actions_aligned() {
        let mut app = OdApp::new();
        app.collections_rows = vec![
            collections_test_row(1, "Alvarez, Ada", 91, 100.0),
            collections_test_row(2, "Bennett, Bo", 30, 50.0),
            collections_test_row(3, "Chen, Cai", 61, 25.0),
            collections_test_row(4, "Diaz, Dee", 60, 75.0),
        ];
        let totals = collections_bucket_totals(&app.collections_rows);
        assert_eq!(totals[CollectionsAgingBucket::Current.index()].patients, 1);
        assert_eq!(
            totals[CollectionsAgingBucket::Days61To90.index()].balance,
            25.0
        );
        assert_eq!(totals[CollectionsAgingBucket::Over90.index()].balance, 100.0);

        app.collections_bucket_filter = Some(CollectionsAgingBucket::Days61To90);
        sort_collections_rows(&mut app.collections_rows, 1, false);
        let visible =
            filter_collections_rows(&app.collections_rows, app.collections_bucket_filter);
        assert_eq!(visible.len(), 1, "one row for the one patient in the band");
        assert_eq!(visible[0].pat_num, 3);
        let exported = visible[0].as_table_row();
        assert_eq!(exported[0], "Chen, Cai");
        assert_eq!(exported[1], "25.00");
        assert_eq!(exported[2], "61");
        assert_eq!(app.collections_rows[0].patient, exported[0]);
        assert_eq!(filter_collections_rows(&app.collections_rows, None).len(), 4);
    }

    #[test]
    fn collections_worklist_actions_open_account_and_payment_for_one_patient() {
        let mut app = OdApp::new();
        app.refresh();
        // The worklist honours the same feature/permission gates as Account.
        app.features.set(FeatureKey::AccountBillingEdits, true);
        app.session = Some(Session::local());
        let patient = app.patients.first().expect("demo patients").clone();
        app.collections_rows = vec![CollectionsRow {
            pat_num: patient.pat_num,
            patient: patient.display_name(),
            balance: 125.0,
            days_overdue: 45,
            last_payment: None,
            plan_due_now: 0.0,
            phone: String::new(),
        }];

        app.module = Module::Reports;
        app.apply_collections_action(patient.pat_num, CollectionsAction::OpenAccount);
        assert_eq!(app.module, Module::Account);
        assert_eq!(app.selected_patient, Some(patient.pat_num));

        app.apply_collections_action(patient.pat_num, CollectionsAction::RecordPayment);
        assert!(app.payment_edit_open, "payment dialog should open from worklist");
        assert_eq!(
            app.payment_edit.as_ref().map(|payment| payment.pat_num),
            Some(patient.pat_num),
            "payment must stay on the clicked individual patient"
        );
        assert_eq!(app.payment_rows.len(), 1);
    }

    /// Headless smoke test: the full app constructs, loads demo data and the
    /// eServices slice is wired into the same state as every other module.
    #[test]
    fn gui_app_constructs_and_refreshes() {
        let mut app = OdApp::new();
        app.refresh();
        assert!(!app.patients.is_empty(), "demo patients should load");
        assert!(!app.appointments.is_empty(), "demo appointments should load");
        assert_eq!(app.waitlist.len(), 2, "waitlist should load from OD unsched list");
        assert!(app.waitlist.iter().all(|a| is_waitlist(a)));
        assert_eq!(app.appt_tab, ApptTab::Schedule);
        assert_eq!(app.reminder_rules.len(), 3, "eServices rules should load");
        assert_eq!(app.patient_contacts.len(), app.patients.len());
        assert_eq!(app.eservices_tab, EServicesTab::Channels);
        assert!(!app.definitions.is_empty(), "definitions should load");
        assert!(!app.display_fields.is_empty(), "display fields should load");
        assert_eq!(app.definitions_tab, DefinitionsTab::Definitions);
        assert!(app.features.enabled(FeatureKey::WaitlistEnabled));
        assert!(app.features.enabled(FeatureKey::PatientCorrectionHistory));
        assert!(app.features.enabled(FeatureKey::AppointmentHistory));
        assert!(app.features.enabled(FeatureKey::PatientRegistry));
        assert!(app.features.enabled(FeatureKey::ClinicCardsEnabled));
        assert!(app.features.enabled(FeatureKey::A5Invoice));
        assert!(app.features.enabled(FeatureKey::ReportRecallAsap));
        assert!(app.features.enabled(FeatureKey::ReportPaymentPlans));
        assert!(app.features.enabled(FeatureKey::ReportTreatmentPlans));
        assert!(app.features.enabled(FeatureKey::ReportAging));
        assert!(app.features.enabled(FeatureKey::ReportPayPlanDetail));
        assert!(app.features.enabled(FeatureKey::ReportPayPlanChargeApplications));
        assert!(app.features.enabled(FeatureKey::ReportBirthday));
        assert!(app.features.enabled(FeatureKey::ReportNewPatients));
        assert!(app.features.enabled(FeatureKey::ReportAppointments));
        assert!(app.features.enabled(FeatureKey::ReportActivePatients));
        assert!(app.features.enabled(FeatureKey::ReportProdInc));
        assert!(app.features.enabled(FeatureKey::ReportPaySheet));
        assert!(app.features.enabled(FeatureKey::ReportAdjSheet));
        assert!(app.features.enabled(FeatureKey::ReportFinanceCharge));
        assert!(app.features.enabled(FeatureKey::ReportBrokenAppointments));
        assert!(app.features.enabled(FeatureKey::ReportConfirmations));
        assert!(app.features.enabled(FeatureKey::ReportRecallFields));
        assert!(app.features.enabled(FeatureKey::ReportDailyProcedures));
        assert!(app.features.enabled(FeatureKey::ReportProcedureCodes));
        assert!(app.features.enabled(FeatureKey::ReportPrescriptions));
        assert!(app.features.enabled(FeatureKey::ReportReferralAnalysis));
        assert!(app.features.enabled(FeatureKey::ReportReferrals));
        assert!(app.features.enabled(FeatureKey::ReportCustomAging));
        assert!(app.features.enabled(FeatureKey::ReportPatients));
        assert!(app.features.enabled(FeatureKey::ReportBalanceSheet));
        assert!(app.features.enabled(FeatureKey::ReportUnearnedIncome));
        assert!(app.features.enabled(FeatureKey::ReportProcNote));
        assert!(app.features.enabled(FeatureKey::ReportTpPresentationStats));
        assert_eq!(ReportKind::ALL.len(), 55, "local + upstream reports wired");
        assert_eq!(
            ReportKind::JournalPostingAudit.feature_key(),
            FeatureKey::LedgerJournalPosting
        );
        assert_eq!(
            ReportKind::JournalPostingAudit.file_stem(),
            "journal-posting-audit"
        );
        assert!(ReportKind::JournalPostingAudit.uses_date_range());
        assert_eq!(
            ReportKind::LedgerTieOut.feature_key(),
            FeatureKey::LedgerJournalPosting
        );
        assert_eq!(ReportKind::LedgerTieOut.file_stem(), "ledger-tie-out");
        assert!(ReportKind::LedgerTieOut.uses_date_range());
        assert_eq!(ReportKind::PeriodCloseHistory.label(), "Period close history");
        assert_eq!(
            ReportKind::PeriodCloseHistory.feature_key(),
            FeatureKey::LedgerJournalPosting
        );
        assert_eq!(
            ReportKind::PeriodCloseHistory.file_stem(),
            "period-close-history"
        );
        assert!(!ReportKind::PeriodCloseHistory.uses_date_range());
        assert_eq!(ReportKind::CustomAging.file_stem(), "custom-aging");
        assert_eq!(ReportKind::Patients.file_stem(), "patients");
        assert_eq!(ReportKind::BalanceSheet.file_stem(), "balance-sheet");
        assert_eq!(ReportKind::UnearnedIncome.file_stem(), "unearned-income");
        assert_eq!(
            ReportKind::TpPresentationStats.file_stem(),
            "plan-presentation-statistics"
        );
        assert!(
            ReportKind::ALL
                .iter()
                .all(|kind| app.features.enabled(kind.feature_key())),
            "all report toggles default on"
        );
        let mut without_custom_aging = app.features.clone();
        without_custom_aging.set(FeatureKey::ReportCustomAging, false);
        assert_eq!(
            ReportKind::ALL
                .iter()
                .filter(|kind| without_custom_aging.enabled(kind.feature_key()))
                .count(),
            ReportKind::ALL.len() - 1,
            "turning the toggle off hides only Custom Aging"
        );
        app.report_prefs.practice_name = "Test Clinic".into();
        app.report_prefs.set_export_path("recall-asap", "C:\\r.csv");
        assert_eq!(app.report_prefs.export_path("recall-asap"), Some("C:\\r.csv"));
        let mut sorted = ReportTable::new(&["Item", "Value", "Detail"]);
        sorted.push(&["Nine", "9.00", ""]);
        sorted.push(&["Hundred", "100.00", ""]);
        sorted.sort_by_column(1, false);
        assert_eq!(sorted.rows[0][0], "Nine");

        // The registry is reachable from the treatment/family views and loads
        // conditions, treatments, notes and cards for the selected patient.
        app.open_registry(1);
        assert!(app.registry_open);
        assert_eq!(app.registry_patient, Some(1));
        assert_eq!(app.registry_tab, RegistryTab::Overview);
        assert!(!app.registry_conditions.is_empty());
        assert!(!app.registry_treatments.is_empty());
        assert!(app.registry_note.is_some());
        assert!(app.registry_cards.is_empty());
        let patients = app.patients.clone();
        let name = patients
            .iter()
            .find(|p| p.pat_num == 1)
            .unwrap()
            .display_name();
        let procedures = app.store.list_procedures(1).unwrap();
        let splits = app.store.list_pay_splits(1).unwrap();
        let doc = invoice_for(1, &name, &procedures, &splits, "10", "Test invoice");
        assert!(doc.total() > 0.0, "registry invoice uses completed procedures");
        assert!(doc.lines.iter().all(|line| line.tax_rate == 10.0));
        println!("ALIVE=True");
    }
}

    #[test]
    fn reconcile_dynamic_plans_due_portfolio_issues_and_audits() {
        let mut app = OdApp::new();
        app.refresh();
        let pat_num = app.patients[0].pat_num;
        let start = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let as_of = chrono::NaiveDate::from_ymd_opt(2026, 2, 15).unwrap();
        let adj_num = app.store.upsert_adjustment(&od_domain::Adjustment {
            pat_num,
            adj_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(),
            adj_amount: 300.0,
            ..Default::default()
        }).unwrap();
        let plan_num = app.store.upsert_pay_plan(&PayPlan {
            pat_num,
            guarantor: pat_num,
            pay_plan_date: start,
            is_dynamic: true,
            charge_frequency: od_domain::PayPlanFrequency::Monthly,
            pay_schedule: od_domain::PaymentSchedule::Monthly,
            pay_amt: 100.0,
            date_pay_plan_start: Some(start),
            ..Default::default()
        }).unwrap();
        let _ = app.store.upsert_pay_plan_link(&PayPlanLink {
            pay_plan_num: plan_num,
            link_type: PayPlanLink::ADJUSTMENT,
            fkey: adj_num,
            ..Default::default()
        }).unwrap();
        app.refresh();

        let plans = app.store.list_pay_plans(pat_num).unwrap();
        let links = app.store.list_pay_plan_links().unwrap();
        let procedures = app.store.list_procedures(pat_num).unwrap();
        let adjustments = app.store.list_adjustments(pat_num).unwrap();
        let splits = app.store.list_pay_splits(pat_num).unwrap();
        let charges = app.store.list_pay_plan_charges(pat_num).unwrap();
        app.reconcile_dynamic_plans_due_portfolio(&plans, &links, &procedures, &adjustments, &splits, &charges, as_of);

        let notice = app.notice.clone().unwrap_or_default();
        assert!(notice.contains("Issued"), "notice: {notice}; err: {:?}", app.error);
        let after = app.store.list_pay_plan_charges(pat_num).unwrap();
        let own: Vec<_> = after.iter().filter(|c| c.pay_plan_num == plan_num).collect();
        assert!(own.len() >= 2, "charges should be issued for Jan+Feb due periods, got {}", own.len());
        let logs = app.store.list_security_log(200).unwrap();
        assert!(logs.iter().any(|l| l.log_text.contains("portfolio reconcile") && l.log_text.contains(&format!("plan {plan_num}"))), "audit log missing");

        // Idempotent second run.
        app.reconcile_dynamic_plans_due_portfolio(&plans, &links, &procedures, &adjustments, &splits, &after, as_of);
        let notice2 = app.notice.clone().unwrap_or_default();
        assert!(notice2.contains("No dynamic charges due to issue"), "notice2: {notice2}; err: {:?}", app.error);
    }

    #[test]
    fn resolve_stale_dynamic_plan_charges_is_safe_and_audited() {
        let mut app = OdApp::new();
        app.refresh();
        let pat_num = app.patients[0].pat_num;
        let start = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let as_of = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let adj_num = app.store.upsert_adjustment(&od_domain::Adjustment {
            pat_num,
            adj_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(),
            adj_amount: 300.0,
            ..Default::default()
        }).unwrap();
        let plan_num = app.store.upsert_pay_plan(&PayPlan {
            pat_num,
            guarantor: pat_num,
            pay_plan_date: start,
            is_dynamic: true,
            charge_frequency: od_domain::PayPlanFrequency::Monthly,
            pay_schedule: od_domain::PaymentSchedule::Monthly,
            pay_amt: 100.0,
            date_pay_plan_start: Some(start),
            ..Default::default()
        }).unwrap();
        let _ = app.store.upsert_pay_plan_link(&PayPlanLink {
            pay_plan_num: plan_num,
            link_type: PayPlanLink::ADJUSTMENT,
            fkey: adj_num,
            ..Default::default()
        }).unwrap();
        app.refresh();
        let plans = app.store.list_pay_plans(pat_num).unwrap();
        let links = app.store.list_pay_plan_links().unwrap();
        let procedures = app.store.list_procedures(pat_num).unwrap();
        let adjustments = app.store.list_adjustments(pat_num).unwrap();
        let splits = app.store.list_pay_splits(pat_num).unwrap();
        let charges = app.store.list_pay_plan_charges(pat_num).unwrap();
        app.reconcile_dynamic_plans_due_portfolio(&plans, &links, &procedures, &adjustments, &splits, &charges, as_of);
        // Reduce production so issued Mar charge becomes stale/over-issued.
        let _ = app.store.upsert_adjustment(&od_domain::Adjustment {
            adj_num,
            pat_num,
            adj_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(),
            adj_amount: 200.0,
            ..Default::default()
        }).unwrap();
        app.refresh();
        let plans = app.store.list_pay_plans(pat_num).unwrap();
        let plan = plans.iter().find(|p| p.pay_plan_num == plan_num).unwrap().clone();
        let links = app.store.list_pay_plan_links().unwrap();
        let own_links: Vec<_> = links.iter().filter(|l| l.pay_plan_num == plan_num).cloned().collect();
        let procedures = app.store.list_procedures(pat_num).unwrap();
        let adjustments = app.store.list_adjustments(pat_num).unwrap();
        let splits = app.store.list_pay_splits(pat_num).unwrap();
        let charges = app.store.list_pay_plan_charges(pat_num).unwrap();
        let rec = od_core::reconciliation(&plan, &own_links, &procedures, &adjustments, &splits, &charges, as_of);
        assert!(rec.stale_issued_count > 0, "expected stale charges");
        let (_, _, _, stale) = od_core::reconciliation_detail(&plan, &own_links, &procedures, &adjustments, &splits, &charges, as_of);
        let evidence = od_core::stale_charge_evidence(&stale);
        assert_eq!(evidence.len(), rec.stale_issued_count);
        assert!(evidence.iter().any(|line| line.contains("issued")
            && line.contains("expected")
            && line.contains("no splits")), "stale evidence preview missing: {evidence:?}");
        // Empty reason must block resolution before any row is changed.
        app.resolve_stale_dynamic_plan_charges(&plan, &own_links, &procedures, &adjustments, &splits, &charges, "", as_of);
        assert!(app.error.as_deref().unwrap_or("").contains("resolution reason"));
        assert!(app.notice.is_none());
        let unchanged = app.store.list_pay_plan_charges(pat_num).unwrap();
        assert!(unchanged.iter().any(|c| c.pay_plan_num == plan_num && !od_core::is_stale_resolved_charge(c)));

        let reason = "procedure fee corrected after chart review";
        app.stale_resolution_reasons.insert(plan_num, reason.into());
        app.resolve_stale_dynamic_plan_charges(&plan, &own_links, &procedures, &adjustments, &splits, &charges, reason, as_of);
        let notice = app.notice.clone().unwrap_or_default();
        assert!(notice.contains("Resolved"), "notice: {notice}; err: {:?}", app.error);
        let after = app.store.list_pay_plan_charges(pat_num).unwrap();
        let stale_zeroed = after.iter().any(|c| c.pay_plan_num == plan_num && c.note.contains("Stale resolved") && c.note.contains(reason) && (c.principal + c.interest).abs() < 0.01);
        assert!(stale_zeroed, "stale charge should be zeroed and annotated with the staff reason");
        let logs = app.store.list_security_log(200).unwrap();
        assert!(logs.iter().any(|l| l.log_text.contains("Resolved")
            && l.log_text.contains("stale dynamic charge")
            && l.log_text.contains(reason)), "audit log missing staff reason");
    }

    #[test]
    fn stale_resolved_plan_charge_display_filter_respects_toggle() {
        let active = PayPlanCharge {
            pay_plan_num: 1,
            charge_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            charge_type: PayPlanChargeType::Debit,
            principal: 100.0,
            pay_plan_charge_num: 1,
            ..Default::default()
        };
        let resolved = PayPlanCharge {
            pay_plan_num: 1,
            charge_date: chrono::NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            charge_type: PayPlanChargeType::Debit,
            note: "[Stale resolved 2026-03-01: production reduced]".into(),
            pay_plan_charge_num: 2,
            ..Default::default()
        };
        let other_plan = PayPlanCharge {
            pay_plan_num: 2,
            note: "[Stale resolved 2026-03-01: production reduced]".into(),
            pay_plan_charge_num: 3,
            ..Default::default()
        };
        let charges = [active, resolved, other_plan];

        let shown = visible_plan_charge_rows(&charges, 1, true);
        assert_eq!(shown.len(), 2, "resolved rows are visible by default for audit");
        assert!(shown.iter().any(|c| od_core::is_stale_resolved_charge(c)));

        let hidden = visible_plan_charge_rows(&charges, 1, false);
        assert_eq!(hidden.len(), 1);
        assert_eq!(hidden[0].pay_plan_charge_num, 1);
        assert!(!hidden.iter().any(|c| od_core::is_stale_resolved_charge(c)));
    }

    #[test]
    fn account_transfer_filter_and_export_respect_plan_target_and_date() {
        let rows = vec![
            LedgerRow {
                date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                kind: "Refund/Transfer".into(),
                description: "Payment plan #1 (credit moved/refund -> procedure #10)".into(),
                charge: 10.0,
                credit: 0.0,
                plan_num: 1,
                transfer_target: Some(PaymentTarget::Procedure(10)),
                pay_num: 101,
                adj_num: 0,
                note: "moved to proc".into(),
            },
            LedgerRow {
                date: chrono::NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
                kind: "Refund/Transfer".into(),
                description: "Payment plan #2 (credit moved/refund -> patient account (unallocated))".into(),
                charge: 20.0,
                credit: 0.0,
                plan_num: 2,
                transfer_target: Some(PaymentTarget::Unallocated),
                pay_num: 102,
                adj_num: 0,
                note: "refund reason".into(),
            },
            LedgerRow {
                date: chrono::NaiveDate::from_ymd_opt(2026, 2, 15).unwrap(),
                kind: "Payment".into(),
                description: "Payment plan #2 (unapplied to charge)".into(),
                charge: 0.0,
                credit: 20.0,
                plan_num: 2,
                transfer_target: None,
                pay_num: 103,
                adj_num: 0,
                note: String::new(),
            },
        ];
        let filtered = filter_account_ledger_rows(
            &rows,
            true,
            2,
            PaymentTarget::Unallocated,
            Some(chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()),
            Some(chrono::NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()),
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].plan_num, 2);
        let table = account_ledger_export_table(&filtered);
        let csv = render_report_csv("Account ledger (filtered)", "", &table);
        let html = render_report_html("Account ledger (filtered)", "", &table);
        assert!(csv.contains("Refund/Transfer"));
        assert!(csv.contains("plan #2"));
        assert!(csv.contains("Payment #,Note"));
        assert!(csv.contains("102,refund reason"));
        assert!(!csv.contains("procedure #10"));
        assert!(html.contains("patient account (unallocated)"));
    }
