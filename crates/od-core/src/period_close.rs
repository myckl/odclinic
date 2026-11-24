//! Audited period-close gate for the double-entry journal.
//!
//! A closed month is persisted as one OD preference row so the lock survives
//! restarts and is shared by the demo and MySQL backends. Posting helpers read
//! the same `ClosedPeriods` snapshot the GUI shows; a source dated inside a
//! closed month is never written, and every close/reopen is written to the OD
//! security log by the caller.

use chrono::{Datelike, NaiveDate, NaiveDateTime};
use od_domain::{Id, Pref, SecurityEvent, Userod};

use crate::ReportTable;
use serde_json::{Value, json};

/// Preference row holding the JSON array of closed periods.
pub const CLOSED_PERIODS_PREF: &str = "AccountingClosedPeriods";

/// One signed-off accounting month.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosedPeriod {
    pub year: i32,
    pub month: u32,
    pub reason: String,
    pub closed_at: String,
}

impl ClosedPeriod {
    /// `YYYY-MM`, the stable label used in the audit log and the preference.
    pub fn key(&self) -> String {
        format!("{:04}-{:02}", self.year, self.month)
    }

    pub fn label(&self) -> String {
        self.key()
    }

    fn to_json(&self) -> Value {
        json!({
            "year": self.year,
            "month": self.month,
            "reason": self.reason,
            "closed_at": self.closed_at,
        })
    }

    fn from_json(value: &Value) -> Option<Self> {
        let year = value.get("year")?.as_i64()? as i32;
        let month = value.get("month")?.as_u64()? as u32;
        if (1..=12).contains(&month) {
            Some(Self {
                year,
                month,
                reason: value
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                closed_at: value
                    .get("closed_at")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        } else {
            None
        }
    }
}

/// Canonical audit wording for a close. The GUI writes this exact text and the
/// history report reads it back, so the two cannot drift apart.
pub fn close_audit_text(key: &str, reason: &str) -> String {
    format!("Closed accounting period {key}: {}", reason.trim())
}

/// Canonical audit wording for a reopen, retaining the reason being undone.
pub fn reopen_audit_text(key: &str, previous_reason: &str, reason: &str) -> String {
    format!(
        "Reopened accounting period {key} (was closed for: {previous_reason}) — reason: {}",
        reason.trim()
    )
}

/// First and last day of the month containing `date`. Closing operates on
/// whole months so a backdated source cannot slip into a signed-off period.
pub fn month_bounds(date: NaiveDate) -> (NaiveDate, NaiveDate) {
    let first = date.with_day(1).unwrap_or(date);
    let last = (first + chrono::Duration::days(32))
        .with_day(1)
        .unwrap_or(first)
        - chrono::Duration::days(1);
    (first, last)
}

/// The closed months in force, kept in date order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClosedPeriods(pub Vec<ClosedPeriod>);

impl ClosedPeriods {
    /// Read the persisted snapshot. A missing or malformed preference is an
    /// empty set, so an old database opens with no lock rather than failing.
    pub fn from_prefs(prefs: &[Pref]) -> Self {
        let text = prefs
            .iter()
            .find(|p| p.pref_name == CLOSED_PERIODS_PREF)
            .map(|p| p.pref_value.as_str())
            .unwrap_or("[]");
        Self::from_json(text)
    }

    pub fn from_json(text: &str) -> Self {
        let rows: Vec<Value> = serde_json::from_str(text).unwrap_or_default();
        let mut periods: Vec<ClosedPeriod> = rows
            .iter()
            .filter_map(ClosedPeriod::from_json)
            .collect();
        periods.sort_by(|a, b| (a.year, a.month).cmp(&(b.year, b.month)));
        periods.dedup_by(|a, b| (a.year, a.month) == (b.year, b.month));
        Self(periods)
    }

    pub fn to_json(&self) -> String {
        let rows: Vec<Value> = self.0.iter().map(ClosedPeriod::to_json).collect();
        serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn all(&self) -> &[ClosedPeriod] {
        &self.0
    }

    pub fn closed_for(&self, date: NaiveDate) -> Option<&ClosedPeriod> {
        self.0
            .iter()
            .find(|p| p.year == date.year() && p.month == date.month())
    }

    pub fn is_closed(&self, date: NaiveDate) -> bool {
        self.closed_for(date).is_some()
    }

    /// Any closed month overlapping a date range, in date order.
    pub fn in_range(&self, from: NaiveDate, to: NaiveDate) -> Vec<&ClosedPeriod> {
        self.0
            .iter()
            .filter(|p| {
                let (first, last) = month_bounds(
                    NaiveDate::from_ymd_opt(p.year, p.month, 1).unwrap_or(from),
                );
                first <= to && last >= from
            })
            .collect()
    }

    /// Close a month. The reason is mandatory so the audit entry explains why
    /// the gate was applied; a second close is refused until it is reopened.
    pub fn close(
        &mut self,
        date: NaiveDate,
        reason: &str,
        closed_at: String,
    ) -> Result<&ClosedPeriod, String> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err("Enter a reason for closing this period.".into());
        }
        if let Some(existing) = self.closed_for(date) {
            return Err(format!(
                "Period {} is already closed ({}). Reopen it before closing again.",
                existing.label(),
                existing.reason
            ));
        }
        let period = ClosedPeriod {
            year: date.year(),
            month: date.month(),
            reason: reason.to_string(),
            closed_at,
        };
        self.0.push(period);
        self.0.sort_by(|a, b| (a.year, a.month).cmp(&(b.year, b.month)));
        Ok(self.closed_for(date).expect("period just inserted"))
    }

    /// Reopen a month, returning the entry that was removed for auditing.
    pub fn reopen(&mut self, date: NaiveDate) -> Result<ClosedPeriod, String> {
        let position = self
            .0
            .iter()
            .position(|p| p.year == date.year() && p.month == date.month())
            .ok_or_else(|| {
                format!(
                    "Period {:04}-{:02} is not closed.",
                    date.year(),
                    date.month()
                )
            })?;
        Ok(self.0.remove(position))
    }

    /// Guard used by every posting window before a write.
    pub fn reject_posting(&self, date: NaiveDate, what: &str) -> Result<(), String> {
        match self.closed_for(date) {
            Some(period) => Err(format!(
                "Cannot post {what} dated {date}: period {} was closed ({}). Reopen it in the Ledger tie-out report before posting. If this is a late correction, close it again with a new reason.",
                period.label(),
                period.reason
            )),
            None => Ok(()),
        }
    }
}

/// What an audited period action did to the posting gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeriodCloseAction {
    Closed,
    Reopened,
}

