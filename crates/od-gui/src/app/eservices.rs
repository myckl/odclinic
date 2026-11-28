//! eServices UI: local channels, reminder rules, outbox and manual send.
//!
//! Everything here is local-first. No button in this module contacts an Open
//! Dental HQ server, and no registration key is ever required.

use super::*;
use od_core::{deliver, delivery_preview, due_messages, manual_message, planned_note, ReminderContext};

/// The three tabs of the Setup → eServices page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EServicesTab {
    Channels,
    Rules,
    Outbox,
}

impl EServicesTab {
    const ALL: [Self; 3] = [Self::Channels, Self::Rules, Self::Outbox];

    fn label(self) -> &'static str {
        match self {
            Self::Channels => "Channels",
            Self::Rules => "Reminder rules",
            Self::Outbox => "Outbox",
        }
    }
}

fn readiness_color(readiness: TransportReadiness) -> Color32 {
    match readiness {
        TransportReadiness::Ready => Color32::from_rgb(60, 150, 80),
        TransportReadiness::NeedsConfig => Color32::from_rgb(190, 120, 20),
        TransportReadiness::Disabled => Color32::from_rgb(130, 130, 130),
        TransportReadiness::Planned => Color32::from_rgb(90, 130, 190),
    }
}

fn status_color(status: OutboxStatus) -> Color32 {
    match status {
        OutboxStatus::Sent => Color32::from_rgb(60, 150, 80),
        OutboxStatus::Failed => Color32::from_rgb(190, 60, 60),
        OutboxStatus::DryRun => Color32::from_rgb(120, 90, 190),
        OutboxStatus::Skipped => Color32::from_rgb(130, 130, 130),
        OutboxStatus::Pending => Color32::from_rgb(190, 120, 20),
    }
}

fn short_body(body: &str) -> String {
    let text = body.lines().next().unwrap_or("");
    let mut preview: String = text.chars().take(72).collect();
    if text.chars().count() > 72 {
        preview.push('…');
    }
    preview
}

impl OdApp {
    // ---------------------------------------------------------- Setup page

