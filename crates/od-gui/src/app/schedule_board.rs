//! Decluttered day/week scheduling board.
//!
//! The board is a presentation layer over the same OD tables as the older
//! schedule grid. Appointments, the unscheduled-list "pin board" and recalls
//! remain OD-backed. Free-text blockout holds and visit-stage quick actions are
//! port-local because OD has no equivalent columns in this port's model; they
//! are persisted beside the other GUI preferences and never invented as OD rows.

use super::*;
use chrono::{Datelike, NaiveTime, Timelike};
use egui::{
    Align, Align2, Color32, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const DAY_START_MIN: i32 = 7 * 60;
const DAY_END_MIN: i32 = 19 * 60;
const SNAP_MIN: i32 = 15;
const PIXELS_PER_MIN: f32 = 1.55;
const HEADER_HEIGHT: f32 = 42.0;
const GUTTER_WIDTH: f32 = 52.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoardView {
    Day,
    Week,
}

impl BoardView {
    fn label(self) -> &'static str {
        match self {
            Self::Day => "Day",
            Self::Week => "Week",
        }
    }

    fn from_text(value: &str) -> Self {
        if value == "week" {
            Self::Week
        } else {
            Self::Day
        }
    }

    fn as_text(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoardStage {
    Arrived,
    InRoom,
    Checkout,
    Complete,
}

impl BoardStage {
    fn label(self) -> &'static str {
        match self {
            Self::Arrived => "Arrived",
            Self::InRoom => "In room",
            Self::Checkout => "Check out",
            Self::Complete => "Done",
        }
    }

    fn next(self) -> Option<Self> {
        match self {
            Self::Arrived => Some(Self::InRoom),
            Self::InRoom => Some(Self::Checkout),
            Self::Checkout => Some(Self::Complete),
            Self::Complete => None,
        }
    }

    fn color(self) -> Color32 {
        match self {
            Self::Arrived => theme::info(),
            Self::InRoom => Color32::from_rgb(112, 84, 145),
            Self::Checkout => Color32::from_rgb(160, 106, 40),
            Self::Complete => theme::success(),
        }
    }

    fn from_text(value: &str) -> Option<Self> {
        match value {
            "arrived" => Some(Self::Arrived),
            "inroom" => Some(Self::InRoom),
            "checkout" => Some(Self::Checkout),
            "complete" => Some(Self::Complete),
            _ => None,
        }
    }

    fn as_text(self) -> &'static str {
        match self {
            Self::Arrived => "arrived",
            Self::InRoom => "inroom",
            Self::Checkout => "checkout",
            Self::Complete => "complete",
        }
    }
}

#[derive(Clone, Debug)]
struct BoardHold {
    hold_num: u64,
    date: NaiveDate,
    start_min: i32,
    len: i32,
    operatory_num: Id,
    text: String,
    color: Color32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoardDragKind {
    Appointment,
    Hold,
    Waitlist,
    ResizeAppointment,
    ResizeHold,
}

#[derive(Clone, Debug)]
struct BoardDrag {
    kind: BoardDragKind,
    id: u64,
    source_col: usize,
    start_min: i32,
    len: i32,
    preview_start: i32,
    preview_len: i32,
    target_col: usize,
}

#[derive(Clone, Debug)]
enum BoardAction {
    SetView(BoardView),
    SetAnchor(NaiveDate),
    JumpTo(Id),
    OpenNew {
        date: NaiveDate,
        start_min: i32,
        operatory_num: Id,
        provider_num: Id,
        pat_num: Option<Id>,
        hold_id: Option<u64>,
    },
    OpenEdit(Id),
    SaveAppointment(Appointment, String),
    DeleteAppointment(Id),
    CancelVisit(Id),
    SetStage(Id, Option<BoardStage>),
    ToggleAsap(Id),
    ToWaitlist(Id),
    FromWaitlist(Id, NaiveDate, i32, Id),
    AddHold(BoardHold),
    UpdateHold(BoardHold),
    DeleteHold(u64),
    DuplicateHold(BoardHold),
    NextHoldColor(u64),
    EditHold(BoardHold),
    SendMessage,
    SetConfirmed(Id, Id),
    OpenCallLog(Id, Id),
}

#[derive(Clone)]
struct BoardColumn {
    label: String,
    sub: String,
    color: Color32,
    date: NaiveDate,
    operatory_num: Option<Id>,
    provider_num: Id,
}

#[derive(Clone)]
struct BoardBlock {
    apt: Appointment,
    start_min: i32,
    len: i32,
    lane: usize,
    lanes: usize,
}

pub(crate) struct ScheduleBoard {
    view: BoardView,
    anchor: NaiveDate,
    cal_month: NaiveDate,
    show_sidebar: bool,
    search: String,
    holds: Vec<BoardHold>,
    hold_seq: u64,
    stages: HashMap<Id, (BoardStage, chrono::NaiveDateTime)>,
    selected_apt: Option<Id>,
    selected_hold: Option<u64>,
    flash_apt: Option<Id>,
    column_rects: Vec<Rect>,
    pin_rect: Rect,
    drag: Option<BoardDrag>,
    hold_edit: Option<BoardHold>,
    hold_edit_open: bool,
    hold_text: String,
    hold_date_text: String,
    hold_start_text: String,
    hold_len_text: String,
    hold_color_index: usize,
    /// Visible ApptConfirmed definitions for the confirmation submenu.
    confirmation_defs: Vec<Definition>,
    /// Port-local call history shown in appointment hover cards.
    comm_logs: Vec<CommLog>,
    call_log_enabled: bool,
    /// Set when a blockout is converted into an appointment; the appointment
    /// editor's successful save consumes it.
    pub(crate) pending_hold_delete: Option<u64>,
}

impl ScheduleBoard {
    pub(crate) fn load(today: NaiveDate) -> Self {
        let mut board = Self {
            view: BoardView::Day,
            anchor: today,
            cal_month: first_of_month(today),
            show_sidebar: true,
            search: String::new(),
            holds: Vec::new(),
            hold_seq: 0,
            stages: HashMap::new(),
            selected_apt: None,
            selected_hold: None,
            flash_apt: None,
            column_rects: Vec::new(),
            pin_rect: Rect::NOTHING,
            drag: None,
            hold_edit: None,
            hold_edit_open: false,
            hold_text: String::new(),
            hold_date_text: today.format("%Y-%m-%d").to_string(),
            hold_start_text: "09:00".to_string(),
            hold_len_text: "30".to_string(),
            hold_color_index: 0,
            confirmation_defs: Vec::new(),
            comm_logs: Vec::new(),
            call_log_enabled: false,
            pending_hold_delete: None,
        };
        board.load_prefs();
        board
    }

    pub(crate) fn appointment_range(
        &self,
        day: NaiveDate,
    ) -> (chrono::NaiveDateTime, chrono::NaiveDateTime) {
        let from = (day - Duration::days(21))
            .and_hms_opt(0, 0, 0)
            .unwrap_or_default();
        let to = (day + Duration::days(22))
            .and_hms_opt(0, 0, 0)
            .unwrap_or_default();
        (from, to)
    }

    pub(crate) fn remove_hold(&mut self, hold_num: u64) {
        self.holds.retain(|h| h.hold_num != hold_num);
        if self.selected_hold == Some(hold_num) {
            self.selected_hold = None;
        }
        self.save_prefs();
    }

