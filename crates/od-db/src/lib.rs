//! Persistence for the scoped port.
//!
//! `DemoStore` is an in-memory backend seeded with sample data so the desktop
//! application is usable without a database. `MySqlStore` speaks to an Open
//! Dental-compatible MySQL schema (see `schema.sql`).

mod demo;
mod mysql_store;

pub use demo::DemoStore;
pub use mysql_store::{MySqlStore, SchemaMode, WritePolicy};

use chrono::{NaiveDate, NaiveDateTime};
use od_domain::definitions::{Definition, DisplayField, DisplayFieldCategory};
use od_domain::eservices::{
    OutboxMessage, OutboxStatus, PatientContact, ReminderRule,
};
use od_domain::*;

/// Synchronous store interface used by the GUI.
pub trait Store: Send + Sync {
    /// OD audit columns need the acting user. Demo data has no OD user, so the
    /// default is a no-op; MySQL overrides it.
    fn set_current_user(&self, _user_num: Id) {}
    fn current_user(&self) -> Id {
        0
    }

    fn list_patients(&self) -> Result<Vec<Patient>, String>;
    fn find_patients(&self, query: &str) -> Result<Vec<Patient>, String>;
    fn get_patient(&self, pat_num: Id) -> Result<Option<Patient>, String>;
    fn upsert_patient(&self, patient: &Patient) -> Result<Id, String>;
    fn delete_patient(&self, pat_num: Id) -> Result<(), String>;

    fn list_appointments(
        &self,
        from: NaiveDateTime,
        to: NaiveDateTime,
    ) -> Result<Vec<Appointment>, String>;
    fn get_appointment(&self, apt_num: Id) -> Result<Option<Appointment>, String>;
    fn upsert_appointment(&self, apt: &Appointment) -> Result<Id, String>;
    fn delete_appointment(&self, apt_num: Id) -> Result<(), String>;
    /// OD's unscheduled list: appointments with `AptStatus = 3`. This is the
    /// waitlist; removing a row from it never sends a cancellation message.
    fn list_waitlist_appointments(&self) -> Result<Vec<Appointment>, String>;
    /// OD `histappointment` rows for one appointment, newest first.
    fn list_appointment_changes(&self, apt_num: Id) -> Result<Vec<AppointmentChange>, String>;
    fn append_appointment_change(&self, change: &AppointmentChange) -> Result<Id, String>;
    /// Local snapshots of patient edits, used for migration corrections and
    /// rollback. Kept outside OD's tables because OD has no patient history.
    fn list_patient_corrections(&self, pat_num: Id) -> Result<Vec<PatientCorrection>, String>;
    fn append_patient_correction(&self, correction: &PatientCorrection) -> Result<Id, String>;
    /// All OD `recall` rows; due-state filtering happens in `od-core`.
    fn list_recalls(&self) -> Result<Vec<Recall>, String>;
    fn upsert_recall(&self, recall: &Recall) -> Result<Id, String>;
    /// ASAP backlog: appointments with OD `Priority = 1` and an active status.
    fn list_asap_appointments(&self) -> Result<Vec<Appointment>, String>;

    fn list_providers(&self) -> Result<Vec<Provider>, String>;
    fn list_operatories(&self) -> Result<Vec<Operatory>, String>;
    fn list_clinics(&self) -> Result<Vec<Clinic>, String>;
    fn list_procedures(&self, pat_num: Id) -> Result<Vec<Procedure>, String>;
    /// Insert a new OD `procedurelog` row with `ProcStatus = TreatmentPlanned`.
    /// The caller supplies the date, CDT code, fee and provider; OD assigns
    /// the key and the bookkeeping columns.
    fn insert_procedure(&self, procedure: &Procedure) -> Result<Id, String>;
    /// Move a procedure to Complete, setting OD's `DateComplete` and storing
    /// the booked discount amount on `procedurelog` without rewriting the
    /// service date or any other field.
    fn set_procedure_complete(&self, proc_num: Id, discount_amount: f64) -> Result<(), String>;
    /// All OD `procedurecode` rows, needed by reports that identify a service
    /// by its CDT code (e.g. the broken-appointment D9986/D9987 report).
    fn list_procedure_codes(&self) -> Result<Vec<ProcedureCode>, String>;
    /// OD `feesched` rows for the Procedure Codes report's schedule picker.
    fn list_fee_schedules(&self) -> Result<Vec<FeeSchedule>, String>;
    /// OD `fee` rows for one schedule / provider at the default clinic (0).
    fn list_fees(&self, schedule_num: Id, provider_num: Id) -> Result<Vec<Fee>, String>;
    /// All OD `rxpat` prescriptions, needed by the Prescriptions report.
    fn list_prescriptions(&self) -> Result<Vec<Prescription>, String>;
    /// All OD `referral` rows for the Referral Analysis report.
    fn list_referrals(&self) -> Result<Vec<Referral>, String>;
    /// All OD `refattach` rows; the builder keeps only `RefFrom` (1) rows.
    fn list_refattaches(&self) -> Result<Vec<RefAttach>, String>;
    /// All OD `procnote` rows, needed by the Procedure Notes report.
    fn list_proc_notes(&self) -> Result<Vec<ProcNote>, String>;
    /// Append an OD `procnote` row. OD notes are immutable, so there is no
    /// update or delete. Writes require the explicit OD-write opt-in.
    fn append_proc_note(&self, _note: &ProcNote) -> Result<Id, String> {
        Err("Procedure note writes are not supported by this store".into())
    }
    /// All port-local appointment call-log rows (confirmations and phone calls).
    fn list_comm_logs(&self) -> Result<Vec<CommLog>, String> {
        Ok(Vec::new())
    }
    /// Insert or update one port-local call log row. No OD-owned write is made.
    fn upsert_comm_log(&self, _log: &CommLog) -> Result<Id, String> {
        Err("Call log writes are not supported by this store".into())
    }
    /// OD chart-of-accounts rows, needed by the three accounting reports.
    fn list_accounts(&self) -> Result<Vec<Account>, String>;
    /// All OD `journalentry` rows; the builders apply the date filters.
    fn list_journal_entries(&self) -> Result<Vec<JournalEntry>, String>;
    /// Writes one balanced `journalentry` transaction. Every line must carry the
    /// same `TransactionNum` and date, and debit/credit totals must match; the
    /// `od_core` builder guarantees this. Both backends insert atomically so a
    /// partial pair can never reach the accounting reports. Returns that
    /// transaction number.
    fn post_journal_transaction(&self, entries: &[JournalEntry]) -> Result<Id, String>;
    /// Writes several journal transactions as one atomic batch. Each inner
    /// slice is one transaction group (normally a balanced debit/credit pair)
    /// with its own `TransactionNum`; every line across every group lands
    /// together or none do. Returns the transaction numbers in the order
    /// supplied. The default keeps external implementors working, but both
    /// bundled backends override it to guarantee all-or-nothing writes.
    fn post_journal_batch(&self, batches: &[Vec<JournalEntry>]) -> Result<Vec<Id>, String> {
        batches
            .iter()
            .map(|entries| self.post_journal_transaction(entries))
            .collect()
    }

