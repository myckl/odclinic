//! Definitions and display fields — Open Dental's two main customisation tables.
//!
//! `definition` supplies the drop-down lists used across almost every module:
//! adjustment types, payment types, appointment colours, recall statuses and so
//! on. `displayfield` controls which columns appear in the various OD grids and
//! lets the practice rename and resize them. Both table shapes are taken from
//! the public 24.3 C# source (`OpenDentBusiness/TableTypes/Def.cs` and
//! `DisplayField.cs`) and verified against the shipped 25.3.48.0
//! `OpenDentBusiness.dll` with REA.

use crate::Id;

/// An Open Dental `DefCat` code. Unknown codes are preserved so a real OD
/// database never loses rows this build does not render.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct DefinitionCategory(pub i16);

impl DefinitionCategory {
    pub const ACCOUNT_COLORS: Self = Self(0);
    pub const ADJ_TYPES: Self = Self(1);
    pub const APPT_CONFIRMED: Self = Self(2);
    pub const APPT_PROCS_QUICK_ADD: Self = Self(3);
    pub const BILLING_TYPES: Self = Self(4);
    pub const CLAIM_FORMATS: Self = Self(5);
    pub const DUNNING_MESSAGES: Self = Self(6);
    pub const FEE_SCHED_NAMES_OLD: Self = Self(7);
    pub const MEDICAL_NOTES: Self = Self(8);
    pub const OPERATORIES_OLD: Self = Self(9);
    pub const PAYMENT_TYPES: Self = Self(10);
    pub const PROC_CODE_CATS: Self = Self(11);
    pub const PROG_NOTE_COLORS: Self = Self(12);
    pub const RECALL_UNSCHED_STATUS: Self = Self(13);
    pub const SERVICE_NOTES: Self = Self(14);
    pub const DISCOUNT_TYPES: Self = Self(15);
    pub const DIAGNOSIS: Self = Self(16);
    pub const APPOINTMENT_COLORS: Self = Self(17);
    pub const IMAGE_CATS: Self = Self(18);
    pub const APPT_PHONE_NOTES: Self = Self(19);
    pub const TX_PRIORITIES: Self = Self(20);
    pub const MISC_COLORS: Self = Self(21);
    pub const CHART_GRAPHIC_COLORS: Self = Self(22);
    pub const CONTACT_CATEGORIES: Self = Self(23);
    pub const LETTER_MERGE_CATS: Self = Self(24);
    pub const BLOCKOUT_TYPES: Self = Self(25);
    pub const PROC_BUTTON_CATS: Self = Self(26);
    pub const COMM_LOG_TYPES: Self = Self(27);
    pub const SUPPLY_CATS: Self = Self(28);
    pub const PAY_SPLIT_UNEARNED_TYPE: Self = Self(29);
    pub const PROGNOSIS: Self = Self(30);
    pub const CLAIM_CUSTOM_TRACKING: Self = Self(31);
    pub const INSURANCE_PAYMENT_TYPE: Self = Self(32);
    pub const TASK_PRIORITIES: Self = Self(33);
    pub const FEE_COLORS: Self = Self(34);
    pub const PROVIDER_SPECIALTIES: Self = Self(35);
    pub const CLAIM_PAYMENT_TRACKING: Self = Self(36);
    pub const ACCOUNT_QUICK_CHARGE: Self = Self(37);
    pub const INSURANCE_VERIFICATION_STATUS: Self = Self(38);
    pub const REGIONS: Self = Self(39);
    pub const CLAIM_PAYMENT_GROUPS: Self = Self(40);
    pub const AUTO_NOTE_CATS: Self = Self(41);
    pub const WEB_SCHED_NEW_PAT_APPT_TYPES: Self = Self(42);
    pub const CLAIM_ERROR_CODE: Self = Self(43);
    pub const CLINIC_SPECIALTY: Self = Self(44);
    pub const JOB_PRIORITIES: Self = Self(45);
    pub const CARRIER_GROUP_NAMES: Self = Self(46);
    pub const PAY_PLAN_CATEGORIES: Self = Self(47);
    pub const AUTO_DEPOSIT: Self = Self(48);
    pub const INSURANCE_FILING_CODE_GROUP: Self = Self(49);
    pub const TIME_CARD_ADJ_TYPES: Self = Self(50);
    pub const WEB_SCHED_EXISTING_APPT_TYPES: Self = Self(51);
    pub const CERTIFICATION_CATEGORIES: Self = Self(52);
    pub const ECLIPBOARD_IMAGE_CAPTURE: Self = Self(53);
    pub const TASK_CATEGORIES: Self = Self(54);
    pub const OPERATORY_TYPES: Self = Self(55);