    fn load_prefs(&mut self) {
        let Some(path) = prefs_path() else { return };
        let Ok(text) = fs::read_to_string(path) else {
            return;
        };
        for line in text.lines() {
            let parts: Vec<&str> = line.split('|').collect();
            match parts.as_slice() {
                ["view", value] => self.view = BoardView::from_text(value),
                ["sidebar", value] => self.show_sidebar = value != &"false",
                ["month", value] => {
                    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
                        self.cal_month = first_of_month(date);
                    }
                }
                ["hold", id, date, start, len, op, color, text] => {
                    let parsed = (
                        id.parse::<u64>(),
                        NaiveDate::parse_from_str(date, "%Y-%m-%d"),
                        start.parse::<i32>(),
                        len.parse::<i32>(),
                        op.parse::<Id>(),
                    );
                    let (Ok(hold_num), Ok(date), Ok(start), Ok(len), Ok(op)) = parsed else {
                        continue;
                    };
                    self.holds.push(BoardHold {
                        hold_num,
                        date,
                        start_min: start,
                        len: len.clamp(SNAP_MIN, DAY_END_MIN - start),
                        operatory_num: op,
                        text: unescape(text),
                        color: color_from_hex(color),
                    });
                    self.hold_seq = self.hold_seq.max(hold_num);
                }
                ["stage", apt, stage, since] => {
                    let parsed = (apt.parse::<Id>(), since.parse::<i64>());
                    let (Ok(apt_num), Ok(since)) = parsed else {
                        continue;
                    };
                    if let Some(stage) = BoardStage::from_text(stage) {
                        if let Some(dt) = chrono::DateTime::from_timestamp(since, 0) {
                            self.stages.insert(apt_num, (stage, dt.naive_utc()));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn save_prefs(&self) {
        let Some(path) = prefs_path() else { return };
        let Some(parent) = path.parent() else { return };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        let mut lines = vec![
            format!("view|{}", self.view.as_text()),
            format!("sidebar|{}", self.show_sidebar),
            format!("month|{}", self.cal_month.format("%Y-%m-%d")),
        ];
        for hold in &self.holds {
            lines.push(format!(
                "hold|{}|{}|{}|{}|{}|{}|{}",
                hold.hold_num,
                hold.date.format("%Y-%m-%d"),
                hold.start_min,
                hold.len,
                hold.operatory_num,
                color_to_hex(hold.color),
                escape(&hold.text),
            ));
        }
        for (apt_num, (stage, since)) in &self.stages {
            lines.push(format!(
                "stage|{}|{}|{}",
                apt_num,
                stage.as_text(),
                since.and_utc().timestamp()
            ));
        }
        let _ = fs::write(path, lines.join("\n") + "\n");
    }

    fn set_stage(&mut self, apt_num: Id, stage: Option<BoardStage>) {
        match stage {
            Some(stage) => {
                let since = self
                    .stages
                    .get(&apt_num)
                    .map(|(_, since)| *since)
                    .unwrap_or_else(|| chrono::Local::now().naive_local());
                self.stages.insert(apt_num, (stage, since));
            }
            None => {
                self.stages.remove(&apt_num);
            }
        }
        self.save_prefs();
    }

    fn stage(&self, apt_num: Id) -> Option<BoardStage> {
        self.stages.get(&apt_num).map(|(stage, _)| *stage)
    }

    fn arrived_at(&self, apt_num: Id) -> Option<chrono::NaiveDateTime> {
        self.stages.get(&apt_num).map(|(_, since)| *since)
    }

    fn matches_column(&self, apt: &Appointment, col: &BoardColumn) -> bool {
        if apt.apt_datetime.date() != col.date {
            return false;
        }
        match self.view {
            BoardView::Day => col.operatory_num.is_none_or(|op| apt.operatory_num == op),
            BoardView::Week => true,
        }
    }

    fn holds_for_column(&self, col: &BoardColumn) -> Vec<BoardHold> {
        self.holds
            .iter()
            .filter(|hold| {
                hold.date == col.date && col.operatory_num.is_none_or(|op| hold.operatory_num == op)
            })
            .cloned()
            .collect()
    }
}

pub(crate) fn board_ui(app: &mut OdApp, ui: &mut Ui) {
    let anchor = app.schedule_day;
    app.board.anchor = anchor;
    let view = app.board.view;
    let cols = build_columns(
        view,
        anchor,
        &app.operatories,
        &app.providers,
        &app.appointments,
    );
    let first_operatory = first_operatory(&app.operatories);
    let patients = app.patients.clone();
    let providers = app.providers.clone();
    let appointments = app.appointments.clone();
    let waitlist = app.waitlist.clone();
    let recalls = app.recalls.clone();
    let can_create = app.features.enabled(FeatureKey::AppointmentCreate)
        && app.can(Permission::APPOINTMENT_CREATE);
    let can_edit =
        app.features.enabled(FeatureKey::AppointmentEdit) && app.can(Permission::APPOINTMENT_EDIT);
    let can_delete = app.features.enabled(FeatureKey::AppointmentDelete)
        && app.can(Permission::APPOINTMENT_DELETE);
    let can_hold = can_create;
    let can_message = app.features.enabled(FeatureKey::EServicesEnabled);
    let recall_enabled = app.features.enabled(FeatureKey::RecallsEnabled);
    let status_colors = app.features.enabled(FeatureKey::AppointmentStatusColors);

    let mut actions = Vec::new();
    {
        let board = &mut app.board;
        board.confirmation_defs = app
            .definitions
            .iter()
            .filter(|def| {
                def.category == DefinitionCategory::APPT_CONFIRMED && !def.is_hidden
            })
            .cloned()
            .collect();
        board.comm_logs = app.comm_logs.clone();
        board.call_log_enabled = app.features.enabled(FeatureKey::AppointmentCallLog);
        board.toolbar(
            &patients,
            &providers,
            can_create,
            can_hold,
            can_message,
            ui,
            &mut actions,
        );
        ui.separator();
        board.body(
            &patients,
            &providers,
            &appointments,
            &waitlist,
            &recalls,
            &cols,
            first_operatory,
            recall_enabled,
            can_create,
            can_edit,
            can_delete,
            can_hold,
            status_colors,
            ui,
            &mut actions,
        );
    }
    for action in actions {
        app.apply_board_action(action);
    }
}

impl ScheduleBoard {
    fn toolbar(
        &mut self,
        patients: &[Patient],
        providers: &[Provider],
        can_create: bool,
        can_hold: bool,
        can_message: bool,
        ui: &mut Ui,
        actions: &mut Vec<BoardAction>,
    ) {
        egui::Frame::new()
            .fill(theme::panel())
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let step = match self.view {
                        BoardView::Day => 1,
                        BoardView::Week => 7,
                    };
                    if ui.small_button("‹").clicked() {
                        actions.push(BoardAction::SetAnchor(self.anchor - Duration::days(step)));
                    }
                    if ui.button("Today").clicked() {
                        actions.push(BoardAction::SetAnchor(Local::now().date_naive()));
                    }
                    if ui.small_button("›").clicked() {
                        actions.push(BoardAction::SetAnchor(self.anchor + Duration::days(step)));
                    }
                    let label = match self.view {
                        BoardView::Day => self.anchor.format("%a, %b %d, %Y").to_string(),
                        BoardView::Week => {
                            let days = days_of_week(self.anchor);
                            format!(
                                "{} {} – {} {}",
                                days[0].format("%b %d"),
                                days[0].year(),
                                days[6].format("%b %d"),
                                days[6].year()
                            )
                        }
                    };
                    ui.label(RichText::new(label).strong().size(15.0).color(theme::ink()));
                    ui.add_space(8.0);
                    for view in [BoardView::Day, BoardView::Week] {
                        if ui
                            .selectable_label(self.view == view, view.label())
                            .clicked()
                        {
                            actions.push(BoardAction::SetView(view));
                        }
                    }
                    ui.separator();
                    let search_response = ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("Search patient…")
                            .desired_width(170.0),
                    );
                    if !self.search.trim().is_empty() {
                        let query = self.search.trim().to_lowercase();
                        let matches: Vec<&Patient> = patients
                            .iter()
                            .filter(|p| {
                                p.display_name().to_lowercase().contains(&query)
                                    || p.chart_number.to_lowercase().contains(&query)
                                    || p.home_phone.contains(&query)
                                    || p.wireless_phone.contains(&query)
                                    || p.wk_phone.contains(&query)
                            })
                            .take(8)
                            .collect();
                        if !matches.is_empty() {
                            let ctx = ui.ctx().clone();
                            let action_target = &mut *actions;
                            egui::Window::new("board_patient_search")
                                .anchor(
                                    Align2::LEFT_TOP,
                                    Vec2::new(
                                        search_response.rect.left_bottom().x,
                                        search_response.rect.left_bottom().y + 3.0,
                                    ),
                                )
                                .resizable(false)
                                .collapsible(false)
                                .title_bar(false)
                                .show(&ctx, |ui| {
                                    for patient in matches {
                                        let note = if patient.chart_number.is_empty() {
                                            patient.home_phone.clone()
                                        } else {
                                            format!(
                                                "Chart #{} · {}",
                                                patient.chart_number, patient.home_phone
                                            )
                                        };
                                        if ui
                                            .button(format!(
                                                "{}  ·  {}",
                                                patient.display_name(),
                                                note
                                            ))
                                            .clicked()
                                        {
                                            action_target
                                                .push(BoardAction::JumpTo(patient.pat_num));
                                            self.search.clear();
                                        }
                                    }
                                });
                        }
                    }
                    ui.separator();
                    let palette = provider_palette();
                    for (index, provider) in providers.iter().take(6).enumerate() {
                        let color = palette[index % palette.len()];
                        ui.horizontal(|ui| {
                            ui.painter().circle_filled(
                                ui.cursor().min + Vec2::new(4.0, 6.0),
                                4.0,
                                color,
                            );
                            ui.add_space(9.0);
                            ui.label(
                                RichText::new(&provider.abbreviation)
                                    .small()
                                    .color(theme::muted()),
                            );
                        });
                    }
                    ui.separator();
                    ui.checkbox(&mut self.show_sidebar, "Calendar & Recalls");
                    if can_hold && ui.button("Blockout").clicked() {
                        self.open_hold_editor(
                            None,
                            self.anchor,
                            9 * 60,
                            providers.first().map(|p| p.provider_num).unwrap_or(0),
                        );
                    }
                    if can_create && ui.button("+ New appt").clicked() {
                        actions.push(BoardAction::OpenNew {
                            date: self.anchor,
                            start_min: 9 * 60,
                            operatory_num: 0,
                            provider_num: providers.first().map(|p| p.provider_num).unwrap_or(0),
                            pat_num: None,
                            hold_id: None,
                        });
                    }
                    if can_message && ui.button("Send message").clicked() {
                        actions.push(BoardAction::SendMessage);
                    }
                });
            });
        ui.add_space(8.0);
    }

