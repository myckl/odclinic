//! Domain types for the scoped Open Dental port.
//!
//! Field names are Rust snake_case; the SQL layer maps them to Open Dental's
//! PascalCase column names. Types and enum values mirror Open Dental 24.3,
//! verified against the 25.3.48.0 assembly with REA where noted.

pub mod definitions;
pub mod eservices;
pub mod password;
pub mod reports;
pub mod reports_ext;
pub mod reports_source;

pub use reports_source::{
    Carrier, ClaimSnapshot, County, DiscountPlan, DiscountPlanSub, PatientRace, PayPlanLink,
    Schedule, ScheduleOp, Screen, ScreenGroup, Site,
};

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{NaiveDate, NaiveDateTime};

pub type Id = u32;

/// The seven desktop modules carried over from Open Dental.
///
/// `Imaging` is deliberately absent: it is out of scope for this port.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Module {
    Appointments,
    Family,
    Account,
    TreatmentPlan,
    Chart,
    Reports,
    Setup,
}

impl Module {
    pub const ALL: [Module; 7] = [
        Module::Appointments,
        Module::Family,
        Module::Account,
        Module::TreatmentPlan,
        Module::Chart,
        Module::Reports,
        Module::Setup,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Module::Appointments => "Appointments",
            Module::Family => "Family",
            Module::Account => "Account",
            Module::TreatmentPlan => "Treatment Plan",
            Module::Chart => "Chart",
            Module::Reports => "Reports",
            Module::Setup => "Setup",
        }
    }

    pub fn feature_key(self) -> FeatureKey {
        match self {
            Module::Appointments => FeatureKey::ModuleAppointments,
            Module::Family => FeatureKey::ModuleFamily,
            Module::Account => FeatureKey::ModuleAccount,
            Module::TreatmentPlan => FeatureKey::ModuleTreatmentPlan,
            Module::Chart => FeatureKey::ModuleChart,
            Module::Reports => FeatureKey::ModuleReports,
            Module::Setup => FeatureKey::ModuleSetup,
        }
    }
}

/// Every switch that can be changed in the Settings screen.
///
/// Keeping these in the domain layer means the GUI, persistence and any future
/// security rules all refer to the same stable keys. The text form is a simple
/// `key=true|false` file so no extra dependency is needed.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FeatureKey {
    ModuleAppointments,
    ModuleFamily,
    ModuleAccount,
    ModuleTreatmentPlan,
    ModuleChart,
    ModuleReports,
    ModuleSetup,
    PatientSearch,
    PatientEdit,
    PatientDelete,
    AppointmentCreate,
    AppointmentEdit,
    AppointmentDelete,
    AppointmentConflictWarnings,
    AppointmentStatusColors,
    RecallsEnabled,
    AsapListEnabled,
    WaitlistEnabled,
    PatientCorrectionHistory,
    AppointmentHistory,
    AccountBalanceSummary,
    AccountDetailedBalance,
    AccountShowAdjustments,
    AccountShowClaims,
    InsuranceEnabled,
    AccountShowStatements,
    AccountBillingEdits,
    /// Explicitly post a completed-procedure charge, patient payment or account adjustment into the double-entry journal.
    LedgerJournalPosting,
    PaymentPlansEnabled,
    DiscountPlans,
    DiscountPlanAutoAdjust,
    ProcedureCompletion,
    TreatmentPlanLines,
    TreatmentPlanEdit,
    AuditTreatmentPlans,
    ChartToothSelection,
    ReportPatientList,
    ReportProduction,
    ReportCollections,
    ReportSchedule,
    ShowImagingNote,
    ShowDatabaseLabel,
    ShowPreferences,
    ShowUsers,
    ShowPermissions,
    ShowAuditLog,
    RequireLogin,
    EnforcePermissions,
    AuditLogins,
    AuditPatientChanges,
    AuditAppointmentChanges,
    AuditBillingChanges,
    EServicesEnabled,
    RemindersEnabled,
    ConfirmationsEnabled,
    ReminderDryRun,
    AuditReminders,
    ShowDefinitions,
    ShowDisplayFields,
    AuditDefinitionChanges,
    PreferDemoData,
    PatientRegistry,
    ClinicCardsEnabled,
    A5Invoice,
    ReportRecallAsap,
    ReportPaymentPlans,
    ReportTreatmentPlans,
    ReportAging,
    ReportPayPlanDetail,
    ReportPayPlanChargeApplications,
    ReportBirthday,
    ReportNewPatients,
    ReportAppointments,
    ReportActivePatients,
    ReportProdInc,
    ReportPaySheet,
    ReportAdjSheet,
    ReportFinanceCharge,
    ReportBrokenAppointments,
    ReportConfirmations,
    ReportRecallFields,
    ReportDailyProcedures,
    ReportProcedureCodes,
    ReportPrescriptions,
    ReportReferralAnalysis,
    ReportReferrals,
    ReportCustomAging,
    ReportPatients,
    ReportBalanceSheet,
    ReportGeneralLedger,
    ReportProfitLoss,
    ReportHiddenPaySplits,
    ReportUnearnedIncome,
    ReportProcOverpaid,
    ReportReceivablesBreakdown,
    ReportServiceDateView,
    ReportProcNote,
    ReportDentalSealant,
    ReportDiscountPlan,
    ReportDppOvercharged,
    ReportPatientPortionUncollected,
    ReportTreatmentFinder,
    ReportPhRawPop,
    ReportPhRawScreen,
    ReportRouting,
    ReportLaserLabels,
    ReportPrintPreview,
    ReportStatement,
    ReportProdGoal,
    ReportProviderPayroll,
    ReportPresentedTreatment,
    ReportTpPresentationStats,
}