    // --- Report fidelity tranche 3 ---------------------------------------
    // These source tables are read-only in the port. Defaults are empty so
    // backends that have not loaded them keep working unchanged.
    fn list_schedules(&self) -> Result<Vec<Schedule>, String> {
        Ok(Vec::new())
    }
    fn list_schedule_ops(&self) -> Result<Vec<ScheduleOp>, String> {
        Ok(Vec::new())
    }
    fn list_pay_plan_links(&self) -> Result<Vec<PayPlanLink>, String> {
        Ok(Vec::new())
    }
    /// Insert or update an OD `payplanlink` row. A `pay_plan_link_num` of 0 inserts.
    fn upsert_pay_plan_link(&self, link: &PayPlanLink) -> Result<Id, String>;
    /// Delete an OD `payplanlink` row by primary key.
    fn delete_pay_plan_link(&self, link_num: Id) -> Result<(), String>;
    fn list_claim_snapshots(&self) -> Result<Vec<ClaimSnapshot>, String> {
        Ok(Vec::new())
    }
    fn list_discount_plans(&self) -> Result<Vec<DiscountPlan>, String> {
        Ok(Vec::new())
    }
    fn list_discount_plan_subs(&self) -> Result<Vec<DiscountPlanSub>, String> {
        Ok(Vec::new())
    }
    /// Insert or update an OD `discountplan` row. A `discount_plan_num` of 0 inserts.
    fn upsert_discount_plan(&self, plan: &DiscountPlan) -> Result<Id, String>;
    /// Insert or update an OD `discountplansub` row. A `discount_sub_num` of 0
    /// inserts. A term date before 1880 (or `None`) means open ended upstream.
    fn upsert_discount_plan_sub(&self, sub: &DiscountPlanSub) -> Result<Id, String>;
    /// End a subscription by setting its `DateTerm`, as OD's zero/pre-1880 term
    /// sentinel does. The row is kept so historic adjustments still resolve the
    /// plan that produced them; subscriptions are never hard-deleted here.
    fn terminate_discount_plan_sub(
        &self,
        discount_sub_num: Id,
        term: NaiveDate,
    ) -> Result<(), String>;
    fn list_screen_groups(&self) -> Result<Vec<ScreenGroup>, String> {
        Ok(Vec::new())
    }
    fn list_screens(&self) -> Result<Vec<Screen>, String> {
        Ok(Vec::new())
    }
    fn list_sites(&self) -> Result<Vec<Site>, String> {
        Ok(Vec::new())
    }
    fn list_counties(&self) -> Result<Vec<County>, String> {
        Ok(Vec::new())
    }
    fn list_patient_races(&self) -> Result<Vec<PatientRace>, String> {
        Ok(Vec::new())
    }
    fn list_carriers(&self) -> Result<Vec<Carrier>, String> {
        Ok(Vec::new())
    }

    // -------------------------------------------------------- Patient registry
    /// OD `disease` rows joined to `diseasedef` for the condition name, surfaced
    /// on the patient registry's clinical record.
    fn list_diseases(&self, pat_num: Id) -> Result<Vec<Disease>, String>;
    /// OD `patientnote` free-text medical/treatment history.
    fn get_patient_note(&self, pat_num: Id) -> Result<Option<PatientNote>, String>;
    /// Existing treatments from OD `procedurelog`.
    fn list_registry_treatments(&self, pat_num: Id) -> Result<Vec<RegistryTreatment>, String>;
    /// Port-local clinic cards. Issuing one never writes a statement, payment
    /// or adjustment, so it cannot create unwanted financial detail.
    fn list_clinic_cards(&self, pat_num: Id) -> Result<Vec<ClinicCard>, String>;
    fn issue_clinic_card(&self, card: &ClinicCard) -> Result<Id, String>;
    /// Cards are deactivated, never deleted, so issuing history survives.
    fn deactivate_clinic_card(&self, card_num: Id) -> Result<(), String>;

