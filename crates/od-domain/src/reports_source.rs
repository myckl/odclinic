//! Read-only support models for report fidelity tranches.
//!
//! These mirror Open Dental tables that the first two report tranches did not
//! need. Most are read-only: the port never writes schedules, claim snapshots
//! or public-health screening rows. Discount plans and their subscriptions are
//! the exception — they are editable in Setup because they are the
//! patient-facing pricing mechanism in an insurance-free practice. Column names
//! follow the installed OD 25.3 demo schema and the public 24.3 business layer.

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime};

use crate::Id;

/// OD `schedule` — a calendar block on a date for a provider. `scheduleop`
/// rows link a block to one or more operatories.
#[derive(Clone, Debug, Default)]
pub struct Schedule {
    pub schedule_num: Id,
    pub sched_date: NaiveDate,
    pub start_time: NaiveTime,
    pub stop_time: NaiveTime,
    /// OD `ScheduleType`: Practice=0, Provider=1, Blockout=2, Employee=3,
    /// WebSchedASAP=4. Only Provider rows with `SchedStatus::Open` count toward
    /// the production goal.
    pub sched_type: u8,
    pub prov_num: Id,
    pub blockout_type: Id,
    pub note: String,
    /// OD `SchedStatus`: Open=0, Closed=1, Holiday=2.
    pub status: u8,
    pub employee_num: Id,
    pub clinic_num: Id,
}

/// OD `scheduleop` — join row between `schedule` and `operatory`.
#[derive(Clone, Debug, Default)]
pub struct ScheduleOp {
    pub schedule_op_num: Id,
    pub schedule_num: Id,
    pub operatory_num: Id,
}

/// OD `payplanlink` — ties a dynamic payment plan to a procedure, adjustment
/// or insurance estimate. `fkey` is the linked row's primary key; its meaning
/// depends on `link_type`.
#[derive(Clone, Debug, Default)]
pub struct PayPlanLink {
    pub pay_plan_link_num: Id,
    pub pay_plan_num: Id,
    /// OD `PayPlanLinkType`: None=0, Adjustment=1, Procedure=2, OrthoCase=3.
    pub link_type: i8,
    pub fkey: Id,
    /// Overrides the linked fee/amount when non-zero.
    pub amount_override: f64,
}

impl PayPlanLink {
    pub const NONE: i8 = 0;
    pub const ADJUSTMENT: i8 = 1;
    pub const PROCEDURE: i8 = 2;
    pub const ORTHO_CASE: i8 = 3;
}

/// OD `claimsnapshot` — a historical copy of a `claimproc` row taken when a
/// claim is created or received. Provider payroll uses these instead of the
/// current claim values so later edits cannot change a past report.
#[derive(Clone, Debug, Default)]
pub struct ClaimSnapshot {
    pub claim_snapshot_num: Id,
    pub proc_num: Id,
    pub claim_type: String,
    pub write_off: f64,
    pub ins_pay_est: f64,
    pub fee: f64,
    pub date_t_entry: NaiveDateTime,
    pub claim_proc_num: Id,
    pub snapshot_trigger: i8,
}

/// OD `discountplan` header.
///
/// All limits mirror upstream: `-1` means "no limit" for the annual maximum
/// and for each frequency category (OD `DiscountPlan.cs`). Storing them here
/// keeps live OD rows lossless even though a practice may leave a category
/// unrestricted.
#[derive(Clone, Debug)]
pub struct DiscountPlan {
    pub discount_plan_num: Id,
    pub description: String,
    pub fee_sched_num: Id,
    /// FK to `definition.DefNum`; the discount adjustment type.
    pub def_num: Id,
    pub is_hidden: bool,
    pub note: String,
    pub exam_freq_limit: i32,
    pub xray_freq_limit: i32,
    pub prophy_freq_limit: i32,
    pub fluoride_freq_limit: i32,
    pub perio_freq_limit: i32,
    pub limited_exam_freq_limit: i32,
    pub pa_freq_limit: i32,
    /// `-1` means unlimited (OD `AnnualMax = -1`).
    pub annual_max: f64,
}

impl Default for DiscountPlan {
    fn default() -> Self {
        Self {
            discount_plan_num: 0,
            description: String::new(),
            fee_sched_num: 0,
            def_num: 0,
            is_hidden: false,
            note: String::new(),
            exam_freq_limit: -1,
            xray_freq_limit: -1,
            prophy_freq_limit: -1,
            fluoride_freq_limit: -1,
            perio_freq_limit: -1,
            limited_exam_freq_limit: -1,
            pa_freq_limit: -1,
            annual_max: -1.0,
        }
    }
}

impl DiscountPlan {
    /// The seven frequency limits in OD's `HasMetDiscountFrequencyLimitation`
    /// order. Prefix order matters for the priority categories used by the
    /// treatment-plan estimator.
    pub fn frequency_limits(&self) -> [i32; 7] {
        [
            self.exam_freq_limit,
            self.xray_freq_limit,
            self.prophy_freq_limit,
            self.fluoride_freq_limit,
            self.perio_freq_limit,
            self.limited_exam_freq_limit,
            self.pa_freq_limit,
        ]
    }
}

