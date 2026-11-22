//! Pure rules for the clinical workflow slice: the waitlist (OD's unscheduled
//! list), correction history and appointment-time editing.
//!
//! Nothing here touches the database. The GUI uses these rules so the same
//! behaviour applies to demo data and to a real Open Dental schema.

use chrono::NaiveDateTime;
use od_domain::definitions::{Definition, DefinitionCategory};
use od_domain::*;

/// OD `ApptStatus.UnschedList = 3`. An appointment in this state is on the
/// unscheduled list; it is the port's waitlist. It is not a confirmed booking,
/// which is why removing it must never send a cancellation or reconfirmation.
pub fn is_waitlist(apt: &Appointment) -> bool {
    apt.apt_status == AppointmentStatus::UnschedList
}

/// Waitlist entries ordered the way OD's `FormUnsched` shows them: by the
/// appointment date first, then patient number for a stable order.
pub fn waitlist_appointments(appointments: &[Appointment]) -> Vec<&Appointment> {
    let mut rows: Vec<&Appointment> = appointments.iter().filter(|a| is_waitlist(a)).collect();
    rows.sort_by(|a, b| {
        a.apt_datetime
            .cmp(&b.apt_datetime)
            .then(a.pat_num.cmp(&b.pat_num))
            .then(a.apt_num.cmp(&b.apt_num))
    });
    rows
}

/// The waitlist status is a definition in OD category 13
/// (`DefCat.RecallUnschedStatus`), not a free-text field.
pub fn waitlist_status_name(definitions: &[Definition], def_num: Id) -> String {
    if def_num == 0 {
        return "—".to_string();
    }
    definitions
        .iter()
        .find(|d| {
            d.def_num == def_num && d.category == DefinitionCategory::RECALL_UNSCHED_STATUS
        })
        .map(|d| d.item_name.clone())
        .unwrap_or_else(|| format!("Definition {def_num}"))
}

/// OD's unscheduled-list statuses, alphabetised as `FormDefinitions` shows them.
pub fn waitlist_status_definitions(definitions: &[Definition]) -> Vec<&Definition> {
    let mut rows: Vec<&Definition> = definitions
        .iter()
        .filter(|d| d.category == DefinitionCategory::RECALL_UNSCHED_STATUS && !d.is_hidden)
        .collect();
    rows.sort_by(|a, b| a.item_name.to_lowercase().cmp(&b.item_name.to_lowercase()));
    rows
}

/// Turn a waitlist entry into a real booking.
///
/// OD copies the unscheduled appointment rather than editing it in place, so
/// this clears `UnschedStatus`, sets the chosen time and rebuilds `Pattern`.
/// ASAP priority is preserved: an ASAP patient promoted off the waitlist stays
/// at the top of the ASAP list.
pub fn schedule_from_waitlist(apt: &Appointment, when: NaiveDateTime, length: u32) -> Appointment {
    let mut scheduled = apt.clone();
    scheduled.apt_status = AppointmentStatus::Scheduled;
    scheduled.apt_datetime = when;
    scheduled.apt_length = length;
    scheduled.pattern = pattern_for_length(length);
    scheduled.waitlist_status = 0;
    scheduled
}

/// Parse the editable appointment date/time strictly.
///
/// The old editor used `unwrap_or_default()`, so a typo silently moved an
/// appointment to year 1970. Returning an error keeps the original value.
pub fn parse_appointment_datetime(text: &str) -> Result<NaiveDateTime, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Enter a date and time, for example 2026-11-21 09:30.".into());
    }
    NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M")
        .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S"))
        .map_err(|_| format!("Invalid date/time '{text}'. Use YYYY-MM-DD HH:MM."))
}

