//! Treatment-plan workflow rules derived from Open Dental 24_3.
//!
//! OD keeps a single Active plan per patient (`TreatPlans.AuditPlans` and
//! `SetOtherActiveTPsToInactive` in `Data Interface/TreatPlans.cs`) and stores
//! Saved plans as immutable `proctp` snapshots (`CreateArchivedTreatPlan`).
//! Both rules live here so the GUI and both backends behave identically.

use chrono::{NaiveDate, NaiveDateTime};
use od_domain::*;

/// Headings OD uses when it creates a plan or promotes one to Active.
pub fn default_heading(status: TreatPlanStatus) -> &'static str {
    match status {
        TreatPlanStatus::Saved => "Proposed Treatment Plan",
        TreatPlanStatus::Active => "Active Treatment Plan",
        TreatPlanStatus::Inactive => "Inactive Treatment Plan",
    }
}

/// Build a new plan with the heading and status OD's "New Treatment Plan"
/// button uses. OD creates a new *Inactive* plan; "Make Active" is a separate
/// action so the user can assemble the plan before exposing it to the chart.
pub fn new_treat_plan(pat_num: Id, status: TreatPlanStatus, note: &str) -> TreatPlan {
    TreatPlan {
        treat_plan_num: 0,
        pat_num,
        date_saved: None,
        heading: default_heading(status).to_string(),
        status,
        provider_num: 0,
        note: note.to_string(),
    }
}

/// The plans that must be written to promote `target` to `new_status`.
///
/// Only the target and any demoted plans are returned; callers simply feed the
/// result to `Store::upsert_treat_plan`. This is the port's equivalent of OD's
/// `SetOtherActiveTPsToInactive`: an Active plan is unique per patient, and a
/// demoted plan's default heading is renamed so the list stays unambiguous.
pub fn set_status(
    plans: &[TreatPlan],
    target: Id,
    new_status: TreatPlanStatus,
) -> Vec<TreatPlan> {
    let mut changed = Vec::new();
    for plan in plans {
        if plan.treat_plan_num == target {
            if plan.status != new_status {
                let mut updated = plan.clone();
                updated.status = new_status;
                if new_status == TreatPlanStatus::Active
                    && matches!(
                        updated.heading.as_str(),
                        "Inactive Treatment Plan" | "Unassigned"
                    )
                {
                    updated.heading = default_heading(TreatPlanStatus::Active).to_string();
                } else if new_status == TreatPlanStatus::Inactive
                    && updated.heading == default_heading(TreatPlanStatus::Active)
                {
                    updated.heading = default_heading(TreatPlanStatus::Inactive).to_string();
                }
                changed.push(updated);
            }
        } else if new_status == TreatPlanStatus::Active && plan.status == TreatPlanStatus::Active {
            let mut demoted = plan.clone();
            demoted.status = TreatPlanStatus::Inactive;
            if demoted.heading == default_heading(TreatPlanStatus::Active) {
                demoted.heading = default_heading(TreatPlanStatus::Inactive).to_string();
            }
            changed.push(demoted);
        }
    }
    changed
}

/// Create the immutable Saved snapshot OD's "Save TP" toolbar button makes.
///
/// The new plan keeps the source heading and note, gets `DateTP = today`, and
/// its lines get fresh keys and a stable `ItemOrder`. The caller writes the
/// plan first so the returned lines can carry its generated `TreatPlanNum`.
pub fn snapshot_plan(
    source: &TreatPlan,
    lines: &[TreatPlanLine],
    today: NaiveDate,
) -> (TreatPlan, Vec<TreatPlanLine>) {
    let when: NaiveDateTime = today
        .and_hms_opt(0, 0, 0)
        .unwrap_or_else(|| NaiveDateTime::default());
    let plan = TreatPlan {
        treat_plan_num: 0,
        pat_num: source.pat_num,
        date_saved: Some(when),
        heading: source.heading.clone(),
        status: TreatPlanStatus::Saved,
        provider_num: source.provider_num,
        note: source.note.clone(),
    };
    let copied = lines
        .iter()
        .enumerate()
        .map(|(index, line)| TreatPlanLine {
            line_num: 0,
            treat_plan_num: 0,
            pat_num: source.pat_num,
            proc_num: line.proc_num,
            item_order: index as i32,
            priority: line.priority,
            tooth_num: line.tooth_num.clone(),
            surf: line.surf.clone(),
            proc_code: line.proc_code.clone(),
            description: line.description.clone(),
            fee: line.fee,
            provider_num: line.provider_num,
            date_tp: line.date_tp.or(Some(today)),
        })
        .collect();
    (plan, copied)
}

