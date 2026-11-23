//! Persisted preferences for the Reports module.
//!
//! These are deliberately kept separate from `FeatureSettings` (the on/off
//! switches) and `EServicesConfig` (the message transports). They hold the
//! small, practice-specific values that make exported reports useful: the
//! practice name printed on each export, the last export path per report, and
//! the option checkboxes of the ported `FormRp*` forms. Defaults mirror the
//! upstream form defaults so a fresh install behaves like Open Dental.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::Id;
use crate::reports_ext::{
    AccountingReportOptions, HiddenPaySplitsReportOptions, LaserInsMode, LaserLabelOptions,
    LaserLabelTab, PayPlanChargeApplicationsReportOptions, ProcNoteGroup, ProcNoteReportOptions,
    ProcOverpaidReportOptions, TreatmentFinderReportOptions, UnearnedIncomeReportOptions,
    UnearnedIncomeTab,
};

/// `FormRpAging` → "Age of Account" radio group.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AgingAgeFilter {
    #[default]
    Any,
    Over30,
    Over60,
    Over90,
}

impl AgingAgeFilter {
    pub const ALL: [Self; 4] = [Self::Any, Self::Over30, Self::Over60, Self::Over90];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Over30 => "over30",
            Self::Over60 => "over60",
            Self::Over90 => "over90",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "over30" => Self::Over30,
            "over60" => Self::Over60,
            "over90" => Self::Over90,
            _ => Self::Any,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Any => "Any Balance",
            Self::Over30 => "Over 30 Days",
            Self::Over60 => "Over 60 Days",
            Self::Over90 => "Over 90 Days",
        }
    }
}

/// `FormRpPayPlans` → "Payment Plan Types" radio group.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlanTypeFilter {
    #[default]
    Both,
    Patient,
    Insurance,
}

impl PlanTypeFilter {
    pub const ALL: [Self; 3] = [Self::Both, Self::Patient, Self::Insurance];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Both => "both",
            Self::Patient => "patient",
            Self::Insurance => "insurance",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "patient" => Self::Patient,
            "insurance" => Self::Insurance,
            _ => Self::Both,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Both => "Both",
            Self::Patient => "Patient",
            Self::Insurance => "Insurance",
        }
    }
}

/// Options from the original `FormRpAging` that change rows or columns.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AgingOptions {
    pub age: AgingAgeFilter,
    pub include_negative: bool,
    pub only_negative: bool,
    pub show_last_pay: bool,
    pub age_writeoffs: bool,
}

/// `FormRpCustomAging` → "Include the following" transaction-type checkboxes.
/// The four insurance types are gated by the Insurance & claims feature toggle
/// at runtime; they default off for the insurance-free Ethiopia domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CustomAgingInclude {
    pub procedure_fees: bool,
    pub adjustments: bool,
    pub pay_plan_charges: bool,
    pub pay_plan_production: bool,
    pub patient_payments: bool,
    /// `claimproc.InsPayAmt` for statuses 1/4/5/7, dated `DateCP`.
    pub insurance_payments: bool,
    /// `claimproc.InsPayEst` for status 0, dated `DateCP`.
    pub insurance_estimates: bool,
    /// Actual `claimproc.WriteOff` using the port's procedure-date mode.
    pub write_offs: bool,
    /// `claimproc.WriteOffEst` for not-received claims, dated `ProcDate`.
    pub write_off_estimates: bool,
}

impl Default for CustomAgingInclude {
    fn default() -> Self {
        // FormRpCustomAging_Load with the traditional payment-plan version:
        // charges are off, production is on; every other non-insurance type is
        // checked in the designer. Insurance types are off by default here.
        Self {
            procedure_fees: true,
            adjustments: true,
            pay_plan_charges: false,
            pay_plan_production: true,
            patient_payments: true,
            insurance_payments: false,
            insurance_estimates: false,
            write_offs: false,
            write_off_estimates: false,
        }
    }
}

impl CustomAgingInclude {
    /// The form refuses to refresh unless at least one type is selected.
    pub fn any(self) -> bool {
        self.procedure_fees
            || self.adjustments
            || self.pay_plan_charges
            || self.pay_plan_production
            || self.patient_payments
            || self.insurance_payments
            || self.insurance_estimates
            || self.write_offs
            || self.write_off_estimates
    }
}

/// `AgingOptions.FamilyGrouping`: whether each row is an individual patient or
/// the guarantor of a family. Upstream's default is `Individual`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum AgingFamilyGrouping {
    #[default]
    Individual = 0,
    Family = 1,
}

impl AgingFamilyGrouping {
    pub const ALL: [Self; 2] = [Self::Individual, Self::Family];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Individual => "individual",
            Self::Family => "family",
        }
    }
    pub fn from_str(value: &str) -> Self {
        if value.trim().eq_ignore_ascii_case("family") {
            Self::Family
        } else {
            Self::Individual
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Individual => "Individual",
            Self::Family => "Family",
        }
    }
}

/// Upstream `PPOWriteoffDateCalc`: which date ages an actual write-off. The
/// port has no `claimproc.DateCP`, so `InsPayDate` is not modelled; the two
/// supported modes are the procedure date and the claim-payment date carried
/// by `claimsnapshot`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum CustomAgingWriteoffDate {
    #[default]
    ProcedureDate = 0,
    ClaimPayDate = 1,
}

impl CustomAgingWriteoffDate {
    pub const ALL: [Self; 2] = [Self::ProcedureDate, Self::ClaimPayDate];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProcedureDate => "procedure_date",
            Self::ClaimPayDate => "claim_pay_date",
        }
    }
    pub fn from_str(value: &str) -> Self {
        if value.trim().eq_ignore_ascii_case("claim_pay_date") {
            Self::ClaimPayDate
        } else {
            Self::ProcedureDate
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::ProcedureDate => "Procedure date",
            Self::ClaimPayDate => "Claim payment date",
        }
    }
}

/// `FormRpCustomAging` → "Negative Balances" radio group.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CustomAgingNegative {
    #[default]
    Exclude,
    Include,
    ShowOnly,
}

impl CustomAgingNegative {
    pub const ALL: [Self; 3] = [Self::Exclude, Self::Include, Self::ShowOnly];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exclude => "exclude",
            Self::Include => "include",
            Self::ShowOnly => "show_only",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "include" => Self::Include,
            "show_only" | "showonly" => Self::ShowOnly,
            _ => Self::Exclude,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Exclude => "Exclude",
            Self::Include => "Include",
            Self::ShowOnly => "Show Only",
        }
    }
}

/// Options from the original `FormRpCustomAging` that change rows or columns.
/// Family grouping, billing-type filtering and inactive/archive exclusion are
/// now modelled; all of them use the same patient columns as upstream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomAgingReportOptions {
    pub include: CustomAgingInclude,
    pub age: AgingAgeFilter,
    pub negative: CustomAgingNegative,
    pub age_credits: bool,
    pub all_providers: bool,
    pub provider_nums: Vec<Id>,
    pub all_clinics: bool,
    pub clinic_nums: Vec<Id>,
    pub exclude_bad_address: bool,
    /// `Individual` (upstream default) or `Family` (group by `Guarantor`).
    pub family_grouping: AgingFamilyGrouping,
    /// Empty means "all billing types", matching upstream's unfiltered list.
    pub bill_type_nums: Vec<Id>,
    pub exclude_inactive: bool,
    pub exclude_archive: bool,
    /// How actual write-offs are dated when the Write-offs type is selected.
    pub writeoff_date: CustomAgingWriteoffDate,
}

impl Default for CustomAgingReportOptions {
    fn default() -> Self {
        Self {
            include: CustomAgingInclude::default(),
            age: AgingAgeFilter::Any,
            negative: CustomAgingNegative::Exclude,
            age_credits: false,
            all_providers: true,
            provider_nums: Vec::new(),
            all_clinics: true,
            clinic_nums: Vec::new(),
            exclude_bad_address: false,
            family_grouping: AgingFamilyGrouping::default(),
            bill_type_nums: Vec::new(),
            exclude_inactive: false,
            exclude_archive: false,
            writeoff_date: CustomAgingWriteoffDate::default(),
        }
    }
}

/// Options from the original `FormRpPayPlans` that change rows or columns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PayPlanReportOptions {
    pub plan_type: PlanTypeFilter,
    pub show_family_balance: bool,
    pub limit_date_range: bool,
    pub hide_completed: bool,
    /// Show the audit count and recorded reasons for zeroed/annotated
    /// "stale resolved" charge rows. On by default so financial history stays
    /// visible; staff can hide it.
    pub show_stale_resolved: bool,
    /// Limit the report to plans that have at least one stale-resolved charge.
    /// Used to produce an audit subset that can be exported with the reasons.
    pub only_stale_resolved: bool,
}

impl Default for PayPlanReportOptions {
    fn default() -> Self {
        Self {
            plan_type: PlanTypeFilter::Both,
            show_family_balance: false,
            limit_date_range: true, // checked in the upstream form
            hide_completed: false,  // unchecked in the upstream form
            show_stale_resolved: true,
            only_stale_resolved: false,
        }
    }
}

/// Options from the original `FormRpNewPatients`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NewPatientReportOptions {
    pub include_address: bool,
    pub exclude_no_production: bool,
}

/// `FormRpAppointments` → "Appointment Date" vs "Appointment Date Created".
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AppointmentDateBasis {
    #[default]
    Appointment,
    Created,
}

