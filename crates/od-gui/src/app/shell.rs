//! Open Dental-style application shell and the patient-to-appointment loop.
//!
//! This module deliberately sits beside the older `app.rs`, `schedule_board.rs`
//! and `theme.rs` work. It adds the familiar OD menu strip and a persistent
//! patient context bar, then wires them to the existing patient, appointment,
//! recall, waitlist and messaging dialogs rather than duplicating their writes.

use super::*;
use egui::RichText;

/// Where a patient selection should land.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PatientPickerTarget {
    #[default]
    Global,
    Appointment,
    Recall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellMenuAction {
    Refresh,
    FindPatient,
    NewPatient,
    NewAppointment,
    NewRecall,
    OpenModule(Module),
    OpenApptTab(ApptTab),
    OpenReport(ReportKind),
    OpenDefinitions(DefinitionsTab),
    OpenEServices(EServicesTab),
    OpenSettings,
    OpenPreferences,
    SendMessage,
    Logout,
    ScopeNote,
}

impl ShellMenuAction {
    fn label(self) -> &'static str {
        match self {
            Self::Refresh => "Refresh",
            Self::FindPatient => "Find patient…",
            Self::NewPatient => "New patient",
            Self::NewAppointment => "New appointment",
            Self::NewRecall => "New recall",
            Self::OpenModule(module) => module.label(),
            Self::OpenApptTab(tab) => tab.label(),
            Self::OpenReport(kind) => kind.label(),
            Self::OpenDefinitions(tab) => match tab {
                DefinitionsTab::Definitions => "Definitions",
                DefinitionsTab::DisplayFields => "Display fields",
            },
            Self::OpenEServices(tab) => match tab {
                EServicesTab::Channels => "Channels",
                EServicesTab::Rules => "Reminder rules",
                EServicesTab::Outbox => "Outbox",
            },
            Self::OpenSettings => "Show features",
            Self::OpenPreferences => "Preferences…",
            Self::SendMessage => "Send message…",
            Self::Logout => "Log off",
            Self::ScopeNote => "Scope and limitations",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BannerAction {
    Select,
    New,
    Edit,
    Schedule,
    Recall,
    Asap,
    Waitlist,
    Message,
    LogCall,
    Chart,
    Account,
    OpenNext,
}

fn shell_menus(features: &FeatureSettings) -> Vec<(&'static str, Vec<ShellMenuAction>)> {
    let mut file = vec![ShellMenuAction::Refresh];
    if features.enabled(FeatureKey::PatientSearch) {
        file.push(ShellMenuAction::FindPatient);
    }
    if features.enabled(FeatureKey::PatientEdit) && features.module_enabled(Module::Family) {
        file.push(ShellMenuAction::NewPatient);
    }
    if features.enabled(FeatureKey::AppointmentCreate)
        && features.module_enabled(Module::Appointments)
    {
        file.push(ShellMenuAction::NewAppointment);
        if features.enabled(FeatureKey::RecallsEnabled) {
            file.push(ShellMenuAction::NewRecall);
        }
    }

    let mut setup = Vec::new();
    if features.module_enabled(Module::Setup) {
        setup.push(ShellMenuAction::OpenModule(Module::Setup));
    }
    if features.enabled(FeatureKey::ShowDefinitions) {
        setup.push(ShellMenuAction::OpenDefinitions(
            DefinitionsTab::Definitions,
        ));
    }
    if features.enabled(FeatureKey::ShowDisplayFields) {
        setup.push(ShellMenuAction::OpenDefinitions(
            DefinitionsTab::DisplayFields,
        ));
    }
    if features.enabled(FeatureKey::EServicesEnabled) {
        setup.push(ShellMenuAction::OpenEServices(EServicesTab::Channels));
    }
    if features.enabled(FeatureKey::ShowPreferences) {
        setup.push(ShellMenuAction::OpenPreferences);
    }
    setup.push(ShellMenuAction::OpenSettings);

    let mut lists = Vec::new();
    if features.module_enabled(Module::Family) {
        lists.push(ShellMenuAction::OpenModule(Module::Family));
    }
    if features.module_enabled(Module::Appointments) {
        lists.push(ShellMenuAction::OpenApptTab(ApptTab::Schedule));
        if features.enabled(FeatureKey::RecallsEnabled) {
            lists.push(ShellMenuAction::OpenApptTab(ApptTab::Recalls));
        }
        if features.enabled(FeatureKey::AsapListEnabled) {
            lists.push(ShellMenuAction::OpenApptTab(ApptTab::Asap));
        }
        if features.enabled(FeatureKey::WaitlistEnabled) {
            lists.push(ShellMenuAction::OpenApptTab(ApptTab::Waitlist));
        }
    }

    let report_items = [
        (ReportKind::PatientList, FeatureKey::ReportPatientList),
        (ReportKind::Production, FeatureKey::ReportProduction),
        (ReportKind::Collections, FeatureKey::ReportCollections),
        (ReportKind::Schedule, FeatureKey::ReportSchedule),
        (ReportKind::RecallAsap, FeatureKey::ReportRecallAsap),
        (ReportKind::Aging, FeatureKey::ReportAging),
    ];
    let mut reports = Vec::new();
    let mut custom = Vec::new();
    if features.module_enabled(Module::Reports) {
        for (kind, key) in report_items {
            if features.enabled(key) {
                reports.push(ShellMenuAction::OpenReport(kind));
            }
        }
        for (kind, key) in [
            (ReportKind::Patients, FeatureKey::ReportPatients),
            (ReportKind::RecallFields, FeatureKey::ReportRecallFields),
            (
                ReportKind::TreatmentFinder,
                FeatureKey::ReportTreatmentFinder,
            ),
        ] {
            if features.enabled(key) {
                custom.push(ShellMenuAction::OpenReport(kind));
            }
        }
    }

    let mut tools = Vec::new();
    if features.enabled(FeatureKey::EServicesEnabled) {
        if features.enabled(FeatureKey::RemindersEnabled) {
            tools.push(ShellMenuAction::OpenEServices(EServicesTab::Rules));
        }
        tools.push(ShellMenuAction::OpenEServices(EServicesTab::Outbox));
        tools.push(ShellMenuAction::SendMessage);
    }

    let mut clinics = Vec::new();
    if features.module_enabled(Module::Setup) {
        clinics.push(ShellMenuAction::OpenModule(Module::Setup));
    }
    if features.module_enabled(Module::Appointments) {
        clinics.push(ShellMenuAction::OpenApptTab(ApptTab::Schedule));
    }
    if features.enabled(FeatureKey::ClinicCardsEnabled) && features.module_enabled(Module::Family) {
        clinics.push(ShellMenuAction::OpenModule(Module::Family));
    }

    let mut eservices = Vec::new();
    if features.enabled(FeatureKey::EServicesEnabled) {
        eservices.push(ShellMenuAction::OpenEServices(EServicesTab::Channels));
        eservices.push(ShellMenuAction::OpenEServices(EServicesTab::Rules));
        eservices.push(ShellMenuAction::OpenEServices(EServicesTab::Outbox));
        eservices.push(ShellMenuAction::SendMessage);
    }

    let mut alerts = Vec::new();
    if features.enabled(FeatureKey::ShowAuditLog) {
        alerts.push(ShellMenuAction::OpenModule(Module::Setup));
    }
    if features.enabled(FeatureKey::EServicesEnabled) {
        alerts.push(ShellMenuAction::OpenEServices(EServicesTab::Outbox));
    }

    vec![
        ("File", file),
        ("Setup", setup),
        ("Lists", lists),
        ("Reports", reports),
        ("Custom Reports", custom),
        ("Tools", tools),
        ("Clinics", clinics),
        ("eServices", eservices),
        ("Alerts", alerts),
        (
            "Help",
            vec![ShellMenuAction::ScopeNote, ShellMenuAction::OpenSettings],
        ),
    ]
}

pub(super) fn appointment_draft(
    pat_num: Id,
    when: chrono::NaiveDateTime,
    operatory_num: Id,
    provider_num: Id,
) -> Appointment {
    Appointment {
        pat_num,
        apt_datetime: when,
        apt_length: 30,
        apt_status: AppointmentStatus::Scheduled,
        operatory_num,
        provider_num,
        clinic_num: 1,
        pattern: pattern_for_length(30),
        ..Default::default()
    }
}

pub(super) fn waitlist_draft(
    pat_num: Id,
    definitions: &[Definition],
    operatory_num: Id,
    provider_num: Id,
    today: NaiveDate,
) -> Appointment {
    Appointment {
        pat_num,
        apt_datetime: today.and_hms_opt(9, 0, 0).unwrap_or_default(),
        apt_length: 30,
        apt_status: AppointmentStatus::UnschedList,
        waitlist_status: waitlist_status_definitions(definitions)
            .first()
            .map(|d| d.def_num)
            .unwrap_or(0),
        operatory_num,
        provider_num,
        clinic_num: 1,
        pattern: pattern_for_length(30),
        ..Default::default()
    }
}

pub(super) fn asap_draft(
    pat_num: Id,
    definitions: &[Definition],
    operatory_num: Id,
    provider_num: Id,
    today: NaiveDate,
) -> Appointment {
    Appointment {
        priority: RecallPriority::Asap,
        ..waitlist_draft(pat_num, definitions, operatory_num, provider_num, today)
    }
}

pub(super) fn recall_draft(pat_num: Id, today: NaiveDate) -> Recall {
    Recall {
        pat_num,
        date_due: Some(today),
        date_due_calc: Some(today),
        recall_interval: interval_from_parts(0, 6, 0, 0),
        ..Default::default()
    }
}

pub(super) fn broken_visit(mut apt: Appointment) -> Appointment {
    apt.apt_status = AppointmentStatus::Broken;
    apt
}

pub(super) fn upcoming_appointment<'a>(
    appointments: &'a [Appointment],
    pat_num: Id,
    now: chrono::NaiveDateTime,
) -> Option<&'a Appointment> {
    appointments
        .iter()
        .filter(|apt| apt.pat_num == pat_num && apt.apt_datetime >= now)
        .filter(|apt| {
            matches!(
                apt.apt_status,
                AppointmentStatus::Scheduled
                    | AppointmentStatus::Asap
                    | AppointmentStatus::Planned
                    | AppointmentStatus::PtNote
            )
        })
        .min_by_key(|apt| apt.apt_datetime)
}

