//! End-to-end check that discount plans and subscriptions round-trip through
//! the DemoStore and feed the same joined rows as `FormRpDiscountPlan`.

use chrono::{Duration, Local};
use od_core::{
    annual_bucket, discount_amount, discount_plan_report, DiscountInput, DiscountPlanSubscription,
    FrequencyCodeSets,
};
use od_db::{DemoStore, Store};
use od_domain::{Adjustment, DiscountPlan, DiscountPlanSub, Procedure, ProcedureStatus};

/// Same join as the GUI's `gather_discount_subscriptions`.
fn joined(store: &DemoStore) -> Vec<DiscountPlanSubscription> {
    let plans = store.list_discount_plans().unwrap();
    let subs = store.list_discount_plan_subs().unwrap();
    let fee_schedules = store.list_fee_schedules().unwrap();
    let definitions = store.list_definitions().unwrap();
    let patients = store.list_patients().unwrap();
    subs.iter()
        .filter_map(|sub| {
            let plan = plans
                .iter()
                .find(|p| p.discount_plan_num == sub.discount_plan_num)?;
            let patient = patients.iter().find(|p| p.pat_num == sub.pat_num)?;
            Some(DiscountPlanSubscription {
                description: plan.description.clone(),
                fee_schedule: fee_schedules
                    .iter()
                    .find(|f| f.fee_sched_num == plan.fee_sched_num)
                    .map(|f| f.description.clone())
                    .unwrap_or_default(),
                adj_type: definitions
                    .iter()
                    .find(|d| d.def_num == plan.def_num)
                    .map(|d| d.item_name.clone())
                    .unwrap_or_default(),
                date_effective: sub.date_effective,
                date_term: sub.date_term,
                patient: patient.display_name(),
            })
        })
        .collect()
}

#[test]
fn seeded_discount_plans_feed_the_report_with_resolved_names() {
    let store = DemoStore::seeded();
    let rows = joined(&store);
    assert_eq!(rows.len(), 2, "both seeded subscribers are reported");
    assert!(rows.iter().all(|row| row.fee_schedule == "Reduced Fees"));
    assert!(rows.iter().all(|row| row.adj_type == "Courtesy adjustment"));

    let table = discount_plan_report(&rows, "");
    assert_eq!(
        table.headers,
        [
            "Description",
            "FeeSched",
            "AdjType",
            "DateEffective",
            "DateTerm",
            "Patient"
        ]
    );
    assert_eq!(table.rows.len(), 2);
    assert!(table.rows[0][0].contains("Community Smile") || table.rows[1][0].contains("Community Smile"));

    let filtered = discount_plan_report(&rows, "senior");
    assert_eq!(filtered.rows.len(), 1, "description filter is case-insensitive");
    assert!(filtered.rows[0][0].contains("Senior"));
}

#[test]
fn discount_plan_writes_round_trip_and_subscriptions_can_end_and_reopen() {
    let store = DemoStore::seeded();
    let today = Local::now().date_naive();

    // New plans and subscriptions get keys assigned by the backend.
    let num = store
        .upsert_discount_plan(&DiscountPlan {
            discount_plan_num: 0,
            description: "Pensioner Plan".into(),
            fee_sched_num: 54,
            def_num: 1,
            is_hidden: false,
            note: "Test plan".into(),
            annual_max: 250.0,
            ..Default::default()
        })
        .unwrap();
    assert!(num > 2, "new key follows the seeded plans");

    let sub_num = store
        .upsert_discount_plan_sub(&DiscountPlanSub {
            discount_sub_num: 0,
            discount_plan_num: num,
            pat_num: 2,
            date_effective: Some(today - Duration::days(10)),
            date_term: None,
            note: "Enrolled today".into(),
        })
        .unwrap();
    assert!(sub_num > 2, "new key follows the seeded subscriptions");

    let subs = store.list_discount_plan_subs().unwrap();
    let sub = subs.iter().find(|s| s.discount_sub_num == sub_num).unwrap();
    assert!(sub.is_valid_for_date(today));
    assert!(sub.is_open_ended());

    // Editing keeps the same key and updates every field.
    store
        .upsert_discount_plan(&DiscountPlan {
            discount_plan_num: num,
            description: "Pensioner Plan Plus".into(),
            annual_max: 300.0,
            ..store
                .list_discount_plans()
                .unwrap()
                .into_iter()
                .find(|p| p.discount_plan_num == num)
                .unwrap()
        })
        .unwrap();
    let plan = store
        .list_discount_plans()
        .unwrap()
        .into_iter()
        .find(|p| p.discount_plan_num == num)
        .unwrap();
    assert_eq!(plan.description, "Pensioner Plan Plus");
    assert_eq!(plan.annual_max, 300.0);
    assert_eq!(store.list_discount_plans().unwrap().len(), 3, "no duplicate");

    // Ending a subscription sets DateTerm but keeps the row for history.
    let term = today;
    store.terminate_discount_plan_sub(sub_num, term).unwrap();
    let sub = store
        .list_discount_plan_subs()
        .unwrap()
        .into_iter()
        .find(|s| s.discount_sub_num == sub_num)
        .unwrap();
    assert_eq!(sub.date_term, Some(term));
    assert!(sub.is_valid_for_date(term), "term date is inclusive");
    assert!(!sub.is_open_ended());
    assert!(!sub.is_valid_for_date(term + Duration::days(1)));
    assert_eq!(store.list_discount_plan_subs().unwrap().len(), 3);

    // Reopening clears the term again without inserting a new row.
    let mut reopened = sub;
    reopened.date_term = None;
    store.upsert_discount_plan_sub(&reopened).unwrap();
    let sub = store
        .list_discount_plan_subs()
        .unwrap()
        .into_iter()
        .find(|s| s.discount_sub_num == sub_num)
        .unwrap();
    assert!(sub.is_open_ended());
    assert!(sub.is_valid_for_date(today + Duration::days(365)));
    assert_eq!(store.list_discount_plan_subs().unwrap().len(), 3);

    // The report sees the new subscriber after the write.
    let table = discount_plan_report(&joined(&store), "pensioner");
    assert_eq!(table.rows.len(), 1);
    assert!(table.rows[0][0].contains("Pensioner Plan Plus"));
}