/// Parse the editable appointment length. OD stores one pattern character per
/// five-minute increment and allows up to nine hours in the appointment editor.
pub fn parse_appointment_length(text: &str) -> Result<u32, String> {
    let text = text.trim();
    let minutes: u32 = text
        .parse()
        .map_err(|_| format!("Invalid appointment length '{text}'. Use whole minutes."))?;
    if !(5..=540).contains(&minutes) {
        return Err("Appointment length must be between 5 and 540 minutes.".into());
    }
    if minutes % 5 != 0 {
        return Err("Appointment length must be a multiple of five minutes (OD's pattern increment).".into());
    }
    Ok(minutes)
}

/// OD `appointment.Pattern`: one character per five-minute increment.
pub fn pattern_for_length(length: u32) -> String {
    "X".repeat((length / 5).max(1) as usize)
}

/// A short before/after description for `securitylog`.
pub fn appointment_change_note(before: &Appointment, after: &Appointment) -> String {
    let mut changes: Vec<String> = Vec::new();
    if before.apt_datetime != after.apt_datetime {
        changes.push(format!(
            "time {} -> {}",
            before.apt_datetime.format("%Y-%m-%d %H:%M"),
            after.apt_datetime.format("%Y-%m-%d %H:%M")
        ));
    }
    if before.apt_length != after.apt_length {
        changes.push(format!("length {} -> {} min", before.apt_length, after.apt_length));
    }
    if before.apt_status != after.apt_status {
        changes.push(format!(
            "status {} -> {}",
            before.apt_status.label(),
            after.apt_status.label()
        ));
    }
    if before.pat_num != after.pat_num {
        changes.push(format!("patient {} -> {}", before.pat_num, after.pat_num));
    }
    if before.waitlist_status != after.waitlist_status {
        changes.push(format!(
            "waitlist status {} -> {}",
            before.waitlist_status, after.waitlist_status
        ));
    }
    if before.notes != after.notes {
        changes.push("notes changed".to_string());
    }
    if changes.is_empty() {
        format!("Appointment {} saved", after.apt_num)
    } else {
        format!("Appointment {} {}", after.apt_num, changes.join("; "))
    }
}

/// A short before/after description for a patient correction.
pub fn patient_change_note(before: &Patient, after: &Patient) -> String {
    let mut changes: Vec<String> = Vec::new();
    let pairs = [
        ("name", format!("{}, {}", before.l_name, before.f_name), format!("{}, {}", after.l_name, after.f_name)),
        ("DOB", format!("{:?}", before.birth_date), format!("{:?}", after.birth_date)),
        ("phone", before.home_phone.clone(), after.home_phone.clone()),
        ("mobile", before.wireless_phone.clone(), after.wireless_phone.clone()),
        ("email", before.email.clone(), after.email.clone()),
        ("address", before.address.clone(), after.address.clone()),
        ("city", before.city.clone(), after.city.clone()),
        ("state", before.state.clone(), after.state.clone()),
        ("zip", before.zip.clone(), after.zip.clone()),
        ("gender", before.gender.label().to_string(), after.gender.label().to_string()),
        ("title", before.title.clone(), after.title.clone()),
    ];
    for (label, old, new) in pairs {
        if old != new {
            changes.push(format!("{label}: '{old}' -> '{new}'"));
        }
    }
    if changes.is_empty() {
        format!("Patient {} saved", after.pat_num)
    } else {
        format!("Patient {} {}", after.pat_num, changes.join("; "))
    }
}

