//! Recall interval and due-date rules, mirroring OD's `Interval` struct and
//! recall list. The ASAP list is derived from appointment priority.

use chrono::{Datelike, Duration, NaiveDate};
use od_domain::{Appointment, Recall, RecallPriority};

/// OD packs `Interval` into an int with 8 bits per part: days, weeks, months,
/// then years. Unpack as `(years, months, weeks, days)`.
pub fn interval_parts(packed: i32) -> (u32, u32, u32, u32) {
    let raw = packed as u32;
    (
        (raw >> 24) & 0xFF,
        (raw >> 16) & 0xFF,
        (raw >> 8) & 0xFF,
        raw & 0xFF,
    )
}

/// The inverse of `interval_parts`; each part is clamped to OD's 8-bit limit.
pub fn interval_from_parts(years: u32, months: u32, weeks: u32, days: u32) -> i32 {
    let clamped = |v: u32| v.min(255);
    ((clamped(years) << 24) | (clamped(months) << 16) | (clamped(weeks) << 8) | clamped(days)) as i32
}

/// A short human form of a packed interval, e.g. `1y6m2w3d` (OD's `ToString`).
pub fn format_interval(packed: i32) -> String {
    let (years, months, weeks, days) = interval_parts(packed);
    let mut text = String::new();
    if years > 0 {
        text.push_str(&format!("{years}y"));
    }
    if months > 0 {
        text.push_str(&format!("{months}m"));
    }
    if weeks > 0 {
        text.push_str(&format!("{weeks}w"));
    }
    if days > 0 {
        text.push_str(&format!("{days}d"));
    }
    if text.is_empty() {
        "none".to_string()
    } else {
        text
    }
}

/// Add a packed interval to a date. Year and month overflow is clamped to the
/// last valid day of the month, the way OD's `DateTime.AddMonths` behaves.
pub fn add_interval(date: NaiveDate, packed: i32) -> Option<NaiveDate> {
    let (years, months, weeks, days) = interval_parts(packed);
    let total_months = date.year() * 12 + (date.month() as i32 - 1) + years as i32 * 12 + months as i32;
    let year = total_months.div_euclid(12);
    let month = total_months.rem_euclid(12) as u32 + 1;
    let last_day = match month {
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 31,
    };
    let adjusted = NaiveDate::from_ymd_opt(year, month, date.day().min(last_day))?;
    adjusted.checked_add_signed(Duration::days((weeks * 7 + days) as i64))
}

fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// Where a recall sits relative to today. Used to sort and colour the list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecallDueState {
    Disabled,
    NoDate,
    Overdue(i64),
    DueToday,
    Soon(i64),
    Later(i64),
}

impl RecallDueState {
    pub fn label(self) -> String {
        match self {
            Self::Disabled => "Disabled".into(),
            Self::NoDate => "No due date".into(),
            Self::Overdue(days) => format!("{days}d overdue"),
            Self::DueToday => "Due today".into(),
            Self::Soon(days) => format!("Due in {days}d"),
            Self::Later(days) => format!("Due in {days}d"),
        }
    }

    /// Lower values sort first: overdue, then due today, then upcoming.
    pub fn sort_rank(self) -> (u8, i64) {
        match self {
            Self::Disabled => (5, 0),
            Self::NoDate => (4, 0),
            Self::Overdue(days) => (0, -days),
            Self::DueToday => (1, 0),
            Self::Soon(days) => (2, days),
            Self::Later(days) => (3, days),
        }
    }
}

/// `DateDue` is the user-editable field OD reports on; `DateDueCalc` is only a
/// fallback when it is empty.
pub fn recall_due_date(recall: &Recall) -> Option<NaiveDate> {
    recall.date_due.or(recall.date_due_calc)
}