impl FeatureKey {
    pub const ALL: [Self; 112] = [
        Self::ModuleAppointments,
        Self::ModuleFamily,
        Self::ModuleAccount,
        Self::ModuleTreatmentPlan,
        Self::ModuleChart,
        Self::ModuleReports,
        Self::ModuleSetup,
        Self::PatientSearch,
        Self::PatientEdit,
        Self::PatientDelete,
        Self::AppointmentCreate,
        Self::AppointmentEdit,
        Self::AppointmentDelete,
        Self::AppointmentConflictWarnings,
        Self::AppointmentStatusColors,
        Self::RecallsEnabled,
        Self::AsapListEnabled,
        Self::WaitlistEnabled,
        Self::PatientCorrectionHistory,
        Self::AppointmentHistory,
        Self::AccountBalanceSummary,
        Self::AccountDetailedBalance,
        Self::AccountShowAdjustments,
        Self::AccountShowClaims,
        Self::InsuranceEnabled,
        Self::AccountShowStatements,
        Self::AccountBillingEdits,
        Self::LedgerJournalPosting,
        Self::PaymentPlansEnabled,
        Self::DiscountPlans,
        Self::DiscountPlanAutoAdjust,
        Self::ProcedureCompletion,
        Self::TreatmentPlanLines,
        Self::TreatmentPlanEdit,
        Self::AuditTreatmentPlans,
        Self::ChartToothSelection,
        Self::ReportPatientList,
        Self::ReportProduction,
        Self::ReportCollections,
        Self::ReportSchedule,
        Self::ShowImagingNote,
        Self::ShowDatabaseLabel,
        Self::ShowPreferences,
        Self::ShowUsers,
        Self::ShowPermissions,
        Self::ShowAuditLog,
        Self::RequireLogin,
        Self::EnforcePermissions,
        Self::AuditLogins,
        Self::AuditPatientChanges,
        Self::AuditAppointmentChanges,
        Self::AuditBillingChanges,
        Self::EServicesEnabled,
        Self::RemindersEnabled,
        Self::ConfirmationsEnabled,
        Self::ReminderDryRun,
        Self::AuditReminders,
        Self::ShowDefinitions,
        Self::ShowDisplayFields,
        Self::AuditDefinitionChanges,
        Self::PreferDemoData,
        Self::PatientRegistry,
        Self::ClinicCardsEnabled,
        Self::A5Invoice,
        Self::ReportRecallAsap,
        Self::ReportPaymentPlans,
        Self::ReportTreatmentPlans,
        Self::ReportAging,
        Self::ReportPayPlanDetail,
        Self::ReportPayPlanChargeApplications,
        Self::ReportBirthday,
        Self::ReportNewPatients,
        Self::ReportAppointments,
        Self::ReportActivePatients,
        Self::ReportProdInc,
        Self::ReportPaySheet,
        Self::ReportAdjSheet,
        Self::ReportFinanceCharge,
        Self::ReportBrokenAppointments,
        Self::ReportConfirmations,
        Self::ReportRecallFields,
        Self::ReportDailyProcedures,
        Self::ReportProcedureCodes,
        Self::ReportPrescriptions,
        Self::ReportReferralAnalysis,
        Self::ReportReferrals,
        Self::ReportCustomAging,
        Self::ReportPatients,
        Self::ReportBalanceSheet,
        Self::ReportGeneralLedger,
        Self::ReportProfitLoss,
        Self::ReportHiddenPaySplits,
        Self::ReportUnearnedIncome,
        Self::ReportProcOverpaid,
        Self::ReportReceivablesBreakdown,
        Self::ReportServiceDateView,
        Self::ReportProcNote,
        Self::ReportDentalSealant,
        Self::ReportDiscountPlan,
        Self::ReportDppOvercharged,
        Self::ReportPatientPortionUncollected,
        Self::ReportTreatmentFinder,
        Self::ReportPhRawPop,
        Self::ReportPhRawScreen,
        Self::ReportRouting,
        Self::ReportLaserLabels,
        Self::ReportPrintPreview,
        Self::ReportStatement,
        Self::ReportProdGoal,
        Self::ReportProviderPayroll,
        Self::ReportPresentedTreatment,
        Self::ReportTpPresentationStats,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ModuleAppointments => "module_appointments",
            Self::ModuleFamily => "module_family",
            Self::ModuleAccount => "module_account",
            Self::ModuleTreatmentPlan => "module_treatment_plan",
            Self::ModuleChart => "module_chart",
            Self::ModuleReports => "module_reports",
            Self::ModuleSetup => "module_setup",
            Self::PatientSearch => "patient_search",
            Self::PatientEdit => "patient_edit",
            Self::PatientDelete => "patient_delete",
            Self::AppointmentCreate => "appointment_create",
            Self::AppointmentEdit => "appointment_edit",
            Self::AppointmentDelete => "appointment_delete",
            Self::AppointmentConflictWarnings => "appointment_conflict_warnings",
            Self::AppointmentStatusColors => "appointment_status_colors",
            Self::RecallsEnabled => "recalls_enabled",
            Self::AsapListEnabled => "asap_list_enabled",
            Self::WaitlistEnabled => "waitlist_enabled",
            Self::PatientCorrectionHistory => "patient_correction_history",
            Self::AppointmentHistory => "appointment_history",
            Self::AccountBalanceSummary => "account_balance_summary",
            Self::AccountDetailedBalance => "account_detailed_balance",
            Self::AccountShowAdjustments => "account_show_adjustments",
            Self::AccountShowClaims => "account_show_claims",
            Self::InsuranceEnabled => "insurance_enabled",
            Self::AccountShowStatements => "account_show_statements",
            Self::AccountBillingEdits => "account_billing_edits",
            Self::LedgerJournalPosting => "ledger_journal_posting",
            Self::PaymentPlansEnabled => "payment_plans",
            Self::DiscountPlans => "discount_plans",
            Self::DiscountPlanAutoAdjust => "discount_plan_auto_adjust",
            Self::ProcedureCompletion => "procedure_completion",
            Self::TreatmentPlanLines => "treatment_plan_lines",
            Self::TreatmentPlanEdit => "treatment_plan_edit",
            Self::AuditTreatmentPlans => "audit_treatment_plans",
            Self::ChartToothSelection => "chart_tooth_selection",
            Self::ReportPatientList => "report_patient_list",
            Self::ReportProduction => "report_production",
            Self::ReportCollections => "report_collections",
            Self::ReportSchedule => "report_schedule",
            Self::ShowImagingNote => "show_imaging_note",
            Self::ShowDatabaseLabel => "show_database_label",
            Self::ShowPreferences => "show_preferences",
            Self::ShowUsers => "show_users",
            Self::ShowPermissions => "show_permissions",
            Self::ShowAuditLog => "show_audit_log",
            Self::RequireLogin => "require_login",
            Self::EnforcePermissions => "enforce_permissions",
            Self::AuditLogins => "audit_logins",
            Self::AuditPatientChanges => "audit_patient_changes",
            Self::AuditAppointmentChanges => "audit_appointment_changes",
            Self::AuditBillingChanges => "audit_billing_changes",
            Self::EServicesEnabled => "eservices_enabled",
            Self::RemindersEnabled => "reminders_enabled",
            Self::ConfirmationsEnabled => "confirmations_enabled",
            Self::ReminderDryRun => "reminder_dry_run",
            Self::AuditReminders => "audit_reminders",
            Self::ShowDefinitions => "show_definitions",
            Self::ShowDisplayFields => "show_display_fields",
            Self::AuditDefinitionChanges => "audit_definition_changes",
            Self::PreferDemoData => "prefer_demo_data",
            Self::PatientRegistry => "patient_registry",
            Self::ClinicCardsEnabled => "clinic_cards_enabled",
            Self::A5Invoice => "a5_invoice",
            Self::ReportRecallAsap => "report_recall_asap",
            Self::ReportPaymentPlans => "report_payment_plans",
            Self::ReportTreatmentPlans => "report_treatment_plans",
            Self::ReportAging => "report_aging",
            Self::ReportPayPlanDetail => "report_pay_plan_detail",
            Self::ReportPayPlanChargeApplications => "report_pay_plan_charge_applications",
            Self::ReportBirthday => "report_birthday",
            Self::ReportNewPatients => "report_new_patients",
            Self::ReportAppointments => "report_appointments",
            Self::ReportActivePatients => "report_active_patients",
            Self::ReportProdInc => "report_prod_inc",
            Self::ReportPaySheet => "report_pay_sheet",
            Self::ReportAdjSheet => "report_adj_sheet",
            Self::ReportFinanceCharge => "report_finance_charge",
            Self::ReportBrokenAppointments => "report_broken_appointments",
            Self::ReportConfirmations => "report_confirmations",
            Self::ReportRecallFields => "report_recall_fields",
            Self::ReportDailyProcedures => "report_daily_procedures",
            Self::ReportProcedureCodes => "report_procedure_codes",
            Self::ReportPrescriptions => "report_prescriptions",
            Self::ReportReferralAnalysis => "report_referral_analysis",
            Self::ReportReferrals => "report_referrals",
            Self::ReportCustomAging => "report_custom_aging",
            Self::ReportPatients => "report_patients",
            Self::ReportBalanceSheet => "report_balance_sheet",
            Self::ReportGeneralLedger => "report_general_ledger",
            Self::ReportProfitLoss => "report_profit_loss",
            Self::ReportHiddenPaySplits => "report_hidden_pay_splits",
            Self::ReportUnearnedIncome => "report_unearned_income",
            Self::ReportProcOverpaid => "report_proc_overpaid",
            Self::ReportReceivablesBreakdown => "report_receivables_breakdown",
            Self::ReportServiceDateView => "report_service_date_view",
            Self::ReportProcNote => "report_proc_note",
            Self::ReportDentalSealant => "report_dental_sealant",
            Self::ReportDiscountPlan => "report_discount_plan",
            Self::ReportDppOvercharged => "report_dpp_overcharged",
            Self::ReportPatientPortionUncollected => "report_patient_portion_uncollected",
            Self::ReportTreatmentFinder => "report_treatment_finder",
            Self::ReportPhRawPop => "report_ph_raw_pop",
            Self::ReportPhRawScreen => "report_ph_raw_screen",
            Self::ReportRouting => "report_routing",
            Self::ReportLaserLabels => "report_laser_labels",
            Self::ReportPrintPreview => "report_print_preview",
            Self::ReportStatement => "report_statement",
            Self::ReportProdGoal => "report_prod_goal",
            Self::ReportProviderPayroll => "report_provider_payroll",
            Self::ReportPresentedTreatment => "report_presented_treatment",
            Self::ReportTpPresentationStats => "report_tp_presentation_stats",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ModuleAppointments => "Appointments module",
            Self::ModuleFamily => "Family module",
            Self::ModuleAccount => "Account module",
            Self::ModuleTreatmentPlan => "Treatment Plan module",
            Self::ModuleChart => "Chart module",
            Self::ModuleReports => "Reports module",
            Self::ModuleSetup => "Setup module",
            Self::PatientSearch => "Patient search box",
            Self::PatientEdit => "Edit patients",
            Self::PatientDelete => "Delete patients",
            Self::AppointmentCreate => "New appointments",
            Self::AppointmentEdit => "Edit appointments",
            Self::AppointmentDelete => "Delete appointments",
            Self::AppointmentConflictWarnings => "Schedule conflict warnings",
            Self::AppointmentStatusColors => "Colour-coded appointment statuses",
            Self::RecallsEnabled => "Recall list",
            Self::AsapListEnabled => "ASAP list",
            Self::WaitlistEnabled => "Waitlist",
            Self::PatientCorrectionHistory => "Patient correction history",
            Self::AppointmentHistory => "Appointment history",
            Self::AccountBalanceSummary => "Balance summary",
            Self::AccountDetailedBalance => "Detailed balance breakdown",
            Self::AccountShowAdjustments => "Adjustments in Account",
            Self::AccountShowClaims => "Claims in Account",
            Self::InsuranceEnabled => "Insurance & claims",
            Self::AccountShowStatements => "Statements in Account",
            Self::AccountBillingEdits => "Record payments and adjustments",
            Self::LedgerJournalPosting => "Post charges, payments and adjustments to the ledger",
            Self::PaymentPlansEnabled => "Payment plans",
            Self::DiscountPlans => "Discount plans in Setup",
            Self::DiscountPlanAutoAdjust => "Automatic discount-plan adjustments",
            Self::ProcedureCompletion => "Complete procedures in Chart",
            Self::TreatmentPlanLines => "Procedure lines in treatment plans",
            Self::TreatmentPlanEdit => "Edit treatment plans",
            Self::AuditTreatmentPlans => "Audit treatment plan changes",
            Self::ChartToothSelection => "Tooth selection in Chart",
            Self::ReportPatientList => "Patient list report",
            Self::ReportProduction => "Production report",
            Self::ReportCollections => "Collections report",
            Self::ReportSchedule => "Schedule report",
            Self::ShowImagingNote => "Imaging-omitted note",
            Self::ShowDatabaseLabel => "Database label in title bar",
            Self::ShowPreferences => "Preferences in Setup",
            Self::ShowUsers => "Users in Setup",
            Self::ShowPermissions => "Permission codes in Setup",
            Self::ShowAuditLog => "Audit log in Setup",
            Self::RequireLogin => "Require login at startup",
            Self::EnforcePermissions => "Enforce Open Dental permissions",
            Self::AuditLogins => "Audit logins and logouts",
            Self::AuditPatientChanges => "Audit patient changes",
            Self::AuditAppointmentChanges => "Audit appointment changes",
            Self::AuditBillingChanges => "Audit billing changes",
            Self::EServicesEnabled => "eServices (local reminders)",
            Self::RemindersEnabled => "Appointment reminders",
            Self::ConfirmationsEnabled => "Future-day confirmations",
            Self::ReminderDryRun => "Reminder dry-run mode",
            Self::AuditReminders => "Audit reminder sends",
            Self::ShowDefinitions => "Definitions in Setup",
            Self::ShowDisplayFields => "Display fields in Setup",
            Self::AuditDefinitionChanges => "Audit definition changes",
            Self::PreferDemoData => "Always use demo data",
            Self::PatientRegistry => "Patient registry (Treatment Plan)",
            Self::ClinicCardsEnabled => "Clinic card issuing in registry",
            Self::A5Invoice => "A5 invoice output",
            Self::ReportRecallAsap => "Recall & ASAP report",
            Self::ReportPaymentPlans => "Payment-plan balance report",
            Self::ReportTreatmentPlans => "Treatment-plan summary report",
            Self::ReportAging => "Patient aging report",
            Self::ReportPayPlanDetail => "Payment-plan detail report",
            Self::ReportPayPlanChargeApplications => "Payment plan charge applications report",
            Self::ReportBirthday => "Birthday report",
            Self::ReportNewPatients => "New patients report",
            Self::ReportAppointments => "Appointments report (FormRpAppointments)",
            Self::ReportActivePatients => "Active patients report (FormRpActivePatients)",
            Self::ReportProdInc => "Production & income report (FormRpProdInc)",
            Self::ReportPaySheet => "Daily payments report (FormRpPaySheet)",
            Self::ReportAdjSheet => "Daily adjustments report (FormRpAdjSheet)",
            Self::ReportFinanceCharge => "Finance charge report (FormRpFinanceCharge)",
            Self::ReportBrokenAppointments => "Broken appointments report (FormRpBrokenAppointments)",
            Self::ReportConfirmations => "Confirmation report (FormRpConfirm)",
            Self::ReportRecallFields => "Recall field-selection report (FormRpRecall)",
            Self::ReportDailyProcedures => "Daily procedures report (FormRpProcSheet)",
            Self::ReportProcedureCodes => "Procedure codes report (FormRpProcCodes)",
            Self::ReportPrescriptions => "Prescriptions report (FormRpPrescriptions)",
            Self::ReportReferralAnalysis => "Referral analysis report (FormRpReferralAnalysis)",
            Self::ReportReferrals => "Referrals report (FormRpReferrals)",
            Self::ReportCustomAging => "Custom aging report (FormRpCustomAging)",
            Self::ReportPatients => "Patient field-selection report (FormRpPatients)",
            Self::ReportBalanceSheet => "Balance sheet report (FormRpAccountingBalanceSheet)",
            Self::ReportGeneralLedger => "General ledger report (FormRpAccountingGenLedg)",
            Self::ReportProfitLoss => "Profit & loss report (FormRpAccountingProfitLoss)",
            Self::ReportHiddenPaySplits => "Hidden pay splits report (FormRpHiddenPaySplits)",
            Self::ReportUnearnedIncome => "Unearned income report (FormRpUnearnedIncome)",
            Self::ReportProcOverpaid => "Overpaid procedures report (FormRpProcOverpaid)",
            Self::ReportReceivablesBreakdown => "Receivables breakdown (FormRpReceivablesBreakdown)",
            Self::ReportServiceDateView => "Service-date view (FormRpServiceDateView)",
            Self::ReportProcNote => "Incomplete procedure notes (FormRpProcNote)",
            Self::ReportDentalSealant => "Dental sealant measure (FormRpDentalSealantMeasure)",
            Self::ReportDiscountPlan => "Discount plan list (FormRpDiscountPlan)",
            Self::ReportDppOvercharged => "Overcharged payment plans (FormRpDPPOvercharged)",
            Self::ReportPatientPortionUncollected => {
                "Patient portion uncollected (FormRpPatPortionUncollected)"
            }
            Self::ReportTreatmentFinder => "Treatment finder (FormRpTreatmentFinder)",
            Self::ReportPhRawPop => "Raw population extract (FormRpPHRawPop)",
            Self::ReportPhRawScreen => "Raw screening extract (FormRpPHRawScreen)",
            Self::ReportRouting => "Routing slips (FormRpRouting)",
            Self::ReportLaserLabels => "Laser labels (FormRpLaserLabels)",
            Self::ReportPrintPreview => "Print preview (FormRpPrintPreview)",
            Self::ReportStatement => "Statements report (FormRpStatement)",
            Self::ReportProdGoal => "Production goal report (FormRpProdGoal)",
            Self::ReportProviderPayroll => "Provider payroll report (FormRpProviderPayroll)",
            Self::ReportPresentedTreatment => "Presented treatment production (FormRpPresentedTreatmentProduction)",
            Self::ReportTpPresentationStats => "Treatment-plan presentation statistics (FormRpTreatPlanPresentationStatistics)",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::ModuleAppointments => "Show or hide the Appointments module in the menu and side bar.",
            Self::ModuleFamily => "Show or hide the Family module in the menu and side bar.",
            Self::ModuleAccount => "Show or hide the Account module in the menu and side bar.",
            Self::ModuleTreatmentPlan => "Show or hide the Treatment Plan module in the menu and side bar.",
            Self::ModuleChart => "Show or hide the Chart module in the menu and side bar.",
            Self::ModuleReports => "Show or hide the Reports module in the menu and side bar.",
            Self::ModuleSetup => "Show or hide the Setup module. Settings stays available either way.",
            Self::PatientSearch => "Show the search box. When off, the full patient list is shown.",
            Self::PatientEdit => "Allow editing patient details.",
            Self::PatientDelete => "Show the Delete patient button.",
            Self::AppointmentCreate => "Show the New appointment button.",
            Self::AppointmentEdit => "Allow appointments to be opened for editing.",
            Self::AppointmentDelete => "Show the Delete button in the appointment editor.",
            Self::AppointmentConflictWarnings => "Warn when an appointment overlaps another booking.",
            Self::AppointmentStatusColors => "Colour appointment buttons by status.",
            Self::RecallsEnabled => "Show the patient recall list with due and overdue reminders. Uses the OD recall table.",
            Self::AsapListEnabled => "Show the ASAP list. In OD this is derived from appointment.Priority = 1, not a separate table.",
            Self::WaitlistEnabled => "Show the Waitlist tab in Appointments. This is OD's unscheduled list (AptStatus = 3), so removing someone deletes only that unscheduled row — no cancellation or reconfirmation message is generated.",
            Self::PatientCorrectionHistory => "Keep snapshots of patient edits so details missed during migration can be corrected and rolled back. Also writes a securitylog audit with the reason.",
            Self::AppointmentHistory => "Keep OD histappointment snapshots when an appointment is created, changed or removed, so a wrong time or a waitlist removal can be restored.",
            Self::AccountBalanceSummary => "Show the balance and charges summary at the top of Account.",
            Self::AccountDetailedBalance => "Break the balance into charges, adjustments, patient payments and insurance payments, following Ledgers.GetAgingQueryString.",
            Self::AccountShowAdjustments => "Show adjustment rows in the Account ledger.",
            Self::AccountShowClaims => "Show insurance claims and their procedure lines in Account. Requires Insurance & claims to be on.",
            Self::InsuranceEnabled => "Enable insurance claims, insurance payments/write-offs and estimates. Off by default for practices that use only patient payments and payment plans (e.g. Ethiopia). Turning it off hides insurance in Account and skips the claim queries entirely.",
            Self::AccountShowStatements => "Show statements in Account.",
            Self::AccountBillingEdits => "Allow recording a payment (with one paysplit) or an adjustment. Also requires the matching Open Dental permission when permission enforcement is on.",
            Self::LedgerJournalPosting => "Allow posting completed-procedure charges, patient payments and account adjustments into OD's double-entry journal, so the Balance Sheet, General Ledger and Profit & Loss reports include them. Writes one balanced debit/credit pair with a shared transaction number; charges can also be posted as an atomic batch from the accounting reports. Also requires the OD Adjustment Create permission when enforcement is on. On by default.",
            Self::PaymentPlansEnabled => "Show payment plans in Account, allow creating plans and charges, and allow applying payments to a plan. On by default because Ethiopian practices collect balances this way.",
            Self::DiscountPlans => "Show the discount-plan editor in Setup: define plans (fee schedule, adjustment type, annual maximum, frequency limits and note) and subscribe patients with effective/term dates. Discount plans are the patient-facing alternative to insurance. On by default.",
            Self::DiscountPlanAutoAdjust => "When a procedure is completed for a patient with an active discount subscription, price it from the plan's reduced fee schedule and post the difference as a negative adjustment of the plan's adjustment type. Mirrors OD Adjustments.CreateAdjustmentForDiscountPlan, including the annual maximum and frequency limits. Requires Discount plans.",
            Self::ProcedureCompletion => "Allow creating a procedure in Chart and moving it to Complete. Completing a procedure is the trigger for automatic discount-plan pricing. Also requires the OD Adjustment Create permission when enforcement is on.",
            Self::TreatmentPlanLines => "Show the procedure table inside each treatment plan.",
            Self::TreatmentPlanEdit => "Allow creating and editing treatment plans, their status and their proctp procedure lines. Also requires the OD Treat Plan Edit permission when enforcement is on.",
            Self::AuditTreatmentPlans => "Write a securitylog entry when a treatment plan or its lines are created, changed or set active.",
            Self::ChartToothSelection => "Show the clickable tooth chart in Chart.",
            Self::ReportPatientList => "Offer the patient list report.",
            Self::ReportProduction => "Offer the production by provider report.",
            Self::ReportCollections => "Offer the collections report.",
            Self::ReportSchedule => "Offer the scheduled appointments report.",
            Self::ShowImagingNote => "Show the reminder that Imaging is outside this port.",
            Self::ShowDatabaseLabel => "Show which data source is active.",
            Self::ShowPreferences => "Show the preferences table in Setup.",
            Self::ShowUsers => "Show the users table in Setup.",
            Self::ShowPermissions => "Show the permission-code reference in Setup.",
            Self::ShowAuditLog => "Show recent securitylog entries in Setup.",
            Self::RequireLogin => "Authenticate against userod. Settings stays reachable from the login screen. Turning it off unlocks the app; turning it on returns to the login screen.",
            Self::EnforcePermissions => "Gate modules and actions on the user's Open Dental group permissions. When off, only the feature toggles apply.",
            Self::AuditLogins => "Write a securitylog entry when a user logs on or off.",
            Self::AuditPatientChanges => "Write a securitylog entry when a patient is created, edited or deleted.",
            Self::AuditAppointmentChanges => "Write a securitylog entry when an appointment is created, edited or deleted.",
            Self::AuditBillingChanges => "Write a securitylog entry when a payment or adjustment is recorded.",
            Self::EServicesEnabled => "Master switch for local appointment reminders. When off, no rules are evaluated and no messages are queued. Uses your own outbox and transports — never the Open Dental HQ servers.",
            Self::RemindersEnabled => "Evaluate reminder rules for upcoming appointments and queue messages to the local outbox.",
            Self::ConfirmationsEnabled => "Evaluate future-day confirmation rules (OD ApptReminderType.ConfirmationFutureDay).",
            Self::ReminderDryRun => "Render messages and write them to the outbox with Dry run status, but do not call any transport. On by default so nothing leaves the machine until you configure a channel.",
            Self::AuditReminders => "Write a securitylog entry when reminders are generated or delivered.",
            Self::ShowDefinitions => "Show the OD definition list editor in Setup (adjustment types, payment types, colours and other drop-downs). Requires the OD Definition Edit permission when enforcement is on.",
            Self::ShowDisplayFields => "Show the OD display-field grid editor in Setup. Requires the OD Setup permission when enforcement is on.",
            Self::AuditDefinitionChanges => "Write a securitylog entry when a definition or display field is created, edited or deleted.",
            Self::PreferDemoData => "Use built-in sample data even if a database is configured. Takes effect after restarting.",
            Self::PatientRegistry => "Show the patient registry in the Treatment Plan module: one patient-centred view with contact details, payment status, existing treatments and conditions, clinic card and A5 invoices.",
            Self::ClinicCardsEnabled => "Allow a clinic card to be issued from the registry, only with a billing reason, an authorisation code and an explicit confirmation. Issuing a card never creates a statement, adjustment or payment. Cards can only be deactivated, never deleted.",
            Self::A5Invoice => "Generate a simplified A5 invoice (patient, date, items with tooth/site, unit price, discount, tax, total, amount paid and balance) and store it as a statement marked as an invoice.",
            Self::ReportRecallAsap => "Offer a recall/ASAP due list in Reports, using OD's editable recall date and appointment priority, with overdue patients first.",
            Self::ReportPaymentPlans => "Offer a payment-plan balance report: per-plan charges due, receipts and outstanding balance as of the report date, using the same OD paysplit model as Account.",
            Self::ReportTreatmentPlans => "Offer a treatment-plan summary: plan status, procedure-line count and total, grouped by patient from OD treatplan and treatplanattach/proctp lines.",
            Self::ReportAging => "Offer OD's patient aging report (FormRpAging): balances in 0-30, 31-60, 61-90 and over-90 buckets, with credits applied to the oldest debt first. Insurance columns stay hidden unless Insurance & claims is on.",
            Self::ReportPayPlanDetail => "Offer OD's payment-plan detail report (FormRpPayPlans): provider, guarantor, principal, accumulated interest, paid, balance, due now, balance not due, family balance, and stale-resolved charge count with recorded staff reasons per plan. Closed plans are hidden by default.",
            Self::ReportPayPlanChargeApplications => "Auditable payment-plan charge applications: pay date, patient, plan, charge, charge due date, debit type, amount applied, payment, and unapplied plan amount.",
            Self::ReportBirthday => "Offer OD's birthday report (FormRpBirthday): patients whose birthday falls in the selected range, comparing month and day so the birth year is ignored.",
            Self::ReportNewPatients => "Offer OD's new-patient report (FormRpNewPatients): patients created in the selected date range with provider and contact details.",
            Self::ReportAppointments => "Offer OD's appointments report (FormRpAppointments): bookings in the date range with patient, age, time, length, description and phones. Planned, unscheduled-list and note appointments are excluded unless the note option is on.",
            Self::ReportActivePatients => "Offer OD's active-patients report (FormRpActivePatients): one row per patient with a completed procedure in the range, with address details and a patient count. Billing-type, patient-status and clinic lists are not stored by this port.",
            Self::ReportProdInc => "Offer OD's production and income report (FormRpProdInc) in Daily, Monthly, Annual and Provider modes: production, adjustments, patient income and totals. Insurance income and write-offs appear only when Insurance & claims is on.",
            Self::ReportPaySheet => "Offer the patient-payment half of OD's daily payments report (FormRpPaySheet): payments received in the date range grouped by payment type, with date, paying patient, split-level provider, check number and amount, plus totals. The insurance-payment section needs claim-payment data this port does not store.",
            Self::ReportAdjSheet => "Offer OD's daily adjustments report (FormRpAdjSheet): every adjustment in the date range with patient name, provider, adjustment type, note and the stored signed amount, sorted by date. Provider and adjustment-type filters are ported; clinics do not apply to this port.",
            Self::ReportFinanceCharge => "Offer OD's finance-charge report (FormRpFinanceCharge): finance-charge adjustments in the date range with patient number, name, preferred name and the signed amount, sorted by patient name then amount. OD's provider filter matches the patient's primary provider; billing types are not stored by this port.",
            Self::ReportBrokenAppointments => "Offer OD's broken appointments report (FormRpBrokenAppointments) with all three upstream modes: by appointment status, by broken-appointment adjustment, and by ADA code D9986/D9987. Provider and date filters are ported; clinics, hygienist and IsHygiene columns are omitted because this port does not store them.",
            Self::ReportConfirmations => "Offer OD's confirmation report (FormRpConfirm): a field-selection report over appointments in the date range with the same standard/other field lists and defaults. Unmodelled fields (Preferred, Address2, WkPhone, insurance carriers, etc.) render blank, as they must when the practice does not store them.",
            Self::ReportRecallFields => "Offer OD's recall report (FormRpRecall): a field-selection report over recalls due in the date range, with the upstream standard/other field lists and the ROUND(RecallInterval/65536) month-interval formula. Unmodelled patient fields render blank.",
            Self::ReportDailyProcedures => "Offer OD's Daily Procedures report (FormRpProcSheet) with both upstream layouts: one row per completed procedure, or totals grouped by procedure code with category, quantity, average fee and total fees. Provider and code-substring filters are ported; clinics, capitation write-offs, tooth-surface Area and unit quantities are omitted because this port does not store them.",
            Self::ReportProcedureCodes => "Offer OD's Procedure Codes - Fee Schedules report (FormRpProcCodes): every CDT code with its description, abbreviation and fee from a chosen fee schedule, optionally grouped by ProcCodeCats with duplicate category names suppressed. Blank fees are hidden unless requested. Fee schedules, provider and clinic selection are ported; the default clinic (0) is used because this port has none.",
            Self::ReportPrescriptions => "Offer OD's Prescriptions report (FormRpPrescriptions): prescriptions from rxpat with patient name, date, drug, directions, dispense and prescriber abbreviation, filtered by a patient-surname or drug-name prefix and ordered exactly as upstream. The date range is ignored, matching the original form, which has no date filter.",
            Self::ReportReferralAnalysis => "Offer OD's Referral Analysis report (FormRpReferralAnalysis): referring doctors ranked by the number of distinct patients they sent and the production those patients generated, with optional address/specialty columns and a 'new patients only' mode. Production uses ProcFee x (UnitQty + BaseUnits), D9986/D9987 are excluded, and the provider multi-select is ported.",
            Self::ReportReferrals => "Offer OD's Referrals report (FormRpReferrals): the field-selection query builder over the OD referral table. Pick any of the 19 upstream columns and stack WHERE filters with LIKE, =, >, <, >=, <= or <>. Specialty filters use Provider Specialties definitions and IsHidden/NotPerson/UsingTIN use False/True, exactly as upstream's dropdowns do.",
            Self::ReportCustomAging => "Offer OD's Custom Aging report (FormRpCustomAging): patient balances in 0-30/31-60/61-90/over-90 buckets with the transaction types that make them up (procedure fees, adjustments, payment-plan charges and credits, patient payments, and the four insurance types when Insurance & claims is on), Age of Account, negative-balance handling, 'Age Credits', provider/clinic/billing-type filters, inactive/archive exclusion, family grouping by guarantor, and procedure-date or claim-payment-date write-off aging.",
            Self::ReportPatients => "Offer OD's Patients report (FormRpPatients): a field-selection query builder over the OD patient table. Pick any of the 20 supported columns (including First/Last Complete Proc and RecallStatus) and stack WHERE prerequisites with LIKE, =, >, <, >=, <= or <>. Insurance and aging-balance columns, carrier/relationship filters and columns with no field in this port's patient model are omitted.",
            Self::ReportBalanceSheet => "Chart-of-accounts balance sheet (FormRpAccountingBalanceSheet): assets, liabilities, equity, income and expense sections with OD's debit/credit sign rule and retained earnings derived from prior-year results.",
            Self::ReportGeneralLedger => "General ledger (FormRpAccountingGenLedg): per-account opening balance, dated journal lines and closing balance over the report range.",
            Self::ReportProfitLoss => "Profit & loss (FormRpAccountingProfitLoss): income and expense accounts for the range with total income, total expenses and net income.",
            Self::ReportHiddenPaySplits => "Hidden pay splits (FormRpHiddenPaySplits): unearned deposits recorded as paysplits with a hidden unearned type; optionally exclude refunds.",
            Self::ReportUnearnedIncome => "Unearned income (FormRpUnearnedIncome): line-item, accounts, allocation and net tabs over the port's unearned paysplans.",
            Self::ReportProcOverpaid => "Overpaid procedures (FormRpProcOverpaid): completed procedures where payments plus insurance exceed the fee.",
            Self::ReportReceivablesBreakdown => "Receivables breakdown (FormRpReceivablesBreakdown): aged patient balances grouped by billing type; insurance estimate column appears only when Insurance is enabled.",
            Self::ReportServiceDateView => "Service-date view (FormRpServiceDateView): charges, payments, write-offs and balance by procedure service date.",
            Self::ReportProcNote => "Incomplete procedure notes (FormRpProcNote): latest notes with prompt placeholders, optionally adding same-day patients with no note and unsigned notes.",
            Self::ReportDentalSealant => "UDS dental sealant measure (FormRpDentalSealantMeasure): per-provider numerator, denominator and percentage for children aged 6-9 in a reporting year.",
            Self::ReportDiscountPlan => "Discount plan list (FormRpDiscountPlan): description-filtered discount-plan subscribers with fee schedule, adjustment type, effective/term dates and patient. Plans and subscriptions are editable in Setup when Discount plans is on.",
            Self::ReportDppOvercharged => "Overcharged payment plans (FormRpDPPOvercharged): plans whose debits exceed the patient's portion, using the modelled payplan/payplancharge/paysplit tables.",
            Self::ReportPatientPortionUncollected => "Patient portion uncollected (FormRpPatPortionUncollected): completed procedures with a remaining patient balance after adjustments and patient payments. The core patient-payment collection report; insurance estimates are included only when Insurance & claims is on.",
            Self::ReportTreatmentFinder => "Treatment finder (FormRpTreatmentFinder): patients with treatment-planned procedures in the date range and the amount planned, excluding those with a future scheduled appointment unless requested. The insurance benefit columns appear only when Insurance & claims is on.",
            Self::ReportPhRawPop => "Raw population extract (FormRpPHRawPop). OD reads screen/site/county/patientrace tables this port does not model, so a caller must supply the rows.",
            Self::ReportPhRawScreen => "Raw screening extract (FormRpPHRawScreen). OD reads the screening tables this port does not model, so a caller must supply the rows.",
            Self::ReportRouting => "Routing slips (FormRpRouting): one slip per appointment on the selected day. Data fields are exported; OD's absolute sheet layout is outside this port's PDF engine.",
            Self::ReportLaserLabels => "Laser labels (FormRpLaserLabels): patient and birthday addresses flattened into a 30-per-page label stream. The insurance-company tab needs the unmodelled carrier table.",
            Self::ReportPrintPreview => "Print preview (FormRpPrintPreview): debug-only print host with no report query; exposed here for parity with upstream.",
            Self::ReportStatement => "Statements (FormRpStatement): per-statement charges, payments, adjustments and balance. The printable detail uses the existing simplified statement PDF.",
            Self::ReportProdGoal => "Monthly production goal (FormRpProdGoal): per-day production, scheduled value, adjustments and write-offs. The goal column is zero until schedules/hourly goals are modelled.",
            Self::ReportProviderPayroll => "Provider payroll (FormRpProviderPayroll): per-provider/day production, write-offs and patient/insurance income. Historic write-off snapshots are not modelled.",
            Self::ReportPresentedTreatment => "Presented treatment production (FormRpPresentedTreatmentProduction): net production of completed procedures that appeared on a saved treatment plan, grouped by presenter.",
            Self::ReportTpPresentationStats => "Treatment-plan presentation statistics (FormRpTreatPlanPresentationStatistics): planned/scheduled/completed counts and gross or net amounts per presenter.",
        }
    }

    pub fn category(self) -> &'static str {
        match self {
            Self::ModuleAppointments
            | Self::ModuleFamily
            | Self::ModuleAccount
            | Self::ModuleTreatmentPlan
            | Self::ModuleChart
            | Self::ModuleReports
            | Self::ModuleSetup => "Modules",
            Self::PatientSearch | Self::PatientEdit | Self::PatientDelete => "Family",
            Self::AppointmentCreate
            | Self::AppointmentEdit
            | Self::AppointmentDelete
            | Self::AppointmentConflictWarnings
            | Self::AppointmentStatusColors
            | Self::RecallsEnabled
            | Self::AsapListEnabled
            | Self::WaitlistEnabled => "Appointments",
            Self::AccountBalanceSummary
            | Self::AccountDetailedBalance
            | Self::AccountShowAdjustments
            | Self::AccountShowClaims
            | Self::InsuranceEnabled
            | Self::AccountShowStatements
            | Self::AccountBillingEdits
            | Self::LedgerJournalPosting
            | Self::PaymentPlansEnabled => "Account",
            Self::TreatmentPlanLines
            | Self::TreatmentPlanEdit
            | Self::AuditTreatmentPlans => "Treatment Plan",
            Self::ChartToothSelection | Self::ProcedureCompletion => "Chart",
            Self::ReportPatientList
            | Self::ReportProduction
            | Self::ReportCollections
            | Self::ReportSchedule
            | Self::ReportRecallAsap
            | Self::ReportPaymentPlans
            | Self::ReportTreatmentPlans
            | Self::ReportAging
            | Self::ReportPayPlanDetail
            | Self::ReportPayPlanChargeApplications
            | Self::ReportBirthday
            | Self::ReportNewPatients
            | Self::ReportAppointments
            | Self::ReportActivePatients
            | Self::ReportProdInc
            | Self::ReportPaySheet
            | Self::ReportAdjSheet
            | Self::ReportFinanceCharge
            | Self::ReportBrokenAppointments
            | Self::ReportConfirmations
            | Self::ReportRecallFields
            | Self::ReportDailyProcedures
            | Self::ReportProcedureCodes
            | Self::ReportPrescriptions
            | Self::ReportReferralAnalysis
            | Self::ReportReferrals
            | Self::ReportCustomAging
            | Self::ReportPatients
            | Self::ReportBalanceSheet
            | Self::ReportGeneralLedger
            | Self::ReportProfitLoss
            | Self::ReportHiddenPaySplits
            | Self::ReportUnearnedIncome
            | Self::ReportProcOverpaid
            | Self::ReportReceivablesBreakdown
            | Self::ReportServiceDateView
            | Self::ReportProcNote
            | Self::ReportDentalSealant
            | Self::ReportDiscountPlan
            | Self::ReportDppOvercharged
            | Self::ReportPatientPortionUncollected
            | Self::ReportTreatmentFinder
            | Self::ReportPhRawPop
            | Self::ReportPhRawScreen
            | Self::ReportRouting
            | Self::ReportLaserLabels
            | Self::ReportPrintPreview
            | Self::ReportStatement
            | Self::ReportProdGoal
            | Self::ReportProviderPayroll
            | Self::ReportPresentedTreatment
            | Self::ReportTpPresentationStats => "Reports",
            Self::EServicesEnabled
            | Self::RemindersEnabled
            | Self::ConfirmationsEnabled
            | Self::ReminderDryRun
            | Self::AuditReminders => "eServices",
            Self::ShowImagingNote | Self::ShowDatabaseLabel => "Display",
            Self::ShowPreferences
            | Self::ShowUsers
            | Self::ShowPermissions
            | Self::ShowAuditLog
            | Self::RequireLogin
            | Self::EnforcePermissions
            | Self::AuditLogins
            | Self::AuditPatientChanges
            | Self::AuditAppointmentChanges
            | Self::AuditBillingChanges
            | Self::PatientCorrectionHistory
            | Self::AppointmentHistory
            | Self::ShowDefinitions
            | Self::ShowDisplayFields
            | Self::AuditDefinitionChanges
            | Self::DiscountPlans
            | Self::DiscountPlanAutoAdjust
            | Self::PreferDemoData => "Setup & data",
            Self::PatientRegistry | Self::ClinicCardsEnabled | Self::A5Invoice => {
                "Patient registry"
            }
        }
    }

    pub fn default_enabled(self) -> bool {
        // Insurance/claims do not exist in every country (e.g. Ethiopia uses
        // patient payments and payment plans), so they start off. The code and
        // OD-compatible tables are kept and can be enabled at any time.
        !matches!(
            self,
            Self::PreferDemoData | Self::InsuranceEnabled | Self::EServicesEnabled
        )
    }

    pub fn from_config_key(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.as_str() == value)
    }
}