/// Exercises the same sequence as the Chart "Complete" action: insert a
/// planned procedure, price it from the plan's fee schedule, post the negative
/// adjustment, then mark it complete. The pure pricing rules themselves are
/// unit tested in od-core; this test guards the store round trip.
#[test]
fn completing_a_procedure_posts_a_negative_discount_adjustment() {
    let store = DemoStore::seeded();
    let today = Local::now().date_naive();
    let pat_num = 1; // subscriber to the seeded Community Smile Plan

    let proc_num = store
        .insert_procedure(&Procedure {
            pat_num,
            code_num: 1,
            proc_date: today,
            fee: 100.0,
            unit_qty: 1.0,
            provider_num: 1,
            ..Default::default()
        })
        .unwrap();
    let proc = store
        .list_procedures(pat_num)
        .unwrap()
        .into_iter()
        .find(|p| p.proc_num == proc_num)
        .unwrap();
    assert_eq!(proc.proc_status, ProcedureStatus::TreatmentPlanned);

    let plan = store
        .list_discount_plans()
        .unwrap()
        .into_iter()
        .find(|p| p.discount_plan_num == 1)
        .unwrap();
    let sub = store
        .list_discount_plan_subs()
        .unwrap()
        .into_iter()
        .find(|s| s.pat_num == pat_num)
        .unwrap();
    let fee = store
        .list_fees(plan.fee_sched_num, 0)
        .unwrap()
        .into_iter()
        .find(|f| f.code_num == proc.code_num)
        .unwrap()
        .amount;
    assert_eq!(fee, 75.0, "seeded reduced fee for code 1");

    let history = store.list_procedures(pat_num).unwrap();
    let (start, end) = annual_bucket(
        proc.proc_date,
        sub.date_effective,
        sub.date_term,
        today,
    );
    let running_total: f64 = store
        .list_adjustments(pat_num)
        .unwrap()
        .iter()
        .filter(|a| {
            a.adj_amount < 0.0
                && a.adj_type == plan.def_num
                && a.proc_num > 0
                && a.proc_num != proc.proc_num
                && a.proc_date >= start
                && a.proc_date <= end
        })
        .map(|a| -a.adj_amount)
        .sum();
    let result = discount_amount(&DiscountInput {
        procedure: &proc,
        plan: &plan,
        subscription: &sub,
        discount_fee: Some(fee),
        running_total,
        history: &history,
        frequency_codes: &FrequencyCodeSets::default(),
        today,
    });
    assert_eq!(result.amount, 25.0, "100 charged minus 75 reduced fee");

    let adj_num = store
        .upsert_adjustment(&Adjustment {
            adj_date: today,
            proc_date: proc.proc_date,
            adj_amount: -result.amount,
            pat_num,
            adj_type: plan.def_num,
            provider_num: proc.provider_num,
            proc_num: proc.proc_num,
            note: format!("Discount plan: {}", plan.description),
            ..Default::default()
        })
        .unwrap();
    store
        .set_procedure_complete(proc_num, result.amount)
        .unwrap();

    let adjustment = store
        .list_adjustments(pat_num)
        .unwrap()
        .into_iter()
        .find(|a| a.adj_num == adj_num)
        .unwrap();
    assert_eq!(adjustment.adj_amount, -25.0);
    assert_eq!(adjustment.proc_num, proc_num);
    assert_eq!(adjustment.adj_type, plan.def_num);
    assert_eq!(adjustment.proc_date, today);

    let completed = store
        .list_procedures(pat_num)
        .unwrap()
        .into_iter()
        .find(|p| p.proc_num == proc_num)
        .unwrap();
    assert_eq!(completed.proc_status, ProcedureStatus::Complete);
    assert_eq!(completed.proc_date, today, "service date is not rewritten");
}
