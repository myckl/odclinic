//! Persisted options for the second tranche of upstream `FormRp*` reports.
//!
//! These forms have no free-form field selection (unlike Referrals/Patients);
//! they only carry checkbox/radio choices. Keeping them in one module makes the
//! `ReportPreferences` file format small and easy to extend.

use super::Id;

/// Options shared by the three accounting statements. Upstream keeps its own
/// date pickers; this port reuses the report module's date range (the Balance
/// Sheet uses the range's end date as its "as of" date).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountingReportOptions {
    /// `FormRpAccountingBalanceSheet`: include the synthetic
    /// "NetIncomeThisYear" equity row (`Accounts.NetIncomeThisYear`).
    pub include_net_income: bool,
    /// `FormRpAccountingGenLedg`: show the opening-balance pseudo-row and the
    /// retained-earnings carry-forward on each RE row.
    pub show_opening_balances: bool,
}

impl Default for AccountingReportOptions {
    fn default() -> Self {
        // Both are on in upstream (the forms always call those engine helpers).
        Self {
            include_net_income: true,
            show_opening_balances: true,
        }
    }
}

/// `FormRpProcNote` "Group By" radio group.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum ProcNoteGroup {
    #[default]
    Procedure = 0,
    Patient = 1,
    DateAndPatient = 2,
}

impl ProcNoteGroup {
    pub const ALL: [Self; 3] = [Self::Procedure, Self::Patient, Self::DateAndPatient];

    pub fn from_od(v: u8) -> Self {
        match v {
            1 => Self::Patient,
            2 => Self::DateAndPatient,
            _ => Self::Procedure,
        }
    }
    pub fn as_od(self) -> u8 {
        self as u8
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Procedure => "procedure",
            Self::Patient => "patient",
            Self::DateAndPatient => "date-patient",
        }
    }
    pub fn from_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "patient" => Self::Patient,
            "date-patient" => Self::DateAndPatient,
            _ => Self::Procedure,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Procedure => "Procedure",
            Self::Patient => "Patient",
            Self::DateAndPatient => "Date and Patient",
        }
    }
}

/// Options from `FormRpProcNote`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProcNoteReportOptions {
    /// `checkNoNotes` — include procedures where the patient has no note on
    /// any procedure on the same day.
    pub include_no_notes: bool,
    /// `checkUnsignedNote` — include procedures whose latest note is unsigned.
    pub include_unsigned: bool,
    /// `checkShowExcludedCodes` — show D9986/D9987 broken-appointment codes.
    pub show_excluded_codes: bool,
    pub group_by: ProcNoteGroup,
}

/// `FormRpHiddenPaySplits`: the report has no option beyond the date range,
/// provider list and unearned-type list. The port offers all hidden unearned
/// types (definition category 29 with a non-empty `ItemValue`) and all
/// providers, which is upstream's default ("All"). It can exclude refunds.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HiddenPaySplitsReportOptions {
    /// Exclude negative splits (refunds) as well as positive unearned deposits.
    pub exclude_refunds: bool,
}

/// Options for the payment-plan charge applications / refund-transfer history report.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PayPlanChargeApplicationsReportOptions {
    /// Show only negative/unapplied-resolution transfers and refunds.
    pub only_refunds_transfers: bool,
}

/// The four tabs of `FormRpLaserLabels`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum LaserLabelTab {
    /// "Patient Labels" — one label per patient in name order.
    #[default]
    Patient = 0,
    /// "Insurance Company Labels" — one label per OD `carrier` row.
    Insurance = 1,
    /// "Custom Labels" — custom patient fields are not modelled, so this tab
    /// uses the same address block as patient labels.
    Custom = 2,
    /// "Birthday Labels" — patients with the selected month/day birthday.
    Birthday = 3,
}

impl LaserLabelTab {
    pub const ALL: [Self; 4] = [
        Self::Patient,
        Self::Insurance,
        Self::Custom,
        Self::Birthday,
    ];

    pub fn from_od(v: u8) -> Self {
        match v {
            1 => Self::Insurance,
            2 => Self::Custom,
            3 => Self::Birthday,
            _ => Self::Patient,
        }
    }
    pub fn as_od(self) -> u8 {
        self as u8
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Patient => "patient",
            Self::Insurance => "insurance",
            Self::Custom => "custom",
            Self::Birthday => "birthday",
        }
    }
    pub fn from_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "insurance" => Self::Insurance,
            "custom" => Self::Custom,
            "birthday" => Self::Birthday,
            _ => Self::Patient,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Patient => "Patient Labels",
            Self::Insurance => "Insurance Company Labels",
            Self::Custom => "Custom Labels",
            Self::Birthday => "Birthday Labels",
        }
    }
}

/// The two radio modes of the `FormRpLaserLabels` insurance-company tab.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum LaserInsMode {
    /// "Single Company" — equality on `CarrierName + Address`, repeated by
    /// `copies`. Upstream's default.
    #[default]
    Single = 0,
    /// "Range of Companies" — `CarrierName + Address + City` between the
    /// start and end keys, ordered by that key.
    Range = 1,
}