/// User-controlled feature switches, persisted as plain text.
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureSettings {
    values: HashMap<FeatureKey, bool>,
}

impl Default for FeatureSettings {
    fn default() -> Self {
        let mut values = HashMap::new();
        for key in FeatureKey::ALL {
            values.insert(key, key.default_enabled());
        }
        Self { values }
    }
}

impl FeatureSettings {
    pub fn enabled(&self, key: FeatureKey) -> bool {
        *self.values.get(&key).unwrap_or(&key.default_enabled())
    }

    pub fn set(&mut self, key: FeatureKey, enabled: bool) {
        self.values.insert(key, enabled);
    }

    pub fn module_enabled(&self, module: Module) -> bool {
        self.enabled(module.feature_key())
    }

    pub fn to_text(&self) -> String {
        let mut lines: Vec<String> = FeatureKey::ALL
            .into_iter()
            .map(|key| format!("{}={}", key.as_str(), self.enabled(key)))
            .collect();
        lines.sort();
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }

    pub fn from_text(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let Some(key) = FeatureKey::from_config_key(key.trim()) else {
                continue;
            };
            let enabled = matches!(value.trim(), "1" | "true" | "True" | "TRUE");
            settings.set(key, enabled);
        }
        settings
    }

    pub fn load() -> Self {
        match Self::settings_path() {
            Some(path) => std::fs::read_to_string(path)
                .map(|text| Self::from_text(&text))
                .unwrap_or_default(),
            None => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::settings_path().ok_or_else(|| "Could not find a settings folder".to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Could not create settings folder: {e}"))?;
        }
        std::fs::write(path, self.to_text())
            .map_err(|e| format!("Could not save settings: {e}"))
    }

    fn settings_path() -> Option<PathBuf> {
        if let Ok(appdata) = std::env::var("APPDATA") {
            if !appdata.trim().is_empty() {
                return Some(PathBuf::from(appdata).join("odclinic").join("settings.txt"));
            }
        }
        std::env::current_dir()
            .ok()
            .map(|dir| dir.join("odclinic-settings.txt"))
    }
}