    fn list_payments(&self, pat_num: Id) -> Result<Vec<Payment>, String>;
    fn list_adjustments(&self, pat_num: Id) -> Result<Vec<Adjustment>, String>;
    fn list_pay_splits(&self, pat_num: Id) -> Result<Vec<PaySplit>, String>;
    fn list_claims(&self, pat_num: Id) -> Result<Vec<Claim>, String>;
    fn list_claim_procs(&self, pat_num: Id) -> Result<Vec<ClaimProc>, String>;
    fn list_statements(&self, pat_num: Id) -> Result<Vec<Statement>, String>;
    /// Writes an OD `statement` row. `DateSent` is set when the statement is
    /// generated, leaving a dated record for the patient's account history.
    fn insert_statement(&self, statement: &Statement) -> Result<Id, String>;
    /// OD laboratories, for the lab-case picker. Backends without the optional
    /// `laboratory` table return an empty list.
    fn list_laboratories(&self) -> Result<Vec<Laboratory>, String> {
        Ok(Vec::new())
    }
    /// OD `labcase` rows for one patient. Backends without the optional table
    /// return an empty list rather than failing the whole application startup.
    fn list_lab_cases(&self, _pat_num: Id) -> Result<Vec<LabCase>, String> {
        Ok(Vec::new())
    }
    /// Insert or update an OD `labcase` row. A `lab_case_num` of 0 inserts.
    /// The dated created/sent/received/checked columns are written as supplied.
    fn upsert_lab_case(&self, case: &LabCase) -> Result<Id, String>;

    /// OD suppliers, for the supply list. Optional table.
    fn list_suppliers(&self) -> Result<Vec<Supplier>, String> {
        Ok(Vec::new())
    }
    /// OD `supply` rows. Optional table; an absent table yields an empty list.
    fn list_supplies(&self) -> Result<Vec<Supply>, String> {
        Ok(Vec::new())
    }
    /// Recent OD `dispsupply` movements for one supply item, newest first.
    fn list_supply_dispenses(&self, _supply_num: Id) -> Result<Vec<SupplyDispense>, String> {
        Ok(Vec::new())
    }
    /// Record one OD `dispsupply` movement. Supply stock levels are not
    /// recalculated from it, matching OD's deprecated `LevelOnHand` semantics.
    fn record_supply_dispense(&self, dispense: &SupplyDispense) -> Result<Id, String>;

    fn list_pay_plans(&self, pat_num: Id) -> Result<Vec<PayPlan>, String>;
    fn list_pay_plan_charges(&self, pat_num: Id) -> Result<Vec<PayPlanCharge>, String>;
    fn upsert_pay_plan(&self, plan: &PayPlan) -> Result<Id, String>;
    fn upsert_pay_plan_charge(&self, charge: &PayPlanCharge) -> Result<Id, String>;
    /// Delete an OD `payplancharge` row by primary key. Used only for
    /// explicit staff-resolved stale dynamic-plan charge cleanup where no
    /// splits are applied to the charge; never called implicitly.
    fn delete_pay_plan_charge(&self, charge_num: Id) -> Result<(), String>;
    fn upsert_adjustment(&self, adjustment: &Adjustment) -> Result<Id, String>;
    /// Writes the `payment` row and one `paysplit` row linked to it.
    /// `pay_plan_num` is 0 for a one-off payment; otherwise the split credits
    /// that plan via OD's `paysplit.PayPlanNum`.
    fn record_payment(
        &self,
        payment: &Payment,
        proc_num: Id,
        pay_plan_num: Id,
    ) -> Result<Id, String>;
    /// Writes one `payment` row and one `paysplit` row per allocation.
    /// Allocation amounts must sum to `payment.pay_amount` (OD's invariant).
    fn record_payment_splits(
        &self,
        payment: &Payment,
        allocations: &[PaymentAllocation],
    ) -> Result<Id, String>;
    fn list_treat_plans(&self, pat_num: Id) -> Result<Vec<TreatPlan>, String>;
    /// Insert or update an OD `treatplan` row. The caller is responsible for
    /// the one-Active-plan-per-patient rule (`od_core::set_status`).
    fn upsert_treat_plan(&self, plan: &TreatPlan) -> Result<Id, String>;
    /// Delete a plan and its lines. OD refuses to delete a plan that still has
    /// `proctp` rows, so both backends clear the lines first.
    fn delete_treat_plan(&self, plan_num: Id) -> Result<(), String>;
    /// Saved plans read `proctp`; Active/Inactive plans read `treatplanattach`
    /// joined to `procedurelog`. Both are returned as the same domain type.
    fn list_treat_plan_lines(&self, plan_num: Id) -> Result<Vec<TreatPlanLine>, String>;
    /// `plan.status` selects the physical table: `proctp` for Saved,
    /// `treatplanattach` for Active/Inactive.
    fn upsert_treat_plan_line(
        &self,
        plan: &TreatPlan,
        line: &TreatPlanLine,
    ) -> Result<Id, String>;
    fn delete_treat_plan_line(&self, plan: &TreatPlan, line_num: Id) -> Result<(), String>;

    // ------------------------------------------------------ Definitions/fields
    /// All OD `definition` rows, including hidden ones and unknown categories.
    fn list_definitions(&self) -> Result<Vec<Definition>, String>;
    fn upsert_definition(&self, definition: &Definition) -> Result<Id, String>;
    /// OD only permits deletion in a few categories; the UI hides rows instead.
    fn delete_definition(&self, def_num: Id) -> Result<(), String>;
    /// All OD `displayfield` rows across every category and chart view.
    fn list_display_fields(&self) -> Result<Vec<DisplayField>, String>;
    fn upsert_display_field(&self, field: &DisplayField) -> Result<Id, String>;
    /// OD `DisplayFields.SaveListForCategory`: delete the category's rows and
    /// reinsert the list with `ItemOrder = index`. Saving the exact default
    /// list leaves no rows, so OD falls back to its built-in defaults.
    fn replace_display_fields(
        &self,
        category: DisplayFieldCategory,
        fields: &[DisplayField],
    ) -> Result<(), String>;

