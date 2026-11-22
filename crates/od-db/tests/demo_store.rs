//! Unit coverage for the in-memory backend, including the security methods
//! added with login/permission enforcement.

use chrono::{NaiveDate, NaiveDateTime};
use od_db::{Backend, DemoStore, Store};
use od_domain::definitions::{Definition, DefinitionCategory, DisplayField, DisplayFieldCategory};
use od_domain::eservices::*;
use od_domain::password;
use od_domain::*;

fn seeded() -> DemoStore {
    DemoStore::seeded()
}

fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(y, m, d)
        .unwrap()
        .and_hms_opt(h, min, 0)
        .unwrap()
}

#[test]
fn demo_store_seeds_all_reference_tables() {
    let store = seeded();
    assert_eq!(store.list_patients().unwrap().len(), 6);
    assert_eq!(store.list_providers().unwrap().len(), 3);
    assert_eq!(store.list_operatories().unwrap().len(), 3);
    assert_eq!(store.list_clinics().unwrap().len(), 2);
    assert_eq!(store.list_users().unwrap().len(), 2);
    assert!(!store.list_prefs().unwrap().is_empty());
    assert_eq!(store.list_group_permissions().unwrap().len(), 29);
    assert_eq!(store.list_user_group_attaches().unwrap().len(), 2);
    assert!(store.list_security_log(50).unwrap().is_empty());
    assert_eq!(store.list_adjustments(1).unwrap().len(), 1);
    assert_eq!(store.list_pay_splits(1).unwrap().len(), 1);
    assert_eq!(store.list_claims(3).unwrap().len(), 1);
    assert_eq!(store.list_claim_procs(3).unwrap().len(), 1);
    assert_eq!(store.list_statements(1).unwrap().len(), 1);
    // Recall slice: four seeded reminders and two ASAP-flagged appointments.
    assert_eq!(store.list_recalls().unwrap().len(), 4);
    assert_eq!(store.list_asap_appointments().unwrap().len(), 2);
    assert!(
        store
            .list_asap_appointments()
            .unwrap()
            .iter()
            .all(|a| a.priority == RecallPriority::Asap)
    );
    // Clinical slice: the waitlist is OD's unscheduled list.
    let waitlist = store.list_waitlist_appointments().unwrap();
    assert_eq!(waitlist.len(), 2);
    assert!(waitlist.iter().all(|a| a.apt_status == AppointmentStatus::UnschedList));
    assert!(waitlist.iter().all(|a| a.waitlist_status != 0));
    // eServices slice: three OD-style rules, one contact per patient and an
    // empty outbox/dedupe log.
    assert_eq!(store.list_reminder_rules().unwrap().len(), 3);
    assert_eq!(store.list_patient_contacts().unwrap().len(), 6);
    assert!(store.list_reminder_sent().unwrap().is_empty());
    assert!(store.list_outbox(50).unwrap().is_empty());
    // Treatment plan slice: one seeded line on the active plan.
    assert_eq!(store.list_treat_plan_lines(1).unwrap().len(), 1);
    // Fee schedules for the Procedure Codes report: the default schedule has
    // all four codes, the reduced one two, and no provider-specific fees exist.
    assert_eq!(store.list_fee_schedules().unwrap().len(), 2);
    assert_eq!(store.list_fees(53, 0).unwrap().len(), 4);
    assert_eq!(store.list_fees(54, 0).unwrap().len(), 2);
    assert!(store.list_fees(53, 1).unwrap().is_empty());
    // Prescriptions for the FormRpPrescriptions report: two drugs for one
    // patient and one for another.
    assert_eq!(store.list_prescriptions().unwrap().len(), 3);
    // Referral seed: three sources, one with two RefFrom patients, plus
    // RefTo/RefCustom rows that must never be reported by Referral Analysis.
    // The third source has no attachments so it only appears in the Referrals
    // field-selection report, where every one of the 19 columns is populated.
    let referrals = store.list_referrals().unwrap();
    let attaches = store.list_refattaches().unwrap();
    assert_eq!(referrals.len(), 3);
    assert_eq!(attaches.len(), 5);
    assert_eq!(attaches.iter().filter(|a| a.ref_type == 1).count(), 3);
    assert!(referrals.iter().any(|r| r.specialty_num == 17));
    assert!(referrals.iter().any(|r| r.not_person == 1 && r.is_hidden == 1));
    assert!(referrals.iter().any(|r| !r.m_name.is_empty() && !r.note.is_empty()));
    // Both demo procedures carry one unit, so production equals their fees.
    assert!(store
        .list_procedures(3)
        .unwrap()
        .iter()
        .all(|p| p.unit_qty == 1.0 && p.base_units == 0.0));
    // Definitions slice: hidden rows stay in the table, and every seeded
    // display-field grid has its OD default columns. Three ProcCodeCats
    // definitions support the Daily Procedures grouped layout, and one
    // Provider Specialties definition resolves the referral specialty column.
    assert_eq!(store.list_definitions().unwrap().len(), 17);
    assert_eq!(
        store
            .list_definitions()
            .unwrap()
            .iter()
            .filter(|d| d.category == DefinitionCategory::PROC_CODE_CATS)
            .count(),
        3
    );
    assert!(store.list_definitions().unwrap().iter().any(|d| d.is_hidden));
    assert_eq!(store.list_display_fields().unwrap().len(), 63);
}