/// Open Dental `ApptStatus`.
///
/// Values were verified against the shipped 25.3 `OpenDentBusiness.dll` with
/// REA (evidence `ev_1296df0d3b5ceb6cd4e8cd6d6aec0708ffc474030b94be4b239cb6bde286671c`).
/// `Unscheduled` here represents OD `None` (0); the two are equivalent for the
/// purposes of this port.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(i8)]
pub enum AppointmentStatus {
    #[default]
    Unscheduled = 0,
    Scheduled = 1,
    Complete = 2,
    UnschedList = 3,
    Asap = 4,
    Broken = 5,
    Planned = 6,
    PtNote = 7,
    PtNoteCompleted = 8,
}

impl AppointmentStatus {
    pub const ALL: [Self; 9] = [
        Self::Unscheduled,
        Self::Scheduled,
        Self::Complete,
        Self::UnschedList,
        Self::Asap,
        Self::Broken,
        Self::Planned,
        Self::PtNote,
        Self::PtNoteCompleted,
    ];

    pub fn from_od(v: i8) -> Self {
        match v {
            0 => Self::Unscheduled,
            1 => Self::Scheduled,
            2 => Self::Complete,
            3 => Self::UnschedList,
            4 => Self::Asap,
            5 => Self::Broken,
            6 => Self::Planned,
            7 => Self::PtNote,
            8 => Self::PtNoteCompleted,
            _ => Self::Unscheduled,
        }
    }
    pub fn as_od(self) -> i8 {
        self as i8
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Unscheduled => "Unscheduled",
            Self::Scheduled => "Scheduled",
            Self::Complete => "Complete",
            Self::UnschedList => "Unscheduled list",
            Self::Asap => "ASAP",
            Self::Broken => "Broken",
            Self::Planned => "Planned",
            Self::PtNote => "Patient note",
            Self::PtNoteCompleted => "Patient note complete",
        }
    }
}

/// Open Dental `PatientGender`. Values verified with REA in the same evidence
/// record as `AppointmentStatus`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(i8)]
pub enum Gender {
    Male = 0,
    Female = 1,
    #[default]
    Unknown = 2,
    Other = 3,
}

impl Gender {
    pub const ALL: [Self; 4] = [Self::Male, Self::Female, Self::Unknown, Self::Other];

    pub fn from_od(v: i8) -> Self {
        match v {
            0 => Self::Male,
            1 => Self::Female,
            2 => Self::Unknown,
            3 => Self::Other,
            _ => Self::Unknown,
        }
    }
    pub fn as_od(self) -> i8 {
        self as i8
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Male => "Male",
            Self::Female => "Female",
            Self::Unknown => "Unknown",
            Self::Other => "Other",
        }
    }
}

/// Open Dental `TreatPlanStatus`.
///
/// The old mapping had these values reversed; REA confirmed
/// `Saved=0, Active=1, Inactive=2` in the 25.3 assembly.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(i8)]
pub enum TreatPlanStatus {
    Saved = 0,
    #[default]
    Active = 1,
    Inactive = 2,
}

impl TreatPlanStatus {
    pub const ALL: [Self; 3] = [Self::Saved, Self::Active, Self::Inactive];

    pub fn from_od(v: i8) -> Self {
        match v {
            0 => Self::Saved,
            1 => Self::Active,
            2 => Self::Inactive,
            _ => Self::Active,
        }
    }
    pub fn as_od(self) -> i8 {
        self as i8
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Saved => "Saved",
            Self::Active => "Active",
            Self::Inactive => "Inactive",
        }
    }
}

/// Open Dental `ProcStat`. Values verified with REA
/// (evidence `ev_ee243a37f6c757b1e3b4640147951b1913eea47115165b2abe2fbaee45e6f856`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(i8)]
pub enum ProcedureStatus {
    #[default]
    TreatmentPlanned = 1,
    Complete = 2,
    ExistingCurrentProvider = 3,
    ExistingOtherProvider = 4,
    ReferredOut = 5,
    Deleted = 6,
    Condition = 7,
    TreatmentPlannedInactive = 8,
}

impl ProcedureStatus {
    pub const ALL: [Self; 8] = [
        Self::TreatmentPlanned,
        Self::Complete,
        Self::ExistingCurrentProvider,
        Self::ExistingOtherProvider,
        Self::ReferredOut,
        Self::Deleted,
        Self::Condition,
        Self::TreatmentPlannedInactive,
    ];

