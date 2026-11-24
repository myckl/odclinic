//! In-memory backend seeded with sample practice data.

use std::sync::Mutex;

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use od_domain::definitions::{
    default_display_fields, Definition, DisplayField, DisplayFieldCategory,
};
use od_domain::eservices::*;
use od_domain::*;

use crate::Store;

#[derive(Default)]
struct Data {
    patients: Vec<Patient>,
    appointments: Vec<Appointment>,
    /// OD `histappointment` snapshots. Kept here so restoring a removed waitlist
    /// entry works without a database.
    appointment_changes: Vec<AppointmentChange>,
    /// Port-local patient correction snapshots (OD has no patient history table).
    patient_corrections: Vec<PatientCorrection>,
    recalls: Vec<Recall>,
    providers: Vec<Provider>,
    operatories: Vec<Operatory>,
    clinics: Vec<Clinic>,
    procedures: Vec<Procedure>,
    /// OD `procedurelog.DiscountPlanAmt`, keyed by `ProcNum`. Kept beside the
    /// procedure rows so the domain `Procedure` stays a report-facing type.
    procedure_discounts: std::collections::HashMap<Id, f64>,
    /// OD `procedurecode` rows, needed to resolve CDT codes by their code text.
    procedure_codes: Vec<ProcedureCode>,
    /// OD `feesched` rows for the Procedure Codes report.
    fee_schedules: Vec<FeeSchedule>,
    /// OD `discountplan` rows used by pricing and the Discount Plan report.
    discount_plans: Vec<DiscountPlan>,
    /// OD `discountplansub` rows — one patient's plan subscription per row.
    discount_plan_subs: Vec<DiscountPlanSub>,
    /// OD `fee` rows for the Procedure Codes report.
    fees: Vec<Fee>,
    /// OD `rxpat` rows for the Prescriptions report.
    prescriptions: Vec<Prescription>,
    /// OD `referral` rows for the Referral Analysis report.
    referrals: Vec<Referral>,
    /// OD `refattach` rows linking referrals to patients.
    refattaches: Vec<RefAttach>,
    /// OD `procnote` rows for the Procedure Notes report.
    proc_notes: Vec<ProcNote>,
    /// OD chart-of-accounts rows for the accounting reports.
    accounts: Vec<Account>,
    /// OD `journalentry` rows for the accounting reports.
    journal_entries: Vec<JournalEntry>,
    /// OD `schedule`/`scheduleop` rows for the production-goal report.
    schedules: Vec<Schedule>,
    schedule_ops: Vec<ScheduleOp>,
    /// OD `claimsnapshot` rows for historic payroll write-offs.
    claim_snapshots: Vec<ClaimSnapshot>,
    /// OD `payplanlink` rows for the overcharged-plan report.
    pay_plan_links: Vec<PayPlanLink>,
    /// OD `carrier` rows for the laser-label insurance tab.
    carriers: Vec<Carrier>,
    /// OD `disease` rows with the `diseasedef` name resolved.
    diseases: Vec<Disease>,
    /// OD `patientnote` free-text medical/treatment history.
    patient_notes: Vec<PatientNote>,
    /// Port-local clinic cards; issuing one never writes financial rows.
    clinic_cards: Vec<ClinicCard>,
    payments: Vec<Payment>,
    adjustments: Vec<Adjustment>,
    pay_splits: Vec<PaySplit>,
    claims: Vec<Claim>,
    claim_procs: Vec<ClaimProc>,
    statements: Vec<Statement>,
    pay_plans: Vec<PayPlan>,
    pay_plan_charges: Vec<PayPlanCharge>,
    treat_plans: Vec<TreatPlan>,
    treat_plan_lines: Vec<TreatPlanLine>,
    definitions: Vec<Definition>,
    display_fields: Vec<DisplayField>,
    users: Vec<Userod>,
    prefs: Vec<Pref>,
    group_permissions: Vec<GroupPermission>,
    group_attaches: Vec<UserGroupAttach>,
    security_log: Vec<SecurityEvent>,
    reminder_rules: Vec<ReminderRule>,
    patient_contacts: Vec<PatientContact>,
    reminder_sent: Vec<(Id, Id, Id)>,
    outbox: Vec<OutboxMessage>,
}

pub struct DemoStore {
    data: Mutex<Data>,
}

impl DemoStore {
    pub fn seeded() -> Self {
        let today = Local::now().date_naive();
        Self {
            data: Mutex::new(seed(today)),
        }
    }
}

fn at(day: NaiveDate, hour: u32, minute: u32) -> NaiveDateTime {
    day.and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
}

/// OD stores `Interval` as 8 bits each for days, weeks, months and years.
fn interval_months(months: u32) -> i32 {
    (((months.min(255)) << 16) & 0x00FF_0000) as i32
}

/// Demo definitions and display fields use the same OD category codes and
/// column values the MySQL backend writes. One retired payment type is hidden
/// so the Hide toggle has something meaningful to show.
fn seed_definitions() -> (Vec<Definition>, Vec<DisplayField>) {
    use od_domain::definitions::DefinitionCategory as Cat;

    let rows = [
        (1, Cat::ADJ_TYPES, 0, "Courtesy adjustment", "-", 0, false),
        (2, Cat::ADJ_TYPES, 1, "Late fee", "+", 0, false),
        (3, Cat::BILLING_TYPES, 0, "Statement", "E", 0, false),
        (4, Cat::BILLING_TYPES, 1, "Collections", "C", 0, false),
        (5, Cat::PAYMENT_TYPES, 0, "Cash", "", 0, false),
        (6, Cat::PAYMENT_TYPES, 1, "Card", "", 0, false),
        (7, Cat::PAYMENT_TYPES, 2, "Bank transfer", "", 0, false),
        (8, Cat::PAYMENT_TYPES, 3, "Retired card type", "", 0, true),
        (9, Cat::RECALL_UNSCHED_STATUS, 0, "Due", "Due", 0, false),
        (10, Cat::RECALL_UNSCHED_STATUS, 1, "Called", "Call", 0, false),
        (11, Cat::APPT_CONFIRMED, 0, "Confirmed", "C", -16776961, false),
        (12, Cat::TX_PRIORITIES, 0, "Normal", "", 0, false),
        (13, Cat::TX_PRIORITIES, 1, "Urgent", "", -65536, false),
        (14, Cat::PROC_CODE_CATS, 0, "Diagnostic", "", 0, false),
        (15, Cat::PROC_CODE_CATS, 1, "Restorative", "", 0, false),
        (16, Cat::PROC_CODE_CATS, 2, "Miscellaneous", "", 0, false),
        (17, Cat::PROVIDER_SPECIALTIES, 0, "General Dentistry", "", 0, false),
    ];
    let definitions = rows
        .into_iter()
        .map(|(num, category, order, name, value, color, hidden)| Definition {
            def_num: num,
            category,
            item_order: order,
            item_name: name.into(),
            item_value: value.into(),
            item_color: color,
            is_hidden: hidden,
        })
        .collect();

    let mut display_fields = Vec::new();
    let mut num = 1;
    for category in DisplayFieldCategory::SEEDED {
        for mut field in default_display_fields(category) {
            field.display_field_num = num;
            num += 1;
            display_fields.push(field);
        }
    }
    (definitions, display_fields)
}