impl OdApp {
    fn default_operatory(&self) -> Id {
        self.operatories
            .first()
            .map(|op| op.operatory_num)
            .unwrap_or(0)
    }

    fn default_provider(&self) -> Id {
        self.providers
            .first()
            .map(|provider| provider.provider_num)
            .unwrap_or(0)
    }

    pub(super) fn open_new_patient(&mut self) {
        let mut patient = Patient::default();
        patient.gender = Gender::Unknown;
        patient.clinic_num = 1;
        self.patient_edit = Some(patient);
        self.birth_text.clear();
        self.patient_reason_text.clear();
        self.patient_edit_open = true;
    }

    pub(super) fn open_patient_editor(&mut self, patient: Patient) {
        self.birth_text = patient
            .birth_date
            .map(|date| date.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        self.patient_reason_text.clear();
        self.patient_edit = Some(patient);
        self.patient_edit_open = true;
    }

    pub(super) fn open_patient_picker(&mut self, target: PatientPickerTarget) {
        if !self.features.enabled(FeatureKey::PatientSearch) {
            self.error = Some("Patient search is turned off in Settings".into());
            return;
        }
        self.patient_picker_target = target;
        self.patient_picker_open = true;
        if target == PatientPickerTarget::Global {
            self.patient_picker_query.clear();
        }
    }

    /// Apply a selection from the OD-style patient picker. When the picker was
    /// opened from a draft, the draft is filled in instead of opening a second
    /// editor; when it was opened from a menu, the first daily-loop draft opens.
    pub(super) fn apply_patient_picker_selection(&mut self, pat_num: Id) {
        self.selected_patient = Some(pat_num);
        match self.patient_picker_target {
            PatientPickerTarget::Global => {}
            PatientPickerTarget::Appointment => {
                if self.apt_edit_open && self.apt_edit.is_some() {
                    if let Some(apt) = self.apt_edit.as_mut() {
                        apt.pat_num = pat_num;
                    }
                } else if self.features.enabled(FeatureKey::AppointmentCreate)
                    && self.can(Permission::APPOINTMENT_CREATE)
                {
                    let when = self.schedule_day.and_hms_opt(9, 0, 0).unwrap_or_default();
                    let apt = appointment_draft(
                        pat_num,
                        when,
                        self.default_operatory(),
                        self.default_provider(),
                    );
                    self.open_appointment_editor(apt);
                }
            }
            PatientPickerTarget::Recall => {
                if self.recall_edit_open && self.recall_edit.is_some() {
                    if let Some(recall) = self.recall_edit.as_mut() {
                        recall.pat_num = pat_num;
                    }
                } else if self.features.enabled(FeatureKey::AppointmentCreate)
                    && self.can(Permission::APPOINTMENT_CREATE)
                {
                    let recall = recall_draft(pat_num, Local::now().date_naive());
                    self.open_recall_editor(recall);
                }
            }
        }
        self.patient_picker_open = false;
        self.patient_picker_query.clear();
        self.patient_picker_target = PatientPickerTarget::Global;
        self.refresh();
    }

    /// OD has no `Cancelled` appointment status: a cancellation is recorded as
    /// `Broken` (AptStatus 5), with the same history and audit path as a reschedule.
    pub(super) fn cancel_visit(&mut self, apt: Appointment) {
        if !(self.features.enabled(FeatureKey::AppointmentEdit)
            && self.can(Permission::APPOINTMENT_EDIT))
        {
            self.error = Some("Appointment edit permission/feature required".into());
            return;
        }
        let before = self
            .store
            .get_appointment(apt.apt_num)
            .ok()
            .flatten()
            .unwrap_or_else(|| apt.clone());
        let mut saved = broken_visit(before);
        if !apt.notes.trim().is_empty() {
            saved.notes = apt.notes;
        }
        if self.features.enabled(FeatureKey::AppointmentHistory) {
            let mut change =
                AppointmentChange::from_snapshot(&saved, AppointmentChangeAction::Changed);
            change.user_num = self.session.as_ref().map(|s| s.user_num).unwrap_or(0);
            if let Err(err) = self.store.append_appointment_change(&change) {
                self.error = Some(err);
                return;
            }
        }
        match self.store.upsert_appointment(&saved) {
            Ok(_) => {
                self.audit_if(
                    FeatureKey::AuditAppointmentChanges,
                    Permission::APPOINTMENT_EDIT,
                    saved.pat_num,
                    format!(
                        "Appointment {} cancelled (OD AptStatus = Broken)",
                        saved.apt_num
                    ),
                );
                self.notice = Some("Visit cancelled and marked Broken".into());
                self.error = None;
                self.refresh();
            }
            Err(err) => self.error = Some(err),
        }
    }

    fn apply_shell_action(&mut self, action: ShellMenuAction) {
        match action {
            ShellMenuAction::Refresh => {
                self.refresh();
                self.notice = Some("Refreshed".into());
            }
            ShellMenuAction::FindPatient => self.open_patient_picker(PatientPickerTarget::Global),
            ShellMenuAction::NewPatient => {
                if self.features.enabled(FeatureKey::PatientEdit)
                    && self.can(Permission::PATIENT_CREATE)
                {
                    self.open_new_patient();
                }
            }
            ShellMenuAction::NewAppointment => {
                if !(self.features.enabled(FeatureKey::AppointmentCreate)
                    && self.can(Permission::APPOINTMENT_CREATE))
                {
                    return;
                }
                if let Some(pat_num) = self.selected_patient {
                    let when = self.schedule_day.and_hms_opt(9, 0, 0).unwrap_or_default();
                    let apt = appointment_draft(
                        pat_num,
                        when,
                        self.default_operatory(),
                        self.default_provider(),
                    );
                    self.open_appointment_editor(apt);
                } else {
                    self.open_patient_picker(PatientPickerTarget::Appointment);
                }
            }
            ShellMenuAction::NewRecall => {
                if self.features.enabled(FeatureKey::RecallsEnabled)
                    && self.features.enabled(FeatureKey::AppointmentCreate)
                    && self.can(Permission::APPOINTMENT_CREATE)
                {
                    self.open_patient_picker(PatientPickerTarget::Recall);
                }
            }
            ShellMenuAction::OpenModule(module) => {
                if self.can_module(module) {
                    self.module = module;
                    self.show_settings = false;
                    self.refresh();
                }
            }
            ShellMenuAction::OpenApptTab(tab) => {
                if !tab.enabled(&self.features) || !self.can_module(Module::Appointments) {
                    return;
                }
                self.module = Module::Appointments;
                self.appt_tab = tab;
                self.show_settings = false;
                self.refresh();
            }
            ShellMenuAction::OpenReport(kind) => {
                if self.features.enabled(kind.feature_key()) && self.can_module(Module::Reports) {
                    self.report = kind;
                    self.module = Module::Reports;
                    self.show_settings = false;
                    self.refresh();
                }
            }
            ShellMenuAction::OpenDefinitions(tab) => {
                if self.can_module(Module::Setup) {
                    self.module = Module::Setup;
                    self.definitions_tab = tab;
                    self.show_settings = false;
                    self.refresh();
                }
            }
            ShellMenuAction::OpenEServices(tab) => {
                if self.features.enabled(FeatureKey::EServicesEnabled)
                    && self.can_module(Module::Setup)
                {
                    self.module = Module::Setup;
                    self.eservices_tab = tab;
                    self.show_settings = false;
                    self.refresh();
                }
            }
            ShellMenuAction::OpenSettings => {
                self.show_settings = true;
                self.preferences.open = false;
                self.notice = Some("Show Features toggles are per user on this workstation".into());
            }
            ShellMenuAction::OpenPreferences => {
                preferences::open(self);
                self.show_settings = false;
            }
            ShellMenuAction::SendMessage => {
                if self.features.enabled(FeatureKey::EServicesEnabled) {
                    self.manual_patient = self.selected_patient;
                    self.send_message_open = true;
                }
            }
            ShellMenuAction::Logout => self.logout(),
            ShellMenuAction::ScopeNote => {
                self.notice = Some(
                    "Wave 1: OD-style shell plus the patient → appointment daily loop. Full form parity is out of scope."
                        .into(),
                );
            }
        }
    }
}

pub(super) fn menu_bar(app: &mut OdApp, ui: &mut Ui) {
    egui::Panel::top("od_menu").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new("odclinic").color(theme::teal()).strong());
            ui.separator();