    pub fn from_od(v: i8) -> Self {
        match v {
            0..=1 => Self::TreatmentPlanned,
            2 => Self::Complete,
            3 => Self::ExistingCurrentProvider,
            4 => Self::ExistingOtherProvider,
            5 => Self::ReferredOut,
            6 => Self::Deleted,
            7 => Self::Condition,
            8 => Self::TreatmentPlannedInactive,
            _ => Self::TreatmentPlanned,
        }
    }
    pub fn as_od(self) -> i8 {
        self as i8
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::TreatmentPlanned => "TP",
            Self::Complete => "Complete",
            Self::ExistingCurrentProvider => "EC",
            Self::ExistingOtherProvider => "EO",
            Self::ReferredOut => "Referred",
            Self::Deleted => "Deleted",
            Self::Condition => "Condition",
            Self::TreatmentPlannedInactive => "TP inactive",
        }
    }
    /// Matches the historic balance/production behaviour: only completed
    /// procedures count as charges or production.
    pub fn is_completed(self) -> bool {
        self == Self::Complete
    }
}

/// Open Dental `ClaimStatus`.
///
/// Unlike most OD enums this is stored as a one-character **string** in
/// `claim.ClaimStatus`, not a number. The seven codes and their order come from
/// the 24.3 `Claim.cs` source; the `Claim` and `ClaimStatus` types were both
/// confirmed present in the shipped 25.3.48.0 assembly with REA
/// (evidence `ev_fc318f31f8ebbc27ada18860f5417e2658fb971dae37779767261af3a8e27e41`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ClaimStatus {
    #[default]
    Unsent,
    HoldUntilPriReceived,
    WaitingToSend,
    ProbablySent,
    Sent,
    Received,
    HoldForInProcess,
}

impl ClaimStatus {
    pub const ALL: [Self; 7] = [
        Self::Unsent,
        Self::HoldUntilPriReceived,
        Self::WaitingToSend,
        Self::ProbablySent,
        Self::Sent,
        Self::Received,
        Self::HoldForInProcess,
    ];

    pub fn from_od(value: &str) -> Self {
        match value.trim() {
            "H" => Self::HoldUntilPriReceived,
            "W" => Self::WaitingToSend,
            "P" => Self::ProbablySent,
            "S" => Self::Sent,
            "R" => Self::Received,
            "I" => Self::HoldForInProcess,
            _ => Self::Unsent,
        }
    }

    pub fn as_od(self) -> &'static str {
        match self {
            Self::Unsent => "U",
            Self::HoldUntilPriReceived => "H",
            Self::WaitingToSend => "W",
            Self::ProbablySent => "P",
            Self::Sent => "S",
            Self::Received => "R",
            Self::HoldForInProcess => "I",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Unsent => "Unsent",
            Self::HoldUntilPriReceived => "Hold until primary received",
            Self::WaitingToSend => "Waiting to send",
            Self::ProbablySent => "Probably sent",
            Self::Sent => "Sent — verified",
            Self::Received => "Received",
            Self::HoldForInProcess => "Hold for in process",
        }
    }

    /// A claim that has been accepted to the point where its procedure rows
    /// reduce the patient balance (OD ages `Status != 0` claimprocs as paid).
    pub fn is_received(self) -> bool {
        self == Self::Received
    }
}

/// A practice location (OD `clinic`).
#[derive(Clone, Debug, Default)]
pub struct Clinic {
    pub clinic_num: Id,
    pub description: String,
    pub abbr: String,
    pub is_hidden: bool,
}

/// A treatment provider (OD `provider`).
#[derive(Clone, Debug, Default)]
pub struct Provider {
    pub provider_num: Id,
    pub l_name: String,
    pub f_name: String,
    pub abbreviation: String,
    pub is_hidden: bool,
    /// OD `provider.HourlyProdGoalAmt`; the per-hour production goal used by
    /// `FormRpProdGoal`. Zero when the practice has not set a goal.
    pub hourly_prod_goal_amt: f64,
}

impl Provider {
    pub fn display_name(&self) -> String {
        format!("{}, {}", self.l_name, self.f_name)
    }
}

/// A chair/room used by the schedule (OD `operatory`).
#[derive(Clone, Debug, Default)]
pub struct Operatory {
    pub operatory_num: Id,
    pub description: String,
    pub abbrev: String,
    pub clinic_num: Id,
    pub is_hidden: bool,
}

/// A patient (OD `patient`).
#[derive(Clone, Debug, Default)]
pub struct Patient {
    pub pat_num: Id,
    pub l_name: String,
    pub f_name: String,
    pub middle_i: String,
    pub title: String,
    pub suffix: String,
    pub birth_date: Option<NaiveDate>,
    pub gender: Gender,
    pub home_phone: String,
    pub wireless_phone: String,
    pub email: String,
    pub address: String,
    pub address2: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    /// OD `patient.WkPhone` (work telephone).
    pub wk_phone: String,
    /// OD `patient.BillingType`, a FK to `definition` in `DefCat.BillingTypes`.
    pub billing_type: Id,
    /// OD `patient.ChartNumber`.
    pub chart_number: String,
    /// OD `patient.SSN`.
    pub ssn: String,
    /// OD `patient.MedicaidID`.
    pub medicaid_id: String,
    /// OD `patient.PatStatus`: Patient=0, NonPatient=1, Inactive=2,
    /// Archived=3, Deleted=4, Deceased=5, Prospective=6.
    pub pat_status: u8,
    /// OD `patient.Guarantor`, a self-FK used by Custom Aging's family
    /// grouping. Zero means the patient is their own guarantor.
    pub guarantor: Id,
    pub clinic_num: Id,
    pub primary_provider_num: Id,
    pub date_created: Option<NaiveDateTime>,
    /// OD `patient.County`; used by the raw public-health population extract.
    pub county: String,
    /// OD `patient.SiteNum`, a FK to `site.SiteNum`.
    pub site_num: Id,
    /// OD `patient.GradeLevel`.
    pub grade_level: i8,
    /// OD `patient.Urgency`.
    pub urgency: i8,
    /// OD `patient.PlannedIsDone`.
    pub planned_is_done: bool,
}

impl Patient {
    pub fn display_name(&self) -> String {
        let middle = if self.middle_i.is_empty() {
            String::new()
        } else {
            format!("{} ", self.middle_i)
        };
        format!("{}, {} {}", self.l_name, self.f_name, middle).trim().to_string()
    }
}

/// An appointment (OD `appointment`).
#[derive(Clone, Debug, Default)]
pub struct Appointment {
    pub apt_num: Id,
    pub pat_num: Id,
    pub apt_datetime: NaiveDateTime,
    pub apt_length: u32,
    pub apt_status: AppointmentStatus,
    pub operatory_num: Id,
    pub provider_num: Id,
    pub clinic_num: Id,
    pub notes: String,
    pub pattern: String,
    pub is_time_locked: bool,
    /// OD `appointment.Priority`: 1 marks an ASAP booking. The ASAP list is a
    /// filtered view of appointments, not a separate table.
    pub priority: RecallPriority,
    /// OD `appointment.UnschedStatus`: a FK to `definition.DefNum` in category
    /// `DefCat.RecallUnschedStatus` (13). Only meaningful for an appointment on
    /// the unscheduled list — this port's waitlist. It lets a clinic classify
    /// why a patient is waiting without changing the appointment status.
    pub waitlist_status: Id,
}

/// A snapshot of an appointment, mapping to OD's `histappointment` table.
///
/// OD's `HistAppointment.ToAppt()` turns a history row back into an editable
/// appointment; `from_snapshot` is the same operation here, so a bad time edit
/// or a mistaken waitlist removal can be undone.
#[derive(Clone, Debug, Default)]
pub struct AppointmentChange {
    pub change_num: Id,
    pub apt_num: Id,
    pub pat_num: Id,
    pub changed_at: NaiveDateTime,
    pub user_num: Id,
    pub action: AppointmentChangeAction,
    pub snapshot: Appointment,
}

impl AppointmentChange {
    pub fn from_snapshot(snapshot: &Appointment, action: AppointmentChangeAction) -> Self {
        Self {
            apt_num: snapshot.apt_num,
            pat_num: snapshot.pat_num,
            action,
            snapshot: snapshot.clone(),
            ..Default::default()
        }
    }
}

/// OD `HistAppointmentAction`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AppointmentChangeAction {
    #[default]
    Created,
    Changed,
    Missed,
    Cancelled,
    Deleted,
}

impl AppointmentChangeAction {
    pub const ALL: [Self; 5] = [
        Self::Created,
        Self::Changed,
        Self::Missed,
        Self::Cancelled,
        Self::Deleted,
    ];

    pub fn from_od(value: i8) -> Self {
        match value {
            1 => Self::Changed,
            2 => Self::Missed,
            3 => Self::Cancelled,
            4 => Self::Deleted,
            _ => Self::Created,
        }
    }

    pub fn as_od(self) -> i8 {
        match self {
            Self::Created => 0,
            Self::Changed => 1,
            Self::Missed => 2,
            Self::Cancelled => 3,
            Self::Deleted => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Created => "Created",
            Self::Changed => "Changed",
            Self::Missed => "Missed",
            Self::Cancelled => "Cancelled",
            Self::Deleted => "Deleted",
        }
    }
}

/// A local snapshot taken before a patient record is edited.
///
/// Open Dental has no patient-history table, so this lives in the port's own
/// `odclinic_patient_correction` table (and in `DemoStore` for demo data). It
/// exists for the two clinic issues "can't correct data missed during
/// migration" and "roll back patient details".
#[derive(Clone, Debug, Default)]
pub struct PatientCorrection {
    pub correction_num: Id,
    pub pat_num: Id,
    pub corrected_at: NaiveDateTime,
    pub user_num: Id,
    pub reason: String,
    pub before: Patient,
}

/// Scheduling priority shared by OD's `Recall.Priority` (RecallPriority) and
/// `Appointment.Priority` (ApptPriority). Both encode ASAP as 1.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RecallPriority {
    #[default]
    Normal,
    Asap,
}

impl RecallPriority {
    pub const ALL: [Self; 2] = [Self::Normal, Self::Asap];

    pub fn from_od(value: i8) -> Self {
        if value == 1 { Self::Asap } else { Self::Normal }
    }

    pub fn as_od(self) -> i8 {
        match self {
            Self::Normal => 0,
            Self::Asap => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Asap => "ASAP",
        }
    }
}

/// A patient recall (OD `recall`). `recall_interval` is OD's packed
/// `Interval` integer (8 bits each for days, weeks, months and years).
#[derive(Clone, Debug, Default)]
pub struct Recall {
    pub recall_num: Id,
    pub pat_num: Id,
    pub date_due_calc: Option<NaiveDate>,
    /// The due date actually used in reports; this is the editable one.
    pub date_due: Option<NaiveDate>,
    pub date_previous: Option<NaiveDate>,
    pub recall_interval: i32,
    /// FK to `definition.DefNum` in OD `DefCat.RecallUnschedStatus`; 0 means
    /// no status. The Definitions editor in Setup maintains those rows.
    pub recall_status: Id,
    pub note: String,
    pub is_disabled: bool,
    pub date_scheduled: Option<NaiveDate>,
    pub priority: RecallPriority,
}

/// A CDT procedure code (OD `procedurecode`).
#[derive(Clone, Debug, Default)]
pub struct ProcedureCode {
    pub code_num: Id,
    pub code: String,
    pub code_name: String,
    pub abbreviation: String,
    pub fee: f64,
    /// FK to `definition.DefNum` in OD `DefCat.ProcCodeCats` (category code 11).
    /// Used by the grouped Daily Procedures report; 0 means uncategorised.
    pub category_num: Id,
}

/// A fee for a procedure code on a schedule (OD `fee`).
#[derive(Clone, Debug, Default)]
pub struct Fee {
    pub fee_num: Id,
    pub code_num: Id,
    pub schedule_num: Id,
    pub amount: f64,
}

/// A fee schedule (OD `feesched`), used by the Procedure Codes report.
#[derive(Clone, Debug, Default)]
pub struct FeeSchedule {
    pub fee_sched_num: Id,
    pub description: String,
}

/// A prescription (OD `rxpat`), read by the Prescriptions report. Field names
/// follow the upstream `RpPrescriptions` query: `Patient Name` is resolved from
/// `patient`, `Prov Name` from `provider.abbr`; the remaining fields are copied
/// verbatim from `rxpat`. Clinics and the ePrescribe bookkeeping columns are not
/// exposed because this local-first port never sends prescriptions anywhere.
#[derive(Clone, Debug, Default)]
pub struct Prescription {
    pub rx_num: Id,
    pub pat_num: Id,
    pub rx_date: NaiveDate,
    pub drug: String,
    /// Directions as typed into OD's `Sig` field.
    pub sig: String,
    /// Dispense quantity as typed into OD's `Disp` field (free text).
    pub disp: String,
    /// FK to `provider.ProvNum`; resolved to the prescriber's abbreviation in
    /// the report, mirroring upstream's `provider.abbr` join.
    pub prov_num: Id,
}

/// An insurance plan (OD `insplan`).
#[derive(Clone, Debug, Default)]
pub struct InsPlan {
    pub plan_num: Id,
    pub carrier_num: Id,
    pub group_name: String,
    pub group_number: String,
    pub fee_sched_num: Id,
}

/// A patient's subscription to an insurance plan (OD `inssub`).
#[derive(Clone, Debug, Default)]
pub struct InsSub {
    pub sub_num: Id,
    pub plan_num: Id,
    pub subscriber_num: Id,
    pub subscriber_id: String,
}

/// A patient's insurance ordering (OD `patplan`).
#[derive(Clone, Debug, Default)]
pub struct PatPlan {
    pub pat_plan_num: Id,
    pub pat_num: Id,
    pub ins_sub_num: Id,
    pub ordinal: u8,
}