impl PeriodCloseAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Closed => "Closed",
            Self::Reopened => "Reopened",
        }
    }
}

/// One close or reopen reconstructed from the OD security log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeriodCloseEvent {
    pub security_log_num: Id,
    pub user_num: Id,
    pub timestamp: NaiveDateTime,
    pub month: String,
    pub action: PeriodCloseAction,
    pub reason: String,
    /// The reason of the close that a reopen undid, when applicable.
    pub previous_reason: Option<String>,
}

fn valid_month_key(key: &str) -> bool {
    let Some((year, month)) = key.split_once('-') else {
        return false;
    };
    match (year.parse::<i32>(), month.parse::<u32>()) {
        (Ok(year), Ok(month)) => NaiveDate::from_ymd_opt(year, month, 1).is_some(),
        _ => false,
    }
}

impl PeriodCloseEvent {
    /// Read one canonical period-close audit row. Unrelated log rows are
    /// ignored so the report stays useful on a busy security log.
    pub fn from_security_event(event: &SecurityEvent) -> Option<Self> {
        let text = event.log_text.as_str();
        let (month, action, reason, previous_reason) =
            if let Some(rest) = text.strip_prefix("Closed accounting period ") {
                let (key, reason) = rest.split_once(": ")?;
                (key, PeriodCloseAction::Closed, reason, None)
            } else if let Some(rest) = text.strip_prefix("Reopened accounting period ") {
                let (key, tail) = rest.split_once(" (was closed for: ")?;
                let (previous, reason) = tail.rsplit_once(") — reason: ")?;
                (key, PeriodCloseAction::Reopened, reason, Some(previous))
            } else {
                return None;
            };
        if !valid_month_key(month) {
            return None;
        }
        Some(Self {
            security_log_num: event.security_log_num,
            user_num: event.user_num,
            timestamp: event.log_datetime,
            month: month.to_string(),
            action,
            reason: reason.to_string(),
            previous_reason: previous_reason.map(str::to_string),
        })
    }
}