    /// Code and display name for every OD 24.3 `DefCat` value.
    pub const ALL: [(Self, &'static str); 56] = [
        (Self::ACCOUNT_COLORS, "Account Colors"),
        (Self::ADJ_TYPES, "Adj Types"),
        (Self::APPT_CONFIRMED, "Appt Confirmed"),
        (Self::APPT_PROCS_QUICK_ADD, "Appt Procs Quick Add"),
        (Self::BILLING_TYPES, "Billing Types"),
        (Self::CLAIM_FORMATS, "NotUsed"),
        (Self::DUNNING_MESSAGES, "NotUsed"),
        (Self::FEE_SCHED_NAMES_OLD, "NotUsed"),
        (Self::MEDICAL_NOTES, "NotUsed"),
        (Self::OPERATORIES_OLD, "NotUsed"),
        (Self::PAYMENT_TYPES, "Payment Types"),
        (Self::PROC_CODE_CATS, "Proc Code Categories"),
        (Self::PROG_NOTE_COLORS, "Prog Notes Colors"),
        (Self::RECALL_UNSCHED_STATUS, "Recall/Unsched Status"),
        (Self::SERVICE_NOTES, "NotUsed"),
        (Self::DISCOUNT_TYPES, "NotUsed"),
        (Self::DIAGNOSIS, "Diagnosis Types"),
        (Self::APPOINTMENT_COLORS, "Appointment Colors"),
        (Self::IMAGE_CATS, "Image Categories"),
        (Self::APPT_PHONE_NOTES, "NotUsed"),
        (Self::TX_PRIORITIES, "Treat' Plan Priorities"),
        (Self::MISC_COLORS, "Misc Colors"),
        (Self::CHART_GRAPHIC_COLORS, "Chart Graphic Colors"),
        (Self::CONTACT_CATEGORIES, "Contact Categories"),
        (Self::LETTER_MERGE_CATS, "Letter Merge Cats"),
        (Self::BLOCKOUT_TYPES, "Blockout Types"),
        (Self::PROC_BUTTON_CATS, "Proc Button Categories"),
        (Self::COMM_LOG_TYPES, "Commlog Types"),
        (Self::SUPPLY_CATS, "Supply Categories"),
        (Self::PAY_SPLIT_UNEARNED_TYPE, "PaySplit Unearned Types"),
        (Self::PROGNOSIS, "Prognosis"),
        (Self::CLAIM_CUSTOM_TRACKING, "Claim Custom Tracking"),
        (Self::INSURANCE_PAYMENT_TYPE, "Insurance Payment Types"),
        (Self::TASK_PRIORITIES, "Task Priorities"),
        (Self::FEE_COLORS, "Fee Colors"),
        (Self::PROVIDER_SPECIALTIES, "Provider Specialties"),
        (Self::CLAIM_PAYMENT_TRACKING, "Claim Payment Tracking"),
        (Self::ACCOUNT_QUICK_CHARGE, "Account Procs Quick Add"),
        (Self::INSURANCE_VERIFICATION_STATUS, "Insurance Verification Status"),
        (Self::REGIONS, "Regions"),
        (Self::CLAIM_PAYMENT_GROUPS, "Claim Payment Groups"),
        (Self::AUTO_NOTE_CATS, "Auto Note Categories"),
        (Self::WEB_SCHED_NEW_PAT_APPT_TYPES, "Web Sched New Pat Appt Types"),
        (Self::CLAIM_ERROR_CODE, "Claim Error Code"),
        (Self::CLINIC_SPECIALTY, "Clinic Specialties"),
        (Self::JOB_PRIORITIES, "Job Priorities HqOnly"),
        (Self::CARRIER_GROUP_NAMES, "Carrier Group Names"),
        (Self::PAY_PLAN_CATEGORIES, "Payment Plan Categories"),
        (Self::AUTO_DEPOSIT, "Auto Deposit Account"),
        (Self::INSURANCE_FILING_CODE_GROUP, "Insurance Filing Code Group"),
        (Self::TIME_CARD_ADJ_TYPES, "Time Card Adj Types"),
        (Self::WEB_SCHED_EXISTING_APPT_TYPES, "Web Sched Existing Appt Types"),
        (Self::CERTIFICATION_CATEGORIES, "Certification Categories"),
        (Self::ECLIPBOARD_IMAGE_CAPTURE, "eClipboard Images"),
        (Self::TASK_CATEGORIES, "Task Categories HqOnly"),
        (Self::OPERATORY_TYPES, "Operatory Types"),
    ];

    /// Categories this port exposes in the Definitions editor. `NotUsed`, HQ
    /// only and eClipboard categories are hidden the same way OD's `DefL` does.
    pub const EDITABLE: [Self; 18] = [
        Self::ACCOUNT_COLORS,
        Self::ADJ_TYPES,
        Self::APPT_CONFIRMED,
        Self::APPT_PROCS_QUICK_ADD,
        Self::BILLING_TYPES,
        Self::PAYMENT_TYPES,
        Self::PROC_CODE_CATS,
        Self::PROG_NOTE_COLORS,
        Self::RECALL_UNSCHED_STATUS,
        Self::DIAGNOSIS,
        Self::APPOINTMENT_COLORS,
        Self::TX_PRIORITIES,
        Self::MISC_COLORS,
        Self::CHART_GRAPHIC_COLORS,
        Self::BLOCKOUT_TYPES,
        Self::COMM_LOG_TYPES,
        Self::PAY_PLAN_CATEGORIES,
        Self::OPERATORY_TYPES,
    ];

    pub fn from_od(value: i16) -> Self {
        Self(value)
    }

    pub fn as_od(self) -> i16 {
        self.0
    }

    pub fn label(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(category, _)| *category == self)
            .map(|(_, label)| *label)
            .unwrap_or("Other")
    }