    #[allow(clippy::too_many_arguments)]
    fn body(
        &mut self,
        patients: &[Patient],
        providers: &[Provider],
        appointments: &[Appointment],
        waitlist: &[Appointment],
        recalls: &[Recall],
        cols: &[BoardColumn],
        first_operatory: Id,
        recall_enabled: bool,
        can_create: bool,
        can_edit: bool,
        can_delete: bool,
        can_hold: bool,
        status_colors: bool,
        ui: &mut Ui,
        actions: &mut Vec<BoardAction>,
    ) {
        let available_height = ui.available_height().max(360.0);
        let pin_width = 168.0;
        let side_width = if self.show_sidebar { 258.0 } else { 0.0 };
        let total_width = ui.available_width();
        let grid_width = (total_width - pin_width - side_width - 12.0).max(360.0);

        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(pin_width, available_height),
                Layout::top_down(Align::Min),
                |ui| {
                    self.pinboard_ui(
                        patients,
                        waitlist,
                        cols,
                        first_operatory,
                        can_create,
                        can_edit,
                        ui,
                        actions,
                    );
                },
            );
            ui.add_space(6.0);
            ui.allocate_ui_with_layout(
                Vec2::new(grid_width, available_height),
                Layout::top_down(Align::Min),
                |ui| {
                    self.grid_ui(
                        patients,
                        providers,
                        appointments,
                        cols,
                        first_operatory,
                        can_create,
                        can_edit,
                        can_delete,
                        can_hold,
                        status_colors,
                        ui,
                        actions,
                    );
                },
            );
            if self.show_sidebar {
                ui.add_space(6.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(side_width, available_height),
                    Layout::top_down(Align::Min),
                    |ui| {
                        self.sidebar_ui(
                            patients,
                            providers,
                            appointments,
                            recalls,
                            recall_enabled,
                            can_create,
                            ui,
                            actions,
                        );
                    },
                );
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn pinboard_ui(
        &mut self,
        patients: &[Patient],
        waitlist: &[Appointment],
        cols: &[BoardColumn],
        first_operatory: Id,
        can_create: bool,
        can_edit: bool,
        ui: &mut Ui,
        actions: &mut Vec<BoardAction>,
    ) {
        self.pin_rect = ui.available_rect_before_wrap();
        egui::Frame::new()
            .fill(ui.visuals().panel_fill)
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .corner_radius(8.0)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Pin board");
                    ui.weak(format!("({})", waitlist.len()));
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if waitlist.is_empty() {
                        ui.weak("No patients pinned. Move an appointment here to unschedule it.");
                        return;
                    }
                    for apt in waitlist {
                        let name = patient_name(patients, apt.pat_num);
                        let response = ui.add(
                            egui::Button::new(RichText::new(&name).strong())
                                .sense(Sense::click_and_drag()),
                        );
                        let response = response.on_hover_ui(|ui| {
                            ui.strong(&name);
                            ui.label(format!("Patient #{}", apt.pat_num));
                            ui.label(format!("Held length: {} min", apt.apt_length));
                            if !apt.notes.is_empty() {
                                ui.label(&apt.notes);
                            }
                        });
                        if response.drag_started() {
                            self.drag = Some(BoardDrag {
                                kind: BoardDragKind::Waitlist,
                                id: apt.apt_num as u64,
                                source_col: 0,
                                start_min: 9 * 60,
                                len: apt.apt_length as i32,
                                preview_start: 9 * 60,
                                preview_len: apt.apt_length as i32,
                                target_col: 0,
                            });
                        }
                        if let Some(drag) = self.drag.as_mut() {
                            if drag.kind == BoardDragKind::Waitlist && drag.id == apt.apt_num as u64
                            {
                                if let Some(pointer) = ui.ctx().pointer_latest_pos() {
                                    drag.target_col = self
                                        .column_rects
                                        .iter()
                                        .position(|rect| rect.contains(pointer))
                                        .unwrap_or(drag.target_col);
                                }
                            }
                        }
                        if response.drag_stopped() {
                            if let Some(drag) = self.drag.take() {
                                if let Some(col) = cols.get(drag.target_col) {
                                    let operatory = col.operatory_num.unwrap_or(first_operatory);
                                    actions.push(BoardAction::FromWaitlist(
                                        apt.apt_num,
                                        col.date,
                                        drag.preview_start,
                                        operatory,
                                    ));
                                }
                            }
                        }
                        response.context_menu(|ui| {
                            if can_create && ui.button("Schedule at 9:00").clicked() {
                                actions.push(BoardAction::FromWaitlist(
                                    apt.apt_num,
                                    self.anchor,
                                    9 * 60,
                                    first_operatory,
                                ));
                            }
                            if can_edit && ui.button("Edit appointment").clicked() {
                                actions.push(BoardAction::OpenEdit(apt.apt_num));
                            }
                        });
                        ui.horizontal(|ui| {
                            if can_create && ui.small_button("Schedule").clicked() {
                                actions.push(BoardAction::FromWaitlist(
                                    apt.apt_num,
                                    self.anchor,
                                    9 * 60,
                                    first_operatory,
                                ));
                            }
                            if can_edit && ui.small_button("Edit").clicked() {
                                actions.push(BoardAction::OpenEdit(apt.apt_num));
                            }
                        });
                        ui.add_space(3.0);
                    }
                });
            });
    }

    #[allow(clippy::too_many_arguments)]
    fn grid_ui(
        &mut self,
        patients: &[Patient],
        providers: &[Provider],
        appointments: &[Appointment],
        cols: &[BoardColumn],
        first_operatory: Id,
        can_create: bool,
        can_edit: bool,
        can_delete: bool,
        can_hold: bool,
        status_colors: bool,
        ui: &mut Ui,
        actions: &mut Vec<BoardAction>,
    ) {
        if cols.is_empty() {
            ui.label("No visible operatories. Add rows to the operatory table.");
            return;
        }
        self.column_rects.clear();
        let confirmation_defs = self.confirmation_defs.clone();
        let comm_logs = self.comm_logs.clone();
        let call_log_enabled = self.call_log_enabled;
        let col_width = ((ui.available_width() - GUTTER_WIDTH) / cols.len() as f32).max(120.0);
        let grid_height = (DAY_END_MIN - DAY_START_MIN) as f32 * PIXELS_PER_MIN;
        let full_width = GUTTER_WIDTH + col_width * cols.len() as f32;

        egui::ScrollArea::both()
            .id_salt("scheduling_board_grid")
            .show(ui, |ui| {
                let (area, _) = ui.allocate_exact_size(
                    Vec2::new(full_width, HEADER_HEIGHT + grid_height + 8.0),
                    Sense::hover(),
                );
                let header = Rect::from_min_size(area.min, Vec2::new(full_width, HEADER_HEIGHT));
                let body = Rect::from_min_size(
                    area.min + Vec2::new(0.0, HEADER_HEIGHT),
                    Vec2::new(full_width, grid_height),
                );
                ui.painter()
                    .rect_filled(header, 4.0, ui.visuals().panel_fill);
                ui.painter().line_segment(
                    [header.left_bottom(), header.right_bottom()],
                    ui.visuals().widgets.noninteractive.bg_stroke,
                );
                for (index, col) in cols.iter().enumerate() {
                    let x = GUTTER_WIDTH + index as f32 * col_width;
                    ui.painter().circle_filled(
                        header.min + Vec2::new(x + 11.0, 20.0),
                        4.5,
                        col.color,
                    );
                    ui.painter().text(
                        header.min + Vec2::new(x + 21.0, 11.0),
                        Align2::LEFT_TOP,
                        &col.label,
                        egui::FontId::proportional(12.0),
                        ui.visuals().text_color(),
                    );
                    if !col.sub.is_empty() {
                        ui.painter().text(
                            header.min + Vec2::new(x + 21.0, 26.0),
                            Align2::LEFT_TOP,
                            &col.sub,
                            egui::FontId::proportional(10.0),
                            ui.visuals().weak_text_color(),
                        );
                    }
                }

                for minute in (DAY_START_MIN..=DAY_END_MIN).step_by(15) {
                    let y = body.top() + (minute - DAY_START_MIN) as f32 * PIXELS_PER_MIN;
                    let base = ui.visuals().widgets.noninteractive.bg_stroke.color;
                    let color = if minute % 60 == 0 {
                        base
                    } else {
                        faded(base, 0.55)
                    };
                    ui.painter().line_segment(
                        [
                            Pos2::new(body.left() + GUTTER_WIDTH, y),
                            Pos2::new(body.right(), y),
                        ],
                        Stroke::new(1.0, color),
                    );
                    if minute % 60 == 0 {
                        ui.painter().text(
                            Pos2::new(body.left(), y),
                            Align2::RIGHT_TOP,
                            format_time(minute),
                            egui::FontId::proportional(10.0),
                            ui.visuals().weak_text_color(),
                        );
                    }
                }

                for (index, col) in cols.iter().enumerate() {
                    let x = body.left() + GUTTER_WIDTH + index as f32 * col_width;
                    let col_rect = Rect::from_min_size(
                        body.min + Vec2::new(GUTTER_WIDTH + index as f32 * col_width, 0.0),
                        Vec2::new(col_width, grid_height),
                    );
                    self.column_rects.push(col_rect);
                    ui.painter().rect_filled(
                        col_rect,
                        0.0,
                        ui.visuals().panel_fill.gamma_multiply(0.995),
                    );
                    ui.painter().line_segment(
                        [col_rect.right_top(), col_rect.right_bottom()],
                        Stroke::new(
                            1.0,
                            faded(ui.visuals().widgets.noninteractive.bg_stroke.color, 0.7),
                        ),
                    );

                    let column_response = ui.interact(
                        col_rect,
                        ui.scope_id().with("board_column").with(index),
                        Sense::click(),
                    );
                    if column_response.double_clicked() && can_create {
                        if let Some(start) = pointer_minute(ui, body) {
                            actions.push(BoardAction::OpenNew {
                                date: col.date,
                                start_min: start,
                                operatory_num: col.operatory_num.unwrap_or(first_operatory),
                                provider_num: col.provider_num,
                                pat_num: None,
                                hold_id: None,
                            });
                        }
                    }
                    if can_hold {
                        column_response.context_menu(|ui| {
                            if ui.button("Add blockout here…").clicked() {
                                let start = pointer_minute(ui, body).unwrap_or(9 * 60);
                                self.open_hold_editor(None, col.date, start, col.provider_num);
                            }
                        });
                    }

                    for hold in self.holds_for_column(col) {
                        let top =
                            body.top() + (hold.start_min - DAY_START_MIN) as f32 * PIXELS_PER_MIN;
                        let rect = Rect::from_min_size(
                            Pos2::new(x + 4.0, top),
                            Vec2::new(
                                col_width - 8.0,
                                (hold.len as f32 * PIXELS_PER_MIN).max(20.0),
                            ),
                        );
                        let text = if hold.text.is_empty() {
                            "Hold"
                        } else {
                            &hold.text
                        };
                        let response = ui.put(
                            rect,
                            egui::Button::new(RichText::new(text).small())
                                .fill(hold.color.gamma_multiply(0.75))
                                .sense(Sense::click_and_drag()),
                        );
                        if response.drag_started() {
                            self.drag = Some(BoardDrag {
                                kind: BoardDragKind::Hold,
                                id: hold.hold_num,
                                source_col: index,
                                start_min: hold.start_min,
                                len: hold.len,
                                preview_start: hold.start_min,
                                preview_len: hold.len,
                                target_col: index,
                            });
                        }
                        if let Some(drag) = self.drag.as_mut() {
                            if drag.kind == BoardDragKind::Hold && drag.id == hold.hold_num {
                                let delta = response.drag_delta();
                                drag.preview_start = snap_minute(
                                    drag.start_min + (delta.y / PIXELS_PER_MIN).round() as i32,
                                );
                                if let Some(pointer) = ui.ctx().pointer_latest_pos() {
                                    drag.target_col = self
                                        .column_rects
                                        .iter()
                                        .position(|r| r.contains(pointer))
                                        .unwrap_or(drag.target_col);
                                }
                            }
                        }
                        if response.drag_stopped() {
                            if let Some(drag) = self.drag.take() {
                                let target = cols.get(drag.target_col).unwrap_or(col);
                                let mut updated = hold.clone();
                                updated.start_min = drag.preview_start;
                                updated.date = target.date;
                                updated.operatory_num =
                                    target.operatory_num.unwrap_or(first_operatory);
                                actions.push(BoardAction::UpdateHold(updated));
                            }
                        }
                        response.context_menu(|ui| {
                            if can_hold && ui.button("Edit text…").clicked() {
                                actions.push(BoardAction::EditHold(hold.clone()));
                            }
                            if can_hold && ui.button("Next colour").clicked() {
                                actions.push(BoardAction::NextHoldColor(hold.hold_num));
                            }
                            if can_hold && ui.button("Book as appointment…").clicked() {
                                actions.push(BoardAction::OpenNew {
                                    date: hold.date,
                                    start_min: hold.start_min,
                                    operatory_num: hold.operatory_num,
                                    provider_num: col.provider_num,
                                    pat_num: None,
                                    hold_id: Some(hold.hold_num),
                                });
                            }
                            if can_hold && ui.button("Duplicate").clicked() {
                                actions.push(BoardAction::DuplicateHold(hold.clone()));
                            }
                            if can_hold && ui.button("Delete blockout").clicked() {
                                actions.push(BoardAction::DeleteHold(hold.hold_num));
                            }
                        });

                        let resize_rect = Rect::from_min_size(
                            Pos2::new(rect.left() + 3.0, rect.bottom() - 6.0),
                            Vec2::new(rect.width() - 6.0, 6.0),
                        );
                        let resize = ui.put(
                            resize_rect,
                            egui::Button::new("")
                                .fill(Color32::TRANSPARENT)
                                .sense(Sense::drag()),
                        );
                        if resize.drag_started() {
                            self.drag = Some(BoardDrag {
                                kind: BoardDragKind::ResizeHold,
                                id: hold.hold_num,
                                source_col: index,
                                start_min: hold.start_min,
                                len: hold.len,
                                preview_start: hold.start_min,
                                preview_len: hold.len,
                                target_col: index,
                            });
                        }
                        if let Some(drag) = self.drag.as_mut() {
                            if drag.kind == BoardDragKind::ResizeHold && drag.id == hold.hold_num {
                                drag.preview_len = resize_len(drag, resize.drag_delta());
                            }
                        }
                        if resize.drag_stopped() {
                            if let Some(drag) = self.drag.take() {
                                let mut updated = hold.clone();
                                updated.len = drag.preview_len;
                                actions.push(BoardAction::UpdateHold(updated));
                            }
                        }
                    }

                    let in_column: Vec<Appointment> = appointments
                        .iter()
                        .filter(|a| !is_waitlist(a) && self.matches_column(a, col))
                        .cloned()
                        .collect();
                    for block in lane_blocks(in_column) {
                        let apt = block.apt;
                        let lane_width = (col_width - 6.0) / block.lanes as f32;
                        let top =
                            body.top() + (block.start_min - DAY_START_MIN) as f32 * PIXELS_PER_MIN;
                        let rect = Rect::from_min_size(
                            Pos2::new(x + 3.0 + block.lane as f32 * lane_width, top),
                            Vec2::new(
                                lane_width - 1.0,
                                (block.len as f32 * PIXELS_PER_MIN).max(20.0),
                            ),
                        );
                        let provider_color = palette_color(providers, apt.provider_num, index);
                        let stage = self.stage(apt.apt_num);
                        let stroke_color = stage.map(|s| s.color()).unwrap_or(if status_colors {
                            status_color(apt.apt_status)
                        } else {
                            provider_color
                        });
                        let stroke_width = if self.selected_apt == Some(apt.apt_num) {
                            2.5
                        } else if stage.is_some() {
                            2.0
                        } else {
                            1.0
                        };
                        let name = patient_name(patients, apt.pat_num);
                        let mut title = String::new();
                        if apt.priority == RecallPriority::Asap {
                            title.push_str("▲ ");
                        }
                        title.push_str(&name);
                        if let Some(stage) = stage {
                            title.push_str(&format!(" · {}", stage.label()));
                        }
                        let detail = format!(
                            "{}–{} · {} min",
                            format_hhmm(block.start_min),
                            format_hhmm(block.start_min + block.len),
                            block.len
                        );
                        let text = if rect.height() > 34.0 {
                            format!("{title}\n{detail}")
                        } else {
                            title.clone()
                        };
                        let response = ui.put(
                            rect,
                            egui::Button::new(RichText::new(text).small())
                                .fill(blend(provider_color, 0.68))
                                .stroke(Stroke::new(stroke_width, stroke_color))
                                .sense(Sense::click_and_drag()),
                        );
                        let hover_name = name.clone();
                        let hover_apt = apt.clone();
                        let hover_logs = comm_logs.clone();
                        let hover_defs = confirmation_defs.clone();
                        let response = response.on_hover_ui(move |ui| {
                            ui.strong(&hover_name);
                            ui.label(format!(
                                "Patient #{} · {}–{}",
                                hover_apt.pat_num,
                                format_time(block.start_min),
                                format_time(block.start_min + block.len)
                            ));
                            ui.label(format!("Length: {} min", block.len));
                            ui.label(format!("Status: {}", hover_apt.apt_status.label()));
                            let confirmed = hover_defs
                                .iter()
                                .find(|def| def.def_num == hover_apt.confirmed)
                                .map(|def| def.item_name.clone())
                                .unwrap_or_else(|| {
                                    if hover_apt.confirmed == 0 {
                                        "Not confirmed".into()
                                    } else {
                                        format!("Definition #{}", hover_apt.confirmed)
                                    }
                                });
                            ui.label(format!("Confirmation: {confirmed}"));
                            let recent: Vec<&CommLog> = hover_logs
                                .iter()
                                .filter(|log| log.apt_num == hover_apt.apt_num)
                                .take(3)
                                .collect();
                            if !recent.is_empty() {
                                ui.label("Recent calls");
                                for log in recent {
                                    ui.small(format!(
                                        "{} · {} · {}",
                                        log.comm_date_time.format("%b %d %H:%M"),
                                        log.mode.label(),
                                        log.note
                                    ));
                                }
                            }
                            if !hover_apt.notes.is_empty() {
                                ui.label("Notes");
                                ui.label(&hover_apt.notes);
                            }
                        });
                        if response.clicked() {
                            self.selected_apt = Some(apt.apt_num);
                            self.selected_hold = None;
                        }
                        if response.double_clicked() && can_edit {
                            actions.push(BoardAction::OpenEdit(apt.apt_num));
                        }
                        if response.drag_started() {
                            self.drag = Some(BoardDrag {
                                kind: BoardDragKind::Appointment,
                                id: apt.apt_num as u64,
                                source_col: index,
                                start_min: block.start_min,
                                len: block.len,
                                preview_start: block.start_min,
                                preview_len: block.len,
                                target_col: index,
                            });
                        }
                        if let Some(drag) = self.drag.as_mut() {
                            if drag.kind == BoardDragKind::Appointment
                                && drag.id == apt.apt_num as u64
                            {
                                let delta = response.drag_delta();
                                drag.preview_start = snap_minute(
                                    drag.start_min + (delta.y / PIXELS_PER_MIN).round() as i32,
                                );
                                if let Some(pointer) = ui.ctx().pointer_latest_pos() {
                                    if self.pin_rect.contains(pointer) {
                                        drag.target_col = usize::MAX;
                                    } else {
                                        drag.target_col = self
                                            .column_rects
                                            .iter()
                                            .position(|r| r.contains(pointer))
                                            .unwrap_or(drag.source_col);
                                    }
                                }
                            }
                        }
                        if response.drag_stopped() {
                            if let Some(drag) = self.drag.take() {
                                if drag.target_col == usize::MAX {
                                    actions.push(BoardAction::ToWaitlist(apt.apt_num));
                                } else {
                                    let target = cols.get(drag.target_col).unwrap_or(col);
                                    let mut updated = apt.clone();
                                    updated.apt_datetime = target
                                        .date
                                        .and_hms_opt(
                                            (drag.preview_start / 60) as u32,
                                            (drag.preview_start % 60) as u32,
                                            0,
                                        )
                                        .unwrap_or_default();
                                    if let Some(op) = target.operatory_num {
                                        updated.operatory_num = op;
                                    }
                                    updated.provider_num = target.provider_num;
                                    actions.push(BoardAction::SaveAppointment(
                                        updated,
                                        format!("Appointment {} moved on the board", apt.apt_num),
                                    ));
                                }
                            }
                        }
                        response.context_menu(|ui| {
                            if can_edit {
                                match stage {
                                    None => {
                                        if ui.button("Arrived — add to waiting list").clicked() {
                                            actions.push(BoardAction::SetStage(
                                                apt.apt_num,
                                                Some(BoardStage::Arrived),
                                            ));
                                        }
                                    }
                                    Some(current) => {
                                        if let Some(next) = current.next() {
                                            if ui.button(next.label()).clicked() {
                                                actions.push(BoardAction::SetStage(
                                                    apt.apt_num,
                                                    Some(next),
                                                ));
                                            }
                                        }
                                        if ui
                                            .button(format!("Cancel {}", current.label()))
                                            .clicked()
                                        {
                                            actions.push(BoardAction::SetStage(apt.apt_num, None));
                                        }
                                    }
                                }
                                if ui.button("Reschedule…").clicked() {
                                    actions.push(BoardAction::OpenEdit(apt.apt_num));
                                }
                                if call_log_enabled && !confirmation_defs.is_empty() {
                                    ui.menu_button("Mark confirmed", |ui| {
                                        for def in &confirmation_defs {
                                            if ui.button(&def.item_name).clicked() {
                                                actions.push(BoardAction::SetConfirmed(
                                                    apt.apt_num,
                                                    def.def_num,
                                                ));
                                            }
                                        }
                                    });
                                }
                                if call_log_enabled && ui.button("Log call / confirmation…").clicked() {
                                    actions.push(BoardAction::OpenCallLog(apt.apt_num, apt.pat_num));
                                }
                                if ui
                                    .button(if apt.priority == RecallPriority::Asap {
                                        "Clear ASAP"
                                    } else {
                                        "Toggle ASAP"
                                    })
                                    .clicked()
                                {
                                    actions.push(BoardAction::ToggleAsap(apt.apt_num));
                                }
                                let longer = apt.clone();
                                if ui.button("+15 min").clicked() {
                                    let mut longer = longer;
                                    longer.apt_length = longer.apt_length.saturating_add(15);
                                    longer.pattern = pattern_for_length(longer.apt_length);
                                    actions.push(BoardAction::SaveAppointment(
                                        longer,
                                        format!("Appointment {} lengthened", apt.apt_num),
                                    ));
                                }
                                let shorter = apt.clone();
                                if ui.button("−15 min").clicked() {
                                    let mut shorter = shorter;
                                    shorter.apt_length =
                                        shorter.apt_length.saturating_sub(15).max(15);
                                    shorter.pattern = pattern_for_length(shorter.apt_length);
                                    actions.push(BoardAction::SaveAppointment(
                                        shorter,
                                        format!("Appointment {} shortened", apt.apt_num),
                                    ));
                                }
                                if ui.button("Send to pin board").clicked() {
                                    actions.push(BoardAction::ToWaitlist(apt.apt_num));
                                }
                                if apt.apt_status != AppointmentStatus::Broken
                                    && ui
                                        .button("Cancel visit (Broken)")
                                        .on_hover_text(
                                            "OD has no Cancelled status; this saves AptStatus = Broken and keeps the history.",
                                        )
                                        .clicked()
                                {
                                    actions.push(BoardAction::CancelVisit(apt.apt_num));
                                }
                                ui.separator();
                                if ui.button("Edit…").clicked() {
                                    actions.push(BoardAction::OpenEdit(apt.apt_num));
                                }
                            }
                            if can_delete && ui.button("Delete").clicked() {
                                actions.push(BoardAction::DeleteAppointment(apt.apt_num));
                            }
                        });

                        let resize_rect = Rect::from_min_size(
                            Pos2::new(rect.left() + 3.0, rect.bottom() - 6.0),
                            Vec2::new(rect.width() - 6.0, 6.0),
                        );
                        let resize = ui.put(
                            resize_rect,
                            egui::Button::new("")
                                .fill(Color32::TRANSPARENT)
                                .sense(Sense::drag()),
                        );
                        if resize.drag_started() {
                            self.drag = Some(BoardDrag {
                                kind: BoardDragKind::ResizeAppointment,
                                id: apt.apt_num as u64,
                                source_col: index,
                                start_min: block.start_min,
                                len: block.len,
                                preview_start: block.start_min,
                                preview_len: block.len,
                                target_col: index,
                            });
                        }
                        if let Some(drag) = self.drag.as_mut() {
                            if drag.kind == BoardDragKind::ResizeAppointment
                                && drag.id == apt.apt_num as u64
                            {
                                drag.preview_len = resize_len(drag, resize.drag_delta());
                            }
                        }
                        if resize.drag_stopped() {
                            if let Some(drag) = self.drag.take() {
                                let mut updated = apt.clone();
                                updated.apt_length = drag.preview_len.max(15) as u32;
                                updated.pattern = pattern_for_length(updated.apt_length);
                                actions.push(BoardAction::SaveAppointment(
                                    updated,
                                    format!(
                                        "Appointment {} length changed on the board",
                                        apt.apt_num
                                    ),
                                ));
                            }
                        }
                    }
                }
                if let Some(flash) = self.flash_apt {
                    if self.selected_apt == Some(flash) {
                        // Selection is the visible feedback; the next frame clears
                        // the flash flag so the grid does not scroll repeatedly.
                        self.flash_apt = None;
                    }
                }
            });
    }

    fn sidebar_ui(
        &mut self,
        patients: &[Patient],
        providers: &[Provider],
        appointments: &[Appointment],
        recalls: &[Recall],
        recall_enabled: bool,
        can_create: bool,
        ui: &mut Ui,
        actions: &mut Vec<BoardAction>,
    ) {
        egui::Frame::new()
            .fill(ui.visuals().panel_fill)
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .corner_radius(8.0)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.small_button("‹").clicked() {
                        self.cal_month = add_months(self.cal_month, -1);
                    }
                    ui.strong(self.cal_month.format("%B %Y").to_string());
                    if ui.small_button("›").clicked() {
                        self.cal_month = add_months(self.cal_month, 1);
                    }
                });
                let mut counts: HashMap<NaiveDate, usize> = HashMap::new();
                for apt in appointments {
                    if !is_waitlist(apt) {
                        *counts.entry(apt.apt_datetime.date()).or_default() += 1;
                    }
                }
                egui::Grid::new("board_mini_calendar")
                    .spacing([2.0, 2.0])
                    .show(ui, |ui| {
                        for day in ["S", "M", "T", "W", "T", "F", "S"] {
                            ui.centered_and_justified(|ui| ui.small(day));
                        }
                        ui.end_row();
                        let first = self.cal_month.with_day(1).unwrap_or(self.cal_month);
                        let start =
                            first - Duration::days(first.weekday().num_days_from_sunday() as i64);
                        for index in 0..42 {
                            let date = start + Duration::days(index);
                            let other = date.month() != self.cal_month.month();
                            let selected = date == self.anchor;
                            let today = date == Local::now().date_naive();
                            let count = counts.get(&date).copied().unwrap_or(0);
                            let label = if count == 0 {
                                date.day().to_string()
                            } else {
                                format!("{} ·", date.day())
                            };
                            let button = egui::Button::new(label).min_size(Vec2::new(26.0, 18.0));
                            let response = ui.add(button);
                            if count > 0 {
                                painter_dot(ui, response.rect, ui.visuals().text_color());
                            }
                            if other {
                                ui.painter().rect_filled(
                                    response.rect,
                                    2.0,
                                    faded(theme::muted(), 0.12),
                                );
                            }
                            if selected || today {
                                ui.painter().rect_stroke(
                                    response.rect,
                                    2.0,
                                    Stroke::new(
                                        if selected { 2.0 } else { 1.0 },
                                        if selected {
                                            ui.visuals().selection.stroke.color
                                        } else {
                                            ui.visuals().weak_text_color()
                                        },
                                    ),
                                    StrokeKind::Middle,
                                );
                            }
                            if response.clicked() {
                                actions.push(BoardAction::SetAnchor(date));
                            }
                            if (index + 1) % 7 == 0 {
                                ui.end_row();
                            }
                        }
                    });
                ui.separator();

                ui.strong("Waiting room");
                let waiting: Vec<&Appointment> = appointments
                    .iter()
                    .filter(|a| {
                        !is_waitlist(a)
                            && a.apt_datetime.date() == self.anchor
                            && self.stage(a.apt_num).is_some()
                            && self.stage(a.apt_num) != Some(BoardStage::Complete)
                    })
                    .collect();
                if waiting.is_empty() {
                    ui.weak("No patients checked in.");
                }
                for apt in waiting {
                    let stage = self.stage(apt.apt_num).unwrap_or(BoardStage::Arrived);
                    let name = patient_name(patients, apt.pat_num);
                    let provider = providers
                        .iter()
                        .find(|p| p.provider_num == apt.provider_num)
                        .map(|p| p.abbreviation.clone())
                        .unwrap_or_default();
                    let elapsed = self
                        .arrived_at(apt.apt_num)
                        .map(|since| {
                            let mins = (chrono::Local::now().naive_local() - since)
                                .num_minutes()
                                .max(0);
                            if mins < 60 {
                                format!("{mins}m")
                            } else {
                                format!("{}h {}m", mins / 60, mins % 60)
                            }
                        })
                        .unwrap_or_default();
                    egui::Frame::new()
                        .fill(theme::paper())
                        .stroke(egui::Stroke::new(1.0, theme::line()))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::same(7))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.strong(&name);
                                    ui.label(
                                        RichText::new(format!(
                                            "{provider} · {} · {elapsed}",
                                            format_hhmm(apt_time_minute(apt))
                                        ))
                                        .small()
                                        .color(theme::muted()),
                                    );
                                });
                                ui.colored_label(stage.color(), stage.label());
                            });
                            ui.horizontal(|ui| {
                                if let Some(next) = stage.next() {
                                    if ui.small_button(next.label()).clicked() {
                                        actions
                                            .push(BoardAction::SetStage(apt.apt_num, Some(next)));
                                    }
                                }
                                if ui.small_button("Clear").clicked() {
                                    actions.push(BoardAction::SetStage(apt.apt_num, None));
                                }
                            });
                        });
                    ui.add_space(4.0);
                }
                ui.separator();

                if recall_enabled {
                    ui.strong("Recall list");
                    let today = Local::now().date_naive();
                    let mut due: Vec<&Recall> = recalls.iter().collect();
                    due.sort_by_key(|r| (recall_due_state(r, today).sort_rank(), r.pat_num));
                    if due.is_empty() {
                        ui.weak("No recalls due.");
                    }
                    for recall in due.into_iter().take(8) {
                        let state = recall_due_state(recall, today);
                        let name = patient_name(patients, recall.pat_num);
                        egui::Frame::new()
                            .fill(theme::paper())
                            .stroke(egui::Stroke::new(1.0, theme::line()))
                            .corner_radius(egui::CornerRadius::same(6))
                            .inner_margin(egui::Margin::same(7))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.strong(&name);
                                        ui.label(
                                            RichText::new(format!("Patient #{}", recall.pat_num))
                                                .small()
                                                .color(theme::muted()),
                                        );
                                    });
                                    ui.colored_label(state_color(state), state.label());
                                    if can_create && ui.small_button("Schedule").clicked() {
                                        actions.push(BoardAction::OpenNew {
                                            date: recall_due_date(recall).unwrap_or(today),
                                            start_min: 9 * 60,
                                            operatory_num: 0,
                                            provider_num: providers
                                                .first()
                                                .map(|p| p.provider_num)
                                                .unwrap_or(0),
                                            pat_num: Some(recall.pat_num),
                                            hold_id: None,
                                        });
                                    }
                                });
                            });
                        ui.add_space(4.0);
                    }
                }
            });
    }

    fn open_hold_editor(
        &mut self,
        hold: Option<BoardHold>,
        date: NaiveDate,
        start_min: i32,
        _provider_num: Id,
    ) {
        let hold = hold.unwrap_or_else(|| BoardHold {
            hold_num: 0,
            date,
            start_min,
            len: 30,
            operatory_num: 0,
            text: String::new(),
            color: hold_colors()[0],
        });
        self.hold_text = hold.text.clone();
        self.hold_date_text = hold.date.format("%Y-%m-%d").to_string();
        self.hold_start_text = format_hhmm(hold.start_min);
        self.hold_len_text = hold.len.to_string();
        self.hold_color_index = hold_colors()
            .iter()
            .position(|c| *c == hold.color)
            .unwrap_or(0);
        self.hold_edit = Some(hold);
        self.hold_edit_open = true;
    }
}