    pub(super) fn eservices_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("eServices");
            ui.separator();
            for tab in EServicesTab::ALL {
                ui.selectable_value(&mut self.eservices_tab, tab, tab.label());
            }
        });
        if !self.features.enabled(FeatureKey::EServicesEnabled) {
            ui.colored_label(
                Color32::from_rgb(190, 120, 20),
                "eServices is OFF. Enable 'eServices (local reminders)' in Settings to generate or deliver messages. You can still configure channels below.",
            );
        }
        ui.weak("Local reminders only — no Open Dental HQ server, registration key or eConnector is used.");
        ui.separator();

        match self.eservices_tab {
            EServicesTab::Channels => self.channels_ui(ui),
            EServicesTab::Rules => self.rules_ui(ui),
            EServicesTab::Outbox => self.outbox_ui(ui),
        }
    }

    fn channels_ui(&mut self, ui: &mut Ui) {
        let mut config = self.eservices_config.clone();
        let mut save = false;

        ui.label("Tick a channel to enable it. SMTP, Telegram and Africa's Talking can deliver in this build; the others keep their settings so they can be added later without a settings-file migration.");
        for kind in TransportKind::ALL {
            ui.horizontal(|ui| {
                let mut enabled = config.is_enabled(kind);
                if ui.checkbox(&mut enabled, kind.label()).changed() {
                    config.set_enabled(kind, enabled);
                }
                let readiness = config.readiness(kind);
                ui.colored_label(readiness_color(readiness), readiness.label());
            });
            if kind.implemented() {
                match kind {
                    TransportKind::Smtp => {
                        ui.collapsing("SMTP settings", |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Host   ");
                                ui.text_edit_singleline(&mut config.smtp_host);
                                ui.label("Port");
                                ui.add(egui::DragValue::new(&mut config.smtp_port));
                            });
                            ui.horizontal(|ui| {
                                ui.label("From   ");
                                ui.text_edit_singleline(&mut config.smtp_from);
                            });
                            ui.horizontal(|ui| {
                                ui.label("User   ");
                                ui.text_edit_singleline(&mut config.smtp_user);
                            });
                            ui.horizontal(|ui| {
                                ui.label("Password");
                                ui.add(egui::TextEdit::singleline(&mut config.smtp_password).password(true));
                            });
                            ui.checkbox(&mut config.smtp_use_tls, "STARTTLS (turn off for a local relay on port 25)");
                        });
                    }
                    TransportKind::Telegram => {
                        ui.collapsing("Telegram Bot API settings", |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Token  ");
                                ui.add(egui::TextEdit::singleline(&mut config.telegram_bot_token).password(true));
                            });
                            ui.horizontal(|ui| {
                                ui.label("Chat ID");
                                ui.text_edit_singleline(&mut config.telegram_chat_id);
                            });
                            ui.weak("Create a bot with @BotFather, then send it a message so the chat ID exists. A per-patient chat ID on the patient contact overrides this default.");
                        });
                    }
                    TransportKind::AfricasTalking => {
                        ui.collapsing("Africa's Talking settings", |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Username");
                                ui.text_edit_singleline(&mut config.africas_talking_username);
                            });
                            ui.horizontal(|ui| {
                                ui.label("API key ");
                                ui.add(egui::TextEdit::singleline(&mut config.africas_talking_key).password(true));
                            });
                            ui.weak("Use the username and API key from your Africa's Talking app. The key is hidden while editing and is never shown in the outbox.");
                            ui.weak("SMS reminders go to the patient's home phone; include the country code (for example +251...).");
                        });
                    }
                    _ => {}
                }
            } else {
                ui.indent(format!("planned-{kind:?}"), |ui| {
                    ui.weak(planned_note(kind));
                });
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            save = ui.button("Save channel settings").clicked();
            ui.weak(format!(
                "Stored in {} (APPDATA\\odclinic\\eservices.txt)",
                "eservices.txt"
            ));
        });

        self.eservices_config = config;
        if save {
            match self.eservices_config.save() {
                Ok(()) => {
                    self.notice = Some("eServices channel settings saved".into());
                    self.error = None;
                }
                Err(err) => self.error = Some(err),
            }
        }
    }

    // ---------------------------------------------------------- Rule list

    fn rules_ui(&mut self, ui: &mut Ui) {
        let dry = self.features.enabled(FeatureKey::ReminderDryRun);
        let master = self.features.enabled(FeatureKey::EServicesEnabled);
        let any_type = self.features.enabled(FeatureKey::RemindersEnabled)
            || self.features.enabled(FeatureKey::ConfirmationsEnabled);

        ui.horizontal(|ui| {
            if ui.button("+ New rule").clicked() {
                self.open_rule_editor(ReminderRule::default());
            }
            let generate = ui.button("Generate reminders");
            if generate.clicked() {
                self.generate_reminders();
            }
            generate.on_hover_text("Evaluate enabled rules against today's appointments and write matching messages to the outbox.");
            if ui.button("Refresh").clicked() {
                self.refresh();
            }
        });
        if dry {
            ui.colored_label(
                Color32::from_rgb(120, 90, 190),
                "Dry-run mode is ON — generated rows are rendered but never delivered.",
            );
        } else if !master || !any_type {
            ui.colored_label(
                Color32::from_rgb(190, 120, 20),
                "Generation is blocked: turn on eServices and at least one reminder type in Settings.",
            );
        }
        ui.weak("Rules mirror OD apptreminderrule. Lead time and 'do not send within' are shown in minutes (OD stores 100 ns ticks).");
        ui.separator();

        if self.reminder_rules.is_empty() {
            ui.label("No reminder rules yet. Add one to start generating messages.");
            return;
        }

        let mut edit_request: Option<ReminderRule> = None;
        egui::Grid::new("eservices_rule_grid")
            .num_columns(7)
            .striped(true)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.strong("Type");
                ui.strong("Lead (min)");
                ui.strong("Min notice");
                ui.strong("Channel");
                ui.strong("Enabled");
                ui.strong("Templates");
                ui.strong("Actions");
                ui.end_row();
                for rule in &self.reminder_rules {
                    ui.label(rule.type_cur.label());
                    ui.label(rule.minutes_prior.to_string());
                    ui.label(rule.do_not_send_within_minutes.to_string());
                    ui.label(rule.comm_type.label());
                    ui.label(if rule.is_enabled { "Yes" } else { "No" });
                    let has_template = !rule.sms_template.trim().is_empty()
                        || !rule.email_body.trim().is_empty();
                    ui.label(if has_template { "Custom" } else { "Default" });
                    if ui.button("Edit").clicked() {
                        edit_request = Some(rule.clone());
                    }
                    ui.end_row();
                }
            });
        if let Some(rule) = edit_request {
            self.open_rule_editor(rule);
        }
    }

    fn open_rule_editor(&mut self, rule: ReminderRule) {
        self.rule_minutes_text = rule.minutes_prior.to_string();
        self.rule_dnd_text = rule.do_not_send_within_minutes.to_string();
        self.rule_sms_template = rule.sms_template.clone();
        self.rule_email_subject = rule.email_subject.clone();
        self.rule_email_body = rule.email_body.clone();
        self.rule_comm_type = rule.comm_type;
        self.rule_type = rule.type_cur;
        self.rule_edit = Some(rule);
        self.rule_edit_open = true;
    }

    fn generate_reminders(&mut self) {
        if !self.features.enabled(FeatureKey::EServicesEnabled) {
            self.error = Some("eServices is turned off in Settings".into());
            return;
        }
        let mut allow = Vec::new();
        if self.features.enabled(FeatureKey::RemindersEnabled) {
            allow.push(ReminderType::Reminder);
            allow.push(ReminderType::Arrival);
        }
        if self.features.enabled(FeatureKey::ConfirmationsEnabled) {
            allow.push(ReminderType::ConfirmationFutureDay);
        }
        if self.features.enabled(FeatureKey::PostTreatmentRemindersEnabled) {
            allow.push(ReminderType::GeneralMessage);
        }
        if allow.is_empty() {
            self.error = Some("Enable appointment reminders, confirmations or post-treatment follow-ups in Settings".into());
            return;
        }

        let now = Local::now().naive_local();
        let dry = self.features.enabled(FeatureKey::ReminderDryRun);
        let sent = self.sent_keys.clone();
        let outbox = self.outbox.clone();
        let messages = {
            // `clinics` is not used by this slice's templates, so an empty
            // slice is passed. Appointment/provider/operatory data is real.
            let clinics: [Clinic; 0] = [];
            let ctx = ReminderContext {
                rules: &self.reminder_rules,
                appointments: &self.appointments,
                procedures: &self.procedures,
                patients: &self.patients,
                contacts: &self.patient_contacts,
                providers: &self.providers,
                clinics: &clinics,
                operatories: &self.operatories,
                config: &self.eservices_config,
                now,
                allow_types: &allow,
                dry_run: dry,
            };
            due_messages(&ctx, |rule, apt| {
                sent.contains(&(rule, apt))
                    || outbox
                        .iter()
                        .any(|m| m.rule_num == rule && m.apt_num == apt)
            })
        };

        let mut inserted = 0;
        let mut errors = Vec::new();
        for message in &messages {
            match self.store.insert_outbox(message) {
                Ok(_) => inserted += 1,
                Err(err) => errors.push(err),
            }
        }
        self.outbox = self.store.list_outbox(200).unwrap_or_default();
        self.audit_if(
            FeatureKey::AuditReminders,
            Permission::APPOINTMENT_EDIT,
            0,
            format!("Generated {inserted} reminder message(s) (dry run: {dry})"),
        );
        if errors.is_empty() {
            self.notice = Some(format!("Queued {inserted} reminder message(s)"));
            self.error = None;
        } else {
            self.error = Some(errors.join("; "));
        }
    }

    // ---------------------------------------------------------- Outbox

    fn outbox_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh();
            }
            let deliver_button = ui.button("Deliver pending");
            if deliver_button.clicked() {
                self.deliver_pending();
            }
            deliver_button.on_hover_text("Send every Pending row through its configured channel. Dry run, Sent and Failed rows are left alone.");
            ui.separator();
            ui.weak(format!("{} row(s) in the local outbox", self.outbox.len()));
        });
        if self.features.enabled(FeatureKey::ReminderDryRun) {
            ui.colored_label(
                Color32::from_rgb(120, 90, 190),
                "Dry run ON — delivery is disabled until you turn it off in Settings.",
            );
        }
        ui.weak("This queue replaces Open Dental's HQ-side AutoComm records. Message IDs are generated locally.");
        ui.separator();

        if self.outbox.is_empty() {
            ui.label("The outbox is empty. Generate reminders or send a message from the Schedule/Recalls tab.");
            return;
        }

        egui::ScrollArea::vertical()
            .max_height(420.0)
            .show(ui, |ui| {
                egui::Grid::new("eservices_outbox_grid")
                    .num_columns(7)
                    .striped(true)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.strong("Status");
                        ui.strong("Channel");
                        ui.strong("To");
                        ui.strong("Patient");
                        ui.strong("Created");
                        ui.strong("Message");
                        ui.strong("Detail");
                        ui.end_row();
                        for message in &self.outbox {
                            let status_label =
                                ui.colored_label(status_color(message.status), message.status.label());
                            if message.status == OutboxStatus::DryRun {
                                status_label
                                    .on_hover_text(delivery_preview(message, &self.eservices_config));
                            }
                            ui.label(message.transport.label());
                            ui.label(short_body(&message.recipient));
                            let name = self
                                .patients
                                .iter()
                                .find(|p| p.pat_num == message.pat_num)
                                .map(|p| format!("{} {}", p.f_name, p.l_name))
                                .unwrap_or_else(|| message.pat_num.to_string());
                            ui.label(name);
                            ui.label(message.created_at.format("%m-%d %H:%M").to_string());
                            ui.label(short_body(&message.body));
                            let detail = if message.last_error.trim().is_empty() {
                                format!("tries {}", message.attempts)
                            } else {
                                short_body(&message.last_error)
                            };
                            ui.label(detail);
                            ui.end_row();
                        }
                    });
            });
    }

    fn deliver_pending(&mut self) {
        if !self.features.enabled(FeatureKey::EServicesEnabled) {
            self.error = Some("eServices is turned off in Settings".into());
            return;
        }
        if self.features.enabled(FeatureKey::ReminderDryRun) {
            self.error = Some("Dry-run mode is on — turn it off in Settings before delivering".into());
            return;
        }
        let pending: Vec<OutboxMessage> = self
            .outbox
            .iter()
            .filter(|m| m.status == OutboxStatus::Pending)
            .cloned()
            .collect();
        if pending.is_empty() {
            self.notice = Some("No pending messages to deliver".into());
            return;
        }

        let mut sent = 0;
        let mut failed = 0;
        for message in &pending {
            let attempts = message.attempts + 1;
            match deliver(message, &self.eservices_config) {
                Ok(receipt) => {
                    let now = Local::now().naive_local();
                    match self.store.update_outbox(
                        message.message_num,
                        OutboxStatus::Sent,
                        attempts,
                        Some(now),
                        &receipt,
                    ) {
                        Ok(()) => {
                            if message.rule_num > 0 && message.apt_num > 0 {
                                let _ = self.store.mark_reminder_sent(
                                    message.rule_num,
                                    message.apt_num,
                                    message.pat_num,
                                );
                            }
                            self.audit_if(
                                FeatureKey::AuditReminders,
                                Permission::APPOINTMENT_EDIT,
                                message.pat_num,
                                format!(
                                    "Delivered {} to {} via {}",
                                    message.message_num,
                                    message.recipient,
                                    message.transport.label()
                                ),
                            );
                            sent += 1;
                        }
                        Err(err) => {
                            self.error = Some(err);
                            failed += 1;
                        }
                    }
                }
                Err(err) => {
                    let _ = self.store.update_outbox(
                        message.message_num,
                        OutboxStatus::Failed,
                        attempts,
                        None,
                        &err,
                    );
                    self.audit_if(
                        FeatureKey::AuditReminders,
                        Permission::APPOINTMENT_EDIT,
                        message.pat_num,
                        format!("Failed to deliver {}: {err}", message.message_num),
                    );
                    failed += 1;
                }
            }
        }
        self.outbox = self.store.list_outbox(200).unwrap_or_default();
        self.sent_keys = self
            .store
            .list_reminder_sent()
            .unwrap_or_default()
            .into_iter()
            .collect();
        self.notice = Some(format!("Delivered {sent} message(s), {failed} failed"));
        if failed == 0 {
            self.error = None;
        }
    }

    // ---------------------------------------------------------- Dialogs

    pub(super) fn send_message_window(&mut self, ctx: &egui::Context) {
        if !self.send_message_open {
            return;
        }
        let mut send = false;
        let mut cancel = false;

        theme::AppWindow::new("Send message")
            .open(&mut self.send_message_open)
            .resizable(true)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label("Queues a message to the local outbox. Use the Outbox tab to deliver it.");
                ui.horizontal(|ui| {
                    ui.label("Patient");
                    egui::ComboBox::from_id_salt("eservices_manual_patient")
                        .selected_text(
                            self.patients
                                .iter()
                                .find(|p| Some(p.pat_num) == self.manual_patient)
                                .map(|p| format!("{} {} (#{})", p.f_name, p.l_name, p.pat_num))
                                .unwrap_or_else(|| "Select patient".into()),
                        )
                        .show_ui(ui, |ui| {
                            for patient in &self.patients {
                                ui.selectable_value(
                                    &mut self.manual_patient,
                                    Some(patient.pat_num),
                                    format!("{} {} (#{})", patient.f_name, patient.l_name, patient.pat_num),
                                );
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Channel");
                    egui::ComboBox::from_id_salt("eservices_manual_transport")
                        .selected_text(self.manual_transport.label())
                        .show_ui(ui, |ui| {
                            for kind in TransportKind::ALL {
                                if !kind.implemented() || !self.eservices_config.is_enabled(kind) {
                                    continue;
                                }
                                ui.selectable_value(
                                    &mut self.manual_transport,
                                    kind,
                                    kind.label(),
                                );
                            }
                        });
                });
                ui.label("Message");
                ui.add(
                    egui::TextEdit::multiline(&mut self.manual_text)
                        .desired_rows(4)
                        .hint_text("Use {DateTime} to insert the next appointment time."),
                );
                if self.features.enabled(FeatureKey::ReminderDryRun) {
                    ui.colored_label(Color32::from_rgb(120, 90, 190), "Dry run is ON — this will be queued but not delivered.");
                }
                ui.separator();
                ui.horizontal(|ui| {
                    send = ui.button("Queue message").clicked();
                    cancel = ui.button("Close").clicked();
                });
            });

        if cancel {
            self.send_message_open = false;
        }
        if send {
            let pat_num = self.manual_patient.or(self.selected_patient);
            let Some(patient) = self.patients.iter().find(|p| Some(p.pat_num) == pat_num).cloned() else {
                self.error = Some("Select a patient first".into());
                return;
            };
            let appointment = self
                .appointments
                .iter()
                .find(|a| a.pat_num == patient.pat_num)
                .cloned();
            let contact = self
                .patient_contacts
                .iter()
                .find(|c| c.pat_num == patient.pat_num)
                .cloned()
                .unwrap_or_else(|| PatientContact {
                    pat_num: patient.pat_num,
                    ..Default::default()
                });
            let dry = self.features.enabled(FeatureKey::ReminderDryRun);
            let now = Local::now().naive_local();
            let result = manual_message(
                &patient,
                appointment.as_ref(),
                &contact,
                &self.eservices_config,
                self.manual_transport,
                &self.manual_text,
                dry,
                now,
            );
            match result {
                Ok(message) => match self.store.insert_outbox(&message) {
                    Ok(num) => {
                        self.audit_if(
                            FeatureKey::AuditReminders,
                            Permission::APPOINTMENT_EDIT,
                            patient.pat_num,
                            format!(
                                "Queued manual {} message #{num} to {}",
                                self.manual_transport.label(),
                                message.recipient
                            ),
                        );
                        self.outbox = self.store.list_outbox(200).unwrap_or_default();
                        self.notice = Some(format!("Queued message #{num}"));
                        self.error = None;
                        self.send_message_open = false;
                        self.manual_text.clear();
                    }
                    Err(err) => self.error = Some(err),
                },
                Err(err) => self.error = Some(err),
            }
        }
    }

    pub(super) fn rule_window(&mut self, ctx: &egui::Context) {
        let Some(mut rule) = self.rule_edit.take() else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        let is_new = rule.rule_num == 0;
        let rule_type = self.rule_type;
        let comm_type = self.rule_comm_type;

        theme::AppWindow::new(if is_new {
            "Reminder rule — New"
        } else {
            "Reminder rule — Edit"
        })
        .open(&mut self.rule_edit_open)
        .resizable(true)
        .default_width(520.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Type    ");
                let label = rule_type.label().to_string();
                egui::ComboBox::from_id_salt("eservices_rule_type")
                    .selected_text(&label)
                    .show_ui(ui, |ui| {
                        for kind in ReminderType::ALL {
                            ui.selectable_value(
                                &mut self.rule_type,
                                kind,
                                kind.label(),
                            );
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label("Lead    ");
                ui.text_edit_singleline(&mut self.rule_minutes_text);
                ui.label(if rule_type == ReminderType::GeneralMessage {
                    "minutes after procedure completion (4320 = 72 hours)"
                } else {
                    "minutes before the appointment"
                });
            });
            ui.horizontal(|ui| {
                ui.label("Minimum");
                ui.text_edit_singleline(&mut self.rule_dnd_text);
                ui.label("minutes notice (OD: DoNotSendWithin)");
            });
            ui.horizontal(|ui| {
                ui.label("Channel ");
                let label = comm_type.label().to_string();
                egui::ComboBox::from_id_salt("eservices_rule_comm")
                    .selected_text(&label)
                    .show_ui(ui, |ui| {
                        for kind in CommType::ALL {
                            ui.selectable_value(
                                &mut self.rule_comm_type,
                                kind,
                                kind.label(),
                            );
                        }
                    });
            });
            ui.checkbox(&mut rule.is_enabled, "Rule enabled");
            ui.separator();
            ui.label("Text message template");
            ui.add(
                egui::TextEdit::multiline(&mut self.rule_sms_template)
                    .desired_rows(2)
                    .hint_text("Hi {FirstName}, your appointment is {DateTime} at {Clinic}."),
            );
            ui.label("Email subject");
            ui.text_edit_singleline(&mut self.rule_email_subject);
            ui.label("Email body");
            ui.add(
                egui::TextEdit::multiline(&mut self.rule_email_body)
                    .desired_rows(4)
                    .hint_text("Hello {PatientName}, this is a reminder for {DateTime} with {Provider}."),
            );
            ui.weak("Tokens: {FirstName}, {LastName}, {PatientName}, {Date}, {Time}, {DateTime}, {Provider}, {Clinic}, {Operatory}, {Today}");
            ui.separator();
            ui.horizontal(|ui| {
                save = ui.button("Save rule").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });

        if cancel {
            self.rule_edit_open = false;
        } else if save {
            rule.type_cur = self.rule_type;
            rule.comm_type = self.rule_comm_type;
            rule.minutes_prior = self.rule_minutes_text.parse().unwrap_or(rule.minutes_prior);
            rule.do_not_send_within_minutes = self
                .rule_dnd_text
                .parse()
                .unwrap_or(rule.do_not_send_within_minutes);
            rule.sms_template = self.rule_sms_template.clone();
            rule.email_subject = self.rule_email_subject.clone();
            rule.email_body = self.rule_email_body.clone();
            let rule_num = rule.rule_num;
            match self.store.upsert_reminder_rule(&rule) {
                Ok(num) => {
                    self.audit_if(
                        FeatureKey::AuditReminders,
                        Permission::SETUP,
                        0,
                        format!("Saved reminder rule {num}"),
                    );
                    self.refresh();
                    self.notice = Some(format!("Saved reminder rule {num}"));
                    self.error = None;
                    self.rule_edit_open = false;
                }
                Err(err) => {
                    self.error = Some(err);
                    self.rule_edit = Some(rule);
                    self.rule_edit_open = true;
                }
            }
            let _ = rule_num;
        } else if self.rule_edit_open {
            self.rule_edit = Some(rule);
        }
    }
}