    pub fn is_editable(self) -> bool {
        Self::EDITABLE.contains(&self)
    }

    /// Editing rules mirror OD's `DefL.GetOptionsForDefCats`.
    pub fn options(self) -> DefinitionCategoryOptions {
        match self {
            Self::ACCOUNT_COLORS => DefinitionCategoryOptions {
                can_edit_name: false,
                enable_value: false,
                enable_color: true,
                can_hide: true,
                can_delete: false,
                value_label: "",
                help: "Changes the colour of text for entries in the Account module.",
                ..DefinitionCategoryOptions::DEFAULT
            },
            Self::ADJ_TYPES => DefinitionCategoryOptions {
                enable_value: true,
                value_label: "+, -, or dp",
                help: "Plus increases the patient balance; minus decreases it. Do not change the value after creating a type.",
                ..DefinitionCategoryOptions::DEFAULT
            },
            Self::APPT_CONFIRMED => DefinitionCategoryOptions {
                enable_value: true,
                enable_color: true,
                value_label: "Abbrev",
                help: "Confirmation types shown on appointments when confirmed colours are enabled.",
                ..DefinitionCategoryOptions::DEFAULT
            },
            Self::APPT_PROCS_QUICK_ADD | Self::ACCOUNT_QUICK_CHARGE => DefinitionCategoryOptions {
                can_delete: true,
                enable_value: true,
                value_label: "Procedure codes",
                help: "Comma-delimited procedure codes for the appointment or account quick-add list.",
                ..DefinitionCategoryOptions::DEFAULT
            },
            Self::BILLING_TYPES => DefinitionCategoryOptions {
                enable_value: true,
                value_label: "E, C, or CE",
                help: "E=email bill, C=collection, CE=collection excluded. Changes affect all patients.",
                ..DefinitionCategoryOptions::DEFAULT
            },
            Self::PAYMENT_TYPES => DefinitionCategoryOptions {
                enable_value: true,
                value_label: "N=Not selected for deposit",
                help: "Types of payments patients might make. Changes affect all patients.",
                ..DefinitionCategoryOptions::DEFAULT
            },
            Self::RECALL_UNSCHED_STATUS => DefinitionCategoryOptions {
                enable_value: true,
                value_label: "Abbreviation",
                help: "Recall/unscheduled statuses. Keep abbreviations to seven characters or less.",
                ..DefinitionCategoryOptions::DEFAULT
            },
            Self::DIAGNOSIS => DefinitionCategoryOptions {
                enable_value: true,
                value_label: "1 or 2 letter abbreviation",
                help: "Diagnosis list shown when entering a procedure. Changes affect all patients.",
                ..DefinitionCategoryOptions::DEFAULT
            },
            Self::APPOINTMENT_COLORS => DefinitionCategoryOptions {
                can_edit_name: false,
                enable_color: true,
                help: "Background colours in the Appointments module and for completed appointments.",
                ..DefinitionCategoryOptions::DEFAULT
            },
            Self::TX_PRIORITIES => DefinitionCategoryOptions {
                enable_value: true,
                enable_color: true,
                do_show_item_order_in_value: true,
                value_label: "Internal Priority",
                help: "Displayed order should match treatment priority. Changes affect all procedures that use it.",
                ..DefinitionCategoryOptions::DEFAULT
            },
            Self::BLOCKOUT_TYPES => DefinitionCategoryOptions {
                enable_color: true,
                enable_value: true,
                value_label: "Flags",
                help: "Blockout types used in the Appointments module.",
                ..DefinitionCategoryOptions::DEFAULT
            },
            Self::COMM_LOG_TYPES => DefinitionCategoryOptions {
                enable_value: true,
                enable_color: true,
                do_show_no_color: true,
                value_label: "Usage",
                help: "Commlog entry types. Optionally set Usage to automate new entries.",
                ..DefinitionCategoryOptions::DEFAULT
            },
            Self::PAY_PLAN_CATEGORIES => DefinitionCategoryOptions {
                help: "Assign payment plans to different categories.",
                ..DefinitionCategoryOptions::DEFAULT
            },
            Self::OPERATORY_TYPES => DefinitionCategoryOptions {
                can_delete: true,
                help: "Informational types for operatories.",
                ..DefinitionCategoryOptions::DEFAULT
            },
            Self::MISC_COLORS | Self::CHART_GRAPHIC_COLORS | Self::PROG_NOTE_COLORS => {
                DefinitionCategoryOptions {
                    can_edit_name: false,
                    enable_color: true,
                    do_show_no_color: self == Self::MISC_COLORS,
                    help: "Colours used by the Chart and appointment views.",
                    ..DefinitionCategoryOptions::DEFAULT
                }
            }
            _ => DefinitionCategoryOptions::DEFAULT,
        }
    }
}