#[test]
fn demo_store_definitions_and_display_fields_crud_round_trip() {
    let store = seeded();

    let added = Definition {
        category: DefinitionCategory::PAYMENT_TYPES,
        item_order: 99,
        item_name: "Mobile money".into(),
        item_value: "M".into(),
        item_color: -16776961,
        ..Definition::default()
    };
    let num = store.upsert_definition(&added).unwrap();
    let mut saved = added.clone();
    saved.def_num = num;
    saved.item_name = "Mobile wallet".into();
    saved.is_hidden = true;
    store.upsert_definition(&saved).unwrap();
    let found = store
        .list_definitions()
        .unwrap()
        .into_iter()
        .find(|d| d.def_num == num)
        .unwrap();
    assert_eq!(found.item_name, "Mobile wallet");
    assert_eq!(found.item_value, "M");
    assert_eq!(found.item_color, -16776961);
    assert!(found.is_hidden);
    store.delete_definition(num).unwrap();
    assert!(!store
        .list_definitions()
        .unwrap()
        .iter()
        .any(|d| d.def_num == num));

    let category = DisplayFieldCategory::APPOINTMENT_BUBBLE;
    let field = DisplayField {
        internal_name: "Custom column".into(),
        description: "Local label".into(),
        column_width: 120,
        category,
        item_order: 0,
        ..DisplayField::default()
    };
    store.replace_display_fields(category, &[field]).unwrap();
    let rows = store.list_display_fields().unwrap();
    let saved: Vec<DisplayField> = rows
        .into_iter()
        .filter(|f| f.category == category)
        .collect();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].display_label(), "Local label");

    // Saving the exact OD default empties the category again.
    store
        .replace_display_fields(category, &od_domain::definitions::default_display_fields(category))
        .unwrap();
    assert!(!store
        .list_display_fields()
        .unwrap()
        .iter()
        .any(|f| f.category == category));
}

#[test]
fn demo_store_treatment_plan_crud_and_snapshot_round_trip() {
    let store = seeded();
    let new = od_core::new_treat_plan(3, TreatPlanStatus::Inactive, "Phase 2");
    let num = store.upsert_treat_plan(&new).unwrap();
    let mut plan = new.clone();
    plan.treat_plan_num = num;

    let line = TreatPlanLine {
        proc_num: 77,
        proc_code: "D2740".into(),
        description: "Crown".into(),
        fee: 300.0,
        ..Default::default()
    };
    let line_num = store.upsert_treat_plan_line(&plan, &line).unwrap();
    assert!(line_num > 0);
    assert_eq!(store.list_treat_plan_lines(num).unwrap().len(), 1);

    // Promoting to Active renames the default heading.
    let before = store.list_treat_plans(3).unwrap();
    for changed in od_core::set_status(&before, num, TreatPlanStatus::Active) {
        store.upsert_treat_plan(&changed).unwrap();
    }
    let plans = store.list_treat_plans(3).unwrap();
    let active = plans.iter().find(|p| p.treat_plan_num == num).unwrap();
    assert_eq!(active.status, TreatPlanStatus::Active);
    assert_eq!(active.heading, "Active Treatment Plan");

    // Save TP creates a separate Saved plan with copied lines.
    let lines = store.list_treat_plan_lines(num).unwrap();
    let (saved, copied) = od_core::snapshot_plan(
        active,
        &lines,
        NaiveDate::from_ymd_opt(2026, 11, 21).unwrap(),
    );
    let saved_num = store.upsert_treat_plan(&saved).unwrap();
    let mut saved_plan = saved.clone();
    saved_plan.treat_plan_num = saved_num;
    for mut copied_line in copied {
        copied_line.treat_plan_num = saved_num;
        store
            .upsert_treat_plan_line(&saved_plan, &copied_line)
            .unwrap();
    }
    let saved_lines = store.list_treat_plan_lines(saved_num).unwrap();
    assert_eq!(saved_lines.len(), 1);
    assert_eq!(saved_lines[0].fee, 300.0);
    assert_eq!(saved_lines[0].proc_num, 77);
    assert!(store
        .list_treat_plans(3)
        .unwrap()
        .iter()
        .any(|p| p.treat_plan_num == saved_num && p.status == TreatPlanStatus::Saved));

    // Deleting clears lines and the plan, matching OD's delete guard.
    store.delete_treat_plan(saved_num).unwrap();
    assert!(store.list_treat_plan_lines(saved_num).unwrap().is_empty());
    store.delete_treat_plan(num).unwrap();
    assert!(!store
        .list_treat_plans(3)
        .unwrap()
        .iter()
        .any(|p| p.treat_plan_num == num));
}

#[test]
fn demo_store_recall_crud_round_trip() {
    let store = seeded();
    let due = NaiveDate::from_ymd_opt(2026, 12, 15).unwrap();
    let recall = Recall {
        pat_num: 5,
        date_due: Some(due),
        date_due_calc: Some(due),
        recall_interval: (6 << 16) | 12, // 6 months, 12 days in OD's packing
        recall_status: 7,
        note: "Demo recall round trip".into(),
        priority: RecallPriority::Asap,
        ..Default::default()
    };
    let num = store.upsert_recall(&recall).unwrap();
    assert!(num > 0);

    let saved = store
        .list_recalls()
        .unwrap()
        .into_iter()
        .find(|r| r.recall_num == num)
        .expect("new recall was not readable");
    assert_eq!(saved.pat_num, 5);
    assert_eq!(saved.date_due, Some(due));
    assert_eq!(saved.recall_interval, (6 << 16) | 12);
    assert_eq!(saved.recall_status, 7);
    assert_eq!(saved.priority, RecallPriority::Asap);

    let mut changed = saved.clone();
    changed.is_disabled = true;
    store.upsert_recall(&changed).unwrap();
    let disabled = store
        .list_recalls()
        .unwrap()
        .into_iter()
        .find(|r| r.recall_num == num)
        .unwrap();
    assert!(disabled.is_disabled);
}

#[test]
fn demo_store_patient_crud_and_search() {
    let store = seeded();
    let original_count = store.list_patients().unwrap().len();
    let mut patient = store.list_patients().unwrap().remove(0);
    let pat_num = patient.pat_num;

    patient.f_name = "Edited".into();
    assert_eq!(store.upsert_patient(&patient).unwrap(), pat_num);
    assert_eq!(
        store.get_patient(pat_num).unwrap().unwrap().f_name,
        "Edited"
    );

    let mut new_patient = Patient::default();
    new_patient.l_name = "Ztest".into();
    new_patient.f_name = "New".into();
    new_patient.gender = Gender::Unknown;
    let new_num = store.upsert_patient(&new_patient).unwrap();
    assert!(new_num > pat_num);
    assert_eq!(store.find_patients("ztest").unwrap().len(), 1);
    assert_eq!(store.list_patients().unwrap().len(), original_count + 1);

    store.delete_patient(new_num).unwrap();
    assert!(store.get_patient(new_num).unwrap().is_none());
    assert_eq!(store.list_patients().unwrap().len(), original_count);
}