impl AppointmentDateBasis {
    pub const ALL: [Self; 2] = [Self::Appointment, Self::Created];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Appointment => "appointment",
            Self::Created => "created",
        }
    }

    pub fn from_str(value: &str) -> Self {
        if value.trim().eq_ignore_ascii_case("created") {
            Self::Created
        } else {
            Self::Appointment
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Appointment => "Appointment Date",
            Self::Created => "Appointment Date Created",
        }
    }
}

/// Options from the original `FormRpAppointments`. WebSched and clinic filters
/// are not applicable to this local-first port and are intentionally omitted.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AppointmentReportOptions {
    pub date_basis: AppointmentDateBasis,
    pub show_note_appointments: bool,
}

/// Options from the original `FormRpActivePatients` that this port can honour.
/// Billing types, patient status and clinics are not stored on `Patient`, so
/// the corresponding upstream lists are omitted rather than shown as fake.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivePatientReportOptions {
    pub all_providers: bool,
    /// OD `provider.ProvNum`, used when `all_providers` is false.
    pub provider_num: u32,
}

impl Default for ActivePatientReportOptions {
    fn default() -> Self {
        Self {
            all_providers: true, // "All" is checked in the upstream form
            provider_num: 0,
        }
    }
}

/// `FormRpProdInc` → "Report Type". All four upstream modes are ported;
/// insurance-dependent columns are gated by the Insurance & claims toggle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProdIncMode {
    #[default]
    Daily,
    Monthly,
    Annual,
    Provider,
}

impl ProdIncMode {
    pub const ALL: [Self; 4] = [Self::Daily, Self::Monthly, Self::Annual, Self::Provider];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Monthly => "monthly",
            Self::Annual => "annual",
            Self::Provider => "provider",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "monthly" => Self::Monthly,
            "annual" => Self::Annual,
            "provider" => Self::Provider,
            _ => Self::Daily,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Daily => "Daily",
            Self::Monthly => "Monthly",
            Self::Annual => "Annual",
            Self::Provider => "Provider",
        }
    }
}

/// Options from the original `FormRpProdInc`. Insurance columns and
/// write-offs are additionally gated by the Insurance & claims feature toggle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProdIncReportOptions {
    pub mode: ProdIncMode,
    /// Mirrors the form's "Include Unearned Income" checkbox, which is checked
    /// by default. When on, `paysplit.UnearnedType != 0` splits appear in a
    /// separate "Unearned Pt Income" column instead of "Pt Income".
    pub include_unearned: bool,
}

impl Default for ProdIncReportOptions {
    fn default() -> Self {
        Self {
            mode: ProdIncMode::default(),
            include_unearned: true,
        }
    }
}

/// `FormRpPaySheet` → "Group By": `radioCheck` (one row per payment) versus
/// `radioPatient` (add `patient.PatNum` to the SQL GROUP BY). The upstream form
/// checks "Check" by default.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PaySheetGroupBy {
    #[default]
    Check,
    Patient,
}

impl PaySheetGroupBy {
    pub const ALL: [Self; 2] = [Self::Check, Self::Patient];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Patient => "patient",
        }
    }

    pub fn from_str(value: &str) -> Self {
        if value.trim().eq_ignore_ascii_case("patient") {
            Self::Patient
        } else {
            Self::Check
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Check => "Check",
            Self::Patient => "Patient",
        }
    }
}

/// Options from the original `FormRpPaySheet` that this port can honour.
///
/// Clinics, CareCredit/PayConnect merchant fees, online-payment splitting and
/// unearned prepayments are not stored on this port's `Payment`/`PaySplit`, so
/// the matching upstream controls are intentionally omitted. The insurance
/// payment query is handled by the global Insurance & claims toggle and the
/// patient-payment path is independent of it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaySheetReportOptions {
    pub all_providers: bool,
    /// OD `provider.ProvNum`, used when `all_providers` is false.
    pub provider_num: Id,
    pub group_by: PaySheetGroupBy,
    /// Upstream "Show splits by provider separately" (checked by default).
    pub show_splits_by_provider: bool,
    pub all_payment_types: bool,
    /// OD `definition.DefNum` in `DefCat.PaymentTypes`, used when
    /// `all_payment_types` is false.
    pub payment_type_num: Id,
}

impl Default for PaySheetReportOptions {
    fn default() -> Self {
        Self {
            all_providers: true,
            provider_num: 0,
            group_by: PaySheetGroupBy::Check,
            show_splits_by_provider: true,
            all_payment_types: true,
            payment_type_num: 0,
        }
    }
}

/// Options from the original `FormRpAdjSheet`. Clinics are omitted (this port
/// has none); OD's multi-select type list is represented by a single picker,
/// matching the other report option panels in this port. All-types includes
/// hidden `DefCat.AdjTypes` definitions, as upstream does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdjustmentSheetReportOptions {
    pub all_providers: bool,
    /// OD `provider.ProvNum`, used when `all_providers` is false.
    pub provider_num: Id,
    pub all_types: bool,
    /// OD `definition.DefNum` in `DefCat.AdjTypes`, used when `all_types` is false.
    pub type_num: Id,
}

impl Default for AdjustmentSheetReportOptions {
    fn default() -> Self {
        Self {
            all_providers: true,
            provider_num: 0,
            all_types: true, // "All" is checked in FormRpAdjSheet.Load
            type_num: 0,
        }
    }
}

/// Options from the original `FormRpFinanceCharge`. Upstream reads the
/// finance-charge adjustment type and last-run date from practice preferences
/// and always filters to that type; this port stores the type as a picker and
/// uses `0` for "all adjustment types" so the report is useful before a
/// practice has configured a dedicated finance-charge type. The provider
/// filter is on the patient's primary provider (`patient.PriProv`), as in OD.
/// Billing types are not stored on this port's patient record, so that
/// upstream filter is omitted rather than shown as a fake control.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FinanceChargeReportOptions {
    pub all_providers: bool,
    /// OD `provider.ProvNum`; matches `patient.PriProv` when filtering.
    pub provider_num: Id,
    /// OD `definition.DefNum` in `DefCat.AdjTypes`; 0 means all types.
    pub adjustment_type_num: Id,
}

impl Default for FinanceChargeReportOptions {
    fn default() -> Self {
        Self {
            all_providers: true, // checkAllProv is checked in FormRpFinanceCharge.Load
            provider_num: 0,
            adjustment_type_num: 0,
        }
    }
}

/// The three radio-button modes of `FormRpBrokenAppointments`. Upstream picks
/// the default from practice preferences (`BrokenApptProcedure` /
/// `BrokenApptAdjustment`); this port defaults to appointment status, the only
/// mode that needs no configured codes or adjustment types.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrokenAppointmentMode {
    ByAppointmentStatus,
    ByAdjustments,
    ByProcedures,
}

impl Default for BrokenAppointmentMode {
    fn default() -> Self {
        Self::ByAppointmentStatus
    }
}

impl BrokenAppointmentMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ByAppointmentStatus => "appointment_status",
            Self::ByAdjustments => "adjustments",
            Self::ByProcedures => "procedures",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "adjustments" => Self::ByAdjustments,
            "procedures" => Self::ByProcedures,
            _ => Self::ByAppointmentStatus,
        }
    }
}

/// Which ADA broken-appointment code the by-procedures mode reports on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrokenProcedureOption {
    Missed,   // D9986
    Cancelled, // D9987
    Both,     // D9986 or D9987 (adds a Code column)
}

impl Default for BrokenProcedureOption {
    fn default() -> Self {
        Self::Missed
    }
}

impl BrokenProcedureOption {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missed => "missed",
            Self::Cancelled => "cancelled",
            Self::Both => "both",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "cancelled" => Self::Cancelled,
            "both" => Self::Both,
            _ => Self::Missed,
        }
    }
}

/// The two field-selection lists of `FormRpConfirm`, in the exact order shown in
/// the upstream form. Standard fields are all selected by default; the "other"
/// fields are all initially unselected.
pub const CONFIRMATION_STANDARD_FIELDS: &[&str] = &[
    "LName", "FName", "MiddleI", "Preferred", "Salutation", "Address", "Address2", "City",
    "State", "Zip", "HmPhone", "WkPhone", "WirelessPhone", "Birthdate", "AptDateTime",
];
pub const CONFIRMATION_OTHER_FIELDS: &[&str] = &[
    "BillingType",
    "CreditType",
    "SSN",
    "ChartNumber",
    "FeeSched",
    "Position",
    "Gender",
    "PatStatus",
    "PatNum",
    "Email",
    "EstBalance",
    "AddrNote",
    "FamFinUrgNote",
    "MedUrgNote",
    "ApptModNote",
    "PriCarrier",
    "PriRelationship",
    "SecCarrier",
    "SecRelationship",
    "StudentStatus",
    "SchoolName",
    "PriProv",
    "SecProv",
    "Guarantor",
    "ImageFolder",
];

/// The two field-selection lists of `FormRpRecall`, in the exact order shown in
/// the upstream form.
pub const RECALL_STANDARD_FIELDS: &[&str] = &[
    "LName",
    "FName",
    "MiddleI",
    "Preferred",
    "Salutation",
    "Address",
    "Address2",
    "City",
    "State",
    "Zip",
    "HmPhone",
    "WkPhone",
    "WirelessPhone",
    "Birthdate",
    "RecallStatus",
    "DateDue",
];
pub const RECALL_OTHER_FIELDS: &[&str] = &[
    "BillingType",
    "CreditType",
    "SSN",
    "ChartNumber",
    "FeeSched",
    "Position",
    "Gender",
    "PatStatus",
    "PatNum",
    "Email",
    "EstBalance",
    "AddrNote",
    "FamFinUrgNote",
    "MedUrgNote",
    "ApptModNote",
    "RecallInterval",
    "StudentStatus",
    "SchoolName",
    "PriProv",
    "SecProv",
    "Guarantor",
    "ImageFolder",
];