/// Per-category editing rules shown beside the Definitions grid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DefinitionCategoryOptions {
    pub can_edit_name: bool,
    pub enable_value: bool,
    pub enable_color: bool,
    pub can_hide: bool,
    pub can_delete: bool,
    pub do_show_item_order_in_value: bool,
    pub do_show_no_color: bool,
    pub value_label: &'static str,
    pub help: &'static str,
}

impl DefinitionCategoryOptions {
    pub const DEFAULT: Self = Self {
        can_edit_name: true,
        enable_value: false,
        enable_color: false,
        can_hide: true,
        can_delete: false,
        do_show_item_order_in_value: false,
        do_show_no_color: false,
        value_label: "",
        help: "",
    };
}

/// One row of OD `definition`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Definition {
    pub def_num: Id,
    pub category: DefinitionCategory,
    pub item_order: i32,
    pub item_name: String,
    pub item_value: String,
    /// OD `Color.ToArgb()` as a signed 32-bit integer. Zero means no colour.
    pub item_color: i32,
    pub is_hidden: bool,
}

/// An OD `DisplayFieldCategory` code.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct DisplayFieldCategory(pub i16);

impl DisplayFieldCategory {
    pub const NONE: Self = Self(0); // OD calls this "Progress Notes".
    pub const PATIENT_SELECT: Self = Self(1);
    pub const PATIENT_INFORMATION: Self = Self(2);
    pub const ACCOUNT_MODULE: Self = Self(3);
    pub const RECALL_LIST: Self = Self(4);
    pub const CHART_PATIENT_INFORMATION: Self = Self(5);
    pub const PROCEDURE_GROUP_NOTE: Self = Self(6);
    pub const TREATMENT_PLAN_MODULE: Self = Self(7);
    pub const ORTHO_CHART: Self = Self(8);
    pub const APPOINTMENT_BUBBLE: Self = Self(9);
    pub const ACCOUNT_PATIENT_INFORMATION: Self = Self(10);
    pub const STATEMENT_MAIN_GRID: Self = Self(11);
    pub const FAMILY_RECALL_GRID: Self = Self(12);
    pub const APPOINTMENT_EDIT: Self = Self(13);
    pub const PLANNED_APPOINTMENT_EDIT: Self = Self(14);
    pub const OUTSTANDING_INS_REPORT: Self = Self(15);
    pub const CEMT_SEARCH_PATIENTS: Self = Self(16);
    pub const AR_MANAGER_SENT_GRID: Self = Self(17);
    pub const AR_MANAGER_UNSENT_GRID: Self = Self(18);
    pub const AR_MANAGER_EXCLUDED_GRID: Self = Self(19);
    pub const LIMITED_CUSTOM_STATEMENT: Self = Self(20);
    pub const SUPER_FAMILY_GRID_COLS: Self = Self(21);