#[test]
fn demo_store_appointment_crud_range_and_patient_cascade() {
    let store = seeded();
    let from = at(2099, 1, 1, 0, 0);
    let to = at(2099, 1, 2, 0, 0);
    assert!(store.list_appointments(from, to).unwrap().is_empty());

    let pat_num = store.list_patients().unwrap()[0].pat_num;
    let mut appointment = Appointment::default();
    appointment.pat_num = pat_num;
    appointment.apt_datetime = at(2099, 1, 1, 9, 0);
    appointment.apt_length = 30;
    appointment.pattern = "XXXXXX".into();
    appointment.apt_status = AppointmentStatus::Scheduled;
    appointment.operatory_num = 1;
    appointment.notes = "demo test".into();

    let apt_num = store.upsert_appointment(&appointment).unwrap();
    assert!(apt_num > 0);
    assert_eq!(store.list_appointments(from, to).unwrap().len(), 1);
    assert_eq!(
        store.get_appointment(apt_num).unwrap().unwrap().notes,
        "demo test"
    );

    appointment.apt_num = apt_num;
    appointment.notes = "edited".into();
    assert_eq!(store.upsert_appointment(&appointment).unwrap(), apt_num);
    assert_eq!(
        store.get_appointment(apt_num).unwrap().unwrap().notes,
        "edited"
    );

    // Deleting a patient also removes their appointments from the schedule.
    store.delete_patient(pat_num).unwrap();
    assert!(store.get_appointment(apt_num).unwrap().is_none());
    assert!(store.list_appointments(from, to).unwrap().is_empty());
}

#[test]
fn demo_store_credentials_and_group_permissions_resolve() {
    let store = seeded();
    let users = store.list_users().unwrap();
    let admin = users.iter().find(|u| u.user_name == "admin").unwrap();
    let clinical = users
        .iter()
        .find(|u| u.user_name == "aharrison")
        .unwrap();

    assert!(password::verify(&admin.password, "admin"));
    assert!(!password::verify(&admin.password, "wrong"));
    assert!(password::verify(&clinical.password, "harrison"));
    assert!(!password::verify(&clinical.password, "admin"));

    let attaches = store.list_user_group_attaches().unwrap();
    let grants = store.list_group_permissions().unwrap();
    let admin_perms = od_core::auth::resolve_permissions(admin, &attaches, &grants);
    let clinical_perms = od_core::auth::resolve_permissions(clinical, &attaches, &grants);

    assert!(admin_perms.contains(&Permission::SETUP));
    assert!(admin_perms.contains(&Permission::SECURITY_ADMIN));
    assert!(admin_perms.contains(&Permission::AUDIT_TRAIL));
    assert!(!clinical_perms.contains(&Permission::SETUP));
    assert!(!clinical_perms.contains(&Permission::SECURITY_ADMIN));
    assert!(clinical_perms.contains(&Permission::APPOINTMENTS_MODULE));
    assert!(clinical_perms.contains(&Permission::PATIENT_EDIT));
}

#[test]
fn demo_store_security_log_is_newest_first_and_limited() {
    let store = seeded();
    let older = SecurityEvent {
        security_log_num: 999,
        perm_type: Permission::USER_LOG_ON_OFF,
        user_num: 1,
        log_datetime: at(2026, 11, 20, 8, 0),
        log_text: "older".into(),
        pat_num: 0,
    };
    let newer = SecurityEvent {
        security_log_num: 999,
        perm_type: Permission::PATIENT_EDIT,
        user_num: 1,
        log_datetime: at(2026, 11, 21, 9, 0),
        log_text: "newer".into(),
        pat_num: 7,
    };

    store.append_security_log(&older).unwrap();
    store.append_security_log(&newer).unwrap();

    let all = store.list_security_log(50).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].log_text, "newer");
    assert_eq!(all[0].security_log_num, 2);
    assert_eq!(all[1].security_log_num, 1);

    let latest = store.list_security_log(1).unwrap();
    assert_eq!(latest.len(), 1);
    assert_eq!(latest[0].log_text, "newer");
}

#[test]
fn demo_store_records_adjustments_and_payments_with_splits() {
    let store = seeded();
    let adj_num = store
        .upsert_adjustment(&Adjustment {
            adj_date: NaiveDate::from_ymd_opt(2026, 11, 21).unwrap(),
            adj_amount: -12.5,
            pat_num: 2,
            note: "test discount".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(adj_num > 0);
    let adjustments = store.list_adjustments(2).unwrap();
    assert_eq!(adjustments.len(), 1);
    assert_eq!(adjustments[0].adj_amount, -12.5);

    let splits_before = store.list_pay_splits(2).unwrap().len();
    let pay_num = store
        .record_payment(
            &Payment {
                pat_num: 2,
                pay_date: NaiveDate::from_ymd_opt(2026, 11, 21).unwrap(),
                pay_amount: 40.0,
                ..Default::default()
            },
            7,
            0,
        )
        .unwrap();
    assert!(pay_num > 0);
    let splits = store.list_pay_splits(2).unwrap();
    assert_eq!(splits.len(), splits_before + 1);
    let new_split = splits.iter().find(|s| s.pay_num == pay_num).unwrap();
    assert_eq!(new_split.split_amount, 40.0);
    assert_eq!(new_split.proc_num, 7);
}

#[test]
fn demo_store_records_statements() {
    let store = seeded();
    let before = store.list_statements(3).unwrap().len();
    let sent = NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
    let num = store
        .insert_statement(&Statement {
            pat_num: 3,
            date_sent: Some(sent),
            note: "Balance statement".into(),
            is_invoice: false,
            ..Default::default()
        })
        .unwrap();
    assert!(num > 0);
    let statements = store.list_statements(3).unwrap();
    assert_eq!(statements.len(), before + 1);
    let stored = statements.iter().find(|s| s.statement_num == num).unwrap();
    assert_eq!(stored.date_sent, Some(sent));
    assert_eq!(stored.note, "Balance statement");
    assert!(!stored.is_invoice);
}

#[test]
fn demo_store_exposes_registry_clinical_records() {
    let store = seeded();
    let conditions = store.list_diseases(1).unwrap();
    assert_eq!(conditions.len(), 2);
    assert!(conditions.iter().any(|d| d.disease_name == "Hypertension"
        && d.status == od_domain::ProblemStatus::Active));
    let note = store.get_patient_note(1).unwrap().unwrap();
    assert!(note.medical.contains("Hypertension"));
    assert!(note.treatment.contains("16"));
    let treatments = store.list_registry_treatments(1).unwrap();
    assert!(!treatments.is_empty());
    assert!(treatments.iter().all(|t| !t.status_label().is_empty()));
    assert!(store.list_diseases(6).unwrap().is_empty());
}

#[test]
fn demo_store_issues_and_deactivates_clinic_cards_without_financial_rows() {
    use od_domain::ClinicCardReason;

    let store = seeded();
    let statements_before = store.list_statements(3).unwrap().len();
    assert!(store.list_clinic_cards(3).unwrap().is_empty());
    let card = od_domain::ClinicCard {
        pat_num: 3,
        card_number: "CC-TEST".into(),
        issued_at: NaiveDate::from_ymd_opt(2026, 11, 21).unwrap(),
        issued_by: 1,
        reason: ClinicCardReason::PaymentPlan,
        authorization: "PLAN-77".into(),
        ..Default::default()
    };
    let card_num = store.issue_clinic_card(&card).unwrap();
    let cards = store.list_clinic_cards(3).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].card_num, card_num);
    assert!(cards[0].is_active);
    assert_eq!(cards[0].reason, ClinicCardReason::PaymentPlan);

    // Issuing a card must never create financial detail.
    assert_eq!(store.list_statements(3).unwrap().len(), statements_before);

    store.deactivate_clinic_card(card_num).unwrap();
    let cards = store.list_clinic_cards(3).unwrap();
    assert_eq!(cards.len(), 1, "deactivation must not delete the card");
    assert!(!cards[0].is_active);
}