pub(crate) fn hold_window(app: &mut OdApp, ctx: &egui::Context) {
    if !app.board.hold_edit_open {
        return;
    }
    let Some(hold) = app.board.hold_edit.clone() else {
        return;
    };
    let is_edit = hold.hold_num != 0;
    let mut save = false;
    let mut delete = false;
    let mut cancel = false;
    egui::Window::new(if is_edit {
        "Edit blockout"
    } else {
        "New blockout"
    })
    .open(&mut app.board.hold_edit_open)
    .resizable(false)
    .default_width(420.0)
    .show(ctx, |ui| {
        ui.weak(
            "Free-text hold — reserve a slot for a call-in or a patient not yet on the schedule.",
        );
        ui.label("Text");
        ui.text_edit_singleline(&mut app.board.hold_text);
        ui.horizontal(|ui| {
            ui.label("Date");
            ui.text_edit_singleline(&mut app.board.hold_date_text);
            ui.label("Start");
            ui.text_edit_singleline(&mut app.board.hold_start_text);
            ui.label("Minutes");
            ui.text_edit_singleline(&mut app.board.hold_len_text);
        });
        ui.label("Colour");
        ui.horizontal(|ui| {
            for (index, color) in hold_colors().iter().enumerate() {
                if ui
                    .add(
                        egui::Button::new(" ")
                            .fill(*color)
                            .min_size(Vec2::new(28.0, 22.0))
                            .stroke(Stroke::new(
                                if index == app.board.hold_color_index {
                                    2.5
                                } else {
                                    0.5
                                },
                                if index == app.board.hold_color_index {
                                    ui.visuals().text_color()
                                } else {
                                    faded(*color, 0.5)
                                },
                            )),
                    )
                    .clicked()
                {
                    app.board.hold_color_index = index;
                }
            }
        });
        ui.separator();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            save = ui.button("Save").clicked();
            cancel = ui.button("Cancel").clicked();
            delete = is_edit && ui.button("Delete").clicked();
        });
    });

    if delete {
        app.board.remove_hold(hold.hold_num);
        app.board.hold_edit_open = false;
        return;
    }
    if cancel {
        app.board.hold_edit_open = false;
        return;
    }
    if save {
        let Ok(date) = NaiveDate::parse_from_str(&app.board.hold_date_text, "%Y-%m-%d") else {
            app.error = Some("Blockout date must be YYYY-MM-DD.".into());
            return;
        };
        let Ok(start) = NaiveTime::parse_from_str(&app.board.hold_start_text, "%H:%M") else {
            app.error = Some("Blockout start must be HH:MM.".into());
            return;
        };
        let start_min = snap_minute(start.hour() as i32 * 60 + start.minute() as i32);
        let len = app
            .board
            .hold_len_text
            .parse::<i32>()
            .unwrap_or(30)
            .clamp(SNAP_MIN, DAY_END_MIN - start_min);
        let mut updated = hold;
        updated.date = date;
        updated.start_min = start_min;
        updated.len = len;
        updated.text = if app.board.hold_text.trim().is_empty() {
            "Hold".into()
        } else {
            app.board.hold_text.trim().to_string()
        };
        updated.color = hold_colors()[app.board.hold_color_index % hold_colors().len()];
        if is_edit {
            app.apply_board_action(BoardAction::UpdateHold(updated));
        } else {
            app.apply_board_action(BoardAction::AddHold(updated));
        }
        app.board.hold_edit_open = false;
    }
}