/// A completed or planned procedure (OD `proc`).
#[derive(Clone, Debug, Default)]
pub struct Procedure {
    pub proc_num: Id,
    pub pat_num: Id,
    pub code_num: Id,
    /// OD `procedurelog.AptNum`. The Monthly production report joins a
    /// scheduled appointment to its treatment-planned procedures through this
    /// FK to price the `Sched` column.
    pub apt_num: Id,
    pub proc_date: NaiveDate,
    pub proc_status: ProcedureStatus,
    pub tooth_num: String,
    pub provider_num: Id,
    pub fee: f64,
    /// OD `procedurelog.UnitQty`. The Referral Analysis report multiplies the
    /// fee by `(UnitQty + BaseUnits)`, exactly as upstream's SQL does. A missing
    /// database value is read as 0.
    pub unit_qty: f64,
    /// OD `procedurelog.BaseUnits`, the second half of the production formula.
    pub base_units: f64,
    /// OD `procedurelog.Surf` — tooth surface notation used by clinical reports.
    pub surf: String,
    /// OD `procedurelog.ToothRange` — comma/space separated primary-tooth numbers.
    pub tooth_range: String,
    /// OD `procedurelog.MedicalCode` — medical diagnosis code (sealant measure).
    pub medical_code: String,
    /// OD `procedurelog.SnomedBodySite` — SNOMED body-site code (sealant measure).
    pub snomed_body_site: String,
    pub description_override: String,
}

/// A referring doctor or source (OD `referral`). The name and address fields
/// feed `FormRpReferralAnalysis`; the remaining columns exist so
/// `FormRpReferrals` can expose the same 19 selectable fields as upstream.
#[derive(Clone, Debug, Default)]
pub struct Referral {
    pub referral_num: Id,
    pub l_name: String,
    pub f_name: String,
    pub m_name: String,
    pub title: String,
    pub address: String,
    pub address2: String,
    pub city: String,
    /// OD `ST` (state).
    pub state: String,
    pub zip: String,
    /// OD `Telephone`.
    pub telephone: String,
    /// OD `Phone2`.
    pub phone2: String,
    /// OD `EMail` (the upstream field name in `FormRpReferrals` is `Email`).
    pub email: String,
    pub note: String,
    pub ssn: String,
    /// OD `UsingTIN` — a 0/1 flag in the SELECT and filter dropdowns.
    pub using_tin: u8,
    pub is_hidden: u8,
    pub not_person: u8,
    /// FK to `definition.DefNum` in `DefCat.ProviderSpecialties` (code 35).
    /// Resolved to `definition.ItemName` by Referral Analysis when addresses
    /// show; the Referrals report emits the raw number, like upstream's SELECT.
    pub specialty_num: Id,
}

/// One patient attached to a referral source (OD `refattach`). Upstream counts
/// only `RefFrom` (1) rows; `RefTo` (0) and `RefCustom` (2) are deliberately
/// excluded from the report by the store query and again by the builder.
#[derive(Clone, Debug, Default)]
pub struct RefAttach {
    pub ref_attach_num: Id,
    pub referral_num: Id,
    pub pat_num: Id,
    /// OD `ReferralType`: RefTo = 0, RefFrom = 1, RefCustom = 2.
    pub ref_type: u8,
}

/// A treatment plan (OD `treatplan`).
///
/// Open Dental writes a number of NOT NULL bookkeeping columns that this port
/// does not expose; `MySqlStore` supplies their conventional zero values. In
/// particular `treatplan` has no `ProvNum` column — `provider_num` is kept for
/// the in-memory demo store and is ignored by the MySQL backend.
#[derive(Clone, Debug, Default)]
pub struct TreatPlan {
    pub treat_plan_num: Id,
    pub pat_num: Id,
    pub date_saved: Option<NaiveDateTime>,
    pub heading: String,
    pub status: TreatPlanStatus,
    pub provider_num: Id,
    pub note: String,
}

/// One line on a treatment plan.
///
/// OD keeps two different physical representations: Saved plans own immutable
/// `proctp` copies, while Active/Inactive plans link to `procedurelog` through
/// `treatplanattach`. This struct is the common view returned by the store so
/// the GUI can render either kind with the same grid. `line_num` is `ProcTPNum`
/// or `TreatPlanAttachNum` respectively.
#[derive(Clone, Debug, Default)]
pub struct TreatPlanLine {
    pub line_num: Id,
    pub treat_plan_num: Id,
    pub pat_num: Id,
    /// `ProcNumOrig` on `proctp`, `ProcNum` on `treatplanattach`.
    pub proc_num: Id,
    pub item_order: i32,
    pub priority: Id,
    pub tooth_num: String,
    pub surf: String,
    pub proc_code: String,
    pub description: String,
    pub fee: f64,
    pub provider_num: Id,
    pub date_tp: Option<NaiveDate>,
}

/// A payment (OD `payment`).
#[derive(Clone, Debug, Default)]
pub struct Payment {
    pub pay_num: Id,
    pub pat_num: Id,
    pub pay_date: NaiveDate,
    pub pay_amount: f64,
    pub pay_type: i8,
    pub check_num: String,
    pub note: String,
}

/// A split of a payment across procedures or a plan (OD `paysplit`).
#[derive(Clone, Debug, Default)]
pub struct PaySplit {
    pub split_num: Id,
    pub pay_num: Id,
    pub pat_num: Id,
    pub proc_num: Id,
    pub split_amount: f64,
    pub date_pay: NaiveDate,
    /// OD `ProvNum` — a split always affects exactly one provider.
    pub provider_num: Id,
    /// OD `PayPlanNum`; 0 when not attached to a payment plan.
    pub pay_plan_num: Id,
    /// OD `AdjNum`; non-zero when this split counteracts an adjustment.
    pub adj_num: Id,
    /// OD `paysplit.UnearnedType`, a FK to `definition.DefNum` in
    /// `DefCat.PaySplitUnearnedType` (29). Zero for an ordinary split; a
    /// non-zero value marks unearned income / a hidden pay split.
    pub unearned_type: Id,
    /// OD `PayPlanChargeNum`; 0 when not applied to a specific plan charge.
    pub pay_plan_charge_num: Id,
    /// OD `PayPlanDebitType`.
    pub pay_plan_debit_type: PayPlanDebitType,
}

/// OD `PayPlanDebitTypes`: whether a paysplit applied to a payment-plan charge
/// counts toward principal, interest, or is unknown (legacy/manual).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum PayPlanDebitType {
    #[default]
    Unknown = 0,
    Principal = 1,
    Interest = 2,
}

impl PayPlanDebitType {
    pub fn from_db(value: i32) -> Self {
        match value {
            1 => Self::Principal,
            2 => Self::Interest,
            _ => Self::Unknown,
        }
    }

    pub fn as_db(self) -> i32 {
        self as i32
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Principal => "Principal",
            Self::Interest => "Interest",
        }
    }
}

/// Where one allocation of a payment is credited (OD `paysplit` target).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum PaymentTarget {
    /// `ProcNum = 0` and `PayPlanNum = 0`: cash on the patient account.
    #[default]
    Unallocated,
    /// Credit the procedure identified by `ProcNum`.
    Procedure(Id),
    /// Credit the payment plan identified by `PayPlanNum`.
    PayPlan(Id),
    /// Credit a specific payment-plan charge (OD `PayPlanChargeNum`).
    PayPlanCharge { plan_num: Id, charge_num: Id },
}

impl PaymentTarget {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unallocated => "Unallocated",
            Self::Procedure(_) => "Procedure",
            Self::PayPlan(_) => "Payment plan",
            Self::PayPlanCharge { .. } => "Plan charge",
        }
    }
}

/// One line of a payment allocation. A single OD `payment` can fund many
/// `paysplit` rows across procedures, family members and payment plans.
#[derive(Clone, Copy, Debug, Default)]
pub struct PaymentAllocation {
    pub target: PaymentTarget,
    /// OD `PatNum`; 0 means the payment's own patient.
    pub pat_num: Id,
    /// OD `ProvNum`; 0 when not specified.
    pub provider_num: Id,
    pub amount: f64,
    /// OD `PayPlanDebitType` when targeting a payment-plan charge.
    pub pay_plan_debit_type: PayPlanDebitType,
    /// OD `UnearnedType` (DefCat 29). 0 for normal; non-zero for unearned/prepayment.
    pub unearned_type: Id,
}

impl Payment {
    /// OD requires the `paysplit.SplitAmt` rows to sum to `payment.PayAmt`.
    /// Enforce that invariant before either table is written.
    pub fn validate_allocations(&self, allocations: &[PaymentAllocation]) -> Result<(), String> {
        if allocations.is_empty() {
            return Err("Add at least one allocation".into());
        }
        if !self.pay_amount.is_finite() {
            return Err("Invalid payment amount".into());
        }
        let mut total = 0.0;
        for (index, allocation) in allocations.iter().enumerate() {
            if !allocation.amount.is_finite() || allocation.amount == 0.0 {
                return Err(format!("Allocation {} has an invalid amount", index + 1));
            }
            total += allocation.amount;
        }
        if (total - self.pay_amount).abs() > 0.005 {
            return Err(format!(
                "Allocations total {total:.2} but the payment is {:.2}",
                self.pay_amount
            ));
        }
        // Zero-amount payments are allowed only as explicit transfers (balanced +/- splits).
        if self.pay_amount.abs() <= 0.005 {
            let has_pos = allocations.iter().any(|a| a.amount > 0.005);
            let has_neg = allocations.iter().any(|a| a.amount < -0.005);
            if !(has_pos && has_neg) {
                return Err("A zero-amount payment must be a balanced transfer with positive and negative splits".into());
            }
        }
        let has_negative = self.pay_amount < -0.005 || allocations.iter().any(|a| a.amount < -0.005);
        if has_negative && self.note.trim().is_empty() {
            return Err("Refunds/transfers require a note explaining the reason".into());
        }
        Ok(())
    }

    /// Build the OD `paysplit` rows for this payment, validating that the
    /// allocations sum to the payment amount.
    pub fn splits(&self, allocations: &[PaymentAllocation]) -> Result<Vec<PaySplit>, String> {
        self.validate_allocations(allocations)?;
        Ok(allocations
            .iter()
            .map(|a| PaySplit {
                split_num: 0,
                pay_num: self.pay_num,
                pat_num: if a.pat_num == 0 { self.pat_num } else { a.pat_num },
                proc_num: match a.target {
                    PaymentTarget::Procedure(num) => num,
                    _ => 0,
                },
                split_amount: a.amount,
                date_pay: self.pay_date,
                provider_num: a.provider_num,
                pay_plan_num: match a.target {
                    PaymentTarget::PayPlan(num) => num,
                    PaymentTarget::PayPlanCharge { plan_num, .. } => plan_num,
                    _ => 0,
                },
                adj_num: 0,
                unearned_type: a.unearned_type,
                pay_plan_charge_num: match a.target {
                    PaymentTarget::PayPlanCharge { charge_num, .. } => charge_num,
                    _ => 0,
                },
                pay_plan_debit_type: a.pay_plan_debit_type,
            })
            .collect())
    }
}

/// An adjustment in the patient account (OD `adjustment`).
///
/// `adj_amount` is signed: positive adjustments increase the balance and
/// negative ones (discounts, write-offs) reduce it. `proc_num` is 0 unless the
/// adjustment is attached to a specific procedure. `proc_date` is OD's
/// `adjustment.ProcDate`: automatic discount-plan adjustments use the service
/// date of the linked procedure; manual ones leave it unset so the store keeps
/// using `adj_date`. Field names follow the 24.3 `Adjustment.cs` source; the
/// type is present in the 25.3 binary per the REA evidence recorded on
/// `ClaimStatus`.
#[derive(Clone, Debug, Default)]
pub struct Adjustment {
    pub adj_num: Id,
    pub adj_date: NaiveDate,
    pub adj_amount: f64,
    pub pat_num: Id,
    pub adj_type: Id,
    pub provider_num: Id,
    pub proc_num: Id,
    pub proc_date: NaiveDate,
    pub note: String,
}

/// An insurance claim (OD `claim`).
#[derive(Clone, Debug, Default)]
pub struct Claim {
    pub claim_num: Id,
    pub pat_num: Id,
    pub plan_num: Id,
    /// OD stores this as a one-character string (see `ClaimStatus`).
    pub claim_status: ClaimStatus,
    pub date_sent: Option<NaiveDate>,
    pub total_fee: f64,
    pub claim_identifier: String,
}

/// A procedure on a claim (OD `claimproc`).
#[derive(Clone, Debug, Default)]
pub struct ClaimProc {
    pub claim_proc_num: Id,
    pub claim_num: Id,
    pub proc_num: Id,
    pub plan_num: Id,
    /// OD `Status` (0 = NotReceived, 1 = Received, 4 = Supplemental, ...).
    pub status: i8,
    pub fee_billed: f64,
    pub ins_pay_amt: f64,
    pub copay_amt: f64,
    pub allowed_amt: f64,
    /// OD `WriteOff` — credited to the patient when the claim is received.
    pub write_off: f64,
    /// OD `InsPayEst` — estimated insurance payment on unreceived claims.
    pub ins_pay_est: f64,
    /// OD `DateSuppReceived` — when the supplemental insurance payment was
    /// received. Payroll books the historic write-off change on this date.
    pub date_supp_received: Option<NaiveDate>,
    /// OD `WriteOffEst` — estimated write-off captured at claim creation.
    /// `-1` means "no estimate was stored".
    pub write_off_est: f64,
    /// OD `WriteOffEstOverride` — a user-entered write-off estimate that takes
    /// precedence over `WriteOffEst`. `-1` means "no override".
    pub write_off_est_override: f64,
    /// OD `ClaimPaymentNum` — links a received claim proc to its finalized
    /// insurance payment. Zero means the income is not finalized yet.
    pub claim_payment_num: Id,
}