/// Selected fields, in output order, for the `FormRpConfirm` field-selection
/// report. Upstream takes its row scope from the confirmation list's selected
/// appointment numbers; this port scopes rows to the report date range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfirmationReportOptions {
    pub fields: Vec<String>,
}

impl Default for ConfirmationReportOptions {
    fn default() -> Self {
        Self {
            fields: CONFIRMATION_STANDARD_FIELDS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        }
    }
}

/// Selected fields, in output order, for the `FormRpRecall` field-selection
/// report (distinct from the Recall & ASAP due list). Upstream takes its row
/// scope from the recall list's selected `RecallNum`s; this port scopes rows to
/// recalls due in the report date range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecallFieldsReportOptions {
    pub fields: Vec<String>,
}

impl Default for RecallFieldsReportOptions {
    fn default() -> Self {
        Self {
            fields: RECALL_STANDARD_FIELDS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        }
    }
}

/// The two report layouts of `FormRpProcSheet`: one row per procedure, or
/// totals grouped by procedure code. Upstream defaults to Individual.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProcedureSheetMode {
    #[default]
    Individual,
    GroupedByCode,
}

impl ProcedureSheetMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Individual => "individual",
            Self::GroupedByCode => "grouped",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "grouped" | "grouped_by_code" => Self::GroupedByCode,
            _ => Self::Individual,
        }
    }
}

/// Options from the original `FormRpProcCodes` ("Procedure Codes - Fee
/// Schedules"). Upstream defaults to the first fee schedule in the list, the
/// Code (not Categories) layout, no blank fees and the Default provider.
///
/// The clinic selector is omitted because this port has no clinics: fees are
/// always read with `ClinicNum = 0`, which is upstream's "Default" clinic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcedureCodesReportOptions {
    pub fee_schedule_num: Id,
    pub categories: bool,
    pub include_blanks: bool,
    pub provider_num: Id,
}

impl Default for ProcedureCodesReportOptions {
    fn default() -> Self {
        Self {
            fee_schedule_num: 0,
            categories: false,
            include_blanks: false,
            provider_num: 0,
        }
    }
}

/// Options from the original `FormRpProcSheet` ("Daily Procedures"). Upstream
/// defaults to all providers and today's date; the date range itself is shared
/// with every other report. `code_filter` is the "Procedure Code" text box and
/// is matched case-insensitively as a substring of `procedurecode.ProcCode`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcedureSheetReportOptions {
    pub mode: ProcedureSheetMode,
    pub all_providers: bool,
    pub provider_num: Id,
    pub code_filter: String,
}

impl Default for ProcedureSheetReportOptions {
    fn default() -> Self {
        Self {
            mode: ProcedureSheetMode::default(),
            all_providers: true,
            provider_num: 0,
            code_filter: String::new(),
        }
    }
}

/// Options from the original `FormRpPrescriptions`. Upstream defaults to the
/// "Patient" radio button and an empty input box; with no text typed the report
/// returns every prescription. `search_patient = false` selects the "Drug"
/// radio, which filters on `rxpat.Drug` instead of `patient.LName`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrescriptionReportOptions {
    pub search_patient: bool,
    pub input_text: String,
}

impl Default for PrescriptionReportOptions {
    fn default() -> Self {
        Self {
            search_patient: true, // `radioPatient.Checked = true` in the designer
            input_text: String::new(),
        }
    }
}

/// Options from the original `FormRpReferralAnalysis`. Upstream opens on the
/// current month with "All" providers and both checkboxes unchecked. The
/// provider picker is a true multi-select; `all_providers` short-circuits it,
/// while an empty `provider_nums` with `all_providers = false` matches no rows,
/// just like upstream's "at least one provider must be selected" guard.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferralAnalysisReportOptions {
    /// `checkAddress` — adds Title/Address/Add2/City/State/Zip/Specialty.
    pub include_address: bool,
    /// `checkNewPat` — restricts production and the patient set to new patients.
    pub only_new_patients: bool,
    /// The first "All" entry of upstream's `listProv` is selected.
    pub all_providers: bool,
    /// Selected `provider.ProvNum` values (without the "All" sentinel).
    pub provider_nums: Vec<Id>,
}

impl Default for ReferralAnalysisReportOptions {
    fn default() -> Self {
        Self {
            include_address: false,
            only_new_patients: false,
            all_providers: true,
            provider_nums: Vec::new(),
        }
    }
}

/// The 19 selectable columns of `FormRpReferrals`, in the exact order upstream
/// adds them to both its SELECT list and its filter dropdown.
pub const REFERRAL_REPORT_FIELDS: &[&str] = &[
    "Address",
    "Address2",
    "City",
    "Email",
    "FName",
    "IsHidden",
    "LName",
    "MName",
    "Note",
    "NotPerson",
    "Phone2",
    "ReferralNum",
    "Specialty",
    "SSN",
    "ST",
    "Telephone",
    "Title",
    "UsingTIN",
    "Zip",
];

/// A compact default selection — the identifying and contact fields. Upstream
/// itself starts with nothing selected (the OK button stays disabled), so this
/// is a port-only convenience that can be cleared with "None".
pub const REFERRAL_REPORT_DEFAULT_FIELDS: &[&str] = &[
    "LName",
    "FName",
    "Title",
    "Telephone",
    "Address",
    "City",
    "ST",
    "Zip",
    "Specialty",
];

/// The seven conditions in upstream's `listConditions`, in the same order.
/// `FormRpReferrals` and `FormRpPatients` both build their WHERE clauses from
/// this exact dropdown, so the port shares one representation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReportFilterCondition {
    #[default]
    Like,
    Eq,
    Gt,
    Lt,
    Gte,
    Lte,
    Ne,
}

impl ReportFilterCondition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Like => "LIKE",
            Self::Eq => "=",
            Self::Gt => ">",
            Self::Lt => "<",
            Self::Gte => ">=",
            Self::Lte => "<=",
            Self::Ne => "<>",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value.trim() {
            "=" => Self::Eq,
            ">" => Self::Gt,
            "<" => Self::Lt,
            ">=" => Self::Gte,
            "<=" => Self::Lte,
            "<>" | "!=" => Self::Ne,
            _ => Self::Like,
        }
    }
}

/// One row of upstream's `listPrerequisites`: `Field <condition> 'value'`.
/// `value` is the raw text box input, a definition `DefNum`, or `"0"`/`"1"`
/// for a boolean dropdown.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportFilter {
    pub field: String,
    pub condition: ReportFilterCondition,
    pub value: String,
}

/// Upstream-specific spelling retained for the `FormRpReferrals` call sites.
pub type ReferralFilterCondition = ReportFilterCondition;
/// Upstream-specific spelling retained for the `FormRpReferrals` call sites.
pub type ReferralFilter = ReportFilter;

/// Options from the original `FormRpReferrals`. Upstream lets the user pick
/// any subset of the 19 `referral` columns and stack WHERE prerequisites
/// joined with AND. This port keeps the same field and condition vocabulary;
/// the row scope is the whole table because the original form has no date
/// filter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferralsReportOptions {
    /// Selected fields, in output order.
    pub fields: Vec<String>,
    /// Stacked WHERE prerequisites, applied in order.
    pub filters: Vec<ReferralFilter>,
}

impl Default for ReferralsReportOptions {
    fn default() -> Self {
        Self {
            fields: REFERRAL_REPORT_DEFAULT_FIELDS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            filters: Vec::new(),
        }
    }
}

/// The `FormRpPatients` SELECT columns this port can actually populate. Upstream
/// offers 67 patient columns; the rest are deliberately not offered here:
/// insurance and aging balance columns (`InsEst`, `Bal_0_30` … `BalTotal`,
/// `HasIns`, `PayPlanDue`) are excluded by the Ethiopia default, carrier and
/// relationship filters need `inplan` data this port does not store, and many
/// columns (`WkPhone`, `SSN`, `Address2`, `ChartNumber`, `MedicaidID`, …) have no
/// field in the port's `patient` model. The three derived columns at the end are
/// computed with the same subqueries as upstream.
pub const PATIENT_REPORT_FIELDS: &[&str] = &[
    "PatNum",
    "LName",
    "FName",
    "MiddleI",
    "Title",
    "Birthdate",
    "Gender",
    "HmPhone",
    "WirelessPhone",
    "Email",
    "Address",
    "City",
    "State",
    "Zip",
    "PriProv",
    "ClinicNum",
    "DateTStamp",
    "First Complete Proc",
    "Last Complete Proc",
    "RecallStatus",
];

/// A compact default selection. Upstream starts with nothing selected and keeps
/// OK disabled until the user picks at least one column; this is a port-only
/// convenience that can be cleared with "None".
pub const PATIENT_REPORT_DEFAULT_FIELDS: &[&str] = &[
    "PatNum",
    "LName",
    "FName",
    "Birthdate",
    "HmPhone",
    "Email",
];

/// Options from the original `FormRpPatients`. Upstream is a raw SQL query
/// builder: it concatenates a SELECT list, comma-joined lookup tables and a
/// stack of AND prerequisites. This port keeps the same field vocabulary, the
/// same seven conditions and the same AND chaining, but evaluates them over the
/// in-memory patient registry rather than generating SQL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatientsReportOptions {
    /// Selected columns, in output order.
    pub fields: Vec<String>,
    /// Stacked WHERE prerequisites, applied in order.
    pub filters: Vec<ReportFilter>,
}