#[test]
fn demo_store_manages_payment_plans_and_charges() {
    use od_domain::{PayPlan, PayPlanCharge, PayPlanChargeType, PayPlanFrequency, PaymentSchedule};

    let store = seeded();
    assert_eq!(store.list_pay_plans(1).unwrap().len(), 1);
    assert_eq!(store.list_pay_plan_charges(1).unwrap().len(), 1);

    let plan_num = store
        .upsert_pay_plan(&PayPlan {
            pat_num: 5,
            guarantor: 5,
            pay_plan_date: NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            completed_amt: 300.0,
            number_of_payments: 3,
            pay_schedule: PaymentSchedule::Weekly,
            charge_frequency: PayPlanFrequency::Weekly,
            ..Default::default()
        })
        .unwrap();
    assert!(plan_num > 1);
    let plans = store.list_pay_plans(5).unwrap();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].pay_schedule, PaymentSchedule::Weekly);

    let charge_num = store
        .upsert_pay_plan_charge(&PayPlanCharge {
            pay_plan_num: plan_num,
            pat_num: 5,
            charge_date: NaiveDate::from_ymd_opt(2026, 11, 8).unwrap(),
            principal: 100.0,
            charge_type: PayPlanChargeType::Debit,
            ..Default::default()
        })
        .unwrap();
    assert!(charge_num > 1);
    let charges = store.list_pay_plan_charges(5).unwrap();
    assert_eq!(charges.len(), 1);
    assert_eq!(charges[0].principal, 100.0);

    // A payment applied to a plan must set paysplit.PayPlanNum.
    let pay_num = store
        .record_payment(
            &Payment {
                pat_num: 5,
                pay_date: NaiveDate::from_ymd_opt(2026, 11, 8).unwrap(),
                pay_amount: 100.0,
                ..Default::default()
            },
            0,
            plan_num,
        )
        .unwrap();
    let split = store
        .list_pay_splits(5)
        .unwrap()
        .into_iter()
        .find(|s| s.pay_num == pay_num)
        .unwrap();
    assert_eq!(split.pay_plan_num, plan_num);
}

#[test]
fn demo_store_persists_amortized_plan_terms_and_generated_charges() {
    let store = seeded();
    let start = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
    let interest_from = NaiveDate::from_ymd_opt(2026, 12, 1).unwrap();
    let plan = PayPlan {
        pat_num: 6,
        guarantor: 6,
        pay_plan_date: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
        date_pay_plan_start: Some(start),
        date_interest_start: Some(interest_from),
        completed_amt: 1000.0,
        pay_amt: 250.0,
        number_of_payments: 5,
        apr: 12.0,
        down_payment: 100.0,
        charge_frequency: PayPlanFrequency::Monthly,
        ..Default::default()
    };
    let plan_num = store.upsert_pay_plan(&plan).unwrap();

    let stored = store
        .list_pay_plans(6)
        .unwrap()
        .into_iter()
        .find(|p| p.pay_plan_num == plan_num)
        .unwrap();
    assert_eq!(stored.date_interest_start, Some(interest_from));
    assert_eq!(stored.apr, 12.0);
    assert_eq!(stored.down_payment, 100.0);

    let existing = store.list_pay_plan_charges(6).unwrap();
    assert!(
        od_core::charge_generation_block(&stored, &existing).is_none(),
        "a fresh plan has no debit yet"
    );
    let generated = od_core::generate_plan_charges(
        &stored,
        NaiveDate::from_ymd_opt(2026, 11, 20).unwrap(),
    );
    assert_eq!(generated.len(), 5); // down payment + four periodic charges
    assert_eq!(generated[0].principal, 100.0);
    assert_eq!(generated[0].charge_date, NaiveDate::from_ymd_opt(2026, 11, 20).unwrap());
    assert_eq!(generated[1].interest, 0.0); // November charge precedes interest start
    assert!(generated[2].interest > 0.0); // December onwards accrues 1% per month
    let principal: f64 = generated.iter().map(|c| c.principal).sum();
    assert_eq!(principal, 1000.0);

    for charge in &generated {
        store.upsert_pay_plan_charge(charge).unwrap();
    }
    let saved = store.list_pay_plan_charges(6).unwrap();
    assert_eq!(saved.len(), 5);
    assert!(
        od_core::charge_generation_block(&stored, &saved).is_some(),
        "a second generation must be refused"
    );
}

