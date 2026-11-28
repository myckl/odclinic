//! OD-style Preferences dialog.
//!
//! Open Dental keeps Preferences (typed `preference` rows, two-pane tree,
//! search and explicit Save) separate from Show Features (grouped booleans).
//! This module mirrors that split: the feature toggles stay on their own
//! page, while the dialog below edits real OD preference rows through
//! `Store::upsert_pref` under the normal OD write policy.

use super::*;
use std::collections::HashMap;

const PAGES: [&str; 10] = [
    "Main Window - General",
    "Main Window - Miscellaneous",
    "Appointment - General",
    "Appointment - Appearance",
    "Family - General",
    "Account - General",
    "Treat Plan - General",
    "Chart - General",
    "Chart - Procedures",
    "Server Connections - General",
];

#[derive(Clone, Copy)]
enum Kind {
    Bool,
    Text,
    Integer,
    Choice(&'static [(&'static str, &'static str)]),
}

struct Field {
    page: usize,
    name: &'static str,
    label: &'static str,
    kind: Kind,
    default: &'static str,
    help: &'static str,
}

const SHOW_ID: [(&str, &str); 4] = [
    ("None", "0"),
    ("PatNum", "1"),
    ("ChartNumber", "2"),
    ("Birthdate", "3"),
];
const TOOTH_NUMBERS: [(&str, &str); 4] = [
    ("Universal (1-32)", "0"),
    ("FDI (11-48)", "1"),
    ("Haderup", "2"),
    ("Palmer", "3"),
];
const ICD_VERSION: [(&str, &str); 2] = [("ICD-9", "9"), ("ICD-10", "10")];

macro_rules! field {
    ($page:expr, $name:expr, $label:expr, $kind:expr, $default:expr, $help:expr) => {
        Field {
            page: $page,
            name: $name,
            label: $label,
            kind: $kind,
            default: $default,
            help: $help,
        }
    };
}

/// Preference rows the port can safely round-trip with OD's own value encoding.
/// Booleans are `1`/`0` (`POut.Bool`); numbers and strings are stored as text,
/// exactly as OD stores them in `preference.ValueString`.
fn fields() -> Vec<Field> {
    vec![
        field!(0, "MainWindowTitle", "Main window title", Kind::Text, "", "OD: PrefName.MainWindowTitle"),
        field!(0, "ShowIDinTitleBar", "Show ID in title bar", Kind::Choice(&SHOW_ID), "0", "OD: PrefName.ShowIDinTitleBar"),
        field!(0, "TitleBarShowSpecialty", "Show specialty in title bar", Kind::Bool, "0", "OD: PrefName.TitleBarShowSpecialty"),
        field!(0, "TitleBarShowSite", "Show site in title bar", Kind::Bool, "0", "OD: PrefName.TitleBarShowSite"),
        field!(0, "TitleBarClinicUseAbbr", "Use clinic abbreviation", Kind::Bool, "0", "OD: PrefName.TitleBarClinicUseAbbr"),
        field!(0, "PatientSelectUsesSearchButton", "Patient select uses Search button", Kind::Bool, "1", "OD: PrefName.PatientSelectUsesSearchButton"),
        field!(0, "PatientSelectUseFNameForPreferred", "Use first name for preferred name", Kind::Bool, "0", "OD: PrefName.PatientSelectUseFNameForPreferred"),
        field!(0, "PatientSelectWindowShowGetAll", "Patient select shows Get All", Kind::Bool, "0", "OD: PrefName.PatientSelectWindowShowGetAll"),
        field!(0, "PatientSelectShowInactive", "Show inactive patients by default", Kind::Bool, "0", "OD: PrefName.PatientSelectShowInactive"),
        field!(0, "PopupsDisableTimeSpan", "Disable popups for (days.hh:mm:ss)", Kind::Text, "", "Blank means never. OD: PrefName.PopupsDisableTimeSpan"),
        field!(2, "BrokenApptAdjustment", "Broken appointment: create adjustment", Kind::Bool, "0", "OD: PrefName.BrokenApptAdjustment"),
        field!(2, "BrokenApptCommLog", "Broken appointment: create commlog", Kind::Bool, "0", "OD: PrefName.BrokenApptCommLog"),
        field!(2, "AppointmentSearchBehavior", "Appointment search behavior", Kind::Integer, "0", "OD: PrefName.AppointmentSearchBehavior"),
        field!(2, "AppointmentTimeIsLocked", "Appointment time is locked", Kind::Bool, "0", "OD: PrefName.AppointmentTimeIsLocked"),
        field!(2, "AppointmentClinicTimeReset", "Reset time when clinic changes", Kind::Bool, "0", "OD: PrefName.AppointmentClinicTimeReset"),
        field!(2, "ApptModuleAdjustmentsInProd", "Count adjustments in production", Kind::Bool, "0", "OD: PrefName.ApptModuleAdjustmentsInProd"),
        field!(2, "ApptModuleProductionUsesOps", "Production uses operatories", Kind::Bool, "0", "OD: PrefName.ApptModuleProductionUsesOps"),
        field!(2, "ApptAllowFutureComplete", "Allow future completion", Kind::Bool, "0", "OD: PrefName.ApptAllowFutureComplete"),
        field!(2, "ApptAllowEmptyComplete", "Allow completion without procedures", Kind::Bool, "0", "OD: PrefName.ApptAllowEmptyComplete"),
        field!(2, "AppointmentWithoutProcsDefaultLength", "Default length without procedures", Kind::Text, "15", "OD: PrefName.AppointmentWithoutProcsDefaultLength"),
        field!(2, "UnscheduledListNoRecalls", "Hide recalls from unscheduled list", Kind::Bool, "0", "OD: PrefName.UnscheduledListNoRecalls"),
        field!(2, "ApptAutoRefreshRange", "Auto-refresh range", Kind::Text, "", "OD: PrefName.ApptAutoRefreshRange"),
        field!(2, "ApptPreventChangesToCompleted", "Prevent changes to completed appointments", Kind::Bool, "0", "OD: PrefName.ApptPreventChangesToCompleted"),
        field!(2, "ApptsAllowOverlap", "Allow overlapping appointments", Kind::Bool, "0", "OD: PrefName.ApptsAllowOverlap"),
        field!(2, "BrokenApptRequiredOnMove", "Require broken appointment on move", Kind::Bool, "0", "OD: PrefName.BrokenApptRequiredOnMove"),
        field!(5, "BalancesDontSubtractIns", "Balances do not subtract insurance", Kind::Bool, "0", "OD: PrefName.BalancesDontSubtractIns"),
        field!(5, "AccountShowPaymentNums", "Show payment numbers", Kind::Bool, "0", "OD: PrefName.AccountShowPaymentNums"),
        field!(5, "ShowAccountFamilyCommEntries", "Show family comm entries", Kind::Bool, "0", "OD: PrefName.ShowAccountFamilyCommEntries"),
        field!(5, "CommLogAutoSave", "Save commlog automatically", Kind::Bool, "0", "OD: PrefName.CommLogAutoSave"),
        field!(5, "InvoicePaymentsGridShowNetProd", "Invoice grid shows net production", Kind::Bool, "0", "OD: PrefName.InvoicePaymentsGridShowNetProd"),
        field!(5, "FutureTransDatesAllowed", "Allow future transaction dates", Kind::Bool, "0", "OD: PrefName.FutureTransDatesAllowed"),
        field!(5, "AllowFutureInsPayments", "Allow future insurance payments", Kind::Bool, "0", "OD: PrefName.AllowFutureInsPayments"),
        field!(5, "AccountAllowFutureDebits", "Allow future debits", Kind::Bool, "0", "OD: PrefName.AccountAllowFutureDebits"),
        field!(6, "TreatmentPlanNote", "Default treatment plan note", Kind::Text, "", "OD: PrefName.TreatmentPlanNote"),
        field!(6, "TreatPlanShowCompleted", "Show completed procedures", Kind::Bool, "0", "OD: PrefName.TreatPlanShowCompleted"),
        field!(6, "TreatPlanItemized", "Itemize treatment plan", Kind::Bool, "0", "OD: PrefName.TreatPlanItemized"),
        field!(6, "TreatPlanDiscountPercent", "Discount percent", Kind::Text, "0", "OD: PrefName.TreatPlanDiscountPercent"),
        field!(6, "TreatPlanSortByTooth", "Sort by tooth", Kind::Bool, "0", "OD: PrefName.TreatPlanSortByTooth"),
        field!(6, "TreatPlanPromptSave", "Prompt to save signed plan", Kind::Bool, "0", "OD: PrefName.TreatPlanPromptSave"),
        field!(7, "UseInternationalToothNumbers", "Tooth numbering", Kind::Choice(&TOOTH_NUMBERS), "0", "OD: PrefName.UseInternationalToothNumbers"),
        field!(7, "AutoResetTPEntryStatus", "Reset treatment plan entry status", Kind::Bool, "0", "OD: PrefName.AutoResetTPEntryStatus"),
        field!(7, "ChartNonPatientWarn", "Warn on non-patient chart", Kind::Bool, "0", "OD: PrefName.ChartNonPatientWarn"),
        field!(7, "MedicalFeeUsedForNewProcs", "Use medical fee for new procedures", Kind::Bool, "0", "OD: PrefName.MedicalFeeUsedForNewProcs"),
        field!(7, "UseProviderColorsInChart", "Provider colors in chart", Kind::Bool, "0", "OD: PrefName.UseProviderColorsInChart"),
        field!(7, "PerioSkipMissingTeeth", "Perio skips missing teeth", Kind::Bool, "0", "OD: PrefName.PerioSkipMissingTeeth"),
        field!(7, "PerioTreatImplantsAsNotMissing", "Treat implants as present", Kind::Bool, "0", "OD: PrefName.PerioTreatImplantsAsNotMissing"),
        field!(7, "DxIcdVersion", "Diagnosis code version", Kind::Choice(&ICD_VERSION), "9", "OD: PrefName.DxIcdVersion"),
        field!(7, "ICD9DefaultForNewProcs", "Default diagnosis code", Kind::Text, "", "OD: PrefName.ICD9DefaultForNewProcs"),
        field!(7, "MedDefaultStopDays", "Medication stop days", Kind::Integer, "0", "OD: PrefName.MedDefaultStopDays"),
        field!(7, "ScreeningsUseSheets", "Screenings use sheets", Kind::Bool, "0", "OD: PrefName.ScreeningsUseSheets"),
        field!(7, "ShowPlannedAppointmentPrompt", "Prompt for planned appointments", Kind::Bool, "0", "OD: PrefName.ShowPlannedAppointmentPrompt"),
        field!(8, "IsAlertRadiologyProcsEnabled", "Alert on radiology procedures", Kind::Bool, "0", "OD: PrefName.IsAlertRadiologyProcsEnabled"),
    ]
}

#[derive(Default)]
pub(super) struct PreferencesDialog {
    pub(super) open: bool,
    page: usize,
    search: String,
    edits: HashMap<&'static str, String>,
    original: HashMap<&'static str, String>,
}

impl PreferencesDialog {
    fn load(&mut self, store: &dyn Store) {
        let stored = store.list_prefs().unwrap_or_default();
        self.edits = fields()
            .into_iter()
            .map(|field| {
                let value = stored
                    .iter()
                    .find(|pref| pref.pref_name == field.name)
                    .map(|pref| pref.pref_value.clone())
                    .unwrap_or_else(|| field.default.to_string());
                (field.name, value)
            })
            .collect();
        self.original = self.edits.clone();
        self.open = true;
    }

