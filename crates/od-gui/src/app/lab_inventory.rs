//! Lab cases (Chart) and supplies (Setup).
//!
//! Both pages read Open Dental's own optional tables. When those tables are
//! absent the lists stay empty; writes still go through the shared OD-write
//! policy so a strict plug-and-play connection is never modified by accident.
//! No procedure charge or payment is created by a lab-stage action, and OD's
//! deprecated `LevelOnHand` is only displayed, never recomputed.

use super::*;
use chrono::{NaiveDate, NaiveDateTime};

fn lab_name(labs: &[Laboratory], num: Id) -> String {
    labs.iter()
        .find(|lab| lab.laboratory_num == num)
        .map(|lab| lab.description.clone())
        .unwrap_or_else(|| {
            if num == 0 {
                "—".to_string()
            } else {
                format!("Lab #{num}")
            }
        })
}

fn supplier_name(suppliers: &[Supplier], num: Id) -> String {
    suppliers
        .iter()
        .find(|supplier| supplier.supplier_num == num)
        .map(|supplier| supplier.name.clone())
        .unwrap_or_else(|| {
            if num == 0 {
                "—".to_string()
            } else {
                format!("Supplier #{num}")
            }
        })
}

fn parse_optional_datetime(text: &str) -> Result<Option<NaiveDateTime>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M")
        .map(Some)
        .map_err(|_| format!("Expected date/time 'YYYY-MM-DD HH:MM', got '{trimmed}'"))
}

