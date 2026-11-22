//! End-to-end check that `FormRpTreatmentFinder` reads the real store: a
//! treatment-planned procedure becomes a row, and a patient with a future
//! scheduled appointment is left off the list unless requested.

use chrono::Local;
use od_core::treatment_finder_report;
use od_db::{DemoStore, Store};
use od_domain::reports_ext::TreatmentFinderReportOptions;
use od_domain::Procedure;

fn options(pats_with_appts: bool) -> TreatmentFinderReportOptions {
    TreatmentFinderReportOptions {
        pats_with_appts,
        ..Default::default()
    }
}

#[test]
fn demo_store_feeds_treatment_finder_report() {
    let store = DemoStore::seeded();
    let today = Local::now().date_naive();
    let from = today - chrono::Duration::days(30);
    let to = today + chrono::Duration::days(30);

    // Patient 6 (Foster, Norman) has no future scheduled appointment; patient 1
    // does. Insert a TP procedure for each with a distinct planned amount.
    for (pat_num, fee) in [(6u32, 250.0_f64), (1u32, 999.0)] {
        store
            .insert_procedure(&Procedure {
                pat_num,
                code_num: 1,
                proc_date: today,
                fee,
                unit_qty: 1.0,
                ..Default::default()
            })
            .unwrap();
    }

    let patients = store.list_patients().unwrap();
    let mut procedures = store.list_procedures(6).unwrap();
    procedures.extend(store.list_procedures(1).unwrap());
    let codes = store.list_procedure_codes().unwrap();
    let appointments = store
        .list_appointments(
            today.and_hms_opt(0, 0, 0).unwrap(),
            (today + chrono::Duration::days(3650))
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        )
        .unwrap();

    let default = treatment_finder_report(
        &patients,
        &procedures,
        &codes,
        &appointments,
        &[],
        &[],
        &[],
        from,
        to,
        today,
        &options(false),
        false,
    );
    assert!(
        default.rows.iter().any(|row| {
            row.len() == 4 && row[0] == "Foster" && row[1] == "Norman" && row[3] == "250.00"
        }),
        "patient 6 should be listed with 250.00 planned; got {:?}",
        default.rows
    );
    assert!(
        !default.rows.iter().any(|row| row[3] == "999.00"),
        "patient 1 has a future appointment and should be excluded"
    );

    let with_appts = treatment_finder_report(
        &patients,
        &procedures,
        &codes,
        &appointments,
        &[],
        &[],
        &[],
        from,
        to,
        today,
        &options(true),
        false,
    );
    assert!(
        with_appts.rows.iter().any(|row| row[3] == "999.00"),
        "patient 1 should appear when future appointments are included"
    );
}