impl Default for PatientsReportOptions {
    fn default() -> Self {
        Self {
            fields: PATIENT_REPORT_DEFAULT_FIELDS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            filters: Vec::new(),
        }
    }
}

/// Options from the original `FormRpBrokenAppointments`. Upstream's date range
/// and provider list are shared by all three modes. The provider filter maps to
/// `procedurelog.ProvNum`, `adjustment.ProvNum` or `appointment.ProvNum` /
/// `ProvHyg` depending on mode; this port has no `ProvHyg`, so the status mode
/// filters on the appointment's primary provider only. Clinics are omitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrokenAppointmentReportOptions {
    pub mode: BrokenAppointmentMode,
    pub all_providers: bool,
    pub provider_num: Id,
    pub procedure_option: BrokenProcedureOption,
    /// ByAdjustments: `definition.DefNum` in `DefCat.AdjTypes`; `0` means all
    /// types. Upstream defaults to the "positive" (`ItemValue == "+"`) types.
    pub adjustment_type_num: Id,
}

impl Default for BrokenAppointmentReportOptions {
    fn default() -> Self {
        Self {
            mode: BrokenAppointmentMode::default(),
            all_providers: true,
            provider_num: 0,
            procedure_option: BrokenProcedureOption::default(),
            adjustment_type_num: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReportPreferences {
    /// Printed on exported CSV/HTML so a practice can identify its reports.
    pub practice_name: String,
    /// Last export path per report key (e.g. `recall-asap`). Values are full
    /// paths so a practice can send different reports to different folders.
    export_paths: HashMap<String, String>,
    /// `FormRpAging` options, persisted between runs.
    pub aging: AgingOptions,
    /// `FormRpCustomAging` options.
    pub custom_aging: CustomAgingReportOptions,
    /// `FormRpPayPlans` options.
    pub pay_plans: PayPlanReportOptions,
    /// Payment plan charge applications / refund-transfer history report options.
    pub pay_plan_charge_applications: PayPlanChargeApplicationsReportOptions,
    /// `FormRpNewPatients` options.
    pub new_patients: NewPatientReportOptions,
    /// `FormRpAppointments` options.
    pub appointments: AppointmentReportOptions,
    /// `FormRpActivePatients` options.
    pub active_patients: ActivePatientReportOptions,
    /// `FormRpProdInc` options.
    pub prod_inc: ProdIncReportOptions,
    /// `FormRpPaySheet` options.
    pub pay_sheet: PaySheetReportOptions,
    /// `FormRpAdjSheet` options.
    pub adj_sheet: AdjustmentSheetReportOptions,
    /// `FormRpFinanceCharge` options.
    pub finance_charge: FinanceChargeReportOptions,
    /// `FormRpBrokenAppointments` options.
    pub broken_appointments: BrokenAppointmentReportOptions,
    /// `FormRpConfirm` field selection.
    pub confirmation: ConfirmationReportOptions,
    /// `FormRpRecall` field selection.
    pub recall_fields: RecallFieldsReportOptions,
    /// `FormRpProcSheet` options.
    pub procedure_sheet: ProcedureSheetReportOptions,
    /// `FormRpProcCodes` options.
    pub procedure_codes: ProcedureCodesReportOptions,
    /// `FormRpPrescriptions` options.
    pub prescriptions: PrescriptionReportOptions,
    /// `FormRpReferralAnalysis` options.
    pub referral_analysis: ReferralAnalysisReportOptions,
    /// `FormRpReferrals` field selection and WHERE filters.
    pub referrals: ReferralsReportOptions,
    /// `FormRpPatients` field selection and WHERE filters.
    pub patients: PatientsReportOptions,
    /// `FormRpAccountingBalanceSheet`, `FormRpAccountingGenLedg` and
    /// `FormRpAccountingProfitLoss` options.
    pub accounting: AccountingReportOptions,
    /// `FormRpProcNote` options.
    pub proc_note: ProcNoteReportOptions,
    /// `FormRpHiddenPaySplits` options.
    pub hidden_pay_splits: HiddenPaySplitsReportOptions,
    /// `FormRpUnearnedIncome` options.
    pub unearned_income: UnearnedIncomeReportOptions,
    /// `FormRpProcOverpaid` options.
    pub proc_overpaid: ProcOverpaidReportOptions,
    /// `FormRpLaserLabels` tab, insurance mode and carrier range keys.
    pub laser_labels: LaserLabelOptions,
    /// `FormRpTreatmentFinder` filters.
    pub treatment_finder: TreatmentFinderReportOptions,
}

impl Default for ReportPreferences {
    fn default() -> Self {
        Self {
            practice_name: String::new(),
            export_paths: HashMap::new(),
            aging: AgingOptions::default(),
            custom_aging: CustomAgingReportOptions::default(),
            pay_plans: PayPlanReportOptions::default(),
            pay_plan_charge_applications: PayPlanChargeApplicationsReportOptions::default(),
            new_patients: NewPatientReportOptions::default(),
            appointments: AppointmentReportOptions::default(),
            active_patients: ActivePatientReportOptions::default(),
            prod_inc: ProdIncReportOptions::default(),
            pay_sheet: PaySheetReportOptions::default(),
            adj_sheet: AdjustmentSheetReportOptions::default(),
            finance_charge: FinanceChargeReportOptions::default(),
            broken_appointments: BrokenAppointmentReportOptions::default(),
            confirmation: ConfirmationReportOptions::default(),
            recall_fields: RecallFieldsReportOptions::default(),
            procedure_sheet: ProcedureSheetReportOptions::default(),
            procedure_codes: ProcedureCodesReportOptions::default(),
            prescriptions: PrescriptionReportOptions::default(),
            referral_analysis: ReferralAnalysisReportOptions::default(),
            referrals: ReferralsReportOptions::default(),
            patients: PatientsReportOptions::default(),
            accounting: AccountingReportOptions::default(),
            proc_note: ProcNoteReportOptions::default(),
            hidden_pay_splits: HiddenPaySplitsReportOptions::default(),
            unearned_income: UnearnedIncomeReportOptions::default(),
            proc_overpaid: ProcOverpaidReportOptions::default(),
            laser_labels: LaserLabelOptions::default(),
            treatment_finder: TreatmentFinderReportOptions::default(),
        }
    }
}

/// Persist one stacked prerequisite as `field|condition|value`. Percent escapes
/// keep `|` and `;` in user text from breaking the encoding. Shared by the
/// `FormRpReferrals` and `FormRpPatients` query builders.
fn encode_report_filter(filter: &ReportFilter) -> String {
    let value = filter
        .value
        .replace('%', "%25")
        .replace('|', "%7C")
        .replace(';', "%3B");
    format!("{}|{}|{}", filter.field, filter.condition.as_str(), value)
}

fn decode_report_filter(encoded: &str) -> Option<ReportFilter> {
    let mut parts = encoded.splitn(3, '|');
    let field = parts.next()?.to_string();
    let condition = ReportFilterCondition::from_str(parts.next()?);
    let value = parts
        .next()?
        .replace("%3B", ";")
        .replace("%7C", "|")
        .replace("%25", "%");
    Some(ReportFilter {
        field,
        condition,
        value,
    })
}

impl ReportPreferences {
    /// The key prefix used for per-report export paths in the text file.
    const EXPORT_PREFIX: &'static str = "export.";
    /// The key prefix for the upstream report option checkboxes.
    const OPTION_PREFIX: &'static str = "option.";

    pub fn export_path(&self, report_key: &str) -> Option<&str> {
        self.export_paths.get(report_key).map(String::as_str)
    }

    pub fn set_export_path(&mut self, report_key: impl Into<String>, path: impl Into<String>) {
        self.export_paths.insert(report_key.into(), path.into());
    }

    fn bool_text(value: bool) -> &'static str {
        if value { "true" } else { "false" }
    }

    fn parse_bool(value: &str) -> bool {
        matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes")
    }

    /// Parse a comma-separated field list, keeping only recognised names in the
    /// order written. An empty or fully unknown list falls back to `fallback`.
    fn parse_fields(value: &str, allowed: &[&str], fallback: &[&str]) -> Vec<String> {
        let fields: Vec<String> = value
            .split(',')
            .map(str::trim)
            .filter(|name| allowed.contains(name))
            .map(str::to_string)
            .collect();
        if fields.is_empty() {
            fallback.iter().map(|s| (*s).to_string()).collect()
        } else {
            fields
        }
    }

    pub fn to_text(&self) -> String {
        let mut lines = vec![format!("practice_name={}", self.practice_name)];
        lines.push(format!(
            "{}aging.age={}",
            Self::OPTION_PREFIX,
            self.aging.age.as_str()
        ));
        lines.push(format!(
            "{}custom_aging.age={}",
            Self::OPTION_PREFIX,
            self.custom_aging.age.as_str()
        ));
        lines.push(format!(
            "{}custom_aging.negative={}",
            Self::OPTION_PREFIX,
            self.custom_aging.negative.as_str()
        ));
        for (name, value) in [
            ("aging.include_negative", self.aging.include_negative),
            ("aging.only_negative", self.aging.only_negative),
            ("aging.show_last_pay", self.aging.show_last_pay),
            ("aging.age_writeoffs", self.aging.age_writeoffs),
            (
                "custom_aging.include.procedure_fees",
                self.custom_aging.include.procedure_fees,
            ),
            (
                "custom_aging.include.adjustments",
                self.custom_aging.include.adjustments,
            ),
            (
                "custom_aging.include.pay_plan_charges",
                self.custom_aging.include.pay_plan_charges,
            ),
            (
                "custom_aging.include.pay_plan_production",
                self.custom_aging.include.pay_plan_production,
            ),
            (
                "custom_aging.include.patient_payments",
                self.custom_aging.include.patient_payments,
            ),
            ("custom_aging.age_credits", self.custom_aging.age_credits),
            ("custom_aging.all_providers", self.custom_aging.all_providers),
            ("custom_aging.all_clinics", self.custom_aging.all_clinics),
            (
                "custom_aging.exclude_bad_address",
                self.custom_aging.exclude_bad_address,
            ),
            ("pay_plans.show_family_balance", self.pay_plans.show_family_balance),
            ("pay_plans.limit_date_range", self.pay_plans.limit_date_range),
            ("pay_plans.hide_completed", self.pay_plans.hide_completed),
            ("pay_plans.only_stale_resolved", self.pay_plans.only_stale_resolved),
            (
                "pay_plan_charge_applications.only_refunds_transfers",
                self.pay_plan_charge_applications.only_refunds_transfers,
            ),
            ("new_patients.include_address", self.new_patients.include_address),
            (
                "new_patients.exclude_no_production",
                self.new_patients.exclude_no_production,
            ),
            (
                "appointments.show_note_appointments",
                self.appointments.show_note_appointments,
            ),
            ("active_patients.all_providers", self.active_patients.all_providers),
            ("pay_sheet.all_providers", self.pay_sheet.all_providers),
            (
                "pay_sheet.show_splits_by_provider",
                self.pay_sheet.show_splits_by_provider,
            ),
            ("pay_sheet.all_payment_types", self.pay_sheet.all_payment_types),
            ("adj_sheet.all_providers", self.adj_sheet.all_providers),
            ("adj_sheet.all_types", self.adj_sheet.all_types),
            ("finance_charge.all_providers", self.finance_charge.all_providers),
            (
                "broken_appointments.all_providers",
                self.broken_appointments.all_providers,
            ),
            (
                "procedure_sheet.all_providers",
                self.procedure_sheet.all_providers,
            ),
            (
                "procedure_codes.categories",
                self.procedure_codes.categories,
            ),
            (
                "procedure_codes.include_blanks",
                self.procedure_codes.include_blanks,
            ),
            (
                "prescriptions.search_patient",
                self.prescriptions.search_patient,
            ),
            (
                "referral_analysis.include_address",
                self.referral_analysis.include_address,
            ),
            (
                "referral_analysis.only_new_patients",
                self.referral_analysis.only_new_patients,
            ),
            (
                "referral_analysis.all_providers",
                self.referral_analysis.all_providers,
            ),
        ] {
            lines.push(format!(
                "{}{}={}",
                Self::OPTION_PREFIX,
                name,
                Self::bool_text(value)
            ));
        }
        for (name, value) in [
            (
                "pay_plans.plan_type",
                self.pay_plans.plan_type.as_str(),
            ),
            (
                "appointments.date_basis",
                self.appointments.date_basis.as_str(),
            ),
            ("prod_inc.mode", self.prod_inc.mode.as_str()),
            (
                "custom_aging.family_grouping",
                self.custom_aging.family_grouping.as_str(),
            ),
            (
                "custom_aging.writeoff_date",
                self.custom_aging.writeoff_date.as_str(),
            ),
            ("pay_sheet.group_by", self.pay_sheet.group_by.as_str()),
            (
                "broken_appointments.mode",
                self.broken_appointments.mode.as_str(),
            ),
            (
                "broken_appointments.procedure_option",
                self.broken_appointments.procedure_option.as_str(),
            ),
            (
                "procedure_sheet.mode",
                self.procedure_sheet.mode.as_str(),
            ),
        ] {
            lines.push(format!("{}{}={}", Self::OPTION_PREFIX, name, value));
        }
        lines.push(format!(
            "{}confirmation.fields={}",
            Self::OPTION_PREFIX,
            self.confirmation.fields.join(",")
        ));
        lines.push(format!(
            "{}recall_fields.fields={}",
            Self::OPTION_PREFIX,
            self.recall_fields.fields.join(",")
        ));
        lines.push(format!(
            "{}procedure_sheet.code_filter={}",
            Self::OPTION_PREFIX,
            self.procedure_sheet.code_filter
        ));
        lines.push(format!(
            "{}prescriptions.input_text={}",
            Self::OPTION_PREFIX,
            self.prescriptions.input_text
        ));
        lines.push(format!(
            "{}referral_analysis.provider_nums={}",
            Self::OPTION_PREFIX,
            self.referral_analysis
                .provider_nums
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ));
        lines.push(format!(
            "{}custom_aging.provider_nums={}",
            Self::OPTION_PREFIX,
            self.custom_aging
                .provider_nums
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ));
        lines.push(format!(
            "{}custom_aging.clinic_nums={}",
            Self::OPTION_PREFIX,
            self.custom_aging
                .clinic_nums
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ));
        lines.push(format!(
            "{}custom_aging.bill_type_nums={}",
            Self::OPTION_PREFIX,
            self.custom_aging
                .bill_type_nums
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ));
        lines.push(format!(
            "{}referrals.fields={}",
            Self::OPTION_PREFIX,
            self.referrals.fields.join(",")
        ));
        lines.push(format!(
            "{}referrals.filters={}",
            Self::OPTION_PREFIX,
            self.referrals
                .filters
                .iter()
                .map(encode_report_filter)
                .collect::<Vec<_>>()
                .join(";")
        ));
        lines.push(format!(
            "{}patients.fields={}",
            Self::OPTION_PREFIX,
            self.patients.fields.join(",")
        ));
        lines.push(format!(
            "{}patients.filters={}",
            Self::OPTION_PREFIX,
            self.patients
                .filters
                .iter()
                .map(encode_report_filter)
                .collect::<Vec<_>>()
                .join(";")
        ));
        for (name, value) in [
            (
                "accounting.include_net_income",
                self.accounting.include_net_income,
            ),
            (
                "accounting.show_opening_balances",
                self.accounting.show_opening_balances,
            ),
            ("proc_note.include_no_notes", self.proc_note.include_no_notes),
            ("proc_note.include_unsigned", self.proc_note.include_unsigned),
            (
                "proc_note.show_excluded_codes",
                self.proc_note.show_excluded_codes,
            ),
            (
                "hidden_pay_splits.exclude_refunds",
                self.hidden_pay_splits.exclude_refunds,
            ),
            (
                "unearned_income.exclude_zero_allocation",
                self.unearned_income.exclude_zero_allocation,
            ),
            (
                "unearned_income.exclude_zero_net",
                self.unearned_income.exclude_zero_net,
            ),
            ("unearned_income.show_provider", self.unearned_income.show_provider),
            (
                "proc_overpaid.exclude_without_insurance",
                self.proc_overpaid.exclude_without_insurance,
            ),
            ("prod_inc.include_unearned", self.prod_inc.include_unearned),
            (
                "custom_aging.exclude_inactive",
                self.custom_aging.exclude_inactive,
            ),
            (
                "custom_aging.exclude_archive",
                self.custom_aging.exclude_archive,
            ),
            (
                "custom_aging.include.insurance_payments",
                self.custom_aging.include.insurance_payments,
            ),
            (
                "custom_aging.include.insurance_estimates",
                self.custom_aging.include.insurance_estimates,
            ),
            (
                "custom_aging.include.write_offs",
                self.custom_aging.include.write_offs,
            ),
            (
                "custom_aging.include.write_off_estimates",
                self.custom_aging.include.write_off_estimates,
            ),
            (
                "treatment_finder.pats_with_appts",
                self.treatment_finder.pats_with_appts,
            ),
            (
                "treatment_finder.use_treating_provider",
                self.treatment_finder.use_treating_provider,
            ),
        ] {
            lines.push(format!(
                "{}{}={}",
                Self::OPTION_PREFIX,
                name,
                Self::bool_text(value)
            ));
        }
        lines.push(format!(
            "{}proc_note.group_by={}",
            Self::OPTION_PREFIX,
            self.proc_note.group_by.as_str()
        ));
        lines.push(format!(
            "{}unearned_income.tab={}",
            Self::OPTION_PREFIX,
            self.unearned_income.tab.as_str()
        ));
        lines.push(format!(
            "{}proc_overpaid.pat_num={}",
            Self::OPTION_PREFIX,
            self.proc_overpaid.pat_num
        ));
        lines.push(format!(
            "{}laser.tab={}",
            Self::OPTION_PREFIX,
            self.laser_labels.tab.as_str()
        ));
        lines.push(format!(
            "{}laser.mode={}",
            Self::OPTION_PREFIX,
            self.laser_labels.ins_mode.as_str()
        ));
        lines.push(format!(
            "{}laser.copies={}",
            Self::OPTION_PREFIX,
            self.laser_labels.ins_copies
        ));
        lines.push(format!(
            "{}laser.start_name={}",
            Self::OPTION_PREFIX,
            self.laser_labels.start_name
        ));
        lines.push(format!(
            "{}laser.start_addr={}",
            Self::OPTION_PREFIX,
            self.laser_labels.start_addr
        ));
        lines.push(format!(
            "{}laser.end_name={}",
            Self::OPTION_PREFIX,
            self.laser_labels.end_name
        ));
        lines.push(format!(
            "{}laser.end_addr={}",
            Self::OPTION_PREFIX,
            self.laser_labels.end_addr
        ));
        lines.push(format!(
            "{}treatment_finder.code1={}",
            Self::OPTION_PREFIX,
            self.treatment_finder.code1
        ));
        lines.push(format!(
            "{}treatment_finder.code2={}",
            Self::OPTION_PREFIX,
            self.treatment_finder.code2
        ));
        lines.push(format!(
            "{}treatment_finder.above_amount={}",
            Self::OPTION_PREFIX,
            self.treatment_finder.above_amount
        ));
        for (name, value) in [
            ("active_patients.provider_num", self.active_patients.provider_num),
            ("pay_sheet.provider_num", self.pay_sheet.provider_num),
            ("pay_sheet.payment_type_num", self.pay_sheet.payment_type_num),
            ("adj_sheet.provider_num", self.adj_sheet.provider_num),
            ("adj_sheet.type_num", self.adj_sheet.type_num),
            ("finance_charge.provider_num", self.finance_charge.provider_num),
            (
                "finance_charge.adjustment_type_num",
                self.finance_charge.adjustment_type_num,
            ),
            (
                "broken_appointments.provider_num",
                self.broken_appointments.provider_num,
            ),
            (
                "broken_appointments.adjustment_type_num",
                self.broken_appointments.adjustment_type_num,
            ),
            (
                "procedure_sheet.provider_num",
                self.procedure_sheet.provider_num,
            ),
            (
                "procedure_codes.fee_schedule_num",
                self.procedure_codes.fee_schedule_num,
            ),
            (
                "procedure_codes.provider_num",
                self.procedure_codes.provider_num,
            ),
            (
                "treatment_finder.provider_num",
                self.treatment_finder.provider_num,
            ),
            (
                "treatment_finder.billing_type_num",
                self.treatment_finder.billing_type_num,
            ),
            (
                "treatment_finder.clinic_num",
                self.treatment_finder.clinic_num,
            ),
        ] {
            lines.push(format!(
                "{}{}={}",
                Self::OPTION_PREFIX,
                name,
                value
            ));
        }
        let mut paths: Vec<_> = self.export_paths.iter().collect();
        paths.sort_by(|a, b| a.0.cmp(b.0));
        for (key, path) in paths {
            lines.push(format!("{}{}={}", Self::EXPORT_PREFIX, key, path));
        }
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }

    pub fn from_text(text: &str) -> Self {
        let mut prefs = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            if key == "practice_name" {
                prefs.practice_name = value.to_string();
            } else if let Some(report_key) = key.strip_prefix(Self::EXPORT_PREFIX) {
                if !report_key.is_empty() {
                    prefs
                        .export_paths
                        .insert(report_key.to_string(), value.to_string());
                }
            } else if let Some(option) = key.strip_prefix(Self::OPTION_PREFIX) {
                let flag = Self::parse_bool(value);
                match option {
                    "aging.age" => prefs.aging.age = AgingAgeFilter::from_str(value),
                    "aging.include_negative" => prefs.aging.include_negative = flag,
                    "aging.only_negative" => prefs.aging.only_negative = flag,
                    "aging.show_last_pay" => prefs.aging.show_last_pay = flag,
                    "aging.age_writeoffs" => prefs.aging.age_writeoffs = flag,
                    "custom_aging.age" => {
                        prefs.custom_aging.age = AgingAgeFilter::from_str(value)
                    }
                    "custom_aging.negative" => {
                        prefs.custom_aging.negative = CustomAgingNegative::from_str(value)
                    }
                    "custom_aging.include.procedure_fees" => {
                        prefs.custom_aging.include.procedure_fees = flag
                    }
                    "custom_aging.include.adjustments" => {
                        prefs.custom_aging.include.adjustments = flag
                    }
                    "custom_aging.include.pay_plan_charges" => {
                        prefs.custom_aging.include.pay_plan_charges = flag
                    }
                    "custom_aging.include.pay_plan_production" => {
                        prefs.custom_aging.include.pay_plan_production = flag
                    }
                    "custom_aging.include.patient_payments" => {
                        prefs.custom_aging.include.patient_payments = flag
                    }
                    "custom_aging.include.insurance_payments" => {
                        prefs.custom_aging.include.insurance_payments = flag
                    }
                    "custom_aging.include.insurance_estimates" => {
                        prefs.custom_aging.include.insurance_estimates = flag
                    }
                    "custom_aging.include.write_offs" => {
                        prefs.custom_aging.include.write_offs = flag
                    }
                    "custom_aging.include.write_off_estimates" => {
                        prefs.custom_aging.include.write_off_estimates = flag
                    }
                    "custom_aging.family_grouping" => {
                        prefs.custom_aging.family_grouping =
                            AgingFamilyGrouping::from_str(value)
                    }
                    "custom_aging.writeoff_date" => {
                        prefs.custom_aging.writeoff_date =
                            CustomAgingWriteoffDate::from_str(value)
                    }
                    "custom_aging.exclude_inactive" => {
                        prefs.custom_aging.exclude_inactive = flag
                    }
                    "custom_aging.exclude_archive" => {
                        prefs.custom_aging.exclude_archive = flag
                    }
                    "custom_aging.age_credits" => prefs.custom_aging.age_credits = flag,
                    "custom_aging.all_providers" => prefs.custom_aging.all_providers = flag,
                    "custom_aging.all_clinics" => prefs.custom_aging.all_clinics = flag,
                    "custom_aging.exclude_bad_address" => {
                        prefs.custom_aging.exclude_bad_address = flag
                    }
                    "custom_aging.provider_nums" => {
                        prefs.custom_aging.provider_nums = value
                            .split(',')
                            .filter_map(|part| part.trim().parse::<Id>().ok())
                            .collect()
                    }
                    "custom_aging.clinic_nums" => {
                        prefs.custom_aging.clinic_nums = value
                            .split(',')
                            .filter_map(|part| part.trim().parse::<Id>().ok())
                            .collect()
                    }
                    "custom_aging.bill_type_nums" => {
                        prefs.custom_aging.bill_type_nums = value
                            .split(',')
                            .filter_map(|part| part.trim().parse::<Id>().ok())
                            .collect()
                    }
                    "pay_plans.plan_type" => {
                        prefs.pay_plans.plan_type = PlanTypeFilter::from_str(value)
                    }
                    "pay_plans.show_family_balance" => prefs.pay_plans.show_family_balance = flag,
                    "pay_plans.limit_date_range" => prefs.pay_plans.limit_date_range = flag,
                    "pay_plans.hide_completed" => prefs.pay_plans.hide_completed = flag,
                    "pay_plans.only_stale_resolved" => {
                        prefs.pay_plans.only_stale_resolved = flag
                    }
                    "pay_plan_charge_applications.only_refunds_transfers" => {
                        prefs.pay_plan_charge_applications.only_refunds_transfers = flag
                    }
                    "new_patients.include_address" => prefs.new_patients.include_address = flag,
                    "new_patients.exclude_no_production" => {
                        prefs.new_patients.exclude_no_production = flag
                    }
                    "appointments.date_basis" => {
                        prefs.appointments.date_basis = AppointmentDateBasis::from_str(value)
                    }
                    "appointments.show_note_appointments" => {
                        prefs.appointments.show_note_appointments = flag
                    }
                    "active_patients.all_providers" => prefs.active_patients.all_providers = flag,
                    "active_patients.provider_num" => {
                        prefs.active_patients.provider_num = value.parse().unwrap_or(0)
                    }
                    "prod_inc.mode" => prefs.prod_inc.mode = ProdIncMode::from_str(value),
                    "prod_inc.include_unearned" => prefs.prod_inc.include_unearned = flag,
                    "pay_sheet.all_providers" => prefs.pay_sheet.all_providers = flag,
                    "pay_sheet.provider_num" => {
                        prefs.pay_sheet.provider_num = value.parse().unwrap_or(0)
                    }
                    "pay_sheet.group_by" => {
                        prefs.pay_sheet.group_by = PaySheetGroupBy::from_str(value)
                    }
                    "pay_sheet.show_splits_by_provider" => {
                        prefs.pay_sheet.show_splits_by_provider = flag
                    }
                    "pay_sheet.all_payment_types" => prefs.pay_sheet.all_payment_types = flag,
                    "pay_sheet.payment_type_num" => {
                        prefs.pay_sheet.payment_type_num = value.parse().unwrap_or(0)
                    }
                    "adj_sheet.all_providers" => prefs.adj_sheet.all_providers = flag,
                    "adj_sheet.provider_num" => {
                        prefs.adj_sheet.provider_num = value.parse().unwrap_or(0)
                    }
                    "adj_sheet.all_types" => prefs.adj_sheet.all_types = flag,
                    "adj_sheet.type_num" => prefs.adj_sheet.type_num = value.parse().unwrap_or(0),
                    "finance_charge.all_providers" => {
                        prefs.finance_charge.all_providers = flag
                    }
                    "finance_charge.provider_num" => {
                        prefs.finance_charge.provider_num = value.parse().unwrap_or(0)
                    }
                    "finance_charge.adjustment_type_num" => {
                        prefs.finance_charge.adjustment_type_num = value.parse().unwrap_or(0)
                    }
                    "broken_appointments.all_providers" => {
                        prefs.broken_appointments.all_providers = flag
                    }
                    "broken_appointments.provider_num" => {
                        prefs.broken_appointments.provider_num = value.parse().unwrap_or(0)
                    }
                    "broken_appointments.adjustment_type_num" => {
                        prefs.broken_appointments.adjustment_type_num = value.parse().unwrap_or(0)
                    }
                    "broken_appointments.mode" => {
                        prefs.broken_appointments.mode = BrokenAppointmentMode::from_str(value)
                    }
                    "broken_appointments.procedure_option" => {
                        prefs.broken_appointments.procedure_option =
                            BrokenProcedureOption::from_str(value)
                    }
                    "confirmation.fields" => {
                        let mut allowed: Vec<&str> = CONFIRMATION_STANDARD_FIELDS.to_vec();
                        allowed.extend_from_slice(CONFIRMATION_OTHER_FIELDS);
                        prefs.confirmation.fields = Self::parse_fields(
                            value,
                            &allowed,
                            CONFIRMATION_STANDARD_FIELDS,
                        );
                    }
                    "recall_fields.fields" => {
                        let mut allowed: Vec<&str> = RECALL_STANDARD_FIELDS.to_vec();
                        allowed.extend_from_slice(RECALL_OTHER_FIELDS);
                        prefs.recall_fields.fields =
                            Self::parse_fields(value, &allowed, RECALL_STANDARD_FIELDS);
                    }
                    "procedure_sheet.all_providers" => {
                        prefs.procedure_sheet.all_providers = flag
                    }
                    "procedure_sheet.provider_num" => {
                        prefs.procedure_sheet.provider_num = value.parse().unwrap_or(0)
                    }
                    "procedure_sheet.code_filter" => {
                        prefs.procedure_sheet.code_filter = value.trim().to_string()
                    }
                    "procedure_sheet.mode" => {
                        prefs.procedure_sheet.mode = ProcedureSheetMode::from_str(value)
                    }
                    "procedure_codes.categories" => {
                        prefs.procedure_codes.categories = flag
                    }
                    "procedure_codes.include_blanks" => {
                        prefs.procedure_codes.include_blanks = flag
                    }
                    "procedure_codes.fee_schedule_num" => {
                        prefs.procedure_codes.fee_schedule_num = value.parse().unwrap_or(0)
                    }
                    "procedure_codes.provider_num" => {
                        prefs.procedure_codes.provider_num = value.parse().unwrap_or(0)
                    }
                    "prescriptions.search_patient" => {
                        prefs.prescriptions.search_patient = flag
                    }
                    "prescriptions.input_text" => {
                        prefs.prescriptions.input_text = value.trim().to_string()
                    }
                    "referral_analysis.include_address" => {
                        prefs.referral_analysis.include_address = flag
                    }
                    "referral_analysis.only_new_patients" => {
                        prefs.referral_analysis.only_new_patients = flag
                    }
                    "referral_analysis.all_providers" => {
                        prefs.referral_analysis.all_providers = flag
                    }
                    "referral_analysis.provider_nums" => {
                        prefs.referral_analysis.provider_nums = value
                            .split(',')
                            .filter_map(|part| part.trim().parse::<Id>().ok())
                            .collect()
                    }
                    "referrals.fields" => {
                        prefs.referrals.fields = Self::parse_fields(
                            value,
                            REFERRAL_REPORT_FIELDS,
                            REFERRAL_REPORT_DEFAULT_FIELDS,
                        );
                    }
                    "referrals.filters" => {
                        prefs.referrals.filters = value
                            .split(';')
                            .filter(|part| !part.trim().is_empty())
                            .filter_map(decode_report_filter)
                            .filter(|filter| {
                                REFERRAL_REPORT_FIELDS.contains(&filter.field.as_str())
                            })
                            .collect()
                    }
                    "patients.fields" => {
                        prefs.patients.fields = Self::parse_fields(
                            value,
                            PATIENT_REPORT_FIELDS,
                            PATIENT_REPORT_DEFAULT_FIELDS,
                        );
                    }
                    "patients.filters" => {
                        prefs.patients.filters = value
                            .split(';')
                            .filter(|part| !part.trim().is_empty())
                            .filter_map(decode_report_filter)
                            .filter(|filter| {
                                PATIENT_REPORT_FIELDS.contains(&filter.field.as_str())
                            })
                            .collect()
                    }
                    "accounting.include_net_income" => {
                        prefs.accounting.include_net_income = flag
                    }
                    "accounting.show_opening_balances" => {
                        prefs.accounting.show_opening_balances = flag
                    }
                    "proc_note.include_no_notes" => prefs.proc_note.include_no_notes = flag,
                    "proc_note.include_unsigned" => prefs.proc_note.include_unsigned = flag,
                    "proc_note.show_excluded_codes" => {
                        prefs.proc_note.show_excluded_codes = flag
                    }
                    "proc_note.group_by" => {
                        prefs.proc_note.group_by = ProcNoteGroup::from_str(value)
                    }
                    "hidden_pay_splits.exclude_refunds" => {
                        prefs.hidden_pay_splits.exclude_refunds = flag
                    }
                    "unearned_income.tab" => {
                        prefs.unearned_income.tab = UnearnedIncomeTab::from_str(value)
                    }
                    "unearned_income.exclude_zero_allocation" => {
                        prefs.unearned_income.exclude_zero_allocation = flag
                    }
                    "unearned_income.exclude_zero_net" => {
                        prefs.unearned_income.exclude_zero_net = flag
                    }
                    "unearned_income.show_provider" => {
                        prefs.unearned_income.show_provider = flag
                    }
                    "laser.tab" => {
                        prefs.laser_labels.tab = LaserLabelTab::from_str(value)
                    }
                    "laser.mode" => {
                        prefs.laser_labels.ins_mode = LaserInsMode::from_str(value)
                    }
                    "laser.copies" => {
                        prefs.laser_labels.ins_copies = value.parse().unwrap_or(1)
                    }
                    "laser.start_name" => prefs.laser_labels.start_name = value.to_string(),
                    "laser.start_addr" => prefs.laser_labels.start_addr = value.to_string(),
                    "laser.end_name" => prefs.laser_labels.end_name = value.to_string(),
                    "laser.end_addr" => prefs.laser_labels.end_addr = value.to_string(),
                    "proc_overpaid.exclude_without_insurance" => {
                        prefs.proc_overpaid.exclude_without_insurance = flag
                    }
                    "proc_overpaid.pat_num" => {
                        prefs.proc_overpaid.pat_num = value.parse().unwrap_or(0)
                    }
                    "treatment_finder.pats_with_appts" => {
                        prefs.treatment_finder.pats_with_appts = flag
                    }
                    "treatment_finder.use_treating_provider" => {
                        prefs.treatment_finder.use_treating_provider = flag
                    }
                    "treatment_finder.provider_num" => {
                        prefs.treatment_finder.provider_num = value.parse().unwrap_or(0)
                    }
                    "treatment_finder.billing_type_num" => {
                        prefs.treatment_finder.billing_type_num = value.parse().unwrap_or(0)
                    }
                    "treatment_finder.clinic_num" => {
                        prefs.treatment_finder.clinic_num = value.parse().unwrap_or(0)
                    }
                    "treatment_finder.above_amount" => {
                        prefs.treatment_finder.above_amount = value.parse().unwrap_or(0.0)
                    }
                    "treatment_finder.code1" => {
                        prefs.treatment_finder.code1 = value.trim().to_string()
                    }
                    "treatment_finder.code2" => {
                        prefs.treatment_finder.code2 = value.trim().to_string()
                    }
                    _ => {}
                }
            }
        }
        prefs
    }

    pub fn load() -> Self {
        match Self::config_path() {
            Some(path) => std::fs::read_to_string(path)
                .map(|text| Self::from_text(&text))
                .unwrap_or_default(),
            None => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path().ok_or_else(|| "Could not find a settings folder".to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Could not create settings folder: {e}"))?;
        }
        std::fs::write(path, self.to_text())
            .map_err(|e| format!("Could not save report preferences: {e}"))
    }

    fn config_path() -> Option<PathBuf> {
        if let Ok(appdata) = std::env::var("APPDATA") {
            if !appdata.trim().is_empty() {
                return Some(PathBuf::from(appdata).join("odclinic").join("reports.txt"));
            }
        }
        std::env::current_dir()
            .ok()
            .map(|dir| dir.join("odclinic-reports.txt"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_practice_name_per_report_paths_and_options() {
        let mut prefs = ReportPreferences::default();
        prefs.practice_name = "Highland Dental Clinic".into();
        prefs.set_export_path("recall-asap", "C:\\Reports\\recall.csv");
        prefs.set_export_path("payment-plans", "D:\\Finance\\plans.html");
        prefs.aging.age = AgingAgeFilter::Over60;
        prefs.aging.only_negative = true;
        prefs.aging.show_last_pay = true;
        prefs.custom_aging.age = AgingAgeFilter::Over30;
        prefs.custom_aging.negative = CustomAgingNegative::ShowOnly;
        prefs.custom_aging.include = CustomAgingInclude {
            procedure_fees: true,
            adjustments: false,
            pay_plan_charges: true,
            pay_plan_production: false,
            patient_payments: true,
            insurance_payments: true,
            insurance_estimates: false,
            write_offs: true,
            write_off_estimates: false,
        };
        prefs.custom_aging.age_credits = true;
        prefs.custom_aging.all_providers = false;
        prefs.custom_aging.provider_nums = vec![3, 7];
        prefs.custom_aging.all_clinics = false;
        prefs.custom_aging.clinic_nums = vec![2, 9];
        prefs.custom_aging.exclude_bad_address = true;
        prefs.custom_aging.family_grouping = AgingFamilyGrouping::Family;
        prefs.custom_aging.bill_type_nums = vec![4, 9];
        prefs.custom_aging.exclude_inactive = true;
        prefs.custom_aging.exclude_archive = true;
        prefs.custom_aging.writeoff_date = CustomAgingWriteoffDate::ClaimPayDate;
        prefs.pay_plans.plan_type = PlanTypeFilter::Patient;
        prefs.pay_plans.show_family_balance = true;
        prefs.pay_plans.hide_completed = true;
        prefs.pay_plans.only_stale_resolved = true;
        prefs.pay_plan_charge_applications.only_refunds_transfers = true;
        prefs.new_patients.exclude_no_production = true;
        prefs.appointments.date_basis = AppointmentDateBasis::Created;
        prefs.appointments.show_note_appointments = true;
        prefs.active_patients.all_providers = false;
        prefs.active_patients.provider_num = 42;
        prefs.prod_inc.mode = ProdIncMode::Provider;
        prefs.prod_inc.include_unearned = false;
        prefs.pay_sheet.all_providers = false;
        prefs.pay_sheet.provider_num = 7;
        prefs.pay_sheet.group_by = PaySheetGroupBy::Patient;
        prefs.pay_sheet.show_splits_by_provider = false;
        prefs.pay_sheet.all_payment_types = false;
        prefs.pay_sheet.payment_type_num = 12;
        prefs.adj_sheet.all_providers = false;
        prefs.adj_sheet.provider_num = 9;
        prefs.adj_sheet.all_types = false;
        prefs.adj_sheet.type_num = 15;
        prefs.finance_charge.all_providers = false;
        prefs.finance_charge.provider_num = 3;
        prefs.finance_charge.adjustment_type_num = 21;
        prefs.broken_appointments.mode = BrokenAppointmentMode::ByProcedures;
        prefs.broken_appointments.all_providers = false;
        prefs.broken_appointments.provider_num = 5;
        prefs.broken_appointments.procedure_option = BrokenProcedureOption::Both;
        prefs.broken_appointments.adjustment_type_num = 17;
        prefs.confirmation.fields = vec!["LName".into(), "FName".into(), "AptDateTime".into()];
        prefs.recall_fields.fields = vec!["DateDue".into(), "PatNum".into()];
        prefs.procedure_sheet.mode = ProcedureSheetMode::GroupedByCode;
        prefs.procedure_sheet.all_providers = false;
        prefs.procedure_sheet.provider_num = 7;
        prefs.procedure_sheet.code_filter = "D1110".into();
        prefs.procedure_codes.fee_schedule_num = 53;
        prefs.procedure_codes.categories = true;
        prefs.procedure_codes.include_blanks = true;
        prefs.procedure_codes.provider_num = 7;
        prefs.prescriptions.search_patient = false;
        prefs.prescriptions.input_text = "Amoxicillin".into();
        prefs.referrals.fields = vec!["LName".into(), "Specialty".into()];
        prefs.referrals.filters = vec![
            ReferralFilter {
                field: "LName".into(),
                condition: ReferralFilterCondition::Like,
                value: "Gir;ma|100%".into(),
            },
            ReferralFilter {
                field: "Specialty".into(),
                condition: ReferralFilterCondition::Eq,
                value: "17".into(),
            },
        ];
        prefs.patients.fields = vec!["LName".into(), "First Complete Proc".into()];
        prefs.patients.filters = vec![ReportFilter {
            field: "LName".into(),
            condition: ReportFilterCondition::Like,
            value: "O'Ne|ll;100%".into(),
        }];
        prefs.laser_labels.tab = LaserLabelTab::Insurance;
        prefs.laser_labels.ins_mode = LaserInsMode::Range;
        prefs.laser_labels.ins_copies = 3;
        prefs.laser_labels.start_name = "Alpha Insurance".into();
        prefs.laser_labels.start_addr = "1 Main St".into();
        prefs.laser_labels.end_name = "Zeta Insurance".into();
        prefs.laser_labels.end_addr = "9 High St".into();
        prefs.treatment_finder.pats_with_appts = true;
        prefs.treatment_finder.use_treating_provider = true;
        prefs.treatment_finder.provider_num = 7;
        prefs.treatment_finder.billing_type_num = 3;
        prefs.treatment_finder.clinic_num = 2;
        prefs.treatment_finder.above_amount = 250.0;
        prefs.treatment_finder.code1 = "D1110".into();
        prefs.treatment_finder.code2 = "D2999".into();
        let text = prefs.to_text();
        let loaded = ReportPreferences::from_text(&text);
        assert_eq!(loaded, prefs);
        assert_eq!(loaded.export_path("recall-asap"), Some("C:\\Reports\\recall.csv"));
        assert_eq!(loaded.export_path("unknown"), None);
    }

    #[test]
    fn defaults_match_upstream_form_checkboxes() {
        let prefs = ReportPreferences::default();
        assert_eq!(prefs.aging.age, AgingAgeFilter::Any);
        assert!(!prefs.aging.show_last_pay);
        assert_eq!(prefs.custom_aging.age, AgingAgeFilter::Any);
        assert_eq!(prefs.custom_aging.negative, CustomAgingNegative::Exclude);
        assert_eq!(prefs.custom_aging.include, CustomAgingInclude::default());
        assert!(prefs.custom_aging.include.any());
        assert!(prefs.custom_aging.include.procedure_fees);
        assert!(prefs.custom_aging.include.adjustments);
        assert!(!prefs.custom_aging.include.pay_plan_charges);
        assert!(prefs.custom_aging.include.pay_plan_production);
        assert!(prefs.custom_aging.include.patient_payments);
        assert!(!prefs.custom_aging.age_credits);
        assert!(prefs.custom_aging.all_providers);
        assert!(prefs.custom_aging.all_clinics);
        assert!(!prefs.custom_aging.exclude_bad_address);
        assert_eq!(prefs.custom_aging.family_grouping, AgingFamilyGrouping::Individual);
        assert!(prefs.custom_aging.bill_type_nums.is_empty());
        assert!(!prefs.custom_aging.exclude_inactive);
        assert!(!prefs.custom_aging.exclude_archive);
        assert_eq!(prefs.custom_aging.writeoff_date, CustomAgingWriteoffDate::ProcedureDate);
        assert_eq!(prefs.pay_plans.plan_type, PlanTypeFilter::Both);
        assert!(prefs.pay_plans.limit_date_range);
        assert!(!prefs.pay_plans.hide_completed);
        assert!(!prefs.pay_plans.only_stale_resolved);
        assert!(!prefs.pay_plan_charge_applications.only_refunds_transfers);
        assert!(!prefs.new_patients.include_address);
        assert_eq!(prefs.appointments.date_basis, AppointmentDateBasis::Appointment);
        assert!(!prefs.appointments.show_note_appointments);
        assert!(prefs.active_patients.all_providers);
        assert_eq!(prefs.prod_inc.mode, ProdIncMode::Daily);
        assert!(prefs.prod_inc.include_unearned, "upstream checks Include Unearned by default");
        assert!(prefs.pay_sheet.all_providers);
        assert_eq!(prefs.pay_sheet.group_by, PaySheetGroupBy::Check);
        assert!(prefs.pay_sheet.show_splits_by_provider);
        assert!(prefs.pay_sheet.all_payment_types);
        assert!(prefs.adj_sheet.all_providers);
        assert!(prefs.adj_sheet.all_types);
        assert!(prefs.finance_charge.all_providers);
        assert_eq!(prefs.finance_charge.adjustment_type_num, 0);
        assert_eq!(
            prefs.broken_appointments.mode,
            BrokenAppointmentMode::ByAppointmentStatus
        );
        assert_eq!(
            prefs.broken_appointments.procedure_option,
            BrokenProcedureOption::Missed
        );
        assert!(prefs.broken_appointments.all_providers);
        assert_eq!(
            prefs.confirmation.fields,
            CONFIRMATION_STANDARD_FIELDS
                .iter()
                .map(|s| (*s).to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            prefs.recall_fields.fields,
            RECALL_STANDARD_FIELDS
                .iter()
                .map(|s| (*s).to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            prefs.procedure_sheet.mode,
            ProcedureSheetMode::Individual
        );
        assert!(prefs.procedure_sheet.all_providers);
        assert_eq!(prefs.procedure_sheet.code_filter, "");
        assert_eq!(prefs.procedure_codes, ProcedureCodesReportOptions::default());
        assert!(!prefs.procedure_codes.categories, "upstream defaults to Code");
        assert!(!prefs.procedure_codes.include_blanks);
        assert!(prefs.prescriptions.search_patient, "upstream defaults to Patient");
        assert_eq!(prefs.prescriptions.input_text, "");
        assert_eq!(
            prefs.referrals.fields,
            REFERRAL_REPORT_DEFAULT_FIELDS
                .iter()
                .map(|s| (*s).to_string())
                .collect::<Vec<_>>()
        );
        assert!(prefs.referrals.filters.is_empty());
        assert_eq!(
            prefs.patients.fields,
            PATIENT_REPORT_DEFAULT_FIELDS
                .iter()
                .map(|s| (*s).to_string())
                .collect::<Vec<_>>()
        );
        assert!(prefs.patients.filters.is_empty());
        assert_eq!(
            ReferralFilterCondition::from_str("<>"),
            ReferralFilterCondition::Ne
        );
        assert_eq!(ReferralFilterCondition::Gte.as_str(), ">=");
    }

    #[test]
    fn practice_name_may_contain_equals_and_paths_may_contain_backslashes() {
        let text = "practice_name=Clinic = Main\nexport.recall-asap=C:\\a=b\\r.csv\n";
        let prefs = ReportPreferences::from_text(text);
        assert_eq!(prefs.practice_name, "Clinic = Main");
        assert_eq!(
            prefs.export_path("recall-asap"),
            Some("C:\\a=b\\r.csv")
        );
    }

    #[test]
    fn ignores_unknown_and_malformed_lines() {
        let text = "unknown=1\nexport.=ignored\nno-equals-line\npractice_name=Ok\noption.unknown=true\n";
        let prefs = ReportPreferences::from_text(text);
        assert_eq!(prefs.practice_name, "Ok");
        assert_eq!(prefs.export_path(""), None);
    }
}