/// Recreate the appointment a history row describes. This is the inverse of
/// `HistAppointment.ToAppt()` in Open Dental.
pub fn restore_appointment(change: &AppointmentChange) -> Appointment {
    let mut restored = change.snapshot.clone();
    restored.apt_num = if change.apt_num == 0 {
        restored.apt_num
    } else {
        change.apt_num
    };
    restored.pat_num = if change.pat_num == 0 {
        restored.pat_num
    } else {
        change.pat_num
    };
    restored
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};

    fn apt(when: &str, status: AppointmentStatus) -> Appointment {
        Appointment {
            apt_num: 1,
            pat_num: 7,
            apt_datetime: NaiveDateTime::parse_from_str(when, "%Y-%m-%d %H:%M").unwrap(),
            apt_length: 30,
            apt_status: status,
            pattern: "XXXXXX".into(),
            waitlist_status: 42,
            ..Default::default()
        }
    }

    #[test]
    fn waitlist_filters_on_unsched_list_and_sorts_by_time() {
        let later = apt("2026-11-22 09:00", AppointmentStatus::UnschedList);
        let scheduled = apt("2026-11-20 09:00", AppointmentStatus::Scheduled);
        let earlier = apt("2026-11-21 08:30", AppointmentStatus::UnschedList);
        let all = [later, scheduled, earlier];
        let rows = waitlist_appointments(&all);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].apt_datetime, all[2].apt_datetime);
        assert_eq!(rows[1].apt_datetime, all[0].apt_datetime);
    }

    #[test]
    fn scheduling_from_waitlist_clears_status_and_rebuilds_pattern() {
        let waiting = apt("2026-11-21 08:30", AppointmentStatus::UnschedList);
        let when = NaiveDateTime::parse_from_str("2026-11-28 14:00", "%Y-%m-%d %H:%M").unwrap();
        let scheduled = schedule_from_waitlist(&waiting, when, 45);
        assert_eq!(scheduled.apt_status, AppointmentStatus::Scheduled);
        assert_eq!(scheduled.apt_datetime, when);
        assert_eq!(scheduled.apt_length, 45);
        assert_eq!(scheduled.pattern, "XXXXXXXXX");
        assert_eq!(scheduled.waitlist_status, 0);
    }

    #[test]
    fn appointment_time_parsing_rejects_typos_instead_of_defaulting() {
        assert!(parse_appointment_datetime("not a date").is_err());
        assert!(parse_appointment_datetime("").is_err());
        assert!(parse_appointment_datetime("2026-13-45 99:99").is_err());
        let parsed = parse_appointment_datetime("2026-11-21 09:30").unwrap();
        assert_eq!(parsed.date(), NaiveDate::from_ymd_opt(2026, 11, 21).unwrap());
        assert_eq!(parsed.time(), NaiveTime::from_hms_opt(9, 30, 0).unwrap());
    }

    #[test]
    fn appointment_length_uses_od_five_minute_increments() {
        assert_eq!(parse_appointment_length("45"), Ok(45));
        assert!(parse_appointment_length("43").is_err());
        assert!(parse_appointment_length("0").is_err());
        assert!(parse_appointment_length("541").is_err());
        assert!(parse_appointment_length("abc").is_err());
    }

    #[test]
    fn change_note_describes_what_was_corrected() {
        let before = apt("2026-11-21 08:30", AppointmentStatus::UnschedList);
        let after = schedule_from_waitlist(
            &before,
            NaiveDateTime::parse_from_str("2026-11-28 14:00", "%Y-%m-%d %H:%M").unwrap(),
            30,
        );
        let note = appointment_change_note(&before, &after);
        assert!(note.contains("time 2026-11-21 08:30 -> 2026-11-28 14:00"), "{note}");
        assert!(note.contains("status Unscheduled list -> Scheduled"), "{note}");
        assert!(note.contains("waitlist status 42 -> 0"), "{note}");
    }

    #[test]
    fn appointment_history_action_round_trips_od_codes() {
        for action in AppointmentChangeAction::ALL {
            assert_eq!(AppointmentChangeAction::from_od(action.as_od()), action);
        }
    }

    #[test]
    fn restoring_a_snapshot_recreates_the_appointment() {
        let original = apt("2026-11-21 08:30", AppointmentStatus::UnschedList);
        let change = AppointmentChange::from_snapshot(&original, AppointmentChangeAction::Deleted);
        let restored = restore_appointment(&change);
        assert_eq!(restored.apt_datetime, original.apt_datetime);
        assert_eq!(restored.apt_status, original.apt_status);
        assert_eq!(restored.waitlist_status, 42);
    }
}