            let mut clicked = Vec::new();
            for (label, items) in shell_menus(&app.features) {
                ui.menu_button(label, |ui| {
                    if items.is_empty() {
                        ui.label(RichText::new("Unavailable in Settings").weak().small());
                        return;
                    }
                    for item in items {
                        if ui.button(item.label()).clicked() {
                            clicked.push(item);
                        }
                    }
                });
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if app.login_required() && ui.button("Log off").clicked() {
                    app.apply_shell_action(ShellMenuAction::Logout);
                }
                if app.features.enabled(FeatureKey::ShowDatabaseLabel) {
                    ui.separator();
                    ui.label(RichText::new(&app.db_label).color(theme::muted()).small());
                }
                if let Some(session) = &app.session {
                    ui.label(format!("Signed in: {}", session.user_name));
                }
            });

            for action in clicked {
                app.apply_shell_action(action);
            }
        });
    });
}

pub(super) fn patient_banner(app: &mut OdApp, ui: &mut Ui) {
    egui::Panel::top("patient_banner").show(ui, |ui| {
        egui::Frame::new()
            .fill(theme::paper())
            .inner_margin(egui::Margin::symmetric(12, 4))
            .show(ui, |ui| {
                let patient = app
                    .selected_patient
                    .and_then(|num| app.patients.iter().find(|p| p.pat_num == num).cloned());
                let next = patient.as_ref().and_then(|p| {
                    upcoming_appointment(
                        &app.appointments,
                        p.pat_num,
                        Local::now().naive_local(),
                    )
                    .cloned()
                });
                let mut action = None;

                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("PATIENT").color(theme::muted()).small());
                    match &patient {
                        Some(patient) => {
                            ui.label(
                                RichText::new(patient.display_name())
                                    .strong()
                                    .color(theme::ink()),
                            );
                            ui.label(RichText::new(format!("#{}", patient.pat_num)).weak());
                            if app.features.enabled(FeatureKey::PatientBadges) {
                                if let Some(badge) = app.badges.get(&patient.pat_num) {
                                    for label in &badge.labels {
                                        ui.label(RichText::new(label).small().color(theme::teal()));
                                    }
                                }
                            }
                            ui.separator();
                            ui.label(
                                RichText::new(format!(
                                    "DOB {} · Home {} · Mobile {}",
                                    patient
                                        .birth_date
                                        .map(|date| date.format("%Y-%m-%d").to_string())
                                        .unwrap_or_else(|| "-".into()),
                                    if patient.home_phone.trim().is_empty() {
                                        "-"
                                    } else {
                                        &patient.home_phone
                                    },
                                    if patient.wireless_phone.trim().is_empty() {
                                        "-"
                                    } else {
                                        &patient.wireless_phone
                                    }
                                ))
                                .small()
                                .color(theme::muted()),
                            );
                        }
                        None => {
                            ui.label(
                                RichText::new("No patient selected")
                                    .italics()
                                    .color(theme::muted()),
                            );
                        }
                    }
                    ui.separator();
                    match &next {
                        Some(apt) => ui.label(
                            RichText::new(format!(
                                "Next: {}",
                                apt.apt_datetime.format("%a %Y-%m-%d %H:%M")
                            ))
                            .small()
                            .color(theme::teal()),
                        ),
                        None => ui.label(
                            RichText::new("No upcoming visit in this board range")
                                .small()
                                .weak(),
                        ),
                    }
                });

                ui.horizontal_wrapped(|ui| {
                    if app.features.enabled(FeatureKey::PatientSearch)
                        && ui.button("Select patient").clicked()
                    {
                        action = Some(BannerAction::Select);
                    }
                    if app.features.enabled(FeatureKey::PatientEdit)
                        && app.can(Permission::PATIENT_CREATE)
                        && ui.button("New").clicked()
                    {
                        action = Some(BannerAction::New);
                    }
                    if patient.is_some()
                        && app.features.enabled(FeatureKey::PatientEdit)
                        && app.can(Permission::PATIENT_EDIT)
                        && ui.button("Edit").clicked()
                    {
                        action = Some(BannerAction::Edit);
                    }
                    if patient.is_some()
                        && app.features.enabled(FeatureKey::AppointmentCreate)
                        && app.can(Permission::APPOINTMENT_CREATE)
                        && ui.button("Schedule").clicked()
                    {
                        action = Some(BannerAction::Schedule);
                    }
                    if patient.is_some()
                        && app.features.enabled(FeatureKey::AppointmentCreate)
                        && app.can(Permission::APPOINTMENT_CREATE)
                    {
                        if app.features.enabled(FeatureKey::RecallsEnabled)
                            && ui.button("Recall").clicked()
                        {
                            action = Some(BannerAction::Recall);
                        }
                        if ui
                            .button("ASAP")
                            .on_hover_text("Opens an unscheduled ASAP draft; press Save to apply.")
                            .clicked()
                        {
                            action = Some(BannerAction::Asap);
                        }
                        if app.features.enabled(FeatureKey::WaitlistEnabled)
                            && ui.button("Waitlist").clicked()
                        {
                            action = Some(BannerAction::Waitlist);
                        }
                    }
                    if patient.is_some()
                        && app.can_module(Module::Chart)
                        && ui.button("Chart").clicked()
                    {
                        action = Some(BannerAction::Chart);
                    }
                    if patient.is_some()
                        && app.can_module(Module::Account)
                        && ui.button("Account").clicked()
                    {
                        action = Some(BannerAction::Account);
                    }
                    if next.is_some()
                        && app.features.enabled(FeatureKey::AppointmentEdit)
                        && app.can(Permission::APPOINTMENT_EDIT)
                        && ui.button("Open visit").clicked()
                    {
                        action = Some(BannerAction::OpenNext);
                    }
                    if patient.is_some()
                        && app.features.enabled(FeatureKey::AppointmentCallLog)
                        && next.is_some()
                        && ui
                            .button("Log call")
                            .on_hover_text(
                                "Record a confirmation or phone call against the next appointment.",
                            )
                            .clicked()
                    {
                        action = Some(BannerAction::LogCall);
                    }
                    if patient.is_some()
                        && app.features.enabled(FeatureKey::EServicesEnabled)
                        && app.can(Permission::APPOINTMENT_EDIT)
                        && ui
                            .button("Call / message")
                            .on_hover_text(
                                "Queues a manual message to the local outbox. Use Log call for appointment contact history.",
                            )
                            .clicked()
                    {
                        action = Some(BannerAction::Message);
                    }
                });
                if let Some(action) = action {
                    let notice = match action {
                        BannerAction::Select => {
                            app.open_patient_picker(PatientPickerTarget::Global);
                            "Patient picker opened"
                        }
                        BannerAction::New => {
                            app.open_new_patient();
                            "New patient draft opened"
                        }
                        BannerAction::Edit => {
                            if let Some(patient) = patient.clone() {
                                app.open_patient_editor(patient);
                            }
                            "Patient editor opened"
                        }
                        BannerAction::Schedule => {
                            if let Some(patient) = &patient {
                                let when =
                                    app.schedule_day.and_hms_opt(9, 0, 0).unwrap_or_default();
                                let draft = appointment_draft(
                                    patient.pat_num,
                                    when,
                                    app.default_operatory(),
                                    app.default_provider(),
                                );
                                app.open_appointment_editor(draft);
                            }
                            "Appointment draft opened"
                        }
                        BannerAction::Recall => {
                            if let Some(patient) = &patient {
                                let draft = recall_draft(patient.pat_num, Local::now().date_naive());
                                app.open_recall_editor(draft);
                            }
                            "Recall draft opened"
                        }
                        BannerAction::Asap => {
                            if let Some(patient) = &patient {
                                let draft = asap_draft(
                                    patient.pat_num,
                                    &app.definitions,
                                    app.default_operatory(),
                                    app.default_provider(),
                                    Local::now().date_naive(),
                                );
                                app.open_appointment_editor(draft);
                            }
                            "ASAP draft opened"
                        }
                        BannerAction::Waitlist => {
                            if let Some(patient) = &patient {
                                let draft = waitlist_draft(
                                    patient.pat_num,
                                    &app.definitions,
                                    app.default_operatory(),
                                    app.default_provider(),
                                    Local::now().date_naive(),
                                );
                                app.open_appointment_editor(draft);
                            }
                            "Waitlist draft opened"
                        }
                        BannerAction::Chart => {
                            app.module = Module::Chart;
                            app.show_settings = false;
                            "Chart module opened"
                        }
                        BannerAction::Account => {
                            app.module = Module::Account;
                            app.show_settings = false;
                            "Account module opened"
                        }
                        BannerAction::OpenNext => {
                            if let Some(apt) = next.clone() {
                                app.open_appointment_editor(apt);
                            }
                            "Visit opened"
                        }
                        BannerAction::Message => {
                            app.manual_patient = patient.as_ref().map(|p| p.pat_num);
                            app.send_message_open = true;
                            "Message composer opened"
                        }
                        BannerAction::LogCall => {
                            if let (Some(patient), Some(apt)) = (&patient, &next) {
                                app.comm_log_apt_num = apt.apt_num;
                                app.comm_log_pat_num = patient.pat_num;
                                app.comm_log_note.clear();
                                app.comm_log_mode = CommLogMode::Phone;
                                app.comm_log_direction = CommLogDirection::Received;
                                app.comm_log_open = true;
                            }
                            "Call log opened"
                        }
                    };
                    app.notice = Some(notice.into());
                }
            });
    });
}