/// Reject a plan OD's editor would not save: the heading is mandatory.
pub fn validate_treat_plan(plan: &TreatPlan) -> Result<(), String> {
    if plan.heading.trim().is_empty() {
        return Err("Treatment plan heading cannot be empty.".to_string());
    }
    Ok(())
}

/// Total of the line fees shown on a plan.
pub fn treat_plan_total(lines: &[TreatPlanLine]) -> f64 {
    lines.iter().map(|line| line.fee).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(num: Id, status: TreatPlanStatus) -> TreatPlan {
        TreatPlan {
            treat_plan_num: num,
            pat_num: 7,
            heading: default_heading(status).to_string(),
            status,
            ..Default::default()
        }
    }

    #[test]
    fn new_plan_is_inactive_with_od_heading() {
        let new = new_treat_plan(4, TreatPlanStatus::Inactive, "Phase 1");
        assert_eq!(new.heading, "Inactive Treatment Plan");
        assert_eq!(new.status.as_od(), 2);
        assert_eq!(new.note, "Phase 1");
        assert_eq!(new.pat_num, 4);
    }

    #[test]
    fn promoting_a_plan_demotes_the_other_active_plan() {
        let plans = vec![
            plan(1, TreatPlanStatus::Active),
            plan(2, TreatPlanStatus::Inactive),
        ];
        let changes = set_status(&plans, 2, TreatPlanStatus::Active);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].treat_plan_num, 1);
        assert_eq!(changes[0].status, TreatPlanStatus::Inactive);
        assert_eq!(changes[0].heading, "Inactive Treatment Plan");
        assert_eq!(changes[1].treat_plan_num, 2);
        assert_eq!(changes[1].status, TreatPlanStatus::Active);
        assert_eq!(changes[1].heading, "Active Treatment Plan");
    }

    #[test]
    fn changing_to_inactive_only_touches_the_target() {
        let plans = vec![plan(1, TreatPlanStatus::Active), plan(2, TreatPlanStatus::Active)];
        let changes = set_status(&plans, 1, TreatPlanStatus::Inactive);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].treat_plan_num, 1);
        assert_eq!(changes[0].heading, "Inactive Treatment Plan");
    }

    #[test]
    fn snapshot_resets_keys_and_keeps_line_data() {
        let source = plan(1, TreatPlanStatus::Active);
        let lines = vec![TreatPlanLine {
            line_num: 18,
            treat_plan_num: 1,
            proc_num: 42,
            fee: 250.0,
            description: "Crown".into(),
            ..Default::default()
        }];
        let today = NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let (saved, copied) = snapshot_plan(&source, &lines, today);
        assert_eq!(saved.status.as_od(), 0);
        assert_eq!(saved.date_saved.map(|d| d.date()), Some(today));
        assert_eq!(saved.treat_plan_num, 0);
        assert_eq!(copied.len(), 1);
        assert_eq!(copied[0].line_num, 0);
        assert_eq!(copied[0].treat_plan_num, 0);
        assert_eq!(copied[0].proc_num, 42);
        assert_eq!(copied[0].fee, 250.0);
        assert_eq!(copied[0].date_tp, Some(today));
        assert_eq!(treat_plan_total(&copied), 250.0);
    }

    #[test]
    fn empty_heading_is_rejected() {
        let mut plan = plan(1, TreatPlanStatus::Saved);
        plan.heading = "  ".into();
        assert!(validate_treat_plan(&plan).is_err());
        plan.heading = "Proposed".into();
        assert!(validate_treat_plan(&plan).is_ok());
    }
}