#[test]
fn demo_store_dynamic_plan_links_issue_due_charges() {
    let store = seeded();
    let plan = PayPlan {
        pat_num: 1,
        guarantor: 1,
        pay_plan_date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        date_pay_plan_start: Some(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
        apr: 0.0,
        pay_amt: 100.0,
        number_of_payments: 0,
        charge_frequency: PayPlanFrequency::Monthly,
        pay_schedule: PaymentSchedule::Monthly,
        is_dynamic: true,
        dynamic_pay_plan_tp_option: DynamicPayPlanTPOption::None,
        ..Default::default()
    };
    let plan_num = store.upsert_pay_plan(&plan).unwrap();
    let stored = store
        .list_pay_plans(1)
        .unwrap()
        .into_iter()
        .find(|p| p.pay_plan_num == plan_num)
        .unwrap();
    assert!(stored.is_dynamic);

    // Create a fresh adjustment as linked production to avoid seeded split noise.
    let adj_num = store
        .upsert_adjustment(&Adjustment {
            pat_num: 1,
            adj_date: NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(),
            adj_amount: 250.0,
            provider_num: 1,
            ..Default::default()
        })
        .unwrap();
    let link = PayPlanLink {
        pay_plan_num: plan_num,
        link_type: PayPlanLink::ADJUSTMENT,
        fkey: adj_num,
        ..Default::default()
    };
    let link_num = store.upsert_pay_plan_link(&link).unwrap();
    assert!(link_num > 0);

    let links = store.list_pay_plan_links().unwrap();
    let own_links: Vec<_> = links.into_iter().filter(|l| l.pay_plan_num == plan_num).collect();
    assert_eq!(own_links.len(), 1);

    let procs = store.list_procedures(1).unwrap();
    let adjs = store.list_adjustments(1).unwrap();
    let splits = store.list_pay_splits(1).unwrap();
    let existing = store.list_pay_plan_charges(1).unwrap();
    let due = od_core::issue_charges_due(
        &stored,
        &own_links,
        &procs,
        &adjs,
        &splits,
        &existing,
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
    );
    assert!(!due.is_empty(), "linked production should produce due charges");
    for c in &due {
        store.upsert_pay_plan_charge(c).unwrap();
    }
    let after = store.list_pay_plan_charges(1).unwrap();
    let again = od_core::issue_charges_due(
        &stored,
        &own_links,
        &procs,
        &adjs,
        &splits,
        &after,
        NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
    );
    assert!(again.is_empty(), "issuance must be idempotent");

    // Mid-period reconciliation: as of mid-Feb, Jan+Feb are due, Mar is not.
    let mid_feb = NaiveDate::from_ymd_opt(2026, 2, 15).unwrap();
    let rec = od_core::reconciliation(&stored, &own_links, &procs, &adjs, &splits, &after, mid_feb);
    assert!(rec.expected_due_count >= 2);
    assert!(rec.issued_count <= rec.expected_due_count);
    assert!(rec.expected_full_count >= rec.expected_due_count);

    store.delete_pay_plan_link(link_num).unwrap();
    let remaining = store.list_pay_plan_links().unwrap();
    assert!(!remaining.iter().any(|l| l.pay_plan_link_num == link_num));
}

#[test]
fn demo_store_records_one_payment_with_multiple_splits() {
    let store = seeded();
    let payment = Payment {
        pat_num: 1,
        pay_date: NaiveDate::from_ymd_opt(2026, 11, 21).unwrap(),
        pay_amount: 150.0,
        note: "ODCLINIC_MULTI_SPLIT".into(),
        ..Default::default()
    };
    let allocations = [
        PaymentAllocation { unearned_type: 0, 
            target: PaymentTarget::Procedure(7),
            amount: 60.0,
            ..Default::default()
        },
        PaymentAllocation { unearned_type: 0, 
            target: PaymentTarget::PayPlan(1),
            pat_num: 2,
            provider_num: 3,
            amount: 50.0,
        
                        pay_plan_debit_type: Default::default(),
                    },
        PaymentAllocation { unearned_type: 0, 
            target: PaymentTarget::Unallocated,
            amount: 40.0,
            ..Default::default()
        },
    ];
    let split_count = || -> usize {
        store.list_pay_splits(1).unwrap().len() + store.list_pay_splits(2).unwrap().len()
    };
    let before = split_count();
    let pay_num = store
        .record_payment_splits(&payment, &allocations)
        .unwrap();
    let splits: Vec<PaySplit> = [1u32, 2]
        .into_iter()
        .flat_map(|pat| store.list_pay_splits(pat).unwrap())
        .filter(|s| s.pay_num == pay_num)
        .collect();
    assert_eq!(splits.len(), 3);
    let proc = splits.iter().find(|s| s.proc_num == 7).unwrap();
    assert_eq!(proc.split_amount, 60.0);
    assert_eq!(proc.pay_plan_num, 0);
    let plan = splits.iter().find(|s| s.pay_plan_num == 1).unwrap();
    assert_eq!(plan.pat_num, 2);
    assert_eq!(plan.provider_num, 3);
    assert_eq!(plan.split_amount, 50.0);
    let unallocated = splits
        .iter()
        .find(|s| s.proc_num == 0 && s.pay_plan_num == 0)
        .unwrap();
    assert_eq!(unallocated.split_amount, 40.0);
    let total: f64 = splits.iter().map(|s| s.split_amount).sum();
    assert_eq!(total, 150.0);
    assert_eq!(split_count(), before + 3);

    // A mismatched total is rejected before anything is written.
    let bad = store.record_payment_splits(
        &payment,
        &[PaymentAllocation { unearned_type: 0, 
            amount: 100.0,
            ..Default::default()
        }],
    );
    assert!(bad.is_err());
    assert_eq!(split_count(), before + 3);
}

#[test]
fn backend_demo_dispatches_store_calls() {
    let backend = Backend::Demo(seeded());
    assert_eq!(backend.list_patients().unwrap().len(), 6);
    assert_eq!(backend.list_users().unwrap().len(), 2);
    assert!(backend.list_security_log(10).unwrap().is_empty());

    backend
        .append_security_log(&SecurityEvent {
            security_log_num: 0,
            perm_type: Permission::USER_LOG_ON_OFF,
            user_num: 1,
            log_datetime: at(2026, 11, 21, 10, 0),
            log_text: "backend dispatch".into(),
            pat_num: 0,
        })
        .unwrap();
    assert_eq!(backend.list_security_log(10).unwrap().len(), 1);

    let payment = Payment {
        pat_num: 1,
        pay_amount: 25.0,
        ..Default::default()
    };
    let pay_num = backend
        .record_payment_splits(
            &payment,
            &[PaymentAllocation { unearned_type: 0, 
                target: PaymentTarget::Procedure(7),
                amount: 25.0,
                ..Default::default()
            }],
        )
        .unwrap();
    assert!(pay_num > 0);

    let statement_num = backend
        .insert_statement(&Statement {
            pat_num: 1,
            date_sent: Some(NaiveDate::from_ymd_opt(2026, 11, 21).unwrap()),
            note: "backend dispatch".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(statement_num > 0);
}

#[test]
fn eservices_rules_and_contacts_round_trip() {
    let backend = Backend::Demo(seeded());
    let new_rule = ReminderRule {
        rule_num: 0,
        type_cur: ReminderType::Birthday,
        minutes_prior: 10_080,
        comm_type: CommType::Email,
        email_body: "Happy birthday {FirstName}!".into(),
        ..Default::default()
    };
    let rule_num = backend.upsert_reminder_rule(&new_rule).unwrap();
    assert!(rule_num > 0);
    let rules = backend.list_reminder_rules().unwrap();
    assert_eq!(rules.len(), 4);
    let stored = rules.iter().find(|r| r.rule_num == rule_num).unwrap();
    assert_eq!(stored.type_cur, ReminderType::Birthday);
    assert_eq!(stored.minutes_prior, 10_080);
    assert_eq!(stored.comm_type, CommType::Email);

    let contact = PatientContact {
        pat_num: 1,
        telegram_chat_id: "987654321".into(),
        email_override: "marta.work@example.test".into(),
        opt_out_email: false,
        opt_out_telegram: true,
    };
    backend.upsert_patient_contact(&contact).unwrap();
    let contacts = backend.list_patient_contacts().unwrap();
    let stored = contacts.iter().find(|c| c.pat_num == 1).unwrap();
    assert_eq!(stored.telegram_chat_id, "987654321");
    assert_eq!(stored.email_override, "marta.work@example.test");
    assert!(stored.opt_out_telegram);
}

#[test]
fn eservices_dedupe_log_and_outbox_status_round_trip() {
    let backend = Backend::Demo(seeded());
    backend.mark_reminder_sent(1, 2, 3).unwrap();
    backend.mark_reminder_sent(1, 2, 3).unwrap(); // idempotent
    let sent = backend.list_reminder_sent().unwrap();
    assert_eq!(sent, vec![(1, 2)]);

    let mut message = OutboxMessage {
        pat_num: 3,
        apt_num: 2,
        rule_num: 1,
        reminder_type: ReminderType::Reminder,
        transport: TransportKind::Smtp,
        recipient: "marta@example.test".into(),
        subject: "Reminder".into(),
        body: "See you soon".into(),
        status: OutboxStatus::Pending,
        created_at: at(2026, 11, 21, 9, 0),
        ..Default::default()
    };
    let num = backend.insert_outbox(&message).unwrap();
    assert!(num > 0);
    assert_eq!(backend.list_outbox(10).unwrap().len(), 1);

    message.message_num = num;
    backend
        .update_outbox(
            num,
            OutboxStatus::Sent,
            1,
            Some(at(2026, 11, 21, 9, 1)),
            "smtp accepted",
        )
        .unwrap();
    let stored = &backend.list_outbox(10).unwrap()[0];
    assert_eq!(stored.status, OutboxStatus::Sent);
    assert_eq!(stored.attempts, 1);
    assert_eq!(stored.last_error, "smtp accepted");
    assert!(stored.sent_at.is_some());
}

#[test]
fn demo_store_waitlist_removal_and_history_restore() {
    let backend = Backend::Demo(seeded());
    let waiting = backend.list_waitlist_appointments().unwrap();
    let removed = waiting[0].clone();

    // Model FormUnsched.Delete_Click: snapshot, delete, then restore.
    let mut change = AppointmentChange::from_snapshot(&removed, AppointmentChangeAction::Deleted);
    change.user_num = 1;
    let change_num = backend.append_appointment_change(&change).unwrap();
    assert!(change_num > 0);
    backend.delete_appointment(removed.apt_num).unwrap();

    assert_eq!(backend.list_waitlist_appointments().unwrap().len(), 1);
    let history = backend.list_appointment_changes(removed.apt_num).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].action, AppointmentChangeAction::Deleted);
    assert_eq!(history[0].snapshot.waitlist_status, removed.waitlist_status);

    let restored = od_core::restore_appointment(&history[0]);
    let restored_num = backend.upsert_appointment(&restored).unwrap();
    assert_eq!(restored_num, removed.apt_num);
    assert_eq!(backend.list_waitlist_appointments().unwrap().len(), 2);
}

#[test]
fn demo_store_patient_corrections_round_trip() {
    let backend = Backend::Demo(seeded());
    let before = backend.get_patient(1).unwrap().unwrap();
    let mut correction = PatientCorrection {
        pat_num: before.pat_num,
        reason: "migrated DOB was wrong".into(),
        before: before.clone(),
        ..Default::default()
    };
    correction.user_num = 1;
    let num = backend.append_patient_correction(&correction).unwrap();
    assert!(num > 0);

    let rows = backend.list_patient_corrections(before.pat_num).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].reason, "migrated DOB was wrong");
    assert_eq!(rows[0].before.f_name, before.f_name);

    let mut changed = before.clone();
    changed.birth_date = Some(NaiveDate::from_ymd_opt(1990, 2, 3).unwrap());
    backend.upsert_patient(&changed).unwrap();
    assert_eq!(
        backend.get_patient(before.pat_num).unwrap().unwrap().birth_date,
        changed.birth_date
    );
    // The stored snapshot still has the pre-correction value, so rollback is possible.
    assert_eq!(rows[0].before.birth_date, before.birth_date);
}