    pub const ALL: [(Self, &'static str); 22] = [
        (Self::NONE, "Progress Notes"),
        (Self::PATIENT_SELECT, "Patient Select"),
        (Self::PATIENT_INFORMATION, "Patient Information"),
        (Self::ACCOUNT_MODULE, "Account Module"),
        (Self::RECALL_LIST, "Recall List"),
        (Self::CHART_PATIENT_INFORMATION, "Chart Patient Information"),
        (Self::PROCEDURE_GROUP_NOTE, "Procedure Group Note"),
        (Self::TREATMENT_PLAN_MODULE, "Treatment Plan Module"),
        (Self::ORTHO_CHART, "Ortho Chart"),
        (Self::APPOINTMENT_BUBBLE, "Appointment Bubble"),
        (Self::ACCOUNT_PATIENT_INFORMATION, "Account Patient Information"),
        (Self::STATEMENT_MAIN_GRID, "Statement Main Grid"),
        (Self::FAMILY_RECALL_GRID, "Family Recall Grid"),
        (Self::APPOINTMENT_EDIT, "Appointment Edit"),
        (Self::PLANNED_APPOINTMENT_EDIT, "Planned Appointment Edit"),
        (Self::OUTSTANDING_INS_REPORT, "Outstanding Ins Report"),
        (Self::CEMT_SEARCH_PATIENTS, "Patient Search"),
        (Self::AR_MANAGER_SENT_GRID, "A/R Manager Sent Grid"),
        (Self::AR_MANAGER_UNSENT_GRID, "A/R Manager Unsent Grid"),
        (Self::AR_MANAGER_EXCLUDED_GRID, "A/R Manager Excluded Grid"),
        (Self::LIMITED_CUSTOM_STATEMENT, "Limited Custom Statement"),
        (Self::SUPER_FAMILY_GRID_COLS, "SuperFamily Grid Columns"),
    ];

    /// Categories with defaults in this port. Insurance, ortho and HQ-only
    /// grids can still be edited if a real OD database already has rows.
    pub const SEEDED: [Self; 7] = [
        Self::NONE,
        Self::PATIENT_SELECT,
        Self::PATIENT_INFORMATION,
        Self::ACCOUNT_MODULE,
        Self::RECALL_LIST,
        Self::TREATMENT_PLAN_MODULE,
        Self::APPOINTMENT_BUBBLE,
    ];

    pub fn from_od(value: i16) -> Self {
        Self(value)
    }

    pub fn as_od(self) -> i16 {
        self.0
    }

    pub fn label(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(category, _)| *category == self)
            .map(|(_, label)| *label)
            .unwrap_or("Other")
    }
}

/// One row of OD `displayfield`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayField {
    pub display_field_num: Id,
    pub internal_name: String,
    pub item_order: i32,
    pub description: String,
    pub column_width: i32,
    pub category: DisplayFieldCategory,
    pub chart_view_num: Id,
    pub pick_list: String,
    pub description_override: String,
}

impl DisplayField {
    /// OD `DisplayField.ToDescriptionString()`: the override is preferred for
    /// Ortho and SuperFamily grids, then the translated description, then the
    /// field's internal name.
    pub fn display_label(&self) -> &str {
        for candidate in [
            &self.description_override,
            &self.description,
            &self.internal_name,
        ] {
            if !candidate.trim().is_empty() {
                return candidate;
            }
        }
        ""
    }
}

/// Build one default display field row, matching `new DisplayField(name,width,category)`.
fn field(internal_name: &str, column_width: i32, category: DisplayFieldCategory) -> DisplayField {
    DisplayField {
        internal_name: internal_name.into(),
        column_width,
        category,
        ..DisplayField::default()
    }
}