    fn list_users(&self) -> Result<Vec<Userod>, String>;
    fn list_prefs(&self) -> Result<Vec<Pref>, String>;
    /// Insert or update one OD `preference` row by name, used for persisted
    /// practice state such as closed accounting periods.
    fn upsert_pref(&self, name: &str, value: &str) -> Result<(), String>;

    fn list_group_permissions(&self) -> Result<Vec<GroupPermission>, String>;
    fn list_user_group_attaches(&self) -> Result<Vec<UserGroupAttach>, String>;
    fn list_security_log(&self, limit: usize) -> Result<Vec<SecurityEvent>, String>;
    fn append_security_log(&self, event: &SecurityEvent) -> Result<(), String>;

    // ------------------------------------------------------------ eServices
    /// OD `apptreminderrule` rows.
    fn list_reminder_rules(&self) -> Result<Vec<ReminderRule>, String>;
    fn upsert_reminder_rule(&self, rule: &ReminderRule) -> Result<Id, String>;
    /// Local per-patient channel data (Telegram chat ID, email override, opt-outs).
    fn list_patient_contacts(&self) -> Result<Vec<PatientContact>, String>;
    fn upsert_patient_contact(&self, contact: &PatientContact) -> Result<Id, String>;
    /// OD `apptremindersent` keys: (rule number, appointment number).
    fn list_reminder_sent(&self) -> Result<Vec<(Id, Id)>, String>;
    fn mark_reminder_sent(&self, rule_num: Id, apt_num: Id, pat_num: Id) -> Result<(), String>;
    /// Local delivery outbox — the port's replacement for OD HQ AutoComm records.
    fn insert_outbox(&self, message: &OutboxMessage) -> Result<Id, String>;
    fn list_outbox(&self, limit: usize) -> Result<Vec<OutboxMessage>, String>;
    fn update_outbox(
        &self,
        message_num: Id,
        status: OutboxStatus,
        attempts: u32,
        sent_at: Option<NaiveDateTime>,
        last_error: &str,
    ) -> Result<(), String>;
}

/// Runtime backend selection.
pub enum Backend {
    Demo(DemoStore),
    MySql(MySqlStore),
}

impl Store for Backend {
    fn set_current_user(&self, user_num: Id) {
        match self {
            Backend::Demo(s) => s.set_current_user(user_num),
            Backend::MySql(s) => s.set_current_user(user_num),
        }
    }

    fn current_user(&self) -> Id {
        match self {
            Backend::Demo(s) => s.current_user(),
            Backend::MySql(s) => s.current_user(),
        }
    }

