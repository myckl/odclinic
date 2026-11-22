//! End-to-end check that the seeded demo ledger feeds `FormRpCustomAging`'s
//! builder with the same transaction classification the live query would give.

use chrono::Local;
use od_core::custom_aging_report;
use od_db::{DemoStore, Store};
use od_domain::reports::{CustomAgingInclude, CustomAgingReportOptions};
use od_domain::{Adjustment, Patient, PayPlanCharge, PaySplit, Procedure};

fn ledger(
    store: &DemoStore,
) -> (
    Vec<Procedure>,
    Vec<Adjustment>,
    Vec<PaySplit>,
    Vec<PayPlanCharge>,
    Vec<Patient>,
) {
    let mut procedures = Vec::new();
    let mut adjustments = Vec::new();
    let mut splits = Vec::new();
    let mut charges = Vec::new();
    for patient in store.list_patients().unwrap() {
        procedures.extend(store.list_procedures(patient.pat_num).unwrap());
        adjustments.extend(store.list_adjustments(patient.pat_num).unwrap());
        splits.extend(store.list_pay_splits(patient.pat_num).unwrap());
        charges.extend(store.list_pay_plan_charges(patient.pat_num).unwrap());
    }
    (procedures, adjustments, splits, charges, store.list_patients().unwrap())
}

#[test]
fn demo_custom_aging_report_classifies_demo_ledger_transactions() {
    let store = DemoStore::seeded();
    let (procedures, adjustments, splits, charges, patients) = ledger(&store);
    let table = custom_aging_report(
        &patients,
        &procedures,
        &adjustments,
        &splits,
        &charges,
        &[],
        &[],
        Local::now().date_naive(),
        false,
        CustomAgingReportOptions::default(),
    );
    assert_eq!(table.headers, [
        "Patient", "0-30 Days", "31-60 Days", "61-90 Days", "> 90 Days", "Total"
    ]);
    // Patient 1: 120 fee − 10 adjustment − 60 payment = 50; patient 3:
    // 185 fee − 37 copay = 148. The plan debit is off by default.
    assert_eq!(table.rows.len(), 3, "two patients plus the Totals row");
    assert_eq!(table.rows[0], ["Alvarez, Marta", "50.00", "0.00", "0.00", "0.00", "50.00"]);
    assert_eq!(table.rows[1], ["Chen, Lily", "148.00", "0.00", "0.00", "0.00", "148.00"]);
    assert_eq!(table.rows[2], ["Totals:", "198.00", "0.00", "0.00", "0.00", "198.00"]);
}

#[test]
fn demo_custom_aging_report_honours_the_transaction_include_flags() {
    let store = DemoStore::seeded();
    let (procedures, adjustments, splits, charges, patients) = ledger(&store);
    let as_of = Local::now().date_naive();
    let build = |options| {
        custom_aging_report(
            &patients, &procedures, &adjustments, &splits, &charges, &[], &[], as_of, false,
            options,
        )
    };

    // Selecting the plan debit adds patient 1's 60.00 instalment charge.
    let with_charges = build(CustomAgingReportOptions {
        include: CustomAgingInclude {
            pay_plan_charges: true,
            ..Default::default()
        },
        ..Default::default()
    });
    assert_eq!(with_charges.rows[0][1], "110.00");
    assert_eq!(with_charges.rows.last().unwrap()[5], "258.00");

    // Upstream refuses to refresh with no transaction type selected.
    let none = build(CustomAgingReportOptions {
        include: CustomAgingInclude {
            procedure_fees: false,
            adjustments: false,
            pay_plan_charges: false,
            pay_plan_production: false,
            patient_payments: false,
            ..CustomAgingInclude::default()
        },
        ..Default::default()
    });
    assert!(none.rows.is_empty());
}