/// The default column lists used by OD when a category has never been saved.
/// Only grids this port can show are seeded; unknown categories come back empty
/// but remain editable through the store.
pub fn default_display_fields(category: DisplayFieldCategory) -> Vec<DisplayField> {
    let names: &[(&str, i32)] = match category {
        DisplayFieldCategory::NONE => &[
            ("Date", 67),
            ("Th", 27),
            ("Surf", 40),
            ("Dx", 28),
            ("Description", 218),
            ("Stat", 25),
            ("Prov", 42),
            ("Amount", 48),
            ("Proc Code", 62),
            ("User", 62),
            ("Signed", 55),
        ],
        DisplayFieldCategory::PATIENT_SELECT => &[
            ("LastName", 75),
            ("First Name", 75),
            ("Pref Name", 60),
            ("Age", 30),
            ("Hm Phone", 90),
            ("Wk Phone", 90),
            ("PatNum", 80),
            ("Address", 100),
            ("Status", 65),
        ],
        DisplayFieldCategory::PATIENT_INFORMATION => &[
            ("Last", 0),
            ("First", 0),
            ("Preferred", 0),
            ("Title", 0),
            ("Status", 0),
            ("Gender", 0),
            ("Birthdate", 0),
            ("Age", 0),
            ("Address", 0),
            ("City", 0),
            ("State", 0),
            ("Zip", 0),
            ("Hm Phone", 0),
            ("Wk Phone", 0),
            ("E-mail", 0),
            ("Billing Type", 0),
            ("Primary Provider", 0),
            ("Language", 0),
        ],
        DisplayFieldCategory::ACCOUNT_MODULE => &[
            ("Date", 65),
            ("Patient", 100),
            ("Prov", 40),
            ("Code", 46),
            ("Tth", 26),
            ("Description", 270),
            ("Charges", 60),
            ("Credits", 60),
            ("Balance", 60),
        ],
        DisplayFieldCategory::RECALL_LIST => &[
            ("Patient", 150),
            ("Due Date", 80),
            ("Status", 80),
            ("Phone", 100),
            ("Providers", 100),
        ],
        DisplayFieldCategory::TREATMENT_PLAN_MODULE => &[
            ("ItemOrder", 40),
            ("Priority", 70),
            ("Proc Code", 70),
            ("Description", 220),
            ("Tooth", 50),
            ("Fee", 70),
        ],
        DisplayFieldCategory::APPOINTMENT_BUBBLE => &[
            ("Patient Name", 100),
            ("Time", 60),
            ("Provider", 80),
            ("Procedure", 160),
            ("Note", 160),
        ],
        _ => &[],
    };
    names
        .iter()
        .enumerate()
        .map(|(index, (name, width))| {
            let mut row = field(name, *width, category);
            row.item_order = index as i32;
            row
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn od_category_codes_round_trip() {
        for (category, label) in DefinitionCategory::ALL {
            assert_eq!(DefinitionCategory::from_od(category.as_od()), category);
            assert_eq!(category.label(), label);
        }
        for (category, label) in DisplayFieldCategory::ALL {
            assert_eq!(DisplayFieldCategory::from_od(category.as_od()), category);
            assert_eq!(category.label(), label);
        }
    }

    #[test]
    fn saved_codes_are_preserved() {
        let unknown = DefinitionCategory::from_od(999);
        assert_eq!(unknown.as_od(), 999);
        assert_eq!(unknown.label(), "Other");
        assert!(!unknown.is_editable());
    }

    #[test]
    fn defaults_have_unique_sorted_orders() {
        for category in DisplayFieldCategory::SEEDED {
            let rows = default_display_fields(category);
            assert!(!rows.is_empty());
            for (index, row) in rows.iter().enumerate() {
                assert_eq!(row.item_order, index as i32);
                assert_eq!(row.category, category);
            }
        }
    }

    #[test]
    fn display_label_prefers_override() {
        let mut row = field("Hm Phone", 90, DisplayFieldCategory::PATIENT_SELECT);
        assert_eq!(row.display_label(), "Hm Phone");
        row.description = "Phone".into();
        assert_eq!(row.display_label(), "Phone");
        row.description_override = "Tel".into();
        assert_eq!(row.display_label(), "Tel");
    }
}
