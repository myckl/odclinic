//! Live compatibility check for the MySQL backend.
//!
//! Run with:
//!   ODCLINIC_DB_URL=mysql://user:password@host:3306/opendental \
//!     cargo test -p od-db --test mysql_live -- --ignored --nocapture --test-threads=1
//!
//! `--test-threads=1` is required: several tests write temporary payment splits
//! to the same demo database, so parallel runs let one test's temp split leak
//! into another test's report totals.

use chrono::{NaiveDate, NaiveDateTime};
use mysql::prelude::Queryable;
use od_core::{
    account_summary_without_insurance, active_patient_report, adj_sheet_report, aging_report,
    appointments_report, asap_report, balance_sheet_report, birthday_report,
    broken_appointments_report, build_statement_document, confirmation_report, custom_aging_report,
    dental_sealant_measure_report, discount_amount, discount_plan_report, dpp_overcharged_report,
    annual_bucket, charge_generation_block, generate_plan_charges, DiscountInput, FrequencyCodeSets,
    finance_charge_report, general_ledger_report, hidden_pay_splits_report, laser_labels_report,
    new_patient_report, patient_label_addresses, patient_portion_uncollected_report,
    ph_raw_population_report, ph_raw_screen_report,
    pay_plan_detail_report, pay_sheet_report,
    patients_report, payment_plan_report,
    presented_treatment_production_report, print_preview_report, proc_note_report,
    proc_overpaid_report, profit_loss_report, production_goal_report, provider_payroll_report,
    prescriptions_report, procedure_codes_report, prod_inc_report,
    procedure_sheet_report, receivables_breakdown_report, recall_fields_report, recall_report,
    referral_analysis_report, routing_slips_report, service_date_view_report,
    referrals_report, render_report_csv, render_report_html, render_statement_pdf,
    statement_report, treat_plan_presentation_statistics_report, treatment_finder_report,
    treatment_plan_report, unearned_income_report, PublicHealthRow,
};
use od_db::{MySqlStore, Store};
use od_domain::definitions::{Definition, DefinitionCategory, DisplayField, DisplayFieldCategory};
use od_domain::eservices::*;
use od_domain::reports_ext::{
    HiddenPaySplitsReportOptions, ProcNoteReportOptions, ProcOverpaidReportOptions,
    TreatmentFinderReportOptions, UnearnedIncomeReportOptions, UnearnedIncomeTab,
};
use od_domain::reports::{
    ActivePatientReportOptions, AdjustmentSheetReportOptions, AgingOptions, AppointmentDateBasis,
    AppointmentReportOptions, BrokenAppointmentMode, BrokenAppointmentReportOptions,
    BrokenProcedureOption, ConfirmationReportOptions, CustomAgingInclude,
    CustomAgingReportOptions, FinanceChargeReportOptions, NewPatientReportOptions,
    PatientsReportOptions, PayPlanReportOptions, PaySheetReportOptions,
    PrescriptionReportOptions, ProcedureCodesReportOptions, ProcedureSheetMode,
    ProcedureSheetReportOptions, ProdIncMode, ProdIncReportOptions, RecallFieldsReportOptions,
    ReferralAnalysisReportOptions, ReferralFilter, ReportFilter, ReportFilterCondition,
    ReferralsReportOptions, PATIENT_REPORT_FIELDS,
};
use od_domain::{
    Adjustment, Appointment, AppointmentChange, AppointmentChangeAction, AppointmentStatus,
    DiscountPlan, DiscountPlanSub, Gender, Patient, PatientCorrection, PayPlan, PayPlanCharge,
    PayPlanChargeType, Payment, PaymentAllocation, PaymentTarget, PaySplit, Procedure, ProcedureStatus,
    Recall, RecallPriority, Statement, TreatPlan, TreatPlanLine, TreatPlanStatus,
};

#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL"]
fn mysql_backend_reads_compatible_od_tables() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");

    let mut failures: Vec<String> = Vec::new();
    let mut check = |name: &str, result: Result<usize, String>| match result {
        Ok(count) => println!("OK   {name}: {count}"),
        Err(err) => {
            println!("FAIL {name}: {err}");
            failures.push(format!("{name}: {err}"));
        }
    };

    check("list_patients", store.list_patients().map(|v| v.len()));
    check("list_providers", store.list_providers().map(|v| v.len()));
    check("list_operatories", store.list_operatories().map(|v| v.len()));
    check("list_clinics", store.list_clinics().map(|v| v.len()));
    check("list_users", store.list_users().map(|v| v.len()));
    check("list_prefs", store.list_prefs().map(|v| v.len()));
    check(
        "list_group_permissions",
        store.list_group_permissions().map(|v| v.len()),
    );
    check(
        "list_user_group_attaches",
        store.list_user_group_attaches().map(|v| v.len()),
    );
    check(
        "list_security_log",
        store.list_security_log(10).map(|v| v.len()),
    );

    let from = NaiveDate::from_ymd_opt(2000, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let to = NaiveDate::from_ymd_opt(2100, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    check(
        "list_appointments",
        store.list_appointments(from, to).map(|v| v.len()),
    );
    check("list_recalls", store.list_recalls().map(|v| v.len()));
    check(
        "list_asap_appointments",
        store.list_asap_appointments().map(|v| v.len()),
    );

    let patients = store.list_patients().unwrap_or_default();
    if let Some(patient) = patients.first() {
        let pat_num = patient.pat_num;
        let l_name = patient.l_name.clone();
        check(
            "find_patients",
            store.find_patients(&l_name).map(|v| v.len()),
        );
        check(
            "get_patient",
            store
                .get_patient(pat_num)
                .map(|v| usize::from(v.is_some())),
        );
        check(
            "list_procedures",
            store.list_procedures(pat_num).map(|v| v.len()),
        );
        check(
            "list_payments",
            store.list_payments(pat_num).map(|v| v.len()),
        );
        check(
            "list_adjustments",
            store.list_adjustments(pat_num).map(|v| v.len()),
        );
        check(
            "list_pay_splits",
            store.list_pay_splits(pat_num).map(|v| v.len()),
        );
        check("list_claims", store.list_claims(pat_num).map(|v| v.len()));
        check(
            "list_claim_procs",
            store.list_claim_procs(pat_num).map(|v| v.len()),
        );
        check(
            "list_statements",
            store.list_statements(pat_num).map(|v| v.len()),
        );
        check(
            "list_treat_plans",
            store.list_treat_plans(pat_num).map(|v| v.len()),
        );
    } else {
        println!("No patients found; skipped per-patient reads.");
    }
    check("list_proc_notes", store.list_proc_notes().map(|v| v.len()));
    check("list_accounts", store.list_accounts().map(|v| v.len()));
    check(
        "list_journal_entries",
        store.list_journal_entries().map(|v| v.len()),
    );

    // Dump the actual columns of every table the port reads. This makes any
    // remaining schema drift immediately visible without another test run.
    if !failures.is_empty() {
        let normalized = if url.contains("://") {
            url.trim().to_string()
        } else {
            format!("mysql://{}", url.trim())
        };
        let diagnostic_conn = mysql::Opts::from_url(&normalized)
            .ok()
            .and_then(|opts| mysql::Pool::new(opts).ok())
            .and_then(|pool| pool.get_conn().ok());
        if let Some(mut conn) = diagnostic_conn {
            let tables = [
                "patient",
                "appointment",
                "provider",
                "operatory",
                "clinic",
                "procedurelog",
                "payment",
                "paysplit",
                "adjustment",
                "claim",
                "claimproc",
                "statement",
                "treatplan",
                "userod",
                "usergroupattach",
                "grouppermission",
                "securitylog",
                "preference",
            ];
            let sql = "SELECT TABLE_NAME,COLUMN_NAME FROM information_schema.COLUMNS \
                       WHERE TABLE_SCHEMA=DATABASE() ORDER BY TABLE_NAME,ORDINAL_POSITION";
            if let Ok(rows) = conn.query::<(String, String), _>(sql) {
                for table in tables {
                    let cols: Vec<String> = rows
                        .iter()
                        .filter(|(t, _)| t == table)
                        .map(|(_, c)| c.clone())
                        .collect();
                    println!("COLUMNS {table}: {}", cols.join(","));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "MySQL compatibility failures:\n{}",
        failures.join("\n")
    );
}

/// Creates temporary rows with unmistakable names/numbers, reads them back,
/// then deletes them before asserting. The database is demo-only by design.
#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL; writes and deletes a temporary row"]
fn mysql_backend_writes_compatible_od_tables() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");

    let pat_num = 2_100_000_001u32;
    let apt_num = 2_100_000_002u32;

    let mut patient = Patient::default();
    patient.pat_num = pat_num;
    patient.l_name = "ODCLINIC_LIVE_TEST".into();
    patient.f_name = "TEMP".into();
    patient.gender = Gender::Unknown;

    let patient_write = store.upsert_patient(&patient);
    let patient_read = match &patient_write {
        Ok(_) => store.get_patient(pat_num),
        Err(_) => Ok(None),
    };
    let patient_cleanup = store.delete_patient(pat_num);

    println!("upsert_patient: {:?}", patient_write);
    println!("readback patient: {:?}", patient_read.as_ref().map(|p| p.is_some()));
    println!("delete_patient: {:?}", patient_cleanup);
    patient_write.expect("upsert_patient failed");
    assert!(
        patient_read.expect("get_patient failed").is_some(),
        "temporary patient was not readable"
    );
    patient_cleanup.expect("delete_patient failed");

    let real_patient = store
        .list_patients()
        .expect("list_patients failed")
        .into_iter()
        .next()
        .expect("need at least one patient for the appointment write test");

    let mut appointment = Appointment::default();
    appointment.apt_num = apt_num;
    appointment.pat_num = real_patient.pat_num;
    appointment.apt_datetime = chrono::NaiveDate::from_ymd_opt(2099, 1, 1)
        .unwrap()
        .and_hms_opt(9, 0, 0)
        .unwrap();
    appointment.apt_status = AppointmentStatus::Scheduled;
    appointment.pattern = "XX".into(); // 10 minutes
    appointment.apt_length = 10;
    appointment.notes = "ODCLINIC_LIVE_TEST".into();

    let appointment_write = store.upsert_appointment(&appointment);
    let appointment_read = match &appointment_write {
        Ok(_) => store.get_appointment(apt_num),
        Err(_) => Ok(None),
    };
    let appointment_cleanup = store.delete_appointment(apt_num);

    println!("upsert_appointment: {:?}", appointment_write);
    println!(
        "readback appointment: {:?}",
        appointment_read.as_ref().map(|a| a.is_some())
    );
    println!("delete_appointment: {:?}", appointment_cleanup);
    appointment_write.expect("upsert_appointment failed");
    assert!(
        appointment_read.expect("get_appointment failed").is_some(),
        "temporary appointment was not readable"
    );
    appointment_cleanup.expect("delete_appointment failed");
}

/// Writes one temporary securitylog row, reads it back, then deletes it.
#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL; writes and deletes one audit row"]
fn mysql_backend_supports_security_tables() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");

    let users = store.list_users().expect("list_users failed");
    let with_passwords = users
        .iter()
        .filter(|u| !u.password.is_empty())
        .count();
    println!("users: {}, with encoded passwords: {with_passwords}", users.len());
    println!(
        "group permissions: {}, group attaches: {}",
        store.list_group_permissions().map(|v| v.len()).unwrap_or(0),
        store.list_user_group_attaches().map(|v| v.len()).unwrap_or(0),
    );

    let event = od_domain::SecurityEvent {
        security_log_num: 0,
        perm_type: od_domain::Permission::USER_LOG_ON_OFF,
        user_num: 0,
        log_datetime: chrono::Local::now().naive_local(),
        log_text: "ODCLINIC_LIVE_TEST_AUDIT".into(),
        pat_num: 0,
    };
    let write = store.append_security_log(&event);
    println!("append_security_log: {write:?}");

    let normalized = if url.contains("://") {
        url.trim().to_string()
    } else {
        format!("mysql://{}", url.trim())
    };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("bad pool");
    let mut conn = pool.get_conn().expect("bad connection");
    let read_back: usize = conn
        .query_first::<u64, _>("SELECT COUNT(*) FROM securitylog WHERE LogText='ODCLINIC_LIVE_TEST_AUDIT'")
        .expect("could not read securitylog")
        .unwrap_or(0) as usize;
    let deleted = conn
        .query_drop("DELETE FROM securitylog WHERE LogText='ODCLINIC_LIVE_TEST_AUDIT'")
        .is_ok();
    println!("readback audit rows: {read_back}, cleaned up: {deleted}");

    write.expect("append_security_log failed");
    assert_eq!(read_back, 1, "temporary audit row was not written");
    assert!(deleted, "temporary audit row was not deleted");
}

/// Writes a temporary adjustment and a payment+paysplit, reads them back
/// through the Store API, then deletes them. The database is demo-only, so
/// direct writes to OD tables are intentional.
#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL; writes and deletes temporary billing rows"]
fn mysql_backend_supports_billing_tables() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");

    let pat_num = store
        .list_patients()
        .expect("list_patients failed")
        .into_iter()
        .next()
        .expect("need at least one patient for the billing write test")
        .pat_num;

    let normalized = if url.contains("://") {
        url.trim().to_string()
    } else {
        format!("mysql://{}", url.trim())
    };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("bad pool");
    let mut conn = pool.get_conn().expect("bad connection");

    // --- Adjustment ---------------------------------------------------------
    let adjustment = Adjustment {
        adj_date: NaiveDate::from_ymd_opt(2099, 2, 1).unwrap(),
        adj_amount: -7.25,
        pat_num,
        note: "ODCLINIC_LIVE_TEST_ADJ".into(),
        ..Default::default()
    };
    let adj_write = store.upsert_adjustment(&adjustment);
    println!("upsert_adjustment: {adj_write:?}");
    let adj_num = adj_write.clone().unwrap_or(0);
    let adj_read = if adj_num > 0 {
        let rows = store.list_adjustments(pat_num).unwrap_or_default();
        rows.iter().any(|a| a.adj_num == adj_num && a.adj_amount == -7.25)
    } else {
        false
    };
    let adj_cleanup = conn.query_drop(format!(
        "DELETE FROM adjustment WHERE AdjNum = {adj_num}"
    ));
    println!("adjustment readback: {adj_read}, cleaned up: {:?}", adj_cleanup.is_ok());

    // --- Payment + paysplit -------------------------------------------------
    let payment = Payment {
        pat_num,
        pay_date: NaiveDate::from_ymd_opt(2099, 2, 2).unwrap(),
        pay_amount: 11.50,
        note: "ODCLINIC_LIVE_TEST_PAY".into(),
        ..Default::default()
    };
    let pay_write = store.record_payment(&payment, 0, 0);
    println!("record_payment: {pay_write:?}");
    let pay_num = pay_write.clone().unwrap_or(0);
    let split_read = if pay_num > 0 {
        store
            .list_pay_splits(pat_num)
            .unwrap_or_default()
            .iter()
            .any(|sp| sp.pay_num == pay_num && sp.split_amount == 11.50)
    } else {
        false
    };
    // Delete the child paysplit before its parent payment.
    let split_cleanup = conn.query_drop(format!(
        "DELETE FROM paysplit WHERE PayNum = {pay_num}"
    ));
    let pay_cleanup = conn.query_drop(format!("DELETE FROM payment WHERE PayNum = {pay_num}"));
    println!(
        "paysplit readback: {split_read}, cleaned up split: {:?}, payment: {:?}",
        split_cleanup.is_ok(),
        pay_cleanup.is_ok()
    );

    adj_write.expect("upsert_adjustment failed");
    assert!(adj_read, "temporary adjustment was not readable");
    assert!(adj_cleanup.is_ok(), "adjustment cleanup failed");
    pay_write.expect("record_payment failed");
    assert!(split_read, "temporary paysplit was not readable");
    assert!(split_cleanup.is_ok(), "paysplit cleanup failed");
    assert!(pay_cleanup.is_ok(), "payment cleanup failed");
}

/// Writes one payment header with several paysplit rows, reads them back, and
/// deletes them. A payment can fund several procedures, so this exercises the
/// OD `paysplit` model the account ledger already uses.
#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL; writes and deletes temporary multi-split payment rows"]
fn mysql_backend_writes_multi_split_payments() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");

    let pat_num = store
        .list_patients()
        .expect("list_patients failed")
        .into_iter()
        .next()
        .expect("need at least one patient for the multi-split write test")
        .pat_num;
    let proc_num = store
        .list_procedures(pat_num)
        .unwrap_or_default()
        .first()
        .map(|p| p.proc_num)
        .unwrap_or(0);

    let payment = Payment {
        pat_num,
        pay_date: NaiveDate::from_ymd_opt(2099, 4, 1).unwrap(),
        pay_amount: 100.0,
        note: "ODCLINIC_LIVE_TEST_MULTI_SPLIT".into(),
        ..Default::default()
    };
    let allocations = [
        PaymentAllocation { unearned_type: 0, 
            target: PaymentTarget::Procedure(proc_num),
            amount: 40.0,
            ..Default::default()
        },
        PaymentAllocation { unearned_type: 0, 
            target: PaymentTarget::Procedure(proc_num),
            amount: 35.0,
            ..Default::default()
        },
        PaymentAllocation { unearned_type: 0, 
            target: PaymentTarget::Unallocated,
            amount: 25.0,
            ..Default::default()
        },
    ];

    let payments_before = store.list_payments(pat_num).unwrap_or_default().len();
    let bad = store.record_payment_splits(
        &payment,
        &[PaymentAllocation { unearned_type: 0, 
            amount: 10.0,
            ..Default::default()
        }],
    );
    println!("rejected mismatched allocations: {:?}", bad.is_err());
    assert!(bad.is_err(), "mismatched allocation total was accepted");
    assert_eq!(
        store.list_payments(pat_num).unwrap_or_default().len(),
        payments_before,
        "a rejected payment wrote a header row"
    );

    let pay_write = store.record_payment_splits(&payment, &allocations);
    println!("record_payment_splits: {pay_write:?}");
    let pay_num = pay_write.clone().unwrap_or(0);
    let splits: Vec<_> = store
        .list_pay_splits(pat_num)
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.pay_num == pay_num)
        .collect();
    let amounts: Vec<f64> = splits.iter().map(|s| s.split_amount).collect();
    println!("split amounts written: {amounts:?}");
    assert_eq!(splits.len(), 3, "expected three paysplit rows");
    assert!(amounts.contains(&40.0));
    assert!(amounts.contains(&35.0));
    assert!(amounts.contains(&25.0));
    assert!(splits.iter().all(|s| s.proc_num == proc_num
        || (s.proc_num == 0 && s.pay_plan_num == 0)));

    let normalized = if url.contains("://") {
        url.trim().to_string()
    } else {
        format!("mysql://{}", url.trim())
    };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("bad pool");
    let mut conn = pool.get_conn().expect("bad connection");
    let split_cleanup = conn.query_drop(format!("DELETE FROM paysplit WHERE PayNum = {pay_num}"));
    let pay_cleanup = conn.query_drop(format!("DELETE FROM payment WHERE PayNum = {pay_num}"));
    println!(
        "cleaned up split: {:?}, payment: {:?}",
        split_cleanup.is_ok(),
        pay_cleanup.is_ok()
    );

    pay_write.expect("record_payment_splits failed");
    assert!(split_cleanup.is_ok(), "paysplit cleanup failed");
    assert!(pay_cleanup.is_ok(), "payment cleanup failed");
}