#[test]
fn demo_store_plan_payment_applies_to_due_charges() {
    use od_core::allocate_plan_payment;
    use od_domain::{PayPlan, PayPlanCharge, PayPlanChargeType, PayPlanDebitType, Payment, PaymentTarget};

    let store = seeded();
    let pat_num = 1u32;
    let today = chrono::Local::now().date_naive();
    let plan = PayPlan {
        pat_num,
        guarantor: pat_num,
        pay_plan_date: today,
        is_dynamic: false,
        note: "charge-apply".into(),
        ..Default::default()
    };
    let plan_num = store.upsert_pay_plan(&plan).unwrap();
    let c1 = store.upsert_pay_plan_charge(&PayPlanCharge {
        pay_plan_num: plan_num, pat_num, guarantor: pat_num,
        charge_date: today, principal: 100.0, interest: 10.0,
        charge_type: PayPlanChargeType::Debit, ..Default::default()
    }).unwrap();
    let _c2 = store.upsert_pay_plan_charge(&PayPlanCharge {
        pay_plan_num: plan_num, pat_num, guarantor: pat_num,
        charge_date: today + chrono::Duration::days(30), principal: 100.0, interest: 0.0,
        charge_type: PayPlanChargeType::Debit, ..Default::default()
    }).unwrap();

    let charges = store.list_pay_plan_charges(pat_num).unwrap().into_iter().filter(|c| c.pay_plan_num == plan_num).collect::<Vec<_>>();
    let splits = store.list_pay_splits(pat_num).unwrap();
    let allocs = allocate_plan_payment(plan_num, 50.0, &charges, &splits, today + chrono::Duration::days(1));
    assert!(allocs.iter().any(|a| matches!(a.target, PaymentTarget::PayPlanCharge { charge_num, .. } if charge_num == c1)));
    assert!(allocs.iter().any(|a| a.pay_plan_debit_type == PayPlanDebitType::Interest));

    let payment = Payment { pat_num, pay_amount: 50.0, pay_date: today, ..Default::default() };
    let pay_num = store.record_payment_splits(&payment, &allocs).unwrap();
    assert!(pay_num > 0);
    let saved_splits = store.list_pay_splits(pat_num).unwrap();
    assert!(saved_splits.iter().any(|s| s.pay_plan_charge_num == c1 && s.pay_plan_debit_type == PayPlanDebitType::Interest));
    let balance = od_core::pay_plan_balance(&PayPlan { pay_plan_num: plan_num, ..plan }, &store.list_pay_plan_charges(pat_num).unwrap(), &saved_splits, today + chrono::Duration::days(1));
    assert!((balance - 160.0).abs() < 0.01);
}