pub fn recall_due_state(recall: &Recall, today: NaiveDate) -> RecallDueState {
    if recall.is_disabled {
        return RecallDueState::Disabled;
    }
    let Some(due) = recall_due_date(recall) else {
        return RecallDueState::NoDate;
    };
    match (due - today).num_days() {
        d if d < 0 => RecallDueState::Overdue(-d),
        0 => RecallDueState::DueToday,
        d if d <= 30 => RecallDueState::Soon(d),
        d => RecallDueState::Later(d),
    }
}

/// OD's ASAP query: `appointment.Priority = 1` with an active-status filter.
pub fn asap_appointments(appointments: &[Appointment]) -> Vec<Appointment> {
    let mut rows: Vec<Appointment> = appointments
        .iter()
        .filter(|a| a.priority == RecallPriority::Asap)
        .cloned()
        .collect();
    rows.sort_by_key(|a| a.apt_datetime);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_domain::AppointmentStatus;

    #[test]
    fn interval_round_trips_through_od_packing() {
        let packed = interval_from_parts(1, 6, 2, 3);
        assert_eq!(interval_parts(packed), (1, 6, 2, 3));
        assert_eq!(format_interval(packed), "1y6m2w3d");
        assert_eq!(format_interval(interval_from_parts(0, 0, 0, 0)), "none");
        // OD clamps each part to 255.
        assert_eq!(interval_parts(interval_from_parts(256, 0, 0, 0)), (255, 0, 0, 0));
    }

    #[test]
    fn adding_interval_matches_od_month_and_leap_behaviour() {
        let start = NaiveDate::from_ymd_opt(2024, 1, 31).unwrap();
        let six_months = add_interval(start, interval_from_parts(0, 6, 0, 0)).unwrap();
        assert_eq!(six_months, NaiveDate::from_ymd_opt(2024, 7, 31).unwrap());
        let with_weeks = add_interval(start, interval_from_parts(0, 0, 2, 3)).unwrap();
        assert_eq!(with_weeks, NaiveDate::from_ymd_opt(2024, 2, 17).unwrap());
        let across_leap = add_interval(
            NaiveDate::from_ymd_opt(2024, 2, 28).unwrap(),
            interval_from_parts(1, 0, 0, 0),
        )
        .unwrap();
        assert_eq!(across_leap, NaiveDate::from_ymd_opt(2025, 2, 28).unwrap());
    }

    #[test]
    fn recall_states_cover_due_overdue_upcoming_and_disabled() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let mut recall = Recall {
            date_due: Some(today - Duration::days(3)),
            ..Default::default()
        };
        assert_eq!(recall_due_state(&recall, today), RecallDueState::Overdue(3));
        recall.date_due = Some(today);
        assert_eq!(recall_due_state(&recall, today), RecallDueState::DueToday);
        recall.date_due = Some(today + Duration::days(10));
        assert_eq!(recall_due_state(&recall, today), RecallDueState::Soon(10));
        recall.date_due = None;
        recall.date_due_calc = Some(today + Duration::days(60));
        assert_eq!(recall_due_state(&recall, today), RecallDueState::Later(60));
        recall.is_disabled = true;
        assert_eq!(recall_due_state(&recall, today), RecallDueState::Disabled);
    }

    #[test]
    fn asap_list_filters_on_priority_and_sorts_by_time() {
        let day = NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let normal = Appointment {
            apt_num: 1,
            priority: RecallPriority::Normal,
            apt_status: AppointmentStatus::Scheduled,
            ..Default::default()
        };
        let later = Appointment {
            apt_num: 2,
            priority: RecallPriority::Asap,
            apt_datetime: day.and_hms_opt(15, 0, 0).unwrap(),
            apt_status: AppointmentStatus::Scheduled,
            ..Default::default()
        };
        let earlier = Appointment {
            apt_num: 3,
            priority: RecallPriority::Asap,
            apt_datetime: day.and_hms_opt(9, 0, 0).unwrap(),
            apt_status: AppointmentStatus::Broken,
            ..Default::default()
        };
        let rows = asap_appointments(&[normal, later, earlier]);
        assert_eq!(rows.iter().map(|a| a.apt_num).collect::<Vec<_>>(), vec![3, 2]);
    }
}