fn format_optional_datetime(value: Option<NaiveDateTime>) -> String {
    value
        .map(|date| date.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

impl OdApp {
    fn reload_lab_cases(&mut self, pat_num: Id) {
        match self.store.list_lab_cases(pat_num) {
            Ok(rows) => self.lab_cases = rows,
            Err(err) => self.error = Some(err),
        }
    }

    fn reload_supply_dispenses(&mut self, supply_num: Id) {
        self.inventory_supply_num = supply_num;
        if supply_num == 0 {
            self.supply_dispenses.clear();
            return;
        }
        match self.store.list_supply_dispenses(supply_num) {
            Ok(rows) => self.supply_dispenses = rows,
            Err(err) => self.error = Some(err),
        }
    }

    /// Chart → lab cases. Every row is a single patient's case; the stage is
    /// derived from OD's dated columns and advanced by filling one of them.
    pub(super) fn lab_cases_ui(&mut self, ui: &mut Ui, pat_num: Id) {
        if !self.features.enabled(FeatureKey::LabCasesEnabled) || !self.can_module(Module::Chart) {
            return;
        }
        ui.heading("Lab cases");
        ui.horizontal(|ui| {
            if ui.button("New lab case").clicked() {
                self.lab_case_edit = Some(LabCase {
                    pat_num,
                    ..Default::default()
                });
                self.lab_case_lab_num = self.laboratories.first().map(|l| l.laboratory_num).unwrap_or(0);
                self.lab_case_apt_num = 0;
                self.lab_case_prov_num = 0;
                self.lab_case_due_text.clear();
                self.lab_case_created_text.clear();
                self.lab_case_sent_text.clear();
                self.lab_case_recd_text.clear();
                self.lab_case_checked_text.clear();
                self.lab_case_fee_text.clear();
                self.lab_case_instructions.clear();
                self.lab_case_edit_open = true;
                self.error = None;
            }
            ui.label("Stages are derived from the dated lab-slip columns; no charge or payment is posted.");
        });

        if self.lab_cases.is_empty() {
            ui.label("No lab cases for this patient.");
            return;
        }
        let labs = self.laboratories.clone();
        let mut advance: Option<(Id, &'static str)> = None;
        let mut edit_num = None;
        egui::Grid::new("lab_case_grid")
            .num_columns(8)
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Case");
                ui.strong("Lab");
                ui.strong("Due");
                ui.strong("Stage");
                ui.strong("Appointment");
                ui.strong("Fee");
                ui.strong("Instructions");
                ui.strong("Actions");
                ui.end_row();
                for case in &self.lab_cases {
                    ui.label(case.lab_case_num.to_string());
                    ui.label(lab_name(&labs, case.laboratory_num));
                    ui.label(format_optional_datetime(case.date_time_due));
                    ui.label(case.stage().label());
                    ui.label(if case.apt_num == 0 {
                        "—".to_string()
                    } else {
                        format!("Apt #{}", case.apt_num)
                    });
                    ui.label(format!("{:.2}", case.lab_fee));
                    ui.label(&case.instructions);
                    ui.horizontal(|ui| {
                        if ui.small_button("Edit").clicked() {
                            edit_num = Some(case.lab_case_num);
                        }
                        let stage = case.stage();
                        let next = match stage {
                            od_domain::LabCaseStage::Created => Some(("Mark sent", "date_time_sent")),
                            od_domain::LabCaseStage::Sent => Some(("Mark received", "date_time_recd")),
                            od_domain::LabCaseStage::Received => {
                                Some(("Mark checked", "date_time_checked"))
                            }
                            od_domain::LabCaseStage::Checked => None,
                        };
                        if let Some((label, column)) = next {
                            if ui.small_button(label).clicked() {
                                advance = Some((case.lab_case_num, column));
                            }
                        }
                    });
                    ui.end_row();
                }
            });

        if let Some(num) = edit_num {
            if let Some(case) = self.lab_cases.iter().find(|c| c.lab_case_num == num).cloned() {
                self.lab_case_edit = Some(case.clone());
                self.lab_case_lab_num = case.laboratory_num;
                self.lab_case_apt_num = case.apt_num;
                self.lab_case_prov_num = case.prov_num;
                self.lab_case_due_text = format_optional_datetime(case.date_time_due);
                self.lab_case_created_text = format_optional_datetime(case.date_time_created);
                self.lab_case_sent_text = format_optional_datetime(case.date_time_sent);
                self.lab_case_recd_text = format_optional_datetime(case.date_time_recd);
                self.lab_case_checked_text = format_optional_datetime(case.date_time_checked);
                self.lab_case_fee_text = format!("{:.2}", case.lab_fee);
                self.lab_case_instructions = case.instructions.clone();
                self.lab_case_edit_open = true;
                self.error = None;
            }
        }
        if let Some((num, column)) = advance {
            if let Some(case) = self.lab_cases.iter().find(|c| c.lab_case_num == num).cloned() {
                let mut updated = case.clone();
                let now = Local::now().naive_local();
                match column {
                    "date_time_sent" => updated.date_time_sent = Some(now),
                    "date_time_recd" => updated.date_time_recd = Some(now),
                    "date_time_checked" => updated.date_time_checked = Some(now),
                    _ => unreachable!("stage buttons only emit dated columns"),
                }
                match self.store.upsert_lab_case(&updated) {
                    Ok(_) => {
                        self.audit_if(
                            FeatureKey::AuditBillingChanges,
                            Permission::CHART_MODULE,
                            updated.pat_num,
                            format!("Lab case #{} advanced to {}", updated.lab_case_num, updated.stage().label()),
                        );
                        self.reload_lab_cases(updated.pat_num);
                        self.notice = Some(format!(
                            "Lab case #{} is now {}",
                            updated.lab_case_num,
                            updated.stage().label()
                        ));
                        self.error = None;
                    }
                    Err(err) => self.error = Some(err),
                }
            }
        }
    }

    pub(super) fn lab_case_window(&mut self, ctx: &egui::Context) {
        if !self.lab_case_edit_open {
            return;
        }
        let Some(mut case) = self.lab_case_edit.take() else {
            self.lab_case_edit_open = false;
            return;
        };
        let pat_num = case.pat_num;
        let mut save = false;
        let mut cancel = false;
        let mut parse_error: Option<String> = None;
        egui::Window::new(if case.lab_case_num == 0 {
            "New lab case"
        } else {
            "Edit lab case"
        })
        .open(&mut self.lab_case_edit_open)
        .resizable(true)
        .default_width(520.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Lab");
                egui::ComboBox::from_id_salt("lab_case_lab")
                    .selected_text(lab_name(&self.laboratories, self.lab_case_lab_num))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.lab_case_lab_num, 0, "—");
                        for lab in &self.laboratories {
                            ui.selectable_value(
                                &mut self.lab_case_lab_num,
                                lab.laboratory_num,
                                lab.description.clone(),
                            );
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label("Appointment #");
                ui.add(
                    egui::DragValue::new(&mut self.lab_case_apt_num)
                        .speed(1.0)
                        .range(0..=u32::MAX),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Provider #");
                ui.add(
                    egui::DragValue::new(&mut self.lab_case_prov_num)
                        .speed(1.0)
                        .range(0..=u32::MAX),
                );
            });
            for (label, text) in [
                ("Due", &mut self.lab_case_due_text),
                ("Created", &mut self.lab_case_created_text),
                ("Sent", &mut self.lab_case_sent_text),
                ("Received", &mut self.lab_case_recd_text),
                ("Checked", &mut self.lab_case_checked_text),
            ] {
                ui.horizontal(|ui| {
                    ui.label(format!("{label:<9}"));
                    ui.add(egui::TextEdit::singleline(text).desired_width(180.0).hint_text("YYYY-MM-DD HH:MM"));
                });
            }
            ui.horizontal(|ui| {
                ui.label("Lab fee");
                ui.add(egui::TextEdit::singleline(&mut self.lab_case_fee_text).desired_width(100.0));
            });
            ui.label("Instructions");
            ui.add(egui::TextEdit::multiline(&mut self.lab_case_instructions).desired_rows(3));
            ui.label("Writes one OD labcase row. Stage dates are independent of billing.");
            ui.separator();
            ui.horizontal(|ui| {
                save = ui.button("Save").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });

        if save {
            case.laboratory_num = self.lab_case_lab_num;
            case.apt_num = self.lab_case_apt_num;
            case.prov_num = self.lab_case_prov_num;
            case.lab_fee = self.lab_case_fee_text.parse().unwrap_or(0.0);
            case.instructions = self.lab_case_instructions.trim().to_string();
            let dates = [
                ("Due", self.lab_case_due_text.clone(), parse_optional_datetime(&self.lab_case_due_text)),
                ("Created", self.lab_case_created_text.clone(), parse_optional_datetime(&self.lab_case_created_text)),
                ("Sent", self.lab_case_sent_text.clone(), parse_optional_datetime(&self.lab_case_sent_text)),
                ("Received", self.lab_case_recd_text.clone(), parse_optional_datetime(&self.lab_case_recd_text)),
                ("Checked", self.lab_case_checked_text.clone(), parse_optional_datetime(&self.lab_case_checked_text)),
            ];
            for (label, _, result) in &dates {
                if let Err(err) = result {
                    parse_error = Some(format!("{label}: {err}"));
                }
            }
            if parse_error.is_none() {
                case.date_time_due = dates[0].2.clone().unwrap_or(None);
                case.date_time_created = dates[1].2.clone().unwrap_or(None);
                case.date_time_sent = dates[2].2.clone().unwrap_or(None);
                case.date_time_recd = dates[3].2.clone().unwrap_or(None);
                case.date_time_checked = dates[4].2.clone().unwrap_or(None);
                match self.store.upsert_lab_case(&case) {
                    Ok(num) => {
                        self.audit_if(
                            FeatureKey::AuditBillingChanges,
                            Permission::CHART_MODULE,
                            pat_num,
                            format!("Lab case #{num} saved for patient #{pat_num} at {}", case.stage().label()),
                        );
                        self.reload_lab_cases(pat_num);
                        self.notice = Some(format!("Lab case #{num} saved"));
                        self.error = None;
                    }
                    Err(err) => self.error = Some(err),
                }
            }
            if let Some(err) = parse_error {
                self.error = Some(err);
                self.lab_case_edit = Some(case);
                self.lab_case_edit_open = true;
                return;
            }
        } else if !cancel {
            self.lab_case_edit = Some(case);
        }
        if cancel {
            self.lab_case_edit = None;
        }
    }

    /// Setup → inventory. Supply rows are OD-owned; dispensing appends a
    /// `dispsupply` movement only and leaves `LevelOnHand` untouched.
    pub(super) fn inventory_ui(&mut self, ui: &mut Ui) {
        if !self.features.enabled(FeatureKey::InventoryEnabled) || !self.can_module(Module::Setup) {
            return;
        }
        ui.collapsing("Inventory (OD supplies)", |ui| {
            if self.supplies.is_empty() {
                ui.label("No supply items found.");
                return;
            }
            let suppliers = self.suppliers.clone();
            let mut dispense_num = None;
            egui::Grid::new("supply_grid")
                .num_columns(7)
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("#");
                    ui.strong("Description");
                    ui.strong("Supplier");
                    ui.strong("Catalog");
                    ui.strong("Price");
                    ui.strong("On hand (stored)");
                    ui.strong("Action");
                    ui.end_row();
                    for supply in &self.supplies {
                        ui.label(supply.supply_num.to_string());
                        ui.label(&supply.description);
                        ui.label(supplier_name(&suppliers, supply.supplier_num));
                        ui.label(&supply.catalog_number);
                        ui.label(format!("{:.2}", supply.price));
                        ui.label(format!("{:.2}", supply.level_on_hand));
                        if ui.small_button("Dispense...").clicked() {
                            dispense_num = Some(supply.supply_num);
                        }
                        ui.end_row();
                    }
                });
            if let Some(num) = dispense_num {
                self.reload_supply_dispenses(num);
                self.supply_dispense_supply_num = num;
                self.supply_dispense_prov_num = 0;
                self.supply_dispense_date_text = Local::now().format("%Y-%m-%d").to_string();
                self.supply_dispense_quantity_text = "1".into();
                self.supply_dispense_note.clear();
                self.supply_dispense_open = true;
                self.error = None;
            }
            if self.inventory_supply_num != 0 {
                ui.separator();
                ui.label(format!(
                    "Recent movements for supply #{} (level on hand is not recalculated):",
                    self.inventory_supply_num
                ));
                egui::Grid::new("supply_dispense_grid")
                    .num_columns(5)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Date");
                        ui.strong("Provider");
                        ui.strong("Quantity");
                        ui.strong("Note");
                        ui.strong("#");
                        ui.end_row();
                        for row in &self.supply_dispenses {
                            ui.label(row.date_dispensed.to_string());
                            ui.label(if row.prov_num == 0 {
                                "—".to_string()
                            } else {
                                format!("#{}", row.prov_num)
                            });
                            ui.label(format!("{:.2}", row.disp_quantity));
                            ui.label(&row.note);
                            ui.label(row.disp_supply_num.to_string());
                            ui.end_row();
                        }
                    });
            }
        });
    }

    pub(super) fn supply_dispense_window(&mut self, ctx: &egui::Context) {
        if !self.supply_dispense_open {
            return;
        }
        let supply_num = self.supply_dispense_supply_num;
        let mut save = false;
        let mut cancel = false;
        let supply_label = self
            .supplies
            .iter()
            .find(|s| s.supply_num == supply_num)
            .map(|s| s.description.clone())
            .unwrap_or_else(|| format!("Supply #{supply_num}"));
        egui::Window::new(format!("Dispense — {supply_label}"))
            .open(&mut self.supply_dispense_open)
            .resizable(true)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Provider #");
                    ui.add(
                        egui::DragValue::new(&mut self.supply_dispense_prov_num)
                            .speed(1.0)
                            .range(0..=u32::MAX),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Date");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.supply_dispense_date_text)
                            .desired_width(120.0),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Quantity");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.supply_dispense_quantity_text)
                            .desired_width(100.0),
                    );
                });
                ui.label("Note");
                ui.add(egui::TextEdit::multiline(&mut self.supply_dispense_note).desired_rows(2));
                ui.label("Appends one OD dispsupply row. OD's LevelOnHand is shown as stored and is never recalculated.");
                ui.separator();
                ui.horizontal(|ui| {
                    save = ui.button("Record dispense").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if save {
            let date = match NaiveDate::parse_from_str(&self.supply_dispense_date_text, "%Y-%m-%d") {
                Ok(date) => date,
                Err(_) => {
                    self.error = Some("Enter the dispense date as YYYY-MM-DD.".into());
                    return;
                }
            };
            let quantity = self.supply_dispense_quantity_text.parse::<f64>().unwrap_or(0.0);
            if quantity <= 0.0 {
                self.error = Some("Dispense quantity must be greater than zero.".into());
                return;
            }
            let dispense = SupplyDispense {
                supply_num,
                prov_num: self.supply_dispense_prov_num,
                date_dispensed: date,
                disp_quantity: quantity,
                note: self.supply_dispense_note.trim().to_string(),
                ..Default::default()
            };
            match self.store.record_supply_dispense(&dispense) {
                Ok(num) => {
                    self.audit_if(
                        FeatureKey::AuditBillingChanges,
                        Permission::SETUP,
                        0,
                        format!(
                            "Dispense #{num}: supply #{supply_num}, {quantity:.2}, provider #{}, note {}",
                            self.supply_dispense_prov_num, dispense.note
                        ),
                    );
                    self.reload_supply_dispenses(supply_num);
                    self.notice = Some(format!("Dispense #{num} recorded"));
                    self.error = None;
                    self.supply_dispense_open = false;
                }
                Err(err) => self.error = Some(err),
            }
        } else if cancel {
            self.supply_dispense_open = false;
        }
    }
}
