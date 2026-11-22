//! Setup UI for OD definitions and display fields.
//!
//! The two grids mirror `FormDefinitions` and `FormDisplayFields`: a category
//! selector, ordered rows, edit/add/hide/reorder buttons and a default-list
//! action. Every write goes through the OD tables and is optionally audited to
//! `securitylog`.

use super::*;

/// The two Setup pages handled by this module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DefinitionsTab {
    Definitions,
    DisplayFields,
}

impl DefinitionsTab {
    const ALL: [Self; 2] = [Self::Definitions, Self::DisplayFields];

    fn label(self) -> &'static str {
        match self {
            Self::Definitions => "Definitions",
            Self::DisplayFields => "Display fields",
        }
    }
}

impl OdApp {
    pub(super) fn definitions_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("Definitions & display fields");
            ui.separator();
            for tab in DefinitionsTab::ALL {
                ui.selectable_value(&mut self.definitions_tab, tab, tab.label());
            }
        });
        ui.weak("OD Setup → Definitions uses permission DefEdit (220); Display Fields uses Setup (8). Hidden definitions stay in the table and keep their references.");

        match self.definitions_tab {
            DefinitionsTab::Definitions => self.definitions_grid(ui),
            DefinitionsTab::DisplayFields => self.display_fields_grid(ui),
        }

        self.definition_editor(ui.ctx().clone());
        self.display_field_editor(ui.ctx().clone());
    }

    // ------------------------------------------------------------- Definitions

    fn definitions_grid(&mut self, ui: &mut Ui) {
        if !self.features.enabled(FeatureKey::ShowDefinitions) {
            ui.colored_label(
                Color32::from_rgb(190, 120, 20),
                "Definitions are OFF. Enable 'Definitions in Setup' in Settings to edit them.",
            );
            return;
        }
        if !self.can(Permission::DEF_EDIT) {
            ui.colored_label(
                Color32::from_rgb(190, 120, 20),
                "Your Open Dental group does not have Definition Edit (220).",
            );
            return;
        }

        let category = self.definition_category;
        let mut categories = DefinitionCategory::EDITABLE.to_vec();
        categories.sort_by_key(|cat| cat.label());
        egui::ComboBox::from_label("Definition category")
            .selected_text(category.label())
            .show_ui(ui, |ui| {
                for cat in categories {
                    ui.selectable_value(&mut self.definition_category, cat, cat.label());
                }
            });
        let category = self.definition_category;
        let options = category.options();
        ui.weak(options.help);
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut self.definitions_show_hidden,
                "Show hidden definitions",
            );
            if ui.button("+ Add definition").clicked() {
                let def = Definition {
                    category,
                    item_order: next_definition_order(&self.definitions, category),
                    ..Definition::default()
                };
                self.open_definition_editor(def);
            }
            if ui.button("Alphabetize").clicked() {
                let mut rows =
                    sorted_definitions(&self.definitions, Some(category), self.definitions_show_hidden);
                rows.sort_by(|a, b| {
                    a.item_name
                        .to_lowercase()
                        .cmp(&b.item_name.to_lowercase())
                        .then(a.def_num.cmp(&b.def_num))
                });
                self.persist_definition_rows(rows, format!("Alphabetized {} definitions", category.label()));
            }
        });

        let rows = sorted_definitions(
            &self.definitions,
            Some(category),
            self.definitions_show_hidden,
        );
        if rows.is_empty() {
            ui.weak("No definitions in this category yet.");
            return;
        }

        let mut edit_request: Option<Definition> = None;
        let mut hide_request: Option<(Definition, bool)> = None;
        let mut move_request: Option<(Id, isize)> = None;
        egui::Grid::new("definitions_grid")
            .num_columns(7)
            .striped(true)
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                ui.strong("Name");
                ui.strong(if options.value_label.is_empty() {
                    "Value"
                } else {
                    options.value_label
                });
                ui.strong("Colour");
                ui.strong("Hidden");
                ui.strong("Up");
                ui.strong("Down");
                ui.strong("Edit");
                ui.end_row();
                for (index, row) in rows.iter().enumerate() {
                    ui.label(&row.item_name);
                    ui.label(&row.item_value);
                    if options.enable_color && row.item_color != 0 {
                        ui.label(format!("ARGB {}", row.item_color));
                    } else {
                        ui.weak("-");
                    }
                    let mut hidden = row.is_hidden;
                    let hide = ui.checkbox(&mut hidden, "");
                    if hide.changed() && options.can_hide {
                        hide_request = Some((row.clone(), hidden));
                    }
                    let up = ui.button("↑");
                    let down = ui.button("↓");
                    if index > 0 && up.clicked() {
                        move_request = Some((row.def_num, -1));
                    }
                    if index + 1 < rows.len() && down.clicked() {
                        move_request = Some((row.def_num, 1));
                    }
                    if ui.button("Edit").clicked() {
                        edit_request = Some(row.clone());
                    }
                    ui.end_row();
                }
            });

        if let Some(def) = edit_request {
            self.open_definition_editor(def);
        }
        if let Some((mut def, hidden)) = hide_request {
            def.is_hidden = hidden;
            let verb = if hidden { "Hid" } else { "Unhid" };
            let label = def.item_name.clone();
            if let Err(err) = self.store.upsert_definition(&def) {
                self.error = Some(err);
            } else {
                self.audit_if(
                    FeatureKey::AuditDefinitionChanges,
                    Permission::DEF_EDIT,
                    0,
                    format!("{verb} definition {label} in {}", category.label()),
                );
                self.refresh();
            }
        }
        if let Some((def_num, delta)) = move_request {
            let mut changed = rows;
            if let Some(index) = changed.iter().position(|d| d.def_num == def_num) {
                let target = (index as isize + delta) as usize;
                changed.swap(index, target);
                self.persist_definition_rows(
                    changed,
                    format!("Reordered definitions in {}", category.label()),
                );
            }
        }
    }

    fn open_definition_editor(&mut self, definition: Definition) {
        self.definition_name_text = definition.item_name.clone();
        self.definition_value_text = definition.item_value.clone();
        self.definition_color_text = definition.item_color.to_string();
        self.definition_edit = Some(definition);
        self.definition_edit_open = true;
    }

    fn definition_editor(&mut self, ctx: egui::Context) {
        let Some(mut edit) = self.definition_edit.clone() else {
            return;
        };
        let options = edit.category.options();
        let is_new = edit.def_num == 0;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new(if is_new {
            "New definition"
        } else {
            "Edit definition"
        })
        .open(&mut self.definition_edit_open)
        .resizable(false)
        .show(&ctx, |ui| {
            ui.label(format!("Category: {}", edit.category.label()));
            ui.horizontal(|ui| {
                ui.label("Name   ");
                if options.can_edit_name {
                    ui.text_edit_singleline(&mut self.definition_name_text);
                } else {
                    ui.label(&edit.item_name);
                }
            });
            if options.enable_value {
                ui.horizontal(|ui| {
                    ui.label(if options.value_label.is_empty() {
                        "Value  "
                    } else {
                        options.value_label
                    });
                    ui.text_edit_singleline(&mut self.definition_value_text);
                });
            }
            if options.enable_color {
                ui.horizontal(|ui| {
                    ui.label("Colour ");
                    ui.text_edit_singleline(&mut self.definition_color_text);
                });
                ui.weak("OD ItemColor is a signed ARGB int. 0 means no colour.");
            }
            ui.checkbox(&mut edit.is_hidden, "Hidden (IsHidden)");
            if options.enable_value {
                ui.weak("Do not change an existing value unless you mean to affect every record using it.");
            }
            ui.separator();
            ui.horizontal(|ui| {
                save = ui.button("Save definition").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });

        if cancel {
            self.definition_edit_open = false;
            return;
        }
        if !save {
            return;
        }
        edit.item_name = self.definition_name_text.trim().to_string();
        edit.item_value = if options.enable_value {
            self.definition_value_text.trim().to_string()
        } else {
            edit.item_value
        };
        if options.enable_color {
            edit.item_color = self.definition_color_text.trim().parse::<i32>().unwrap_or(0);
        }
        if let Err(err) = validate_definition(&edit) {
            self.error = Some(err);
            self.definition_edit_open = true;
            return;
        }
        let verb = if is_new { "Created" } else { "Edited" };
        let label = edit.item_name.clone();
        if let Err(err) = self.store.upsert_definition(&edit) {
            self.error = Some(err);
            return;
        }
        self.audit_if(
            FeatureKey::AuditDefinitionChanges,
            Permission::DEF_EDIT,
            0,
            format!("{verb} definition {label} in {}", edit.category.label()),
        );
        self.definition_edit_open = false;
        self.notice = Some(format!("{verb} definition {label}"));
        self.error = None;
        self.refresh();
    }

    fn persist_definition_rows(&mut self, mut rows: Vec<Definition>, log_text: String) {
        let category = self.definition_category;
        for row in &mut rows {
            row.category = category;
        }
        if rows.is_empty() {
            return;
        }
        for row in &rows {
            if let Err(err) = validate_definition(row) {
                self.error = Some(err);
                return;
            }
        }
        for row in &rows {
            if let Err(err) = self.store.upsert_definition(row) {
                self.error = Some(err);
                return;
            }
        }
        self.audit_if(
            FeatureKey::AuditDefinitionChanges,
            Permission::DEF_EDIT,
            0,
            log_text.clone(),
        );
        self.notice = Some(log_text);
        self.error = None;
        self.refresh();
    }

    // ---------------------------------------------------------- Display fields

    fn effective_display_rows(&self, category: DisplayFieldCategory) -> Vec<DisplayField> {
        let stored = display_fields_for(&self.display_fields, category);
        if stored.is_empty() {
            default_display_fields(category)
        } else {
            stored
        }
    }

    fn display_fields_grid(&mut self, ui: &mut Ui) {
        if !self.features.enabled(FeatureKey::ShowDisplayFields) {
            ui.colored_label(
                Color32::from_rgb(190, 120, 20),
                "Display fields are OFF. Enable 'Display fields in Setup' in Settings to edit them.",
            );
            return;
        }
        if !self.can(Permission::SETUP) {
            ui.colored_label(
                Color32::from_rgb(190, 120, 20),
                "Your Open Dental group does not have Setup (8).",
            );
            return;
        }

        let mut categories: Vec<DisplayFieldCategory> = DisplayFieldCategory::ALL
            .iter()
            .map(|(category, _)| *category)
            .filter(|category| *category != DisplayFieldCategory::NONE)
            .collect();
        categories.sort_by_key(|category| category.label());
        egui::ComboBox::from_label("Display field category")
            .selected_text(self.display_field_category.label())
            .show_ui(ui, |ui| {
                for category in categories {
                    ui.selectable_value(&mut self.display_field_category, category, category.label());
                }
            });
        let category = self.display_field_category;
        if category == DisplayFieldCategory::ORTHO_CHART {
            ui.weak("OD uses a separate Ortho Chart editor; custom columns here are still stored in the same displayfield table.");
        }
        ui.weak("Descriptions rename a column. Widths are pixels; OD stores them in ColumnWidth. Saving the exact default list leaves no rows so OD falls back to its built-in defaults.");

        ui.horizontal(|ui| {
            if ui.button("+ Add column").clicked() {
                let field = DisplayField {
                    category,
                    column_width: 80,
                    ..DisplayField::default()
                };
                self.open_display_field_editor(field);
            }
            let defaults = ui.button("Default columns");
            if defaults.clicked() {
                let rows = default_display_fields(category);
                self.persist_display_rows(rows, format!("Restored default columns for {}", category.label()));
            }
            let save = ui.button("Save columns");
            if save.clicked() {
                let rows = self.effective_display_rows(category);
                self.persist_display_rows(rows, format!("Saved display fields for {}", category.label()));
            }
        });

        let rows = self.effective_display_rows(category);
        if rows.is_empty() {
            ui.weak("No columns configured. Add one, or load the default list.");
            return;
        }

        let mut edit_request: Option<DisplayField> = None;
        let mut remove_request: Option<String> = None;
        let mut move_request: Option<(String, isize)> = None;
        egui::Grid::new("display_fields_grid")
            .num_columns(5)
            .striped(true)
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                ui.strong("Field name");
                ui.strong("New description");
                ui.strong("Width");
                ui.strong("Up / Down");
                ui.strong("Edit / Remove");
                ui.end_row();
                for (index, field) in rows.iter().enumerate() {
                    ui.label(&field.internal_name);
                    ui.label(field.display_label());
                    ui.label(field.column_width.to_string());
                    ui.horizontal(|ui| {
                        if index > 0 && ui.button("↑").clicked() {
                            move_request = Some((field.internal_name.clone(), -1));
                        }
                        if index + 1 < rows.len() && ui.button("↓").clicked() {
                            move_request = Some((field.internal_name.clone(), 1));
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Edit").clicked() {
                            edit_request = Some(field.clone());
                        }
                        if ui.button("Remove").clicked() {
                            remove_request = Some(field.internal_name.clone());
                        }
                    });
                    ui.end_row();
                }
            });

        if let Some(field) = edit_request {
            self.open_display_field_editor(field);
        }
        if let Some(name) = remove_request {
            let mut changed = rows.clone();
            changed.retain(|field| field.internal_name != name);
            self.persist_display_rows(
                changed,
                format!("Removed column {name} from {}", category.label()),
            );
        }
        if let Some((name, delta)) = move_request {
            let mut changed = rows;
            if let Some(index) = changed
                .iter()
                .position(|field| field.internal_name == name)
            {
                let target = (index as isize + delta) as usize;
                changed.swap(index, target);
                self.persist_display_rows(
                    changed,
                    format!("Reordered columns in {}", category.label()),
                );
            }
        }
    }

    fn open_display_field_editor(&mut self, field: DisplayField) {
        self.display_field_name_text = field.internal_name.clone();
        self.display_field_label_text = field.description.clone();
        self.display_field_width_text = field.column_width.to_string();
        self.display_field_edit = Some(field);
        self.display_field_edit_open = true;
    }

    fn display_field_editor(&mut self, ctx: egui::Context) {
        let Some(mut edit) = self.display_field_edit.clone() else {
            return;
        };
        let is_new = edit.display_field_num == 0;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new(if is_new {
            "New display field"
        } else {
            "Edit display field"
        })
        .open(&mut self.display_field_edit_open)
        .resizable(false)
        .show(&ctx, |ui| {
            ui.label(format!("Category: {}", edit.category.label()));
            ui.horizontal(|ui| {
                ui.label("Field name");
                if is_new {
                    ui.text_edit_singleline(&mut self.display_field_name_text);
                } else {
                    ui.label(&edit.internal_name);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Description");
                ui.text_edit_singleline(&mut self.display_field_label_text);
            });
            ui.horizontal(|ui| {
                ui.label("Width     ");
                ui.text_edit_singleline(&mut self.display_field_width_text);
            });
            ui.weak("Descriptions must be unique and must not match a field name (OD FormDisplayFields).");
            ui.separator();
            ui.horizontal(|ui| {
                save = ui.button("Save column").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });

        if cancel {
            self.display_field_edit_open = false;
            return;
        }
        if !save {
            return;
        }
        if is_new {
            edit.internal_name = self.display_field_name_text.trim().to_string();
            edit.item_order = self.effective_display_rows(edit.category).len() as i32;
        }
        edit.description = self.display_field_label_text.trim().to_string();
        edit.column_width = self
            .display_field_width_text
            .trim()
            .parse::<i32>()
            .unwrap_or(-1);
        let category = edit.category;
        let label = edit.description.clone();
        let mut rows = self.effective_display_rows(category);
        if is_new {
            rows.push(edit);
        } else if let Some(existing) = rows
            .iter_mut()
            .find(|field| field.internal_name == edit.internal_name)
        {
            *existing = edit;
        }
        let verb = if is_new { "Added" } else { "Edited" };
        self.persist_display_rows(
            rows,
            format!("{verb} display field {label} in {}", category.label()),
        );
        if !self.display_field_edit_open {
            return;
        }
        self.display_field_edit_open = false;
    }

    fn persist_display_rows(&mut self, mut rows: Vec<DisplayField>, log_text: String) {
        let category = self.display_field_category;
        for row in &mut rows {
            row.category = category;
            row.chart_view_num = 0;
        }
        compact_display_field_orders(&mut rows, category);
        if let Err(err) = validate_display_fields(&rows) {
            self.error = Some(err);
            self.display_field_edit_open = true;
            return;
        }
        if let Err(err) = self.store.replace_display_fields(category, &rows) {
            self.error = Some(err);
            return;
        }
        self.audit_if(
            FeatureKey::AuditDefinitionChanges,
            Permission::SETUP,
            0,
            log_text.clone(),
        );
        self.notice = Some(log_text);
        self.error = None;
        self.refresh();
    }
}