    fn list_patients(&self) -> Result<Vec<Patient>, String> {
        match self {
            Backend::Demo(s) => s.list_patients(),
            Backend::MySql(s) => s.list_patients(),
        }
    }
    fn find_patients(&self, query: &str) -> Result<Vec<Patient>, String> {
        match self {
            Backend::Demo(s) => s.find_patients(query),
            Backend::MySql(s) => s.find_patients(query),
        }
    }
    fn get_patient(&self, pat_num: Id) -> Result<Option<Patient>, String> {
        match self {
            Backend::Demo(s) => s.get_patient(pat_num),
            Backend::MySql(s) => s.get_patient(pat_num),
        }
    }
    fn upsert_patient(&self, patient: &Patient) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_patient(patient),
            Backend::MySql(s) => s.upsert_patient(patient),
        }
    }
    fn delete_patient(&self, pat_num: Id) -> Result<(), String> {
        match self {
            Backend::Demo(s) => s.delete_patient(pat_num),
            Backend::MySql(s) => s.delete_patient(pat_num),
        }
    }
    fn list_appointments(
        &self,
        from: NaiveDateTime,
        to: NaiveDateTime,
    ) -> Result<Vec<Appointment>, String> {
        match self {
            Backend::Demo(s) => s.list_appointments(from, to),
            Backend::MySql(s) => s.list_appointments(from, to),
        }
    }
    fn get_appointment(&self, apt_num: Id) -> Result<Option<Appointment>, String> {
        match self {
            Backend::Demo(s) => s.get_appointment(apt_num),
            Backend::MySql(s) => s.get_appointment(apt_num),
        }
    }
    fn upsert_appointment(&self, apt: &Appointment) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_appointment(apt),
            Backend::MySql(s) => s.upsert_appointment(apt),
        }
    }
    fn delete_appointment(&self, apt_num: Id) -> Result<(), String> {
        match self {
            Backend::Demo(s) => s.delete_appointment(apt_num),
            Backend::MySql(s) => s.delete_appointment(apt_num),
        }
    }
    fn list_waitlist_appointments(&self) -> Result<Vec<Appointment>, String> {
        match self {
            Backend::Demo(s) => s.list_waitlist_appointments(),
            Backend::MySql(s) => s.list_waitlist_appointments(),
        }
    }
    fn list_appointment_changes(&self, apt_num: Id) -> Result<Vec<AppointmentChange>, String> {
        match self {
            Backend::Demo(s) => s.list_appointment_changes(apt_num),
            Backend::MySql(s) => s.list_appointment_changes(apt_num),
        }
    }
    fn append_appointment_change(&self, change: &AppointmentChange) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.append_appointment_change(change),
            Backend::MySql(s) => s.append_appointment_change(change),
        }
    }
    fn list_patient_corrections(&self, pat_num: Id) -> Result<Vec<PatientCorrection>, String> {
        match self {
            Backend::Demo(s) => s.list_patient_corrections(pat_num),
            Backend::MySql(s) => s.list_patient_corrections(pat_num),
        }
    }
    fn append_patient_correction(&self, correction: &PatientCorrection) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.append_patient_correction(correction),
            Backend::MySql(s) => s.append_patient_correction(correction),
        }
    }
    fn list_recalls(&self) -> Result<Vec<Recall>, String> {
        match self {
            Backend::Demo(s) => s.list_recalls(),
            Backend::MySql(s) => s.list_recalls(),
        }
    }
    fn upsert_recall(&self, recall: &Recall) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_recall(recall),
            Backend::MySql(s) => s.upsert_recall(recall),
        }
    }
    fn list_asap_appointments(&self) -> Result<Vec<Appointment>, String> {
        match self {
            Backend::Demo(s) => s.list_asap_appointments(),
            Backend::MySql(s) => s.list_asap_appointments(),
        }
    }
    fn list_providers(&self) -> Result<Vec<Provider>, String> {
        match self {
            Backend::Demo(s) => s.list_providers(),
            Backend::MySql(s) => s.list_providers(),
        }
    }
    fn list_operatories(&self) -> Result<Vec<Operatory>, String> {
        match self {
            Backend::Demo(s) => s.list_operatories(),
            Backend::MySql(s) => s.list_operatories(),
        }
    }
    fn list_clinics(&self) -> Result<Vec<Clinic>, String> {
        match self {
            Backend::Demo(s) => s.list_clinics(),
            Backend::MySql(s) => s.list_clinics(),
        }
    }
    fn list_procedures(&self, pat_num: Id) -> Result<Vec<Procedure>, String> {
        match self {
            Backend::Demo(s) => s.list_procedures(pat_num),
            Backend::MySql(s) => s.list_procedures(pat_num),
        }
    }
    fn insert_procedure(&self, procedure: &Procedure) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.insert_procedure(procedure),
            Backend::MySql(s) => s.insert_procedure(procedure),
        }
    }
    fn set_procedure_complete(&self, proc_num: Id, discount_amount: f64) -> Result<(), String> {
        match self {
            Backend::Demo(s) => s.set_procedure_complete(proc_num, discount_amount),
            Backend::MySql(s) => s.set_procedure_complete(proc_num, discount_amount),
        }
    }
    fn list_procedure_codes(&self) -> Result<Vec<ProcedureCode>, String> {
        match self {
            Backend::Demo(s) => s.list_procedure_codes(),
            Backend::MySql(s) => s.list_procedure_codes(),
        }
    }
    fn list_fee_schedules(&self) -> Result<Vec<FeeSchedule>, String> {
        match self {
            Backend::Demo(s) => s.list_fee_schedules(),
            Backend::MySql(s) => s.list_fee_schedules(),
        }
    }
    fn list_fees(&self, schedule_num: Id, provider_num: Id) -> Result<Vec<Fee>, String> {
        match self {
            Backend::Demo(s) => s.list_fees(schedule_num, provider_num),
            Backend::MySql(s) => s.list_fees(schedule_num, provider_num),
        }
    }
    fn list_prescriptions(&self) -> Result<Vec<Prescription>, String> {
        match self {
            Backend::Demo(s) => s.list_prescriptions(),
            Backend::MySql(s) => s.list_prescriptions(),
        }
    }
    fn list_referrals(&self) -> Result<Vec<Referral>, String> {
        match self {
            Backend::Demo(s) => s.list_referrals(),
            Backend::MySql(s) => s.list_referrals(),
        }
    }
    fn list_refattaches(&self) -> Result<Vec<RefAttach>, String> {
        match self {
            Backend::Demo(s) => s.list_refattaches(),
            Backend::MySql(s) => s.list_refattaches(),
        }
    }
    fn list_proc_notes(&self) -> Result<Vec<ProcNote>, String> {
        match self {
            Backend::Demo(s) => s.list_proc_notes(),
            Backend::MySql(s) => s.list_proc_notes(),
        }
    }
    fn append_proc_note(&self, note: &ProcNote) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.append_proc_note(note),
            Backend::MySql(s) => s.append_proc_note(note),
        }
    }
    fn list_comm_logs(&self) -> Result<Vec<CommLog>, String> {
        match self {
            Backend::Demo(s) => s.list_comm_logs(),
            Backend::MySql(s) => s.list_comm_logs(),
        }
    }
    fn upsert_comm_log(&self, log: &CommLog) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_comm_log(log),
            Backend::MySql(s) => s.upsert_comm_log(log),
        }
    }
    fn list_accounts(&self) -> Result<Vec<Account>, String> {
        match self {
            Backend::Demo(s) => s.list_accounts(),
            Backend::MySql(s) => s.list_accounts(),
        }
    }
    fn list_journal_entries(&self) -> Result<Vec<JournalEntry>, String> {
        match self {
            Backend::Demo(s) => s.list_journal_entries(),
            Backend::MySql(s) => s.list_journal_entries(),
        }
    }
    fn post_journal_transaction(&self, entries: &[JournalEntry]) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.post_journal_transaction(entries),
            Backend::MySql(s) => s.post_journal_transaction(entries),
        }
    }
    fn post_journal_batch(&self, batches: &[Vec<JournalEntry>]) -> Result<Vec<Id>, String> {
        match self {
            Backend::Demo(s) => s.post_journal_batch(batches),
            Backend::MySql(s) => s.post_journal_batch(batches),
        }
    }

    fn list_schedules(&self) -> Result<Vec<Schedule>, String> {
        match self {
            Backend::Demo(s) => s.list_schedules(),
            Backend::MySql(s) => s.list_schedules(),
        }
    }
    fn list_schedule_ops(&self) -> Result<Vec<ScheduleOp>, String> {
        match self {
            Backend::Demo(s) => s.list_schedule_ops(),
            Backend::MySql(s) => s.list_schedule_ops(),
        }
    }
    fn list_pay_plan_links(&self) -> Result<Vec<PayPlanLink>, String> {
        match self {
            Backend::Demo(s) => s.list_pay_plan_links(),
            Backend::MySql(s) => s.list_pay_plan_links(),
        }
    }
    fn upsert_pay_plan_link(&self, link: &PayPlanLink) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_pay_plan_link(link),
            Backend::MySql(s) => s.upsert_pay_plan_link(link),
        }
    }
    fn delete_pay_plan_link(&self, link_num: Id) -> Result<(), String> {
        match self {
            Backend::Demo(s) => s.delete_pay_plan_link(link_num),
            Backend::MySql(s) => s.delete_pay_plan_link(link_num),
        }
    }
    fn list_claim_snapshots(&self) -> Result<Vec<ClaimSnapshot>, String> {
        match self {
            Backend::Demo(s) => s.list_claim_snapshots(),
            Backend::MySql(s) => s.list_claim_snapshots(),
        }
    }
    fn list_discount_plans(&self) -> Result<Vec<DiscountPlan>, String> {
        match self {
            Backend::Demo(s) => s.list_discount_plans(),
            Backend::MySql(s) => s.list_discount_plans(),
        }
    }
    fn list_discount_plan_subs(&self) -> Result<Vec<DiscountPlanSub>, String> {
        match self {
            Backend::Demo(s) => s.list_discount_plan_subs(),
            Backend::MySql(s) => s.list_discount_plan_subs(),
        }
    }
    fn upsert_discount_plan(&self, plan: &DiscountPlan) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_discount_plan(plan),
            Backend::MySql(s) => s.upsert_discount_plan(plan),
        }
    }
    fn upsert_discount_plan_sub(&self, sub: &DiscountPlanSub) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_discount_plan_sub(sub),
            Backend::MySql(s) => s.upsert_discount_plan_sub(sub),
        }
    }
    fn terminate_discount_plan_sub(
        &self,
        discount_sub_num: Id,
        term: NaiveDate,
    ) -> Result<(), String> {
        match self {
            Backend::Demo(s) => s.terminate_discount_plan_sub(discount_sub_num, term),
            Backend::MySql(s) => s.terminate_discount_plan_sub(discount_sub_num, term),
        }
    }
    fn list_screen_groups(&self) -> Result<Vec<ScreenGroup>, String> {
        match self {
            Backend::Demo(s) => s.list_screen_groups(),
            Backend::MySql(s) => s.list_screen_groups(),
        }
    }
    fn list_screens(&self) -> Result<Vec<Screen>, String> {
        match self {
            Backend::Demo(s) => s.list_screens(),
            Backend::MySql(s) => s.list_screens(),
        }
    }
    fn list_sites(&self) -> Result<Vec<Site>, String> {
        match self {
            Backend::Demo(s) => s.list_sites(),
            Backend::MySql(s) => s.list_sites(),
        }
    }
    fn list_counties(&self) -> Result<Vec<County>, String> {
        match self {
            Backend::Demo(s) => s.list_counties(),
            Backend::MySql(s) => s.list_counties(),
        }
    }
    fn list_patient_races(&self) -> Result<Vec<PatientRace>, String> {
        match self {
            Backend::Demo(s) => s.list_patient_races(),
            Backend::MySql(s) => s.list_patient_races(),
        }
    }
    fn list_carriers(&self) -> Result<Vec<Carrier>, String> {
        match self {
            Backend::Demo(s) => s.list_carriers(),
            Backend::MySql(s) => s.list_carriers(),
        }
    }

    fn list_diseases(&self, pat_num: Id) -> Result<Vec<Disease>, String> {
        match self {
            Backend::Demo(s) => s.list_diseases(pat_num),
            Backend::MySql(s) => s.list_diseases(pat_num),
        }
    }
    fn get_patient_note(&self, pat_num: Id) -> Result<Option<PatientNote>, String> {
        match self {
            Backend::Demo(s) => s.get_patient_note(pat_num),
            Backend::MySql(s) => s.get_patient_note(pat_num),
        }
    }
    fn list_registry_treatments(&self, pat_num: Id) -> Result<Vec<RegistryTreatment>, String> {
        match self {
            Backend::Demo(s) => s.list_registry_treatments(pat_num),
            Backend::MySql(s) => s.list_registry_treatments(pat_num),
        }
    }
    fn list_clinic_cards(&self, pat_num: Id) -> Result<Vec<ClinicCard>, String> {
        match self {
            Backend::Demo(s) => s.list_clinic_cards(pat_num),
            Backend::MySql(s) => s.list_clinic_cards(pat_num),
        }
    }
    fn issue_clinic_card(&self, card: &ClinicCard) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.issue_clinic_card(card),
            Backend::MySql(s) => s.issue_clinic_card(card),
        }
    }
    fn deactivate_clinic_card(&self, card_num: Id) -> Result<(), String> {
        match self {
            Backend::Demo(s) => s.deactivate_clinic_card(card_num),
            Backend::MySql(s) => s.deactivate_clinic_card(card_num),
        }
    }
    fn list_payments(&self, pat_num: Id) -> Result<Vec<Payment>, String> {
        match self {
            Backend::Demo(s) => s.list_payments(pat_num),
            Backend::MySql(s) => s.list_payments(pat_num),
        }
    }
    fn list_adjustments(&self, pat_num: Id) -> Result<Vec<Adjustment>, String> {
        match self {
            Backend::Demo(s) => s.list_adjustments(pat_num),
            Backend::MySql(s) => s.list_adjustments(pat_num),
        }
    }
    fn list_pay_splits(&self, pat_num: Id) -> Result<Vec<PaySplit>, String> {
        match self {
            Backend::Demo(s) => s.list_pay_splits(pat_num),
            Backend::MySql(s) => s.list_pay_splits(pat_num),
        }
    }
    fn list_claims(&self, pat_num: Id) -> Result<Vec<Claim>, String> {
        match self {
            Backend::Demo(s) => s.list_claims(pat_num),
            Backend::MySql(s) => s.list_claims(pat_num),
        }
    }
    fn list_claim_procs(&self, pat_num: Id) -> Result<Vec<ClaimProc>, String> {
        match self {
            Backend::Demo(s) => s.list_claim_procs(pat_num),
            Backend::MySql(s) => s.list_claim_procs(pat_num),
        }
    }
    fn list_statements(&self, pat_num: Id) -> Result<Vec<Statement>, String> {
        match self {
            Backend::Demo(s) => s.list_statements(pat_num),
            Backend::MySql(s) => s.list_statements(pat_num),
        }
    }
    fn insert_statement(&self, statement: &Statement) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.insert_statement(statement),
            Backend::MySql(s) => s.insert_statement(statement),
        }
    }
    fn list_laboratories(&self) -> Result<Vec<Laboratory>, String> {
        match self {
            Backend::Demo(s) => s.list_laboratories(),
            Backend::MySql(s) => s.list_laboratories(),
        }
    }
    fn list_lab_cases(&self, pat_num: Id) -> Result<Vec<LabCase>, String> {
        match self {
            Backend::Demo(s) => s.list_lab_cases(pat_num),
            Backend::MySql(s) => s.list_lab_cases(pat_num),
        }
    }
    fn upsert_lab_case(&self, case: &LabCase) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_lab_case(case),
            Backend::MySql(s) => s.upsert_lab_case(case),
        }
    }
    fn list_suppliers(&self) -> Result<Vec<Supplier>, String> {
        match self {
            Backend::Demo(s) => s.list_suppliers(),
            Backend::MySql(s) => s.list_suppliers(),
        }
    }
    fn list_supplies(&self) -> Result<Vec<Supply>, String> {
        match self {
            Backend::Demo(s) => s.list_supplies(),
            Backend::MySql(s) => s.list_supplies(),
        }
    }
    fn list_supply_dispenses(&self, supply_num: Id) -> Result<Vec<SupplyDispense>, String> {
        match self {
            Backend::Demo(s) => s.list_supply_dispenses(supply_num),
            Backend::MySql(s) => s.list_supply_dispenses(supply_num),
        }
    }
    fn record_supply_dispense(&self, dispense: &SupplyDispense) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.record_supply_dispense(dispense),
            Backend::MySql(s) => s.record_supply_dispense(dispense),
        }
    }
    fn upsert_adjustment(&self, adjustment: &Adjustment) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_adjustment(adjustment),
            Backend::MySql(s) => s.upsert_adjustment(adjustment),
        }
    }
    fn record_payment(
        &self,
        payment: &Payment,
        proc_num: Id,
        pay_plan_num: Id,
    ) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.record_payment(payment, proc_num, pay_plan_num),
            Backend::MySql(s) => s.record_payment(payment, proc_num, pay_plan_num),
        }
    }
    fn record_payment_splits(
        &self,
        payment: &Payment,
        allocations: &[PaymentAllocation],
    ) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.record_payment_splits(payment, allocations),
            Backend::MySql(s) => s.record_payment_splits(payment, allocations),
        }
    }
    fn list_pay_plans(&self, pat_num: Id) -> Result<Vec<PayPlan>, String> {
        match self {
            Backend::Demo(s) => s.list_pay_plans(pat_num),
            Backend::MySql(s) => s.list_pay_plans(pat_num),
        }
    }
    fn list_pay_plan_charges(&self, pat_num: Id) -> Result<Vec<PayPlanCharge>, String> {
        match self {
            Backend::Demo(s) => s.list_pay_plan_charges(pat_num),
            Backend::MySql(s) => s.list_pay_plan_charges(pat_num),
        }
    }
    fn upsert_pay_plan(&self, plan: &PayPlan) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_pay_plan(plan),
            Backend::MySql(s) => s.upsert_pay_plan(plan),
        }
    }
    fn upsert_pay_plan_charge(&self, charge: &PayPlanCharge) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_pay_plan_charge(charge),
            Backend::MySql(s) => s.upsert_pay_plan_charge(charge),
        }
    }
    fn delete_pay_plan_charge(&self, charge_num: Id) -> Result<(), String> {
        match self {
            Backend::Demo(s) => s.delete_pay_plan_charge(charge_num),
            Backend::MySql(s) => s.delete_pay_plan_charge(charge_num),
        }
    }
    fn list_treat_plans(&self, pat_num: Id) -> Result<Vec<TreatPlan>, String> {
        match self {
            Backend::Demo(s) => s.list_treat_plans(pat_num),
            Backend::MySql(s) => s.list_treat_plans(pat_num),
        }
    }
    fn upsert_treat_plan(&self, plan: &TreatPlan) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_treat_plan(plan),
            Backend::MySql(s) => s.upsert_treat_plan(plan),
        }
    }
    fn delete_treat_plan(&self, plan_num: Id) -> Result<(), String> {
        match self {
            Backend::Demo(s) => s.delete_treat_plan(plan_num),
            Backend::MySql(s) => s.delete_treat_plan(plan_num),
        }
    }
    fn list_treat_plan_lines(&self, plan_num: Id) -> Result<Vec<TreatPlanLine>, String> {
        match self {
            Backend::Demo(s) => s.list_treat_plan_lines(plan_num),
            Backend::MySql(s) => s.list_treat_plan_lines(plan_num),
        }
    }
    fn upsert_treat_plan_line(
        &self,
        plan: &TreatPlan,
        line: &TreatPlanLine,
    ) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_treat_plan_line(plan, line),
            Backend::MySql(s) => s.upsert_treat_plan_line(plan, line),
        }
    }
    fn delete_treat_plan_line(&self, plan: &TreatPlan, line_num: Id) -> Result<(), String> {
        match self {
            Backend::Demo(s) => s.delete_treat_plan_line(plan, line_num),
            Backend::MySql(s) => s.delete_treat_plan_line(plan, line_num),
        }
    }
    fn list_definitions(&self) -> Result<Vec<Definition>, String> {
        match self {
            Backend::Demo(s) => s.list_definitions(),
            Backend::MySql(s) => s.list_definitions(),
        }
    }
    fn upsert_definition(&self, definition: &Definition) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_definition(definition),
            Backend::MySql(s) => s.upsert_definition(definition),
        }
    }
    fn delete_definition(&self, def_num: Id) -> Result<(), String> {
        match self {
            Backend::Demo(s) => s.delete_definition(def_num),
            Backend::MySql(s) => s.delete_definition(def_num),
        }
    }
    fn list_display_fields(&self) -> Result<Vec<DisplayField>, String> {
        match self {
            Backend::Demo(s) => s.list_display_fields(),
            Backend::MySql(s) => s.list_display_fields(),
        }
    }
    fn upsert_display_field(&self, field: &DisplayField) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_display_field(field),
            Backend::MySql(s) => s.upsert_display_field(field),
        }
    }
    fn replace_display_fields(
        &self,
        category: DisplayFieldCategory,
        fields: &[DisplayField],
    ) -> Result<(), String> {
        match self {
            Backend::Demo(s) => s.replace_display_fields(category, fields),
            Backend::MySql(s) => s.replace_display_fields(category, fields),
        }
    }
    fn list_users(&self) -> Result<Vec<Userod>, String> {
        match self {
            Backend::Demo(s) => s.list_users(),
            Backend::MySql(s) => s.list_users(),
        }
    }
    fn list_prefs(&self) -> Result<Vec<Pref>, String> {
        match self {
            Backend::Demo(s) => s.list_prefs(),
            Backend::MySql(s) => s.list_prefs(),
        }
    }
    fn upsert_pref(&self, name: &str, value: &str) -> Result<(), String> {
        match self {
            Backend::Demo(s) => s.upsert_pref(name, value),
            Backend::MySql(s) => s.upsert_pref(name, value),
        }
    }
    fn list_group_permissions(&self) -> Result<Vec<GroupPermission>, String> {
        match self {
            Backend::Demo(s) => s.list_group_permissions(),
            Backend::MySql(s) => s.list_group_permissions(),
        }
    }
    fn list_user_group_attaches(&self) -> Result<Vec<UserGroupAttach>, String> {
        match self {
            Backend::Demo(s) => s.list_user_group_attaches(),
            Backend::MySql(s) => s.list_user_group_attaches(),
        }
    }
    fn list_security_log(&self, limit: usize) -> Result<Vec<SecurityEvent>, String> {
        match self {
            Backend::Demo(s) => s.list_security_log(limit),
            Backend::MySql(s) => s.list_security_log(limit),
        }
    }
    fn append_security_log(&self, event: &SecurityEvent) -> Result<(), String> {
        match self {
            Backend::Demo(s) => s.append_security_log(event),
            Backend::MySql(s) => s.append_security_log(event),
        }
    }
    fn list_reminder_rules(&self) -> Result<Vec<ReminderRule>, String> {
        match self {
            Backend::Demo(s) => s.list_reminder_rules(),
            Backend::MySql(s) => s.list_reminder_rules(),
        }
    }
    fn upsert_reminder_rule(&self, rule: &ReminderRule) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_reminder_rule(rule),
            Backend::MySql(s) => s.upsert_reminder_rule(rule),
        }
    }
    fn list_patient_contacts(&self) -> Result<Vec<PatientContact>, String> {
        match self {
            Backend::Demo(s) => s.list_patient_contacts(),
            Backend::MySql(s) => s.list_patient_contacts(),
        }
    }
    fn upsert_patient_contact(&self, contact: &PatientContact) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.upsert_patient_contact(contact),
            Backend::MySql(s) => s.upsert_patient_contact(contact),
        }
    }
    fn list_reminder_sent(&self) -> Result<Vec<(Id, Id)>, String> {
        match self {
            Backend::Demo(s) => s.list_reminder_sent(),
            Backend::MySql(s) => s.list_reminder_sent(),
        }
    }
    fn mark_reminder_sent(&self, rule_num: Id, apt_num: Id, pat_num: Id) -> Result<(), String> {
        match self {
            Backend::Demo(s) => s.mark_reminder_sent(rule_num, apt_num, pat_num),
            Backend::MySql(s) => s.mark_reminder_sent(rule_num, apt_num, pat_num),
        }
    }
    fn insert_outbox(&self, message: &OutboxMessage) -> Result<Id, String> {
        match self {
            Backend::Demo(s) => s.insert_outbox(message),
            Backend::MySql(s) => s.insert_outbox(message),
        }
    }
    fn list_outbox(&self, limit: usize) -> Result<Vec<OutboxMessage>, String> {
        match self {
            Backend::Demo(s) => s.list_outbox(limit),
            Backend::MySql(s) => s.list_outbox(limit),
        }
    }
    fn update_outbox(
        &self,
        message_num: Id,
        status: OutboxStatus,
        attempts: u32,
        sent_at: Option<NaiveDateTime>,
        last_error: &str,
    ) -> Result<(), String> {
        match self {
            Backend::Demo(s) => s.update_outbox(message_num, status, attempts, sent_at, last_error),
            Backend::MySql(s) => s.update_outbox(message_num, status, attempts, sent_at, last_error),
        }
    }
}