pub(super) fn patient_picker_window(app: &mut OdApp, ctx: &egui::Context) {
    if !app.patient_picker_open {
        return;
    }
    let query = if app.features.enabled(FeatureKey::PatientSearch) {
        app.patient_picker_query.clone()
    } else {
        String::new()
    };
    let rows = app.store.find_patients(&query).unwrap_or_default();
    let providers = app.providers.clone();
    let can_create =
        app.features.enabled(FeatureKey::PatientEdit) && app.can(Permission::PATIENT_CREATE);
    let mut selected = None;
    let mut new_patient = false;
    let mut cancel = false;

    theme::AppWindow::new("Select Patient")
        .open(&mut app.patient_picker_open)
        .resizable(true)
        .default_width(760.0)
        .default_height(460.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Search");
                ui.add(
                    egui::TextEdit::singleline(&mut app.patient_picker_query)
                        .desired_width(260.0)
                        .hint_text("name, chart # or phone"),
                );
                if can_create && ui.button("+ New patient").clicked() {
                    new_patient = true;
                }
            });
            ui.label(
                RichText::new(
                    "Each chart is an individual patient account. OD family grouping is not used.",
                )
                .small()
                .color(theme::muted()),
            );
            ui.separator();

            egui::Frame::new()
                .fill(theme::paper())
                .stroke(egui::Stroke::new(1.0, theme::line()))
                .corner_radius(egui::CornerRadius::same(6))
                .show(ui, |ui| {
                    let show_badges = app.features.enabled(FeatureKey::PatientBadges);
                    egui::Grid::new("od_patient_picker_grid")
                        .num_columns(if show_badges { 7 } else { 6 })
                        .striped(true)
                        .spacing([10.0, 4.0])
                        .show(ui, |ui| {
                            OdApp::table_header(ui, "Chart");
                            OdApp::table_header(ui, "Last name");
                            OdApp::table_header(ui, "First name");
                            OdApp::table_header(ui, "DOB");
                            OdApp::table_header(ui, "Phone");
                            OdApp::table_header(ui, "Provider");
                            if show_badges {
                                OdApp::table_header(ui, "Flags");
                            }
                            ui.end_row();
                            for patient in &rows {
                                ui.label(patient.pat_num.to_string());
                                if ui.button(&patient.l_name).clicked() {
                                    selected = Some(patient.pat_num);
                                }
                                ui.label(&patient.f_name);
                                ui.label(
                                    patient
                                        .birth_date
                                        .map(|date| date.format("%Y-%m-%d").to_string())
                                        .unwrap_or_default(),
                                );
                                ui.label(&patient.home_phone);
                                ui.label(
                                    providers
                                        .iter()
                                        .find(|p| p.provider_num == patient.primary_provider_num)
                                        .map(|p| p.abbreviation.clone())
                                        .unwrap_or_default(),
                                );
                                if show_badges {
                                    ui.label(
                                        app.badges
                                            .get(&patient.pat_num)
                                            .map(|badge| badge.labels.join(", "))
                                            .unwrap_or_default(),
                                    );
                                }
                                ui.end_row();
                            }
                            if rows.is_empty() {
                                ui.label("No matching patients.");
                                ui.label("");
                                ui.label("");
                                ui.label("");
                                ui.label("");
                                ui.label("");
                                if show_badges {
                                    ui.label("");
                                }
                                ui.end_row();
                            }
                        });
                });
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel = ui.button("Cancel").clicked();
                if ui.button("OK").clicked() {
                    selected = app.selected_patient;
                }
            });
        });

    if let Some(pat_num) = selected {
        app.apply_patient_picker_selection(pat_num);
    } else if new_patient {
        // Keep the picker target so saving the new chart continues the loop.
        app.open_new_patient();
    } else if cancel {
        app.patient_picker_open = false;
        app.patient_picker_query.clear();
        app.patient_picker_target = PatientPickerTarget::Global;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menus_hide_disabled_sections() {
        let mut features = FeatureSettings::default();
        let full = shell_menus(&features);
        assert!(full.iter().any(|(label, _)| *label == "eServices"));

        features.set(FeatureKey::EServicesEnabled, false);
        let lean = shell_menus(&features);
        let eservices = lean
            .iter()
            .find(|(label, _)| *label == "eServices")
            .map(|(_, items)| items)
            .unwrap();
        assert!(eservices.is_empty());
    }

    #[test]
    fn file_menu_exposes_the_daily_loop_when_enabled() {
        let features = FeatureSettings::default();
        let file = shell_menus(&features)
            .into_iter()
            .find(|(label, _)| *label == "File")
            .map(|(_, items)| items)
            .unwrap();
        assert!(file.contains(&ShellMenuAction::FindPatient));
        assert!(file.contains(&ShellMenuAction::NewPatient));
        assert!(file.contains(&ShellMenuAction::NewAppointment));
        assert!(file.contains(&ShellMenuAction::NewRecall));
        assert_ne!(PatientPickerTarget::Recall, PatientPickerTarget::Global);
    }

    #[test]
    fn appointment_draft_carries_selected_patient_and_slot() {
        let when = NaiveDate::from_ymd_opt(2026, 12, 1)
            .unwrap()
            .and_hms_opt(10, 30, 0)
            .unwrap();
        let apt = appointment_draft(42, when, 7, 3);
        assert_eq!(apt.pat_num, 42);
        assert_eq!(apt.apt_datetime, when);
        assert_eq!(apt.apt_length, 30);
        assert_eq!(apt.apt_status, AppointmentStatus::Scheduled);
        assert_eq!(apt.operatory_num, 7);
        assert_eq!(apt.provider_num, 3);
    }

    #[test]
    fn cancellation_marks_broken_without_changing_patient_or_slot() {
        let when = NaiveDate::from_ymd_opt(2026, 12, 1)
            .unwrap()
            .and_hms_opt(10, 30, 0)
            .unwrap();
        let apt = appointment_draft(42, when, 7, 3);
        let cancelled = broken_visit(apt.clone());
        assert_eq!(cancelled.apt_status, AppointmentStatus::Broken);
        assert_eq!(cancelled.pat_num, apt.pat_num);
        assert_eq!(cancelled.apt_datetime, apt.apt_datetime);
    }

    #[test]
    fn recall_and_waitlist_drafts_use_todays_loop_defaults() {
        let today = NaiveDate::from_ymd_opt(2026, 12, 1).unwrap();
        let recall = recall_draft(9, today);
        assert_eq!(recall.pat_num, 9);
        assert_eq!(recall.date_due, Some(today));

        let waitlist = waitlist_draft(9, &[], 2, 4, today);
        assert_eq!(waitlist.apt_status, AppointmentStatus::UnschedList);
        assert_eq!(waitlist.waitlist_status, 0);

        let asap = asap_draft(9, &[], 2, 4, today);
        assert_eq!(asap.priority, RecallPriority::Asap);
        assert_eq!(asap.apt_status, AppointmentStatus::UnschedList);
    }

    #[test]
    fn upcoming_appointment_ignores_past_and_closed_visits() {
        let now = NaiveDate::from_ymd_opt(2026, 12, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let past = appointment_draft(9, now - Duration::hours(2), 1, 1);
        let broken = broken_visit(appointment_draft(9, now + Duration::hours(1), 1, 1));
        let next = appointment_draft(9, now + Duration::hours(3), 1, 1);
        let other = appointment_draft(10, now + Duration::minutes(30), 1, 1);
        let rows = vec![past, broken, next.clone(), other];
        let found = upcoming_appointment(&rows, 9, now).unwrap();
        assert_eq!(found.apt_datetime, next.apt_datetime);
    }
}