/// OD `discountplansub` — one patient's subscription to a discount plan.
#[derive(Clone, Debug, Default)]
pub struct DiscountPlanSub {
    pub discount_sub_num: Id,
    pub discount_plan_num: Id,
    pub pat_num: Id,
    pub date_effective: Option<NaiveDate>,
    pub date_term: Option<NaiveDate>,
    pub note: String,
}

impl DiscountPlanSub {
    /// Mirrors OD `DiscountPlanSub.IsValidForDate`: a term date before 1880
    /// (including OD's zero date `0001-01-01`) means the subscription is open
    /// ended. A missing effective date is treated as open ended too.
    pub fn is_valid_for_date(&self, date: NaiveDate) -> bool {
        let started = self
            .date_effective
            .map(|effective| date >= effective)
            .unwrap_or(true);
        let not_ended = self.is_open_ended() || self.date_term.is_some_and(|term| date <= term);
        started && not_ended
    }

    /// True when `date_term` represents "no end" upstream — `None`, or OD's
    /// zero date / any year before 1880 (`DiscountPlanSubs.GetAnnualMaxDateTerm`).
    pub fn is_open_ended(&self) -> bool {
        self.date_term
            .map(|term| term.year() < 1880)
            .unwrap_or(true)
    }
}

/// OD `screengroup` — a public-health screening event.
#[derive(Clone, Debug, Default)]
pub struct ScreenGroup {
    pub screen_group_num: Id,
    pub description: String,
    pub sg_date: Option<NaiveDate>,
    pub prov_name: String,
    pub prov_num: Id,
    pub place_service: i8,
    pub county: String,
    pub grade_school: String,
}

/// OD `screen` — one patient's record within a `screengroup`.
#[derive(Clone, Debug, Default)]
pub struct Screen {
    pub screen_num: Id,
    pub gender: u8,
    pub race_old: i8,
    pub grade_level: i8,
    pub age: u8,
    pub urgency: i8,
    pub has_caries: bool,
    pub needs_sealants: bool,
    pub caries_experience: bool,
    pub early_child_caries: bool,
    pub existing_sealants: bool,
    pub missing_all_teeth: bool,
    pub birth_date: Option<NaiveDate>,
    pub screen_group_num: Id,
    pub screen_group_order: u16,
    pub comments: String,
    pub screen_pat_num: Id,
}

/// OD `site` — a service site used by the raw public-health extracts.
#[derive(Clone, Debug, Default)]
pub struct Site {
    pub site_num: Id,
    pub description: String,
    pub note: String,
    pub address: String,
    pub address2: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub prov_num: Id,
    pub place_service: i8,
}

/// OD `county` — county code/name lookup.
#[derive(Clone, Debug, Default)]
pub struct County {
    pub county_num: Id,
    pub county_name: String,
    pub county_code: String,
}

/// OD `patientrace` — a patient's race as a CDC race code.
#[derive(Clone, Debug, Default)]
pub struct PatientRace {
    pub patient_race_num: Id,
    pub pat_num: Id,
    pub race: i8,
    pub cdcrec_code: String,
}

/// OD `carrier` — needed by the laser-label insurance-company tab.
#[derive(Clone, Debug, Default)]
pub struct Carrier {
    pub carrier_num: Id,
    pub carrier_name: String,
    pub address: String,
    pub address2: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub phone: String,
    pub is_hidden: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(effective: Option<NaiveDate>, term: Option<NaiveDate>) -> DiscountPlanSub {
        DiscountPlanSub {
            date_effective: effective,
            date_term: term,
            ..Default::default()
        }
    }

    #[test]
    fn discount_plan_sub_validity_follows_upstream_date_rules() {
        let start = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2025, 12, 31).unwrap();
        let inside = NaiveDate::from_ymd_opt(2025, 6, 15).unwrap();

        let bounded = sub(Some(start), Some(end));
        assert!(bounded.is_valid_for_date(inside));
        assert!(bounded.is_valid_for_date(start), "effective date is inclusive");
        assert!(bounded.is_valid_for_date(end), "term date is inclusive");
        assert!(!bounded.is_valid_for_date(start - chrono::Duration::days(1)));
        assert!(!bounded
            .is_valid_for_date(end + chrono::Duration::days(1)));
        assert!(!bounded.is_open_ended());

        // OD's zero date and any pre-1880 term mean "no end".
        let zero = NaiveDate::from_ymd_opt(1, 1, 1).unwrap();
        let open = sub(Some(start), Some(zero));
        assert!(open.is_open_ended());
        assert!(open.is_valid_for_date(inside));
        assert!(open.is_valid_for_date(NaiveDate::from_ymd_opt(2099, 1, 1).unwrap()));
        assert!(sub(Some(start), None).is_open_ended());
        assert!(sub(None, None).is_valid_for_date(inside));
    }
}