    fn value(&self, field: &Field) -> String {
        self.edits
            .get(field.name)
            .cloned()
            .unwrap_or_else(|| field.default.to_string())
    }
}

pub(super) fn open(app: &mut OdApp) {
    if !app.features.enabled(FeatureKey::ShowPreferences) || !app.can(Permission::SETUP) {
        app.error = Some("Preferences requires the Setup permission and feature.".into());
        return;
    }
    app.preferences.load(app.store.as_ref());
}

fn validate(field: &Field, value: &str) -> Result<(), String> {
    match field.kind {
        Kind::Bool => {
            if value == "0" || value == "1" {
                Ok(())
            } else {
                Err(format!("{} must be 1 or 0", field.label))
            }
        }
        Kind::Integer => match value.parse::<i64>() {
            Ok(number) if number >= 0 => Ok(()),
            _ => Err(format!("{} must be a non-negative whole number", field.label)),
        },
        Kind::Choice(options) => {
            if options.iter().any(|(_, raw)| *raw == value) {
                Ok(())
            } else {
                Err(format!("{} has an invalid choice", field.label))
            }
        }
        Kind::Text => Ok(()),
    }
}

pub(super) fn window(app: &mut OdApp, ctx: &egui::Context) {
    if !app.preferences.open {
        return;
    }
    if !app.can(Permission::SETUP) {
        app.preferences.open = false;
        return;
    }

    let mut save = false;
    let mut cancel = false;
    let mut local_error: Option<String> = None;
    let fields = fields();
    let query = app.preferences.search.trim().to_lowercase();
    let visible_pages: Vec<usize> = (0..PAGES.len())
        .filter(|index| {
            query.is_empty()
                || PAGES[*index].to_lowercase().contains(&query)
                || fields
                    .iter()
                    .any(|f| f.page == *index && f.label.to_lowercase().contains(&query))
        })
        .collect();
    if !visible_pages.contains(&app.preferences.page) {
        app.preferences.page = visible_pages.first().copied().unwrap_or(0);
    }

    let mut is_open = app.preferences.open;
    theme::AppWindow::new("Preferences")
        .open(&mut is_open)
        .resizable(true)
        .default_width(760.0)
        .default_height(560.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Search");
                ui.add(
                    egui::TextEdit::singleline(&mut app.preferences.search)
                        .desired_width(240.0)
                        .hint_text("preference or page"),
                );
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::Vec2::new(190.0, 400.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("pref_pages")
                            .min_scrolled_height(360.0)
                            .show(ui, |ui| {
                                ui.strong("Categories");
                                for index in &visible_pages {
                                    if ui
                                        .selectable_value(
                                            &mut app.preferences.page,
                                            *index,
                                            PAGES[*index],
                                        )
                                        .clicked()
                                    {
                                        app.preferences.search.clear();
                                    }
                                }
                            });
                    },
                );
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("pref_fields")
                    .min_scrolled_height(360.0)
                    .show(ui, |ui| {
                        ui.set_min_width(420.0);
                        ui.strong(PAGES[app.preferences.page]);
                        let page_fields: Vec<&Field> = fields
                            .iter()
                            .filter(|field| field.page == app.preferences.page)
                            .filter(|field| {
                                query.is_empty()
                                    || field.label.to_lowercase().contains(&query)
                                    || field.name.to_lowercase().contains(&query)
                            })
                            .collect();
                        if page_fields.is_empty() {
                            ui.add_space(12.0);
                            ui.weak("No editable preference rows for this page in the port.");
                        }
                        egui::Grid::new("pref_fields_grid")
                            .num_columns(2)
                            .spacing([16.0, 6.0])
                            .striped(true)
                            .show(ui, |ui| {
                                for field in page_fields {
                                    let mut value = app.preferences.value(field);
                                    let changed = match field.kind {
                                        Kind::Bool => {
                                            let mut checked = value == "1";
                                            let response = ui.checkbox(&mut checked, field.label);
                                            value = if checked { "1" } else { "0" }.to_string();
                                            response.changed()
                                        }
                                        Kind::Choice(options) => {
                                            let selected = options
                                                .iter()
                                                .find(|(_, raw)| *raw == value)
                                                .map(|(label, _)| *label)
                                                .unwrap_or(options[0].0);
                                            let mut index = options
                                                .iter()
                                                .position(|(label, _)| *label == selected)
                                                .unwrap_or(0);
                                            let response = egui::ComboBox::from_id_salt(field.name)
                                                .selected_text(selected)
                                                .show_ui(ui, |ui| {
                                                    for (i, (label, _)) in options.iter().enumerate() {
                                                        ui.selectable_value(&mut index, i, *label);
                                                    }
                                                });
                                            if response.inner.is_some() {
                                                value = options[index].1.to_string();
                                            }
                                            response.inner.is_some()
                                                && options[index].1 != app.preferences.value(field)
                                        }
                                        Kind::Integer | Kind::Text => {
                                            let response = ui.text_edit_singleline(&mut value);
                                            response.changed()
                                        }
                                    };
                                    ui.label(field.name).on_hover_text(field.help);
                                    ui.end_row();
                                    if changed {
                                        if let Err(err) = validate(field, &value) {
                                            local_error = Some(err);
                                        } else {
                                            app.preferences.edits.insert(field.name, value);
                                        }
                                    }
                                }
                            });
                    });
            });
            ui.separator();
            if !app.can(Permission::SETUP) {
                ui.colored_label(theme::danger(), "Setup permission is required to save.");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                save = ui.button("Save").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
    app.preferences.open = is_open;

    if let Some(err) = local_error {
        app.error = Some(err);
        return;
    }
    if cancel {
        app.preferences.edits = app.preferences.original.clone();
        app.preferences.open = false;
    }
    if save {
        let mut errors = Vec::new();
        let mut count = 0;
        for field in fields {
            let value = app.preferences.value(&field);
            let original = app
                .preferences
                .original
                .get(field.name)
                .cloned()
                .unwrap_or_else(|| field.default.to_string());
            if value == original {
                continue;
            }
            if let Err(err) = validate(&field, &value) {
                errors.push(err);
                continue;
            }
            match app.store.upsert_pref(field.name, &value) {
                Ok(()) => count += 1,
                Err(err) => errors.push(format!("{}: {err}", field.name)),
            }
        }
        if errors.is_empty() {
            app.notice = Some(format!("Preferences saved ({count} rows changed)"));
            // OD logs Preferences with the Setup permission.
            app.audit(Permission::SETUP, 0, "Preferences saved".into());
            app.preferences.original = app.preferences.edits.clone();
            app.preferences.open = false;
        } else {
            app.error = Some(errors.join("; "));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preference_fields_are_unique_with_valid_defaults() {
        let fields = fields();
        let mut names = std::collections::HashSet::new();
        for field in &fields {
            assert!(names.insert(field.name), "duplicate preference {}", field.name);
            assert!(validate(field, field.default).is_ok(), "{}", field.name);
        }
    }

    #[test]
    fn preference_values_use_od_encoding() {
        let fields = fields();
        let bool_field = fields
            .iter()
            .find(|f| f.name == "CommLogAutoSave")
            .expect("boolean preference missing");
        assert!(validate(bool_field, "1").is_ok());
        assert!(validate(bool_field, "0").is_ok());
        assert!(validate(bool_field, "true").is_err());

        let choice = fields
            .iter()
            .find(|f| f.name == "ShowIDinTitleBar")
            .expect("choice preference missing");
        assert!(validate(choice, "2").is_ok());
        assert!(validate(choice, "9").is_err());

        let integer = fields
            .iter()
            .find(|f| f.name == "MedDefaultStopDays")
            .expect("integer preference missing");
        assert!(validate(integer, "30").is_ok());
        assert!(validate(integer, "-1").is_err());
    }
}
