//! End-to-end check that `FormRpPatPortionUncollected` reads the real store
//! writes: a completed procedure, an adjustment and a patient payment all
//! combine into the patient's remaining balance.

use chrono::Local;
use od_core::patient_portion_uncollected_report;
use od_db::{DemoStore, Store};
use od_domain::{Adjustment, Payment, Procedure, ProcedureStatus};

#[test]
fn demo_store_feeds_patient_portion_uncollected_report() {
    let store = DemoStore::seeded();
    let today = Local::now().date_naive();
    let pat_num = 1;
    let from = today - chrono::Duration::days(30);
    let to = today + chrono::Duration::days(30);

    let mut proc = Procedure {
        pat_num,
        code_num: 1,
        proc_date: today,
        fee: 100.0,
        unit_qty: 1.0,
        ..Default::default()
    };
    let proc_num = store.insert_procedure(&proc).unwrap();
    store
        .set_procedure_complete(proc_num, 0.0)
        .expect("complete failed");
    proc.proc_num = proc_num;
    proc.proc_status = ProcedureStatus::Complete;

    store
        .upsert_adjustment(&Adjustment {
            adj_date: today,
            proc_date: today,
            proc_num,
            pat_num,
            adj_amount: -20.0,
            ..Default::default()
        })
        .unwrap();
    store
        .record_payment(
            &Payment {
                pat_num,
                pay_date: today,
                pay_amount: 30.0,
                ..Default::default()
            },
            proc_num,
            0,
        )
        .unwrap();

    let procedures = store.list_procedures(pat_num).unwrap();
    let table = patient_portion_uncollected_report(
        &store.list_patients().unwrap(),
        &procedures,
        &store.list_procedure_codes().unwrap(),
        &store.list_adjustments(pat_num).unwrap(),
        &store.list_pay_splits(pat_num).unwrap(),
        &store.list_claim_procs(pat_num).unwrap(),
        &store.list_definitions().unwrap(),
        from,
        to,
        false,
    );

    let expected = ["100.00", "100.00", "-20.00", "30.00", "50.00"];
    assert!(
        table.rows.iter().any(|row| {
            row.len() == 8 && row[3..] == expected
        }),
        "expected a 100 fee, -20 adjustment, 30 paid and 50 uncollected; got {:?}",
        table.rows
    );
}