/// A procedure note (OD `procnote`). One or more dated notes can exist for a
/// procedure; each carries a free-text note, a user signature and a timestamp.
/// Feeds the `FormRpProcNote` report.
#[derive(Clone, Debug, Default)]
pub struct ProcNote {
    pub proc_note_num: Id,
    pub pat_num: Id,
    pub proc_num: Id,
    pub entry_datetime: NaiveDateTime,
    pub user_num: Id,
    pub note: String,
    pub signature: String,
}

/// OD `AccountType` (the chart-of-accounts classification).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(i8)]
pub enum AccountType {
    #[default]
    Asset = 0,
    Liability = 1,
    Equity = 2,
    Income = 3,
    Expense = 4,
}

impl AccountType {
    pub const ALL: [Self; 5] = [
        Self::Asset,
        Self::Liability,
        Self::Equity,
        Self::Income,
        Self::Expense,
    ];

    pub fn from_od(v: i8) -> Self {
        match v {
            0 => Self::Asset,
            1 => Self::Liability,
            2 => Self::Equity,
            3 => Self::Income,
            4 => Self::Expense,
            _ => Self::Asset,
        }
    }
    pub fn as_od(self) -> i8 {
        self as i8
    }
    /// OD `Accounts.DebitIsPos`: Asset and Expense accounts increase with a
    /// debit; Liability, Equity and Income accounts increase with a credit.
    pub fn debit_is_pos(self) -> bool {
        matches!(self, Self::Asset | Self::Expense)
    }
}

/// One row of OD's chart of accounts (`account`).
#[derive(Clone, Debug, Default)]
pub struct Account {
    pub account_num: Id,
    pub description: String,
    pub account_type: AccountType,
    pub is_retained_earnings: bool,
    pub inactive: bool,
}

/// One line of OD's double-entry journal (`journalentry`). Debit and credit
/// amounts are separate non-negative values; balanced pairs share a
/// `transaction_num`.
#[derive(Clone, Debug, Default)]
pub struct JournalEntry {
    /// OD `journalentry.JournalEntryNum`.
    pub journal_entry_num: Id,
    pub account_num: Id,
    pub date_displayed: NaiveDate,
    pub debit_amt: f64,
    pub credit_amt: f64,
    pub memo: String,
    pub splits: String,
    pub check_number: String,
    pub transaction_num: Id,
}

/// OD `PaymentSchedule` — how often a payment plan is paid.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PaymentSchedule {
    #[default]
    Monthly,
    MonthlyDayOfWeek,
    Weekly,
    BiWeekly,
    Quarterly,
}

impl PaymentSchedule {
    pub const ALL: [Self; 5] = [
        Self::Monthly,
        Self::MonthlyDayOfWeek,
        Self::Weekly,
        Self::BiWeekly,
        Self::Quarterly,
    ];

    pub fn from_od(value: i8) -> Self {
        match value {
            1 => Self::MonthlyDayOfWeek,
            2 => Self::Weekly,
            3 => Self::BiWeekly,
            4 => Self::Quarterly,
            _ => Self::Monthly,
        }
    }

    pub fn as_od(self) -> i8 {
        match self {
            Self::Monthly => 0,
            Self::MonthlyDayOfWeek => 1,
            Self::Weekly => 2,
            Self::BiWeekly => 3,
            Self::Quarterly => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Monthly => "Monthly",
            Self::MonthlyDayOfWeek => "Monthly (day of week)",
            Self::Weekly => "Weekly",
            Self::BiWeekly => "Every other week",
            Self::Quarterly => "Quarterly",
        }
    }
}

/// OD `PayPlanFrequency` — how often charges are created on a dynamic plan.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PayPlanFrequency {
    Weekly,
    EveryOtherWeek,
    OrdinalWeekday,
    #[default]
    Monthly,
    Quarterly,
}

impl PayPlanFrequency {
    pub const ALL: [Self; 5] = [
        Self::Weekly,
        Self::EveryOtherWeek,
        Self::OrdinalWeekday,
        Self::Monthly,
        Self::Quarterly,
    ];

    pub fn from_od(value: i8) -> Self {
        match value {
            0 => Self::Weekly,
            1 => Self::EveryOtherWeek,
            2 => Self::OrdinalWeekday,
            4 => Self::Quarterly,
            _ => Self::Monthly,
        }
    }

    pub fn as_od(self) -> i8 {
        match self {
            Self::Weekly => 0,
            Self::EveryOtherWeek => 1,
            Self::OrdinalWeekday => 2,
            Self::Monthly => 3,
            Self::Quarterly => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Weekly => "Weekly",
            Self::EveryOtherWeek => "Every other week",
            Self::OrdinalWeekday => "Specific day of month",
            Self::Monthly => "Monthly",
            Self::Quarterly => "Quarterly",
        }
    }
}

/// OD `PayPlanChargeType`. Debits are scheduled amounts due; credits are
/// production moved onto the plan.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PayPlanChargeType {
    #[default]
    Debit,
    Credit,
}

impl PayPlanChargeType {
    pub fn from_od(value: i8) -> Self {
        if value == 1 {
            Self::Credit
        } else {
            Self::Debit
        }
    }

    pub fn as_od(self) -> i8 {
        match self {
            Self::Debit => 0,
            Self::Credit => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Debit => "Charge due",
            Self::Credit => "Production",
        }
    }
}

/// OD `DynamicPayPlanTPOptions` — how a dynamic plan treats linked treatment
/// planned (not yet completed) procedures. Ported from `PayPlan.cs` (24_3).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(i8)]
pub enum DynamicPayPlanTPOption {
    /// OD `None` (0): only completed production is charged.
    #[default]
    None = 0,
    /// OD `AwaitComplete` (1): a TP procedure contributes no principal and
    /// issues no charges until the procedure is marked complete.
    AwaitComplete = 1,
    /// OD `TreatAsComplete` (2): TP procedures are billed as if complete.
    TreatAsComplete = 2,
}

impl DynamicPayPlanTPOption {
    pub fn from_od(value: i8) -> Self {
        match value {
            1 => Self::AwaitComplete,
            2 => Self::TreatAsComplete,
            _ => Self::None,
        }
    }

    pub fn as_od(self) -> i8 {
        self as i8
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::AwaitComplete => "Await completion",
            Self::TreatAsComplete => "Treat as complete",
        }
    }
}

/// A signed agreement to pay over time (OD `payplan`). Fields follow the 24.3
/// `PayPlan.cs` source; REA confirmed `PayPlan`, `PayPlanCharge` and
/// `PayPlanLink` still exist in 25.3.48.0 (evidence
/// `ev_7ae375936c75cc6c2abdcdd03699d78dd5b17ffef51332e252ae57daad9a5dec`).
#[derive(Clone, Debug, Default)]
pub struct PayPlan {
    pub pay_plan_num: Id,
    /// OD `payplan.PlanNum`: 0 for a patient plan, non-zero for an insurance
    /// plan. Used by the payment-plan report's plan-type filter.
    pub insurance_plan_num: Id,
    /// Patient who received the treatment.
    pub pat_num: Id,
    /// Person responsible for the payments.
    pub guarantor: Id,
    pub pay_plan_date: NaiveDate,
    pub apr: f64,
    pub note: String,
    /// Amount of completed treatment covered by the plan.
    pub completed_amt: f64,
    pub pay_schedule: PaymentSchedule,
    pub number_of_payments: i32,
    pub pay_amt: f64,
    pub down_payment: f64,
    pub is_closed: bool,
    pub charge_frequency: PayPlanFrequency,
    pub date_pay_plan_start: Option<NaiveDate>,
    /// OD `payplan.DateInterestStart`. Periodic interest starts accruing once
    /// a charge's due date reaches this date. `None` falls back to the first
    /// payment date, which is what the plan editor uses when APR is entered.
    pub date_interest_start: Option<NaiveDate>,
    pub is_dynamic: bool,
    /// OD `payplan.DynamicPayPlanTPOption`. Only meaningful when `is_dynamic`.
    pub dynamic_pay_plan_tp_option: DynamicPayPlanTPOption,
    pub is_locked: bool,
}

/// One dated charge on a payment plan (OD `payplancharge`). The amount that
/// hits the account is `principal + interest`.
#[derive(Clone, Debug, Default)]
pub struct PayPlanCharge {
    pub pay_plan_charge_num: Id,
    pub pay_plan_num: Id,
    pub guarantor: Id,
    pub pat_num: Id,
    pub charge_date: NaiveDate,
    pub principal: f64,
    pub interest: f64,
    pub note: String,
    pub provider_num: Id,
    pub proc_num: Id,
    pub charge_type: PayPlanChargeType,
    /// OD `payplancharge.FKey`; the linked procedure/adjustment for dynamic
    /// plans (0 for legacy charges).
    pub fkey: Id,
    /// OD `payplancharge.LinkType` (`PayPlanLinkType`).
    pub link_type: i8,
}

/// A billing statement (OD `statement`).
#[derive(Clone, Debug, Default)]
pub struct Statement {
    pub statement_num: Id,
    pub pat_num: Id,
    pub date_sent: Option<NaiveDate>,
    pub note: String,
    pub is_invoice: bool,
}

/// Tooth-specific clinical finding (OD `toothinitial`).
#[derive(Clone, Debug, Default)]
pub struct ToothInitial {
    pub tooth_initial_num: Id,
    pub pat_num: Id,
    pub tooth_num: String,
    pub initial_type: i8,
    pub value: String,
}

/// Problem status (OD `ProblemStatus`, verified in 25.3.48.0 by REA).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProblemStatus {
    #[default]
    Active = 0,
    Resolved = 1,
    Inactive = 2,
}

impl ProblemStatus {
    pub fn from_od(value: i8) -> Self {
        match value {
            1 => Self::Resolved,
            2 => Self::Inactive,
            _ => Self::Active,
        }
    }

    pub fn as_od(self) -> i8 {
        self as i8
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Resolved => "Resolved",
            Self::Inactive => "Inactive",
        }
    }
}

/// A diagnosed condition (OD `disease`, with the name resolved from
/// `diseasedef`). Surfaced on the patient registry's clinical record.
#[derive(Clone, Debug, Default)]
pub struct Disease {
    pub disease_num: Id,
    pub pat_num: Id,
    pub disease_def_num: Id,
    pub disease_name: String,
    pub note: String,
    pub status: ProblemStatus,
    pub date_start: NaiveDate,
    pub date_stop: Option<NaiveDate>,
}

/// Free-text clinical notes for a patient (OD `patientnote`). The `medical`
/// and `treatment` fields are the existing conditions and treatments that the
/// registry folds onto one screen.
#[derive(Clone, Debug, Default)]
pub struct PatientNote {
    pub pat_num: Id,
    pub medical: String,
    pub treatment: String,
    pub service: String,
    pub medical_company: String,
    pub fam_financial: String,
}

/// An existing treatment on the clinical record, read from OD `procedurelog`.
#[derive(Clone, Debug, Default)]
pub struct RegistryTreatment {
    pub proc_num: Id,
    pub pat_num: Id,
    pub proc_date: NaiveDate,
    pub description: String,
    pub tooth_site: String,
    pub fee: f64,
    /// OD `ProcStat`: `TP=1`, `C=2`, `EC=3`, `EO=4`, `R=5`, `D=6`, `Cn=7`, `TPi=8`.
    pub proc_status: i8,
}

impl RegistryTreatment {
    pub fn status_label(&self) -> &'static str {
        match self.proc_status {
            1 => "Planned",
            2 => "Completed",
            3 => "Essentially complete",
            4 => "Existing elsewhere",
            5 => "Referred",
            6 => "Deleted",
            7 => "Cancelled",
            8 => "Inactive planned",
            _ => "Unknown",
        }
    }
}

/// Why a clinic card is being issued. A card is only allowed when billing
/// genuinely needs one — never as a side effect of opening the patient.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ClinicCardReason {
    #[default]
    CorporateAccount,
    PaymentPlan,
    ThirdPartyBilling,
    Other,
}

impl ClinicCardReason {
    pub const ALL: [Self; 4] = [
        Self::CorporateAccount,
        Self::PaymentPlan,
        Self::ThirdPartyBilling,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CorporateAccount => "corporate_account",
            Self::PaymentPlan => "payment_plan",
            Self::ThirdPartyBilling => "third_party_billing",
            Self::Other => "other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::CorporateAccount => "Corporate account",
            Self::PaymentPlan => "Payment plan",
            Self::ThirdPartyBilling => "Third-party billing",
            Self::Other => "Other billing requirement",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "payment_plan" => Self::PaymentPlan,
            "third_party_billing" => Self::ThirdPartyBilling,
            "other" => Self::Other,
            _ => Self::CorporateAccount,
        }
    }
}

/// A clinic card issued from the patient registry. This is port-local: Open
/// Dental has no clinic-card table, and issuing one deliberately writes no
/// statement, adjustment or payment row. Cards are deactivated, never deleted.
#[derive(Clone, Debug, Default)]
pub struct ClinicCard {
    pub card_num: Id,
    pub pat_num: Id,
    pub card_number: String,
    pub issued_at: NaiveDate,
    pub issued_by: Id,
    pub reason: ClinicCardReason,
    pub authorization: String,
    pub note: String,
    pub is_active: bool,
}