/// Every logged sign-off and reopen, newest first. Ties keep log-number order
/// so two actions in the same second still read in the order they were written.
pub fn period_close_events(log: &[SecurityEvent]) -> Vec<PeriodCloseEvent> {
    let mut events: Vec<_> = log
        .iter()
        .filter_map(PeriodCloseEvent::from_security_event)
        .collect();
    events.sort_by(|a, b| {
        (b.timestamp, b.security_log_num).cmp(&(a.timestamp, a.security_log_num))
    });
    events
}

/// Resolve the audit row's `UserNum` against `userod`. User 0 is the implicit
/// session used when login is turned off; an unrecognised number is still
/// shown so a stale user record cannot hide who signed the period off.
fn user_label(users: &[Userod], user_num: Id) -> String {
    if user_num == 0 {
        return "Local user".to_string();
    }
    users
        .iter()
        .find(|user| user.user_num == user_num)
        .map(|user| {
            let name = user.user_name.trim();
            if name.is_empty() {
                format!("User #{user_num}")
            } else {
                name.to_string()
            }
        })
        .unwrap_or_else(|| format!("User #{user_num}"))
}

/// Read-only sign-off history for staff. Each row is one audited close or
/// reopen; the final column shows whether that month is still locked by the
/// persisted preference, so a reopen that was later re-signed reads as locked.
/// The acting user is resolved from `userod` so staff need not decode log numbers.
pub fn period_close_history_report(
    log: &[SecurityEvent],
    users: &[Userod],
    closed: &ClosedPeriods,
) -> ReportTable {
    let mut table = ReportTable::new(&[
        "Time",
        "Month",
        "Action",
        "User",
        "Reason",
        "Previous close reason",
        "Current posting lock",
    ]);
    for event in period_close_events(log) {
        let time = event.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
        let user = user_label(users, event.user_num);
        let lock = if closed.all().iter().any(|p| p.key() == event.month) {
            "Locked"
        } else {
            "Unlocked"
        };
        table.push(&[
            &time,
            &event.month,
            event.action.label(),
            &user,
            &event.reason,
            event.previous_reason.as_deref().unwrap_or(""),
            lock,
        ]);
    }
    table
}

