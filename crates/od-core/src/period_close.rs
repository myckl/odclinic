//! Audited period-close gate for the double-entry journal.
//!
//! A closed month is persisted as one OD preference row so the lock survives
//! restarts and is shared by the demo and MySQL backends. Posting helpers read
//! the same `ClosedPeriods` snapshot the GUI shows; a source dated inside a
//! closed month is never written, and every close/reopen is written to the OD
//! security log by the caller.

use chrono::{Datelike, NaiveDate};
use od_domain::Pref;
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
}