/// One charge on an A5 invoice. Discounts are currency amounts per line; tax
/// is a percentage applied to the discounted line amount.
#[derive(Clone, Debug)]
pub struct InvoiceLine {
    pub description: String,
    pub tooth_site: String,
    pub quantity: f64,
    pub unit_price: f64,
    pub discount: f64,
    pub tax_rate: f64,
}

impl Default for InvoiceLine {
    fn default() -> Self {
        Self {
            description: String::new(),
            tooth_site: String::new(),
            quantity: 1.0,
            unit_price: 0.0,
            discount: 0.0,
            tax_rate: 0.0,
        }
    }
}

impl InvoiceLine {
    pub fn net(&self) -> f64 {
        (self.unit_price * self.quantity - self.discount).max(0.0)
    }

    pub fn tax(&self) -> f64 {
        self.net() * self.tax_rate / 100.0
    }

    pub fn total(&self) -> f64 {
        self.net() + self.tax()
    }
}

/// A simplified patient invoice laid out for A5 paper.
#[derive(Clone, Debug, Default)]
pub struct InvoiceDocument {
    pub pat_num: Id,
    pub patient_name: String,
    pub invoice_date: NaiveDate,
    pub lines: Vec<InvoiceLine>,
    pub amount_paid: f64,
    pub note: String,
}

impl InvoiceDocument {
    pub fn subtotal(&self) -> f64 {
        self.lines.iter().map(InvoiceLine::total).sum()
    }

    pub fn discount_total(&self) -> f64 {
        self.lines.iter().map(|line| line.discount).sum()
    }

    pub fn tax_total(&self) -> f64 {
        self.lines.iter().map(InvoiceLine::tax).sum()
    }

    pub fn total(&self) -> f64 {
        self.subtotal()
    }

    pub fn balance(&self) -> f64 {
        self.total() - self.amount_paid
    }
}

/// A medication taken by a patient (OD `medicationpat`).
#[derive(Clone, Debug, Default)]
pub struct MedicationPat {
    pub medication_pat_num: Id,
    pub pat_num: Id,
    pub medication_num: Id,
    pub date_start: NaiveDate,
    pub note: String,
}

/// Vitals (OD `vitalsign`).
#[derive(Clone, Debug, Default)]
pub struct Vitalsign {
    pub vital_sign_num: Id,
    pub pat_num: Id,
    pub date_taken: NaiveDate,
    pub height: f64,
    pub weight: f64,
    pub blood_pressure_systolic: i32,
    pub blood_pressure_diastolic: i32,
    pub pulse: i32,
    pub temperature: f64,
}

/// An Open Dental `EnumPermType` code.
///
/// The codes used by this port are verified against the 24.3 documentation
/// (`DocumentationBuilder/OpenDentalDocumentation.xml`) and, for the module
/// codes, against the shipped 25.3 assembly with REA. Unknown codes are kept
/// intact so permissions from a real database are never discarded.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Permission(pub i16);

impl Permission {
    pub const NONE: Self = Self(0);
    pub const APPOINTMENTS_MODULE: Self = Self(1);
    pub const FAMILY_MODULE: Self = Self(2);
    pub const ACCOUNT_MODULE: Self = Self(3);
    pub const TP_MODULE: Self = Self(4);
    pub const CHART_MODULE: Self = Self(5);
    pub const SETUP: Self = Self(8);
    pub const PAYMENT_CREATE: Self = Self(15);
    pub const PAYMENT_EDIT: Self = Self(16);
    pub const ADJUSTMENT_CREATE: Self = Self(17);
    pub const ADJUSTMENT_EDIT: Self = Self(18);
    pub const REPORTS: Self = Self(22);
    pub const BILLING: Self = Self(62);
    pub const SECURITY_ADMIN: Self = Self(24);
    pub const APPOINTMENT_CREATE: Self = Self(25);
    pub const APPOINTMENT_EDIT: Self = Self(27);
    pub const TREAT_PLAN_EDIT: Self = Self(38);
    pub const USER_LOG_ON_OFF: Self = Self(83);
    pub const PATIENT_CREATE: Self = Self(106);
    pub const PATIENT_EDIT: Self = Self(108);
    pub const AUDIT_TRAIL: Self = Self(122);
    /// OD `EnumPermType.DefEdit`, verified in the 25.3 assembly.
    pub const DEF_EDIT: Self = Self(220);
    pub const APPOINTMENT_DELETE: Self = Self(237);

    /// Permissions this port checks, with the codes shown in Setup.
    pub const ALL: [Self; 23] = [
        Self::NONE,
        Self::APPOINTMENTS_MODULE,
        Self::FAMILY_MODULE,
        Self::ACCOUNT_MODULE,
        Self::TP_MODULE,
        Self::CHART_MODULE,
        Self::SETUP,
        Self::PAYMENT_CREATE,
        Self::PAYMENT_EDIT,
        Self::ADJUSTMENT_CREATE,
        Self::ADJUSTMENT_EDIT,
        Self::REPORTS,
        Self::BILLING,
        Self::SECURITY_ADMIN,
        Self::APPOINTMENT_CREATE,
        Self::APPOINTMENT_EDIT,
        Self::TREAT_PLAN_EDIT,
        Self::USER_LOG_ON_OFF,
        Self::PATIENT_CREATE,
        Self::PATIENT_EDIT,
        Self::AUDIT_TRAIL,
        Self::DEF_EDIT,
        Self::APPOINTMENT_DELETE,
    ];

    pub fn from_od(value: i16) -> Self {
        Self(value)
    }

    pub fn as_od(self) -> i16 {
        self.0
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NONE => "None",
            Self::APPOINTMENTS_MODULE => "Appointments module",
            Self::FAMILY_MODULE => "Family module",
            Self::ACCOUNT_MODULE => "Account module",
            Self::TP_MODULE => "Treatment Plan module",
            Self::CHART_MODULE => "Chart module",
            Self::SETUP => "Setup",
            Self::PAYMENT_CREATE => "Payment Create",
            Self::PAYMENT_EDIT => "Payment Edit",
            Self::ADJUSTMENT_CREATE => "Adjustment Create",
            Self::ADJUSTMENT_EDIT => "Adjustment Edit",
            Self::REPORTS => "Reports",
            Self::BILLING => "Billing",
            Self::SECURITY_ADMIN => "Security Admin",
            Self::APPOINTMENT_CREATE => "Appointment Create",
            Self::APPOINTMENT_EDIT => "Appointment Edit",
            Self::TREAT_PLAN_EDIT => "Treat Plan Edit",
            Self::USER_LOG_ON_OFF => "User Log On/Off",
            Self::PATIENT_CREATE => "Patient Create",
            Self::PATIENT_EDIT => "Patient Edit",
            Self::AUDIT_TRAIL => "Audit Trail",
            Self::DEF_EDIT => "Definition Edit",
            Self::APPOINTMENT_DELETE => "Appointment Delete",
            _ => "Other",
        }
    }

    /// The permission that opens a module in Open Dental.
    pub fn for_module(module: Module) -> Self {
        match module {
            Module::Appointments => Self::APPOINTMENTS_MODULE,
            Module::Family => Self::FAMILY_MODULE,
            Module::Account => Self::ACCOUNT_MODULE,
            Module::TreatmentPlan => Self::TP_MODULE,
            Module::Chart => Self::CHART_MODULE,
            Module::Reports => Self::REPORTS,
            Module::Setup => Self::SETUP,
        }
    }
}

/// The authenticated user for the current session.
#[derive(Clone, Debug)]
pub struct Session {
    pub user_num: Id,
    pub user_name: String,
    pub permissions: Vec<Permission>,
}

impl Session {
    /// An implicit session used when the login requirement is turned off.
    pub fn local() -> Self {
        Self {
            user_num: 0,
            user_name: "Local user".into(),
            permissions: Permission::ALL.to_vec(),
        }
    }

    pub fn has(&self, permission: Permission) -> bool {
        self.permissions.contains(&permission)
    }
}

/// One row of the OD `securitylog` audit table.
#[derive(Clone, Debug)]
pub struct SecurityEvent {
    pub security_log_num: Id,
    pub perm_type: Permission,
    pub user_num: Id,
    pub log_datetime: NaiveDateTime,
    pub log_text: String,
    pub pat_num: Id,
}

/// A login (OD `userod`).
#[derive(Clone, Debug, Default)]
pub struct Userod {
    pub user_num: Id,
    pub user_name: String,
    /// Encoded `HashType$Salt$Hash` value from `userod.Password`.
    pub password: String,
    /// Deprecated single group, still honoured as a fallback.
    pub user_group_num: Id,
    pub employee_num: Id,
    pub clinic_num: Id,
    pub is_hidden: bool,
}

/// A grant in OD `grouppermission`.
#[derive(Clone, Debug, Default)]
pub struct GroupPermission {
    pub group_perm_num: Id,
    pub user_group_num: Id,
    pub perm_type: Permission,
    pub permission_level: i16,
    pub fkey: Id,
}

impl Default for Permission {
    fn default() -> Self {
        Self::NONE
    }
}

/// Links a user to a group in OD `usergroupattach`.
#[derive(Clone, Debug, Default)]
pub struct UserGroupAttach {
    pub user_group_attach_num: Id,
    pub user_num: Id,
    pub user_group_num: Id,
}

/// A preference row (OD `preference` / `preferences`).
#[derive(Clone, Debug, Default)]
pub struct Pref {
    pub pref_num: Id,
    pub pref_name: String,
    pub pref_value: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appointment_status_round_trips_od_values() {
        for status in AppointmentStatus::ALL {
            assert_eq!(AppointmentStatus::from_od(status.as_od()), status);
        }
        assert_eq!(AppointmentStatus::from_od(5), AppointmentStatus::Broken);
        assert_eq!(AppointmentStatus::from_od(6), AppointmentStatus::Planned);
    }

    #[test]
    fn gender_round_trips_od_values() {
        for gender in Gender::ALL {
            assert_eq!(Gender::from_od(gender.as_od()), gender);
        }
        assert_eq!(Gender::from_od(3), Gender::Other);
    }

    #[test]
    fn treat_plan_status_matches_od_values() {
        assert_eq!(TreatPlanStatus::Saved.as_od(), 0);
        assert_eq!(TreatPlanStatus::Active.as_od(), 1);
        assert_eq!(TreatPlanStatus::Inactive.as_od(), 2);
        for status in TreatPlanStatus::ALL {
            assert_eq!(TreatPlanStatus::from_od(status.as_od()), status);
        }
    }

    #[test]
    fn procedure_status_round_trips_od_values() {
        for status in ProcedureStatus::ALL {
            assert_eq!(ProcedureStatus::from_od(status.as_od()), status);
        }
        assert!(ProcedureStatus::Complete.is_completed());
        assert!(!ProcedureStatus::TreatmentPlanned.is_completed());
    }

    #[test]
    fn claim_status_round_trips_od_string_codes() {
        for status in ClaimStatus::ALL {
            assert_eq!(ClaimStatus::from_od(status.as_od()), status);
        }
        assert_eq!(ClaimStatus::from_od("U"), ClaimStatus::Unsent);
        assert_eq!(ClaimStatus::from_od("R"), ClaimStatus::Received);
        assert_eq!(ClaimStatus::from_od(""), ClaimStatus::Unsent);
        assert_eq!(ClaimStatus::Sent.as_od(), "S");
        assert!(ClaimStatus::Received.is_received());
        assert!(!ClaimStatus::Sent.is_received());
    }

    #[test]
    fn payment_plan_enums_round_trip_od_codes() {
        for schedule in PaymentSchedule::ALL {
            assert_eq!(PaymentSchedule::from_od(schedule.as_od()), schedule);
        }
        for frequency in PayPlanFrequency::ALL {
            assert_eq!(PayPlanFrequency::from_od(frequency.as_od()), frequency);
        }
        assert_eq!(PaymentSchedule::from_od(3), PaymentSchedule::BiWeekly);
        assert_eq!(PayPlanFrequency::from_od(3), PayPlanFrequency::Monthly);
        assert_eq!(PayPlanChargeType::from_od(1), PayPlanChargeType::Credit);
        assert_eq!(PayPlanChargeType::Debit.as_od(), 0);
        assert!(FeatureSettings::default().enabled(FeatureKey::PaymentPlansEnabled));
    }

    #[test]
    fn insurance_is_off_by_default_but_can_be_turned_on() {
        let defaults = FeatureSettings::default();
        assert!(!defaults.enabled(FeatureKey::InsuranceEnabled));
        assert!(!FeatureKey::InsuranceEnabled.default_enabled());

        let mut settings = defaults.clone();
        settings.set(FeatureKey::InsuranceEnabled, true);
        let parsed = FeatureSettings::from_text(&settings.to_text());
        assert!(parsed.enabled(FeatureKey::InsuranceEnabled));
        assert_eq!(
            FeatureKey::from_config_key("insurance_enabled"),
            Some(FeatureKey::InsuranceEnabled)
        );
    }

    #[test]
    fn feature_settings_round_trip_as_text() {
        let mut settings = FeatureSettings::default();
        settings.set(FeatureKey::ModuleFamily, false);
        settings.set(FeatureKey::PreferDemoData, true);
        let parsed = FeatureSettings::from_text(&settings.to_text());
        assert!(!parsed.enabled(FeatureKey::ModuleFamily));
        assert!(parsed.enabled(FeatureKey::PreferDemoData));
        assert!(parsed.enabled(FeatureKey::ModuleAppointments));
    }

    #[test]
    fn every_module_has_a_feature_switch() {
        for module in Module::ALL {
            assert!(FeatureSettings::default().module_enabled(module));
        }
    }
}