fn seed(today: NaiveDate) -> Data {
    let clinics = vec![
        Clinic {
            clinic_num: 1,
            description: "Main Office".into(),
            abbr: "MAIN".into(),
            is_hidden: false,
        },
        Clinic {
            clinic_num: 2,
            description: "North Branch".into(),
            abbr: "NORTH".into(),
            is_hidden: false,
        },
    ];

    let providers = vec![
        Provider {
            provider_num: 1,
            l_name: "Harrison".into(),
            f_name: "Alice".into(),
            abbreviation: "AH".into(),
            is_hidden: false,
            hourly_prod_goal_amt: 800.0,
        },
        Provider {
            provider_num: 2,
            l_name: "Nguyen".into(),
            f_name: "David".into(),
            abbreviation: "DN".into(),
            is_hidden: false,
            hourly_prod_goal_amt: 750.0,
        },
        Provider {
            provider_num: 3,
            l_name: "Okafor".into(),
            f_name: "Grace".into(),
            abbreviation: "GO".into(),
            is_hidden: false,
            hourly_prod_goal_amt: 900.0,
        },
    ];

    let operatories = vec![
        Operatory {
            operatory_num: 1,
            description: "Operatory 1".into(),
            abbrev: "OP1".into(),
            clinic_num: 1,
            is_hidden: false,
        },
        Operatory {
            operatory_num: 2,
            description: "Operatory 2".into(),
            abbrev: "OP2".into(),
            clinic_num: 1,
            is_hidden: false,
        },
        Operatory {
            operatory_num: 3,
            description: "Hygiene".into(),
            abbrev: "HYG".into(),
            clinic_num: 1,
            is_hidden: false,
        },
    ];

    let people = [
        (1u32, "Alvarez", "Marta", Gender::Female, 11, 3, 1979, 3, 12),
        (2, "Bennett", "James", Gender::Male, 4, 22, 1968, 7, 18),
        (3, "Chen", "Lily", Gender::Female, 18, 45, 1995, 2, 9),
        (4, "Diaz", "Robert", Gender::Male, 6, 30, 1984, 9, 27),
        (5, "Evans", "Sofia", Gender::Female, 23, 15, 2001, 12, 5),
        (6, "Foster", "Norman", Gender::Male, 14, 5, 1957, 5, 21),
    ];
    let mut patients: Vec<Patient> = people
        .iter()
        .map(|(num, last, first, gender, _day, _hour, year, month, bday)| Patient {
            pat_num: *num,
            l_name: (*last).into(),
            f_name: (*first).into(),
            middle_i: String::new(),
            title: String::new(),
            suffix: String::new(),
            birth_date: NaiveDate::from_ymd_opt(*year, *month, *bday),
            gender: *gender,
            home_phone: format!("(503) 555-01{:02}", num),
            wireless_phone: String::new(),
            email: format!("{}@example.test", first.to_lowercase()),
            address: format!("{} Elm St", 100 + num),
            city: "Salem".into(),
            state: "OR".into(),
            zip: "97301".into(),
            clinic_num: 1,
            primary_provider_num: (*num % 3) + 1,
            date_created: Some(at(today - Duration::days(400 + *num as i64), 9, 0)),
            ..Default::default()
        })
        .collect();

    let apt_rows = [
        (1u32, 1u32, 0i64, 9u32, 0u32, 60u32, AppointmentStatus::Scheduled, 1u32, 1u32, false),
        (2, 2, 0, 10, 30, 30, AppointmentStatus::Scheduled, 2, 2, false),
        (3, 3, 0, 13, 0, 60, AppointmentStatus::Complete, 1, 3, false),
        (4, 4, 0, 15, 0, 45, AppointmentStatus::Scheduled, 3, 1, false),
        (5, 5, 1, 8, 30, 60, AppointmentStatus::Scheduled, 2, 2, true),
        (6, 6, 1, 11, 0, 90, AppointmentStatus::Broken, 1, 3, false),
        (7, 1, 2, 9, 30, 30, AppointmentStatus::Scheduled, 3, 1, false),
        (8, 2, 3, 14, 0, 60, AppointmentStatus::Planned, 2, 2, true),
        (9, 3, 4, 10, 0, 60, AppointmentStatus::Scheduled, 1, 3, false),
        // OD's unscheduled list is this port's waitlist. These two rows give
        // the new tab real data without putting them on the schedule grid.
        (10, 4, 5, 9, 0, 30, AppointmentStatus::UnschedList, 1, 1, false),
        (11, 6, 6, 11, 30, 60, AppointmentStatus::UnschedList, 2, 3, false),
    ];
    let appointments: Vec<Appointment> = apt_rows
        .iter()
        .map(|(num, pat, day, hour, minute, len, status, op, prov, asap)| Appointment {
            apt_num: *num,
            pat_num: *pat,
            apt_datetime: at(today + Duration::days(*day), *hour, *minute),
            apt_length: *len,
            apt_status: *status,
            operatory_num: *op,
            provider_num: *prov,
            clinic_num: 1,
            notes: String::new(),
            pattern: "X".repeat((*len / 5) as usize),
            is_time_locked: false,
            priority: if *asap {
                RecallPriority::Asap
            } else {
                RecallPriority::Normal
            },
            // Definition 9 = "Due", 10 = "Called" in DefCat.RecallUnschedStatus.
            waitlist_status: if *status == AppointmentStatus::UnschedList {
                if *num % 2 == 0 { 10 } else { 9 }
            } else {
                0
            },
        })
        .collect();

    // Recalls are seeded around today so the list shows every due state.
    let six_months = interval_months(6);
    let recalls = vec![
        Recall {
            recall_num: 1,
            pat_num: 1,
            date_due: Some(today - Duration::days(12)),
            date_due_calc: Some(today - Duration::days(12)),
            date_previous: Some(today - Duration::days(200)),
            recall_interval: six_months,
            note: "Overdue hygiene".into(),
            ..Default::default()
        },
        Recall {
            recall_num: 2,
            pat_num: 2,
            date_due: Some(today),
            date_due_calc: Some(today),
            recall_interval: six_months,
            note: "Recall exam".into(),
            ..Default::default()
        },
        Recall {
            recall_num: 3,
            pat_num: 3,
            date_due: Some(today + Duration::days(9)),
            date_due_calc: Some(today + Duration::days(9)),
            recall_interval: six_months,
            note: "Fresh recall".into(),
            ..Default::default()
        },
        Recall {
            recall_num: 4,
            pat_num: 4,
            date_due: Some(today - Duration::days(30)),
            is_disabled: true,
            note: "Paused while treatment plan is active".into(),
            ..Default::default()
        },
    ];

    // OD-style reminder rules. Ticks are 100 ns in OD; the domain type stores
    // minutes, so a one-day lead is 1440 here (OD would use 864_000_000_000).
    let reminder_rules = vec![
        ReminderRule {
            rule_num: 1,
            type_cur: ReminderType::Reminder,
            minutes_prior: 1440,
            do_not_send_within_minutes: 60,
            send_order: "0,1,2".into(),
            is_enabled: true,
            comm_type: CommType::Preferred,
            sms_template: "Hi {FirstName}, reminder: {DateTime} at {Clinic}. Reply to confirm.".into(),
            email_subject: "Appointment reminder — {Date}".into(),
            email_body: "Hello {FirstName} {LastName},\n\nThis is a reminder for your appointment on {DateTime} with {Provider} at {Clinic}.\n\nPlease reply if you need to change it.".into(),
        },
        ReminderRule {
            rule_num: 2,
            type_cur: ReminderType::ConfirmationFutureDay,
            minutes_prior: 4320,
            do_not_send_within_minutes: 1440,
            send_order: "0,1,2".into(),
            is_enabled: true,
            comm_type: CommType::Email,
            sms_template: "Please confirm {DateTime} at {Clinic}.".into(),
            email_subject: "Please confirm your appointment".into(),
            email_body: "Hello {FirstName},\n\nPlease confirm your {DateTime} appointment at {Clinic}.".into(),
        },
        ReminderRule {
            rule_num: 3,
            type_cur: ReminderType::Arrival,
            minutes_prior: 0,
            do_not_send_within_minutes: 0,
            send_order: "0,1,2".into(),
            is_enabled: false,
            comm_type: CommType::Text,
            sms_template: "Welcome to {Clinic}, {FirstName}. Your appointment is at {Time}.".into(),
            email_subject: "Arrival".into(),
            email_body: String::new(),
        },
    ];
    let patient_contacts: Vec<PatientContact> = people
        .iter()
        .map(|(num, ..)| PatientContact {
            pat_num: *num,
            telegram_chat_id: format!("100000000{num}"),
            email_override: String::new(),
            opt_out_email: false,
            opt_out_telegram: false,
        })
        .collect();
    let reminder_sent: Vec<(Id, Id, Id)> = Vec::new();
    let outbox: Vec<OutboxMessage> = Vec::new();

    let procedures = vec![
        Procedure {
            proc_num: 1,
            pat_num: 3,
            code_num: 1,
            proc_date: today - Duration::days(1),
            proc_status: ProcedureStatus::Complete,
            tooth_num: "14".into(),
            provider_num: 3,
            fee: 185.0,
            unit_qty: 1.0,
            base_units: 0.0,
            description_override: String::new(),
            ..Default::default()
        },
        Procedure {
            proc_num: 2,
            pat_num: 1,
            code_num: 2,
            proc_date: today - Duration::days(8),
            proc_status: ProcedureStatus::Complete,
            tooth_num: "30".into(),
            provider_num: 1,
            fee: 120.0,
            unit_qty: 1.0,
            base_units: 0.0,
            description_override: String::new(),
            ..Default::default()
        },
    ];

    // CDT codes used by reports that classify services by code text. D9986 and
    // D9987 are the ADA broken-appointment (missed / cancelled) codes.
    let procedure_codes = vec![
        ProcedureCode {
            code_num: 1,
            code: "D1110".into(),
            code_name: "Prophylaxis - adult".into(),
            abbreviation: "Adult prophy".into(),
            fee: 95.0,
            category_num: 14,
        },
        ProcedureCode {
            code_num: 2,
            code: "D2140".into(),
            code_name: "Amalgam - one surface".into(),
            abbreviation: "Amalg 1s".into(),
            fee: 120.0,
            category_num: 15,
        },
        ProcedureCode {
            code_num: 3,
            code: "D9986".into(),
            code_name: "Missed appointment".into(),
            abbreviation: "Missed appt".into(),
            fee: 0.0,
            category_num: 16,
        },
        ProcedureCode {
            code_num: 4,
            code: "D9987".into(),
            code_name: "Cancelled appointment".into(),
            abbreviation: "Cancelled appt".into(),
            fee: 0.0,
            category_num: 16,
        },
    ];

    // Two fee schedules. The default schedule carries a fee for every code;
    // the reduced schedule covers two codes so "Show blank fees" has a purpose.
    let fee_schedules = vec![
        FeeSchedule {
            fee_sched_num: 53,
            description: "General Fees".into(),
        },
        FeeSchedule {
            fee_sched_num: 54,
            description: "Reduced Fees".into(),
        },
    ];
    let fees = vec![
        Fee { fee_num: 1, code_num: 1, schedule_num: 53, amount: 95.0 },
        Fee { fee_num: 2, code_num: 2, schedule_num: 53, amount: 120.0 },
        Fee { fee_num: 3, code_num: 3, schedule_num: 53, amount: 0.0 },
        Fee { fee_num: 4, code_num: 4, schedule_num: 53, amount: 0.0 },
        Fee { fee_num: 5, code_num: 1, schedule_num: 54, amount: 75.0 },
        Fee { fee_num: 6, code_num: 2, schedule_num: 54, amount: 90.0 },
    ];

    // Patient-facing discount plans: the insurance-free alternative that
    // prices completed procedures from a reduced fee schedule and books the
    // difference as an adjustment of the plan's adjustment type.
    let discount_plans = vec![
        DiscountPlan {
            discount_plan_num: 1,
            description: "Community Smile Plan".into(),
            fee_sched_num: 54,
            def_num: 1,
            is_hidden: false,
            note: "Reduced fees for registered community members".into(),
            annual_max: 500.0,
            ..Default::default()
        },
        DiscountPlan {
            discount_plan_num: 2,
            description: "Senior Discount".into(),
            fee_sched_num: 54,
            def_num: 1,
            is_hidden: true,
            note: "Hidden legacy plan; kept for historic adjustments".into(),
            annual_max: -1.0,
            ..Default::default()
        },
    ];
    let discount_plan_subs = vec![
        DiscountPlanSub {
            discount_sub_num: 1,
            discount_plan_num: 1,
            pat_num: 1,
            date_effective: Some(today - Duration::days(180)),
            date_term: None,
            note: "Open ended".into(),
        },
        DiscountPlanSub {
            discount_sub_num: 2,
            discount_plan_num: 2,
            pat_num: 3,
            date_effective: Some(today - Duration::days(60)),
            date_term: Some(today + Duration::days(305)),
            note: "One-year term".into(),
        },
    ];

    // Prescriptions for the FormRpPrescriptions report. Two drugs for one
    // patient and one for another so both the Patient and Drug prefix filters
    // have meaningful results.
    let prescriptions = vec![
        Prescription {
            rx_num: 1,
            pat_num: 1,
            rx_date: today - Duration::days(20),
            drug: "Amoxicillin 500mg".into(),
            sig: "Take one capsule three times daily for 7 days".into(),
            disp: "21".into(),
            prov_num: 1,
        },
        Prescription {
            rx_num: 2,
            pat_num: 1,
            rx_date: today - Duration::days(5),
            drug: "Ibuprofen 400mg".into(),
            sig: "Take one tablet every 8 hours as needed for pain".into(),
            disp: "30".into(),
            prov_num: 2,
        },
        Prescription {
            rx_num: 3,
            pat_num: 3,
            rx_date: today - Duration::days(12),
            drug: "Amoxicillin 500mg".into(),
            sig: "Take one capsule twice daily for 5 days".into(),
            disp: "10".into(),
            prov_num: 3,
        },
    ];

    // Referral sources for the two referral reports. Referral 1 has both demo
    // patients attached (RefFrom), so it ranks first with 305.00 of production;
    // referral 2 has one. The RefTo and RefCustom rows must never appear in
    // Referral Analysis, matching upstream's RefType = RefFrom filter. The
    // third source has no attachments, so it stays out of that report while
    // exercising every extra column of the FormRpReferrals field list.
    let referrals = vec![
        Referral {
            referral_num: 1,
            l_name: "Girma".into(),
            f_name: "Aster".into(),
            m_name: "K".into(),
            title: "Dr".into(),
            address: "12 Haile Gebreselassie St".into(),
            address2: "Suite 3".into(),
            city: "Addis Ababa".into(),
            state: "AA".into(),
            zip: "1000".into(),
            telephone: "+251 11 123 4567".into(),
            phone2: "+251 91 234 5678".into(),
            email: "aster.girma@example.com".into(),
            note: "Introduced by Dr Hailu".into(),
            ssn: String::new(),
            using_tin: 0,
            is_hidden: 0,
            not_person: 0,
            specialty_num: 17, // General Dentistry
        },
        Referral {
            referral_num: 2,
            l_name: "Tesfaye".into(),
            f_name: "Daniel".into(),
            m_name: String::new(),
            title: String::new(),
            address: "8 Bole Road".into(),
            address2: String::new(),
            city: "Addis Ababa".into(),
            state: "AA".into(),
            zip: "1001".into(),
            telephone: "+251 11 765 4321".into(),
            phone2: String::new(),
            email: String::new(),
            note: String::new(),
            ssn: String::new(),
            using_tin: 1,
            is_hidden: 1,
            not_person: 0,
            specialty_num: 0,
        },
        Referral {
            referral_num: 3,
            l_name: "Ministry of Health".into(),
            f_name: String::new(),
            m_name: String::new(),
            title: String::new(),
            address: "Bole Health Bureau".into(),
            address2: String::new(),
            city: "Addis Ababa".into(),
            state: "AA".into(),
            zip: "1002".into(),
            telephone: "+251 11 555 0100".into(),
            phone2: String::new(),
            email: "referrals@moh.example".into(),
            note: "Institutional source, no individual patients".into(),
            ssn: String::new(),
            using_tin: 1,
            is_hidden: 1,
            not_person: 1,
            specialty_num: 0,
        },
    ];
    let refattaches = vec![
        RefAttach { ref_attach_num: 1, referral_num: 1, pat_num: 3, ref_type: 1 },
        RefAttach { ref_attach_num: 2, referral_num: 1, pat_num: 1, ref_type: 1 },
        RefAttach { ref_attach_num: 3, referral_num: 2, pat_num: 3, ref_type: 1 },
        // RefTo and RefCustom rows are deliberately present to prove exclusion.
        RefAttach { ref_attach_num: 4, referral_num: 1, pat_num: 2, ref_type: 0 },
        RefAttach { ref_attach_num: 5, referral_num: 2, pat_num: 1, ref_type: 2 },
    ];

    let payments = vec![
        Payment {
            pay_num: 1,
            pat_num: 3,
            pay_date: today - Duration::days(1),
            pay_amount: 37.0,
            pay_type: 0,
            check_num: String::new(),
            note: "Copay at visit".into(),
        },
        Payment {
            pay_num: 2,
            pat_num: 1,
            pay_date: today - Duration::days(8),
            pay_amount: 60.0,
            pay_type: 1,
            check_num: "1042".into(),
            note: String::new(),
        },
    ];

    // Billing slice: the copay above is represented by a paysplit on
    // procedure 1. A small discount adjustment and an insurance claim round out
    // the ledger so all OD transaction classes are visible in demo mode.
    let pay_splits = vec![
        PaySplit {
            split_num: 1,
            pay_num: 1,
            pat_num: 3,
            proc_num: 1,
            split_amount: 37.0,
            date_pay: today - Duration::days(1),
            provider_num: 3,
            pay_plan_num: 0,
            adj_num: 0,
            unearned_type: 0,
                pay_plan_charge_num: 0,
                pay_plan_debit_type: Default::default(),
        },
        PaySplit {
            split_num: 2,
            pay_num: 2,
            pat_num: 1,
            proc_num: 2,
            split_amount: 60.0,
            date_pay: today - Duration::days(8),
            provider_num: 1,
            pay_plan_num: 0,
            adj_num: 0,
            unearned_type: 0,
                pay_plan_charge_num: 0,
                pay_plan_debit_type: Default::default(),
        },
    ];

    let adjustments = vec![Adjustment {
        adj_num: 1,
        adj_date: today - Duration::days(8),
        adj_amount: -10.0,
        pat_num: 1,
        adj_type: 1,
        provider_num: 1,
        proc_num: 2,
        proc_date: today - Duration::days(8),
        note: "Courtesy discount".into(),
    }];

    let claims = vec![Claim {
        claim_num: 1,
        pat_num: 3,
        plan_num: 1,
        claim_status: ClaimStatus::Received,
        date_sent: Some(today - Duration::days(3)),
        total_fee: 185.0,
        claim_identifier: "CLM-1001".into(),
    }];

    let claim_procs = vec![ClaimProc {
        claim_proc_num: 1,
        claim_num: 1,
        proc_num: 1,
        plan_num: 1,
        status: 1,
        fee_billed: 185.0,
        ins_pay_amt: 148.0,
        copay_amt: 37.0,
        allowed_amt: 185.0,
        write_off: 0.0,
        ins_pay_est: 0.0,
        date_supp_received: Some(today - Duration::days(3)),
        write_off_est: 10.0,
        write_off_est_override: -1.0,
        claim_payment_num: 1,
    }];

    // A simple instalment plan for patient 1, with the first charge already
    // due. This is the common Ethiopian workflow (no insurance involved).
    let pay_plans = vec![PayPlan {
        pay_plan_num: 1,
        pat_num: 1,
        guarantor: 1,
        pay_plan_date: today - Duration::days(20),
        date_pay_plan_start: Some(today - Duration::days(10)),
        completed_amt: 240.0,
        pay_schedule: PaymentSchedule::Monthly,
        number_of_payments: 4,
        pay_amt: 60.0,
        note: "Instalment plan".into(),
        charge_frequency: PayPlanFrequency::Monthly,
        ..Default::default()
    }];

    let pay_plan_charges = vec![PayPlanCharge {
        pay_plan_charge_num: 1,
        pay_plan_num: 1,
        guarantor: 1,
        pat_num: 1,
        charge_date: today - Duration::days(10),
        principal: 60.0,
        note: "Plan charge 1 of 4".into(),
        charge_type: PayPlanChargeType::Debit,
        fkey: 2,
        link_type: PayPlanLink::PROCEDURE,
        ..Default::default()
    }];

    // Two overlapping provider blocks: the report must count 3 hours, not 4.
    let schedules = vec![
        Schedule {
            schedule_num: 1,
            sched_date: today,
            start_time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            stop_time: NaiveTime::from_hms_opt(11, 0, 0).unwrap(),
            sched_type: 1,
            prov_num: 1,
            clinic_num: 1,
            ..Default::default()
        },
        Schedule {
            schedule_num: 2,
            sched_date: today,
            start_time: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
            stop_time: NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
            sched_type: 1,
            prov_num: 1,
            clinic_num: 1,
            ..Default::default()
        },
    ];
    let schedule_ops = vec![
        ScheduleOp {
            schedule_op_num: 1,
            schedule_num: 1,
            operatory_num: 1,
        },
        ScheduleOp {
            schedule_op_num: 2,
            schedule_num: 2,
            operatory_num: 2,
        },
    ];
    let claim_snapshots = vec![ClaimSnapshot {
        claim_snapshot_num: 1,
        proc_num: 1,
        claim_proc_num: 1,
        claim_type: "P".into(),
        write_off: 10.0,
        fee: 185.0,
        date_t_entry: at(today - Duration::days(3), 9, 0),
        ..Default::default()
    }];
    let pay_plan_links = vec![PayPlanLink {
        pay_plan_link_num: 1,
        pay_plan_num: 1,
        link_type: PayPlanLink::PROCEDURE,
        fkey: 2,
        ..Default::default()
    }];
    let carriers = vec![
        Carrier {
            carrier_num: 1,
            carrier_name: "Ethiopian Dental Insurance".into(),
            address: "PO Box 1247".into(),
            address2: String::new(),
            city: "Addis Ababa".into(),
            state: "AA".into(),
            zip: "1000".into(),
            ..Default::default()
        },
        Carrier {
            carrier_num: 2,
            carrier_name: "Highland Care Providers".into(),
            address: "12 Bole Road".into(),
            address2: "Suite 3".into(),
            city: "Addis Ababa".into(),
            state: "AA".into(),
            zip: "1001".into(),
            ..Default::default()
        },
    ];

    let statements = vec![Statement {
        statement_num: 1,
        pat_num: 1,
        date_sent: Some(today - Duration::days(5)),
        note: "Monthly statement".into(),
        is_invoice: false,
    }];

    let treat_plans = vec![
        TreatPlan {
            treat_plan_num: 1,
            pat_num: 1,
            date_saved: Some(at(today - Duration::days(2), 16, 30)),
            heading: "Active Treatment".into(),
            status: TreatPlanStatus::Active,
            provider_num: 1,
            note: "Phase 1 - restorative".into(),
        },
        TreatPlan {
            treat_plan_num: 2,
            pat_num: 4,
            date_saved: None,
            heading: "Proposed".into(),
            status: TreatPlanStatus::Saved,
            provider_num: 2,
            note: String::new(),
        },
    ];
    // Active plans link to procedurelog in OD; the demo keeps the display
    // fields directly so the same grid can render both kinds of plan.
    let treat_plan_lines = vec![
        TreatPlanLine {
            line_num: 1,
            treat_plan_num: 1,
            pat_num: 1,
            proc_num: 101,
            item_order: 0,
            tooth_num: "14".into(),
            proc_code: "D2740".into(),
            description: "Crown - porcelain/ceramic".into(),
            fee: 850.0,
            provider_num: 1,
            date_tp: Some(today),
            ..Default::default()
        },
        TreatPlanLine {
            line_num: 2,
            treat_plan_num: 2,
            pat_num: 4,
            proc_num: 102,
            item_order: 0,
            proc_code: "D1110".into(),
            description: "Prophylaxis - adult".into(),
            fee: 120.0,
            date_tp: Some(today),
            ..Default::default()
        },
    ];

    // Demo credentials are intentionally simple and documented in README.md.
    // They use the same encoded SHA3-512 format as a real userod row.
    let users = vec![
        Userod {
            user_num: 1,
            user_name: "admin".into(),
            password: password::generate("admin", password::HashType::Sha3_512).encode(),
            user_group_num: 1,
            employee_num: 0,
            clinic_num: 0,
            is_hidden: false,
        },
        Userod {
            user_num: 2,
            user_name: "aharrison".into(),
            password: password::generate("harrison", password::HashType::Sha3_512).encode(),
            user_group_num: 2,
            employee_num: 0,
            clinic_num: 1,
            is_hidden: false,
        },
    ];

    let group_attaches = vec![
        UserGroupAttach {
            user_group_attach_num: 1,
            user_num: 1,
            user_group_num: 1,
        },
        UserGroupAttach {
            user_group_attach_num: 2,
            user_num: 2,
            user_group_num: 2,
        },
    ];

    // Group 1 is a full-access admin group; group 2 is a clinical user who
    // cannot reach Setup, Reports or security administration.
    let mut group_permissions: Vec<GroupPermission> = Vec::new();
    let admin_perms = [
        Permission::APPOINTMENTS_MODULE,
        Permission::FAMILY_MODULE,
        Permission::ACCOUNT_MODULE,
        Permission::TP_MODULE,
        Permission::CHART_MODULE,
        Permission::REPORTS,
        Permission::SETUP,
        Permission::SECURITY_ADMIN,
        Permission::PAYMENT_CREATE,
        Permission::PAYMENT_EDIT,
        Permission::ADJUSTMENT_CREATE,
        Permission::ADJUSTMENT_EDIT,
        Permission::BILLING,
        Permission::APPOINTMENT_CREATE,
        Permission::APPOINTMENT_EDIT,
        Permission::APPOINTMENT_DELETE,
        Permission::TREAT_PLAN_EDIT,
        Permission::PATIENT_CREATE,
        Permission::PATIENT_EDIT,
        Permission::AUDIT_TRAIL,
        Permission::USER_LOG_ON_OFF,
    ];
    let clinical_perms = [
        Permission::APPOINTMENTS_MODULE,
        Permission::FAMILY_MODULE,
        Permission::CHART_MODULE,
        Permission::TP_MODULE,
        Permission::APPOINTMENT_CREATE,
        Permission::APPOINTMENT_EDIT,
        Permission::PATIENT_EDIT,
        Permission::USER_LOG_ON_OFF,
    ];
    for (group, perms) in [(1u32, &admin_perms[..]), (2u32, &clinical_perms[..])] {
        for perm in perms {
            group_permissions.push(GroupPermission {
                group_perm_num: group_permissions.len() as u32 + 1,
                user_group_num: group,
                perm_type: *perm,
                permission_level: 0,
                fkey: 0,
            });
        }
    }
    let security_log: Vec<SecurityEvent> = Vec::new();

    let prefs = vec![
        Pref {
            pref_num: 1,
            pref_name: "PracticeName".into(),
            pref_value: "Oak Hill Dental".into(),
        },
        Pref {
            pref_num: 2,
            pref_name: "AppointmentTimeIncrement".into(),
            pref_value: "5".into(),
        },
        Pref {
            pref_num: 3,
            pref_name: "UseClinics".into(),
            pref_value: "1".into(),
        },
    ];

    // Keep the patient list sorted the way the Family module shows it.
    patients.sort_by(|a, b| a.l_name.cmp(&b.l_name));
    let (definitions, display_fields) = seed_definitions();

    let diseases = vec![
        Disease {
            disease_num: 1,
            pat_num: 1,
            disease_def_num: 101,
            disease_name: "Hypertension".into(),
            note: "Controlled with medication".into(),
            status: ProblemStatus::Active,
            date_start: NaiveDate::from_ymd_opt(2024, 3, 1).unwrap(),
            date_stop: None,
        },
        Disease {
            disease_num: 2,
            pat_num: 1,
            disease_def_num: 102,
            disease_name: "Allergic rhinitis".into(),
            note: String::new(),
            status: ProblemStatus::Resolved,
            date_start: NaiveDate::from_ymd_opt(2023, 5, 12).unwrap(),
            date_stop: Some(NaiveDate::from_ymd_opt(2025, 8, 2).unwrap()),
        },
    ];
    let patient_notes = vec![PatientNote {
        pat_num: 1,
        medical: "Hypertension; allergic rhinitis".into(),
        treatment: "Composite filling on tooth 16 (2026-11-21)".into(),
        service: "General dentistry".into(),
        medical_company: String::new(),
        fam_financial: "Self-pay".into(),
    }];
    let clinic_cards = vec![ClinicCard {
        card_num: 1,
        pat_num: 2,
        card_number: "CC-0002041".into(),
        issued_at: NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(),
        issued_by: 1,
        reason: ClinicCardReason::CorporateAccount,
        authorization: "CORP-2041".into(),
        note: "Corporate billing agreement".into(),
        is_active: true,
    }];

    // A handful of procedure notes so the ProcNote report has demo output;
    // signatures and timestamps follow OD's procnote semantics.
    let proc_notes = vec![
        ProcNote {
            proc_note_num: 1,
            pat_num: 1,
            proc_num: 2,
            entry_datetime: today.and_hms_opt(9, 15, 0).unwrap(),
            user_num: 1,
            note: "Composite placed, occlusion checked.".into(),
            signature: "Dr. Ames".into(),
        },
        ProcNote {
            proc_note_num: 2,
            pat_num: 3,
            proc_num: 1,
            entry_datetime: today.and_hms_opt(14, 30, 0).unwrap(),
            user_num: 3,
            note: "Sealant applied to #14.".into(),
            signature: "Dr. Chen".into(),
        },
    ];

    // A minimal chart of accounts plus balanced journal entries for the current
    // year, enough to exercise the three accounting reports in demo mode.
    let accounts = vec![
        Account { account_num: 1, description: "Petty Cash".into(), account_type: AccountType::Asset, ..Default::default() },
        Account { account_num: 2, description: "Accounts Receivable".into(), account_type: AccountType::Asset, ..Default::default() },
        Account { account_num: 3, description: "Accounts Payable".into(), account_type: AccountType::Liability, ..Default::default() },
        Account { account_num: 4, description: "Retained Earnings".into(), account_type: AccountType::Equity, is_retained_earnings: true, ..Default::default() },
        Account { account_num: 5, description: "Patient Income".into(), account_type: AccountType::Income, ..Default::default() },
        Account { account_num: 6, description: "Supplies Expense".into(), account_type: AccountType::Expense, ..Default::default() },
    ];
    let jan1 = NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap();
    let journal_entries = vec![
        JournalEntry { journal_entry_num: 1, account_num: 2, date_displayed: jan1 + Duration::days(20), debit_amt: 1200.0, credit_amt: 0.0, memo: "Production".into(), check_number: "".into(), transaction_num: 1, ..Default::default() },
        JournalEntry { journal_entry_num: 2, account_num: 5, date_displayed: jan1 + Duration::days(20), debit_amt: 0.0, credit_amt: 1200.0, memo: "Production".into(), check_number: "".into(), transaction_num: 1, ..Default::default() },
        JournalEntry { journal_entry_num: 3, account_num: 6, date_displayed: jan1 + Duration::days(45), debit_amt: 300.0, credit_amt: 0.0, memo: "Supplies".into(), check_number: "101".into(), transaction_num: 2, ..Default::default() },
        JournalEntry { journal_entry_num: 4, account_num: 1, date_displayed: jan1 + Duration::days(45), debit_amt: 0.0, credit_amt: 300.0, memo: "Supplies".into(), check_number: "101".into(), transaction_num: 2, ..Default::default() },
    ];

    Data {
        patients,
        appointments,
        appointment_changes: Vec::new(),
        patient_corrections: Vec::new(),
        recalls,
        providers,
        operatories,
        clinics,
        procedures,
        procedure_discounts: std::collections::HashMap::new(),
        procedure_codes,
        fee_schedules,
        discount_plans,
        discount_plan_subs,
        fees,
        prescriptions,
        referrals,
        refattaches,
        proc_notes,
        accounts,
        journal_entries,
        schedules,
        schedule_ops,
        claim_snapshots,
        pay_plan_links,
        carriers,
        diseases,
        patient_notes,
        clinic_cards,
        payments,
        adjustments,
        pay_splits,
        claims,
        claim_procs,
        statements,
        pay_plans,
        pay_plan_charges,
        treat_plans,
        treat_plan_lines,
        definitions,
        display_fields,
        users,
        prefs,
        group_permissions,
        group_attaches,
        security_log,
        reminder_rules,
        patient_contacts,
        reminder_sent,
        outbox,
    }
}