/// Writes a temporary payment plan, one charge and a plan-linked payment,
/// reads them all back through the Store API, then deletes them. The schema is
/// created idempotently so this also verifies our `payplan` / `payplancharge`
/// DDL against the live server.
#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL; writes and deletes temporary payment plan rows"]
fn mysql_backend_supports_payment_plan_tables() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");

    let pat_num = store
        .list_patients()
        .expect("list_patients failed")
        .into_iter()
        .next()
        .expect("need at least one patient for the payment plan write test")
        .pat_num;

    let normalized = if url.contains("://") {
        url.trim().to_string()
    } else {
        format!("mysql://{}", url.trim())
    };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("bad pool");
    let mut conn = pool.get_conn().expect("bad connection");

    // Idempotent DDL — matches crates/od-db/schema.sql.
    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS payplan (\
           PayPlanNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,\
           PatNum bigint NOT NULL DEFAULT 0,\
           Guarantor bigint NOT NULL DEFAULT 0,\
           PayPlanDate date NOT NULL,\
           APR double NOT NULL DEFAULT 0,\
           Note text NOT NULL,\
           CompletedAmt double NOT NULL DEFAULT 0,\
           PaySchedule tinyint NOT NULL DEFAULT 0,\
           NumberOfPayments int NOT NULL DEFAULT 0,\
           PayAmt double NOT NULL DEFAULT 0,\
           DownPayment double NOT NULL DEFAULT 0,\
           IsClosed tinyint NOT NULL DEFAULT 0,\
           ChargeFrequency tinyint NOT NULL DEFAULT 3,\
           DatePayPlanStart date NOT NULL DEFAULT '0001-01-01',\
           IsDynamic tinyint NOT NULL DEFAULT 0,\
           IsLocked tinyint NOT NULL DEFAULT 0,\
           KEY (PatNum), KEY (Guarantor))",
    )
    .expect("could not ensure payplan table");
    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS payplancharge (\
           PayPlanChargeNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,\
           PayPlanNum bigint NOT NULL DEFAULT 0,\
           Guarantor bigint NOT NULL DEFAULT 0,\
           PatNum bigint NOT NULL DEFAULT 0,\
           ChargeDate date NOT NULL,\
           Principal double NOT NULL DEFAULT 0,\
           Interest double NOT NULL DEFAULT 0,\
           Note text NOT NULL,\
           ProvNum bigint NOT NULL DEFAULT 0,\
           ProcNum bigint NOT NULL DEFAULT 0,\
           ChargeType tinyint NOT NULL DEFAULT 0,\
           KEY (PayPlanNum), KEY (ChargeDate))",
    )
    .expect("could not ensure payplancharge table");

    // --- Plan ----------------------------------------------------------------
    let plan = PayPlan {
        pat_num,
        guarantor: pat_num,
        pay_plan_date: NaiveDate::from_ymd_opt(2099, 3, 1).unwrap(),
        date_pay_plan_start: Some(NaiveDate::from_ymd_opt(2099, 3, 15).unwrap()),
        completed_amt: 300.0,
        number_of_payments: 2,
        pay_amt: 150.0,
        note: "ODCLINIC_LIVE_TEST_PLAN".into(),
        ..Default::default()
    };
    let plan_write = store.upsert_pay_plan(&plan);
    println!("upsert_pay_plan: {plan_write:?}");
    let plan_num = plan_write.clone().unwrap_or(0);
    let plan_read = if plan_num > 0 {
        store
            .list_pay_plans(pat_num)
            .unwrap_or_default()
            .iter()
            .any(|p| p.pay_plan_num == plan_num && p.completed_amt == 300.0)
    } else {
        false
    };

    // --- Charge --------------------------------------------------------------
    let charge = PayPlanCharge {
        pay_plan_num: plan_num,
        pat_num,
        guarantor: pat_num,
        charge_date: NaiveDate::from_ymd_opt(2099, 3, 15).unwrap(),
        principal: 150.0,
        note: "ODCLINIC_LIVE_TEST_CHARGE".into(),
        charge_type: PayPlanChargeType::Debit,
        ..Default::default()
    };
    let charge_write = store.upsert_pay_plan_charge(&charge);
    println!("upsert_pay_plan_charge: {charge_write:?}");
    let charge_num = charge_write.clone().unwrap_or(0);
    let charge_read = if charge_num > 0 {
        store
            .list_pay_plan_charges(pat_num)
            .unwrap_or_default()
            .iter()
            .any(|c| c.pay_plan_charge_num == charge_num && c.principal == 150.0)
    } else {
        false
    };

    // --- Payment applied to the plan ----------------------------------------
    let payment = Payment {
        pat_num,
        pay_date: NaiveDate::from_ymd_opt(2099, 3, 16).unwrap(),
        pay_amount: 25.0,
        note: "ODCLINIC_LIVE_TEST_PLAN_PAY".into(),
        ..Default::default()
    };
    let pay_write = store.record_payment(&payment, 0, plan_num);
    println!("record_payment(plan): {pay_write:?}");
    let pay_num = pay_write.clone().unwrap_or(0);
    let split_read = if pay_num > 0 {
        store
            .list_pay_splits(pat_num)
            .unwrap_or_default()
            .iter()
            .any(|sp| sp.pay_num == pay_num && sp.pay_plan_num == plan_num)
    } else {
        false
    };

    // Child rows first.
    let split_cleanup = conn.query_drop(format!("DELETE FROM paysplit WHERE PayNum = {pay_num}"));
    let pay_cleanup = conn.query_drop(format!("DELETE FROM payment WHERE PayNum = {pay_num}"));
    let charge_cleanup = conn.query_drop(format!(
        "DELETE FROM payplancharge WHERE PayPlanChargeNum = {charge_num}"
    ));
    let plan_cleanup = conn.query_drop(format!("DELETE FROM payplan WHERE PayPlanNum = {plan_num}"));
    println!(
        "plan readback: {plan_read}, charge readback: {charge_read}, plan-linked split: {split_read}; cleaned up: split {:?}, payment {:?}, charge {:?}, plan {:?}",
        split_cleanup.is_ok(),
        pay_cleanup.is_ok(),
        charge_cleanup.is_ok(),
        plan_cleanup.is_ok()
    );

    plan_write.expect("upsert_pay_plan failed");
    assert!(plan_read, "temporary payment plan was not readable");
    charge_write.expect("upsert_pay_plan_charge failed");
    assert!(charge_read, "temporary plan charge was not readable");
    pay_write.expect("record_payment failed");
    assert!(split_read, "paysplit did not reference the payment plan");
    assert!(split_cleanup.is_ok(), "paysplit cleanup failed");
    assert!(pay_cleanup.is_ok(), "payment cleanup failed");
    assert!(charge_cleanup.is_ok(), "plan charge cleanup failed");
    assert!(plan_cleanup.is_ok(), "payment plan cleanup failed");
}

/// Amortised charge generation must round-trip through MySQL: the new
/// `DateInterestStart` term is stored, the OD amortisation vectors come back
/// from the pure builder, the generated debits are readable, and a second
/// generation is refused. All temporary rows are removed at the end.
#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL; writes and deletes temporary amortised plan rows"]
fn mysql_live_amortized_plan_charges_round_trip() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");
    let pat_num = store
        .list_patients()
        .expect("list_patients failed")
        .into_iter()
        .next()
        .expect("need at least one patient for the amortised plan test")
        .pat_num;

    let normalized = if url.contains("://") {
        url.trim().to_string()
    } else {
        format!("mysql://{}", url.trim())
    };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("bad pool");
    let mut conn = pool.get_conn().expect("bad connection");

    let start = NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
    let interest_start = NaiveDate::from_ymd_opt(2099, 2, 1).unwrap();
    let plan = PayPlan {
        pat_num,
        guarantor: pat_num,
        pay_plan_date: NaiveDate::from_ymd_opt(2098, 12, 1).unwrap(),
        date_pay_plan_start: Some(start),
        date_interest_start: Some(interest_start),
        completed_amt: 1000.0,
        pay_amt: 250.0,
        number_of_payments: 5,
        apr: 12.0,
        note: "ODCLINIC_LIVE_AMORTIZED_PLAN".into(),
        ..Default::default()
    };
    let plan_num = store.upsert_pay_plan(&plan).expect("upsert_pay_plan failed");

    let stored = store
        .list_pay_plans(pat_num)
        .expect("list_pay_plans failed")
        .into_iter()
        .find(|p| p.pay_plan_num == plan_num)
        .expect("temporary plan was not readable");
    assert_eq!(stored.date_interest_start, Some(interest_start));
    assert_eq!(stored.apr, 12.0);

    let generated = generate_plan_charges(&stored, NaiveDate::from_ymd_opt(2098, 12, 15).unwrap());
    assert_eq!(generated.len(), 5);
    let principal: f64 = generated.iter().map(|c| c.principal).sum();
    assert_eq!(principal, 1000.0);
    assert!(charge_generation_block(&stored, &[]).is_none());

    let mut written = Vec::new();
    for charge in &generated {
        let num = store
            .upsert_pay_plan_charge(charge)
            .expect("upsert_pay_plan_charge failed");
        written.push(num);
    }
    let saved = store
        .list_pay_plan_charges(pat_num)
        .expect("list_pay_plan_charges failed");
    let saved_for_plan: Vec<&PayPlanCharge> = saved
        .iter()
        .filter(|c| c.pay_plan_num == plan_num)
        .collect();
    assert_eq!(saved_for_plan.len(), 5);
    assert!(
        charge_generation_block(&stored, &saved).is_some(),
        "a second generation must be refused"
    );

    let nums = written
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let charge_cleanup = conn.query_drop(format!(
        "DELETE FROM payplancharge WHERE PayPlanChargeNum IN ({nums})"
    ));
    let plan_cleanup = conn.query_drop(format!("DELETE FROM payplan WHERE PayPlanNum = {plan_num}"));
    println!(
        "amortised round trip: 5 charges written, second generation refused; cleaned up charge {:?}, plan {:?}",
        charge_cleanup.is_ok(),
        plan_cleanup.is_ok()
    );
    assert!(charge_cleanup.is_ok(), "charge cleanup failed");
    assert!(plan_cleanup.is_ok(), "plan cleanup failed");
}

/// Writes a temporary statement row with a real sent date, reads it back, and
/// deletes it. This verifies the `statement` table columns and the OD-style
/// 0001-01-01 sentinel used for unsent statements.
#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL; writes and deletes a temporary statement row"]
fn mysql_backend_writes_statement_rows() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");

    let pat_num = store
        .list_patients()
        .expect("list_patients failed")
        .into_iter()
        .next()
        .expect("need at least one patient for the statement write test")
        .pat_num;
    let sent = NaiveDate::from_ymd_opt(2099, 5, 6).unwrap();

    let write = store.insert_statement(&Statement {
        pat_num,
        date_sent: Some(sent),
        note: "ODCLINIC_LIVE_TEST_STATEMENT".into(),
        is_invoice: false,
        ..Default::default()
    });
    println!("insert_statement: {write:?}");
    let statement_num = write.clone().unwrap_or(0);
    let read = if statement_num > 0 {
        store
            .list_statements(pat_num)
            .unwrap_or_default()
            .iter()
            .any(|s| {
                s.statement_num == statement_num
                    && s.date_sent == Some(sent)
                    && s.note == "ODCLINIC_LIVE_TEST_STATEMENT"
            })
    } else {
        false
    };

    let normalized = if url.contains("://") {
        url.trim().to_string()
    } else {
        format!("mysql://{}", url.trim())
    };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("bad pool");
    let mut conn = pool.get_conn().expect("bad connection");
    let cleanup = conn.query_drop(format!(
        "DELETE FROM statement WHERE StatementNum = {statement_num}"
    ));
    println!("statement readback: {read}, cleaned up: {:?}", cleanup.is_ok());

    write.expect("insert_statement failed");
    assert!(read, "temporary statement was not readable");
    assert!(cleanup.is_ok(), "statement cleanup failed");
}

/// Generates a statement PDF from a real patient's account data and checks the
/// bytes. Read-only: no rows are written.
#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL; read-only PDF generation check"]
fn mysql_backend_generates_statement_pdf_from_live_data() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");
    let patient = store
        .list_patients()
        .expect("list_patients failed")
        .into_iter()
        .next()
        .expect("need at least one patient for the PDF check");
    let pat_num = patient.pat_num;

    let procedures = store.list_procedures(pat_num).unwrap_or_default();
    let adjustments = store.list_adjustments(pat_num).unwrap_or_default();
    let splits = store.list_pay_splits(pat_num).unwrap_or_default();
    let plans = store.list_pay_plans(pat_num).unwrap_or_default();
    let charges = store.list_pay_plan_charges(pat_num).unwrap_or_default();
    let summary = account_summary_without_insurance(&procedures, &adjustments, &splits);
    let doc = build_statement_document(
        pat_num,
        &patient.display_name(),
        &procedures,
        &adjustments,
        &splits,
        &plans,
        &charges,
        &summary,
        chrono::Local::now().date_naive(),
        "Live PDF generation check",
    );
    let bytes = render_statement_pdf(&doc);
    println!(
        "patient {} PDF: {} bytes, {} line(s), balance {:.2}",
        pat_num,
        bytes.len(),
        doc.lines.len(),
        doc.balance
    );

    assert!(bytes.starts_with(b"%PDF-1.4"));
    assert!(bytes.ends_with(b"%%EOF\n"));
    assert!(bytes.len() > 1000, "PDF looks too small to contain a statement");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Count "));
    assert!(text.contains("Balance due:"));

    // startxref must point at the xref table.
    let marker = "startxref\n";
    let pos = text.rfind(marker).unwrap() + marker.len();
    let offset: usize = text[pos..].lines().next().unwrap().trim().parse().unwrap();
    assert_eq!(&bytes[offset..offset + 4], b"xref");
}

/// Writes a recall and an ASAP appointment into OD tables, reads them back
/// through the Store API, then deletes the temporary rows.
#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL; writes and deletes temporary recall rows"]
fn mysql_backend_supports_recall_and_asap() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");
    let pat_num = store
        .list_patients()
        .expect("list_patients failed")
        .into_iter()
        .next()
        .expect("need at least one patient for the recall write test")
        .pat_num;

    let due = NaiveDate::from_ymd_opt(2099, 3, 1).unwrap();
    let recall = Recall {
        pat_num,
        date_due: Some(due),
        date_due_calc: Some(due),
        recall_interval: (6 << 16) | 12,
        recall_status: 7,
        note: "ODCLINIC_LIVE_TEST_RECALL".into(),
        priority: RecallPriority::Asap,
        ..Default::default()
    };
    let recall_num = store.upsert_recall(&recall).expect("upsert_recall failed");
    println!("upsert_recall: {recall_num}");
    assert!(recall_num > 0, "MySQL did not return an auto-increment recall number");

    let recall_read = store
        .list_recalls()
        .unwrap_or_default()
        .into_iter()
        .find(|r| r.recall_num == recall_num)
        .expect("temporary recall was not readable");
    println!(
        "recall readback: due {:?}, interval {}, status {}, ASAP {}",
        recall_read.date_due,
        recall_read.recall_interval,
        recall_read.recall_status,
        recall_read.priority == RecallPriority::Asap
    );
    assert_eq!(recall_read.date_due, Some(due));
    assert_eq!(recall_read.recall_interval, (6 << 16) | 12);
    assert_eq!(recall_read.recall_status, 7);
    assert_eq!(recall_read.priority, RecallPriority::Asap);

    let apt = Appointment {
        pat_num,
        apt_datetime: NaiveDate::from_ymd_opt(2099, 3, 2)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap(),
        apt_length: 30,
        apt_status: AppointmentStatus::UnschedList,
        operatory_num: 0,
        provider_num: 0,
        clinic_num: 0,
        notes: "ODCLINIC_LIVE_TEST_ASAP".into(),
        pattern: "XXXXXX".into(),
        priority: RecallPriority::Asap,
        ..Default::default()
    };
    let apt_num = store.upsert_appointment(&apt).expect("upsert_appointment failed");
    let asap_read = store
        .list_asap_appointments()
        .unwrap_or_default()
        .into_iter()
        .any(|a| a.apt_num == apt_num && a.notes == "ODCLINIC_LIVE_TEST_ASAP");
    println!("ASAP appointment readback: {asap_read}");
    assert!(asap_read, "temporary ASAP appointment was not in the ASAP list");

    let normalized = if url.contains("://") {
        url.trim().to_string()
    } else {
        format!("mysql://{}", url.trim())
    };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("bad pool");
    let mut conn = pool.get_conn().expect("bad connection");
    let cleanup_recall = conn.query_drop(format!(
        "DELETE FROM recall WHERE RecallNum = {recall_num}"
    ));
    let cleanup_apt = conn.query_drop(format!(
        "DELETE FROM appointment WHERE AptNum = {apt_num}"
    ));
    println!(
        "cleanup: recall {:?}, appointment {:?}",
        cleanup_recall.is_ok(),
        cleanup_apt.is_ok()
    );
    assert!(cleanup_recall.is_ok() && cleanup_apt.is_ok(), "temporary row cleanup failed");
}

