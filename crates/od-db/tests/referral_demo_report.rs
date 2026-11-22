//! End-to-end check that the seeded demo data feeds the Referral Analysis
//! builder with the same counts and production the live query would produce.

use chrono::{Duration, Local};
use od_core::{referral_analysis_report, referrals_report};
use od_db::{DemoStore, Store};
use od_domain::reports::{
    ReferralAnalysisReportOptions, ReferralFilter, ReferralFilterCondition, ReferralsReportOptions,
};

#[test]
fn demo_referral_analysis_report_ranks_sources_and_resolves_specialty() {
    let store = DemoStore::seeded();
    let today = Local::now().date_naive();
    let mut procedures = Vec::new();
    for patient in store.list_patients().unwrap() {
        procedures.extend(store.list_procedures(patient.pat_num).unwrap());
    }
    let table = referral_analysis_report(
        &procedures,
        &store.list_procedure_codes().unwrap(),
        &store.list_referrals().unwrap(),
        &store.list_refattaches().unwrap(),
        &store.list_definitions().unwrap(),
        today - Duration::days(30),
        today,
        &ReferralAnalysisReportOptions {
            include_address: true,
            ..Default::default()
        },
    );
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0][0], "Girma");
    assert_eq!(table.rows[0][2], "2");
    assert_eq!(table.rows[0][3], "305.00");
    assert_eq!(table.rows[0][10], "General Dentistry");
    assert_eq!(table.rows[1][0], "Tesfaye");
    assert_eq!(table.rows[1][2], "1");
    assert_eq!(table.rows[1][3], "185.00");
    assert_eq!(table.rows[1][10], "");
}

#[test]
fn demo_referrals_report_selects_fields_and_filters_like_upstream() {
    let store = DemoStore::seeded();
    let referrals = store.list_referrals().unwrap();
    let table = referrals_report(
        &referrals,
        &ReferralsReportOptions {
            fields: vec![
                "LName".into(),
                "MName".into(),
                "Telephone".into(),
                "Specialty".into(),
                "IsHidden".into(),
                "NotPerson".into(),
            ],
            filters: Vec::new(),
        },
    );
    assert_eq!(
        table.headers,
        ["LName", "MName", "Telephone", "Specialty", "IsHidden", "NotPerson"]
    );
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0][0], "Girma");
    assert_eq!(table.rows[0][1], "K");
    assert_eq!(table.rows[0][2], "+251 11 123 4567");
    assert_eq!(table.rows[0][3], "17");
    assert_eq!(table.rows[0][4], "0");
    assert_eq!(table.rows[0][5], "0");
    assert_eq!(table.rows[2][0], "Ministry of Health");
    assert_eq!(table.rows[2][4], "1");
    assert_eq!(table.rows[2][5], "1");

    // Stacked AND filters: hidden institutional sources only.
    let filtered = referrals_report(
        &referrals,
        &ReferralsReportOptions {
            fields: vec!["LName".into()],
            filters: vec![
                ReferralFilter {
                    field: "IsHidden".into(),
                    condition: ReferralFilterCondition::Eq,
                    value: "1".into(),
                },
                ReferralFilter {
                    field: "NotPerson".into(),
                    condition: ReferralFilterCondition::Eq,
                    value: "1".into(),
                },
            ],
        },
    );
    assert_eq!(filtered.rows.len(), 1);
    assert_eq!(filtered.rows[0][0], "Ministry of Health");

    // A case-insensitive LIKE, upstream's default condition.
    let liked = referrals_report(
        &referrals,
        &ReferralsReportOptions {
            fields: vec!["LName".into()],
            filters: vec![ReferralFilter {
                field: "LName".into(),
                condition: ReferralFilterCondition::Like,
                value: "TESFAYE".into(),
            }],
        },
    );
    assert_eq!(liked.rows.len(), 1);
    assert_eq!(liked.rows[0][0], "Tesfaye");
}