impl Store for DemoStore {
    fn list_patients(&self) -> Result<Vec<Patient>, String> {
        Ok(self.data.lock().unwrap().patients.clone())
    }

    fn find_patients(&self, query: &str) -> Result<Vec<Patient>, String> {
        let q = query.trim().to_lowercase();
        Ok(self
            .data
            .lock()
            .unwrap()
            .patients
            .iter()
            .filter(|p| {
                q.is_empty()
                    || p.l_name.to_lowercase().contains(&q)
                    || p.f_name.to_lowercase().contains(&q)
                    || p.home_phone.contains(&q)
                    || p.pat_num.to_string() == q
            })
            .cloned()
            .collect())
    }

    fn get_patient(&self, pat_num: Id) -> Result<Option<Patient>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .patients
            .iter()
            .find(|p| p.pat_num == pat_num)
            .cloned())
    }

    fn upsert_patient(&self, patient: &Patient) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = if patient.pat_num == 0 {
            data.patients.iter().map(|p| p.pat_num).max().unwrap_or(0) + 1
        } else {
            patient.pat_num
        };
        let mut stored = patient.clone();
        stored.pat_num = num;
        if let Some(existing) = data.patients.iter_mut().find(|p| p.pat_num == num) {
            *existing = stored;
        } else {
            data.patients.push(stored);
        }
        data.patients.sort_by(|a, b| a.l_name.cmp(&b.l_name));
        Ok(num)
    }

    fn delete_patient(&self, pat_num: Id) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        data.patients.retain(|p| p.pat_num != pat_num);
        data.appointments.retain(|a| a.pat_num != pat_num);
        data.recalls.retain(|r| r.pat_num != pat_num);
        Ok(())
    }

    fn list_appointments(
        &self,
        from: NaiveDateTime,
        to: NaiveDateTime,
    ) -> Result<Vec<Appointment>, String> {
        let mut rows: Vec<Appointment> = self
            .data
            .lock()
            .unwrap()
            .appointments
            .iter()
            .filter(|a| a.apt_datetime >= from && a.apt_datetime < to)
            .cloned()
            .collect();
        rows.sort_by_key(|a| (a.apt_datetime, a.operatory_num));
        Ok(rows)
    }

    fn get_appointment(&self, apt_num: Id) -> Result<Option<Appointment>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .appointments
            .iter()
            .find(|a| a.apt_num == apt_num)
            .cloned())
    }

    fn upsert_appointment(&self, apt: &Appointment) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = if apt.apt_num == 0 {
            data.appointments.iter().map(|a| a.apt_num).max().unwrap_or(0) + 1
        } else {
            apt.apt_num
        };
        let mut stored = apt.clone();
        stored.apt_num = num;
        if let Some(existing) = data.appointments.iter_mut().find(|a| a.apt_num == num) {
            *existing = stored;
        } else {
            data.appointments.push(stored);
        }
        Ok(num)
    }

    fn delete_appointment(&self, apt_num: Id) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        data.appointments.retain(|a| a.apt_num != apt_num);
        Ok(())
    }

    fn list_waitlist_appointments(&self) -> Result<Vec<Appointment>, String> {
        let mut rows: Vec<Appointment> = self
            .data
            .lock()
            .unwrap()
            .appointments
            .iter()
            .filter(|a| a.apt_status == AppointmentStatus::UnschedList)
            .cloned()
            .collect();
        rows.sort_by_key(|a| (a.apt_datetime, a.pat_num, a.apt_num));
        Ok(rows)
    }

    fn list_appointment_changes(&self, apt_num: Id) -> Result<Vec<AppointmentChange>, String> {
        let mut rows: Vec<AppointmentChange> = self
            .data
            .lock()
            .unwrap()
            .appointment_changes
            .iter()
            .filter(|c| c.apt_num == apt_num)
            .cloned()
            .collect();
        rows.sort_by(|a, b| {
            b.changed_at
                .cmp(&a.changed_at)
                .then(b.change_num.cmp(&a.change_num))
        });
        Ok(rows)
    }

    fn append_appointment_change(&self, change: &AppointmentChange) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = if change.change_num == 0 {
            data.appointment_changes
                .iter()
                .map(|c| c.change_num)
                .max()
                .unwrap_or(0)
                + 1
        } else {
            change.change_num
        };
        let mut stored = change.clone();
        stored.change_num = num;
        if stored.changed_at == NaiveDateTime::default() {
            stored.changed_at = Local::now().naive_local();
        }
        data.appointment_changes.push(stored);
        Ok(num)
    }

    fn list_patient_corrections(&self, pat_num: Id) -> Result<Vec<PatientCorrection>, String> {
        let mut rows: Vec<PatientCorrection> = self
            .data
            .lock()
            .unwrap()
            .patient_corrections
            .iter()
            .filter(|c| c.pat_num == pat_num)
            .cloned()
            .collect();
        rows.sort_by(|a, b| {
            b.corrected_at
                .cmp(&a.corrected_at)
                .then(b.correction_num.cmp(&a.correction_num))
        });
        Ok(rows)
    }

    fn append_patient_correction(&self, correction: &PatientCorrection) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = if correction.correction_num == 0 {
            data.patient_corrections
                .iter()
                .map(|c| c.correction_num)
                .max()
                .unwrap_or(0)
                + 1
        } else {
            correction.correction_num
        };
        let mut stored = correction.clone();
        stored.correction_num = num;
        if stored.corrected_at == NaiveDateTime::default() {
            stored.corrected_at = Local::now().naive_local();
        }
        data.patient_corrections.push(stored);
        Ok(num)
    }

    fn list_recalls(&self) -> Result<Vec<Recall>, String> {
        let mut rows = self.data.lock().unwrap().recalls.clone();
        rows.sort_by_key(|r| r.recall_num);
        Ok(rows)
    }

    fn upsert_recall(&self, recall: &Recall) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = if recall.recall_num == 0 {
            data.recalls.iter().map(|r| r.recall_num).max().unwrap_or(0) + 1
        } else {
            recall.recall_num
        };
        let mut stored = recall.clone();
        stored.recall_num = num;
        if let Some(existing) = data.recalls.iter_mut().find(|r| r.recall_num == num) {
            *existing = stored;
        } else {
            data.recalls.push(stored);
        }
        Ok(num)
    }

    fn list_asap_appointments(&self) -> Result<Vec<Appointment>, String> {
        let mut rows: Vec<Appointment> = self
            .data
            .lock()
            .unwrap()
            .appointments
            .iter()
            .filter(|a| a.priority == RecallPriority::Asap)
            .cloned()
            .collect();
        rows.sort_by_key(|a| a.apt_datetime);
        Ok(rows)
    }

    fn list_providers(&self) -> Result<Vec<Provider>, String> {
        Ok(self.data.lock().unwrap().providers.clone())
    }
    fn list_operatories(&self) -> Result<Vec<Operatory>, String> {
        Ok(self.data.lock().unwrap().operatories.clone())
    }
    fn list_clinics(&self) -> Result<Vec<Clinic>, String> {
        Ok(self.data.lock().unwrap().clinics.clone())
    }
    fn list_procedures(&self, pat_num: Id) -> Result<Vec<Procedure>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .procedures
            .iter()
            .filter(|p| p.pat_num == pat_num)
            .cloned()
            .collect())
    }
    fn insert_procedure(&self, procedure: &Procedure) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = data.procedures.iter().map(|p| p.proc_num).max().unwrap_or(0) + 1;
        let mut stored = procedure.clone();
        stored.proc_num = num;
        stored.proc_status = ProcedureStatus::TreatmentPlanned;
        data.procedures.push(stored);
        Ok(num)
    }
    fn set_procedure_complete(&self, proc_num: Id, discount_amount: f64) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        let proc = data
            .procedures
            .iter_mut()
            .find(|p| p.proc_num == proc_num)
            .ok_or_else(|| format!("Procedure {proc_num} not found"))?;
        proc.proc_status = ProcedureStatus::Complete;
        data.procedure_discounts.insert(proc_num, discount_amount);
        Ok(())
    }
    fn list_procedure_codes(&self) -> Result<Vec<ProcedureCode>, String> {
        Ok(self.data.lock().unwrap().procedure_codes.clone())
    }
    fn list_fee_schedules(&self) -> Result<Vec<FeeSchedule>, String> {
        Ok(self.data.lock().unwrap().fee_schedules.clone())
    }
    fn list_discount_plans(&self) -> Result<Vec<DiscountPlan>, String> {
        Ok(self.data.lock().unwrap().discount_plans.clone())
    }
    fn list_discount_plan_subs(&self) -> Result<Vec<DiscountPlanSub>, String> {
        Ok(self.data.lock().unwrap().discount_plan_subs.clone())
    }
    fn upsert_discount_plan(&self, plan: &DiscountPlan) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = if plan.discount_plan_num == 0 {
            data.discount_plans
                .iter()
                .map(|p| p.discount_plan_num)
                .max()
                .unwrap_or(0)
                + 1
        } else {
            plan.discount_plan_num
        };
        let mut stored = plan.clone();
        stored.discount_plan_num = num;
        if let Some(existing) = data
            .discount_plans
            .iter_mut()
            .find(|p| p.discount_plan_num == num)
        {
            *existing = stored;
        } else {
            data.discount_plans.push(stored);
        }
        data.discount_plans
            .sort_by(|a, b| a.description.cmp(&b.description));
        Ok(num)
    }
    fn upsert_discount_plan_sub(&self, sub: &DiscountPlanSub) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = if sub.discount_sub_num == 0 {
            data.discount_plan_subs
                .iter()
                .map(|s| s.discount_sub_num)
                .max()
                .unwrap_or(0)
                + 1
        } else {
            sub.discount_sub_num
        };
        let mut stored = sub.clone();
        stored.discount_sub_num = num;
        if let Some(existing) = data
            .discount_plan_subs
            .iter_mut()
            .find(|s| s.discount_sub_num == num)
        {
            *existing = stored;
        } else {
            data.discount_plan_subs.push(stored);
        }
        data.discount_plan_subs.sort_by(|a, b| {
            (a.discount_plan_num, a.pat_num).cmp(&(b.discount_plan_num, b.pat_num))
        });
        Ok(num)
    }
    fn terminate_discount_plan_sub(
        &self,
        discount_sub_num: Id,
        term: NaiveDate,
    ) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        let sub = data
            .discount_plan_subs
            .iter_mut()
            .find(|s| s.discount_sub_num == discount_sub_num)
            .ok_or_else(|| format!("Discount subscription {discount_sub_num} not found"))?;
        sub.date_term = Some(term);
        Ok(())
    }
    fn list_fees(&self, schedule_num: Id, provider_num: Id) -> Result<Vec<Fee>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .fees
            .iter()
            .filter(|f| f.schedule_num == schedule_num)
            // Demo store has no provider-specific fees; OD uses ProvNum=0 for
            // the schedule-wide fee, which is what the "Default" picker means.
            .filter(|_| provider_num == 0)
            .cloned()
            .collect())
    }
    fn list_prescriptions(&self) -> Result<Vec<Prescription>, String> {
        Ok(self.data.lock().unwrap().prescriptions.clone())
    }
    fn list_referrals(&self) -> Result<Vec<Referral>, String> {
        Ok(self.data.lock().unwrap().referrals.clone())
    }
    fn list_refattaches(&self) -> Result<Vec<RefAttach>, String> {
        Ok(self.data.lock().unwrap().refattaches.clone())
    }
    fn list_proc_notes(&self) -> Result<Vec<ProcNote>, String> {
        let mut rows = self.data.lock().unwrap().proc_notes.clone();
        rows.sort_by_key(|note| (note.proc_num, note.entry_datetime));
        Ok(rows)
    }
    fn list_accounts(&self) -> Result<Vec<Account>, String> {
        let mut rows = self.data.lock().unwrap().accounts.clone();
        rows.sort_by_key(|account| account.account_num);
        Ok(rows)
    }
    fn list_journal_entries(&self) -> Result<Vec<JournalEntry>, String> {
        let mut rows = self.data.lock().unwrap().journal_entries.clone();
        rows.sort_by_key(|entry| (entry.date_displayed, entry.journal_entry_num));
        Ok(rows)
    }

    fn post_journal_transaction(&self, entries: &[JournalEntry]) -> Result<Id, String> {
        if entries.is_empty() {
            return Err("No journal lines to post".into());
        }
        let transaction_num = entries[0].transaction_num;
        if transaction_num == 0 {
            return Err("Transaction number must be positive".into());
        }
        if entries
            .iter()
            .any(|e| e.transaction_num != transaction_num)
        {
            return Err("All journal lines must share one transaction number".into());
        }
        let mut data = self.data.lock().unwrap();
        let mut next = data
            .journal_entries
            .iter()
            .map(|e| e.journal_entry_num)
            .max()
            .unwrap_or(0);
        for entry in entries {
            next += 1;
            let mut stored = entry.clone();
            stored.journal_entry_num = next;
            data.journal_entries.push(stored);
        }
        Ok(transaction_num)
    }

    fn post_journal_batch(&self, batches: &[Vec<JournalEntry>]) -> Result<Vec<Id>, String> {
        if batches.is_empty() {
            return Err("No journal transactions to post".into());
        }
        let mut transaction_nums = Vec::with_capacity(batches.len());
        for (index, entries) in batches.iter().enumerate() {
            if entries.is_empty() {
                return Err(format!("Journal batch {index} has no lines"));
            }
            let transaction_num = entries[0].transaction_num;
            if transaction_num == 0 {
                return Err(format!("Journal batch {index} has no transaction number"));
            }
            if entries.iter().any(|e| e.transaction_num != transaction_num) {
                return Err(format!("Journal batch {index} mixes transaction numbers"));
            }
            transaction_nums.push(transaction_num);
        }
        // Hold the lock for the whole batch so no other write can interleave.
        let mut data = self.data.lock().unwrap();
        let mut next = data
            .journal_entries
            .iter()
            .map(|e| e.journal_entry_num)
            .max()
            .unwrap_or(0);
        for entries in batches {
            for entry in entries {
                next += 1;
                let mut stored = entry.clone();
                stored.journal_entry_num = next;
                data.journal_entries.push(stored);
            }
        }
        Ok(transaction_nums)
    }

    fn list_schedules(&self) -> Result<Vec<Schedule>, String> {
        let mut rows = self.data.lock().unwrap().schedules.clone();
        rows.sort_by_key(|s| (s.sched_date, s.start_time));
        Ok(rows)
    }
    fn list_schedule_ops(&self) -> Result<Vec<ScheduleOp>, String> {
        Ok(self.data.lock().unwrap().schedule_ops.clone())
    }
    fn list_claim_snapshots(&self) -> Result<Vec<ClaimSnapshot>, String> {
        Ok(self.data.lock().unwrap().claim_snapshots.clone())
    }
    fn list_pay_plan_links(&self) -> Result<Vec<PayPlanLink>, String> {
        Ok(self.data.lock().unwrap().pay_plan_links.clone())
    }
    fn list_carriers(&self) -> Result<Vec<Carrier>, String> {
        Ok(self.data.lock().unwrap().carriers.clone())
    }

    fn list_diseases(&self, pat_num: Id) -> Result<Vec<Disease>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .diseases
            .iter()
            .filter(|d| d.pat_num == pat_num)
            .cloned()
            .collect())
    }
    fn get_patient_note(&self, pat_num: Id) -> Result<Option<PatientNote>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .patient_notes
            .iter()
            .find(|note| note.pat_num == pat_num)
            .cloned())
    }
    fn list_registry_treatments(&self, pat_num: Id) -> Result<Vec<RegistryTreatment>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .procedures
            .iter()
            .filter(|p| p.pat_num == pat_num)
            .map(|p| RegistryTreatment {
                proc_num: p.proc_num,
                pat_num: p.pat_num,
                proc_date: p.proc_date,
                description: if p.description_override.trim().is_empty() {
                    format!("Procedure code #{}", p.code_num)
                } else {
                    p.description_override.clone()
                },
                tooth_site: p.tooth_num.clone(),
                fee: p.fee,
                proc_status: p.proc_status as i8,
            })
            .collect())
    }
    fn list_clinic_cards(&self, pat_num: Id) -> Result<Vec<ClinicCard>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .clinic_cards
            .iter()
            .filter(|card| card.pat_num == pat_num)
            .cloned()
            .collect())
    }
    fn issue_clinic_card(&self, card: &ClinicCard) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let card_num = data.clinic_cards.iter().map(|c| c.card_num).max().unwrap_or(0) + 1;
        let mut stored = card.clone();
        stored.card_num = card_num;
        stored.is_active = true;
        data.clinic_cards.push(stored);
        Ok(card_num)
    }
    fn deactivate_clinic_card(&self, card_num: Id) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        let card = data
            .clinic_cards
            .iter_mut()
            .find(|card| card.card_num == card_num)
            .ok_or_else(|| format!("clinic card {card_num} not found"))?;
        card.is_active = false;
        Ok(())
    }
    fn list_payments(&self, pat_num: Id) -> Result<Vec<Payment>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .payments
            .iter()
            .filter(|p| p.pat_num == pat_num)
            .cloned()
            .collect())
    }
    fn list_adjustments(&self, pat_num: Id) -> Result<Vec<Adjustment>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .adjustments
            .iter()
            .filter(|a| a.pat_num == pat_num)
            .cloned()
            .collect())
    }
    fn list_pay_splits(&self, pat_num: Id) -> Result<Vec<PaySplit>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .pay_splits
            .iter()
            .filter(|p| p.pat_num == pat_num)
            .cloned()
            .collect())
    }
    fn list_claims(&self, pat_num: Id) -> Result<Vec<Claim>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .claims
            .iter()
            .filter(|c| c.pat_num == pat_num)
            .cloned()
            .collect())
    }
    fn list_claim_procs(&self, pat_num: Id) -> Result<Vec<ClaimProc>, String> {
        let claim_nums: Vec<Id> = self
            .data
            .lock()
            .unwrap()
            .claims
            .iter()
            .filter(|c| c.pat_num == pat_num)
            .map(|c| c.claim_num)
            .collect();
        Ok(self
            .data
            .lock()
            .unwrap()
            .claim_procs
            .iter()
            .filter(|cp| claim_nums.contains(&cp.claim_num))
            .cloned()
            .collect())
    }
    fn list_statements(&self, pat_num: Id) -> Result<Vec<Statement>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .statements
            .iter()
            .filter(|s| s.pat_num == pat_num)
            .cloned()
            .collect())
    }
    fn insert_statement(&self, statement: &Statement) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = if statement.statement_num == 0 {
            data.statements
                .iter()
                .map(|s| s.statement_num)
                .max()
                .unwrap_or(0)
                + 1
        } else {
            statement.statement_num
        };
        let mut stored = statement.clone();
        stored.statement_num = num;
        data.statements.push(stored);
        Ok(num)
    }
    fn list_pay_plans(&self, pat_num: Id) -> Result<Vec<PayPlan>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .pay_plans
            .iter()
            .filter(|p| p.pat_num == pat_num || p.guarantor == pat_num)
            .cloned()
            .collect())
    }
    fn list_pay_plan_charges(&self, pat_num: Id) -> Result<Vec<PayPlanCharge>, String> {
        let plan_nums: Vec<Id> = self
            .data
            .lock()
            .unwrap()
            .pay_plans
            .iter()
            .filter(|p| p.pat_num == pat_num || p.guarantor == pat_num)
            .map(|p| p.pay_plan_num)
            .collect();
        Ok(self
            .data
            .lock()
            .unwrap()
            .pay_plan_charges
            .iter()
            .filter(|c| plan_nums.contains(&c.pay_plan_num))
            .cloned()
            .collect())
    }
    fn upsert_pay_plan(&self, plan: &PayPlan) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = if plan.pay_plan_num == 0 {
            data.pay_plans.iter().map(|p| p.pay_plan_num).max().unwrap_or(0) + 1
        } else {
            plan.pay_plan_num
        };
        let mut stored = plan.clone();
        stored.pay_plan_num = num;
        if let Some(existing) = data.pay_plans.iter_mut().find(|p| p.pay_plan_num == num) {
            *existing = stored;
        } else {
            data.pay_plans.push(stored);
        }
        Ok(num)
    }
    fn upsert_pay_plan_charge(&self, charge: &PayPlanCharge) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = if charge.pay_plan_charge_num == 0 {
            data.pay_plan_charges
                .iter()
                .map(|c| c.pay_plan_charge_num)
                .max()
                .unwrap_or(0)
                + 1
        } else {
            charge.pay_plan_charge_num
        };
        let mut stored = charge.clone();
        stored.pay_plan_charge_num = num;
        if let Some(existing) = data
            .pay_plan_charges
            .iter_mut()
            .find(|c| c.pay_plan_charge_num == num)
        {
            *existing = stored;
        } else {
            data.pay_plan_charges.push(stored);
        }
        Ok(num)
    }
    fn delete_pay_plan_charge(&self, charge_num: Id) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        data.pay_plan_charges
            .retain(|c| c.pay_plan_charge_num != charge_num);
        Ok(())
    }
    fn upsert_pay_plan_link(&self, link: &PayPlanLink) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = if link.pay_plan_link_num == 0 {
            data.pay_plan_links
                .iter()
                .map(|l| l.pay_plan_link_num)
                .max()
                .unwrap_or(0)
                + 1
        } else {
            link.pay_plan_link_num
        };
        let mut stored = link.clone();
        stored.pay_plan_link_num = num;
        if let Some(existing) = data
            .pay_plan_links
            .iter_mut()
            .find(|l| l.pay_plan_link_num == num)
        {
            *existing = stored;
        } else {
            data.pay_plan_links.push(stored);
        }
        Ok(num)
    }
    fn delete_pay_plan_link(&self, link_num: Id) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        data.pay_plan_links.retain(|l| l.pay_plan_link_num != link_num);
        Ok(())
    }
    fn upsert_adjustment(&self, adjustment: &Adjustment) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = if adjustment.adj_num == 0 {
            data.adjustments.iter().map(|a| a.adj_num).max().unwrap_or(0) + 1
        } else {
            adjustment.adj_num
        };
        let mut stored = adjustment.clone();
        stored.adj_num = num;
        // Manual adjustments have no service date; mirror the MySQL store.
        if stored.proc_date.year() < 1880 {
            stored.proc_date = stored.adj_date;
        }
        if let Some(existing) = data.adjustments.iter_mut().find(|a| a.adj_num == num) {
            *existing = stored;
        } else {
            data.adjustments.push(stored);
        }
        Ok(num)
    }
    fn record_payment(
        &self,
        payment: &Payment,
        proc_num: Id,
        pay_plan_num: Id,
    ) -> Result<Id, String> {
        let target = if pay_plan_num != 0 {
            PaymentTarget::PayPlan(pay_plan_num)
        } else if proc_num != 0 {
            PaymentTarget::Procedure(proc_num)
        } else {
            PaymentTarget::Unallocated
        };
        self.record_payment_splits(
            payment,
            &[PaymentAllocation {
                target,
                amount: payment.pay_amount,
                ..Default::default()
            }],
        )
    }

    fn record_payment_splits(
        &self,
        payment: &Payment,
        allocations: &[PaymentAllocation],
    ) -> Result<Id, String> {
        let splits = payment.splits(allocations)?;
        let mut data = self.data.lock().unwrap();
        let pay_num = if payment.pay_num == 0 {
            data.payments.iter().map(|p| p.pay_num).max().unwrap_or(0) + 1
        } else {
            payment.pay_num
        };
        let mut stored = payment.clone();
        stored.pay_num = pay_num;
        data.payments.push(stored);

        let mut split_num = data.pay_splits.iter().map(|s| s.split_num).max().unwrap_or(0) + 1;
        for mut split in splits {
            split.pay_num = pay_num;
            split.split_num = split_num;
            split_num += 1;
            data.pay_splits.push(split);
        }
        Ok(pay_num)
    }
    fn list_treat_plans(&self, pat_num: Id) -> Result<Vec<TreatPlan>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .treat_plans
            .iter()
            .filter(|p| p.pat_num == pat_num)
            .cloned()
            .collect())
    }
    fn upsert_treat_plan(&self, plan: &TreatPlan) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = if plan.treat_plan_num == 0 {
            data.treat_plans
                .iter()
                .map(|p| p.treat_plan_num)
                .max()
                .unwrap_or(0)
                + 1
        } else {
            plan.treat_plan_num
        };
        let mut stored = plan.clone();
        stored.treat_plan_num = num;
        if let Some(existing) = data
            .treat_plans
            .iter_mut()
            .find(|p| p.treat_plan_num == num)
        {
            *existing = stored;
        } else {
            data.treat_plans.push(stored);
        }
        Ok(num)
    }
    fn delete_treat_plan(&self, plan_num: Id) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        data.treat_plan_lines
            .retain(|line| line.treat_plan_num != plan_num);
        data.treat_plans.retain(|p| p.treat_plan_num != plan_num);
        Ok(())
    }
    fn list_treat_plan_lines(&self, plan_num: Id) -> Result<Vec<TreatPlanLine>, String> {
        let mut lines: Vec<TreatPlanLine> = self
            .data
            .lock()
            .unwrap()
            .treat_plan_lines
            .iter()
            .filter(|line| line.treat_plan_num == plan_num)
            .cloned()
            .collect();
        lines.sort_by_key(|line| line.item_order);
        Ok(lines)
    }
    fn upsert_treat_plan_line(
        &self,
        plan: &TreatPlan,
        line: &TreatPlanLine,
    ) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = if line.line_num == 0 {
            data.treat_plan_lines
                .iter()
                .map(|l| l.line_num)
                .max()
                .unwrap_or(0)
                + 1
        } else {
            line.line_num
        };
        let mut stored = line.clone();
        stored.line_num = num;
        stored.treat_plan_num = plan.treat_plan_num;
        stored.pat_num = plan.pat_num;
        if stored.item_order == 0 && line.line_num == 0 {
            stored.item_order = data
                .treat_plan_lines
                .iter()
                .filter(|l| l.treat_plan_num == plan.treat_plan_num)
                .map(|l| l.item_order)
                .max()
                .unwrap_or(-1)
                + 1;
        }
        if let Some(existing) = data
            .treat_plan_lines
            .iter_mut()
            .find(|l| l.line_num == num)
        {
            *existing = stored;
        } else {
            data.treat_plan_lines.push(stored);
        }
        Ok(num)
    }
    fn delete_treat_plan_line(&self, _plan: &TreatPlan, line_num: Id) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        data.treat_plan_lines.retain(|l| l.line_num != line_num);
        Ok(())
    }

    fn list_definitions(&self) -> Result<Vec<Definition>, String> {
        let mut rows = self.data.lock().unwrap().definitions.clone();
        rows.sort_by(|a, b| {
            a.category
                .as_od()
                .cmp(&b.category.as_od())
                .then(a.item_order.cmp(&b.item_order))
                .then(a.def_num.cmp(&b.def_num))
        });
        Ok(rows)
    }

    fn upsert_definition(&self, definition: &Definition) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = if definition.def_num == 0 {
            data.definitions
                .iter()
                .map(|d| d.def_num)
                .max()
                .unwrap_or(0)
                + 1
        } else {
            definition.def_num
        };
        let mut stored = definition.clone();
        stored.def_num = num;
        if let Some(existing) = data.definitions.iter_mut().find(|d| d.def_num == num) {
            *existing = stored;
        } else {
            data.definitions.push(stored);
        }
        Ok(num)
    }

    fn delete_definition(&self, def_num: Id) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        let category = data
            .definitions
            .iter()
            .find(|d| d.def_num == def_num)
            .map(|d| d.category);
        data.definitions.retain(|d| d.def_num != def_num);
        if let Some(category) = category {
            let mut rows: Vec<&mut Definition> = data
                .definitions
                .iter_mut()
                .filter(|d| d.category == category)
                .collect();
            rows.sort_by_key(|d| (d.item_order, d.def_num));
            for (order, row) in rows.into_iter().enumerate() {
                row.item_order = order as i32;
            }
        }
        Ok(())
    }

    fn list_display_fields(&self) -> Result<Vec<DisplayField>, String> {
        let mut rows = self.data.lock().unwrap().display_fields.clone();
        rows.sort_by(|a, b| {
            a.category
                .as_od()
                .cmp(&b.category.as_od())
                .then(a.item_order.cmp(&b.item_order))
                .then(a.display_field_num.cmp(&b.display_field_num))
        });
        Ok(rows)
    }

    fn upsert_display_field(&self, field: &DisplayField) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let num = if field.display_field_num == 0 {
            data.display_fields
                .iter()
                .map(|f| f.display_field_num)
                .max()
                .unwrap_or(0)
                + 1
        } else {
            field.display_field_num
        };
        let mut stored = field.clone();
        stored.display_field_num = num;
        if let Some(existing) = data
            .display_fields
            .iter_mut()
            .find(|f| f.display_field_num == num)
        {
            *existing = stored;
        } else {
            data.display_fields.push(stored);
        }
        Ok(num)
    }

    fn replace_display_fields(
        &self,
        category: DisplayFieldCategory,
        fields: &[DisplayField],
    ) -> Result<(), String> {
        let normalized: Vec<DisplayField> = fields
            .iter()
            .enumerate()
            .map(|(index, field)| DisplayField {
                display_field_num: 0,
                category,
                item_order: index as i32,
                ..field.clone()
            })
            .collect();
        // OD stores no rows when a category is exactly its default list.
        let signature = |rows: &[DisplayField]| -> Vec<_> {
            rows.iter()
                .map(|f| {
                    (
                        f.internal_name.clone(),
                        f.description.clone(),
                        f.column_width,
                        f.pick_list.clone(),
                        f.description_override.clone(),
                    )
                })
                .collect()
        };
        let mut rows = if signature(&normalized) == signature(&default_display_fields(category)) {
            Vec::new()
        } else {
            normalized
        };
        let mut data = self.data.lock().unwrap();
        data.display_fields.retain(|f| f.category != category);
        let mut num = data
            .display_fields
            .iter()
            .map(|f| f.display_field_num)
            .max()
            .unwrap_or(0);
        for field in &mut rows {
            num += 1;
            field.display_field_num = num;
        }
        data.display_fields.extend(rows);
        Ok(())
    }

    fn list_users(&self) -> Result<Vec<Userod>, String> {
        Ok(self.data.lock().unwrap().users.clone())
    }
    fn list_prefs(&self) -> Result<Vec<Pref>, String> {
        Ok(self.data.lock().unwrap().prefs.clone())
    }
    fn list_group_permissions(&self) -> Result<Vec<GroupPermission>, String> {
        Ok(self.data.lock().unwrap().group_permissions.clone())
    }
    fn list_user_group_attaches(&self) -> Result<Vec<UserGroupAttach>, String> {
        Ok(self.data.lock().unwrap().group_attaches.clone())
    }
    fn list_security_log(&self, limit: usize) -> Result<Vec<SecurityEvent>, String> {
        let data = self.data.lock().unwrap();
        let mut events: Vec<SecurityEvent> = data.security_log.clone();
        events.sort_by(|a, b| b.log_datetime.cmp(&a.log_datetime));
        events.truncate(limit);
        Ok(events)
    }
    fn append_security_log(&self, event: &SecurityEvent) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        let mut stored = event.clone();
        stored.security_log_num = data.security_log.len() as u32 + 1;
        data.security_log.push(stored);
        Ok(())
    }

    // ------------------------------------------------------------ eServices

    fn list_reminder_rules(&self) -> Result<Vec<ReminderRule>, String> {
        Ok(self.data.lock().unwrap().reminder_rules.clone())
    }

    fn upsert_reminder_rule(&self, rule: &ReminderRule) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let mut stored = rule.clone();
        if stored.rule_num == 0 {
            stored.rule_num = data.reminder_rules.iter().map(|r| r.rule_num).max().unwrap_or(0) + 1;
        }
        if let Some(existing) = data
            .reminder_rules
            .iter_mut()
            .find(|r| r.rule_num == stored.rule_num)
        {
            *existing = stored.clone();
        } else {
            data.reminder_rules.push(stored.clone());
        }
        Ok(stored.rule_num)
    }

    fn list_patient_contacts(&self) -> Result<Vec<PatientContact>, String> {
        Ok(self.data.lock().unwrap().patient_contacts.clone())
    }

    fn upsert_patient_contact(&self, contact: &PatientContact) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        if let Some(existing) = data
            .patient_contacts
            .iter_mut()
            .find(|c| c.pat_num == contact.pat_num)
        {
            *existing = contact.clone();
        } else {
            data.patient_contacts.push(contact.clone());
        }
        Ok(contact.pat_num)
    }

    fn list_reminder_sent(&self) -> Result<Vec<(Id, Id)>, String> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .reminder_sent
            .iter()
            .map(|(rule, apt, _)| (*rule, *apt))
            .collect())
    }

    fn mark_reminder_sent(&self, rule_num: Id, apt_num: Id, pat_num: Id) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        if !data
            .reminder_sent
            .iter()
            .any(|(rule, apt, _)| *rule == rule_num && *apt == apt_num)
        {
            data.reminder_sent.push((rule_num, apt_num, pat_num));
        }
        Ok(())
    }

    fn insert_outbox(&self, message: &OutboxMessage) -> Result<Id, String> {
        let mut data = self.data.lock().unwrap();
        let mut stored = message.clone();
        stored.message_num = data.outbox.iter().map(|m| m.message_num).max().unwrap_or(0) + 1;
        if stored.created_at == NaiveDateTime::default() {
            stored.created_at = Local::now().naive_local();
        }
        data.outbox.push(stored.clone());
        Ok(stored.message_num)
    }

    fn list_outbox(&self, limit: usize) -> Result<Vec<OutboxMessage>, String> {
        let mut rows = self.data.lock().unwrap().outbox.clone();
        rows.sort_by(|a, b| b.message_num.cmp(&a.message_num));
        rows.truncate(limit);
        Ok(rows)
    }

    fn update_outbox(
        &self,
        message_num: Id,
        status: OutboxStatus,
        attempts: u32,
        sent_at: Option<NaiveDateTime>,
        last_error: &str,
    ) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        let Some(message) = data.outbox.iter_mut().find(|m| m.message_num == message_num) else {
            return Err(format!("Outbox message {message_num} was not found"));
        };
        message.status = status;
        message.attempts = attempts;
        message.sent_at = sent_at;
        message.last_error = last_error.to_string();
        Ok(())
    }
}