#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL"]
fn mysql_eservices_rules_outbox_and_dedupe_round_trip() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");

    println!("rules: {}", store.list_reminder_rules().map(|v| v.len()).unwrap_or_default());
    println!("contacts: {}", store.list_patient_contacts().map(|v| v.len()).unwrap_or_default());
    println!("outbox: {}", store.list_outbox(10).map(|v| v.len()).unwrap_or_default());
    store.list_reminder_sent().expect("list_reminder_sent failed");

    let pat_num = store
        .list_patients()
        .expect("list_patients failed")
        .into_iter()
        .next()
        .expect("need one patient")
        .pat_num;

    let rule = ReminderRule {
        rule_num: 0,
        type_cur: ReminderType::Reminder,
        minutes_prior: 1440,
        do_not_send_within_minutes: 60,
        comm_type: CommType::Email,
        email_subject: "ODCLINIC_LIVE_TEST_REMINDER".into(),
        email_body: "ODCLINIC_LIVE_TEST_REMINDER_BODY".into(),
        ..Default::default()
    };
    let rule_num = store.upsert_reminder_rule(&rule).expect("upsert reminder rule failed");
    println!("rule number: {rule_num}");
    assert!(rule_num > 0);
    let read_rule = store
        .list_reminder_rules()
        .unwrap()
        .into_iter()
        .find(|r| r.rule_num == rule_num)
        .expect("temporary reminder rule was not readable");
    assert_eq!(read_rule.minutes_prior, 1440);
    assert_eq!(read_rule.do_not_send_within_minutes, 60);
    assert_eq!(read_rule.comm_type, CommType::Email);
    assert_eq!(read_rule.email_subject, "ODCLINIC_LIVE_TEST_REMINDER");

    let contact = PatientContact {
        pat_num,
        telegram_chat_id: "999000111".into(),
        email_override: "odclinic-live@example.test".into(),
        opt_out_email: false,
        opt_out_telegram: true,
    };
    store.upsert_patient_contact(&contact).expect("upsert contact failed");
    let read_contact = store
        .list_patient_contacts()
        .unwrap()
        .into_iter()
        .find(|c| c.pat_num == pat_num)
        .expect("temporary contact was not readable");
    assert_eq!(read_contact.telegram_chat_id, "999000111");
    assert!(read_contact.opt_out_telegram);

    store.mark_reminder_sent(rule_num, 999_999, pat_num).unwrap();
    assert!(store
        .list_reminder_sent()
        .unwrap()
        .contains(&(rule_num, 999_999)));

    let created = NaiveDate::from_ymd_opt(2026, 11, 21)
        .unwrap()
        .and_hms_opt(9, 0, 0)
        .unwrap();
    let sent = NaiveDateTime::parse_from_str("2026-11-21 09:01:00", "%Y-%m-%d %H:%M:%S").unwrap();
    let message = OutboxMessage {
        pat_num,
        apt_num: 999_999,
        rule_num,
        reminder_type: ReminderType::Reminder,
        transport: TransportKind::Smtp,
        recipient: "odclinic-live@example.test".into(),
        subject: "ODCLINIC_LIVE_TEST_OUTBOX".into(),
        body: "body".into(),
        status: OutboxStatus::Pending,
        attempts: 0,
        created_at: created,
        ..Default::default()
    };
    let message_num = store.insert_outbox(&message).expect("insert outbox failed");
    println!("outbox message: {message_num}");
    assert!(message_num > 0);
    store
        .update_outbox(message_num, OutboxStatus::Sent, 1, Some(sent), "smtp accepted")
        .expect("update outbox failed");
    let read_message = store
        .list_outbox(10)
        .unwrap()
        .into_iter()
        .find(|m| m.message_num == message_num)
        .expect("temporary outbox row was not readable");
    assert_eq!(read_message.status, OutboxStatus::Sent);
    assert_eq!(read_message.attempts, 1);
    assert_eq!(read_message.last_error, "smtp accepted");

    let normalized = if url.contains("://") {
        url.trim().to_string()
    } else {
        format!("mysql://{}", url.trim())
    };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("bad pool");
    let mut conn = pool.get_conn().expect("bad connection");
    let cleanups = [
        format!("DELETE FROM odclinic_message_outbox WHERE MessageNum = {message_num}"),
        format!("DELETE FROM apptremindersent WHERE ApptReminderRuleNum = {rule_num} AND ApptNum = 999999"),
        format!("DELETE FROM apptreminderrule WHERE ApptReminderRuleNum = {rule_num}"),
        format!("DELETE FROM odclinic_patient_contact WHERE PatNum = {pat_num}"),
    ];
    for sql in &cleanups {
        conn.query_drop(sql).expect("temporary eServices row cleanup failed");
    }
    println!("cleanup: removed 4 temporary eServices rows");
}

#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL; writes and deletes temporary treatment plan rows"]
fn mysql_treatment_plans_round_trip() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");

    // A previous failed run may have left a temporary plan behind.
    let normalized = if url.contains("://") {
        url.trim().to_string()
    } else {
        format!("mysql://{}", url.trim())
    };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("bad pool");
    let mut conn = pool.get_conn().expect("bad connection");
    for sql in [
        "DELETE FROM proctp WHERE TreatPlanNum IN (SELECT TreatPlanNum FROM (SELECT TreatPlanNum FROM treatplan WHERE Note = 'ODCLINIC live test') t)",
        "DELETE FROM treatplanattach WHERE TreatPlanNum IN (SELECT TreatPlanNum FROM (SELECT TreatPlanNum FROM treatplan WHERE Note = 'ODCLINIC live test') t)",
        "DELETE FROM treatplan WHERE Note = 'ODCLINIC live test'",
    ] {
        conn.query_drop(sql).expect("pre-test cleanup failed");
    }

    let patient = store
        .list_patients()
        .expect("list_patients failed")
        .into_iter()
        .find(|p| !store.list_procedures(p.pat_num).unwrap_or_default().is_empty())
        .expect("need a patient with at least one procedure");
    let pat_num = patient.pat_num;
    let procedure = store
        .list_procedures(pat_num)
        .unwrap()
        .into_iter()
        .next()
        .expect("procedure disappeared");
    let original = store.list_treat_plans(pat_num).unwrap();

    let outcome = (|| -> Result<(), String> {
        let new = od_core::new_treat_plan(pat_num, TreatPlanStatus::Inactive, "ODCLINIC live test");
        let num = store.upsert_treat_plan(&new)?;
        let mut plan = new.clone();
        plan.treat_plan_num = num;

        let line = TreatPlanLine {
            proc_num: procedure.proc_num,
            proc_code: "D9999".into(),
            description: "ODCLINIC live test line".into(),
            tooth_num: "30".into(),
            fee: 123.45,
            ..Default::default()
        };
        let line_num = store.upsert_treat_plan_line(&plan, &line)?;
        assert!(line_num > 0);
        let lines = store.list_treat_plan_lines(num)?;
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].proc_num, procedure.proc_num);
        println!("inactive plan {num} line {line_num}: {:.2}", lines[0].fee);

        // Promote to Active: OD keeps only one Active plan per patient.
        let before = store.list_treat_plans(pat_num)?;
        for changed in od_core::set_status(&before, num, TreatPlanStatus::Active) {
            store.upsert_treat_plan(&changed)?;
        }
        let active = store
            .list_treat_plans(pat_num)?
            .into_iter()
            .find(|p| p.treat_plan_num == num)
            .ok_or("temporary plan missing after promotion")?;
        assert_eq!(active.status, TreatPlanStatus::Active);
        assert_eq!(active.heading, "Active Treatment Plan");
        let attached = store.list_treat_plan_lines(num)?;
        assert_eq!(attached.len(), 1);
        assert_eq!(attached[0].proc_num, procedure.proc_num);

        // Save TP produces a Saved proctp snapshot.
        let (saved, copied) = od_core::snapshot_plan(
            &active,
            &attached,
            NaiveDate::from_ymd_opt(2026, 11, 21).unwrap(),
        );
        let saved_num = store.upsert_treat_plan(&saved)?;
        let mut saved_plan = saved.clone();
        saved_plan.treat_plan_num = saved_num;
        for mut copied_line in copied {
            copied_line.treat_plan_num = saved_num;
            store.upsert_treat_plan_line(&saved_plan, &copied_line)?;
        }
        let snapshot_lines = store.list_treat_plan_lines(saved_num)?;
        assert_eq!(snapshot_lines.len(), 1);
        // Active plans read their fees from procedurelog, so the archived
        // snapshot copies the procedure's real fee (OD CreateArchivedTreatPlan).
        assert_eq!(snapshot_lines[0].fee, procedure.fee);
        println!("saved snapshot {saved_num} lines: {}", snapshot_lines.len());

        // Restore the patient's original plan statuses before deleting ours.
        for original_plan in &original {
            store.upsert_treat_plan(original_plan)?;
        }
        store.delete_treat_plan(saved_num)?;
        store.delete_treat_plan(num)?;
        let remaining = store.list_treat_plans(pat_num)?;
        assert!(!remaining.iter().any(|p| p.treat_plan_num == num));
        assert!(!remaining.iter().any(|p| p.treat_plan_num == saved_num));
        Ok(())
    })();

    if let Err(err) = outcome {
        // Best-effort cleanup so a failed assertion does not leave test rows.
        for plan in store.list_treat_plans(pat_num).unwrap_or_default() {
            if plan.note == "ODCLINIC live test" {
                let _ = store.delete_treat_plan(plan.treat_plan_num);
            }
        }
        for original_plan in original {
            let _ = store.upsert_treat_plan(&original_plan);
        }
        panic!("treatment plan round trip failed: {err}");
    }
    println!("cleanup: temporary treatment plans and lines removed");
}

/// Verifies the OD `definition` and `displayfield` table shapes against the
/// live demo database, including the ARGB colour column and OD's "default list
/// means no rows" save rule. All temporary rows are removed afterwards.
#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL; writes and deletes temporary setup rows"]
fn mysql_definitions_and_display_fields_round_trip() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");

    println!(
        "definitions: {}, display fields: {}",
        store.list_definitions().map(|v| v.len()).unwrap_or_default(),
        store.list_display_fields().map(|v| v.len()).unwrap_or_default(),
    );

    let definition = Definition {
        category: DefinitionCategory::PAYMENT_TYPES,
        item_order: 999,
        item_name: "ODCLINIC_LIVE_TEST_DEF".into(),
        item_value: "T".into(),
        item_color: -16776961,
        is_hidden: true,
        ..Default::default()
    };
    let def_num = store
        .upsert_definition(&definition)
        .expect("upsert_definition failed");
    assert!(def_num > 0, "MySQL did not return a definition number");
    let read_definition = store
        .list_definitions()
        .unwrap()
        .into_iter()
        .find(|d| d.def_num == def_num)
        .expect("temporary definition was not readable");
    assert_eq!(read_definition.category, DefinitionCategory::PAYMENT_TYPES);
    assert_eq!(read_definition.item_name, "ODCLINIC_LIVE_TEST_DEF");
    assert_eq!(read_definition.item_value, "T");
    assert_eq!(read_definition.item_color, -16776961);
    assert!(read_definition.is_hidden);
    store
        .delete_definition(def_num)
        .expect("delete_definition failed");
    assert!(!store
        .list_definitions()
        .unwrap()
        .iter()
        .any(|d| d.def_num == def_num));

    // Use an OD category this demo schema does not populate, and restore its
    // original rows so a live database is left exactly as it was found.
    let category = DisplayFieldCategory::SUPER_FAMILY_GRID_COLS;
    let before: Vec<DisplayField> = store
        .list_display_fields()
        .unwrap()
        .into_iter()
        .filter(|field| field.category == category)
        .collect();
    let field = DisplayField {
        internal_name: "ODCLINIC_LIVE_TEST_FIELD".into(),
        description: "Live test column".into(),
        column_width: 123,
        category,
        item_order: 0,
        ..Default::default()
    };
    store
        .replace_display_fields(category, &[field])
        .expect("replace_display_fields failed");
    let read_field = store
        .list_display_fields()
        .unwrap()
        .into_iter()
        .find(|f| f.internal_name == "ODCLINIC_LIVE_TEST_FIELD")
        .expect("temporary display field was not readable");
    assert_eq!(read_field.description, "Live test column");
    assert_eq!(read_field.column_width, 123);
    assert_eq!(read_field.item_order, 0);
    println!("definition {def_num} and display field {} read back", read_field.display_field_num);

    store
        .replace_display_fields(category, &before)
        .expect("restoring display fields failed");
    let after: Vec<DisplayField> = store
        .list_display_fields()
        .unwrap()
        .into_iter()
        .filter(|f| f.category == category)
        .collect();
    assert_eq!(after.len(), before.len(), "display-field restore changed row count");
    println!("cleanup: temporary definition and display field removed");
}

/// Verifies the clinical slice against real OD tables: `appointment.UnschedStatus`
/// and `AptStatus = 3` for the waitlist, `histappointment` snapshots/restore, and
/// the port-local patient correction table. All temporary rows are removed.
#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL; writes and deletes temporary clinical rows"]
fn mysql_waitlist_history_and_patient_corrections_round_trip() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");

    let definitions = store.list_definitions().expect("list_definitions failed");
    println!("definitions available for waitlist status: {}", definitions.len());
    let status_def = definitions
        .iter()
        .find(|d| d.category == DefinitionCategory::RECALL_UNSCHED_STATUS && !d.is_hidden)
        .cloned();
    let temporary_def = if status_def.is_none() {
        let def = Definition {
            category: DefinitionCategory::RECALL_UNSCHED_STATUS,
            item_order: 999,
            item_name: "ODCLINIC_LIVE_TEST_WAITSTATUS".into(),
            is_hidden: true,
            ..Default::default()
        };
        let num = store.upsert_definition(&def).expect("upsert_definition failed");
        Some(Definition { def_num: num, ..def })
    } else {
        None
    };
    let status_def = status_def.or(temporary_def.clone()).expect("no waitlist status");

    let pat_num = store
        .list_patients()
        .expect("list_patients failed")
        .first()
        .map(|p| p.pat_num)
        .expect("demo database has no patients");
    let before_patient = store.get_patient(pat_num).unwrap().unwrap();

    let mut apt = Appointment {
        pat_num,
        apt_datetime: NaiveDate::from_ymd_opt(2026, 11, 25)
            .unwrap()
            .and_hms_opt(10, 30, 0)
            .unwrap(),
        apt_length: 30,
        apt_status: AppointmentStatus::UnschedList,
        notes: "ODCLINIC_LIVE_TEST_WAITLIST".into(),
        pattern: "XXXXXX".into(),
        waitlist_status: status_def.def_num,
        ..Default::default()
    };
    let apt_num = store.upsert_appointment(&apt).expect("upsert_appointment failed");
    apt.apt_num = apt_num;
    let in_waitlist = store
        .list_waitlist_appointments()
        .expect("list_waitlist_appointments failed")
        .iter()
        .any(|a| a.apt_num == apt_num && a.waitlist_status == status_def.def_num);
    assert!(in_waitlist, "temporary waitlist appointment was not readable");

    let mut change = AppointmentChange::from_snapshot(&apt, AppointmentChangeAction::Deleted);
    change.user_num = 1;
    let change_num = store
        .append_appointment_change(&change)
        .expect("append_appointment_change failed");
    assert!(change_num > 0, "histappointment did not return a number");
    let history = store
        .list_appointment_changes(apt_num)
        .expect("list_appointment_changes failed");
    assert!(!history.is_empty(), "histappointment row was not readable");
    assert_eq!(history[0].action, AppointmentChangeAction::Deleted);
    assert_eq!(history[0].snapshot.waitlist_status, status_def.def_num);
    println!("histappointment row {change_num} read back for apt {apt_num}");

    store.delete_appointment(apt_num).expect("delete_appointment failed");
    assert!(!store
        .list_waitlist_appointments()
        .unwrap()
        .iter()
        .any(|a| a.apt_num == apt_num));
    let restored = od_core::restore_appointment(&history[0]);
    let restored_num = store.upsert_appointment(&restored).expect("restore failed");
    assert_eq!(restored_num, apt_num);
    assert!(store
        .list_waitlist_appointments()
        .unwrap()
        .iter()
        .any(|a| a.apt_num == apt_num));

    let reason = "ODCLINIC_LIVE_TEST_CORRECTION";
    let correction = PatientCorrection {
        pat_num,
        reason: reason.into(),
        before: before_patient.clone(),
        ..Default::default()
    };
    let correction_num = store
        .append_patient_correction(&correction)
        .expect("append_patient_correction failed");
    assert!(correction_num > 0);
    let corrections = store
        .list_patient_corrections(pat_num)
        .expect("list_patient_corrections failed");
    let read = corrections.iter().find(|c| c.reason == reason).expect("correction not readable");
    assert_eq!(read.before.f_name, before_patient.f_name);
    println!("patient correction {correction_num} read back for patient {pat_num}");

    let normalized = if url.contains("://") {
        url.trim().to_string()
    } else {
        format!("mysql://{}", url.trim())
    };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("bad pool");
    let mut conn = pool.get_conn().expect("bad connection");
    for sql in [
        format!("DELETE FROM appointment WHERE AptNum = {apt_num}"),
        format!("DELETE FROM histappointment WHERE AptNum = {apt_num}"),
        format!("DELETE FROM odclinic_patient_correction WHERE CorrectionNum = {correction_num}"),
    ] {
        conn.query_drop(sql).expect("temporary clinical row cleanup failed");
    }
    if let Some(def) = temporary_def {
        conn.query_drop(format!("DELETE FROM definition WHERE DefNum = {}", def.def_num))
            .expect("temporary waitlist definition cleanup failed");
    }
    println!("cleanup: temporary waitlist, history and correction rows removed");
}

/// Registry round-trip against the demo OD schema: existing clinical rows are
/// read through OD tables, while the port-local clinic-card and invoice rows
/// are temporary and removed afterwards.
#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL; writes and deletes a temporary card and invoice"]
fn mysql_registry_and_clinic_card_round_trip() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");
    let patient = store
        .list_patients()
        .expect("list_patients failed")
        .into_iter()
        .next()
        .expect("need at least one patient for the registry check");
    let pat_num = patient.pat_num;

    let diseases = store
        .list_diseases(pat_num)
        .expect("list_diseases (OD disease/diseasedef) failed");
    let note = store
        .get_patient_note(pat_num)
        .expect("get_patient_note (OD patientnote) failed");
    let treatments = store
        .list_registry_treatments(pat_num)
        .expect("list_registry_treatments (OD procedurelog) failed");
    println!(
        "registry reads: {} diseases, note {}, {} treatments",
        diseases.len(),
        note.is_some(),
        treatments.len()
    );
    assert!(
        diseases.iter().all(|d| !d.disease_name.is_empty() || d.disease_def_num == 0),
        "disease names should resolve through diseasedef"
    );
    assert!(
        treatments
            .iter()
            .all(|t| !t.status_label().is_empty()),
        "procedure status must map to a label"
    );

    let statements_before: Vec<Statement> = store
        .list_statements(pat_num)
        .expect("list_statements failed");
    assert!(
        statements_before
            .iter()
            .all(|s| !s.note.contains("ODCLINIC-LIVE-AUTH")),
        "test precondition: no card-related statement should exist yet"
    );
    let card = od_domain::ClinicCard {
        pat_num,
        card_number: "ODCLINIC-LIVE-CARD".into(),
        issued_at: NaiveDate::from_ymd_opt(2026, 11, 21).unwrap(),
        issued_by: 1,
        reason: od_domain::ClinicCardReason::PaymentPlan,
        authorization: "ODCLINIC-LIVE-AUTH".into(),
        note: "ODCLINIC_LIVE_TEST".into(),
        ..Default::default()
    };
    let card_num = store
        .issue_clinic_card(&card)
        .expect("issue_clinic_card failed");
    assert!(card_num > 0);
    let cards = store
        .list_clinic_cards(pat_num)
        .expect("list_clinic_cards failed");
    let read = cards
        .iter()
        .find(|c| c.card_num == card_num)
        .expect("temporary clinic card not readable");
    assert!(read.is_active);
    assert_eq!(read.reason, od_domain::ClinicCardReason::PaymentPlan);
    assert_eq!(read.authorization, "ODCLINIC-LIVE-AUTH");

    // A card must never create a statement or other financial row. Other live
    // tests run in parallel, so compare by the card's unique authorisation
    // marker rather than by row count.
    let statements_after = store
        .list_statements(pat_num)
        .expect("list_statements failed");
    assert!(
        statements_after
            .iter()
            .all(|s| !s.note.contains("ODCLINIC-LIVE-AUTH")),
        "card created a statement"
    );

    let invoice_num = store
        .insert_statement(&Statement {
            pat_num,
            note: "ODCLINIC_LIVE_TEST invoice".into(),
            is_invoice: true,
            ..Default::default()
        })
        .expect("insert_statement is_invoice failed");
    let invoices: Vec<Statement> = store
        .list_statements(pat_num)
        .expect("list_statements failed")
        .into_iter()
        .filter(|s| s.statement_num == invoice_num)
        .collect();
    assert_eq!(invoices.len(), 1, "invoice statement not readable");
    assert!(invoices[0].is_invoice, "IsInvoice flag did not round-trip");

    store
        .deactivate_clinic_card(card_num)
        .expect("deactivate_clinic_card failed");
    let read = store
        .list_clinic_cards(pat_num)
        .expect("list_clinic_cards failed")
        .into_iter()
        .find(|c| c.card_num == card_num)
        .expect("deactivated card was deleted");
    assert!(!read.is_active, "card should be inactive, not deleted");

    let normalized = if url.contains("://") {
        url.trim().to_string()
    } else {
        format!("mysql://{}", url.trim())
    };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("bad pool");
    let mut conn = pool.get_conn().expect("bad connection");
    for sql in [
        format!("DELETE FROM odclinic_clinic_card WHERE CardNum = {card_num}"),
        format!("DELETE FROM statement WHERE StatementNum = {invoice_num}"),
    ] {
        conn.query_drop(sql).expect("temporary registry row cleanup failed");
    }
    println!("cleanup: temporary clinic card and invoice statement removed");
}