impl OdApp {
    fn apply_board_action(&mut self, action: BoardAction) {
        match action {
            BoardAction::SetView(view) => {
                self.board.view = view;
                self.board.save_prefs();
                self.refresh();
            }
            BoardAction::SetAnchor(date) => {
                self.schedule_day = date;
                self.board.anchor = date;
                self.board.cal_month = first_of_month(date);
                self.board.save_prefs();
                self.refresh();
            }
            BoardAction::JumpTo(apt_num) => {
                if let Some(apt) = self
                    .appointments
                    .iter()
                    .find(|a| a.apt_num == apt_num)
                    .cloned()
                {
                    self.schedule_day = apt.apt_datetime.date();
                    self.board.anchor = self.schedule_day;
                    self.board.view = BoardView::Day;
                    self.board.selected_apt = Some(apt_num);
                    self.board.flash_apt = Some(apt_num);
                    self.board.cal_month = first_of_month(self.schedule_day);
                    self.board.save_prefs();
                    self.refresh();
                } else if self.waitlist.iter().any(|a| a.apt_num == apt_num) {
                    self.notice =
                        Some("That patient is on the pin board. Schedule the card first.".into());
                } else {
                    self.error = Some("No appointment found for that patient.".into());
                }
            }
            BoardAction::OpenNew {
                date,
                start_min,
                operatory_num,
                provider_num,
                pat_num,
                hold_id,
            } => {
                let default_op = first_operatory(&self.operatories);
                let mut apt = Appointment {
                    pat_num: pat_num
                        .or(self.selected_patient)
                        .or_else(|| self.patients.first().map(|p| p.pat_num))
                        .unwrap_or(0),
                    apt_datetime: date
                        .and_hms_opt((start_min / 60) as u32, (start_min % 60) as u32, 0)
                        .unwrap_or_default(),
                    apt_length: 30,
                    apt_status: AppointmentStatus::Scheduled,
                    operatory_num: if operatory_num == 0 {
                        default_op
                    } else {
                        operatory_num
                    },
                    provider_num: if provider_num == 0 {
                        self.providers.first().map(|p| p.provider_num).unwrap_or(0)
                    } else {
                        provider_num
                    },
                    clinic_num: 1,
                    pattern: pattern_for_length(30),
                    ..Default::default()
                };
                if let Some(hold_id) = hold_id {
                    if let Some(hold) = self.board.holds.iter().find(|h| h.hold_num == hold_id) {
                        apt.notes = hold.text.clone();
                        apt.operatory_num = hold.operatory_num;
                    }
                    self.board.pending_hold_delete = Some(hold_id);
                }
                if apt.pat_num == 0 {
                    self.error = Some("Add a patient before scheduling an appointment.".into());
                    return;
                }
                self.open_appointment_editor(apt);
            }
            BoardAction::OpenEdit(apt_num) => {
                let apt = self
                    .appointments
                    .iter()
                    .chain(self.waitlist.iter())
                    .find(|a| a.apt_num == apt_num)
                    .cloned();
                match apt {
                    Some(apt) => self.open_appointment_editor(apt),
                    None => self.error = Some("Appointment no longer exists.".into()),
                }
            }
            BoardAction::SaveAppointment(apt, note) => self.save_board_appointment(apt, &note),
            BoardAction::DeleteAppointment(apt_num) => self.delete_board_appointment(apt_num),
            BoardAction::CancelVisit(apt_num) => {
                if let Some(apt) = self
                    .appointments
                    .iter()
                    .find(|a| a.apt_num == apt_num)
                    .cloned()
                {
                    self.cancel_visit(apt);
                    self.board.set_stage(apt_num, None);
                }
            }
            BoardAction::SetStage(apt_num, stage) => {
                let Some(mut apt) = self.store.get_appointment(apt_num).ok().flatten() else {
                    return;
                };
                if stage == Some(BoardStage::Complete) {
                    apt.apt_status = AppointmentStatus::Complete;
                } else if apt.apt_status == AppointmentStatus::Complete {
                    apt.apt_status = AppointmentStatus::Scheduled;
                }
                let note = match stage {
                    Some(stage) => format!("Appointment {apt_num} marked {}", stage.label()),
                    None => format!("Appointment {apt_num} visit stage cleared"),
                };
                self.save_board_appointment(apt, &note);
                self.board.set_stage(apt_num, stage);
            }
            BoardAction::ToggleAsap(apt_num) => {
                let Some(mut apt) = self.store.get_appointment(apt_num).ok().flatten() else {
                    return;
                };
                apt.priority = if apt.priority == RecallPriority::Asap {
                    RecallPriority::Normal
                } else {
                    RecallPriority::Asap
                };
                self.save_board_appointment(
                    apt,
                    &format!("Appointment {apt_num} ASAP priority toggled"),
                );
            }
            BoardAction::ToWaitlist(apt_num) => {
                let Some(mut apt) = self.store.get_appointment(apt_num).ok().flatten() else {
                    return;
                };
                apt.apt_status = AppointmentStatus::UnschedList;
                self.board.set_stage(apt_num, None);
                self.save_board_appointment(
                    apt,
                    &format!("Appointment {apt_num} moved to the pin board"),
                );
            }
            BoardAction::FromWaitlist(apt_num, date, start_min, operatory_num) => {
                let Some(apt) = self.waitlist.iter().find(|a| a.apt_num == apt_num).cloned() else {
                    return;
                };
                let when = date
                    .and_hms_opt((start_min / 60) as u32, (start_min % 60) as u32, 0)
                    .unwrap_or_default();
                let mut scheduled = schedule_from_waitlist(&apt, when, apt.apt_length);
                scheduled.operatory_num = operatory_num;
                self.board.set_stage(apt_num, None);
                self.save_board_appointment(
                    scheduled,
                    &format!("Appointment {apt_num} scheduled from the pin board"),
                );
            }
            BoardAction::AddHold(hold) => {
                self.board.hold_seq += 1;
                let mut hold = hold;
                hold.hold_num = self.board.hold_seq;
                self.board.selected_hold = Some(hold.hold_num);
                self.board.holds.push(hold);
                self.board.save_prefs();
                self.notice = Some("Blockout added".into());
            }
            BoardAction::UpdateHold(hold) => {
                if let Some(existing) = self
                    .board
                    .holds
                    .iter_mut()
                    .find(|h| h.hold_num == hold.hold_num)
                {
                    *existing = hold;
                }
                self.board.save_prefs();
                self.notice = Some("Blockout saved".into());
            }
            BoardAction::DeleteHold(hold_num) => {
                self.board.remove_hold(hold_num);
                self.notice = Some("Blockout deleted".into());
            }
            BoardAction::DuplicateHold(hold) => {
                self.board.hold_seq += 1;
                let mut copy = hold;
                copy.hold_num = self.board.hold_seq;
                copy.text = format!("{} (copy)", copy.text);
                copy.start_min = (copy.start_min + copy.len).min(DAY_END_MIN - copy.len);
                self.board.holds.push(copy);
                self.board.save_prefs();
            }
            BoardAction::NextHoldColor(hold_num) => {
                if let Some(hold) = self.board.holds.iter_mut().find(|h| h.hold_num == hold_num) {
                    let colors = hold_colors();
                    let index = colors.iter().position(|c| *c == hold.color).unwrap_or(0);
                    hold.color = colors[(index + 1) % colors.len()];
                }
                self.board.save_prefs();
            }
            BoardAction::EditHold(hold) => {
                self.board.open_hold_editor(
                    Some(hold.clone()),
                    hold.date,
                    hold.start_min,
                    hold.operatory_num,
                );
            }
            BoardAction::SendMessage => self.send_message_open = true,
            BoardAction::SetConfirmed(apt_num, def_num) => {
                let Some(mut apt) = self.store.get_appointment(apt_num).ok().flatten() else {
                    return;
                };
                apt.confirmed = def_num;
                let note = format!("Appointment {apt_num} confirmed from the board");
                self.save_board_appointment(apt, &note);
            }
            BoardAction::OpenCallLog(apt_num, pat_num) => {
                if !self.features.enabled(FeatureKey::AppointmentCallLog) {
                    self.error = Some("Appointment call log is turned off in Settings".into());
                    return;
                }
                self.comm_log_apt_num = apt_num;
                self.comm_log_pat_num = pat_num;
                self.comm_log_note.clear();
                self.comm_log_mode = CommLogMode::Phone;
                self.comm_log_direction = CommLogDirection::Received;
                self.comm_log_open = true;
            }
        }
    }