#[test]
fn demo_store_pay_plan_breakdown_reports_unapplied_credit() {
    use od_domain::{PayPlan, PayPlanCharge, PayPlanChargeType};
    let store = seeded();
    let today = chrono::Local::now().date_naive();
    let pat_num = 1u32;
    let plan_num = store.upsert_pay_plan(&PayPlan { pat_num, guarantor: pat_num, pay_plan_date: today, ..Default::default() }).unwrap();
    store.upsert_pay_plan_charge(&PayPlanCharge { pay_plan_num: plan_num, pat_num, guarantor: pat_num, charge_date: today, principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() }).unwrap();
    store.upsert_pay_plan_charge(&PayPlanCharge { pay_plan_num: plan_num, pat_num, guarantor: pat_num, charge_date: today + chrono::Duration::days(30), principal: 50.0, charge_type: PayPlanChargeType::Debit, ..Default::default() }).unwrap();
    store.record_payment_splits(&od_domain::Payment { pat_num, pay_amount: 120.0, pay_date: today, ..Default::default() }, &[od_domain::PaymentAllocation { unearned_type: 0,  target: od_domain::PaymentTarget::PayPlan(plan_num), amount: 120.0, ..Default::default() }]).unwrap();
    let plan = store.list_pay_plans(pat_num).unwrap().into_iter().find(|p| p.pay_plan_num == plan_num).unwrap();
    let b = od_core::pay_plan_balance_breakdown(&plan, &store.list_pay_plan_charges(pat_num).unwrap(), &store.list_pay_splits(pat_num).unwrap(), today + chrono::Duration::days(1));
    assert!((b.due_now - 0.0).abs() < 0.01);
    assert!((b.future_due - 30.0).abs() < 0.01);
    assert!((b.unapplied_credit - 0.0).abs() < 0.01);
}

#[test]
fn demo_store_resolve_unapplied_plan_credit_then_close_plan() {
    use od_domain::{PayPlan, PayPlanCharge, PayPlanChargeType, Payment, PaymentAllocation, PaymentTarget};
    let store = seeded();
    let today = chrono::Local::now().date_naive();
    let pat_num = 1u32;
    let plan_num = store.upsert_pay_plan(&PayPlan { pat_num, guarantor: pat_num, pay_plan_date: today, ..Default::default() }).unwrap();
    store.upsert_pay_plan_charge(&PayPlanCharge { pay_plan_num: plan_num, pat_num, guarantor: pat_num, charge_date: today, principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() }).unwrap();
    store.record_payment_splits(&Payment { pat_num, pay_amount: 120.0, pay_date: today, ..Default::default() }, &[PaymentAllocation { unearned_type: 0, target: PaymentTarget::PayPlan(plan_num), amount: 120.0, ..Default::default() }]).unwrap();
    let mut plan = store.list_pay_plans(pat_num).unwrap().into_iter().find(|p| p.pay_plan_num == plan_num).unwrap();
    let charges = store.list_pay_plan_charges(pat_num).unwrap();
    let splits = store.list_pay_splits(pat_num).unwrap();
    let b = od_core::pay_plan_balance_breakdown(&plan, &charges, &splits, today);
    assert!(b.unapplied_credit > 0.0);
    let (tp, allocs) = od_core::build_unapplied_plan_credit_transfer(&plan, b.unapplied_credit, today, PaymentTarget::Unallocated).unwrap();
    store.record_payment_splits(&tp, &allocs).unwrap();
    let splits2 = store.list_pay_splits(pat_num).unwrap();
    assert_eq!(od_core::pay_plan_close_block_reason(&plan, &charges, &splits2, today), None);
    plan.is_closed = true;
    store.upsert_pay_plan(&plan).unwrap();
    let closed = store.list_pay_plans(pat_num).unwrap().into_iter().find(|p| p.pay_plan_num == plan_num).unwrap();
    assert!(closed.is_closed);
}

