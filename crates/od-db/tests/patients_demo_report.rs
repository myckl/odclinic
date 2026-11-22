//! End-to-end check that the seeded demo registry feeds `FormRpPatients`'s
//! field-selection builder with the same raw values the live query would give.

use chrono::Local;
use od_core::patients_report;
use od_db::{DemoStore, Store};
use od_domain::reports::{
    PatientsReportOptions, ReportFilter, ReportFilterCondition, PATIENT_REPORT_FIELDS,
};

fn fixture() -> (DemoStore, Vec<od_domain::Patient>, Vec<od_domain::Procedure>, Vec<od_domain::Recall>) {
    let store = DemoStore::seeded();
    let patients = store.list_patients().unwrap();
    let mut procedures = Vec::new();
    for patient in &patients {
        procedures.extend(store.list_procedures(patient.pat_num).unwrap());
    }
    let recalls = store.list_recalls().unwrap();
    (store, patients, procedures, recalls)
}

#[test]
fn demo_patients_report_renders_default_fields_and_derived_columns() {
    let (_store, patients, procedures, recalls) = fixture();
    let options = PatientsReportOptions {
        fields: PATIENT_REPORT_FIELDS
            .iter()
            .map(|field| field.to_string())
            .collect(),
        filters: Vec::new(),
    };
    let table = patients_report(&patients, &procedures, &recalls, &options);
    assert_eq!(table.headers.len(), PATIENT_REPORT_FIELDS.len());
    assert_eq!(table.rows.len(), 6, "all seeded patients appear once");
    assert_eq!(table.rows[0][0], "1", "PatNum is first");
    assert_eq!(table.rows[0][1], "Alvarez");
    assert_eq!(table.rows[0][2], "Marta");
    assert_eq!(table.rows[0][3], "", "no middle initial");
    assert_eq!(table.rows[0][5], "1979-03-12", "Birthdate is an ISO date");
    assert_eq!(table.rows[0][6], "1", "Gender is OD's numeric Female");
    assert_eq!(table.rows[0][15], "1", "ClinicNum");

    let first_complete = PATIENT_REPORT_FIELDS
        .iter()
        .position(|field| *field == "First Complete Proc")
        .unwrap();
    let last_complete = PATIENT_REPORT_FIELDS
        .iter()
        .position(|field| *field == "Last Complete Proc")
        .unwrap();
    let recall_status = PATIENT_REPORT_FIELDS
        .iter()
        .position(|field| *field == "RecallStatus")
        .unwrap();
    let today = Local::now().date_naive();
    assert_eq!(
        table.rows[0][first_complete],
        (today - chrono::Duration::days(8)).format("%Y-%m-%d").to_string(),
        "patient 1's only completed procedure"
    );
    assert_eq!(table.rows[0][last_complete], table.rows[0][first_complete]);
    assert_eq!(table.rows[0][recall_status], "0", "recall row has no status");
    assert_eq!(table.rows[2][1], "Chen");
    assert_eq!(
        table.rows[2][first_complete],
        (today - chrono::Duration::days(1)).format("%Y-%m-%d").to_string()
    );
    assert_eq!(table.rows[4][1], "Evans");
    assert_eq!(table.rows[4][first_complete], "", "no completed procedures");
    assert_eq!(table.rows[4][recall_status], "", "no recall row");
}

#[test]
fn demo_patients_report_ands_numeric_and_string_filters() {
    let (_store, patients, procedures, recalls) = fixture();
    let build = |filters| {
        patients_report(
            &patients,
            &procedures,
            &recalls,
            &PatientsReportOptions {
                fields: vec!["LName".into(), "PatNum".into()],
                filters,
            },
        )
    };
    let table = build(vec![
        ReportFilter {
            field: "PatNum".into(),
            condition: ReportFilterCondition::Gte,
            value: "5".into(),
        },
        ReportFilter {
            field: "LName".into(),
            condition: ReportFilterCondition::Like,
            value: "os".into(),
        },
    ]);
    assert_eq!(table.rows.len(), 1, "PatNum >= 5 AND LName LIKE '%os%'");
    assert_eq!(table.rows[0], ["Foster", "6"]);

    // RecallStatus is an OD definition number, so it compares numerically even
    // though the value is stored as text; patients without a recall are blank.
    let with_recall = build(vec![ReportFilter {
        field: "RecallStatus".into(),
        condition: ReportFilterCondition::Eq,
        value: "0".into(),
    }]);
    assert_eq!(with_recall.rows.len(), 4, "patients 1 through 4");
    assert!(with_recall.rows.iter().all(|row| row[0] != "Evans"));
}