    fn save_board_appointment(&mut self, apt: Appointment, note: &str) {
        let before = self.store.get_appointment(apt.apt_num).ok().flatten();
        if self.features.enabled(FeatureKey::AppointmentHistory) && before.is_some() {
            let mut change = AppointmentChange::from_snapshot(
                before.as_ref().unwrap_or(&apt),
                AppointmentChangeAction::Changed,
            );
            change.user_num = self.session.as_ref().map(|s| s.user_num).unwrap_or(0);
            if let Err(e) = self.store.append_appointment_change(&change) {
                self.error = Some(e);
                return;
            }
        }
        match self.store.upsert_appointment(&apt) {
            Ok(_) => {
                self.audit_if(
                    FeatureKey::AuditAppointmentChanges,
                    Permission::APPOINTMENT_EDIT,
                    apt.pat_num,
                    note.to_string(),
                );
                self.notice = Some(note.to_string());
                self.error = None;
                self.refresh();
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn delete_board_appointment(&mut self, apt_num: Id) {
        let Some(apt) = self.store.get_appointment(apt_num).ok().flatten() else {
            return;
        };
        if self.features.enabled(FeatureKey::AppointmentHistory) {
            let mut change =
                AppointmentChange::from_snapshot(&apt, AppointmentChangeAction::Deleted);
            change.user_num = self.session.as_ref().map(|s| s.user_num).unwrap_or(0);
            if let Err(e) = self.store.append_appointment_change(&change) {
                self.error = Some(e);
                return;
            }
        }
        match self.store.delete_appointment(apt_num) {
            Ok(()) => {
                self.audit_if(
                    FeatureKey::AuditAppointmentChanges,
                    Permission::APPOINTMENT_DELETE,
                    apt.pat_num,
                    format!("Appointment {apt_num} deleted from the board"),
                );
                self.board.set_stage(apt_num, None);
                self.board.selected_apt = None;
                self.notice = Some("Appointment deleted".into());
                self.refresh();
            }
            Err(e) => self.error = Some(e),
        }
    }
}

fn first_of_month(date: NaiveDate) -> NaiveDate {
    date.with_day(1).unwrap_or(date)
}

fn add_months(date: NaiveDate, delta: i32) -> NaiveDate {
    let total = date.year() * 12 + date.month0() as i32 + delta;
    let year = total.div_euclid(12);
    let month = total.rem_euclid(12) as u32 + 1;
    NaiveDate::from_ymd_opt(year, month, 1).unwrap_or(date)
}

fn days_of_week(date: NaiveDate) -> Vec<NaiveDate> {
    let offset = date.weekday().num_days_from_monday() as i64;
    let monday = date - Duration::days(offset);
    (0..7).map(|i| monday + Duration::days(i)).collect()
}

fn is_waitlist(apt: &Appointment) -> bool {
    apt.apt_status == AppointmentStatus::UnschedList
}

fn apt_time_minute(apt: &Appointment) -> i32 {
    apt.apt_datetime.hour() as i32 * 60 + apt.apt_datetime.minute() as i32
}

fn snap_minute(minute: i32) -> i32 {
    let clamped = minute.max(DAY_START_MIN).min(DAY_END_MIN - SNAP_MIN);
    let snapped = ((clamped - DAY_START_MIN + SNAP_MIN / 2) / SNAP_MIN) * SNAP_MIN + DAY_START_MIN;
    snapped.min(DAY_END_MIN - SNAP_MIN)
}

fn resize_len(drag: &BoardDrag, delta: Vec2) -> i32 {
    let change = (delta.y / PIXELS_PER_MIN).round() as i32 / SNAP_MIN * SNAP_MIN;
    let max = DAY_END_MIN - drag.start_min;
    (drag.len + change).clamp(SNAP_MIN, max)
}

fn pointer_minute(ui: &Ui, body: Rect) -> Option<i32> {
    let pointer = ui.ctx().pointer_latest_pos()?;
    if !body.contains(pointer) {
        return None;
    }
    Some(snap_minute(
        DAY_START_MIN + ((pointer.y - body.top()) / PIXELS_PER_MIN).round() as i32,
    ))
}

fn format_time(minute: i32) -> String {
    let hour = (minute / 60) % 24;
    let mins = minute % 60;
    let suffix = if hour < 12 { "AM" } else { "PM" };
    let display = if hour % 12 == 0 { 12 } else { hour % 12 };
    format!("{display}:{mins:02} {suffix}")
}

fn format_hhmm(minute: i32) -> String {
    format!("{:02}:{:02}", (minute / 60) % 24, minute % 60)
}

fn build_columns(
    view: BoardView,
    anchor: NaiveDate,
    operatories: &[Operatory],
    providers: &[Provider],
    appointments: &[Appointment],
) -> Vec<BoardColumn> {
    let palette = provider_palette();
    let visible: Vec<&Operatory> = operatories.iter().filter(|o| !o.is_hidden).collect();
    match view {
        BoardView::Day => visible
            .into_iter()
            .enumerate()
            .map(|(index, op)| {
                let provider = appointments
                    .iter()
                    .find(|a| {
                        !is_waitlist(a)
                            && a.apt_datetime.date() == anchor
                            && a.operatory_num == op.operatory_num
                    })
                    .and_then(|a| providers.iter().find(|p| p.provider_num == a.provider_num))
                    .or_else(|| providers.get(index));
                BoardColumn {
                    label: if op.description.is_empty() {
                        op.abbrev.clone()
                    } else {
                        op.description.clone()
                    },
                    sub: provider.map(|p| p.abbreviation.clone()).unwrap_or_default(),
                    color: palette[index % palette.len()],
                    date: anchor,
                    operatory_num: Some(op.operatory_num),
                    provider_num: provider
                        .map(|p| p.provider_num)
                        .or_else(|| providers.first().map(|p| p.provider_num))
                        .unwrap_or(0),
                }
            })
            .collect(),
        BoardView::Week => days_of_week(anchor)
            .into_iter()
            .enumerate()
            .map(|(index, date)| {
                let provider = appointments
                    .iter()
                    .find(|a| !is_waitlist(a) && a.apt_datetime.date() == date)
                    .and_then(|a| providers.iter().find(|p| p.provider_num == a.provider_num))
                    .or_else(|| providers.get(index));
                BoardColumn {
                    label: date.format("%a").to_string(),
                    sub: date.format("%b %d").to_string(),
                    color: palette[index % palette.len()],
                    date,
                    operatory_num: None,
                    provider_num: provider
                        .map(|p| p.provider_num)
                        .or_else(|| providers.first().map(|p| p.provider_num))
                        .unwrap_or(0),
                }
            })
            .collect(),
    }
}

fn lane_blocks(appointments: Vec<Appointment>) -> Vec<BoardBlock> {
    let mut items: Vec<BoardBlock> = appointments
        .into_iter()
        .map(|apt| {
            let start = apt_time_minute(&apt).max(DAY_START_MIN);
            let len = (apt.apt_length as i32).clamp(SNAP_MIN, (DAY_END_MIN - start).max(SNAP_MIN));
            BoardBlock {
                apt,
                start_min: start,
                len,
                lane: 0,
                lanes: 1,
            }
        })
        .collect();
    items.sort_by_key(|b| (b.start_min, std::cmp::Reverse(b.len)));

    let mut clusters: Vec<Vec<BoardBlock>> = Vec::new();
    for item in items {
        if let Some(cluster) = clusters.last_mut() {
            let cluster_end = cluster
                .iter()
                .map(|b| b.start_min + b.len)
                .max()
                .unwrap_or(DAY_START_MIN);
            if item.start_min < cluster_end {
                cluster.push(item);
                continue;
            }
        }
        clusters.push(vec![item]);
    }

    let mut result = Vec::new();
    for mut cluster in clusters {
        let mut lane_ends: Vec<i32> = Vec::new();
        for item in &mut cluster {
            let end = item.start_min + item.len;
            let mut lane = 0;
            while lane < lane_ends.len() && lane_ends[lane] > item.start_min {
                lane += 1;
            }
            if lane == lane_ends.len() {
                lane_ends.push(end);
            } else {
                lane_ends[lane] = end;
            }
            item.lane = lane;
        }
        let lanes = lane_ends.len().max(1);
        for mut item in cluster {
            item.lanes = lanes;
            result.push(item);
        }
    }
    result
}

fn palette_color(providers: &[Provider], provider_num: Id, fallback_index: usize) -> Color32 {
    let palette = provider_palette();
    let index = providers
        .iter()
        .position(|p| p.provider_num == provider_num)
        .unwrap_or(fallback_index);
    palette[index % palette.len()]
}

fn state_color(state: RecallDueState) -> Color32 {
    match state {
        RecallDueState::Overdue(_) => theme::danger(),
        RecallDueState::DueToday => theme::warning(),
        RecallDueState::Disabled | RecallDueState::NoDate => theme::muted(),
        _ => theme::success(),
    }
}

fn painter_dot(ui: &Ui, rect: Rect, color: Color32) {
    ui.painter()
        .circle_filled(rect.right_center() - Vec2::new(3.0, 0.0), 2.0, color);
}

fn first_operatory(operatories: &[Operatory]) -> Id {
    operatories
        .iter()
        .find(|o| !o.is_hidden)
        .or_else(|| operatories.first())
        .map(|o| o.operatory_num)
        .unwrap_or(0)
}

fn provider_palette() -> Vec<Color32> {
    theme::provider_palette().to_vec()
}

fn hold_colors() -> Vec<Color32> {
    theme::hold_palette().to_vec()
}

fn faded(color: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        (color.a() as f32 * alpha).round() as u8,
    )
}

fn blend(color: Color32, white: f32) -> Color32 {
    Color32::from_rgb(
        color.r() + ((255 - color.r() as u16) as f32 * white) as u8,
        color.g() + ((255 - color.g() as u16) as f32 * white) as u8,
        color.b() + ((255 - color.b() as u16) as f32 * white) as u8,
    )
}

fn color_to_hex(color: Color32) -> String {
    format!("{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

fn color_from_hex(value: &str) -> Color32 {
    if value.len() == 6 {
        let Ok(r) = u8::from_str_radix(&value[0..2], 16) else {
            return hold_colors()[0];
        };
        let Ok(g) = u8::from_str_radix(&value[2..4], 16) else {
            return hold_colors()[0];
        };
        let Ok(b) = u8::from_str_radix(&value[4..6], 16) else {
            return hold_colors()[0];
        };
        Color32::from_rgb(r, g, b)
    } else {
        hold_colors()[0]
    }
}

fn escape(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('|', "%7C")
        .replace('\n', "%0A")
        .replace('\r', "%0D")
}

fn unescape(value: &str) -> String {
    value
        .replace("%0D", "\r")
        .replace("%0A", "\n")
        .replace("%7C", "|")
        .replace("%25", "%")
}

fn prefs_path() -> Option<PathBuf> {
    let root = std::env::var_os("APPDATA").map(PathBuf::from)?;
    Some(root.join("odclinic").join("schedule_board.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Weekday;

    #[test]
    fn board_minutes_snap_to_quarter_hours_and_clamp() {
        assert_eq!(snap_minute(420), 420);
        assert_eq!(snap_minute(432), 435);
        assert_eq!(snap_minute(433), 435);
        assert_eq!(snap_minute(0), 420);
        assert_eq!(snap_minute(2000), 1125);
    }

    #[test]
    fn blockout_text_round_trips_through_prefs_escaping() {
        let original = "Call-in | new patient\nneeds onboarding 100%";
        assert_eq!(unescape(&escape(original)), original);
    }

    #[test]
    fn week_columns_start_on_monday() {
        let sunday = NaiveDate::from_ymd_opt(2026, 11, 29).unwrap();
        let days = days_of_week(sunday);
        assert_eq!(days[0].weekday(), Weekday::Mon);
        assert_eq!(days[6], sunday);
    }

    #[test]
    fn overlapping_appointments_get_separate_lanes() {
        let apt = |hour, len| Appointment {
            apt_num: hour as Id,
            apt_datetime: NaiveDate::from_ymd_opt(2026, 11, 27)
                .unwrap()
                .and_hms_opt(hour, 0, 0)
                .unwrap(),
            apt_length: len,
            ..Default::default()
        };
        let blocks = lane_blocks(vec![apt(9, 60), apt(9, 30), apt(11, 30)]);
        let first_two: Vec<&BoardBlock> = blocks.iter().filter(|b| b.start_min == 540).collect();
        assert_eq!(first_two.len(), 2);
        assert_ne!(first_two[0].lane, first_two[1].lane);
        assert!(first_two.iter().all(|b| b.lanes == 2));
    }
}