#[test]
fn demo_store_plan_negative_payment_refund_adjusts_balance() {
    use od_domain::{PayPlan, PayPlanCharge, PayPlanChargeType, Payment, PaymentAllocation, PaymentTarget};
    let store = seeded();
    let today = chrono::Local::now().date_naive();
    let pat_num = 1u32;
    let plan_num = store.upsert_pay_plan(&PayPlan { pat_num, guarantor: pat_num, pay_plan_date: today, ..Default::default() }).unwrap();
    store.upsert_pay_plan_charge(&PayPlanCharge { pay_plan_num: plan_num, pat_num, guarantor: pat_num, charge_date: today, principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() }).unwrap();
    store.record_payment_splits(&Payment { pat_num, pay_amount: 100.0, pay_date: today, ..Default::default() }, &[PaymentAllocation { unearned_type: 0, target: PaymentTarget::PayPlan(plan_num), amount: 100.0, ..Default::default() }]).unwrap();
    store.record_payment_splits(&Payment { pat_num, pay_amount: -20.0, pay_date: today, note: "Refund".into(), ..Default::default() }, &[PaymentAllocation { unearned_type: 0, target: PaymentTarget::PayPlan(plan_num), amount: -20.0, ..Default::default() }]).unwrap();
    let plan = store.list_pay_plans(pat_num).unwrap().into_iter().find(|p| p.pay_plan_num == plan_num).unwrap();
    let b = od_core::pay_plan_balance_breakdown(&plan, &store.list_pay_plan_charges(pat_num).unwrap(), &store.list_pay_splits(pat_num).unwrap(), today);
    assert!((b.due_now - 20.0).abs() < 0.01);
    assert!((b.unapplied_credit - 0.0).abs() < 0.01);
}
#[test]
fn demo_store_plan_unapplied_credit_transfer_to_procedure() {
    use od_domain::{PayPlan, PayPlanCharge, PayPlanChargeType, Payment, PaymentAllocation, PaymentTarget, Procedure, ProcedureStatus};
    let store = seeded();
    let today = chrono::Local::now().date_naive();
    let pat_num = 1u32;
    let proc_num = store.insert_procedure(&Procedure { pat_num, proc_status: ProcedureStatus::Complete, fee: 50.0, proc_date: today, ..Default::default() }).unwrap();
    let plan_num = store.upsert_pay_plan(&PayPlan { pat_num, guarantor: pat_num, pay_plan_date: today, ..Default::default() }).unwrap();
    store.upsert_pay_plan_charge(&PayPlanCharge { pay_plan_num: plan_num, pat_num, guarantor: pat_num, charge_date: today, principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() }).unwrap();
    store.record_payment_splits(&Payment { pat_num, pay_amount: 120.0, pay_date: today, ..Default::default() }, &[PaymentAllocation { unearned_type: 0, target: PaymentTarget::PayPlan(plan_num), amount: 120.0, ..Default::default() }]).unwrap();
    let plan = store.list_pay_plans(pat_num).unwrap().into_iter().find(|p| p.pay_plan_num == plan_num).unwrap();
    let b = od_core::pay_plan_balance_breakdown(&plan, &store.list_pay_plan_charges(pat_num).unwrap(), &store.list_pay_splits(pat_num).unwrap(), today);
    let (tp, allocs) = od_core::build_unapplied_plan_credit_transfer(&plan, b.unapplied_credit, today, PaymentTarget::Procedure(proc_num)).unwrap();
    store.record_payment_splits(&tp, &allocs).unwrap();
    let splits = store.list_pay_splits(pat_num).unwrap();
    assert!(splits.iter().any(|s| s.pay_plan_num == plan_num && s.split_amount < 0.0));
    assert!(splits.iter().any(|s| s.proc_num == proc_num && s.split_amount > 0.0));
}

#[test]
fn demo_store_transfer_to_procedure_visible_in_charge_application_report() {
    use od_domain::{PayPlan, PayPlanCharge, PayPlanChargeType, Payment, PaymentAllocation, PaymentTarget, Procedure, ProcedureStatus};
    let store = seeded();
    let today = chrono::Local::now().date_naive();
    let pat_num = 1u32;
    let proc_num = store.insert_procedure(&Procedure { pat_num, proc_status: ProcedureStatus::Complete, fee: 50.0, proc_date: today, ..Default::default() }).unwrap();
    let plan_num = store.upsert_pay_plan(&PayPlan { pat_num, guarantor: pat_num, pay_plan_date: today, ..Default::default() }).unwrap();
    store.upsert_pay_plan_charge(&PayPlanCharge { pay_plan_num: plan_num, pat_num, guarantor: pat_num, charge_date: today, principal: 100.0, charge_type: PayPlanChargeType::Debit, ..Default::default() }).unwrap();
    store.record_payment_splits(&Payment { pat_num, pay_amount: 120.0, pay_date: today, ..Default::default() }, &[PaymentAllocation { unearned_type: 0, target: PaymentTarget::PayPlan(plan_num), amount: 120.0, ..Default::default() }]).unwrap();
    let plan = store.list_pay_plans(pat_num).unwrap().into_iter().find(|p| p.pay_plan_num == plan_num).unwrap();
    let b = od_core::pay_plan_balance_breakdown(&plan, &store.list_pay_plan_charges(pat_num).unwrap(), &store.list_pay_splits(pat_num).unwrap(), today);
    let (tp, allocs) = od_core::build_unapplied_plan_credit_transfer(&plan, b.unapplied_credit, today, PaymentTarget::Procedure(proc_num)).unwrap();
    store.record_payment_splits(&tp, &allocs).unwrap();

    let patients = store.list_patients().unwrap();
    let plans = store.list_pay_plans(pat_num).unwrap();
    let charges = store.list_pay_plan_charges(pat_num).unwrap();
    let splits = store.list_pay_splits(pat_num).unwrap();
    let payments = store.list_payments(pat_num).unwrap();
    let report = od_core::pay_plan_charge_applications_report(&plans, &charges, &splits, &patients, &payments, today, today, true);
    assert!(report.rows.iter().any(|r| r[5] == "Transfer/Resolve" && r[9].contains(&format!("procedure #{proc_num}"))));
}