#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL"]
fn mysql_local_reports_build_from_live_data() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");
    let patients = store.list_patients().expect("list_patients failed");
    let recalls = store.list_recalls().expect("list_recalls failed");
    let appointments = store
        .list_appointments(
            NaiveDate::from_ymd_opt(2000, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            NaiveDate::from_ymd_opt(2100, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        )
        .expect("list_appointments failed");
    let today = chrono::Local::now().date_naive();

    let recall_rows = recall_report(&recalls, &patients, today);
    println!("recall report: {} row(s)", recall_rows.len());
    assert_eq!(recall_rows.len(), recalls.len());
    assert!(
        recall_rows
            .iter()
            .all(|row| !row.label.is_empty() && !row.value.is_empty()),
        "every recall should name a patient and a due state"
    );

    let asap_rows = asap_report(
        &appointments,
        &patients,
        NaiveDate::from_ymd_opt(2000, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
        NaiveDate::from_ymd_opt(2100, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
    );
    println!("ASAP report: {} row(s)", asap_rows.len());
    assert!(
        asap_rows.len() <= appointments.len(),
        "ASAP report is a filter of all appointments"
    );

    let mut plans: Vec<PayPlan> = Vec::new();
    let mut charges: Vec<PayPlanCharge> = Vec::new();
    let mut splits: Vec<PaySplit> = Vec::new();
    let mut treat_plans: Vec<TreatPlan> = Vec::new();
    let mut treat_lines: Vec<TreatPlanLine> = Vec::new();
    for patient in &patients {
        plans.extend(store.list_pay_plans(patient.pat_num).unwrap_or_default());
        charges.extend(
            store
                .list_pay_plan_charges(patient.pat_num)
                .unwrap_or_default(),
        );
        splits.extend(store.list_pay_splits(patient.pat_num).unwrap_or_default());
        for plan in store.list_treat_plans(patient.pat_num).unwrap_or_default() {
            treat_lines.extend(
                store
                    .list_treat_plan_lines(plan.treat_plan_num)
                    .unwrap_or_default(),
            );
            treat_plans.push(plan);
        }
    }

    let plan_rows = payment_plan_report(&plans, &charges, &splits, &patients, today);
    println!("payment-plan report: {} row(s)", plan_rows.len());
    assert_eq!(
        plan_rows.len(),
        plans.len() + 1,
        "one row per plan plus the total"
    );
    assert_eq!(plan_rows.last().unwrap().label, "Total outstanding");

    let tp_rows = treatment_plan_report(&treat_plans, &treat_lines, &patients);
    println!("treatment-plan report: {} row(s)", tp_rows.len());
    assert_eq!(
        tp_rows.len(),
        treat_plans.len() + 1,
        "one row per plan plus the total"
    );
    assert_eq!(tp_rows.last().unwrap().label, "Total planned");

    let combined: Vec<_> = recall_rows
        .iter()
        .chain(asap_rows.iter())
        .chain(plan_rows.iter())
        .chain(tp_rows.iter())
        .cloned()
        .collect();
    let table = od_core::ReportTable::from_rows(combined);
    let csv = render_report_csv("Local reports", "Live Demo Clinic", &table);
    let html = render_report_html("Local reports", "Live Demo Clinic", &table);
    assert!(csv.starts_with("Practice,Report,Item,Value,Detail\r\n"));
    assert!(csv.contains("Live Demo Clinic"));
    assert!(html.contains("Live Demo Clinic"));
    assert!(html.contains("<table>"));
    println!(
        "report exports: {} CSV bytes, {} HTML bytes",
        csv.len(),
        html.len()
    );
}

#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL"]
fn mysql_upstream_reports_build_from_live_data() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");
    let patients = store.list_patients().expect("list_patients failed");
    let providers = store.list_providers().expect("list_providers failed");
    let definitions = store.list_definitions().expect("list_definitions failed");
    let normalized = if url.contains("://") {
        url.trim().to_string()
    } else {
        format!("mysql://{}", url.trim())
    };
    let pool = mysql::Pool::new(mysql::Opts::from_url(&normalized).expect("bad URL"))
        .expect("bad pool");
    let mut conn = pool.get_conn().expect("bad connection");

    // Idempotent cleanup: a previous failed run may have left temp rows behind
    // before its cleanup block ran. Remove them so the assertions below see
    // exactly the rows this run writes. Child rows go before their parents.
    let _ = conn.query_drop(
        "DELETE FROM paysplit WHERE PayNum IN (SELECT PayNum FROM payment WHERE PayNote LIKE 'ODCLINIC_LIVE_TEST_PAYSHEET%')",
    );
    let _ = conn.query_drop("DELETE FROM payment WHERE PayNote LIKE 'ODCLINIC_LIVE_TEST_PAYSHEET%'");
    let _ = conn.query_drop("DELETE FROM adjustment WHERE AdjNote LIKE 'ODCLINIC_LIVE_TEST_ADJSHEET%'");
    let _ = conn.query_drop("DELETE FROM appointment WHERE Note LIKE 'ODCLINIC_LIVE_TEST_BROKEN_APPT%'");
    let _ = conn.query_drop("DELETE FROM recall WHERE Note LIKE 'ODCLINIC_LIVE_TEST_RECALLFIELDS%'");
    let _ = conn.query_drop("DELETE FROM rxpat WHERE Notes LIKE 'ODCLINIC_LIVE_TEST_RX%'");
    let _ = conn.query_drop("DELETE FROM refattach WHERE Note LIKE 'ODCLINIC_LIVE_TEST_REF%'");
    let _ = conn.query_drop("DELETE FROM referral WHERE Note LIKE 'ODCLINIC_LIVE_TEST_REF%'");

    // The demo database has no payments, so write one temporary payment split
    // across two providers to exercise the daily-payments builder on real rows.
    let temp_pat = patients[0].pat_num;
    let prov1 = providers.first().map(|p| p.provider_num).unwrap_or(0);
    let prov2 = providers.get(1).map(|p| p.provider_num).unwrap_or(prov1);
    let pay_type = definitions
        .iter()
        .find(|d| d.category == DefinitionCategory::PAYMENT_TYPES)
        .map(|d| d.def_num.min(i8::MAX as u32) as i8)
        .unwrap_or(0);
    let temp_pay_num = store
        .record_payment_splits(
            &Payment {
                pat_num: temp_pat,
                pay_date: NaiveDate::from_ymd_opt(2099, 3, 3).unwrap(),
                pay_amount: 75.0,
                pay_type,
                note: "ODCLINIC_LIVE_TEST_PAYSHEET".into(),
                ..Default::default()
            },
            &[
                PaymentAllocation { unearned_type: 0, 
                    target: PaymentTarget::Unallocated,
                    pat_num: temp_pat,
                    provider_num: prov1,
                    amount: 40.0,
                
                        pay_plan_debit_type: Default::default(),
                    },
                PaymentAllocation { unearned_type: 0, 
                    target: PaymentTarget::Unallocated,
                    pat_num: temp_pat,
                    provider_num: prov2,
                    amount: 35.0,
                
                        pay_plan_debit_type: Default::default(),
                    },
            ],
        )
        .expect("could not write temporary paysheet payment");
    println!("temporary paysheet payment: {temp_pay_num}");

    // Likewise write one temporary adjustment so the daily-adjustments builder
    // is exercised on a real OD adjustment row with a resolvable AdjType.
    let adj_type = definitions
        .iter()
        .find(|d| d.category == DefinitionCategory::ADJ_TYPES)
        .map(|d| d.def_num)
        .unwrap_or(0);
    let temp_adj_num = store
        .upsert_adjustment(&Adjustment {
            adj_date: NaiveDate::from_ymd_opt(2099, 3, 4).unwrap(),
            adj_amount: -33.25,
            pat_num: temp_pat,
            adj_type,
            provider_num: prov1,
            note: "ODCLINIC_LIVE_TEST_ADJSHEET".into(),
            ..Default::default()
        })
        .expect("could not write temporary adjsheet adjustment");
    println!("temporary adjsheet adjustment: {temp_adj_num}");

    // One temporary broken appointment so the by-status mode is exercised on a
    // real OD row with a resolvable provider.
    let temp_apt_datetime = NaiveDate::from_ymd_opt(2099, 3, 5)
        .unwrap()
        .and_hms_opt(14, 30, 0)
        .unwrap();
    let temp_apt_num = store
        .upsert_appointment(&Appointment {
            pat_num: temp_pat,
            apt_datetime: temp_apt_datetime,
            apt_length: 30,
            apt_status: AppointmentStatus::Broken,
            provider_num: prov1,
            notes: "ODCLINIC_LIVE_TEST_BROKEN_APPT".into(),
            ..Default::default()
        })
        .expect("could not write temporary broken appointment");
    println!("temporary broken appointment: {temp_apt_num}");

    // One temporary recall so the FormRpRecall field-selection report is
    // exercised on a real OD row with a known interval and due date.
    let temp_recall_due = NaiveDate::from_ymd_opt(2099, 4, 6).unwrap();
    let temp_recall_num = store
        .upsert_recall(&Recall {
            pat_num: temp_pat,
            date_due_calc: Some(temp_recall_due),
            date_due: Some(temp_recall_due),
            recall_interval: 6 * 65536,
            recall_status: 1,
            note: "ODCLINIC_LIVE_TEST_RECALLFIELDS".into(),
            ..Default::default()
        })
        .expect("could not write temporary recall");
    println!("temporary recall: {temp_recall_num}");

    let today = chrono::Local::now().date_naive();
    let from = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap();
    let to = NaiveDate::from_ymd_opt(2100, 1, 1).unwrap();

    let mut procedures = Vec::new();
    let mut adjustments = Vec::new();
    let mut payments = Vec::new();
    let mut splits = Vec::new();
    let mut plans = Vec::new();
    let mut charges = Vec::new();
    let mut seen_splits = std::collections::HashSet::new();
    let mut family_balances = Vec::new();
    for patient in &patients {
        let pat_procs = store
            .list_procedures(patient.pat_num)
            .unwrap_or_default();
        let pat_adj = store
            .list_adjustments(patient.pat_num)
            .unwrap_or_default();
        payments.extend(store.list_payments(patient.pat_num).unwrap_or_default());
        let pat_splits = store
            .list_pay_splits(patient.pat_num)
            .unwrap_or_default();
        family_balances.push((
            patient.pat_num,
            account_summary_without_insurance(&pat_procs, &pat_adj, &pat_splits).balance,
        ));
        procedures.extend(pat_procs);
        adjustments.extend(pat_adj);
        plans.extend(store.list_pay_plans(patient.pat_num).unwrap_or_default());
        charges.extend(
            store
                .list_pay_plan_charges(patient.pat_num)
                .unwrap_or_default(),
        );
        for split in pat_splits {
            if seen_splits.insert(split.split_num) {
                splits.push(split);
            }
        }
    }
    payments.sort_by_key(|payment| payment.pay_num);
    payments.dedup_by_key(|payment| payment.pay_num);

    let pay_sheet = pay_sheet_report(
        &patients,
        &providers,
        &payments,
        &splits,
        &definitions,
        from,
        to,
        PaySheetReportOptions::default(),
    );
    println!(
        "daily payments: {} payments, {} row(s)",
        payments.len(),
        pay_sheet.rows.len()
    );
    assert_eq!(
        pay_sheet.headers,
        ["Date", "Paying Patient", "Provider", "Check#", "Amount"]
    );
    assert!(pay_sheet.rows.iter().all(|row| row.len() == 5));
    assert_eq!(
        pay_sheet.rows.last().unwrap()[0],
        "Total Patient Payments:"
    );
    let temp_rows: Vec<_> = pay_sheet
        .rows
        .iter()
        .filter(|row| row[0] == "2099-03-03")
        .collect();
    assert_eq!(
        temp_rows.len(),
        2,
        "the temporary check's two provider splits are separate rows"
    );
    assert_eq!(temp_rows[0][4], "40.00");
    assert_eq!(temp_rows[1][4], "35.00");
    assert_eq!(pay_sheet.rows.last().unwrap()[4], "75.00");

    let adj_sheet = adj_sheet_report(
        &patients,
        &providers,
        &adjustments,
        &definitions,
        from,
        to,
        AdjustmentSheetReportOptions::default(),
    );
    println!(
        "daily adjustments: {} row(s)",
        adj_sheet.rows.len()
    );
    assert_eq!(
        adj_sheet.headers,
        ["Date", "Patient Name", "Prov", "AdjustmentType", "Note", "Amount"]
    );
    assert!(adj_sheet.rows.iter().all(|row| row.len() == 6));
    let temp_adj_rows: Vec<_> = adj_sheet
        .rows
        .iter()
        .filter(|row| row[4] == "ODCLINIC_LIVE_TEST_ADJSHEET")
        .collect();
    assert_eq!(temp_adj_rows.len(), 1, "temporary adjustment appears once");
    assert_eq!(temp_adj_rows[0][0], "2099-03-04");
    assert_eq!(temp_adj_rows[0][5], "-33.25", "signed AdjAmt verbatim");
    assert!(
        adj_sheet.rows.iter().all(|row| !row[0].contains("Total")),
        "no totals or footers"
    );

    // The same temporary finance-charge adjustment drives this report.
    let finance = finance_charge_report(
        &patients,
        &adjustments,
        from,
        to,
        FinanceChargeReportOptions {
            adjustment_type_num: adj_type,
            ..Default::default()
        },
    );
    println!("finance charge: {} row(s)", finance.rows.len());
    assert_eq!(
        finance.headers,
        ["PatNum", "Patient Name", "Preferred Name", "Amount"]
    );
    assert!(finance.rows.iter().all(|row| row.len() == 4));
    let finance_rows: Vec<_> = finance
        .rows
        .iter()
        .filter(|row| row[0] == temp_pat.to_string())
        .collect();
    assert_eq!(finance_rows.len(), 1, "temporary charge appears once");
    assert_eq!(finance_rows[0][1], patients[0].display_name());
    assert_eq!(finance_rows[0][2], "", "Preferred is not stored");
    assert_eq!(finance_rows[0][3], "-33.25");

    let live_appointments = store
        .list_appointments(
            from.and_hms_opt(0, 0, 0).unwrap(),
            to.and_hms_opt(0, 0, 0).unwrap(),
        )
        .expect("list_appointments failed");
    let live_procedure_codes = store
        .list_procedure_codes()
        .expect("list_procedure_codes failed");

    // All three upstream broken-appointment modes must build from live OD rows.
    let broken_by_status = broken_appointments_report(
        &patients,
        &providers,
        &live_appointments,
        &[],
        &[],
        &[],
        from,
        to,
        BrokenAppointmentReportOptions::default(),
    );
    println!("broken appointments (status): {} row(s)", broken_by_status.rows.len());
    assert_eq!(
        broken_by_status.headers,
        ["AptDate", "Patient", "Doctor", "Hygienist", "IsHyg"]
    );
    assert!(broken_by_status
        .rows
        .iter()
        .any(|r| r[0] == "03/05/2099 14:30:00"), "temporary broken appointment missing");
    assert_eq!(
        broken_by_status.rows.last().unwrap()[0],
        "Total Broken Appointments:"
    );

    let broken_by_adj = broken_appointments_report(
        &patients,
        &providers,
        &[],
        &adjustments,
        &[],
        &[],
        from,
        to,
        BrokenAppointmentReportOptions {
            mode: BrokenAppointmentMode::ByAdjustments,
            adjustment_type_num: adj_type,
            ..Default::default()
        },
    );
    println!("broken appointments (adjustments): {} row(s)", broken_by_adj.rows.len());
    assert_eq!(
        broken_by_adj.headers,
        ["Date", "Provider", "Patient", "Amount", "Note"]
    );
    assert!(broken_by_adj
        .rows
        .iter()
        .any(|r| r[4] == "ODCLINIC_LIVE_TEST_ADJSHEET"));
    assert_eq!(
        broken_by_adj.rows.last().unwrap()[0],
        "Total Broken Appointments:"
    );

    let broken_by_proc = broken_appointments_report(
        &patients,
        &providers,
        &[],
        &[],
        &procedures,
        &live_procedure_codes,
        from,
        to,
        BrokenAppointmentReportOptions {
            mode: BrokenAppointmentMode::ByProcedures,
            procedure_option: BrokenProcedureOption::Both,
            ..Default::default()
        },
    );
    println!("broken appointments (procedures): {} row(s)", broken_by_proc.rows.len());
    assert_eq!(
        broken_by_proc.headers,
        ["Date", "Provider", "Code", "Patient", "Fee"]
    );
    assert!(broken_by_proc
        .rows
        .iter()
        .any(|r| r[0] == "Total Broken Appointment Fees:"));

    // FormRpConfirm field-selection report: the temporary broken appointment
    // is in range, so its patient's LName must appear in the projection.
    let confirm = confirmation_report(
        &patients,
        &live_appointments,
        &ConfirmationReportOptions::default(),
    );
    println!("confirmation fields: {} row(s)", confirm.rows.len());
    assert_eq!(confirm.headers.len(), 15, "all standard fields selected by default");
    assert_eq!(confirm.headers[0], "LName");
    assert_eq!(confirm.headers[14], "AptDateTime");
    assert!(confirm.rows.iter().all(|r| r.len() == 15));
    assert!(
        confirm
            .rows
            .iter()
            .any(|r| r[0] == patients[0].l_name && r[14] == "03/05/2099 14:30:00"),
        "temporary appointment's patient missing from confirmation report"
    );

    // FormRpRecall field-selection report: the temporary recall is due in range
    // and carries a 6-month packed interval, which must render as MonthInterval.
    let live_recalls = store.list_recalls().expect("list_recalls failed");
    let recall_fields = recall_fields_report(
        &patients,
        &live_recalls,
        from,
        to,
        &RecallFieldsReportOptions::default(),
    );
    println!("recall fields: {} row(s)", recall_fields.rows.len());
    assert_eq!(recall_fields.headers.len(), 16, "all standard recall fields");
    assert_eq!(recall_fields.headers[14], "RecallStatus");
    assert_eq!(recall_fields.headers[15], "DateDue");
    assert!(recall_fields.rows.iter().all(|r| r.len() == 16));
    let custom_recall = recall_fields_report(
        &patients,
        &live_recalls,
        from,
        to,
        &RecallFieldsReportOptions {
            fields: vec!["LName".into(), "RecallInterval".into(), "DateDue".into()],
        },
    );
    assert_eq!(custom_recall.headers, ["LName", "MonthInterval", "DateDue"]);
    assert!(
        custom_recall.rows.iter().any(|r| {
            r[0] == patients[0].l_name
                && r[1] == "6"
                && r[2] == temp_recall_due.to_string()
        }),
        "temporary recall's MonthInterval/DateDue missing"
    );

    // FormRpProcSheet Daily Procedures: both layouts build from live OD rows.
    let proc_sheet = procedure_sheet_report(
        &patients,
        &providers,
        &procedures,
        &live_procedure_codes,
        &definitions,
        from,
        to,
        &ProcedureSheetReportOptions::default(),
    );
    println!("daily procedures (individual): {} row(s)", proc_sheet.rows.len());
    assert_eq!(
        proc_sheet.headers,
        ["Date", "Patient Name", "Code", "Tooth", "Area", "Description", "Prov", "Fee"]
    );
    assert!(proc_sheet.rows.iter().all(|r| r.len() == 8 && r[4].is_empty()));
    assert!(
        !proc_sheet.rows.is_empty(),
        "demo database should have completed procedures in range"
    );
    let proc_sheet_grouped = procedure_sheet_report(
        &patients,
        &providers,
        &procedures,
        &live_procedure_codes,
        &definitions,
        from,
        to,
        &ProcedureSheetReportOptions {
            mode: ProcedureSheetMode::GroupedByCode,
            ..Default::default()
        },
    );
    println!("daily procedures (grouped): {} row(s)", proc_sheet_grouped.rows.len());
    assert_eq!(
        proc_sheet_grouped.headers,
        ["Category", "Code", "Description", "Quantity", "Average Fee", "Total Fees"]
    );
    assert!(proc_sheet_grouped.rows.iter().all(|r| r.len() == 6));

    // FormRpProcCodes Procedure Codes report: read real fee schedules and fees
    // and verify both the plain and category layouts from live OD rows.
    let fee_schedules = store.list_fee_schedules().expect("list_fee_schedules failed");
    println!("fee schedules: {}", fee_schedules.len());
    assert!(!fee_schedules.is_empty(), "demo database should have fee schedules");
    let live_fees = store
        .list_fees(fee_schedules[0].fee_sched_num, 0)
        .expect("list_fees failed");
    println!("fees on first schedule: {}", live_fees.len());
    assert!(!live_fees.is_empty(), "first fee schedule should have fees");
    let proc_codes = procedure_codes_report(
        &live_procedure_codes,
        &live_fees,
        &definitions,
        &ProcedureCodesReportOptions {
            fee_schedule_num: fee_schedules[0].fee_sched_num,
            ..Default::default()
        },
    );
    println!("procedure codes (Code): {} row(s)", proc_codes.rows.len());
    assert_eq!(proc_codes.headers, ["Code", "Desc", "Abbr", "Fee"]);
    assert!(proc_codes.rows.iter().all(|r| r.len() == 4));
    // Every returned row has a real fee; blank fees are hidden by default.
    assert!(!proc_codes.rows.is_empty());
    assert!(proc_codes.rows.iter().all(|r| !r[3].is_empty()));
    let proc_codes_categories = procedure_codes_report(
        &live_procedure_codes,
        &live_fees,
        &definitions,
        &ProcedureCodesReportOptions {
            fee_schedule_num: fee_schedules[0].fee_sched_num,
            categories: true,
            include_blanks: true,
            ..Default::default()
        },
    );
    println!("procedure codes (Categories): {} row(s)", proc_codes_categories.rows.len());
    assert_eq!(
        proc_codes_categories.headers,
        ["Category", "Code", "Desc", "Abbr", "Fee"]
    );
    assert!(proc_codes_categories.rows.iter().all(|r| r.len() == 5));
    assert!(proc_codes_categories.rows.len() >= proc_codes.rows.len());

    // FormRpPrescriptions Prescriptions report: the demo rxpat table is empty,
    // so write one temporary prescription and read it back through the store.
    let rx_pat = patients[0].pat_num;
    let rx_prov = providers.first().map(|p| p.provider_num).unwrap_or(0);
    let rx_prov_abbr = providers
        .first()
        .map(|p| p.abbreviation.clone())
        .unwrap_or_default();
    conn.query_drop(format!(
        "INSERT INTO rxpat \
         (PatNum,RxDate,Drug,Sig,Disp,Refills,ProvNum,Notes,PharmacyNum,IsControlled, \
          SendStatus,RxCui,DosageCode,ErxGuid,IsErxOld,ErxPharmacyInfo,IsProcRequired, \
          ProcNum,PatientInstruction,ClinicNum,UserNum,RxType) \
         VALUES ({rx_pat},'2099-04-04','ODCLINIC_LIVE_TEST_RX Amoxicillin', \
          'Take one TID','21','3',{rx_prov},'ODCLINIC_LIVE_TEST_RX', \
          0,0,0,0,'','',0,'',0,0,'test',0,0,0)"
    ))
    .expect("temporary rxpat insert failed");
    let live_rx = store.list_prescriptions().expect("list_prescriptions failed");
    let rx_by_drug = prescriptions_report(
        &patients,
        &providers,
        &live_rx,
        &PrescriptionReportOptions {
            search_patient: false,
            input_text: "ODCLINIC_LIVE_TEST_RX".into(),
        },
    );
    println!("prescriptions (by drug): {} row(s)", rx_by_drug.rows.len());
    assert_eq!(
        rx_by_drug.headers,
        ["Patient Name", "Date", "Drug Name", "Directions", "Dispense", "Prov Name"]
    );
    assert_eq!(rx_by_drug.rows.len(), 1);
    assert_eq!(
        rx_by_drug.rows[0][0],
        format!("{}, {} {}", patients[0].l_name, patients[0].f_name, patients[0].middle_i)
    );
    assert_eq!(rx_by_drug.rows[0][1], "2099-04-04");
    assert_eq!(rx_by_drug.rows[0][2], "ODCLINIC_LIVE_TEST_RX Amoxicillin");
    assert_eq!(rx_by_drug.rows[0][3], "Take one TID");
    assert_eq!(rx_by_drug.rows[0][4], "21");
    assert_eq!(rx_by_drug.rows[0][5], rx_prov_abbr);
    let rx_by_patient = prescriptions_report(
        &patients,
        &providers,
        &live_rx,
        &PrescriptionReportOptions {
            search_patient: true,
            input_text: patients[0].l_name.clone(),
        },
    );
    assert!(
        rx_by_patient
            .rows
            .iter()
            .any(|r| r[2] == "ODCLINIC_LIVE_TEST_RX Amoxicillin"),
        "patient-prefix filter should include the temporary prescription"
    );

    // FormRpReferralAnalysis: point a temporary RefFrom attachment at a patient
    // with real completed production, then check the ranked row and columns.
    let codable = |proc: &&od_domain::Procedure| {
        proc.proc_status == ProcedureStatus::Complete
            && live_procedure_codes.iter().any(|c| {
                c.code_num == proc.code_num && c.code != "D9986" && c.code != "D9987"
            })
    };
    let referral_proc = procedures
        .iter()
        .find(codable)
        .expect("need one completed, codable procedure for referral analysis");
    let ref_pat = referral_proc.pat_num;
    let expected_production: f64 = procedures
        .iter()
        .filter(|p| {
            codable(p)
                && p.pat_num == ref_pat
                && p.proc_date >= from
                && p.proc_date <= to
        })
        .map(|p| p.fee * (p.unit_qty + p.base_units))
        .sum();
    // The live OD schema has several NOT NULL bookkeeping columns without
    // defaults on both tables; supply all of them explicitly.
    conn.query_drop(
        "INSERT INTO referral \
         (LName,FName,MName,Title,Address,Address2,City,ST,Zip,Telephone,Phone2,EMail, \
          Note,SSN,UsingTIN,IsHidden,NotPerson,Specialty,PatNum,Slip, \
          IsDoctor,IsTrustedDirect,IsPreferred,BusinessName,DisplayNote) \
         VALUES ('ODCLINIC_LIVE_TEST_REF','Source','Q','Dr','1 Test St','Suite 9','Addis','AA', \
          '1000','+251 11 000 0000','+251 91 000 0000','ref@live.test', \
          'ODCLINIC_LIVE_TEST_REF','123456789',1,1,0,0,0,0,0,0,0,'','')",
    )
    .expect("temporary referral insert failed");
    let ref_num: u64 = conn
        .query_first(
            "SELECT ReferralNum FROM referral WHERE Note='ODCLINIC_LIVE_TEST_REF' \
             ORDER BY ReferralNum DESC LIMIT 1",
        )
        .expect("temporary referral lookup failed")
        .expect("temporary referral missing");
    conn.query_drop(format!(
        "INSERT INTO refattach \
         (ReferralNum,PatNum,ItemOrder,RefDate,RefType,RefToStatus,IsTransitionOfCare, \
          ProcNum,ProvNum,Note) \
         VALUES ({ref_num},{ref_pat},0,'2099-05-05',1,0,0,0,0,'ODCLINIC_LIVE_TEST_REF')"
    ))
    .expect("temporary refattach insert failed");
    let live_referrals = store.list_referrals().expect("list_referrals failed");
    let live_attaches = store.list_refattaches().expect("list_refattaches failed");
    assert!(live_referrals
        .iter()
        .any(|r| r.referral_num == ref_num as u32 && r.l_name == "ODCLINIC_LIVE_TEST_REF"));
    assert!(live_attaches
        .iter()
        .any(|a| a.referral_num == ref_num as u32 && a.pat_num == ref_pat && a.ref_type == 1));
    let referral_table = referral_analysis_report(
        &procedures,
        &live_procedure_codes,
        &live_referrals,
        &live_attaches,
        &definitions,
        from,
        to,
        &ReferralAnalysisReportOptions::default(),
    );
    println!("referral analysis: {} row(s)", referral_table.rows.len());
    assert_eq!(
        referral_table.headers,
        ["Last Name", "First Name", "Count", "Production"]
    );
    let ref_row = referral_table
        .rows
        .iter()
        .find(|r| r[0] == "ODCLINIC_LIVE_TEST_REF")
        .expect("temporary referral missing from report");
    assert_eq!(ref_row[2], "1");
    assert_eq!(ref_row[3], format!("{expected_production:.2}"));
    let referral_address = referral_analysis_report(
        &procedures,
        &live_procedure_codes,
        &live_referrals,
        &live_attaches,
        &definitions,
        from,
        to,
        &ReferralAnalysisReportOptions {
            include_address: true,
            ..Default::default()
        },
    );
    assert_eq!(
        referral_address.headers,
        [
            "Last Name", "First Name", "Count", "Production", "Title", "Address", "Add2", "City",
            "State", "Zip", "Specialty"
        ]
    );
    let addr_row = referral_address
        .rows
        .iter()
        .find(|r| r[0] == "ODCLINIC_LIVE_TEST_REF")
        .expect("temporary referral missing from address report");
    assert_eq!(addr_row[4], "Dr");
    assert_eq!(addr_row[5], "1 Test St");
    assert_eq!(addr_row[6], "Suite 9");
    assert_eq!(addr_row[7], "Addis");
    assert_eq!(addr_row[8], "AA");
    assert_eq!(addr_row[9], "1000");
    assert_eq!(addr_row[10], "");

    // FormRpReferrals: the field-selection report reads the same temporary
    // referral with all 19 OD columns and applies upstream's WHERE conditions.
    let live_referral_table = referrals_report(
        &live_referrals,
        &ReferralsReportOptions {
            fields: od_domain::reports::REFERRAL_REPORT_FIELDS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            filters: Vec::new(),
        },
    );
    println!(
        "referrals field report: {} row(s), {} column(s)",
        live_referral_table.rows.len(),
        live_referral_table.headers.len()
    );
    assert_eq!(live_referral_table.headers.len(), 19);
    let live_ref_row = live_referral_table
        .rows
        .iter()
        .find(|r| r[0] == "1 Test St")
        .expect("temporary referral missing from field report");
    assert_eq!(live_ref_row[1], "Suite 9"); // Address2
    assert_eq!(live_ref_row[2], "Addis"); // City
    assert_eq!(live_ref_row[3], "ref@live.test"); // Email <- EMail
    assert_eq!(live_ref_row[4], "Source"); // FName
    assert_eq!(live_ref_row[5], "1"); // IsHidden
    assert_eq!(live_ref_row[6], "ODCLINIC_LIVE_TEST_REF"); // LName
    assert_eq!(live_ref_row[7], "Q"); // MName
    assert_eq!(live_ref_row[8], "ODCLINIC_LIVE_TEST_REF"); // Note
    assert_eq!(live_ref_row[9], "0"); // NotPerson
    assert_eq!(live_ref_row[10], "+251 91 000 0000"); // Phone2
    assert_eq!(live_ref_row[11], ref_num.to_string()); // ReferralNum
    assert_eq!(live_ref_row[12], "0"); // Specialty
    assert_eq!(live_ref_row[13], "123456789"); // SSN
    assert_eq!(live_ref_row[14], "AA"); // ST
    // OD's live referral.Telephone column is narrower than the demo value and
    // truncates it, so compare against the raw stored value rather than the
    // literal that was inserted.
    let live_phone: String = conn
        .query_first(format!(
            "SELECT Telephone FROM referral WHERE ReferralNum={ref_num}"
        ))
        .expect("temporary referral phone lookup failed")
        .expect("temporary referral has no telephone");
    assert_eq!(live_ref_row[15], live_phone); // Telephone
    assert_eq!(live_ref_row[16], "Dr"); // Title
    assert_eq!(live_ref_row[17], "1"); // UsingTIN
    assert_eq!(live_ref_row[18], "1000"); // Zip

    let live_referral_filtered = referrals_report(
        &live_referrals,
        &ReferralsReportOptions {
            fields: vec!["LName".into(), "IsHidden".into(), "UsingTIN".into()],
            filters: vec![
                ReferralFilter {
                    field: "LName".into(),
                    condition: ReportFilterCondition::Like,
                    value: "odclinic_live_test_ref".into(),
                },
                ReferralFilter {
                    field: "IsHidden".into(),
                    condition: ReportFilterCondition::Eq,
                    value: "1".into(),
                },
            ],
        },
    );
    assert_eq!(live_referral_filtered.rows.len(), 1);
    assert_eq!(
        live_referral_filtered.rows[0],
        ["ODCLINIC_LIVE_TEST_REF", "1", "1"]
    );

    let live_recalls = store.list_recalls().expect("list_recalls failed");
    let live_patient_table = patients_report(
        &patients,
        &procedures,
        &live_recalls,
        &PatientsReportOptions {
            fields: PATIENT_REPORT_FIELDS
                .iter()
                .map(|field| field.to_string())
                .collect(),
            filters: Vec::new(),
        },
    );
    println!("patients report: {} row(s)", live_patient_table.rows.len());
    assert_eq!(live_patient_table.headers.len(), PATIENT_REPORT_FIELDS.len());
    assert_eq!(live_patient_table.rows.len(), patients.len());
    assert!(live_patient_table
        .rows
        .iter()
        .all(|row| row.len() == PATIENT_REPORT_FIELDS.len()));
    // Insurance and aging-balance columns are deliberately not offered.
    assert!(!live_patient_table
        .headers
        .iter()
        .any(|header| header.contains("Ins") || header.contains("Bal")));
    let live_patient_filtered = patients_report(
        &patients,
        &procedures,
        &live_recalls,
        &PatientsReportOptions {
            fields: vec!["PatNum".into(), "LName".into()],
            filters: vec![ReportFilter {
                field: "PatNum".into(),
                condition: ReportFilterCondition::Eq,
                value: temp_pat.to_string(),
            }],
        },
    );
    assert_eq!(live_patient_filtered.rows.len(), 1);
    assert_eq!(live_patient_filtered.rows[0][0], temp_pat.to_string());

    let aging = aging_report(
        &patients,
        &procedures,
        &adjustments,
        &splits,
        &[],
        today,
        false,
        AgingOptions::default(),
    );
    println!("aging report: {} row(s)", aging.rows.len());
    assert!(!aging.headers.iter().any(|h| h.contains("Ins")));
    assert!(!aging.rows.is_empty(), "aging always writes a total row");
    assert_eq!(aging.rows.last().unwrap()[0], "Total");
    assert!(aging.rows.iter().all(|row| row.len() == aging.headers.len()));

    let custom_aging = custom_aging_report(
        &patients,
        &procedures,
        &adjustments,
        &splits,
        &charges,
        &[],
        &[],
        today,
        false,
        CustomAgingReportOptions::default(),
    );
    println!("custom aging report: {} row(s)", custom_aging.rows.len());
    assert_eq!(custom_aging.headers, [
        "Patient", "0-30 Days", "31-60 Days", "61-90 Days", "> 90 Days", "Total"
    ]);
    assert!(!custom_aging.headers.iter().any(|h| h.contains("Ins")));
    assert!(
        !custom_aging.rows.is_empty(),
        "custom aging always writes a total row"
    );
    assert_eq!(custom_aging.rows.last().unwrap()[0], "Totals:");
    assert!(custom_aging
        .rows
        .iter()
        .all(|row| row.len() == custom_aging.headers.len()));
    // Upstream refuses to refresh without at least one transaction type.
    let no_types = custom_aging_report(
        &patients,
        &procedures,
        &adjustments,
        &splits,
        &charges,
        &[],
        &[],
        today,
        false,
        CustomAgingReportOptions {
            include: CustomAgingInclude {
                procedure_fees: false,
                adjustments: false,
                pay_plan_charges: false,
                pay_plan_production: false,
                patient_payments: false,
                ..CustomAgingInclude::default()
            },
            ..Default::default()
        },
    );
    assert!(no_types.rows.is_empty());

    // Exercise all four insurance transaction types against live OD claim
    // data, including claim snapshots for the ClaimPayDate write-off mode.
    let mut claim_procs = Vec::new();
    for patient in &patients {
        claim_procs.extend(store.list_claim_procs(patient.pat_num).unwrap_or_default());
    }
    let snapshots = store.list_claim_snapshots().unwrap_or_default();
    let custom_aging_ins = custom_aging_report(
        &patients,
        &procedures,
        &adjustments,
        &splits,
        &charges,
        &claim_procs,
        &snapshots,
        today,
        true,
        CustomAgingReportOptions {
            include: CustomAgingInclude {
                insurance_payments: true,
                insurance_estimates: true,
                write_offs: true,
                write_off_estimates: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    println!("custom aging (insurance): {} row(s)", custom_aging_ins.rows.len());
    assert!(custom_aging_ins.rows.iter().all(|row| row.len() == 6));

    let plan_links = store.list_pay_plan_links().unwrap_or_default();
    let mut all_procs = Vec::new();
    let mut all_adjs = Vec::new();
    for p in &patients {
        all_procs.extend(store.list_procedures(p.pat_num).unwrap_or_default());
        all_adjs.extend(store.list_adjustments(p.pat_num).unwrap_or_default());
    }
    let detail = pay_plan_detail_report(
        &plans,
        &charges,
        &splits,
        &plan_links,
        &all_procs,
        &all_adjs,
        &patients,
        &providers,
        &family_balances,
        from,
        to,
        false,
        PayPlanReportOptions {
            show_family_balance: true,
            ..Default::default()
        },
    );
    println!("payment-plan detail: {} row(s)", detail.rows.len());
    assert_eq!(detail.headers.len(), 10, "no Ins column with insurance off, plus unapplied credit");
    assert!(detail.rows.last().unwrap()[0].starts_with("Total ("));
    assert!(detail.rows.iter().all(|row| row.len() == detail.headers.len()));

    let birthdays = birthday_report(&patients, from, to, today);
    println!("birthday report: {} row(s)", birthdays.rows.len());
    assert_eq!(birthdays.headers.len(), 10);
    assert!(birthdays.rows.iter().all(|row| row.len() == 10));

    let new_patients = new_patient_report(
        &patients,
        &procedures,
        from,
        to,
        NewPatientReportOptions {
            include_address: true,
            ..Default::default()
        },
    );
    println!("new-patient report: {} row(s)", new_patients.rows.len());
    assert_eq!(new_patients.headers.len(), 12);
    assert!(new_patients.rows.last().unwrap()[0].starts_with("Total ("));

    let appointments = store
        .list_appointments(
            from.and_hms_opt(0, 0, 0).unwrap(),
            to.and_hms_opt(0, 0, 0).unwrap(),
        )
        .expect("list_appointments failed");
    let appts = appointments_report(
        &appointments,
        &patients,
        from,
        to,
        today,
        AppointmentReportOptions {
            date_basis: AppointmentDateBasis::Appointment,
            show_note_appointments: true,
        },
    );
    println!("appointments report: {} row(s)", appts.rows.len());
    assert_eq!(appts.headers.len(), 10);
    assert!(appts.rows.iter().all(|row| row.len() == 10));
    let appts_created = appointments_report(
        &appointments,
        &patients,
        from,
        to,
        today,
        AppointmentReportOptions {
            date_basis: AppointmentDateBasis::Created,
            ..Default::default()
        },
    );
    assert_eq!(appts_created.headers.len(), 11);
    assert_eq!(appts_created.headers[0], "DateCreated");

    let active = active_patient_report(
        &patients,
        &procedures,
        &providers,
        from,
        to,
        ActivePatientReportOptions::default(),
    );
    println!("active-patient report: {} row(s)", active.rows.len());
    assert_eq!(active.headers.len(), 8);
    assert!(active.rows.last().unwrap()[7].starts_with("Patient Count:"));
    assert!(active.rows.iter().all(|row| row.len() == 8));

    let prod_daily = prod_inc_report(
        &patients,
        &providers,
        &procedures,
        &adjustments,
        &splits,
        &[], &appointments, &definitions,
        from,
        to,
        false,
        ProdIncReportOptions::default(),
    );
    println!("production/income daily: {} row(s)", prod_daily.rows.len());
    assert_eq!(prod_daily.headers.len(), 7, "no insurance columns by default");
    assert!(prod_daily.headers.iter().all(|h| !h.contains("Ins") && !h.contains("Write-off")));
    assert!(prod_daily.rows.iter().all(|row| row.len() == 7));
    let prod_provider = prod_inc_report(
        &patients,
        &providers,
        &procedures,
        &adjustments,
        &splits,
        &[], &appointments, &definitions,
        from,
        to,
        false,
        ProdIncReportOptions {
            mode: ProdIncMode::Provider,
            ..Default::default()
        },
    );
    println!("production/income provider: {} row(s)", prod_provider.rows.len());
    assert_eq!(prod_provider.headers.len(), 6);
    assert!(prod_provider.rows.last().unwrap()[0].starts_with("Total ("));
    let prod_monthly = prod_inc_report(
        &patients,
        &providers,
        &procedures,
        &adjustments,
        &splits,
        &[], &appointments, &definitions,
        from,
        to,
        false,
        ProdIncReportOptions {
            mode: ProdIncMode::Monthly,
            ..Default::default()
        },
    );
    println!("production/income monthly: {} row(s)", prod_monthly.rows.len());
    assert_eq!(prod_monthly.headers.len(), 10, "no insurance columns by default");
    assert_eq!(prod_monthly.headers[3], "Sched");
    assert_eq!(prod_monthly.headers[7], "Unearned Pt Income");
    assert!(prod_monthly.rows.iter().all(|row| row.len() == 10));
    assert!(prod_monthly.rows[prod_monthly.rows.len() - 5][0]
        .starts_with("Total Production (Production + Scheduled"));
    let prod_annual = prod_inc_report(
        &patients,
        &providers,
        &procedures,
        &adjustments,
        &splits,
        &[], &appointments, &definitions,
        from,
        to,
        false,
        ProdIncReportOptions {
            mode: ProdIncMode::Annual,
            ..Default::default()
        },
    );
    println!("production/income annual: {} row(s)", prod_annual.rows.len());
    assert_eq!(prod_annual.headers.len(), 8);
    assert_eq!(prod_annual.headers[0], "Month");
    assert_eq!(prod_annual.headers[5], "Unearned Pt Income");
    assert!(prod_annual.rows.iter().all(|row| row.len() == 8));
    let prod_ins = prod_inc_report(
        &patients,
        &providers,
        &procedures,
        &adjustments,
        &splits,
        &[], &appointments, &definitions,
        from,
        to,
        true,
        ProdIncReportOptions::default(),
    );
    assert_eq!(prod_ins.headers.len(), 9, "insurance columns appear when enabled");
    assert!(prod_ins.headers.contains(&"Write-off".to_string()));
    assert!(prod_ins.headers.contains(&"Ins Income".to_string()));

    assert!(detail.headers.contains(&"Bal Not Due".to_string()));
    for (name, table) in [
        ("Aging", &aging),
        ("Payment plans", &detail),
        ("Birthdays", &birthdays),
        ("New patients", &new_patients),
        ("Appointments", &appts),
        ("Active patients", &active),
        ("Production & income", &prod_daily),
        ("Production & income monthly", &prod_monthly),
        ("Production & income annual", &prod_annual),
        ("Daily payments", &pay_sheet),
        ("Daily adjustments", &adj_sheet),
        ("Finance charge", &finance),
        ("Broken appointments (status)", &broken_by_status),
        ("Broken appointments (adjustments)", &broken_by_adj),
        ("Broken appointments (procedures)", &broken_by_proc),
        ("Confirmations", &confirm),
        ("Recall fields", &recall_fields),
        ("Daily procedures (individual)", &proc_sheet),
        ("Daily procedures (grouped)", &proc_sheet_grouped),
        ("Procedure codes", &proc_codes),
        ("Procedure codes (categories)", &proc_codes_categories),
        ("Prescriptions (by drug)", &rx_by_drug),
        ("Prescriptions (by patient)", &rx_by_patient),
        ("Referral analysis", &referral_table),
        ("Referral analysis (address)", &referral_address),
        ("Referrals", &live_referral_table),
        ("Referrals (filtered)", &live_referral_filtered),
    ] {
        let csv = render_report_csv(name, "Live Demo Clinic", table);
        let html = render_report_html(name, "Live Demo Clinic", table);
        assert!(csv.starts_with("Practice,Report,"), "{name} CSV header");
        assert!(csv.contains("Live Demo Clinic"), "{name} CSV practice");
        assert!(html.contains("Live Demo Clinic"), "{name} HTML practice");
        assert!(html.contains("<table>"), "{name} HTML table");
    }

    // Delete the child splits before their parent payment (OD FK order).
    let split_cleanup = conn.query_drop(format!(
        "DELETE FROM paysplit WHERE PayNum = {temp_pay_num}"
    ));
    let pay_cleanup = conn.query_drop(format!(
        "DELETE FROM payment WHERE PayNum = {temp_pay_num}"
    ));
    println!(
        "paysheet temp payment cleaned up: split {:?}, payment {:?}",
        split_cleanup.is_ok(),
        pay_cleanup.is_ok()
    );
    let adj_cleanup = conn.query_drop(format!(
        "DELETE FROM adjustment WHERE AdjNum = {temp_adj_num}"
    ));
    println!("adjsheet temp adjustment cleaned up: {:?}", adj_cleanup.is_ok());
    let broken_apt_cleanup = conn.query_drop(format!(
        "DELETE FROM appointment WHERE AptNum = {temp_apt_num}"
    ));
    println!(
        "broken-appt temp appointment cleaned up: {:?}",
        broken_apt_cleanup.is_ok()
    );
    let recall_cleanup = conn.query_drop(format!(
        "DELETE FROM recall WHERE RecallNum = {temp_recall_num}"
    ));
    println!(
        "recall-fields temp recall cleaned up: {:?}",
        recall_cleanup.is_ok()
    );
    let rx_cleanup = conn.query_drop("DELETE FROM rxpat WHERE Notes LIKE 'ODCLINIC_LIVE_TEST_RX%'");
    println!("prescriptions temp rxpat cleaned up: {:?}", rx_cleanup.is_ok());
    let ref_cleanup = conn.query_drop("DELETE FROM refattach WHERE Note LIKE 'ODCLINIC_LIVE_TEST_REF%'");
    println!(
        "referral-analysis temp refattach cleaned up: {:?}",
        ref_cleanup.is_ok()
    );
    let referral_cleanup =
        conn.query_drop("DELETE FROM referral WHERE Note LIKE 'ODCLINIC_LIVE_TEST_REF%'");
    println!(
        "referral-analysis temp referral cleaned up: {:?}",
        referral_cleanup.is_ok()
    );
}

/// Read-only smoke test for the 22 second-tranche report builders against the
/// live demo database. It writes nothing; every builder is exercised with the
/// real OD rows the store returns, and the exported grids are checked for
/// consistent column counts.
#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL"]
fn mysql_second_tranche_reports_build_from_live_data() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");
    let patients = store.list_patients().expect("list_patients failed");
    let providers = store.list_providers().expect("list_providers failed");
    let definitions = store.list_definitions().expect("list_definitions failed");
    let procedure_codes = store
        .list_procedure_codes()
        .expect("list_procedure_codes failed");
    let accounts = store.list_accounts().expect("list_accounts failed");
    let journal = store
        .list_journal_entries()
        .expect("list_journal_entries failed");
    let proc_notes = store.list_proc_notes().expect("list_proc_notes failed");
    let schedules = store.list_schedules().expect("list_schedules failed");
    let schedule_ops = store.list_schedule_ops().expect("list_schedule_ops failed");
    let operatories = store.list_operatories().expect("list_operatories failed");
    let snapshots = store.list_claim_snapshots().expect("list_claim_snapshots failed");
    let links = store.list_pay_plan_links().expect("list_pay_plan_links failed");
    let discount_plans = store.list_discount_plans().expect("list_discount_plans failed");
    let discount_subs = store
        .list_discount_plan_subs()
        .expect("list_discount_plan_subs failed");
    let screens = store.list_screens().expect("list_screens failed");
    let screen_groups = store.list_screen_groups().expect("list_screen_groups failed");
    let sites = store.list_sites().expect("list_sites failed");
    let counties = store.list_counties().expect("list_counties failed");
    let races = store.list_patient_races().expect("list_patient_races failed");
    let carriers = store.list_carriers().expect("list_carriers failed");
    println!(
        "tranche 3 live inputs: {} schedules, {} scheduleops, {} operatories, {} snapshots, {} links, {} discount plans, {} discount subs, {} screens, {} groups, {} sites, {} counties, {} races, {} carriers",
        schedules.len(),
        schedule_ops.len(),
        operatories.len(),
        snapshots.len(),
        links.len(),
        discount_plans.len(),
        discount_subs.len(),
        screens.len(),
        screen_groups.len(),
        sites.len(),
        counties.len(),
        races.len(),
        carriers.len()
    );

    let from = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap();
    let to = NaiveDate::from_ymd_opt(2100, 1, 1).unwrap();
    let appointments = store
        .list_appointments(
            from.and_hms_opt(0, 0, 0).unwrap(),
            to.and_hms_opt(0, 0, 0).unwrap(),
        )
        .expect("list_appointments failed");

    let mut procedures = Vec::new();
    let mut adjustments = Vec::new();
    let mut splits = Vec::new();
    let mut claim_procs = Vec::new();
    let mut statements = Vec::new();
    let mut plans = Vec::new();
    let mut charges = Vec::new();
    let mut treat_plans = Vec::new();
    let mut plan_lines = Vec::new();
    let mut seen_splits = std::collections::HashSet::new();
    for patient in &patients {
        let pat_num = patient.pat_num;
        procedures.extend(store.list_procedures(pat_num).unwrap_or_default());
        adjustments.extend(store.list_adjustments(pat_num).unwrap_or_default());
        claim_procs.extend(store.list_claim_procs(pat_num).unwrap_or_default());
        statements.extend(store.list_statements(pat_num).unwrap_or_default());
        plans.extend(store.list_pay_plans(pat_num).unwrap_or_default());
        charges.extend(store.list_pay_plan_charges(pat_num).unwrap_or_default());
        for split in store.list_pay_splits(pat_num).unwrap_or_default() {
            if seen_splits.insert(split.split_num) {
                splits.push(split);
            }
        }
        for plan in store.list_treat_plans(pat_num).unwrap_or_default() {
            let plan_num = plan.treat_plan_num;
            plan_lines.extend(store.list_treat_plan_lines(plan_num).unwrap_or_default());
            treat_plans.push(plan);
        }
    }
    println!(
        "second-tranche live inputs: {} patients, {} providers, {} procedures, {} splits, {} notes, {} accounts, {} journal lines, {} plans",
        patients.len(),
        providers.len(),
        procedures.len(),
        splits.len(),
        proc_notes.len(),
        accounts.len(),
        journal.len(),
        treat_plans.len()
    );

    // A helper closure checks every row has the header's column count and that
    // the CSV/HTML exports include the practice name.
    let check = |name: &str, table: &od_core::ReportTable| {
        assert!(
            table.rows.iter().all(|row| row.len() == table.headers.len()),
            "{name}: row width does not match headers"
        );
        let csv = render_report_csv(name, "Live Demo Clinic", table);
        let html = render_report_html(name, "Live Demo Clinic", table);
        assert!(csv.starts_with("Practice,Report,"), "{name} CSV header");
        assert!(html.contains("<table>"), "{name} HTML table");
        println!("{name}: {} row(s), {} column(s)", table.rows.len(), table.headers.len());
    };

    check(
        "Balance Sheet",
        &balance_sheet_report(&accounts, &journal, to, true),
    );
    check(
        "General Ledger",
        &general_ledger_report(&accounts, &journal, from, to, true),
    );
    check(
        "Profit & Loss",
        &profit_loss_report(&accounts, &journal, from, to),
    );
    check(
        "Hidden Pay Splits",
        &hidden_pay_splits_report(
            &patients,
            &providers,
            &definitions,
            &splits,
            from,
            to,
            &HiddenPaySplitsReportOptions::default(),
        ),
    );
    for (label, tab) in [
        ("Unearned Income (Line Item)", UnearnedIncomeTab::LineItem),
        ("Unearned Income (Accounts)", UnearnedIncomeTab::Accounts),
        ("Unearned Income (Allocation)", UnearnedIncomeTab::Allocation),
        ("Unearned Income (Net)", UnearnedIncomeTab::Net),
    ] {
        check(
            label,
            &unearned_income_report(
                &patients,
                &providers,
                &definitions,
                &procedures,
                &splits,
                from,
                to,
                &UnearnedIncomeReportOptions {
                    tab,
                    exclude_zero_allocation: false,
                    ..Default::default()
                },
            ),
        );
    }
    check(
        "Overpaid Procedures",
        &proc_overpaid_report(
            &patients,
            &procedures,
            &procedure_codes,
            &claim_procs,
            &splits,
            from,
            to,
            &ProcOverpaidReportOptions::default(),
        ),
    );
    check(
        "Receivables Breakdown",
        &receivables_breakdown_report(
            &patients,
            &definitions,
            &procedures,
            &splits,
            &claim_procs,
            to,
            false,
        ),
    );
    check(
        "Service Date View",
        &service_date_view_report(&patients, &procedures, &splits, &claim_procs, from, to),
    );
    check(
        "Procedure Notes",
        &proc_note_report(
            &patients,
            &providers,
            &procedures,
            &procedure_codes,
            &proc_notes,
            from,
            to,
            &ProcNoteReportOptions::default(),
        ),
    );
    check(
        "Dental Sealant Measure",
        &dental_sealant_measure_report(&patients, &providers, &procedures, &procedure_codes, 2026),
    );
    check(
        "Production Goal",
        &production_goal_report(
            &procedures,
            &appointments,
            &adjustments,
            &claim_procs,
            &schedules,
            &schedule_ops,
            &providers,
            &operatories,
            from,
            to,
        ),
    );
    check(
        "Provider Payroll",
        &provider_payroll_report(
            &patients,
            &providers,
            &procedures,
            &claim_procs,
            &snapshots,
            &adjustments,
            &splits,
            from,
            to,
            false,
        ),
    );
    check(
        "Presented Treatment Production",
        &presented_treatment_production_report(
            &patients,
            &providers,
            &procedures,
            &treat_plans,
            &plan_lines,
            &claim_procs,
            &adjustments,
            from,
            to,
            false,
        ),
    );
    check(
        "Plan Presentation Statistics",
        &treat_plan_presentation_statistics_report(
            &providers,
            &treat_plans,
            &plan_lines,
            &procedures,
            &claim_procs,
            &adjustments,
            &appointments,
            from,
            to,
            true,
        ),
    );
    let fee_schedules = store.list_fee_schedules().unwrap_or_default();
    let subscriptions: Vec<od_core::DiscountPlanSubscription> = discount_subs
        .iter()
        .filter_map(|sub| {
            let plan = discount_plans
                .iter()
                .find(|p| p.discount_plan_num == sub.discount_plan_num)?;
            let patient = patients.iter().find(|p| p.pat_num == sub.pat_num)?;
            Some(od_core::DiscountPlanSubscription {
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
        .collect();
    check("Discount Plans", &discount_plan_report(&subscriptions, ""));

    let fmt_date = |d: Option<NaiveDate>| {
        d.map(|d| d.format("%m/%d/%Y").to_string())
            .unwrap_or_default()
    };
    let pop_rows: Vec<PublicHealthRow> = patients
        .iter()
        .filter_map(|patient| {
            let proc = procedures
                .iter()
                .find(|p| p.pat_num == patient.pat_num)?;
            let site = sites.iter().find(|s| s.site_num == patient.site_num);
            let county = counties
                .iter()
                .find(|c| c.county_name == patient.county);
            Some(PublicHealthRow {
                values: vec![
                    patient.pat_num.to_string(),
                    fmt_date(Some(proc.proc_date)),
                    providers
                        .iter()
                        .find(|v| v.provider_num == proc.provider_num)
                        .map(|v| format!("{}, {}", v.l_name, v.f_name))
                        .unwrap_or_default(),
                    patient.county.clone(),
                    county.map(|c| c.county_code.clone()).unwrap_or_default(),
                    site.map(|s| s.description.clone()).unwrap_or_default(),
                    site.map(|s| s.note.clone()).unwrap_or_default(),
                    patient.grade_level.to_string(),
                    fmt_date(patient.birth_date),
                    races
                        .iter()
                        .filter(|r| r.pat_num == patient.pat_num)
                        .map(|r| r.race.to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                    patient.gender.as_od().to_string(),
                    patient.urgency.to_string(),
                    patient.billing_type.to_string(),
                    patient.planned_is_done.to_string(),
                    "0".to_string(),
                ],
            })
        })
        .collect();
    check(
        "Public Health (Population)",
        &ph_raw_population_report(&pop_rows),
    );

    let screen_rows: Vec<PublicHealthRow> = screens
        .iter()
        .map(|screen| {
            let group = screen_groups
                .iter()
                .find(|g| g.screen_group_num == screen.screen_group_num);
            let county = counties.iter().find(|c| {
                group
                    .map(|g| c.county_name == g.county)
                    .unwrap_or(false)
            });
            let school = sites.iter().find(|s| {
                group
                    .map(|g| s.description == g.grade_school)
                    .unwrap_or(false)
            });
            PublicHealthRow {
                values: vec![
                    group
                        .map(|g| fmt_date(g.sg_date))
                        .unwrap_or_default(),
                    group.map(|g| g.prov_name.clone()).unwrap_or_default(),
                    group.map(|g| g.county.clone()).unwrap_or_default(),
                    county.map(|c| c.county_code.clone()).unwrap_or_default(),
                    school.map(|s| s.description.clone()).unwrap_or_default(),
                    school.map(|s| s.note.clone()).unwrap_or_default(),
                    group.map(|g| g.place_service.to_string()).unwrap_or_default(),
                    screen.grade_level.to_string(),
                    screen.age.to_string(),
                    fmt_date(screen.birth_date),
                    screen.race_old.to_string(),
                    screen.gender.to_string(),
                    screen.urgency.to_string(),
                    screen.comments.clone(),
                ],
            }
        })
        .collect();
    check(
        "Public Health (Screen)",
        &ph_raw_screen_report(&screen_rows, false),
    );
    check(
        "Overcharged Payment Plans",
        &dpp_overcharged_report(
            &patients,
            &providers,
            &procedures,
            &procedure_codes,
            &adjustments,
            &claim_procs,
            &links,
            &plans,
            &charges,
            &splits,
            from,
            to,
        ),
    );
    check(
        "Patient Portion Uncollected",
        &patient_portion_uncollected_report(
            &patients,
            &procedures,
            &procedure_codes,
            &adjustments,
            &splits,
            &claim_procs,
            &definitions,
            from,
            to,
            false, // Ethiopia domain: no insurance estimates
        ),
    );
    check(
        "Treatment Finder",
        &treatment_finder_report(
            &patients,
            &procedures,
            &procedure_codes,
            &appointments,
            &claim_procs,
            &[], // OD `insplan` is not exposed through Store
            &carriers,
            from,
            to,
            chrono::Local::now().date_naive(),
            &TreatmentFinderReportOptions::default(),
            false,
        ),
    );
    let slip_day = to;
    check(
        "Routing Slips",
        &routing_slips_report(&patients, &providers, &procedures, &appointments, slip_day),
    );
    let labels: Vec<_> = patients
        .iter()
        .map(|p| od_core::LabelAddress {
            line1: format!("{} {}", p.f_name, p.l_name),
            line2: p.address.clone(),
            city_state_zip: format!("{}, {}   {}", p.city, p.state, p.zip),
            copies: 1,
        })
        .collect();
    check("Laser Labels", &laser_labels_report(&labels));
    let insurance_labels = od_core::carrier_label_addresses(
        &carriers,
        &od_domain::reports_ext::LaserLabelOptions {
            ins_mode: od_domain::reports_ext::LaserInsMode::Range,
            ..Default::default()
        },
    );
    check(
        "Laser Labels (Insurance)",
        &laser_labels_report(&insurance_labels),
    );
    // The birthday helper is exercised separately so it stays covered.
    let _birthday_labels = patient_label_addresses(&patients, 1, 1);
    check("Print Preview", &print_preview_report());
    check(
        "Statements (grid)",
        &statement_report(&patients, &statements, &procedures, &splits, &adjustments),
    );
}

/// Writes one temporary discount plan and subscription into the real OD
/// tables, exercises terminate/reopen, and then removes both rows.
#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL; writes and deletes two rows"]
fn mysql_discount_plans_and_subscriptions_round_trip() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");
    let normalized = if url.contains("://") {
        url.trim().to_string()
    } else {
        format!("mysql://{}", url.trim())
    };
    let pool = mysql::Pool::new(mysql::Opts::from_url(&normalized).expect("bad URL"))
        .expect("bad pool");
    let mut conn = pool.get_conn().expect("bad connection");

    let plan_num: u32 = 2_100_000_011;
    let sub_num: u32 = 2_100_000_012;

    // Idempotent cleanup from a previous failed run: child before parent.
    let _ = conn.exec_drop("DELETE FROM discountplansub WHERE DiscountSubNum=?", (sub_num,));
    let _ = conn.exec_drop("DELETE FROM discountplan WHERE DiscountPlanNum=?", (plan_num,));

    let patient = store
        .list_patients()
        .unwrap()
        .into_iter()
        .next()
        .expect("need at least one patient");
    let fee_sched_num = store
        .list_fee_schedules()
        .unwrap_or_default()
        .first()
        .map(|f| f.fee_sched_num)
        .unwrap_or(0);
    let def_num = store
        .list_definitions()
        .unwrap_or_default()
        .first()
        .map(|d| d.def_num)
        .unwrap_or(0);

    let plan = DiscountPlan {
        discount_plan_num: plan_num,
        description: "ODCLINIC_LIVE_TEST plan".into(),
        fee_sched_num,
        def_num,
        is_hidden: false,
        note: "temporary".into(),
        annual_max: 123.5,
        ..Default::default()
    };
    store
        .upsert_discount_plan(&plan)
        .expect("upsert_discount_plan failed");

    let effective = NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
    let sub = DiscountPlanSub {
        discount_sub_num: sub_num,
        discount_plan_num: plan_num,
        pat_num: patient.pat_num,
        date_effective: Some(effective),
        date_term: None,
        note: "temporary".into(),
    };
    store
        .upsert_discount_plan_sub(&sub)
        .expect("upsert_discount_plan_sub failed");

    let plans = store.list_discount_plans().unwrap();
    let stored_plan = plans
        .iter()
        .find(|p| p.discount_plan_num == plan_num)
        .expect("plan not readable");
    assert_eq!(stored_plan.description, plan.description);
    assert_eq!(stored_plan.annual_max, 123.5);

    let sub = store
        .list_discount_plan_subs()
        .unwrap()
        .into_iter()
        .find(|s| s.discount_sub_num == sub_num)
        .expect("subscription not readable");
    assert_eq!(sub.pat_num, patient.pat_num);
    assert_eq!(sub.date_effective, Some(effective));
    assert!(sub.is_open_ended());
    assert!(sub.is_valid_for_date(effective + chrono::Duration::days(10)));

    // Updating keeps one row and changes every field.
    let mut updated = stored_plan.clone();
    updated.description = "ODCLINIC_LIVE_TEST plan edited".into();
    updated.annual_max = 200.0;
    store.upsert_discount_plan(&updated).expect("plan update failed");
    let plans = store.list_discount_plans().unwrap();
    assert_eq!(
        plans.iter().filter(|p| p.discount_plan_num == plan_num).count(),
        1
    );
    let edited = plans
        .iter()
        .find(|p| p.discount_plan_num == plan_num)
        .unwrap();
    assert_eq!(edited.description, updated.description);
    assert_eq!(edited.annual_max, 200.0);

    // Terminate then reopen; the row survives both operations.
    let term = effective + chrono::Duration::days(30);
    store
        .terminate_discount_plan_sub(sub_num, term)
        .expect("terminate failed");
    let terminated = store
        .list_discount_plan_subs()
        .unwrap()
        .into_iter()
        .find(|s| s.discount_sub_num == sub_num)
        .unwrap();
    assert_eq!(terminated.date_term, Some(term));
    assert!(terminated.is_valid_for_date(term));
    assert!(!terminated.is_valid_for_date(term + chrono::Duration::days(1)));

    let mut reopened = terminated;
    reopened.date_term = None;
    store
        .upsert_discount_plan_sub(&reopened)
        .expect("reopen failed");
    let reopened = store
        .list_discount_plan_subs()
        .unwrap()
        .into_iter()
        .find(|s| s.discount_sub_num == sub_num)
        .unwrap();
    assert!(reopened.is_open_ended());
    assert_eq!(
        store
            .list_discount_plan_subs()
            .unwrap()
            .iter()
            .filter(|s| s.discount_sub_num == sub_num)
            .count(),
        1
    );

    // The live rows feed the same report join as the GUI.
    let live_plans = store.list_discount_plans().unwrap();
    let definitions = store.list_definitions().unwrap_or_default();
    let fee_schedules = store.list_fee_schedules().unwrap_or_default();
    let joined: Vec<od_core::DiscountPlanSubscription> = store
        .list_discount_plan_subs()
        .unwrap()
        .iter()
        .filter_map(|s| {
            let plan = live_plans
                .iter()
                .find(|p| p.discount_plan_num == s.discount_plan_num)?;
            Some(od_core::DiscountPlanSubscription {
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
                date_effective: s.date_effective,
                date_term: s.date_term,
                patient: patient.display_name(),
            })
        })
        .collect();
    let table = discount_plan_report(&joined, "ODCLINIC_LIVE_TEST");
    assert_eq!(table.rows.len(), 1, "only the temporary plan matches the filter");
    assert!(table.rows[0][0].contains("edited"));

    conn.exec_drop("DELETE FROM discountplansub WHERE DiscountSubNum=?", (sub_num,))
        .unwrap();
    conn.exec_drop("DELETE FROM discountplan WHERE DiscountPlanNum=?", (plan_num,))
        .unwrap();
    println!("discount plan live round-trip: OK");
}

/// Writes a temporary plan, subscription, procedure and adjustment to the real
/// OD tables, verifies discount pricing and completion, then removes them.
#[test]
#[ignore = "requires a real demo ODCLINIC_DB_URL; writes and deletes four rows"]
fn mysql_discount_plan_applies_to_procedure_completion() {
    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");
    let normalized = if url.contains("://") {
        url.trim().to_string()
    } else {
        format!("mysql://{}", url.trim())
    };
    let pool = mysql::Pool::new(mysql::Opts::from_url(&normalized).expect("bad URL"))
        .expect("bad pool");
    let mut conn = pool.get_conn().expect("bad connection");

    let plan_num: u32 = 2_100_000_021;
    let sub_num: u32 = 2_100_000_022;
    let adj_num: u32 = 2_100_000_023;
    let mut proc_num: u32 = 0;
    let created_adj_num;

    // Idempotent cleanup from a previous failed run: children before parents.
    let _ = conn.exec_drop("DELETE FROM adjustment WHERE AdjNum=?", (adj_num,));
    let _ = conn.exec_drop(
        "DELETE FROM adjustment WHERE AdjNote='ODCLINIC_LIVE_TEST discount'",
        (),
    );
    let _ = conn.exec_drop("DELETE FROM procedurelog WHERE ProcNum=?", (proc_num,));
    let _ = conn.exec_drop("DELETE FROM discountplansub WHERE DiscountSubNum=?", (sub_num,));
    let _ = conn.exec_drop("DELETE FROM discountplan WHERE DiscountPlanNum=?", (plan_num,));

    let patient = store
        .list_patients()
        .unwrap()
        .into_iter()
        .next()
        .expect("need at least one patient");
    let def_num = store
        .list_definitions()
        .unwrap_or_default()
        .first()
        .map(|d| d.def_num)
        .unwrap_or(0);

    // Find a fee schedule with a usable fee for a real CDT code.
    let mut chosen: Option<(u32, u32, f64)> = None;
    for schedule in store.list_fee_schedules().unwrap_or_default() {
        if let Some(fee) = store
            .list_fees(schedule.fee_sched_num, 0)
            .unwrap_or_default()
            .into_iter()
            .find(|f| f.amount > 0.0 && f.amount != -1.0)
        {
            chosen = Some((schedule.fee_sched_num, fee.code_num, fee.amount));
            break;
        }
    }
    let (fee_sched_num, code_num, reduced_fee) =
        chosen.expect("need a fee schedule with a positive fee");

    let effective = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
    let today = chrono::Local::now().date_naive();
    let plan = DiscountPlan {
        discount_plan_num: plan_num,
        description: "ODCLINIC_LIVE_TEST apply plan".into(),
        fee_sched_num,
        def_num,
        is_hidden: false,
        note: "temporary".into(),
        annual_max: -1.0, // unlimited, matching OD
        ..Default::default()
    };
    store
        .upsert_discount_plan(&plan)
        .expect("upsert_discount_plan failed");
    store
        .upsert_discount_plan_sub(&DiscountPlanSub {
            discount_sub_num: sub_num,
            discount_plan_num: plan_num,
            pat_num: patient.pat_num,
            date_effective: Some(effective),
            date_term: None,
            note: "temporary".into(),
        })
        .expect("upsert_discount_plan_sub failed");

    let charged_fee = reduced_fee + 37.5;
    proc_num = store
        .insert_procedure(&Procedure {
            pat_num: patient.pat_num,
            code_num,
            proc_date: today,
            fee: charged_fee,
            unit_qty: 1.0,
            provider_num: patient.primary_provider_num,
            ..Default::default()
        })
        .expect("insert_procedure failed");
    assert!(proc_num > 0);

    let proc = store
        .list_procedures(patient.pat_num)
        .unwrap()
        .into_iter()
        .find(|p| p.proc_num == proc_num)
        .expect("procedure not readable");
    assert_eq!(proc.proc_status, ProcedureStatus::TreatmentPlanned);
    assert_eq!(proc.fee, charged_fee);

    let history = store.list_procedures(patient.pat_num).unwrap();
    let (bucket_start, bucket_end) = annual_bucket(
        proc.proc_date,
        Some(effective),
        None,
        today,
    );
    let running_total: f64 = store
        .list_adjustments(patient.pat_num)
        .unwrap_or_default()
        .iter()
        .filter(|a| {
            a.adj_amount < 0.0
                && a.adj_type == def_num
                && a.proc_num > 0
                && a.proc_num != proc.proc_num
                && a.proc_date >= bucket_start
                && a.proc_date <= bucket_end
        })
        .map(|a| -a.adj_amount)
        .sum();
    let result = discount_amount(&DiscountInput {
        procedure: &proc,
        plan: &plan,
        subscription: &store
            .list_discount_plan_subs()
            .unwrap()
            .into_iter()
            .find(|s| s.discount_sub_num == sub_num)
            .unwrap(),
        discount_fee: Some(reduced_fee),
        running_total,
        history: &history,
        frequency_codes: &FrequencyCodeSets::default(),
        today,
    });
    assert_eq!(result.amount, 37.5, "charged fee minus reduced fee");

    created_adj_num = store
        .upsert_adjustment(&Adjustment {
            adj_date: today,
            proc_date: proc.proc_date,
            adj_amount: -result.amount,
            pat_num: patient.pat_num,
            adj_type: def_num,
            provider_num: proc.provider_num,
            proc_num: proc.proc_num,
            note: "ODCLINIC_LIVE_TEST discount".into(),
            ..Default::default()
        })
        .expect("upsert_adjustment failed");
    store
        .set_procedure_complete(proc_num, result.amount)
        .expect("set_procedure_complete failed");

    let completed = store
        .list_procedures(patient.pat_num)
        .unwrap()
        .into_iter()
        .find(|p| p.proc_num == proc_num)
        .unwrap();
    assert_eq!(completed.proc_status, ProcedureStatus::Complete);
    assert_eq!(completed.proc_date, today, "service date is preserved");

    let stored_adj = store
        .list_adjustments(patient.pat_num)
        .unwrap()
        .into_iter()
        .find(|a| a.adj_num == created_adj_num)
        .expect("adjustment not readable");
    assert_eq!(stored_adj.adj_amount, -37.5);
    assert_eq!(stored_adj.proc_num, proc_num);
    assert_eq!(stored_adj.adj_type, def_num);
    assert_eq!(stored_adj.proc_date, today);

    // OD stores the booked amount on the procedure itself as well.
    let booked: f64 = conn
        .exec_first(
            "SELECT COALESCE(DiscountPlanAmt,0) FROM procedurelog WHERE ProcNum=?",
            (proc_num,),
        )
        .unwrap()
        .expect("procedure row disappeared");
    assert_eq!(booked, 37.5);

    let _ = conn.exec_drop("DELETE FROM adjustment WHERE AdjNum=?", (created_adj_num,));
    let _ = conn.exec_drop("DELETE FROM procedurelog WHERE ProcNum=?", (proc_num,));
    let _ = conn.exec_drop("DELETE FROM discountplansub WHERE DiscountSubNum=?", (sub_num,));
    let _ = conn.exec_drop("DELETE FROM discountplan WHERE DiscountPlanNum=?", (plan_num,));
    println!("discount plan applied to procedure completion: OK");
}

/// Dynamic payment plan: link production, issue due charges idempotently, and
/// verify the linked FKey/LinkType survive the round trip.
#[test]
#[ignore = "requires ODCLINIC_DB_URL pointing at a MySQL/OpenDental database"]
fn mysql_live_dynamic_plan_links_issue_due_charges() {
    use od_domain::{
        DynamicPayPlanTPOption, PayPlan, PayPlanCharge, PayPlanFrequency, PayPlanLink, PaymentSchedule,
    };

    let url = std::env::var("ODCLINIC_DB_URL")
        .expect("ODCLINIC_DB_URL must be set to run the live MySQL check");
    let store = MySqlStore::new(&url).expect("could not connect to the demo database");
    let pat_num = store
        .list_patients()
        .expect("list_patients failed")
        .into_iter()
        .next()
        .expect("need at least one patient")
        .pat_num;

    let normalized = if url.contains("://") { url.trim().to_string() } else { format!("mysql://{}", url.trim()) };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("bad pool");
    let mut conn = pool.get_conn().expect("bad connection");

    // Pick an existing completed procedure for this patient, or skip gracefully.
    let proc_num = store
        .list_procedures(pat_num)
        .unwrap()
        .into_iter()
        .find(|p| p.proc_status == od_domain::ProcedureStatus::Complete)
        .map(|p| p.proc_num);
    let Some(proc_num) = proc_num else {
        println!("no completed procedure for patient {pat_num}; skipping dynamic plan live check");
        return;
    };

    let plan = PayPlan {
        pat_num,
        guarantor: pat_num,
        pay_plan_date: NaiveDate::from_ymd_opt(2097, 1, 1).unwrap(),
        date_pay_plan_start: Some(NaiveDate::from_ymd_opt(2097, 1, 1).unwrap()),
        apr: 0.0,
        pay_amt: 50.0,
        number_of_payments: 0,
        charge_frequency: PayPlanFrequency::Monthly,
        pay_schedule: PaymentSchedule::Monthly,
        is_dynamic: true,
        dynamic_pay_plan_tp_option: DynamicPayPlanTPOption::None,
        note: "ODCLINIC_LIVE_DYNAMIC_PLAN".into(),
        ..Default::default()
    };
    let plan_num = store.upsert_pay_plan(&plan).expect("upsert_pay_plan failed");
    let link = PayPlanLink {
        pay_plan_num: plan_num,
        link_type: PayPlanLink::PROCEDURE,
        fkey: proc_num,
        ..Default::default()
    };
    let link_num = store.upsert_pay_plan_link(&link).expect("upsert_pay_plan_link failed");

    let stored_plan = store
        .list_pay_plans(pat_num)
        .unwrap()
        .into_iter()
        .find(|p| p.pay_plan_num == plan_num)
        .expect("plan not readable");
    assert!(stored_plan.is_dynamic);
    assert_eq!(stored_plan.dynamic_pay_plan_tp_option, DynamicPayPlanTPOption::None);

    let links = store.list_pay_plan_links().unwrap();
    let own_links: Vec<_> = links.into_iter().filter(|l| l.pay_plan_num == plan_num).collect();
    assert_eq!(own_links.len(), 1);

    let procs = store.list_procedures(pat_num).unwrap();
    let adjs = store.list_adjustments(pat_num).unwrap();
    let splits = store.list_pay_splits(pat_num).unwrap();
    let existing: Vec<PayPlanCharge> = store
        .list_pay_plan_charges(pat_num)
        .unwrap()
        .into_iter()
        .filter(|c| c.pay_plan_num == plan_num)
        .collect();
    let due = od_core::issue_charges_due(
        &stored_plan,
        &own_links,
        &procs,
        &adjs,
        &splits,
        &existing,
        NaiveDate::from_ymd_opt(2097, 3, 1).unwrap(),
    );
    let mut written = Vec::new();
    for c in &due {
        written.push(store.upsert_pay_plan_charge(c).expect("upsert_pay_plan_charge failed"));
    }

    let saved: Vec<PayPlanCharge> = store
        .list_pay_plan_charges(pat_num)
        .unwrap()
        .into_iter()
        .filter(|c| c.pay_plan_num == plan_num)
        .collect();
    let again = od_core::issue_charges_due(
        &stored_plan,
        &own_links,
        &procs,
        &adjs,
        &splits,
        &saved,
        NaiveDate::from_ymd_opt(2097, 3, 1).unwrap(),
    );
    assert!(again.is_empty(), "dynamic issuance must be idempotent");

    let mid = NaiveDate::from_ymd_opt(2097, 2, 15).unwrap();
    let rec = od_core::reconciliation(&stored_plan, &own_links, &procs, &adjs, &splits, &saved, mid);
    assert!(rec.expected_due_count >= 2);
    assert!(rec.issued_count <= rec.expected_due_count);

    // Clean up in dependency order.
    let _ = conn.exec_drop("DELETE FROM payplancharge WHERE PayPlanNum=?", (plan_num,));
    let _ = conn.exec_drop("DELETE FROM payplanlink WHERE PayPlanLinkNum=?", (link_num,));
    let _ = conn.exec_drop("DELETE FROM payplan WHERE PayPlanNum=?", (plan_num,));
    println!(
        "dynamic plan round trip: {} charge(s) issued, idempotent; link {link_num} cleaned up",
        written.len()
    );
}

#[test]
#[ignore = "live MySQL: set ODCLINIC_DB_URL=mysql://user:pass@host:3306/db"]
fn mysql_live_plan_payment_applies_to_due_charges() {
    use od_core::allocate_plan_payment;
    use od_domain::{PayPlan, PayPlanCharge, PayPlanChargeType, PayPlanDebitType, Payment, PaymentTarget};

    let url = std::env::var("ODCLINIC_DB_URL").expect("ODCLINIC_DB_URL must be set");
    let store = MySqlStore::new(&url).expect("connect failed");
    let pat_num = store.list_patients().expect("list_patients failed").into_iter().next().expect("need patient").pat_num;
    let normalized = if url.contains("://") { url.trim().to_string() } else { format!("mysql://{}", url.trim()) };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("pool");
    let mut conn = pool.get_conn().expect("conn");
    let today = NaiveDate::from_ymd_opt(2098, 1, 1).unwrap();

    let plan_num = store.upsert_pay_plan(&PayPlan {
        pat_num, guarantor: pat_num, pay_plan_date: today, is_dynamic: false, note: "ODCLINIC_LIVE_CHARGE_APPLY".into(), ..Default::default()
    }).expect("upsert_pay_plan");
    let c1 = store.upsert_pay_plan_charge(&PayPlanCharge {
        pay_plan_num: plan_num, pat_num, guarantor: pat_num, charge_date: today, principal: 100.0, interest: 10.0,
        charge_type: PayPlanChargeType::Debit, ..Default::default()
    }).expect("charge1");
    let c2 = store.upsert_pay_plan_charge(&PayPlanCharge {
        pay_plan_num: plan_num, pat_num, guarantor: pat_num, charge_date: today + chrono::Duration::days(30), principal: 100.0, interest: 0.0,
        charge_type: PayPlanChargeType::Debit, ..Default::default()
    }).expect("charge2");

    let charges: Vec<_> = store.list_pay_plan_charges(pat_num).unwrap().into_iter().filter(|c| c.pay_plan_num == plan_num).collect();
    let splits = store.list_pay_splits(pat_num).unwrap();
    let allocs = allocate_plan_payment(plan_num, 50.0, &charges, &splits, today + chrono::Duration::days(1));
    assert!(allocs.iter().any(|a| matches!(a.target, PaymentTarget::PayPlanCharge { charge_num, .. } if charge_num == c1)));
    let pay_num = store.record_payment_splits(&Payment { pat_num, pay_amount: 50.0, pay_date: today, ..Default::default() }, &allocs).expect("record_payment_splits");
    let saved_splits = store.list_pay_splits(pat_num).unwrap();
    assert!(saved_splits.iter().any(|s| s.pay_plan_charge_num == c1 && s.pay_plan_debit_type == PayPlanDebitType::Interest));

    let _ = conn.exec_drop("DELETE FROM paysplit WHERE PayNum=?", (pay_num,));
    let _ = conn.exec_drop("DELETE FROM payment WHERE PayNum=?", (pay_num,));
    let _ = conn.exec_drop("DELETE FROM payplancharge WHERE PayPlanChargeNum IN (?,?)", (c1, c2));
    let _ = conn.exec_drop("DELETE FROM payplan WHERE PayPlanNum=?", (plan_num,));
    println!("live charge-apply: plan {plan_num}, pay {pay_num}, charges {c1},{c2} cleaned up");
}

#[test]
#[ignore = "live MySQL: set ODCLINIC_DB_URL=mysql://user:pass@host:3306/db"]
fn mysql_live_pay_plan_breakdown_reports_unapplied_credit() {
    use od_domain::{PayPlan, PayPlanCharge, PayPlanChargeType, Payment, PaymentAllocation, PaymentTarget};
    let url = std::env::var("ODCLINIC_DB_URL").expect("ODCLINIC_DB_URL must be set");
    let store = MySqlStore::new(&url).expect("connect failed");
    let pat_num = store.list_patients().expect("list_patients failed").into_iter().next().expect("need patient").pat_num;
    let normalized = if url.contains("://") { url.trim().to_string() } else { format!("mysql://{}", url.trim()) };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("pool");
    let mut conn = pool.get_conn().expect("conn");
    let today = NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();

    let plan_num = store.upsert_pay_plan(&PayPlan { pat_num, guarantor: pat_num, pay_plan_date: today, note: "ODCLINIC_LIVE_BREAKDOWN".into(), ..Default::default() }).expect("plan");
    let c1 = store.upsert_pay_plan_charge(&PayPlanCharge { pay_plan_num: plan_num, pat_num, guarantor: pat_num, charge_date: today, principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() }).expect("c1");
    let c2 = store.upsert_pay_plan_charge(&PayPlanCharge { pay_plan_num: plan_num, pat_num, guarantor: pat_num, charge_date: today + chrono::Duration::days(30), principal: 50.0, charge_type: PayPlanChargeType::Debit, ..Default::default() }).expect("c2");
    let pay_num = store.record_payment_splits(&Payment { pat_num, pay_amount: 120.0, pay_date: today, ..Default::default() }, &[PaymentAllocation { unearned_type: 0,  target: PaymentTarget::PayPlan(plan_num), amount: 120.0, ..Default::default() }]).expect("pay");

    let plan = store.list_pay_plans(pat_num).unwrap().into_iter().find(|p| p.pay_plan_num == plan_num).unwrap();
    let b = od_core::pay_plan_balance_breakdown(&plan, &store.list_pay_plan_charges(pat_num).unwrap(), &store.list_pay_splits(pat_num).unwrap(), today + chrono::Duration::days(1));
    assert!((b.due_now - 0.0).abs() < 0.01);
    assert!((b.future_due - 30.0).abs() < 0.01);
    assert!((b.unapplied_credit - 0.0).abs() < 0.01);

    let _ = conn.exec_drop("DELETE FROM paysplit WHERE PayNum=?", (pay_num,));
    let _ = conn.exec_drop("DELETE FROM payment WHERE PayNum=?", (pay_num,));
    let _ = conn.exec_drop("DELETE FROM payplancharge WHERE PayPlanChargeNum IN (?,?)", (c1, c2));
    let _ = conn.exec_drop("DELETE FROM payplan WHERE PayPlanNum=?", (plan_num,));
    println!("live breakdown: plan {plan_num} cleaned up");
}

#[test]
#[ignore = "live MySQL: set ODCLINIC_DB_URL=mysql://user:pass@host:3306/db"]
fn mysql_live_resolve_unapplied_plan_credit_then_close_plan() {
    use od_domain::{PayPlan, PayPlanCharge, PayPlanChargeType, Payment, PaymentAllocation, PaymentTarget};
    let url = std::env::var("ODCLINIC_DB_URL").expect("ODCLINIC_DB_URL must be set");
    let store = MySqlStore::new(&url).expect("connect failed");
    let pat_num = store.list_patients().expect("list_patients failed").into_iter().next().expect("need patient").pat_num;
    let normalized = if url.contains("://") { url.trim().to_string() } else { format!("mysql://{}", url.trim()) };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("pool");
    let mut conn = pool.get_conn().expect("conn");
    let today = NaiveDate::from_ymd_opt(2099, 2, 1).unwrap();

    let plan_num = store.upsert_pay_plan(&PayPlan { pat_num, guarantor: pat_num, pay_plan_date: today, note: "ODCLINIC_LIVE_RESOLVE".into(), ..Default::default() }).expect("plan");
    let c1 = store.upsert_pay_plan_charge(&PayPlanCharge { pay_plan_num: plan_num, pat_num, guarantor: pat_num, charge_date: today, principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() }).expect("c1");
    let pay_num = store.record_payment_splits(&Payment { pat_num, pay_amount: 120.0, pay_date: today, ..Default::default() }, &[PaymentAllocation { unearned_type: 0, target: PaymentTarget::PayPlan(plan_num), amount: 120.0, ..Default::default() }]).expect("pay");

    let mut plan = store.list_pay_plans(pat_num).unwrap().into_iter().find(|p| p.pay_plan_num == plan_num).unwrap();
    let charges = store.list_pay_plan_charges(pat_num).unwrap();
    let splits = store.list_pay_splits(pat_num).unwrap();
    let b = od_core::pay_plan_balance_breakdown(&plan, &charges, &splits, today);
    let (tp, allocs) = od_core::build_unapplied_plan_credit_transfer(&plan, b.unapplied_credit, today, PaymentTarget::Unallocated).unwrap();
    let tp_num = store.record_payment_splits(&tp, &allocs).unwrap();
    let splits2 = store.list_pay_splits(pat_num).unwrap();
    assert_eq!(od_core::pay_plan_close_block_reason(&plan, &charges, &splits2, today), None);
    plan.is_closed = true;
    store.upsert_pay_plan(&plan).unwrap();

    let _ = conn.exec_drop("DELETE FROM paysplit WHERE PayNum IN (?,?)", (pay_num, tp_num));
    let _ = conn.exec_drop("DELETE FROM payment WHERE PayNum IN (?,?)", (pay_num, tp_num));
    let _ = conn.exec_drop("DELETE FROM payplancharge WHERE PayPlanChargeNum=?", (c1,));
    let _ = conn.exec_drop("DELETE FROM payplan WHERE PayPlanNum=?", (plan_num,));
    println!("live resolve: plan {plan_num} cleaned up");
}

#[test]
#[ignore = "live MySQL: set ODCLINIC_DB_URL=mysql://user:pass@host:3306/db"]
fn mysql_live_plan_negative_payment_refund_adjusts_balance() {
    use od_domain::{PayPlan, PayPlanCharge, PayPlanChargeType, Payment, PaymentAllocation, PaymentTarget};
    let url = std::env::var("ODCLINIC_DB_URL").expect("ODCLINIC_DB_URL must be set");
    let store = MySqlStore::new(&url).expect("connect failed");
    let pat_num = store.list_patients().expect("list_patients failed").into_iter().next().expect("need patient").pat_num;
    let normalized = if url.contains("://") { url.trim().to_string() } else { format!("mysql://{}", url.trim()) };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("pool");
    let mut conn = pool.get_conn().expect("conn");
    let today = NaiveDate::from_ymd_opt(2099, 3, 1).unwrap();

    let plan_num = store.upsert_pay_plan(&PayPlan { pat_num, guarantor: pat_num, pay_plan_date: today, note: "ODCLINIC_LIVE_REFUND".into(), ..Default::default() }).expect("plan");
    let c1 = store.upsert_pay_plan_charge(&PayPlanCharge { pay_plan_num: plan_num, pat_num, guarantor: pat_num, charge_date: today, principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() }).expect("c1");
    let p1 = store.record_payment_splits(&Payment { pat_num, pay_amount: 100.0, pay_date: today, ..Default::default() }, &[PaymentAllocation { unearned_type: 0, target: PaymentTarget::PayPlan(plan_num), amount: 100.0, ..Default::default() }]).expect("p1");
    let p2 = store.record_payment_splits(&Payment { pat_num, pay_amount: -20.0, pay_date: today, note: "Refund".into(), ..Default::default() }, &[PaymentAllocation { unearned_type: 0, target: PaymentTarget::PayPlan(plan_num), amount: -20.0, ..Default::default() }]).expect("p2");

    let plan = store.list_pay_plans(pat_num).unwrap().into_iter().find(|p| p.pay_plan_num == plan_num).unwrap();
    let b = od_core::pay_plan_balance_breakdown(&plan, &store.list_pay_plan_charges(pat_num).unwrap(), &store.list_pay_splits(pat_num).unwrap(), today);
    assert!((b.due_now - 20.0).abs() < 0.01);

    let _ = conn.exec_drop("DELETE FROM paysplit WHERE PayNum IN (?,?)", (p1, p2));
    let _ = conn.exec_drop("DELETE FROM payment WHERE PayNum IN (?,?)", (p1, p2));
    let _ = conn.exec_drop("DELETE FROM payplancharge WHERE PayPlanChargeNum=?", (c1,));
    let _ = conn.exec_drop("DELETE FROM payplan WHERE PayPlanNum=?", (plan_num,));
    println!("live refund: plan {plan_num} cleaned up");
}
#[test]
#[ignore = "live MySQL: set ODCLINIC_DB_URL=mysql://user:pass@host:3306/db"]
fn mysql_live_plan_unapplied_credit_transfer_to_procedure() {
    use od_domain::{PayPlan, PayPlanCharge, PayPlanChargeType, Payment, PaymentAllocation, PaymentTarget, Procedure, ProcedureStatus};
    let url = std::env::var("ODCLINIC_DB_URL").expect("ODCLINIC_DB_URL must be set");
    let store = MySqlStore::new(&url).expect("connect failed");
    let pat_num = store.list_patients().expect("list_patients failed").into_iter().next().expect("need patient").pat_num;
    let normalized = if url.contains("://") { url.trim().to_string() } else { format!("mysql://{}", url.trim()) };
    let opts = mysql::Opts::from_url(&normalized).expect("bad URL");
    let pool = mysql::Pool::new(opts).expect("pool");
    let mut conn = pool.get_conn().expect("conn");
    let today = NaiveDate::from_ymd_opt(2099, 4, 1).unwrap();

    let proc_num = store.insert_procedure(&Procedure { pat_num, proc_status: ProcedureStatus::Complete, fee: 50.0, proc_date: today, ..Default::default() }).expect("proc");
    let plan_num = store.upsert_pay_plan(&PayPlan { pat_num, guarantor: pat_num, pay_plan_date: today, note: "ODCLINIC_LIVE_XFER_PROC".into(), ..Default::default() }).expect("plan");
    let c1 = store.upsert_pay_plan_charge(&PayPlanCharge { pay_plan_num: plan_num, pat_num, guarantor: pat_num, charge_date: today, principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() }).expect("c1");
    let p1 = store.record_payment_splits(&Payment { pat_num, pay_amount: 120.0, pay_date: today, ..Default::default() }, &[PaymentAllocation { unearned_type: 0, target: PaymentTarget::PayPlan(plan_num), amount: 120.0, ..Default::default() }]).expect("p1");
    let plan = store.list_pay_plans(pat_num).unwrap().into_iter().find(|p| p.pay_plan_num == plan_num).unwrap();
    let b = od_core::pay_plan_balance_breakdown(&plan, &store.list_pay_plan_charges(pat_num).unwrap(), &store.list_pay_splits(pat_num).unwrap(), today);
    let (tp, allocs) = od_core::build_unapplied_plan_credit_transfer(&plan, b.unapplied_credit, today, PaymentTarget::Procedure(proc_num)).unwrap();
    let tp_num = store.record_payment_splits(&tp, &allocs).unwrap();

    let splits = store.list_pay_splits(pat_num).unwrap();
    assert!(splits.iter().any(|s| s.pay_num == tp_num && s.proc_num == proc_num && s.split_amount > 0.0));

    let _ = conn.exec_drop("DELETE FROM paysplit WHERE PayNum IN (?,?)", (p1, tp_num));
    let _ = conn.exec_drop("DELETE FROM payment WHERE PayNum IN (?,?)", (p1, tp_num));
    let _ = conn.exec_drop("DELETE FROM payplancharge WHERE PayPlanChargeNum=?", (c1,));
    let _ = conn.exec_drop("DELETE FROM payplan WHERE PayPlanNum=?", (plan_num,));
    let _ = conn.exec_drop("DELETE FROM procedurelog WHERE ProcNum=?", (proc_num,));
    println!("live xfer proc: plan {plan_num} cleaned up");
}