impl LaserInsMode {
    pub const ALL: [Self; 2] = [Self::Single, Self::Range];
    pub fn from_od(v: u8) -> Self {
        if v == 1 { Self::Range } else { Self::Single }
    }
    pub fn as_od(self) -> u8 {
        self as u8
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Range => "range",
        }
    }
    pub fn from_str(value: &str) -> Self {
        if value.trim().eq_ignore_ascii_case("range") {
            Self::Range
        } else {
            Self::Single
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Single => "Single Company",
            Self::Range => "Range of Companies",
        }
    }
}

/// Persisted options for `FormRpLaserLabels`. The four text fields mirror
/// upstream's selection-button fields: a start/end carrier name paired with
/// its address. OD concatenates them (plus `City` for the range query) to
/// build the SQL key.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LaserLabelOptions {
    pub tab: LaserLabelTab,
    /// Selection mode on the insurance-company tab.
    pub ins_mode: LaserInsMode,
    /// Number of labels printed for the matched single carrier.
    pub ins_copies: usize,
    pub start_name: String,
    pub start_addr: String,
    pub end_name: String,
    pub end_addr: String,
}

/// The four tabs of `FormRpUnearnedIncome`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum UnearnedIncomeTab {
    /// "Line Item Unearned Income" — every non-hidden unearned split in range.
    LineItem = 0,
    /// "Unearned Accounts" — one row per guarantor.
    #[default]
    Accounts = 1,
    /// "Unearned Allocation" — families with prepayments and remaining procs.
    Allocation = 2,
    /// "Net Unearned Income" — families with no remaining procs.
    Net = 3,
}

impl UnearnedIncomeTab {
    pub const ALL: [Self; 4] = [
        Self::LineItem,
        Self::Accounts,
        Self::Allocation,
        Self::Net,
    ];

    pub fn from_od(v: u8) -> Self {
        match v {
            0 => Self::LineItem,
            2 => Self::Allocation,
            3 => Self::Net,
            _ => Self::Accounts,
        }
    }
    pub fn as_od(self) -> u8 {
        self as u8
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LineItem => "line-item",
            Self::Accounts => "accounts",
            Self::Allocation => "allocation",
            Self::Net => "net",
        }
    }
    pub fn from_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "line-item" => Self::LineItem,
            "allocation" => Self::Allocation,
            "net" => Self::Net,
            _ => Self::Accounts,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::LineItem => "Line Item Unearned Income",
            Self::Accounts => "Unearned Accounts",
            Self::Allocation => "Unearned Allocation",
            Self::Net => "Net Unearned Income",
        }
    }
}

/// Options from `FormRpUnearnedIncome`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnearnedIncomeReportOptions {
    pub tab: UnearnedIncomeTab,
    /// `checkUnearnedAllocationExcludeZero` (default true).
    pub exclude_zero_allocation: bool,
    /// `checkNetUnearnedExcludeZero` (default false).
    pub exclude_zero_net: bool,
    /// `checkUnearnedAllocationShowProv` / `checkLineItemShowProv`.
    pub show_provider: bool,
}

impl Default for UnearnedIncomeReportOptions {
    fn default() -> Self {
        Self {
            tab: UnearnedIncomeTab::Accounts,
            exclude_zero_allocation: true,
            exclude_zero_net: false,
            show_provider: false,
        }
    }
}

/// Options from `FormRpProcOverpaid`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProcOverpaidReportOptions {
    /// `checkExcludeOverpaidWithIns` — keep only overpayments that involved an
    /// insurance payment or write-off.
    pub exclude_without_insurance: bool,
    /// Restrict to one patient (`patNum`); 0 means all patients.
    pub pat_num: Id,
}

/// Options from `FormRpTreatmentFinder`: the list of patients who have
/// treatment planned but (usually) no upcoming appointment. Upstream offers
/// multi-select provider/billing/clinic lists; this port keeps a single id per
/// list (`0` = All) to stay consistent with the other report option panels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreatmentFinderReportOptions {
    /// `checkIncludePatsWithApts` — include patients who already have a future
    /// scheduled appointment. Off by default, matching upstream.
    pub pats_with_appts: bool,
    /// `textOverAmount` — when greater than zero, drop patients whose remaining
    /// annual benefit is at or below this amount (insurance mode only).
    pub above_amount: f64,
    /// `0` = all providers; otherwise a `Provider.ProvNum`.
    pub provider_num: Id,
    /// `checkUseTreatingProvider` — filter on the procedure's provider instead
    /// of the patient's primary provider.
    pub use_treating_provider: bool,
    /// `0` = all billing types; otherwise a `DefCat.BillingTypes` DefNum.
    pub billing_type_num: Id,
    /// `0` = all clinics; otherwise a `Clinic.ClinicNum`.
    pub clinic_num: Id,
    /// Inclusive procedure-code range (upstream `StartRange`/`EndRange`). An
    /// empty `code1` disables the filter.
    pub code1: String,
    pub code2: String,
}