/// A tie-out is ready to sign off only when every non-informational check is
/// `OK`; that includes the unposted-source and account-setup `CHECK` rows.
pub fn tie_out_ready(table: &crate::ReportTable) -> Result<(), String> {
    let blockers: Vec<String> = table
        .rows
        .iter()
        .filter(|row| row.get(4).is_some_and(|status| status != "INFO" && status != "OK"))
        .map(|row| {
            format!(
                "{} ({})",
                row.first().map(String::as_str).unwrap_or("Check"),
                row.get(4).map(String::as_str).unwrap_or("?")
            )
        })
        .collect();
    if blockers.is_empty() {
        Ok(())
    } else {
        Err(format!("tie-out not reconciled: {}", blockers.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pref(value: &str) -> Vec<Pref> {
        vec![Pref {
            pref_num: 1,
            pref_name: CLOSED_PERIODS_PREF.to_string(),
            pref_value: value.to_string(),
        }]
    }

    #[test]
    fn month_bounds_cover_leap_february_and_december() {
        assert_eq!(
            month_bounds(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()),
            (
                NaiveDate::from_ymd_opt(2024, 2, 1).unwrap(),
                NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()
            )
        );
        assert_eq!(
            month_bounds(NaiveDate::from_ymd_opt(2026, 12, 24).unwrap()),
            (
                NaiveDate::from_ymd_opt(2026, 12, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()
            )
        );
    }

    #[test]
    fn closed_periods_round_trip_through_the_preference_row() {
        let mut periods = ClosedPeriods::default();
        let reason = "Month-end signed off by Ada; 2026-11-30\nsecond line";
        periods
            .close(
                NaiveDate::from_ymd_opt(2026, 11, 24).unwrap(),
                reason,
                "2026-11-30T18:00:00".into(),
            )
            .unwrap();
        periods
            .close(
                NaiveDate::from_ymd_opt(2026, 10, 5).unwrap(),
                "October close",
                "2026-10-31T17:00:00".into(),
            )
            .unwrap();
        let stored = periods.to_json();
        let loaded = ClosedPeriods::from_prefs(&pref(&stored));
        assert_eq!(loaded, periods);
        assert_eq!(loaded.all()[0].label(), "2026-10");
        assert_eq!(loaded.all()[1].reason, reason);
    }

    #[test]
    fn close_requires_a_reason_and_reopen_removes_the_gate() {
        let mut periods = ClosedPeriods::default();
        let date = NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
        assert!(periods.close(date, "   ", "now".into()).is_err());
        periods.close(date, "Final review", "now".into()).unwrap();
        assert!(periods.close(date, "Second attempt", "now".into()).is_err());
        assert!(periods.is_closed(date));
        assert!(periods
            .reject_posting(date, "Payment #12")
            .unwrap_err()
            .contains("Final review"));
        assert!(periods
            .reject_posting(NaiveDate::from_ymd_opt(2026, 12, 1).unwrap(), "Charge #1")
            .is_ok());
        assert_eq!(periods.reopen(date).unwrap().reason, "Final review");
        assert!(!periods.is_closed(date));
        assert!(periods.reopen(date).is_err());
    }

    #[test]
    fn in_range_finds_overlapping_closed_months_only() {
        let mut periods = ClosedPeriods::default();
        for (year, month) in [(2026, 10), (2026, 11), (2027, 1)] {
            periods
                .close(
                    NaiveDate::from_ymd_opt(year, month, 1).unwrap(),
                    "close",
                    "now".into(),
                )
                .unwrap();
        }
        let hits = periods.in_range(
            NaiveDate::from_ymd_opt(2026, 11, 15).unwrap(),
            NaiveDate::from_ymd_opt(2026, 12, 3).unwrap(),
        );
        assert_eq!(hits.iter().map(|p| p.label()).collect::<Vec<_>>(), ["2026-11"]);
    }

    #[test]
    fn malformed_preference_is_an_empty_gate() {
        assert!(ClosedPeriods::from_prefs(&pref("{not json")).is_empty());
        assert!(ClosedPeriods::from_prefs(&[]).is_empty());
    }

    fn audit(num: Id, text: &str, when: &str) -> SecurityEvent {
        audit_for(7, num, text, when)
    }

    fn audit_for(user: Id, num: Id, text: &str, when: &str) -> SecurityEvent {
        SecurityEvent {
            security_log_num: num,
            perm_type: od_domain::Permission::ADJUSTMENT_CREATE,
            user_num: user,
            log_datetime: NaiveDateTime::parse_from_str(when, "%Y-%m-%d %H:%M:%S").unwrap(),
            log_text: text.to_string(),
            pat_num: 0,
        }
    }

    fn user(num: Id, name: &str) -> Userod {
        Userod {
            user_num: num,
            user_name: name.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn period_close_audit_texts_parse_back_into_events() {
        let events = period_close_events(&[
            audit(
                1,
                &close_audit_text("2026-11", "Month-end signed off"),
                "2026-11-30 18:00:00",
            ),
            audit(
                2,
                &reopen_audit_text("2026-11", "Month-end signed off", "Late correction #209"),
                "2026-12-01 09:00:00",
            ),
            audit(3, "User: Admin has logged off.", "2026-12-01 09:01:00"),
        ]);
        assert_eq!(events.len(), 2);
        // Newest first: the reopen precedes the close it undid.
        assert_eq!(events[0].action, PeriodCloseAction::Reopened);
        assert_eq!(events[0].user_num, 7);
        assert_eq!(events[0].month, "2026-11");
        assert_eq!(events[0].reason, "Late correction #209");
        assert_eq!(
            events[0].previous_reason.as_deref(),
            Some("Month-end signed off")
        );
        assert_eq!(events[1].action, PeriodCloseAction::Closed);
        assert_eq!(events[1].reason, "Month-end signed off");
        assert_eq!(events[1].previous_reason, None);
    }

    #[test]
    fn history_report_shows_reason_timestamp_and_current_lock() {
        let date = NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
        let mut periods = ClosedPeriods::default();
        periods
            .close(date, "Late correction posted", "2026-12-01 10:00:00".into())
            .unwrap();
        let log = vec![
            audit(
                1,
                &close_audit_text("2026-11", "Final sign-off"),
                "2026-11-30 18:00:00",
            ),
            audit(
                2,
                &reopen_audit_text("2026-11", "Final sign-off", "Late correction"),
                "2026-12-01 09:00:00",
            ),
            audit(
                3,
                &close_audit_text("2026-11", "Late correction posted"),
                "2026-12-01 10:00:00",
            ),
        ];
        let users = [user(7, "Ada")];
        let table = period_close_history_report(&log, &users, &periods);
        assert_eq!(
            table.headers,
            [
                "Time",
                "Month",
                "Action",
                "User",
                "Reason",
                "Previous close reason",
                "Current posting lock",
            ]
        );
        assert_eq!(table.rows.len(), 3);
        assert_eq!(table.rows[0][0], "2026-12-01 10:00:00");
        assert_eq!(table.rows[0][2], "Closed");
        assert_eq!(table.rows[0][3], "Ada");
        assert_eq!(table.rows[0][4], "Late correction posted");
        assert_eq!(table.rows[0][6], "Locked");
        assert_eq!(table.rows[1][2], "Reopened");
        assert_eq!(table.rows[1][5], "Final sign-off");
        // The month was re-signed after that reopen, so it is locked again now.
        assert_eq!(table.rows[1][6], "Locked");

        let open = period_close_history_report(&log, &users, &ClosedPeriods::default());
        assert!(open.rows.iter().all(|row| row[6] == "Unlocked"));
    }

    #[test]
    fn history_report_resolves_user_names_with_stale_and_local_fallbacks() {
        let log = vec![
            audit_for(
                7,
                1,
                &close_audit_text("2026-11", "By Ada"),
                "2026-11-30 18:00:00",
            ),
            audit_for(
                99,
                2,
                &reopen_audit_text("2026-11", "By Ada", "By deleted user"),
                "2026-12-01 09:00:00",
            ),
            audit_for(
                0,
                3,
                &close_audit_text("2026-11", "By local session"),
                "2026-12-01 10:00:00",
            ),
            audit_for(
                8,
                4,
                &close_audit_text("2026-11", "By blank name"),
                "2026-12-01 11:00:00",
            ),
        ];
        // Newest first: blank name, local, stale user number, resolved name.
        let table = period_close_history_report(
            &log,
            &[user(7, "Ada"), user(8, "  ")],
            &ClosedPeriods::default(),
        );
        let names: Vec<&str> = table.rows.iter().map(|row| row[3].as_str()).collect();
        assert_eq!(names, ["User #8", "Local user", "User #99", "Ada"]);
    }

    #[test]
    fn malformed_or_unrelated_log_rows_are_ignored() {
        let log = vec![
            audit(
                1,
                "Closed accounting period 2026-13: bad month",
                "2026-11-30 18:00:00",
            ),
            audit(2, "Closed accounting period nonsense", "2026-11-30 18:00:00"),
            audit(
                3,
                "Reopened accounting period 2026-11 (bad separator",
                "2026-11-30 18:00:00",
            ),
        ];
        assert!(period_close_events(&log).is_empty());
    }
}
