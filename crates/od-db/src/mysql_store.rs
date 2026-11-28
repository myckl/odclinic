//! MySQL backend speaking the Open Dental table/column naming convention.
//!
//! Connect with an ODBC-style URL in `ODCLINIC_DB_URL`, for example:
//! `mysql://od_user:secret@localhost:3306/opendental`

use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime};
use mysql::{Opts, Pool, PooledConn, Value, from_row, prelude::Queryable};
use od_domain::definitions::{
    default_display_fields, Definition, DefinitionCategory, DisplayField, DisplayFieldCategory,
};
use od_domain::eservices::*;
use od_domain::*;

use crate::Store;

/// How startup treats schema differences.
///
/// `Strict` is the plug-and-play default: it verifies the Open Dental 24_3
/// objects this port needs and never issues DDL. `BootstrapPortLocal` is an
/// explicit opt-in for demo databases that still need the four `odclinic_*`
/// side tables; it never creates or alters an OD-owned table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemaMode {
    Strict,
    BootstrapPortLocal,
}

/// Whether writes to OD-owned tables are permitted.
///
/// OD verifies `patient`, `appointment`, `paysplit`, `payplan` and other rows
/// with a salted `SecurityHash` produced by the obfuscated `CDT.dll`. That
/// algorithm cannot be reproduced or independently verified from the open
/// source tree, so writing those tables without it would create rows OD itself
/// flags as tampered. The default is `DenyOdWrites`; `ODCLINIC_ALLOW_OD_WRITES=1`
/// is an explicit operator opt-in that accepts that risk for an otherwise
/// compatible schema. Port-local `odclinic_*` tables are never gated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WritePolicy {
    DenyOdWrites,
    AllowOdWrites,
}

impl WritePolicy {
    fn from_env() -> Self {
        match std::env::var("ODCLINIC_ALLOW_OD_WRITES")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "1" | "true" | "yes" | "on" => Self::AllowOdWrites,
            _ => Self::DenyOdWrites,
        }
    }
}

pub struct MySqlStore {
    pool: Pool,
    current_user: AtomicU64,
    write_policy: WritePolicy,
}

impl MySqlStore {
    /// Connect to an Open Dental database without changing its schema.
    /// OD-owned writes are denied unless explicitly enabled in the environment.
    pub fn new(url: &str) -> Result<Self, String> {
        Self::with_policy(url, SchemaMode::Strict, WritePolicy::from_env())
    }

    pub fn with_mode(url: &str, mode: SchemaMode) -> Result<Self, String> {
        Self::with_policy(url, mode, WritePolicy::from_env())
    }

    /// Explicit policy constructor. The compatibility harness uses `AllowOdWrites`
    /// for temporary fixtures; the application uses the environment-derived
    /// default, which fails closed.
    pub fn with_policy(
        url: &str,
        mode: SchemaMode,
        write_policy: WritePolicy,
    ) -> Result<Self, String> {
        // Accept the common shorthand without a scheme, e.g.
        // `user:pass@localhost:3306/opendental`.
        let normalized = if url.contains("://") {
            url.trim().to_string()
        } else {
            format!("mysql://{}", url.trim())
        };
        let opts = Opts::from_url(&normalized).map_err(|e| format!("invalid ODCLINIC_DB_URL: {e}"))?;
        let pool = Pool::new(opts).map_err(|e| format!("could not create MySQL pool: {e}"))?;
        // Fail fast so a bad connection is reported at startup, not at first click.
        let mut conn = pool.get_conn().map_err(|e| format!("MySQL connect failed: {e}"))?;
        conn.query_drop("SELECT 1")
            .map_err(|e| format!("MySQL ping failed: {e}"))?;
        if mode == SchemaMode::BootstrapPortLocal {
            ensure_port_local_tables(&mut conn)?;
        }
        verify_schema(&mut conn)?;
        Ok(Self {
            pool,
            current_user: AtomicU64::new(0),
            write_policy,
        })
    }

    fn conn(&self) -> Result<PooledConn, String> {
        self.pool.get_conn().map_err(|e| format!("MySQL connect failed: {e}"))
    }

    fn ensure_od_writes_allowed(&self) -> Result<(), String> {
        if self.write_policy == WritePolicy::DenyOdWrites {
            return Err("OD writes are disabled: this build cannot reproduce Open Dental's SecurityHash. Set ODCLINIC_ALLOW_OD_WRITES=1 only when you accept rows OD may flag as tampered.".into());
        }
        Ok(())
    }
}

/// OD 24_3 objects that this port reads or writes unconditionally.
/// A genuine Open Dental schema has all of them; a partial demo schema does not.
const OD_TABLES: &[&str] = &[
    "account", "adjustment", "appointment", "apptreminderrule", "apptremindersent", "carrier",
    "claim", "claimproc", "claimsnapshot", "clinic", "county", "definition", "disease",
    "diseasedef", "displayfield", "discountplan", "discountplansub", "fee", "feesched",
    "grouppermission", "histappointment", "operatory", "patient", "patientnote", "patientrace",
    "payment", "payplan", "payplancharge", "payplanlink", "paysplit", "preference",
    "procedurecode", "procedurelog", "procnote", "proctp", "provider", "recall", "referral",
    "refattach", "rxpat", "schedule", "scheduleop", "screen", "screengroup", "securitylog",
    "site", "statement", "treatplan", "treatplanattach", "userod", "usergroupattach",
];

/// Columns whose absence would make a current query fail outright. Optional
/// OD-semantic columns added by later writes are handled at write time instead.
const OD_COLUMNS: &[(&str, &str)] = &[
    ("patient", "BillingType"),
    ("patient", "ChartNumber"),
    ("patient", "SSN"),
    ("patient", "MedicaidID"),
    ("patient", "PatStatus"),
    ("patient", "Guarantor"),
    ("patient", "County"),
    ("patient", "SiteNum"),
    ("patient", "GradeLevel"),
    ("patient", "Urgency"),
    ("patient", "PlannedIsDone"),
    ("patient", "WkPhone"),
    ("patient", "Address2"),
    ("appointment", "Priority"),
    ("appointment", "UnschedStatus"),
    ("histappointment", "HistUserNum"),
    ("histappointment", "HistDateTStamp"),
    ("histappointment", "HistApptAction"),
    ("histappointment", "UnschedStatus"),
    ("histappointment", "Priority"),
    ("disease", "PatNote"),
    ("disease", "ProbStatus"),
    ("definition", "ItemColor"),
    ("payplan", "PlanNum"),
    ("payplan", "DateInterestStart"),
    ("payplan", "DynamicPayPlanTPOption"),
    ("payplancharge", "FKey"),
    ("payplancharge", "LinkType"),
    ("paysplit", "PayPlanChargeNum"),
    ("paysplit", "PayPlanDebitType"),
    ("paysplit", "ProcDate"),
    ("recall", "RecallTypeNum"),
    ("recall", "DisableUntilBalance"),
    ("recall", "DisableUntilDate"),
    ("recall", "Priority"),
    ("recall", "TimePatternOverride"),
    ("apptreminderrule", "SendOrder"),
    ("treatplan", "DateTP"),
    ("treatplan", "Heading"),
    ("treatplan", "Note"),
    ("treatplan", "Signature"),
    ("treatplan", "SigIsTopaz"),
    ("treatplan", "ResponsParty"),
    ("treatplan", "DocNum"),
    ("treatplan", "TPStatus"),
    ("treatplan", "SecUserNumEntry"),
    ("treatplan", "SecDateEntry"),
    ("treatplan", "UserNumPresenter"),
    ("treatplan", "TPType"),
    ("treatplan", "SignaturePractice"),
    ("treatplan", "DateTSigned"),
    ("treatplan", "DateTPracticeSigned"),
    ("treatplan", "SignatureText"),
    ("treatplan", "SignaturePracticeText"),
    ("treatplan", "MobileAppDeviceNum"),
    ("proctp", "ProcNumOrig"),
    ("proctp", "ItemOrder"),
    ("proctp", "Priority"),
    ("proctp", "ToothNumTP"),
    ("proctp", "Surf"),
    ("proctp", "ProcCode"),
    ("proctp", "Descript"),
    ("proctp", "PriInsAmt"),
    ("proctp", "SecInsAmt"),
    ("proctp", "PatAmt"),
    ("proctp", "Discount"),
    ("proctp", "Prognosis"),
    ("proctp", "Dx"),
    ("proctp", "ProcAbbr"),
    ("proctp", "SecUserNumEntry"),
    ("proctp", "SecDateEntry"),
    ("proctp", "FeeAllowed"),
    ("proctp", "TaxAmt"),
    ("proctp", "ProvNum"),
    ("proctp", "DateTP"),
    ("proctp", "ClinicNum"),
    ("proctp", "CatPercUCR"),
    ("treatplanattach", "TreatPlanNum"),
    ("treatplanattach", "ProcNum"),
    ("treatplanattach", "Priority"),
];

/// True when an OD column exists. Used by write paths that can safely omit
/// optional OD-semantic columns on the older demo schema.
pub(crate) fn column_exists(
    conn: &mut PooledConn,
    table: &str,
    column: &str,
) -> Result<bool, String> {
    let found: Option<i64> = conn
        .exec_first(
            "SELECT COUNT(*) FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? AND COLUMN_NAME = ?",
            (table, column),
        )
        .map_err(|e| format!("could not inspect {table}.{column}: {e}"))?;
    Ok(found.unwrap_or(0) > 0)
}

/// True when an optional OD table is present. Lab-case and inventory tables are
/// not part of `OD_TABLES`, so a reduced demo schema can omit them without
/// failing strict startup; their views simply stay empty.
fn table_exists(conn: &mut PooledConn, table: &str) -> Result<bool, String> {
    let found: Option<i64> = conn
        .exec_first(
            "SELECT COUNT(*) FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?",
            (table,),
        )
        .map_err(|e| format!("could not inspect table {table}: {e}"))?;
    Ok(found.unwrap_or(0) > 0)
}

fn verify_schema(conn: &mut PooledConn) -> Result<(), String> {
    let mut missing_tables = Vec::new();
    for table in OD_TABLES {
        let found: Option<i64> = conn
            .exec_first(
                "SELECT COUNT(*) FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?",
                (table,),
            )
            .map_err(|e| format!("could not inspect table {table}: {e}"))?;
        if found.unwrap_or(0) == 0 {
            missing_tables.push((*table).to_string());
        }
    }
    let mut missing_columns = Vec::new();
    for (table, column) in OD_COLUMNS {
        if !column_exists(conn, table, column)? {
            missing_columns.push(format!("{table}.{column}"));
        }
    }
    if missing_tables.is_empty() && missing_columns.is_empty() {
        return Ok(());
    }
    let mut message = String::from("OD 24_3 schema is incompatible; odclinic will not start against a guessed schema.");
    if !missing_tables.is_empty() {
        message.push_str(&format!(" Missing tables: {}.", missing_tables.join(", ")));
    }
    if !missing_columns.is_empty() {
        message.push_str(&format!(" Missing columns: {}.", missing_columns.join(", ")));
    }
    message.push_str(" OD-owned tables must come from the Open Dental schema; odclinic never creates or alters them.");
    Err(message)
}

/// Create only port-local side tables, and only after explicit opt-in. OD-owned
/// tables are never created or altered here.
fn ensure_port_local_tables(conn: &mut PooledConn) -> Result<(), String> {
    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS odclinic_patient_contact (\
         PatNum bigint NOT NULL PRIMARY KEY,\
         TelegramChatId varchar(255) NOT NULL DEFAULT '',\
         EmailOverride varchar(255) NOT NULL DEFAULT '',\
         OptOutEmail tinyint NOT NULL DEFAULT 0,\
         OptOutTelegram tinyint NOT NULL DEFAULT 0)",
    )
    .map_err(|e| format!("could not create patient contact table: {e}"))?;
    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS odclinic_message_outbox (\
         MessageNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,\
         PatNum bigint NOT NULL DEFAULT 0,\
         AptNum bigint NOT NULL DEFAULT 0,\
         RuleNum bigint NOT NULL DEFAULT 0,\
         ReminderType tinyint NOT NULL DEFAULT 0,\
         Transport tinyint NOT NULL DEFAULT 0,\
         Recipient varchar(511) NOT NULL DEFAULT '',\
         Subject text NOT NULL,\
         Body text NOT NULL,\
         Status tinyint NOT NULL DEFAULT 0,\
         Attempts int NOT NULL DEFAULT 0,\
         CreatedAt datetime NOT NULL DEFAULT CURRENT_TIMESTAMP,\
         SentAt datetime NULL DEFAULT NULL,\
         LastError text NOT NULL,\
         KEY (Status), KEY (PatNum), KEY (AptNum))",
    )
    .map_err(|e| format!("could not create local message outbox: {e}"))?;
    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS odclinic_reminder_sent (\
         SentNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,\
         ApptReminderRuleNum bigint NOT NULL DEFAULT 0,\
         ApptNum bigint NOT NULL DEFAULT 0,\
         PatNum bigint NOT NULL DEFAULT 0,\
         SentAt datetime NOT NULL DEFAULT CURRENT_TIMESTAMP,\
         UNIQUE KEY (ApptReminderRuleNum, ApptNum), KEY (PatNum))",
    )
    .map_err(|e| format!("could not create reminder dedupe table: {e}"))?;
    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS odclinic_appointment_call_log (\
         CallLogNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,\
         AptNum bigint NOT NULL DEFAULT 0,\
         PatNum bigint NOT NULL DEFAULT 0,\
         CommDateTime datetime NOT NULL,\
         CommType bigint NOT NULL DEFAULT 0,\
         CommMode tinyint NOT NULL DEFAULT 0,\
         SentOrReceived tinyint NOT NULL DEFAULT 0,\
         Note text NOT NULL,\
         UserNum bigint NOT NULL DEFAULT 0,\
         KEY (AptNum), KEY (PatNum))",
    )
    .map_err(|e| format!("could not create appointment call log table: {e}"))?;
    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS odclinic_patient_correction (\
         CorrectionNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,\
         PatNum bigint NOT NULL DEFAULT 0,\
         CorrectedAt datetime NOT NULL,\
         UserNum bigint NOT NULL DEFAULT 0,\
         Reason varchar(500) NOT NULL DEFAULT '',\
         BeforeData text NOT NULL,\
         KEY (PatNum))",
    )
    .map_err(|e| format!("could not create patient correction table: {e}"))?;
    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS odclinic_clinic_card (\
         CardNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,\
         PatNum bigint NOT NULL DEFAULT 0,\
         CardNumber varchar(64) NOT NULL DEFAULT '',\
         IssuedAt date NOT NULL,\
         IssuedBy bigint NOT NULL DEFAULT 0,\
         Reason varchar(32) NOT NULL DEFAULT 'corporate_account',\
         Authorization varchar(128) NOT NULL DEFAULT '',\
         Note varchar(500) NOT NULL DEFAULT '',\
         IsActive tinyint NOT NULL DEFAULT 1,\
         KEY (PatNum))",
    )
    .map_err(|e| format!("could not create clinic card table: {e}"))?;
    Ok(())
}

/// OD requires `patient.BillingType` to reference a visible billing-type
/// definition (DefCat 4). Keep an existing valid value, then fall back to the
/// first visible definition rather than inventing a new definition row.
fn first_visible_definition(conn: &mut PooledConn, category: i32) -> Result<Id, String> {
    let def_num: Option<i64> = conn
        .exec_first(
            "SELECT MIN(DefNum) FROM definition WHERE Category = ? AND IsHidden = 0",
            (category,),
        )
        .map_err(|e| format!("could not read definition category {category}: {e}"))?;
    Ok(def_num.filter(|n| *n > 0).unwrap_or(0) as Id)
}

fn effective_billing_type(conn: &mut PooledConn, patient: &Patient) -> Result<Id, String> {
    if patient.billing_type != 0 {
        return Ok(patient.billing_type);
    }
    if patient.pat_num != 0 {
        let existing: Option<i64> = conn
            .exec_first(
                "SELECT BillingType FROM patient WHERE PatNum = ?",
                (patient.pat_num,),
            )
            .map_err(|e| format!("could not read existing BillingType: {e}"))?;
        if let Some(value) = existing {
            if value > 0 {
                return Ok(value as Id);
            }
        }
    }
    first_visible_definition(conn, 4)
}

/// Use the patient's OD clinic for writes instead of hardcoding clinic 0. A
/// missing patient resolves to 0, which is OD's Unassigned clinic.
fn patient_clinic_num(conn: &mut PooledConn, pat_num: Id) -> Result<Id, String> {
    if pat_num == 0 {
        return Ok(0);
    }
    let clinic_num: Option<i64> = conn
        .exec_first(
            "SELECT ClinicNum FROM patient WHERE PatNum = ?",
            (pat_num,),
        )
        .map_err(|e| format!("could not read patient clinic: {e}"))?;
    Ok(clinic_num.unwrap_or(0) as Id)
}

fn procedure_clinic_num(conn: &mut PooledConn, proc_num: Id) -> Result<Id, String> {
    if proc_num == 0 {
        return Ok(0);
    }
    let clinic_num: Option<i64> = conn
        .exec_first(
            "SELECT ClinicNum FROM procedurelog WHERE ProcNum = ?",
            (proc_num,),
        )
        .map_err(|e| format!("could not read procedure clinic: {e}"))?;
    Ok(clinic_num.unwrap_or(0) as Id)
}

/// OD `POut.Date` writes its zero date for an absent date. Keeping this helper
/// separate from `od_date_param` makes the write intent explicit.
fn od_write_date(value: Option<NaiveDate>) -> String {
    od_date_param(value)
}

fn od_write_datetime(value: Option<NaiveDateTime>) -> String {
    value
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "0001-01-01 00:00:00".to_string())
}

/// OD's insert paths set `SecUserNumEntry` from the acting user. Before login,
/// use the first visible OD user rather than leaving a zero audit trail.
fn audit_user_num(conn: &mut PooledConn, current: Id) -> Result<Id, String> {
    if current != 0 {
        return Ok(current);
    }
    let first: Option<i64> = conn
        .exec_first("SELECT MIN(UserNum) FROM userod WHERE IsHidden = 0", ())
        .map_err(|e| format!("could not read OD audit user: {e}"))?;
    Ok(first.filter(|n| *n > 0).unwrap_or(0) as Id)
}

/// Append an OD column only when the connected schema actually has it. This
/// keeps strict plug-and-play schemas working while still writing full OD audit
/// data on a genuine 24_3 schema.
fn append_optional_column(
    conn: &mut PooledConn,
    table: &str,
    column: &str,
    columns: &mut String,
    placeholders: &mut String,
    values: &mut Vec<Value>,
    value: Value,
) -> Result<(), String> {
    if column_exists(conn, table, column)? {
        columns.push(',');
        columns.push_str(column);
        placeholders.push_str(",?");
        values.push(value);
    }
    Ok(())
}

/// Resolve the clinic for an OD write. An explicit clinic always wins; a zero
/// falls back to the patient's clinic, which is OD's own convention for rows
/// created from a patient context.
fn write_clinic_num(conn: &mut PooledConn, requested: Id, pat_num: Id) -> Result<Id, String> {
    if requested != 0 {
        return Ok(requested);
    }
    patient_clinic_num(conn, pat_num)
}

/// OD 24_3 journal entries carry reconciliation and audit columns that the
/// reduced demo schema omits. Resolve them once before opening a transaction
/// (a `Transaction` cannot run information_schema checks) and build the INSERT
/// accordingly, so both schemas are written correctly.
struct JournalInsert {
    sql: String,
    has_reconcile: bool,
    has_sec_user_entry: bool,
    has_sec_date_entry: bool,
    has_sec_user_edit: bool,
}

impl JournalInsert {
    fn new(conn: &mut PooledConn) -> Result<Self, String> {
        let mut columns =
            "AccountNum,DateDisplayed,DebitAmt,CreditAmt,Memo,Splits,CheckNumber,TransactionNum"
                .to_string();
        let mut placeholders = "?,?,?,?,?,?,?,?".to_string();
        let mut has_reconcile = false;
        let mut has_sec_user_entry = false;
        let mut has_sec_date_entry = false;
        let mut has_sec_user_edit = false;
        for (name, flag) in [
            ("ReconcileNum", &mut has_reconcile),
            ("SecUserNumEntry", &mut has_sec_user_entry),
            ("SecDateTEntry", &mut has_sec_date_entry),
            ("SecUserNumEdit", &mut has_sec_user_edit),
        ] {
            if column_exists(conn, "journalentry", name)? {
                columns.push(',');
                columns.push_str(name);
                placeholders.push_str(",?");
                *flag = true;
            }
        }
        Ok(Self {
            sql: format!("INSERT INTO journalentry ({columns}) VALUES ({placeholders})"),
            has_reconcile,
            has_sec_user_entry,
            has_sec_date_entry,
            has_sec_user_edit,
        })
    }

    fn params(&self, entry: &JournalEntry, audit_user: Id, now: &str) -> Vec<Value> {
        let mut params: Vec<Value> = vec![
            Value::from(entry.account_num),
            Value::from(od_write_date(Some(entry.date_displayed))),
            Value::from(entry.debit_amt),
            Value::from(entry.credit_amt),
            Value::from(entry.memo.clone()),
            Value::from(entry.splits.clone()),
            Value::from(entry.check_number.clone()),
            Value::from(entry.transaction_num),
        ];
        if self.has_reconcile {
            params.push(Value::from(0u64));
        }
        if self.has_sec_user_entry {
            params.push(Value::from(audit_user));
        }
        if self.has_sec_date_entry {
            params.push(Value::from(now.to_string()));
        }
        if self.has_sec_user_edit {
            params.push(Value::from(audit_user));
        }
        params
    }
}

fn treat_plan_status(
    conn: &mut PooledConn,
    plan_num: Id,
) -> Result<Option<TreatPlanStatus>, String> {
    let value: Option<i32> = conn
        .exec_first(
            "SELECT TPStatus FROM treatplan WHERE TreatPlanNum = ?",
            (Value::from(plan_num),),
        )
        .map_err(|e| e.to_string())?;
    Ok(value.map(|v| TreatPlanStatus::from_od(v as i8)))
}

/// OD uses `0001-01-01` and `0000-00-00` for empty dates.
fn od_date(value: Option<String>) -> Option<NaiveDate> {
    let text = value?;
    if text.starts_with("0001-01-01") || text.starts_with("0000-00-00") {
        return None;
    }
    NaiveDate::parse_from_str(&text, "%Y-%m-%d").ok()
}

fn od_date_param(value: Option<NaiveDate>) -> String {
    value
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "0001-01-01".to_string())
}

fn date(value: Option<String>) -> Option<NaiveDate> {
    value.and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
}

fn datetime(value: Option<String>) -> Option<NaiveDateTime> {
    value.and_then(|s| {
        NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")
            .or_else(|_| NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S"))
            // Several OD date columns (e.g. treatplan.DateTP) are DATE-only.
            .or_else(|_| NaiveDate::parse_from_str(&s, "%Y-%m-%d").map(|d| d.and_hms_opt(0, 0, 0).unwrap()))
            .ok()
    })
}

/// OD's zero date/time sentinel for "not set" is `DateTime.MinValue` or an
/// all-zero MySQL date. Neither should surface as a lab-slip date in the UI.
fn od_datetime(value: Option<String>) -> Option<NaiveDateTime> {
    let text = value?;
    if text.starts_with("0001-01-01") || text.starts_with("0000-00-00") {
        return None;
    }
    datetime(Some(text))
}

fn s(value: Option<String>) -> String {
    value.unwrap_or_default()
}

/// OD `time` columns come back as `HH:MM:SS` (or occasionally `HH:MM:SS.ffffff`).
fn time(value: Option<String>) -> NaiveTime {
    value
        .and_then(|v| {
            NaiveTime::parse_from_str(&v, "%H:%M:%S")
                .or_else(|_| NaiveTime::parse_from_str(&v, "%H:%M"))
                .ok()
        })
        .unwrap_or_default()
}

// Open Dental `patient` has `Title` but no `Suffix`, and `DateTStamp` rather
// than `DateCreated`. `Birthdate` is the exact OD spelling.
const PATIENT_COLUMNS: &str = "PatNum,LName,FName,MiddleI,Title,CAST(Birthdate AS CHAR),Gender,HmPhone,WirelessPhone,Email,Address,City,State,Zip,ClinicNum,PriProv,CAST(DateTStamp AS CHAR),Address2,WkPhone,BillingType,ChartNumber,SSN,MedicaidID,PatStatus,COALESCE(County,''),SiteNum,GradeLevel,Urgency,PlannedIsDone,COALESCE(Guarantor,0)";

/// The subset of OD `histappointment` columns needed to restore an appointment.
const HIST_APPOINTMENT_COLUMNS: &str = "HistApptNum,HistUserNum,CAST(HistDateTStamp AS CHAR),HistApptAction,AptNum,PatNum,AptStatus,Pattern,TimeLocked,Op,Note,ProvNum,ClinicNum,CAST(AptDateTime AS CHAR),UnschedStatus,Priority";

/// `mysql_common` implements `FromRow` for tuples up to 16 elements, so the
/// 18-column patient row is read by index instead.
fn cell(row: &mysql::Row, index: usize) -> Value {
    row.get::<Value, usize>(index).unwrap_or(Value::NULL)
}

fn cell_text(row: &mysql::Row, index: usize) -> Option<String> {
    match cell(row, index) {
        Value::Bytes(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        Value::Int(v) => Some(v.to_string()),
        Value::UInt(v) => Some(v.to_string()),
        _ => None,
    }
}

fn cell_u32(row: &mysql::Row, index: usize) -> u32 {
    match cell(row, index) {
        Value::Int(v) => v as u32,
        Value::UInt(v) => v as u32,
        Value::Bytes(bytes) => String::from_utf8_lossy(&bytes).parse().unwrap_or(0),
        _ => 0,
    }
}

fn cell_i16(row: &mysql::Row, index: usize) -> i16 {
    match cell(row, index) {
        Value::Int(v) => v as i16,
        Value::UInt(v) => v as i16,
        Value::Bytes(bytes) => String::from_utf8_lossy(&bytes).parse().unwrap_or(0),
        _ => 0,
    }
}

fn cell_i32(row: &mysql::Row, index: usize) -> i32 {
    match cell(row, index) {
        Value::Int(v) => v as i32,
        Value::UInt(v) => v as i32,
        Value::Bytes(bytes) => String::from_utf8_lossy(&bytes).parse().unwrap_or(0),
        _ => 0,
    }
}

fn cell_i64(row: &mysql::Row, index: usize) -> i64 {
    match cell(row, index) {
        Value::Int(v) => v as i64,
        Value::UInt(v) => v as i64,
        Value::Bytes(bytes) => String::from_utf8_lossy(&bytes).parse().unwrap_or(0),
        _ => 0,
    }
}

fn cell_f64(row: &mysql::Row, index: usize) -> f64 {
    match cell(row, index) {
        Value::Int(v) => v as f64,
        Value::UInt(v) => v as f64,
        Value::Float(v) => v as f64,
        Value::Double(v) => v,
        Value::Bytes(bytes) => String::from_utf8_lossy(&bytes).parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

// Patient correction snapshots are stored as length-prefixed fields so names,
// notes and addresses can contain any character without a delimiter collision.
fn put_field(out: &mut String, value: &str) {
    out.push_str(&value.len().to_string());
    out.push(':');
    out.push_str(value);
}

fn take_field(input: &str, pos: &mut usize) -> Result<String, String> {
    let rest = input.get(*pos..).ok_or("corrupt patient snapshot")?;
    let colon = rest.find(':').ok_or("corrupt patient snapshot")?;
    let len: usize = rest[..colon]
        .parse()
        .map_err(|_| "corrupt patient snapshot length".to_string())?;
    let start = colon + 1;
    let end = start
        .checked_add(len)
        .ok_or("corrupt patient snapshot length")?;
    let value = rest.get(start..end).ok_or("corrupt patient snapshot")?.to_string();
    *pos += end;
    Ok(value)
}

fn put_num<T: std::fmt::Display>(out: &mut String, value: T) {
    put_field(out, &value.to_string());
}

fn take_num<T: std::str::FromStr>(input: &str, pos: &mut usize) -> Result<T, String> {
    take_field(input, pos)?
        .parse()
        .map_err(|_| "corrupt patient snapshot number".to_string())
}

fn put_opt_date(out: &mut String, value: Option<NaiveDate>) {
    put_field(
        out,
        &value
            .map(|d| format!("D{}", d.format("%Y-%m-%d")))
            .unwrap_or_else(|| "-".into()),
    );
}

fn take_opt_date(input: &str, pos: &mut usize) -> Result<Option<NaiveDate>, String> {
    let value = take_field(input, pos)?;
    match value.strip_prefix('D') {
        Some(date) => NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map(Some)
            .map_err(|e| format!("corrupt patient snapshot date: {e}")),
        None if value == "-" => Ok(None),
        None => Err("corrupt patient snapshot date".into()),
    }
}

fn put_opt_datetime(out: &mut String, value: Option<NaiveDateTime>) {
    put_field(
        out,
        &value
            .map(|d| format!("T{}", d.format("%Y-%m-%d %H:%M:%S")))
            .unwrap_or_else(|| "-".into()),
    );
}

fn take_opt_datetime(input: &str, pos: &mut usize) -> Result<Option<NaiveDateTime>, String> {
    let value = take_field(input, pos)?;
    match value.strip_prefix('T') {
        Some(date) => NaiveDateTime::parse_from_str(date, "%Y-%m-%d %H:%M:%S")
            .map(Some)
            .map_err(|e| format!("corrupt patient snapshot datetime: {e}")),
        None if value == "-" => Ok(None),
        None => Err("corrupt patient snapshot datetime".into()),
    }
}

fn encode_patient(patient: &Patient) -> String {
    let mut out = String::new();
    put_num(&mut out, patient.pat_num);
    for value in [
        &patient.l_name,
        &patient.f_name,
        &patient.middle_i,
        &patient.title,
        &patient.suffix,
    ] {
        put_field(&mut out, value);
    }
    put_opt_date(&mut out, patient.birth_date);
    put_num(&mut out, patient.gender.as_od());
    for value in [
        &patient.home_phone,
        &patient.wireless_phone,
        &patient.email,
        &patient.address,
        &patient.city,
        &patient.state,
        &patient.zip,
    ] {
        put_field(&mut out, value);
    }
    for value in [
        &patient.address2,
        &patient.wk_phone,
        &patient.chart_number,
        &patient.ssn,
        &patient.medicaid_id,
    ] {
        put_field(&mut out, value);
    }
    put_num(&mut out, patient.billing_type);
    put_num(&mut out, patient.pat_status);
    put_num(&mut out, patient.clinic_num);
    put_num(&mut out, patient.primary_provider_num);
    put_opt_datetime(&mut out, patient.date_created);
    put_field(&mut out, &patient.county);
    put_num(&mut out, patient.site_num);
    put_num(&mut out, patient.grade_level);
    put_num(&mut out, patient.urgency);
    put_num(&mut out, u8::from(patient.planned_is_done));
    // Appended so older persisted patient blobs still decode with a zero
    // guarantor (which means "self") instead of failing.
    put_num(&mut out, patient.guarantor);
    out
}

fn decode_patient(input: &str) -> Result<Patient, String> {
    let mut pos = 0;
    let patient = Patient {
        pat_num: take_num(input, &mut pos)?,
        l_name: take_field(input, &mut pos)?,
        f_name: take_field(input, &mut pos)?,
        middle_i: take_field(input, &mut pos)?,
        title: take_field(input, &mut pos)?,
        suffix: take_field(input, &mut pos)?,
        birth_date: take_opt_date(input, &mut pos)?,
        gender: Gender::from_od(take_num::<i16>(input, &mut pos)? as i8),
        home_phone: take_field(input, &mut pos)?,
        wireless_phone: take_field(input, &mut pos)?,
        email: take_field(input, &mut pos)?,
        address: take_field(input, &mut pos)?,
        city: take_field(input, &mut pos)?,
        state: take_field(input, &mut pos)?,
        zip: take_field(input, &mut pos)?,
        address2: take_field(input, &mut pos)?,
        wk_phone: take_field(input, &mut pos)?,
        chart_number: take_field(input, &mut pos)?,
        ssn: take_field(input, &mut pos)?,
        medicaid_id: take_field(input, &mut pos)?,
        billing_type: take_num(input, &mut pos)?,
        pat_status: take_num::<i16>(input, &mut pos)? as u8,
        clinic_num: take_num(input, &mut pos)?,
        primary_provider_num: take_num(input, &mut pos)?,
        date_created: take_opt_datetime(input, &mut pos)?,
        county: take_field(input, &mut pos)?,
        site_num: take_num(input, &mut pos)?,
        grade_level: take_num::<i16>(input, &mut pos)? as i8,
        urgency: take_num::<i16>(input, &mut pos)? as i8,
        planned_is_done: take_num::<i16>(input, &mut pos)? != 0,
        // Snapshots written before this field existed simply decode as 0.
        guarantor: take_num(input, &mut pos).unwrap_or(0),
    };
    if pos != input.len() {
        return Err("trailing data in patient snapshot".into());
    }
    Ok(patient)
}

/// OD `TimeSpan` columns are 100 ns ticks; this port stores minutes.
fn ticks_to_minutes(ticks: i64) -> i32 {
    (ticks / 600_000_000) as i32
}

fn minutes_to_ticks(minutes: i32) -> i64 {
    minutes as i64 * 600_000_000
}

/// OD stores the comm preference as a comma-delimited `SendOrder` string
/// (0=preferred, 1=text, 2=email), not as a dedicated CommType column.
fn comm_from_send_order(order: &str) -> CommType {
    order
        .split(',')
        .filter_map(|part| part.trim().parse::<i8>().ok())
        .next()
        .map(CommType::from_od)
        .unwrap_or_default()
}

/// Build OD's ordered `SendOrder` list for a write. The port's `comm_type` is
/// the primary channel (what the GUI edits), so it replaces the first element;
/// the remaining channels stay in OD's original fallback order. A rule read
/// from OD therefore keeps its full comma-delimited chain on save, while a
/// channel edit actually takes effect.
fn send_order_for_write(rule: &ReminderRule) -> String {
    let primary = rule.comm_type.as_od().to_string();
    let rest: Vec<&str> = rule
        .send_order
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .skip(1)
        .collect();
    if rest.is_empty() {
        primary
    } else {
        format!("{},{}", primary, rest.join(","))
    }
}

fn map_reminder_rule(row: mysql::Row) -> ReminderRule {
    ReminderRule {
        rule_num: cell_u32(&row, 0),
        type_cur: ReminderType::from_od(cell_i16(&row, 1) as i8),
        minutes_prior: ticks_to_minutes(cell_i64(&row, 2)),
        do_not_send_within_minutes: ticks_to_minutes(cell_i64(&row, 9)),
        send_order: s(cell_text(&row, 3)),
        is_enabled: cell_bool(&row, 10),
        comm_type: comm_from_send_order(&s(cell_text(&row, 3))),
        sms_template: s(cell_text(&row, 5)),
        email_subject: s(cell_text(&row, 6)),
        email_body: s(cell_text(&row, 7)),
    }
}

fn map_patient_contact(row: mysql::Row) -> PatientContact {
    PatientContact {
        pat_num: cell_u32(&row, 0),
        telegram_chat_id: s(cell_text(&row, 1)),
        email_override: s(cell_text(&row, 2)),
        opt_out_email: cell_bool(&row, 3),
        opt_out_telegram: cell_bool(&row, 4),
    }
}

fn map_outbox_message(row: mysql::Row) -> OutboxMessage {
    OutboxMessage {
        message_num: cell_u32(&row, 0),
        pat_num: cell_u32(&row, 1),
        apt_num: cell_u32(&row, 2),
        rule_num: cell_u32(&row, 3),
        reminder_type: ReminderType::from_od(cell_i16(&row, 4) as i8),
        transport: match cell_i16(&row, 5) {
            1 => TransportKind::Telegram,
            2 => TransportKind::WhatsApp,
            3 => TransportKind::GsmModem,
            4 => TransportKind::AfricasTalking,
            5 => TransportKind::SmsTo,
            _ => TransportKind::Smtp,
        },
        recipient: s(cell_text(&row, 6)),
        subject: s(cell_text(&row, 7)),
        body: s(cell_text(&row, 8)),
        status: OutboxStatus::from_od(cell_i16(&row, 9) as i8),
        attempts: cell_u32(&row, 10),
        created_at: datetime(cell_text(&row, 11)).unwrap_or_default(),
        sent_at: datetime(cell_text(&row, 12)),
        last_error: s(cell_text(&row, 13)),
    }
}

fn datetime_param(value: Option<NaiveDateTime>) -> Value {
    match value {
        Some(dt) => Value::from(dt.format("%Y-%m-%d %H:%M:%S").to_string()),
        None => Value::NULL,
    }
}

fn cell_bool(row: &mysql::Row, index: usize) -> bool {
    match cell(row, index) {
        Value::Int(v) => v != 0,
        Value::UInt(v) => v != 0,
        Value::Bytes(bytes) => String::from_utf8_lossy(&bytes) != "0",
        _ => false,
    }
}

fn map_patient(row: mysql::Row) -> Patient {
    Patient {
        pat_num: cell_u32(&row, 0),
        l_name: s(cell_text(&row, 1)),
        f_name: s(cell_text(&row, 2)),
        middle_i: s(cell_text(&row, 3)),
        title: s(cell_text(&row, 4)),
        // The OD patient table has no suffix column; keep the domain field
        // available for other backends but leave it empty here.
        suffix: String::new(),
        birth_date: date(cell_text(&row, 5)),
        gender: Gender::from_od(cell_u32(&row, 6) as i8),
        home_phone: s(cell_text(&row, 7)),
        wireless_phone: s(cell_text(&row, 8)),
        email: s(cell_text(&row, 9)),
        address: s(cell_text(&row, 10)),
        address2: s(cell_text(&row, 17)),
        city: s(cell_text(&row, 11)),
        state: s(cell_text(&row, 12)),
        zip: s(cell_text(&row, 13)),
        wk_phone: s(cell_text(&row, 18)),
        billing_type: cell_u32(&row, 19),
        chart_number: s(cell_text(&row, 20)),
        ssn: s(cell_text(&row, 21)),
        medicaid_id: s(cell_text(&row, 22)),
        pat_status: cell_u32(&row, 23) as u8,
        guarantor: cell_u32(&row, 29),
        clinic_num: cell_u32(&row, 14),
        primary_provider_num: cell_u32(&row, 15),
        date_created: datetime(cell_text(&row, 16)),
        county: s(cell_text(&row, 24)),
        site_num: cell_u32(&row, 25),
        grade_level: cell_u32(&row, 26) as i8,
        urgency: cell_u32(&row, 27) as i8,
        planned_is_done: cell_u32(&row, 28) != 0,
    }
}

/// Open Dental stores appointment length implicitly: each character of
/// `Pattern` is one schedule increment (normally five minutes).
fn pattern_minutes(pattern: &str, fallback: u32) -> u32 {
    let increments = pattern.chars().count() as u32;
    if increments == 0 {
        fallback
    } else {
        increments * 5
    }
}

/// OD added `Confirmed` and `IsNewPatient` after older 24_3 installs; a reduced
/// schema may omit either. The extra columns are selected as literal zeros in
/// that case so the row shape stays stable.
fn appointment_select(conn: &mut PooledConn) -> Result<String, String> {
    let confirmed = if column_exists(conn, "appointment", "Confirmed")? {
        "COALESCE(Confirmed,0)"
    } else {
        "0"
    };
    let is_new = if column_exists(conn, "appointment", "IsNewPatient")? {
        "COALESCE(IsNewPatient,0)"
    } else {
        "0"
    };
    Ok(format!(
        "AptNum,PatNum,CAST(AptDateTime AS CHAR),AptStatus,Op,ProvNum,ClinicNum,Note,Pattern,\
         TimeLocked,Priority,UnschedStatus,{confirmed},{is_new}"
    ))
}

fn map_appointment(row: mysql::Row) -> Appointment {
    let pattern = s(cell_text(&row, 8));
    let apt_length = pattern_minutes(&pattern, 30);
    Appointment {
        apt_num: cell_u32(&row, 0),
        pat_num: cell_u32(&row, 1),
        apt_datetime: datetime(cell_text(&row, 2)).unwrap_or_default(),
        apt_length,
        apt_status: AppointmentStatus::from_od(cell_i16(&row, 3) as i8),
        operatory_num: cell_u32(&row, 4),
        provider_num: cell_u32(&row, 5),
        clinic_num: cell_u32(&row, 6),
        notes: s(cell_text(&row, 7)),
        pattern,
        is_time_locked: cell_bool(&row, 9),
        priority: RecallPriority::from_od(cell_i16(&row, 10) as i8),
        waitlist_status: cell_u32(&row, 11),
        confirmed: cell_u32(&row, 12),
        is_new_patient: cell_bool(&row, 13),
    }
}

fn map_appointment_change(row: mysql::Row) -> AppointmentChange {
    let pattern = s(cell_text(&row, 7));
    let snapshot = Appointment {
        apt_num: cell_u32(&row, 4),
        pat_num: cell_u32(&row, 5),
        apt_datetime: datetime(cell_text(&row, 13)).unwrap_or_default(),
        apt_length: pattern_minutes(&pattern, 30),
        apt_status: AppointmentStatus::from_od(cell_i16(&row, 6) as i8),
        operatory_num: cell_u32(&row, 9),
        provider_num: cell_u32(&row, 11),
        clinic_num: cell_u32(&row, 12),
        notes: s(cell_text(&row, 10)),
        pattern,
        is_time_locked: cell_bool(&row, 8),
        priority: RecallPriority::from_od(cell_i16(&row, 15) as i8),
        waitlist_status: cell_u32(&row, 14),
        confirmed: 0,
        is_new_patient: false,
    };
    AppointmentChange {
        change_num: cell_u32(&row, 0),
        apt_num: snapshot.apt_num,
        pat_num: snapshot.pat_num,
        changed_at: datetime(cell_text(&row, 2)).unwrap_or_default(),
        user_num: cell_u32(&row, 1),
        action: AppointmentChangeAction::from_od(cell_i16(&row, 3) as i8),
        snapshot,
    }
}

fn map_patient_correction(row: mysql::Row) -> Result<PatientCorrection, String> {
    Ok(PatientCorrection {
        correction_num: cell_u32(&row, 0),
        pat_num: cell_u32(&row, 1),
        corrected_at: datetime(cell_text(&row, 2)).unwrap_or_default(),
        user_num: cell_u32(&row, 3),
        reason: s(cell_text(&row, 4)),
        before: decode_patient(&s(cell_text(&row, 5)))?,
    })
}

fn map_recall(row: mysql::Row) -> Recall {
    let (
        recall_num,
        pat_num,
        date_due_calc,
        date_due,
        date_previous,
        recall_interval,
        recall_status,
        note,
        is_disabled,
        date_scheduled,
        priority,
    ): (
        u32,
        u32,
        Option<String>,
        Option<String>,
        Option<String>,
        i32,
        u32,
        Option<String>,
        i8,
        Option<String>,
        i8,
    ) = from_row(row);
    Recall {
        recall_num,
        pat_num,
        date_due_calc: od_date(date_due_calc),
        date_due: od_date(date_due),
        date_previous: od_date(date_previous),
        recall_interval,
        recall_status,
        note: s(note),
        is_disabled: is_disabled != 0,
        date_scheduled: od_date(date_scheduled),
        priority: RecallPriority::from_od(priority),
    }
}

fn map_definition(row: mysql::Row) -> Definition {
    Definition {
        def_num: cell_u32(&row, 0),
        category: DefinitionCategory::from_od(cell_i16(&row, 1)),
        item_order: cell_i32(&row, 2),
        item_name: s(cell_text(&row, 3)),
        item_value: s(cell_text(&row, 4)),
        item_color: cell_i32(&row, 5),
        is_hidden: cell_bool(&row, 6),
    }
}

fn map_display_field(row: mysql::Row) -> DisplayField {
    DisplayField {
        display_field_num: cell_u32(&row, 0),
        internal_name: s(cell_text(&row, 1)),
        item_order: cell_i32(&row, 2),
        description: s(cell_text(&row, 3)),
        column_width: cell_i32(&row, 4),
        category: DisplayFieldCategory::from_od(cell_i16(&row, 5)),
        chart_view_num: cell_u32(&row, 6),
        pick_list: s(cell_text(&row, 7)),
        description_override: s(cell_text(&row, 8)),
    }
}

/// OD `SaveListForCategory` compares the list with its built-in defaults and
/// stores nothing when they match. Key fields only; surrogate numbers are
/// ignored because OD deletes and reinserts the rows.
fn display_signature(rows: &[DisplayField]) -> Vec<(String, String, i32, String, String)> {
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
}

impl Store for MySqlStore {
    fn set_current_user(&self, user_num: Id) {
        self.current_user.store(user_num as u64, Ordering::Relaxed);
    }

    fn current_user(&self) -> Id {
        self.current_user.load(Ordering::Relaxed) as Id
    }

    fn list_patients(&self) -> Result<Vec<Patient>, String> {
        let mut conn = self.conn()?;
        let sql = format!("SELECT {PATIENT_COLUMNS} FROM patient ORDER BY LName,FName");
        let rows: Vec<mysql::Row> = conn.query(sql).map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(map_patient).collect())
    }

    fn find_patients(&self, query: &str) -> Result<Vec<Patient>, String> {
        let mut conn = self.conn()?;
        let pattern = format!("%{query}%");
        let sql = format!(
            "SELECT {PATIENT_COLUMNS} FROM patient WHERE LName LIKE ? OR FName LIKE ? OR HmPhone LIKE ? OR PatNum = ? ORDER BY LName,FName"
        );
        let numeric: u32 = query.trim().parse().unwrap_or(0);
        let rows: Vec<mysql::Row> = conn
            .exec(
                sql,
                (
                    Value::from(pattern.clone()),
                    Value::from(pattern.clone()),
                    Value::from(pattern),
                    Value::from(numeric),
                ),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(map_patient).collect())
    }

    fn get_patient(&self, pat_num: Id) -> Result<Option<Patient>, String> {
        let mut conn = self.conn()?;
        let sql = format!("SELECT {PATIENT_COLUMNS} FROM patient WHERE PatNum = ?");
        let rows: Vec<mysql::Row> = conn
            .exec(sql, (Value::from(pat_num),))
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(map_patient).next())
    }

    fn upsert_patient(&self, patient: &Patient) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        // Every field read by PATIENT_COLUMNS is persisted. OD's DateTStamp is
        // left to MySQL/OD so this write never fights the upstream timestamp.
        // Audit/hash columns are appended only when the connected schema has
        // them; OD's SecurityHash is left blank because this port cannot
        // reproduce CDT.dll. The write policy gate is what makes that safe.
        let mut columns = "PatNum,LName,FName,MiddleI,Title,Birthdate,Gender,HmPhone,WirelessPhone,Email,\
                           Address,Address2,City,State,Zip,WkPhone,BillingType,ChartNumber,SSN,MedicaidID,\
                           PatStatus,County,SiteNum,GradeLevel,Urgency,PlannedIsDone,Guarantor,ClinicNum,PriProv"
            .to_string();
        let mut placeholders =
            "?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?".to_string();
        let num = if patient.pat_num == 0 {
            // Let MySQL assign the auto-increment key.
            0
        } else {
            patient.pat_num
        };
        // OD's patient.Birthdate is NOT NULL; use its conventional zero date
        // when the caller did not provide one.
        let birth = od_write_date(patient.birth_date);
        let billing_type = effective_billing_type(&mut conn, patient)?;
        let audit_user = audit_user_num(&mut conn, self.current_user())?;
        let mut params: Vec<Value> = vec![
            Value::from(num),
            Value::from(patient.l_name.clone()),
            Value::from(patient.f_name.clone()),
            Value::from(patient.middle_i.clone()),
            Value::from(patient.title.clone()),
            Value::from(birth),
            Value::from(patient.gender.as_od()),
            Value::from(patient.home_phone.clone()),
            Value::from(patient.wireless_phone.clone()),
            Value::from(patient.email.clone()),
            Value::from(patient.address.clone()),
            Value::from(patient.address2.clone()),
            Value::from(patient.city.clone()),
            Value::from(patient.state.clone()),
            Value::from(patient.zip.clone()),
            Value::from(patient.wk_phone.clone()),
            Value::from(billing_type),
            Value::from(patient.chart_number.clone()),
            Value::from(patient.ssn.clone()),
            Value::from(patient.medicaid_id.clone()),
            Value::from(patient.pat_status as u32),
            Value::from(patient.county.clone()),
            Value::from(patient.site_num),
            Value::from(patient.grade_level as i32),
            Value::from(patient.urgency as i32),
            Value::from(patient.planned_is_done as u8),
            Value::from(patient.guarantor),
            Value::from(patient.clinic_num),
            Value::from(patient.primary_provider_num),
        ];
        append_optional_column(
            &mut conn,
            "patient",
            "SecUserNumEntry",
            &mut columns,
            &mut placeholders,
            &mut params,
            Value::from(audit_user),
        )?;
        append_optional_column(
            &mut conn,
            "patient",
            "SecDateEntry",
            &mut columns,
            &mut placeholders,
            &mut params,
            Value::from(Local::now().date_naive().format("%Y-%m-%d").to_string()),
        )?;
        append_optional_column(
            &mut conn,
            "patient",
            "SecurityHash",
            &mut columns,
            &mut placeholders,
            &mut params,
            Value::from(""),
        )?;
        let sql = format!(
            "INSERT INTO patient ({columns}) VALUES ({placeholders}) \
             ON DUPLICATE KEY UPDATE \
             LName=VALUES(LName),FName=VALUES(FName),MiddleI=VALUES(MiddleI),Title=VALUES(Title),\
             Birthdate=VALUES(Birthdate),Gender=VALUES(Gender),HmPhone=VALUES(HmPhone),\
             WirelessPhone=VALUES(WirelessPhone),Email=VALUES(Email),Address=VALUES(Address),\
             Address2=VALUES(Address2),City=VALUES(City),State=VALUES(State),Zip=VALUES(Zip),\
             WkPhone=VALUES(WkPhone),BillingType=VALUES(BillingType),ChartNumber=VALUES(ChartNumber),\
             SSN=VALUES(SSN),MedicaidID=VALUES(MedicaidID),PatStatus=VALUES(PatStatus),\
             County=VALUES(County),SiteNum=VALUES(SiteNum),GradeLevel=VALUES(GradeLevel),\
             Urgency=VALUES(Urgency),PlannedIsDone=VALUES(PlannedIsDone),Guarantor=VALUES(Guarantor),\
             ClinicNum=VALUES(ClinicNum),PriProv=VALUES(PriProv)"
        );
        conn.exec_drop(sql, params).map_err(|e| e.to_string())?;
        let saved_num = if patient.pat_num == 0 {
            conn.last_insert_id() as Id
        } else {
            patient.pat_num
        };
        // OD treats each patient as their own guarantor unless an explicit one
        // was supplied. A zero here would make OD show the patient as belonging
        // to guarantor 0, which breaks balances and statements.
        if patient.guarantor == 0 {
            conn.exec_drop(
                "UPDATE patient SET Guarantor = PatNum WHERE PatNum = ?",
                (Value::from(saved_num),),
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(saved_num)
    }

    fn delete_patient(&self, pat_num: Id) -> Result<(), String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        // OD never hard-deletes a patient: deleted patients stay in the table
        // with PatStatus=4 so historical procedures and claims still resolve.
        conn.exec_drop(
            "UPDATE patient SET PatStatus = 4, Guarantor = PatNum WHERE PatNum = ?",
            (Value::from(pat_num),),
        )
        .map_err(|e| e.to_string())
    }

    fn list_appointments(
        &self,
        from: NaiveDateTime,
        to: NaiveDateTime,
    ) -> Result<Vec<Appointment>, String> {
        let mut conn = self.conn()?;
        let columns = appointment_select(&mut conn)?;
        let query = format!(
            "SELECT {columns} FROM appointment WHERE AptDateTime >= ? AND AptDateTime < ? ORDER BY AptDateTime,Op"
        );
        let rows: Vec<mysql::Row> = conn
            .exec(
                query,
                (
                    Value::from(from.format("%Y-%m-%d %H:%M:%S").to_string()),
                    Value::from(to.format("%Y-%m-%d %H:%M:%S").to_string()),
                ),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(map_appointment).collect())
    }

    fn get_appointment(&self, apt_num: Id) -> Result<Option<Appointment>, String> {
        let mut conn = self.conn()?;
        let columns = appointment_select(&mut conn)?;
        let query = format!("SELECT {columns} FROM appointment WHERE AptNum = ?");
        let rows: Vec<mysql::Row> = conn
            .exec(query, (Value::from(apt_num),))
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(map_appointment).next())
    }

    fn upsert_appointment(&self, apt: &Appointment) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        // OD appointment writes carry the confirmation definition FK (DefCat 2,
        // never the recall-status category), the acting user and a hash. The
        // hash is blank here; the write gate documents that limitation.
        let mut columns = "AptNum,PatNum,AptDateTime,AptStatus,Op,ProvNum,ClinicNum,Note,Pattern,\
                           TimeLocked,Priority,UnschedStatus"
            .to_string();
        let mut placeholders = "?,?,?,?,?,?,?,?,?,?,?,?".to_string();
        let mut update = "PatNum=VALUES(PatNum),AptDateTime=VALUES(AptDateTime),\
                          AptStatus=VALUES(AptStatus),Op=VALUES(Op),ProvNum=VALUES(ProvNum),\
                          ClinicNum=VALUES(ClinicNum),Note=VALUES(Note),Pattern=VALUES(Pattern),\
                          TimeLocked=VALUES(TimeLocked),Priority=VALUES(Priority),\
                          UnschedStatus=VALUES(UnschedStatus)"
            .to_string();
        let num = if apt.apt_num == 0 { 0 } else { apt.apt_num };
        let pattern = if apt.pattern.is_empty() {
            "X".repeat((apt.apt_length / 5).max(1) as usize)
        } else {
            apt.pattern.clone()
        };
        let clinic_num = write_clinic_num(&mut conn, apt.clinic_num, apt.pat_num)?;
        // A zero keeps the previous compatibility behaviour (OD's first visible
        // confirmation definition); an explicit definition is the user's choice
        // from the board. Never clear a stored confirmation implicitly.
        let confirmed = if apt.confirmed == 0 {
            first_visible_definition(&mut conn, 2)?
        } else {
            apt.confirmed
        };
        let audit_user = audit_user_num(&mut conn, self.current_user())?;
        let mut params: Vec<Value> = vec![
            Value::from(num),
            Value::from(apt.pat_num),
            Value::from(apt.apt_datetime.format("%Y-%m-%d %H:%M:%S").to_string()),
            Value::from(apt.apt_status.as_od()),
            Value::from(apt.operatory_num),
            Value::from(apt.provider_num),
            Value::from(clinic_num),
            Value::from(apt.notes.clone()),
            Value::from(pattern),
            Value::from(i8::from(apt.is_time_locked)),
            Value::from(apt.priority.as_od()),
            Value::from(apt.waitlist_status),
        ];
        let has_confirmed = column_exists(&mut conn, "appointment", "Confirmed")?;
        append_optional_column(
            &mut conn,
            "appointment",
            "Confirmed",
            &mut columns,
            &mut placeholders,
            &mut params,
            Value::from(confirmed),
        )?;
        if has_confirmed {
            update.push_str(",Confirmed=VALUES(Confirmed)");
        }
        let has_is_new = column_exists(&mut conn, "appointment", "IsNewPatient")?;
        append_optional_column(
            &mut conn,
            "appointment",
            "IsNewPatient",
            &mut columns,
            &mut placeholders,
            &mut params,
            Value::from(i8::from(apt.is_new_patient)),
        )?;
        if has_is_new {
            update.push_str(",IsNewPatient=VALUES(IsNewPatient)");
        }
        append_optional_column(
            &mut conn,
            "appointment",
            "SecUserNumEntry",
            &mut columns,
            &mut placeholders,
            &mut params,
            Value::from(audit_user),
        )?;
        append_optional_column(
            &mut conn,
            "appointment",
            "SecDateTEntry",
            &mut columns,
            &mut placeholders,
            &mut params,
            Value::from(od_write_datetime(Some(Local::now().naive_local()))),
        )?;
        append_optional_column(
            &mut conn,
            "appointment",
            "SecurityHash",
            &mut columns,
            &mut placeholders,
            &mut params,
            Value::from(""),
        )?;
        let sql = format!(
            "INSERT INTO appointment ({columns}) VALUES ({placeholders}) \
             ON DUPLICATE KEY UPDATE {update}"
        );
        conn.exec_drop(sql, params).map_err(|e| e.to_string())?;
        if apt.apt_num == 0 {
            Ok(conn.last_insert_id() as Id)
        } else {
            Ok(apt.apt_num)
        }
    }

    fn delete_appointment(&self, apt_num: Id) -> Result<(), String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        conn.exec_drop("DELETE FROM appointment WHERE AptNum = ?", (Value::from(apt_num),))
            .map_err(|e| e.to_string())
    }

    fn list_waitlist_appointments(&self) -> Result<Vec<Appointment>, String> {
        let mut conn = self.conn()?;
        let columns = appointment_select(&mut conn)?;
        let sql = format!(
            "SELECT {columns} FROM appointment \
             WHERE AptStatus = 3 ORDER BY AptDateTime,PatNum,AptNum"
        );
        let rows: Vec<mysql::Row> = conn
            .query(sql.as_str())
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(map_appointment).collect())
    }

    fn list_appointment_changes(&self, apt_num: Id) -> Result<Vec<AppointmentChange>, String> {
        let mut conn = self.conn()?;
        let sql = format!(
            "SELECT {HIST_APPOINTMENT_COLUMNS} FROM histappointment \
             WHERE AptNum = ? ORDER BY HistDateTStamp DESC,HistApptNum DESC"
        );
        let rows: Vec<mysql::Row> = conn
            .exec(sql.as_str(), (Value::from(apt_num),))
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(map_appointment_change).collect())
    }

    fn append_appointment_change(&self, change: &AppointmentChange) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        let sql = "INSERT INTO histappointment \
                   (HistApptNum,HistUserNum,HistDateTStamp,HistApptAction,AptNum,PatNum,\
                    AptStatus,Pattern,TimeLocked,Op,Note,ProvNum,ClinicNum,AptDateTime,\
                    UnschedStatus,Priority) \
                   VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) \
                   ON DUPLICATE KEY UPDATE HistUserNum=VALUES(HistUserNum),\
                    HistDateTStamp=VALUES(HistDateTStamp),HistApptAction=VALUES(HistApptAction),\
                    AptNum=VALUES(AptNum),PatNum=VALUES(PatNum),AptStatus=VALUES(AptStatus),\
                    Pattern=VALUES(Pattern),TimeLocked=VALUES(TimeLocked),Op=VALUES(Op),\
                    Note=VALUES(Note),ProvNum=VALUES(ProvNum),ClinicNum=VALUES(ClinicNum),\
                    AptDateTime=VALUES(AptDateTime),UnschedStatus=VALUES(UnschedStatus),\
                    Priority=VALUES(Priority)";
        let num = if change.change_num == 0 { 0 } else { change.change_num };
        let changed_at = if change.changed_at == NaiveDateTime::default() {
            Local::now().naive_local()
        } else {
            change.changed_at
        };
        let snapshot = &change.snapshot;
        let audit_user = audit_user_num(&mut conn, change.user_num)?;
        let params: Vec<Value> = vec![
            Value::from(num),
            Value::from(audit_user),
            Value::from(changed_at.format("%Y-%m-%d %H:%M:%S").to_string()),
            Value::from(change.action.as_od()),
            Value::from(snapshot.apt_num),
            Value::from(snapshot.pat_num),
            Value::from(snapshot.apt_status.as_od()),
            Value::from(snapshot.pattern.clone()),
            Value::from(i8::from(snapshot.is_time_locked)),
            Value::from(snapshot.operatory_num),
            Value::from(snapshot.notes.clone()),
            Value::from(snapshot.provider_num),
            Value::from(snapshot.clinic_num),
            Value::from(snapshot.apt_datetime.format("%Y-%m-%d %H:%M:%S").to_string()),
            Value::from(snapshot.waitlist_status),
            Value::from(snapshot.priority.as_od()),
        ];
        conn.exec_drop(sql, params).map_err(|e| e.to_string())?;
        if change.change_num == 0 {
            Ok(conn.last_insert_id() as Id)
        } else {
            Ok(change.change_num)
        }
    }

    fn list_patient_corrections(&self, pat_num: Id) -> Result<Vec<PatientCorrection>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT CorrectionNum,PatNum,CAST(CorrectedAt AS CHAR),UserNum,Reason,BeforeData \
                 FROM odclinic_patient_correction WHERE PatNum = ? \
                 ORDER BY CorrectedAt DESC,CorrectionNum DESC",
                (Value::from(pat_num),),
            )
            .map_err(|e| e.to_string())?;
        rows.into_iter().map(map_patient_correction).collect()
    }

    fn append_patient_correction(&self, correction: &PatientCorrection) -> Result<Id, String> {
        let mut conn = self.conn()?;
        let sql = "INSERT INTO odclinic_patient_correction \
                   (CorrectionNum,PatNum,CorrectedAt,UserNum,Reason,BeforeData) \
                   VALUES (?,?,?,?,?,?) \
                   ON DUPLICATE KEY UPDATE PatNum=VALUES(PatNum),CorrectedAt=VALUES(CorrectedAt),\
                    UserNum=VALUES(UserNum),Reason=VALUES(Reason),BeforeData=VALUES(BeforeData)";
        let num = if correction.correction_num == 0 { 0 } else { correction.correction_num };
        let corrected_at = if correction.corrected_at == NaiveDateTime::default() {
            Local::now().naive_local()
        } else {
            correction.corrected_at
        };
        let params: Vec<Value> = vec![
            Value::from(num),
            Value::from(correction.pat_num),
            Value::from(corrected_at.format("%Y-%m-%d %H:%M:%S").to_string()),
            Value::from(correction.user_num),
            Value::from(correction.reason.clone()),
            Value::from(encode_patient(&correction.before)),
        ];
        conn.exec_drop(sql, params).map_err(|e| e.to_string())?;
        if correction.correction_num == 0 {
            Ok(conn.last_insert_id() as Id)
        } else {
            Ok(correction.correction_num)
        }
    }

    fn list_recalls(&self) -> Result<Vec<Recall>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .query(
                "SELECT RecallNum,PatNum,CAST(DateDueCalc AS CHAR),CAST(DateDue AS CHAR),\
                 CAST(DatePrevious AS CHAR),RecallInterval,RecallStatus,Note,IsDisabled,\
                 CAST(DateScheduled AS CHAR),Priority \
                 FROM recall ORDER BY DateDue,RecallNum",
            )
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(map_recall).collect())
    }

    fn upsert_recall(&self, recall: &Recall) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        // OD's recall table also carries these bookkeeping columns. The port
        // has no separate recall-type editor, so RecallTypeNum stays at OD's
        // zero value; the other three use OD's empty-value conventions.
        let sql = "INSERT INTO recall \
                   (RecallNum,PatNum,DateDueCalc,DateDue,DatePrevious,RecallInterval,RecallStatus,\
                    Note,IsDisabled,DateScheduled,Priority,RecallTypeNum,TimePatternOverride,\
                    DisableUntilBalance,DisableUntilDate) \
                   VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) \
                   ON DUPLICATE KEY UPDATE PatNum=VALUES(PatNum),DateDueCalc=VALUES(DateDueCalc),\
                    DateDue=VALUES(DateDue),DatePrevious=VALUES(DatePrevious),\
                    RecallInterval=VALUES(RecallInterval),RecallStatus=VALUES(RecallStatus),\
                    Note=VALUES(Note),IsDisabled=VALUES(IsDisabled),\
                    DateScheduled=VALUES(DateScheduled),Priority=VALUES(Priority),\
                    RecallTypeNum=VALUES(RecallTypeNum),\
                    TimePatternOverride=VALUES(TimePatternOverride),\
                    DisableUntilBalance=VALUES(DisableUntilBalance),\
                    DisableUntilDate=VALUES(DisableUntilDate)";
        let num = if recall.recall_num == 0 { 0 } else { recall.recall_num };
        let params: Vec<Value> = vec![
            Value::from(num),
            Value::from(recall.pat_num),
            Value::from(od_write_date(recall.date_due_calc)),
            Value::from(od_write_date(recall.date_due)),
            Value::from(od_write_date(recall.date_previous)),
            Value::from(recall.recall_interval),
            Value::from(recall.recall_status),
            Value::from(recall.note.clone()),
            Value::from(i8::from(recall.is_disabled)),
            Value::from(od_write_date(recall.date_scheduled)),
            Value::from(recall.priority.as_od()),
            Value::from(0u64),
            Value::from(""),
            Value::from(0.0f64),
            Value::from(od_write_datetime(None)),
        ];
        conn.exec_drop(sql, params).map_err(|e| e.to_string())?;
        if recall.recall_num == 0 {
            Ok(conn.last_insert_id() as Id)
        } else {
            Ok(recall.recall_num)
        }
    }

    fn list_asap_appointments(&self) -> Result<Vec<Appointment>, String> {
        let mut conn = self.conn()?;
        // Mirrors Appointments.RefreshASAP: Priority = 1 and active statuses.
        // The numeric set is unchanged between OD 24.3 and the REA-verified
        // 25.3 enum: Scheduled=1, Complete=2, UnschedList=3, Planned=6.
        // (The old 24.3 comment swapped the labels for 3 and 6.)
        let columns = appointment_select(&mut conn)?;
        let query = format!(
            "SELECT {columns} FROM appointment \
             WHERE Priority = 1 AND AptStatus IN (1,6,3,2) ORDER BY AptDateTime"
        );
        let rows: Vec<mysql::Row> = conn
            .query(query.as_str())
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(map_appointment).collect())
    }

    fn list_providers(&self) -> Result<Vec<Provider>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .query("SELECT ProvNum,LName,FName,Abbr,IsHidden,COALESCE(HourlyProdGoalAmt,0) FROM provider ORDER BY LName")
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (provider_num, l, f, abbr, hidden, goal): (u32, Option<String>, Option<String>, Option<String>, i8, f64) = from_row(row);
                Provider {
                    provider_num,
                    l_name: s(l),
                    f_name: s(f),
                    abbreviation: s(abbr),
                    is_hidden: hidden != 0,
                    hourly_prod_goal_amt: goal,
                }
            })
            .collect())
    }

    fn list_operatories(&self) -> Result<Vec<Operatory>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .query("SELECT OperatoryNum,OpName,Abbrev,ClinicNum,IsHidden FROM operatory ORDER BY ItemOrder")
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (operatory_num, desc, abbr, clinic_num, hidden): (u32, Option<String>, Option<String>, u32, i8) = from_row(row);
                Operatory {
                    operatory_num,
                    description: s(desc),
                    abbrev: s(abbr),
                    clinic_num,
                    is_hidden: hidden != 0,
                }
            })
            .collect())
    }

    fn list_clinics(&self) -> Result<Vec<Clinic>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .query("SELECT ClinicNum,Description,Abbr,IsHidden FROM clinic ORDER BY ItemOrder")
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (clinic_num, desc, abbr, hidden): (u32, Option<String>, Option<String>, i8) = from_row(row);
                Clinic {
                    clinic_num,
                    description: s(desc),
                    abbr: s(abbr),
                    is_hidden: hidden != 0,
                }
            })
            .collect())
    }

    fn list_procedures(&self, pat_num: Id) -> Result<Vec<Procedure>, String> {
        let mut conn = self.conn()?;
        let date_complete = if column_exists(&mut conn, "procedurelog", "DateComplete")? {
            "CAST(DateComplete AS CHAR)"
        } else {
            "NULL"
        };
        let query = format!(
            "SELECT ProcNum,PatNum,CodeNum,COALESCE(AptNum,0),CAST(ProcDate AS CHAR),{date_complete},ProcStatus,ToothNum,ProvNum,ProcFee, \
             CAST(COALESCE(UnitQty,0) AS DOUBLE),CAST(COALESCE(BaseUnits,0) AS DOUBLE), \
             COALESCE(Surf,''),COALESCE(ToothRange,''),COALESCE(MedicalCode,''),COALESCE(SnomedBodySite,'') \
             FROM procedurelog WHERE ? = 0 OR PatNum = ? ORDER BY ProcDate DESC"
        );
        let rows: Vec<mysql::Row> = conn
            .exec(query, (Value::from(pat_num), Value::from(pat_num)))
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| Procedure {
                proc_num: cell_u32(&row, 0),
                pat_num: cell_u32(&row, 1),
                code_num: cell_u32(&row, 2),
                apt_num: cell_u32(&row, 3),
                proc_date: date(cell_text(&row, 4)).unwrap_or_default(),
                date_complete: od_datetime(cell_text(&row, 5)),
                proc_status: ProcedureStatus::from_od(cell_i16(&row, 6) as i8),
                tooth_num: s(cell_text(&row, 7)),
                provider_num: cell_u32(&row, 8),
                fee: cell_f64(&row, 9),
                unit_qty: cell_f64(&row, 10),
                base_units: cell_f64(&row, 11),
                surf: s(cell_text(&row, 12)),
                tooth_range: s(cell_text(&row, 13)),
                medical_code: s(cell_text(&row, 14)),
                snomed_body_site: s(cell_text(&row, 15)),
                description_override: String::new(),
            })
            .collect())
    }

    fn insert_procedure(&self, procedure: &Procedure) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        // Mirrors the OD columns set by Procedures.Insert for a new planned
        // procedure. OD assigns the key and stores DateTime.MinValue
        // (0001-01-01) in DateComplete until the procedure moves to Complete.
        // ClinicNum comes from the patient, never a hardcoded zero.
        let clinic_num = patient_clinic_num(&mut conn, procedure.pat_num)?;
        conn.exec_drop(
            "INSERT INTO procedurelog \
             (PatNum,AptNum,ProvNum,CodeNum,ClinicNum,ProcDate,ProcFee,ProcStatus, \
              ToothNum,Surf,ToothRange,MedicalCode,SnomedBodySite,UnitQty,BaseUnits, \
              DateComplete,DiscountPlanAmt) \
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,'0001-01-01',0)",
            vec![
                Value::from(procedure.pat_num),
                Value::from(procedure.apt_num),
                Value::from(procedure.provider_num),
                Value::from(procedure.code_num),
                Value::from(clinic_num),
                Value::from(procedure.proc_date.format("%Y-%m-%d").to_string()),
                Value::from(procedure.fee),
                Value::from(ProcedureStatus::TreatmentPlanned.as_od()),
                Value::from(procedure.tooth_num.clone()),
                Value::from(procedure.surf.clone()),
                Value::from(procedure.tooth_range.clone()),
                Value::from(procedure.medical_code.clone()),
                Value::from(procedure.snomed_body_site.clone()),
                Value::from(procedure.unit_qty),
                Value::from(procedure.base_units),
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_id() as Id)
    }

    fn set_procedure_complete(&self, proc_num: Id, discount_amount: f64) -> Result<(), String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        // OD Procedures.Insert/Update set DateComplete and DiscountPlanAmt
        // alongside ProcStatus when a procedure becomes complete. The service
        // date is left as entered, exactly as upstream does.
        conn.exec_drop(
            "UPDATE procedurelog SET ProcStatus=?,DateComplete=NOW(),DiscountPlanAmt=? \
             WHERE ProcNum=?",
            (
                ProcedureStatus::Complete.as_od(),
                discount_amount,
                proc_num,
            ),
        )
        .map_err(|e| e.to_string())
    }

    fn list_procedure_codes(&self) -> Result<Vec<ProcedureCode>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                // OD's `procedurecode` has no fee column: fees live only in
                // `fee`. Do not pretend a zero fee came from the schema; the
                // Procedure Codes report resolves amounts from `fee` and
                // renders a blank cell when no fee row exists.
                "SELECT CodeNum,ProcCode,Descript,AbbrDesc,ProcCat FROM procedurecode ORDER BY ProcCode",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (code_num, code, code_name, abbreviation, category_num): (
                    u32,
                    String,
                    String,
                    String,
                    u32,
                ) = from_row(row);
                ProcedureCode {
                    code_num,
                    code,
                    code_name,
                    abbreviation,
                    // No fee is stored on procedurecode; the report uses the
                    // `fee` rows passed to it instead.
                    fee: 0.0,
                    category_num,
                }
            })
            .collect())
    }

    fn list_fee_schedules(&self) -> Result<Vec<FeeSchedule>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT FeeSchedNum,Description FROM feesched ORDER BY ItemOrder,FeeSchedNum",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (fee_sched_num, description): (u32, String) = from_row(row);
                FeeSchedule {
                    fee_sched_num,
                    description,
                }
            })
            .collect())
    }

    fn list_fees(&self, schedule_num: Id, provider_num: Id) -> Result<Vec<Fee>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT FeeNum,CodeNum,FeeSched,Amount FROM fee \
                 WHERE FeeSched=? AND ClinicNum=0 AND ProvNum=? ORDER BY CodeNum,FeeNum",
                (schedule_num, provider_num),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (fee_num, code_num, schedule_num, amount): (u32, u32, u32, f64) =
                    from_row(row);
                Fee {
                    fee_num,
                    code_num,
                    schedule_num,
                    amount,
                }
            })
            .collect())
    }

    fn list_prescriptions(&self) -> Result<Vec<Prescription>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT RxNum,PatNum,CAST(RxDate AS CHAR),COALESCE(Drug,''), \
                 COALESCE(Sig,''),COALESCE(Disp,''),ProvNum FROM rxpat \
                 ORDER BY RxNum",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (rx_num, pat_num, rx_date, drug, sig, disp, prov_num): (
                    u32,
                    u32,
                    String,
                    String,
                    String,
                    String,
                    u32,
                ) = from_row(row);
                Prescription {
                    rx_num,
                    pat_num,
                    // OD uses 0000-00-00 for an empty RxDate; keep a stable
                    // sentinel so the builder can render it as blank.
                    rx_date: od_date(Some(rx_date))
                        .unwrap_or_else(|| NaiveDate::from_ymd_opt(1, 1, 1).unwrap()),
                    drug,
                    sig,
                    disp,
                    prov_num,
                }
            })
            .collect())
    }

    fn list_referrals(&self) -> Result<Vec<Referral>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT ReferralNum,COALESCE(LName,''),COALESCE(FName,''),COALESCE(MName,''), \
                 COALESCE(Title,''),COALESCE(Address,''),COALESCE(Address2,''),COALESCE(City,''), \
                 COALESCE(ST,''),COALESCE(Zip,''),COALESCE(Telephone,''),COALESCE(Phone2,''), \
                 COALESCE(EMail,''),COALESCE(Note,''),COALESCE(SSN,''), \
                 COALESCE(UsingTIN,0),COALESCE(IsHidden,0),COALESCE(NotPerson,0), \
                 COALESCE(Specialty,0) FROM referral ORDER BY ReferralNum",
                (),
            )
            .map_err(|e| e.to_string())?;
        // 19 columns exceed mysql_common's 16-element `FromRow` tuple limit,
        // so read them by index like the patient row does.
        Ok(rows
            .into_iter()
            .map(|row| Referral {
                referral_num: cell_u32(&row, 0),
                l_name: cell_text(&row, 1).unwrap_or_default(),
                f_name: cell_text(&row, 2).unwrap_or_default(),
                m_name: cell_text(&row, 3).unwrap_or_default(),
                title: cell_text(&row, 4).unwrap_or_default(),
                address: cell_text(&row, 5).unwrap_or_default(),
                address2: cell_text(&row, 6).unwrap_or_default(),
                city: cell_text(&row, 7).unwrap_or_default(),
                state: cell_text(&row, 8).unwrap_or_default(),
                zip: cell_text(&row, 9).unwrap_or_default(),
                telephone: cell_text(&row, 10).unwrap_or_default(),
                phone2: cell_text(&row, 11).unwrap_or_default(),
                email: cell_text(&row, 12).unwrap_or_default(),
                note: cell_text(&row, 13).unwrap_or_default(),
                ssn: cell_text(&row, 14).unwrap_or_default(),
                using_tin: cell_u32(&row, 15) as u8,
                is_hidden: cell_u32(&row, 16) as u8,
                not_person: cell_u32(&row, 17) as u8,
                specialty_num: cell_u32(&row, 18),
            })
            .collect())
    }

    fn list_refattaches(&self) -> Result<Vec<RefAttach>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT RefAttachNum,ReferralNum,PatNum,RefType FROM refattach ORDER BY RefAttachNum",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (ref_attach_num, referral_num, pat_num, ref_type): (u32, u32, u32, i8) =
                    from_row(row);
                RefAttach {
                    ref_attach_num,
                    referral_num,
                    pat_num,
                    ref_type: ref_type as u8,
                }
            })
            .collect())
    }

    fn list_proc_notes(&self) -> Result<Vec<ProcNote>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT ProcNoteNum,PatNum,ProcNum,CAST(EntryDateTime AS CHAR),COALESCE(UserNum,0), \
                 COALESCE(Note,''),COALESCE(Signature,'') FROM procnote \
                 ORDER BY ProcNum,EntryDateTime",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (num, pat, proc_num, stamp, user, note, signature): (
                    u32,
                    u32,
                    u32,
                    Option<String>,
                    u32,
                    Option<String>,
                    Option<String>,
                ) = from_row(row);
                ProcNote {
                    proc_note_num: num,
                    pat_num: pat,
                    proc_num,
                    entry_datetime: datetime(stamp).unwrap_or_default(),
                    user_num: user,
                    note: s(note),
                    signature: s(signature),
                }
            })
            .collect())
    }

    fn append_proc_note(&self, note: &ProcNote) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        let user = audit_user_num(&mut conn, note.user_num.max(self.current_user()))?;
        let stamp = if note.entry_datetime == Default::default() {
            Local::now().naive_local()
        } else {
            note.entry_datetime
        };
        conn.exec_drop(
            "INSERT INTO procnote (PatNum,ProcNum,EntryDateTime,UserNum,Note,Signature) \
             VALUES (?,?,?,?,?,?)",
            (
                Value::from(note.pat_num),
                Value::from(note.proc_num),
                Value::from(stamp.format("%Y-%m-%d %H:%M:%S").to_string()),
                Value::from(user),
                Value::from(note.note.clone()),
                Value::from(note.signature.clone()),
            ),
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_id() as Id)
    }

    fn list_comm_logs(&self) -> Result<Vec<CommLog>, String> {
        let mut conn = self.conn()?;
        if !table_exists(&mut conn, "odclinic_appointment_call_log")? {
            return Ok(Vec::new());
        }
        let rows: Vec<mysql::Row> = conn
            .query(
                "SELECT CallLogNum,AptNum,PatNum,CAST(CommDateTime AS CHAR),CommType,CommMode,\
                 SentOrReceived,Note,UserNum FROM odclinic_appointment_call_log \
                 ORDER BY AptNum,CommDateTime",
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| CommLog {
                comm_log_num: cell_u32(&row, 0),
                apt_num: cell_u32(&row, 1),
                pat_num: cell_u32(&row, 2),
                comm_date_time: datetime(cell_text(&row, 3)).unwrap_or_default(),
                comm_type: cell_u32(&row, 4),
                mode: CommLogMode::from_od(cell_i16(&row, 5) as u8),
                sent_or_received: CommLogDirection::from_od(cell_i16(&row, 6) as u8),
                note: s(cell_text(&row, 7)),
                user_num: cell_u32(&row, 8),
            })
            .collect())
    }

    fn upsert_comm_log(&self, log: &CommLog) -> Result<Id, String> {
        let mut conn = self.conn()?;
        if !table_exists(&mut conn, "odclinic_appointment_call_log")? {
            return Err("This database has no appointment call log table. Enable port-local schema bootstrap first.".into());
        }
        let user = audit_user_num(&mut conn, log.user_num.max(self.current_user()))?;
        let stamp = if log.comm_date_time == Default::default() {
            Local::now().naive_local()
        } else {
            log.comm_date_time
        };
        let params = (
            Value::from(log.apt_num),
            Value::from(log.pat_num),
            Value::from(stamp.format("%Y-%m-%d %H:%M:%S").to_string()),
            Value::from(log.comm_type),
            Value::from(log.mode.as_od()),
            Value::from(log.sent_or_received.as_od()),
            Value::from(log.note.clone()),
            Value::from(user),
        );
        if log.comm_log_num == 0 {
            conn.exec_drop(
                "INSERT INTO odclinic_appointment_call_log \
                 (AptNum,PatNum,CommDateTime,CommType,CommMode,SentOrReceived,Note,UserNum) \
                 VALUES (?,?,?,?,?,?,?,?)",
                params,
            )
            .map_err(|e| e.to_string())?;
            Ok(conn.last_insert_id() as Id)
        } else {
            conn.exec_drop(
                "UPDATE odclinic_appointment_call_log SET AptNum=?,PatNum=?,CommDateTime=?,\
                 CommType=?,CommMode=?,SentOrReceived=?,Note=?,UserNum=? WHERE CallLogNum=?",
                (
                    params.0,
                    params.1,
                    params.2,
                    params.3,
                    params.4,
                    params.5,
                    params.6,
                    params.7,
                    Value::from(log.comm_log_num),
                ),
            )
            .map_err(|e| e.to_string())?;
            Ok(log.comm_log_num)
        }
    }

    fn list_accounts(&self) -> Result<Vec<Account>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT AccountNum,COALESCE(Description,''),AcctType,COALESCE(IsRetainedEarnings,0), \
                 COALESCE(Inactive,0) FROM account ORDER BY AcctType,Description",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (num, desc, acct_type, is_re, inactive): (u32, Option<String>, i8, i8, i8) =
                    from_row(row);
                Account {
                    account_num: num,
                    description: s(desc),
                    account_type: AccountType::from_od(acct_type),
                    is_retained_earnings: is_re != 0,
                    inactive: inactive != 0,
                }
            })
            .collect())
    }

    fn list_journal_entries(&self) -> Result<Vec<JournalEntry>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT JournalEntryNum,AccountNum,CAST(DateDisplayed AS CHAR), \
                 COALESCE(DebitAmt,0),COALESCE(CreditAmt,0),COALESCE(Memo,''), \
                 COALESCE(Splits,''),COALESCE(CheckNumber,''),COALESCE(TransactionNum,0) \
                 FROM journalentry ORDER BY DateDisplayed,JournalEntryNum",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (
                    num,
                    account_num,
                    date_displayed,
                    debit,
                    credit,
                    memo,
                    splits,
                    check_number,
                    transaction_num,
                ): (u32, u32, Option<String>, f64, f64, Option<String>, Option<String>, Option<String>, u32) =
                    from_row(row);
                JournalEntry {
                    journal_entry_num: num,
                    account_num,
                    date_displayed: date(date_displayed).unwrap_or_default(),
                    debit_amt: debit,
                    credit_amt: credit,
                    memo: s(memo),
                    splits: s(splits),
                    check_number: s(check_number),
                    transaction_num,
                }
            })
            .collect())
    }

    fn post_journal_transaction(&self, entries: &[JournalEntry]) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        if entries.is_empty() {
            return Err("No journal lines to post".into());
        }
        let transaction_num = entries[0].transaction_num;
        if transaction_num == 0 {
            return Err("Transaction number must be positive".into());
        }
        if entries.iter().any(|e| e.transaction_num != transaction_num) {
            return Err("All journal lines must share one transaction number".into());
        }
        let mut conn = self.conn()?;
        let insert = JournalInsert::new(&mut conn)?;
        let audit_user = audit_user_num(&mut conn, self.current_user())?;
        let now = od_write_datetime(Some(Local::now().naive_local()));
        // A journal transaction is only meaningful as a whole: inserting one
        // side of the pair would make Balance Sheet and P&L disagree.
        let mut tx = conn
            .start_transaction(mysql::TxOpts::default())
            .map_err(|e| e.to_string())?;
        for entry in entries {
            tx.exec_drop(&insert.sql, insert.params(entry, audit_user, &now))
                .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(transaction_num)
    }

    fn post_journal_batch(&self, batches: &[Vec<JournalEntry>]) -> Result<Vec<Id>, String> {
        self.ensure_od_writes_allowed()?;
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
        let mut conn = self.conn()?;
        let insert = JournalInsert::new(&mut conn)?;
        let audit_user = audit_user_num(&mut conn, self.current_user())?;
        let now = od_write_datetime(Some(Local::now().naive_local()));
        // One DB transaction for the whole batch: a failed charge can never
        // leave the ledger with only part of the posting run booked.
        let mut tx = conn
            .start_transaction(mysql::TxOpts::default())
            .map_err(|e| e.to_string())?;
        for entries in batches {
            for entry in entries {
                tx.exec_drop(&insert.sql, insert.params(entry, audit_user, &now))
                    .map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(transaction_nums)
    }

    // --- Report fidelity tranche 3 ---------------------------------------

    fn list_schedules(&self) -> Result<Vec<Schedule>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT ScheduleNum,CAST(SchedDate AS CHAR),CAST(StartTime AS CHAR), \
                 CAST(StopTime AS CHAR),SchedType,ProvNum,BlockoutType,COALESCE(Note,''), \
                 Status,EmployeeNum,ClinicNum FROM schedule ORDER BY SchedDate,StartTime",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (
                    num,
                    sched_date,
                    start,
                    stop,
                    sched_type,
                    prov_num,
                    blockout_type,
                    note,
                    status,
                    employee_num,
                    clinic_num,
                ): (
                    u32,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    u8,
                    u32,
                    u32,
                    Option<String>,
                    u8,
                    u32,
                    u32,
                ) = from_row(row);
                Schedule {
                    schedule_num: num,
                    sched_date: date(sched_date).unwrap_or_default(),
                    start_time: time(start),
                    stop_time: time(stop),
                    sched_type,
                    prov_num,
                    blockout_type,
                    note: s(note),
                    status,
                    employee_num,
                    clinic_num,
                }
            })
            .collect())
    }

    fn list_schedule_ops(&self) -> Result<Vec<ScheduleOp>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<(u32, u32, u32)> = conn
            .exec(
                "SELECT ScheduleOpNum,ScheduleNum,OperatoryNum FROM scheduleop \
                 ORDER BY ScheduleNum,OperatoryNum",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|(num, schedule_num, operatory_num)| ScheduleOp {
                schedule_op_num: num,
                schedule_num,
                operatory_num,
            })
            .collect())
    }

    fn list_pay_plan_links(&self) -> Result<Vec<PayPlanLink>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<(u32, u32, i8, u32, f64)> = conn
            .exec(
                "SELECT PayPlanLinkNum,PayPlanNum,LinkType,FKey,COALESCE(AmountOverride,0) \
                 FROM payplanlink ORDER BY PayPlanNum,PayPlanLinkNum",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(
                |(num, pay_plan_num, link_type, fkey, amount_override)| PayPlanLink {
                    pay_plan_link_num: num,
                    pay_plan_num,
                    link_type,
                    fkey,
                    amount_override,
                },
            )
            .collect())
    }

    fn upsert_pay_plan_link(&self, link: &PayPlanLink) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        if link.pay_plan_link_num == 0 {
            conn.exec_drop(
                "INSERT INTO payplanlink (PayPlanNum,LinkType,FKey,AmountOverride) VALUES (?,?,?,?)",
                (
                    link.pay_plan_num,
                    link.link_type,
                    link.fkey,
                    link.amount_override,
                ),
            )
            .map_err(|e| e.to_string())?;
            Ok(conn.last_insert_id() as Id)
        } else {
            conn.exec_drop(
                "UPDATE payplanlink SET PayPlanNum=?,LinkType=?,FKey=?,AmountOverride=? WHERE PayPlanLinkNum=?",
                (
                    link.pay_plan_num,
                    link.link_type,
                    link.fkey,
                    link.amount_override,
                    link.pay_plan_link_num,
                ),
            )
            .map_err(|e| e.to_string())?;
            Ok(link.pay_plan_link_num)
        }
    }

    fn delete_pay_plan_link(&self, link_num: Id) -> Result<(), String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        conn.exec_drop(
            "DELETE FROM payplanlink WHERE PayPlanLinkNum=?",
            (link_num,),
        )
        .map_err(|e| e.to_string())
    }

    fn delete_pay_plan_charge(&self, charge_num: Id) -> Result<(), String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        conn.exec_drop(
            "DELETE FROM payplancharge WHERE PayPlanChargeNum=?",
            (charge_num,),
        )
        .map_err(|e| e.to_string())
    }

    fn list_claim_snapshots(&self) -> Result<Vec<ClaimSnapshot>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT ClaimSnapshotNum,ProcNum,COALESCE(ClaimType,''), \
                 COALESCE(Writeoff,0),COALESCE(InsPayEst,0),COALESCE(Fee,0), \
                 CAST(DateTEntry AS CHAR),ClaimProcNum,COALESCE(SnapshotTrigger,0) \
                 FROM claimsnapshot ORDER BY DateTEntry,ClaimSnapshotNum",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (
                    num,
                    proc_num,
                    claim_type,
                    write_off,
                    ins_pay_est,
                    fee,
                    stamp,
                    claim_proc_num,
                    trigger,
                ): (
                    u32,
                    u32,
                    Option<String>,
                    f64,
                    f64,
                    f64,
                    Option<String>,
                    u32,
                    i8,
                ) = from_row(row);
                ClaimSnapshot {
                    claim_snapshot_num: num,
                    proc_num,
                    claim_type: s(claim_type),
                    write_off,
                    ins_pay_est,
                    fee,
                    date_t_entry: datetime(stamp).unwrap_or_default(),
                    claim_proc_num,
                    snapshot_trigger: trigger,
                }
            })
            .collect())
    }

    fn list_discount_plans(&self) -> Result<Vec<DiscountPlan>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT DiscountPlanNum,COALESCE(Description,''),FeeSchedNum,DefNum, \
                 COALESCE(IsHidden,0),COALESCE(PlanNote,''), \
                 COALESCE(ExamFreqLimit,-1),COALESCE(XrayFreqLimit,-1), \
                 COALESCE(ProphyFreqLimit,-1),COALESCE(FluorideFreqLimit,-1), \
                 COALESCE(PerioFreqLimit,-1),COALESCE(LimitedExamFreqLimit,-1), \
                 COALESCE(PAFreqLimit,-1),COALESCE(AnnualMax,-1) \
                 FROM discountplan ORDER BY Description",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| DiscountPlan {
                discount_plan_num: cell_u32(&row, 0),
                description: s(cell_text(&row, 1)),
                fee_sched_num: cell_u32(&row, 2),
                def_num: cell_u32(&row, 3),
                is_hidden: cell_i16(&row, 4) != 0,
                note: s(cell_text(&row, 5)),
                exam_freq_limit: cell_i32(&row, 6),
                xray_freq_limit: cell_i32(&row, 7),
                prophy_freq_limit: cell_i32(&row, 8),
                fluoride_freq_limit: cell_i32(&row, 9),
                perio_freq_limit: cell_i32(&row, 10),
                limited_exam_freq_limit: cell_i32(&row, 11),
                pa_freq_limit: cell_i32(&row, 12),
                annual_max: cell_f64(&row, 13),
            })
            .collect())
    }

    fn list_discount_plan_subs(&self) -> Result<Vec<DiscountPlanSub>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT DiscountSubNum,DiscountPlanNum,PatNum,CAST(DateEffective AS CHAR), \
                 CAST(DateTerm AS CHAR),COALESCE(SubNote,'') FROM discountplansub \
                 ORDER BY DiscountPlanNum,PatNum",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (num, plan_num, pat_num, effective, term, note): (
                    u32,
                    u32,
                    u32,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                ) = from_row(row);
                DiscountPlanSub {
                    discount_sub_num: num,
                    discount_plan_num: plan_num,
                    pat_num,
                    date_effective: date(effective),
                    date_term: date(term),
                    note: s(note),
                }
            })
            .collect())
    }

    fn upsert_discount_plan(&self, plan: &DiscountPlan) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        // Mirrors DiscountPlanCrud.Update/Insert. A zero key is OD's convention
        // for a new row; AUTO_INCREMENT assigns the real DiscountPlanNum.
        let sql = "INSERT INTO discountplan \
                   (DiscountPlanNum,Description,FeeSchedNum,DefNum,IsHidden,PlanNote, \
                    ExamFreqLimit,XrayFreqLimit,ProphyFreqLimit,FluorideFreqLimit, \
                    PerioFreqLimit,LimitedExamFreqLimit,PAFreqLimit,AnnualMax) \
                   VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?) \
                   ON DUPLICATE KEY UPDATE Description=VALUES(Description),\
                    FeeSchedNum=VALUES(FeeSchedNum),DefNum=VALUES(DefNum),\
                    IsHidden=VALUES(IsHidden),PlanNote=VALUES(PlanNote),\
                    ExamFreqLimit=VALUES(ExamFreqLimit),XrayFreqLimit=VALUES(XrayFreqLimit),\
                    ProphyFreqLimit=VALUES(ProphyFreqLimit),FluorideFreqLimit=VALUES(FluorideFreqLimit),\
                    PerioFreqLimit=VALUES(PerioFreqLimit),\
                    LimitedExamFreqLimit=VALUES(LimitedExamFreqLimit),PAFreqLimit=VALUES(PAFreqLimit),\
                    AnnualMax=VALUES(AnnualMax)";
        let num = plan.discount_plan_num;
        let params: Vec<Value> = vec![
            Value::from(num),
            Value::from(plan.description.clone()),
            Value::from(plan.fee_sched_num),
            Value::from(plan.def_num),
            Value::from(i8::from(plan.is_hidden)),
            Value::from(plan.note.clone()),
            Value::from(plan.exam_freq_limit),
            Value::from(plan.xray_freq_limit),
            Value::from(plan.prophy_freq_limit),
            Value::from(plan.fluoride_freq_limit),
            Value::from(plan.perio_freq_limit),
            Value::from(plan.limited_exam_freq_limit),
            Value::from(plan.pa_freq_limit),
            Value::from(plan.annual_max),
        ];
        conn.exec_drop(sql, params).map_err(|e| e.to_string())?;
        if num == 0 {
            Ok(conn.last_insert_id() as Id)
        } else {
            Ok(num)
        }
    }

    fn upsert_discount_plan_sub(&self, sub: &DiscountPlanSub) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        let sql = "INSERT INTO discountplansub \
                   (DiscountSubNum,DiscountPlanNum,PatNum,DateEffective,DateTerm,SubNote) \
                   VALUES (?,?,?,?,?,?) \
                   ON DUPLICATE KEY UPDATE DiscountPlanNum=VALUES(DiscountPlanNum),\
                    PatNum=VALUES(PatNum),DateEffective=VALUES(DateEffective),\
                    DateTerm=VALUES(DateTerm),SubNote=VALUES(SubNote)";
        let num = sub.discount_sub_num;
        let params: Vec<Value> = vec![
            Value::from(num),
            Value::from(sub.discount_plan_num),
            Value::from(sub.pat_num),
            Value::from(od_date_param(sub.date_effective)),
            Value::from(od_date_param(sub.date_term)),
            Value::from(sub.note.clone()),
        ];
        conn.exec_drop(sql, params).map_err(|e| e.to_string())?;
        if num == 0 {
            Ok(conn.last_insert_id() as Id)
        } else {
            Ok(num)
        }
    }

    fn terminate_discount_plan_sub(
        &self,
        discount_sub_num: Id,
        term: NaiveDate,
    ) -> Result<(), String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        // OD has no "terminated" flag: ending a subscription means setting
        // DateTerm. The row is retained so historic adjustments still resolve.
        conn.exec_drop(
            "UPDATE discountplansub SET DateTerm=? WHERE DiscountSubNum=?",
            (term.format("%Y-%m-%d").to_string(), discount_sub_num),
        )
        .map_err(|e| e.to_string())
    }

    fn list_screen_groups(&self) -> Result<Vec<ScreenGroup>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT ScreenGroupNum,COALESCE(Description,''),CAST(SGDate AS CHAR), \
                 COALESCE(ProvName,''),ProvNum,PlaceService,COALESCE(County,''), \
                 COALESCE(GradeSchool,'') FROM screengroup ORDER BY ScreenGroupNum",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (num, desc, sg_date, prov_name, prov_num, place, county, school): (
                    u32,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    u32,
                    i8,
                    Option<String>,
                    Option<String>,
                ) = from_row(row);
                ScreenGroup {
                    screen_group_num: num,
                    description: s(desc),
                    sg_date: date(sg_date),
                    prov_name: s(prov_name),
                    prov_num,
                    place_service: place,
                    county: s(county),
                    grade_school: s(school),
                }
            })
            .collect())
    }

    fn list_screens(&self) -> Result<Vec<Screen>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT ScreenNum,Gender,RaceOld,GradeLevel,Age,Urgency,HasCaries, \
                 NeedsSealants,CariesExperience,EarlyChildCaries,ExistingSealants, \
                 MissingAllTeeth,CAST(Birthdate AS CHAR),ScreenGroupNum, \
                 ScreenGroupOrder,COALESCE(Comments,''),ScreenPatNum FROM screen \
                 ORDER BY ScreenGroupNum,ScreenGroupOrder",
                (),
            )
            .map_err(|e| e.to_string())?;
        // 17 columns exceed `FromRow`'s tuple limit, so map by index.
        Ok(rows
            .into_iter()
            .map(|row| Screen {
                screen_num: cell_u32(&row, 0),
                gender: cell_u32(&row, 1) as u8,
                race_old: cell_u32(&row, 2) as i8,
                grade_level: cell_u32(&row, 3) as i8,
                age: cell_u32(&row, 4) as u8,
                urgency: cell_u32(&row, 5) as i8,
                has_caries: cell_u32(&row, 6) != 0,
                needs_sealants: cell_u32(&row, 7) != 0,
                caries_experience: cell_u32(&row, 8) != 0,
                early_child_caries: cell_u32(&row, 9) != 0,
                existing_sealants: cell_u32(&row, 10) != 0,
                missing_all_teeth: cell_u32(&row, 11) != 0,
                birth_date: date(cell_text(&row, 12)),
                screen_group_num: cell_u32(&row, 13),
                screen_group_order: cell_u32(&row, 14) as u16,
                comments: s(cell_text(&row, 15)),
                screen_pat_num: cell_u32(&row, 16),
            })
            .collect())
    }

    fn list_sites(&self) -> Result<Vec<Site>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT SiteNum,COALESCE(Description,''),COALESCE(Note,''), \
                 COALESCE(Address,''),COALESCE(Address2,''),COALESCE(City,''), \
                 COALESCE(State,''),COALESCE(Zip,''),ProvNum,PlaceService \
                 FROM site ORDER BY Description",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (num, desc, note, addr, addr2, city, state, zip, prov_num, place): (
                    u32,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    u32,
                    i8,
                ) = from_row(row);
                Site {
                    site_num: num,
                    description: s(desc),
                    note: s(note),
                    address: s(addr),
                    address2: s(addr2),
                    city: s(city),
                    state: s(state),
                    zip: s(zip),
                    prov_num,
                    place_service: place,
                }
            })
            .collect())
    }

    fn list_counties(&self) -> Result<Vec<County>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<(u32, Option<String>, Option<String>)> = conn
            .exec(
                "SELECT CountyNum,COALESCE(CountyName,''),COALESCE(CountyCode,'') \
                 FROM county ORDER BY CountyName",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|(num, name, code)| County {
                county_num: num,
                county_name: s(name),
                county_code: s(code),
            })
            .collect())
    }

    fn list_patient_races(&self) -> Result<Vec<PatientRace>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<(u32, u32, i8, Option<String>)> = conn
            .exec(
                "SELECT PatientRaceNum,PatNum,Race,COALESCE(CdcrecCode,'') \
                 FROM patientrace ORDER BY PatNum,PatientRaceNum",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|(num, pat_num, race, code)| PatientRace {
                patient_race_num: num,
                pat_num,
                race,
                cdcrec_code: s(code),
            })
            .collect())
    }

    fn list_carriers(&self) -> Result<Vec<Carrier>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT CarrierNum,COALESCE(CarrierName,''),COALESCE(Address,''), \
                 COALESCE(Address2,''),COALESCE(City,''),COALESCE(State,''), \
                 COALESCE(Zip,''),COALESCE(Phone,''),COALESCE(IsHidden,0) \
                 FROM carrier ORDER BY CarrierName",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (num, name, addr, addr2, city, state, zip, phone, hidden): (
                    u32,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    i8,
                ) = from_row(row);
                Carrier {
                    carrier_num: num,
                    carrier_name: s(name),
                    address: s(addr),
                    address2: s(addr2),
                    city: s(city),
                    state: s(state),
                    zip: s(zip),
                    phone: s(phone),
                    is_hidden: hidden != 0,
                }
            })
            .collect())
    }

    fn list_diseases(&self, pat_num: Id) -> Result<Vec<Disease>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT d.DiseaseNum,d.PatNum,d.DiseaseDefNum,COALESCE(dd.DiseaseName,''), \
                 COALESCE(d.PatNote,''),d.ProbStatus,CAST(d.DateStart AS CHAR),CAST(d.DateStop AS CHAR) \
                 FROM disease d LEFT JOIN diseasedef dd ON dd.DiseaseDefNum = d.DiseaseDefNum \
                 WHERE d.PatNum = ? ORDER BY CAST(d.DateStart AS CHAR) DESC",
                (Value::from(pat_num),),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (
                    disease_num,
                    pat,
                    disease_def_num,
                    name,
                    note,
                    status,
                    start,
                    stop,
                ): (u32, u32, u32, Option<String>, Option<String>, i8, Option<String>, Option<String>) =
                    from_row(row);
                Disease {
                    disease_num,
                    pat_num: pat,
                    disease_def_num,
                    disease_name: s(name),
                    note: s(note),
                    status: ProblemStatus::from_od(status),
                    date_start: od_date(start).unwrap_or_default(),
                    date_stop: od_date(stop),
                }
            })
            .collect())
    }

    fn get_patient_note(&self, pat_num: Id) -> Result<Option<PatientNote>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT PatNum,COALESCE(Medical,''),COALESCE(Treatment,''),COALESCE(Service,''), \
                 COALESCE(MedicalComp,''),COALESCE(FamFinancial,'') FROM patientnote WHERE PatNum = ?",
                (Value::from(pat_num),),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().next().map(|row| {
            let (pat, medical, treatment, service, medical_company, fam_financial): (
                u32,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
            ) = from_row(row);
            PatientNote {
                pat_num: pat,
                medical: s(medical),
                treatment: s(treatment),
                service: s(service),
                medical_company: s(medical_company),
                fam_financial: s(fam_financial),
            }
        }))
    }

    fn list_disease_defs(&self) -> Result<Vec<DiseaseDef>, String> {
        let mut conn = self.conn()?;
        if !table_exists(&mut conn, "diseasedef")? {
            return Ok(Vec::new());
        }
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT DiseaseDefNum,COALESCE(DiseaseName,''),COALESCE(ICD9Code,''), \
                 COALESCE(Icd10Code,''),COALESCE(IsHidden,0) FROM diseasedef \
                 ORDER BY DiseaseName,DiseaseDefNum",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (num, name, icd9, icd10, hidden): (
                    u32,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    i8,
                ) = from_row(row);
                DiseaseDef {
                    disease_def_num: num,
                    disease_name: s(name),
                    icd9_code: s(icd9),
                    icd10_code: s(icd10),
                    is_hidden: hidden != 0,
                }
            })
            .collect())
    }

    fn upsert_patient_note(&self, note: &PatientNote) -> Result<(), String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        let found: Option<i64> = conn
            .exec_first(
                "SELECT COUNT(*) FROM patientnote WHERE PatNum = ?",
                (Value::from(note.pat_num),),
            )
            .map_err(|e| e.to_string())?;
        if found.unwrap_or(0) > 0 {
            // OD's patient note is one-to-one with the chart. Only the four
            // chart-owned fields are touched; FamFinancial and the family note
            // semantics are deliberately left alone.
            conn.exec_drop(
                "UPDATE patientnote SET Medical = ?, Treatment = ?, Service = ?, \
                 MedicalComp = ? WHERE PatNum = ?",
                (
                    Value::from(note.medical.clone()),
                    Value::from(note.treatment.clone()),
                    Value::from(note.service.clone()),
                    Value::from(note.medical_company.clone()),
                    Value::from(note.pat_num),
                ),
            )
            .map_err(|e| e.to_string())?;
            return Ok(());
        }
        let mut columns = vec![
            "PatNum",
            "FamFinancial",
            "ApptPhone",
            "Medical",
            "Service",
            "MedicalComp",
            "Treatment",
        ];
        let mut params: Vec<Value> = vec![
            Value::from(note.pat_num),
            Value::from(""),
            Value::from(""),
            Value::from(note.medical.clone()),
            Value::from(note.service.clone()),
            Value::from(note.medical_company.clone()),
            Value::from(note.treatment.clone()),
        ];
        // OD 24_3 added these columns after the original patientnote table.
        // A reduced demo schema can omit them, so each one is checked before
        // being included in the INSERT.
        for (column, value) in [
            ("ICEName", Value::from("")),
            ("ICEPhone", Value::from("")),
            ("OrthoMonthsTreatOverride", Value::from(-1i32)),
            ("DateOrthoPlacementOverride", Value::from("0001-01-01")),
            (
                "SecDateTEntry",
                Value::from(Local::now().format("%Y-%m-%d %H:%M:%S").to_string()),
            ),
            ("Consent", Value::from(0i32)),
            ("UserNumOrthoLocked", Value::from(0u32)),
            ("Pronoun", Value::from(0i32)),
        ] {
            if column_exists(&mut conn, "patientnote", column)? {
                columns.push(column);
                params.push(value);
            }
        }
        let sql = format!(
            "INSERT INTO patientnote ({}) VALUES ({})",
            columns.join(","),
            vec!["?"; columns.len()].join(",")
        );
        conn.exec_drop(&sql, params).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn upsert_disease(&self, disease: &Disease) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        if disease.disease_def_num == 0 {
            return Err("Select a condition before saving.".into());
        }
        let mut conn = self.conn()?;
        let start = disease.date_start.format("%Y-%m-%d").to_string();
        let stop = disease
            .date_stop
            .map(|date| date.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "0001-01-01".into());
        if disease.disease_num != 0 {
            conn.exec_drop(
                "UPDATE disease SET DiseaseDefNum = ?, PatNote = ?, ProbStatus = ?, \
                 DateStart = ?, DateStop = ? WHERE DiseaseNum = ? AND PatNum = ?",
                (
                    Value::from(disease.disease_def_num),
                    Value::from(disease.note.clone()),
                    Value::from(disease.status.as_od()),
                    Value::from(start),
                    Value::from(stop),
                    Value::from(disease.disease_num),
                    Value::from(disease.pat_num),
                ),
            )
            .map_err(|e| e.to_string())?;
            if conn.affected_rows() == 0 {
                return Err(format!("Condition #{} no longer exists.", disease.disease_num));
            }
            return Ok(disease.disease_num);
        }
        let mut columns = vec![
            "PatNum",
            "DiseaseDefNum",
            "PatNote",
            "ProbStatus",
            "DateStart",
            "DateStop",
        ];
        let mut params: Vec<Value> = vec![
            Value::from(disease.pat_num),
            Value::from(disease.disease_def_num),
            Value::from(disease.note.clone()),
            Value::from(disease.status.as_od()),
            Value::from(start),
            Value::from(stop),
        ];
        for (column, value) in [
            ("SnomedProblemType", Value::from("")),
            ("FunctionStatus", Value::from(0i32)),
        ] {
            if column_exists(&mut conn, "disease", column)? {
                columns.push(column);
                params.push(value);
            }
        }
        let sql = format!(
            "INSERT INTO disease ({}) VALUES ({})",
            columns.join(","),
            vec!["?"; columns.len()].join(",")
        );
        conn.exec_drop(&sql, params).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_id() as Id)
    }

    fn list_registry_treatments(&self, pat_num: Id) -> Result<Vec<RegistryTreatment>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT ProcNum,PatNum,CAST(ProcDate AS CHAR),COALESCE(CodeNum,0), \
                 COALESCE(ToothNum,''),COALESCE(Surf,''),ProcFee,ProcStatus \
                 FROM procedurelog WHERE PatNum = ? ORDER BY CAST(ProcDate AS CHAR) DESC",
                (Value::from(pat_num),),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (proc_num, pat, proc_date, code_num, tooth, surf, fee, status): (
                    u32,
                    u32,
                    Option<String>,
                    u32,
                    Option<String>,
                    Option<String>,
                    f64,
                    i8,
                ) = from_row(row);
                let tooth_site = match (s(tooth), s(surf)) {
                    (tooth, surf) if !surf.is_empty() => format!("{tooth} {surf}"),
                    (tooth, _) => tooth,
                };
                RegistryTreatment {
                    proc_num,
                    pat_num: pat,
                    proc_date: date(proc_date).unwrap_or_default(),
                    description: format!("Procedure code #{code_num}"),
                    tooth_site,
                    fee,
                    proc_status: status,
                }
            })
            .collect())
    }

    fn list_clinic_cards(&self, pat_num: Id) -> Result<Vec<ClinicCard>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT CardNum,PatNum,CardNumber,CAST(IssuedAt AS CHAR),IssuedBy,Reason, \
                 Authorization,Note,IsActive FROM odclinic_clinic_card WHERE PatNum = ? \
                 ORDER BY IssuedAt DESC, CardNum DESC",
                (Value::from(pat_num),),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (card_num, pat, number, issued, issued_by, reason, auth, note, active): (
                    u32,
                    u32,
                    Option<String>,
                    Option<String>,
                    u32,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    i8,
                ) = from_row(row);
                ClinicCard {
                    card_num,
                    pat_num: pat,
                    card_number: s(number),
                    issued_at: date(issued).unwrap_or_default(),
                    issued_by,
                    reason: ClinicCardReason::from_str(&s(reason)),
                    authorization: s(auth),
                    note: s(note),
                    is_active: active != 0,
                }
            })
            .collect())
    }

    fn issue_clinic_card(&self, card: &ClinicCard) -> Result<Id, String> {
        let mut conn = self.conn()?;
        conn.exec_drop(
            "INSERT INTO odclinic_clinic_card \
             (PatNum,CardNumber,IssuedAt,IssuedBy,Reason,Authorization,Note,IsActive) \
             VALUES (?,?,?,?,?,?,?,1)",
            (
                Value::from(card.pat_num),
                Value::from(card.card_number.as_str()),
                Value::from(od_date_param(Some(card.issued_at))),
                Value::from(card.issued_by),
                Value::from(card.reason.as_str()),
                Value::from(card.authorization.as_str()),
                Value::from(card.note.as_str()),
            ),
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_id() as Id)
    }

    fn deactivate_clinic_card(&self, card_num: Id) -> Result<(), String> {
        let mut conn = self.conn()?;
        conn.exec_drop(
            "UPDATE odclinic_clinic_card SET IsActive = 0 WHERE CardNum = ?",
            (Value::from(card_num),),
        )
        .map_err(|e| e.to_string())
    }

    fn list_payments(&self, pat_num: Id) -> Result<Vec<Payment>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT PayNum,PatNum,CAST(PayDate AS CHAR),PayAmt,PayType,CheckNum,PayNote FROM payment WHERE PatNum = ? ORDER BY PayDate DESC",
                (Value::from(pat_num),),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (pay_num, pat, pay_date, amount, pay_type, check_num, note): (u32, u32, Option<String>, f64, i8, Option<String>, Option<String>) = from_row(row);
                Payment {
                    pay_num,
                    pat_num: pat,
                    pay_date: date(pay_date).unwrap_or_default(),
                    pay_amount: amount,
                    pay_type,
                    check_num: s(check_num),
                    note: s(note),
                }
            })
            .collect())
    }

    fn list_adjustments(&self, pat_num: Id) -> Result<Vec<Adjustment>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT AdjNum,CAST(AdjDate AS CHAR),AdjAmt,PatNum,AdjType,ProvNum,ProcNum,AdjNote,CAST(ProcDate AS CHAR) FROM adjustment WHERE PatNum = ? ORDER BY AdjDate DESC",
                (Value::from(pat_num),),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (adj_num, adj_date, amount, pat, adj_type, prov_num, proc_num, note, proc_date): (u32, Option<String>, f64, u32, u32, u32, u32, Option<String>, Option<String>) = from_row(row);
                Adjustment {
                    adj_num,
                    adj_date: date(adj_date).unwrap_or_default(),
                    adj_amount: amount,
                    pat_num: pat,
                    adj_type,
                    provider_num: prov_num,
                    proc_num,
                    proc_date: date(proc_date).unwrap_or_default(),
                    note: s(note),
                }
            })
            .collect())
    }

    fn list_pay_splits(&self, pat_num: Id) -> Result<Vec<PaySplit>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT SplitNum,PayNum,PatNum,ProcNum,SplitAmt,CAST(DatePay AS CHAR),ProvNum,PayPlanNum,AdjNum,COALESCE(UnearnedType,0),COALESCE(PayPlanChargeNum,0),COALESCE(PayPlanDebitType,0) FROM paysplit WHERE PatNum = ? ORDER BY DatePay DESC",
                (Value::from(pat_num),),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (split_num, pay_num, pat, proc_num, amount, pay_date, prov_num, pay_plan_num, adj_num, unearned_type, pay_plan_charge_num, pay_plan_debit_type): (u32, u32, u32, u32, f64, Option<String>, u32, u32, u32, u32, u32, i32) = from_row(row);
                PaySplit {
                    split_num,
                    pay_num,
                    pat_num: pat,
                    proc_num,
                    split_amount: amount,
                    date_pay: date(pay_date).unwrap_or_default(),
                    provider_num: prov_num,
                    pay_plan_num,
                    adj_num,
                    unearned_type,
                    pay_plan_charge_num,
                    pay_plan_debit_type: PayPlanDebitType::from_db(pay_plan_debit_type),
                }
            })
            .collect())
    }

    fn list_claims(&self, pat_num: Id) -> Result<Vec<Claim>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                // OD stores ClaimStatus as a one-character string, not a number.
                "SELECT ClaimNum,PatNum,PlanNum,ClaimStatus,CAST(DateSent AS CHAR),ClaimFee,ClaimIdentifier FROM claim WHERE PatNum = ? ORDER BY DateSent DESC",
                (Value::from(pat_num),),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (claim_num, pat, plan_num, status, date_sent, fee, identifier): (u32, u32, u32, Option<String>, Option<String>, f64, Option<String>) = from_row(row);
                Claim {
                    claim_num,
                    pat_num: pat,
                    plan_num,
                    claim_status: ClaimStatus::from_od(&s(status)),
                    date_sent: date(date_sent),
                    total_fee: fee,
                    claim_identifier: s(identifier),
                }
            })
            .collect())
    }

    fn list_claim_procs(&self, pat_num: Id) -> Result<Vec<ClaimProc>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT cp.ClaimProcNum,cp.ClaimNum,cp.ProcNum,cp.PlanNum,cp.Status,cp.FeeBilled,cp.InsPayAmt,cp.CopayAmt,cp.WriteOff,cp.InsPayEst, \
                 cp.WriteOffEst,cp.WriteOffEstOverride,CAST(cp.DateSuppReceived AS CHAR),cp.ClaimPaymentNum \
                 FROM claimproc cp INNER JOIN claim c ON c.ClaimNum = cp.ClaimNum \
                 WHERE c.PatNum = ? ORDER BY cp.ClaimProcNum",
                (Value::from(pat_num),),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                // 14 columns exceeds this crate's tuple `FromRow` support;
                // read by index like the wider patient row.
                ClaimProc {
                    claim_proc_num: cell_u32(&row, 0),
                    claim_num: cell_u32(&row, 1),
                    proc_num: cell_u32(&row, 2),
                    plan_num: cell_u32(&row, 3),
                    status: cell_i16(&row, 4) as i8,
                    fee_billed: cell_f64(&row, 5),
                    ins_pay_amt: cell_f64(&row, 6),
                    copay_amt: cell_f64(&row, 7),
                    write_off: cell_f64(&row, 8),
                    ins_pay_est: cell_f64(&row, 9),
                    // OD 24.3/25.3 claimproc has no AllowedAmt column; the
                    // estimate fields are InsPayEst and WriteOffEst.
                    allowed_amt: 0.0,
                    write_off_est: cell_f64(&row, 10),
                    write_off_est_override: cell_f64(&row, 11),
                    date_supp_received: date(cell_text(&row, 12)),
                    claim_payment_num: cell_u32(&row, 13),
                }
            })
            .collect())
    }

    fn list_statements(&self, pat_num: Id) -> Result<Vec<Statement>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT StatementNum,PatNum,CAST(DateSent AS CHAR),Note,IsInvoice FROM statement WHERE PatNum = ? ORDER BY DateSent DESC",
                (Value::from(pat_num),),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (num, pat, date_sent, note, is_invoice): (u32, u32, Option<String>, Option<String>, i8) = from_row(row);
                Statement {
                    statement_num: num,
                    pat_num: pat,
                    date_sent: date(date_sent),
                    note: s(note),
                    is_invoice: is_invoice != 0,
                }
            })
            .collect())
    }

    fn insert_statement(&self, statement: &Statement) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        // OD stores an unsent statement as the sentinel date 0001-01-01 and
        // requires every bookkeeping column to be populated explicitly.
        let date_sent = od_write_date(statement.date_sent);
        conn.exec_drop(
            "INSERT INTO statement \
             (PatNum,SuperFamily,DateSent,DateRangeFrom,DateRangeTo,Note,NoteBold,Mode_,\
              HidePayment,SinglePatient,Intermingled,IsSent,DocNum,IsReceipt,IsInvoice,\
              IsInvoiceCopy,EmailSubject,EmailBody,IsBalValid,InsEst,BalTotal,StatementType,\
              ShortGUID,StatementURL,StatementShortURL,SmsSendStatus,LimitedCustomFamily) \
             VALUES (?,0,?,?,?,?,'',0,0,1,0,0,0,0,?,0,'','',0,0,0,'NotSet','','','',0,0)",
            (
                statement.pat_num,
                date_sent,
                od_write_date(None),
                od_write_date(None),
                statement.note.clone(),
                statement.is_invoice as i8,
            ),
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_id() as Id)
    }

    fn list_laboratories(&self) -> Result<Vec<Laboratory>, String> {
        let mut conn = self.conn()?;
        if !table_exists(&mut conn, "laboratory")? {
            return Ok(Vec::new());
        }
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT LaboratoryNum,COALESCE(Description,'') FROM laboratory ORDER BY Description",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| Laboratory {
                laboratory_num: cell_u32(&row, 0),
                description: s(cell_text(&row, 1)),
            })
            .collect())
    }

    fn list_lab_cases(&self, pat_num: Id) -> Result<Vec<LabCase>, String> {
        let mut conn = self.conn()?;
        if !table_exists(&mut conn, "labcase")? {
            return Ok(Vec::new());
        }
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT LabCaseNum,PatNum,LaboratoryNum,COALESCE(AptNum,0),COALESCE(PlannedAptNum,0), \
                 CAST(DateTimeDue AS CHAR),CAST(DateTimeCreated AS CHAR),CAST(DateTimeSent AS CHAR), \
                 CAST(DateTimeRecd AS CHAR),CAST(DateTimeChecked AS CHAR),COALESCE(ProvNum,0), \
                 COALESCE(Instructions,''),COALESCE(LabFee,0),COALESCE(InvoiceNum,'') \
                 FROM labcase WHERE PatNum = ? ORDER BY DateTimeDue DESC",
                (Value::from(pat_num),),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| LabCase {
                lab_case_num: cell_u32(&row, 0),
                pat_num: cell_u32(&row, 1),
                laboratory_num: cell_u32(&row, 2),
                apt_num: cell_u32(&row, 3),
                planned_apt_num: cell_u32(&row, 4),
                date_time_due: od_datetime(cell_text(&row, 5)),
                date_time_created: od_datetime(cell_text(&row, 6)),
                date_time_sent: od_datetime(cell_text(&row, 7)),
                date_time_recd: od_datetime(cell_text(&row, 8)),
                date_time_checked: od_datetime(cell_text(&row, 9)),
                prov_num: cell_u32(&row, 10),
                instructions: s(cell_text(&row, 11)),
                lab_fee: cell_f64(&row, 12),
                invoice_num: s(cell_text(&row, 13)),
            })
            .collect())
    }

    fn upsert_lab_case(&self, case: &LabCase) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        if !table_exists(&mut conn, "labcase")? {
            return Err("This Open Dental schema has no labcase table.".into());
        }
        // mysql_common implements positional Params only for small tuples; a
        // Vec keeps wide OD rows readable and still binds every value.
        let values: Vec<Value> = vec![
            Value::from(case.pat_num),
            Value::from(case.laboratory_num),
            Value::from(case.apt_num),
            Value::from(case.planned_apt_num),
            Value::from(od_write_datetime(case.date_time_due)),
            Value::from(od_write_datetime(case.date_time_created)),
            Value::from(od_write_datetime(case.date_time_sent)),
            Value::from(od_write_datetime(case.date_time_recd)),
            Value::from(od_write_datetime(case.date_time_checked)),
            Value::from(case.prov_num),
            Value::from(case.instructions.clone()),
            Value::from(case.lab_fee),
            Value::from(case.invoice_num.clone()),
        ];
        if case.lab_case_num == 0 {
            conn.exec_drop(
                "INSERT INTO labcase \
                 (PatNum,LaboratoryNum,AptNum,PlannedAptNum,DateTimeDue,DateTimeCreated, \
                  DateTimeSent,DateTimeRecd,DateTimeChecked,ProvNum,Instructions,LabFee,InvoiceNum) \
                 VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
                values,
            )
            .map_err(|e| e.to_string())?;
            Ok(conn.last_insert_id() as Id)
        } else {
            conn.exec_drop(
                "UPDATE labcase SET PatNum=?,LaboratoryNum=?,AptNum=?,PlannedAptNum=?, \
                 DateTimeDue=?,DateTimeCreated=?,DateTimeSent=?,DateTimeRecd=?,DateTimeChecked=?, \
                 ProvNum=?,Instructions=?,LabFee=?,InvoiceNum=? WHERE LabCaseNum=?",
                {
                    let mut params = values;
                    params.push(Value::from(case.lab_case_num));
                    params
                },
            )
            .map_err(|e| e.to_string())?;
            Ok(case.lab_case_num)
        }
    }

    fn list_suppliers(&self) -> Result<Vec<Supplier>, String> {
        let mut conn = self.conn()?;
        if !table_exists(&mut conn, "supplier")? {
            return Ok(Vec::new());
        }
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT SupplierNum,COALESCE(Name,'') FROM supplier ORDER BY Name",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| Supplier {
                supplier_num: cell_u32(&row, 0),
                name: s(cell_text(&row, 1)),
            })
            .collect())
    }

    fn list_supplies(&self) -> Result<Vec<Supply>, String> {
        let mut conn = self.conn()?;
        if !table_exists(&mut conn, "supply")? {
            return Ok(Vec::new());
        }
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT SupplyNum,SupplierNum,COALESCE(CatalogNumber,''),COALESCE(Descript,''), \
                 COALESCE(Category,0),ItemOrder,COALESCE(LevelDesired,0),IsHidden, \
                 COALESCE(Price,0),COALESCE(LevelOnHand,0),COALESCE(OrderQty,0) \
                 FROM supply ORDER BY Category,ItemOrder,Descript",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| Supply {
                supply_num: cell_u32(&row, 0),
                supplier_num: cell_u32(&row, 1),
                catalog_number: s(cell_text(&row, 2)),
                description: s(cell_text(&row, 3)),
                category: cell_u32(&row, 4),
                item_order: cell_i32(&row, 5),
                level_desired: cell_f64(&row, 6),
                is_hidden: cell_i16(&row, 7) != 0,
                price: cell_f64(&row, 8),
                level_on_hand: cell_f64(&row, 9),
                order_qty: cell_i32(&row, 10),
            })
            .collect())
    }

    fn list_supply_dispenses(&self, supply_num: Id) -> Result<Vec<SupplyDispense>, String> {
        let mut conn = self.conn()?;
        if !table_exists(&mut conn, "dispsupply")? {
            return Ok(Vec::new());
        }
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT DispSupplyNum,SupplyNum,ProvNum,CAST(DateDispensed AS CHAR), \
                 DispQuantity,COALESCE(Note,'') FROM dispsupply WHERE SupplyNum = ? \
                 ORDER BY DateDispensed DESC",
                (Value::from(supply_num),),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| SupplyDispense {
                disp_supply_num: cell_u32(&row, 0),
                supply_num: cell_u32(&row, 1),
                prov_num: cell_u32(&row, 2),
                date_dispensed: date(cell_text(&row, 3)).unwrap_or_default(),
                disp_quantity: cell_f64(&row, 4),
                note: s(cell_text(&row, 5)),
            })
            .collect())
    }

    fn record_supply_dispense(&self, dispense: &SupplyDispense) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        if !table_exists(&mut conn, "dispsupply")? {
            return Err("This Open Dental schema has no dispsupply table.".into());
        }
        conn.exec_drop(
            "INSERT INTO dispsupply (SupplyNum,ProvNum,DateDispensed,DispQuantity,Note) \
             VALUES (?,?,?,?,?)",
            (
                dispense.supply_num,
                dispense.prov_num,
                dispense.date_dispensed.format("%Y-%m-%d").to_string(),
                dispense.disp_quantity,
                dispense.note.clone(),
            ),
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_id() as Id)
    }

    fn list_pay_plans(&self, pat_num: Id) -> Result<Vec<PayPlan>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT PayPlanNum,PlanNum,PatNum,Guarantor,CAST(PayPlanDate AS CHAR),APR,Note,CompletedAmt,PaySchedule,NumberOfPayments,PayAmt,DownPayment,IsClosed,ChargeFrequency,CAST(DatePayPlanStart AS CHAR),CAST(DateInterestStart AS CHAR),IsDynamic,DynamicPayPlanTPOption,IsLocked \
                 FROM payplan WHERE PatNum = ? ORDER BY PayPlanDate DESC",
                (Value::from(pat_num),),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|mut row| {
                // mysql's tuple FromRow stops at arity 12, so read by index.
                let num: u32 = row.take(0usize).unwrap_or_default();
                let insurance_plan_num: u32 = row.take(1usize).unwrap_or_default();
                let pat: u32 = row.take(2usize).unwrap_or_default();
                let guarantor: u32 = row.take(3usize).unwrap_or_default();
                let plan_date: Option<String> = row.take(4usize);
                let apr: f64 = row.take(5usize).unwrap_or_default();
                let note: Option<String> = row.take(6usize);
                let completed: f64 = row.take(7usize).unwrap_or_default();
                let schedule: i8 = row.take(8usize).unwrap_or_default();
                let count: i32 = row.take(9usize).unwrap_or_default();
                let amount: f64 = row.take(10usize).unwrap_or_default();
                let down: f64 = row.take(11usize).unwrap_or_default();
                let closed: i8 = row.take(12usize).unwrap_or_default();
                let frequency: i8 = row.take(13usize).unwrap_or_default();
                let start: Option<String> = row.take(14usize);
                let interest_start: Option<String> = row.take(15usize);
                let dynamic: i8 = row.take(16usize).unwrap_or_default();
                let tp_option: i8 = row.take(17usize).unwrap_or_default();
                let locked: i8 = row.take(18usize).unwrap_or_default();
                PayPlan {
                    pay_plan_num: num,
                    insurance_plan_num,
                    pat_num: pat,
                    guarantor,
                    pay_plan_date: date(plan_date).unwrap_or_default(),
                    apr,
                    note: s(note),
                    completed_amt: completed,
                    pay_schedule: PaymentSchedule::from_od(schedule),
                    number_of_payments: count,
                    pay_amt: amount,
                    down_payment: down,
                    is_closed: closed != 0,
                    charge_frequency: PayPlanFrequency::from_od(frequency),
                    date_pay_plan_start: date(start),
                    date_interest_start: date(interest_start),
                    is_dynamic: dynamic != 0,
                    dynamic_pay_plan_tp_option: DynamicPayPlanTPOption::from_od(tp_option),
                    is_locked: locked != 0,
                }
            })
            .collect())
    }

    fn list_pay_plan_charges(&self, pat_num: Id) -> Result<Vec<PayPlanCharge>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT pc.PayPlanChargeNum,pc.PayPlanNum,pc.Guarantor,pc.PatNum,CAST(pc.ChargeDate AS CHAR),pc.Principal,pc.Interest,pc.Note,pc.ProvNum,pc.ProcNum,pc.ChargeType,pc.FKey,pc.LinkType \
                 FROM payplancharge pc INNER JOIN payplan pp ON pp.PayPlanNum = pc.PayPlanNum \
                 WHERE pp.PatNum = ? ORDER BY pc.ChargeDate",
                (Value::from(pat_num),),
            )
            .map_err(|e| e.to_string())?;
        // 13 columns exceed `FromRow`'s tuple limit, so map by index.
        Ok(rows
            .into_iter()
            .map(|row| PayPlanCharge {
                pay_plan_charge_num: cell_u32(&row, 0),
                pay_plan_num: cell_u32(&row, 1),
                guarantor: cell_u32(&row, 2),
                pat_num: cell_u32(&row, 3),
                charge_date: date(cell_text(&row, 4)).unwrap_or_default(),
                principal: cell_f64(&row, 5),
                interest: cell_f64(&row, 6),
                note: s(cell_text(&row, 7)),
                provider_num: cell_u32(&row, 8),
                proc_num: cell_u32(&row, 9),
                charge_type: PayPlanChargeType::from_od(cell_u32(&row, 10) as i8),
                fkey: cell_u32(&row, 11),
                link_type: cell_u32(&row, 12) as i8,
            })
            .collect())
    }

    fn upsert_pay_plan(&self, plan: &PayPlan) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        let mut columns = "PlanNum,PatNum,Guarantor,PayPlanDate,APR,Note,CompletedAmt,PaySchedule,\
                           NumberOfPayments,PayAmt,DownPayment,IsClosed,ChargeFrequency,\
                           DatePayPlanStart,DateInterestStart,IsDynamic,DynamicPayPlanTPOption,IsLocked"
            .to_string();
        let mut placeholders = "?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?".to_string();
        let mut params: Vec<Value> = vec![
            Value::from(plan.insurance_plan_num),
            Value::from(plan.pat_num),
            // Patient-scoped practice: OD Guarantor must stay on the same patient.
            Value::from(plan.pat_num),
            Value::from(od_write_date(Some(plan.pay_plan_date))),
            Value::from(plan.apr),
            Value::from(plan.note.clone()),
            Value::from(plan.completed_amt),
            Value::from(plan.pay_schedule.as_od()),
            Value::from(plan.number_of_payments),
            Value::from(plan.pay_amt),
            Value::from(plan.down_payment),
            Value::from(i8::from(plan.is_closed)),
            Value::from(plan.charge_frequency.as_od()),
            Value::from(od_write_date(plan.date_pay_plan_start)),
            Value::from(od_write_date(plan.date_interest_start)),
            Value::from(i8::from(plan.is_dynamic)),
            Value::from(plan.dynamic_pay_plan_tp_option.as_od()),
            Value::from(i8::from(plan.is_locked)),
        ];
        // The remaining OD columns are optional on the reduced schema but
        // required on 24_3. Never fabricate a SecurityHash; the write gate is
        // the mitigation. They are insert-only: OD's update path leaves the
        // signature/audit fields untouched.
        let base_params = params.clone();
        for (name, value) in [
            ("InsSubNum", Value::from(0u64)),
            ("Signature", Value::from("")),
            ("SigIsTopaz", Value::from(0i8)),
            ("PlanCategory", Value::from(0u64)),
            ("MobileAppDeviceNum", Value::from(0u64)),
            ("SecurityHash", Value::from("")),
        ] {
            append_optional_column(
                &mut conn,
                "payplan",
                name,
                &mut columns,
                &mut placeholders,
                &mut params,
                value,
            )?;
        }
        if plan.pay_plan_num == 0 {
            let sql = format!("INSERT INTO payplan ({columns}) VALUES ({placeholders})");
            conn.exec_drop(sql, params).map_err(|e| e.to_string())?;
            Ok(conn.last_insert_id() as Id)
        } else {
            let mut update_params = base_params;
            update_params.push(Value::from(plan.pay_plan_num));
            let sql =
                "UPDATE payplan SET PlanNum=?,PatNum=?,Guarantor=?,PayPlanDate=?,APR=?,Note=?,\
                 CompletedAmt=?,PaySchedule=?,NumberOfPayments=?,PayAmt=?,DownPayment=?,\
                 IsClosed=?,ChargeFrequency=?,DatePayPlanStart=?,DateInterestStart=?,\
                 IsDynamic=?,DynamicPayPlanTPOption=?,IsLocked=? WHERE PayPlanNum=?";
            conn.exec_drop(sql, update_params)
                .map_err(|e| e.to_string())?;
            Ok(plan.pay_plan_num)
        }
    }

    fn upsert_pay_plan_charge(&self, charge: &PayPlanCharge) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        let mut columns = "PayPlanNum,Guarantor,PatNum,ChargeDate,Principal,Interest,Note,ProvNum,\
                           ProcNum,ChargeType,FKey,LinkType"
            .to_string();
        let mut placeholders = "?,?,?,?,?,?,?,?,?,?,?,?".to_string();
        let base_params: Vec<Value> = vec![
            Value::from(charge.pay_plan_num),
            // Patient-scoped practice: OD Guarantor must stay on the same patient.
            Value::from(charge.pat_num),
            Value::from(charge.pat_num),
            Value::from(od_write_date(Some(charge.charge_date))),
            Value::from(charge.principal),
            Value::from(charge.interest),
            Value::from(charge.note.clone()),
            Value::from(charge.provider_num),
            Value::from(charge.proc_num),
            Value::from(charge.charge_type.as_od()),
            Value::from(charge.fkey),
            Value::from(charge.link_type),
        ];
        let clinic_num = if charge.proc_num != 0 {
            procedure_clinic_num(&mut conn, charge.proc_num)?
        } else {
            patient_clinic_num(&mut conn, charge.pat_num)?
        };
        let mut insert_params = base_params.clone();
        for (name, value) in [
            ("ClinicNum", Value::from(clinic_num)),
            ("StatementNum", Value::from(0u64)),
            ("IsOffset", Value::from(0i8)),
            ("SecDateTEntry", Value::from(od_write_datetime(Some(Local::now().naive_local())))),
        ] {
            append_optional_column(
                &mut conn,
                "payplancharge",
                name,
                &mut columns,
                &mut placeholders,
                &mut insert_params,
                value,
            )?;
        }
        // OD upsert semantics: PayPlanChargeNum=0 inserts, otherwise update the
        // existing charge in place. This matters for safe stale resolution,
        // where we zero/annotate a previously issued charge without deleting it.
        if charge.pay_plan_charge_num == 0 {
            let sql = format!("INSERT INTO payplancharge ({columns}) VALUES ({placeholders})");
            conn.exec_drop(sql, insert_params)
                .map_err(|e| e.to_string())?;
            Ok(conn.last_insert_id() as Id)
        } else {
            let mut update_params = base_params;
            update_params.push(Value::from(charge.pay_plan_charge_num));
            conn.exec_drop(
                "UPDATE payplancharge SET \
                 PayPlanNum=?,Guarantor=?,PatNum=?,ChargeDate=?,Principal=?,Interest=?,Note=?,ProvNum=?,ProcNum=?,ChargeType=?,FKey=?,LinkType=? \
                 WHERE PayPlanChargeNum=?",
                mysql::Params::Positional(update_params),
            )
            .map_err(|e| e.to_string())?;
            Ok(charge.pay_plan_charge_num)
        }
    }

    fn upsert_adjustment(&self, adjustment: &Adjustment) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        // OD stores the service date on ProcDate. Manual entries in this port
        // leave proc_date at its zero value, so keep the previous behaviour
        // of using AdjDate for them.
        let proc_date = if adjustment.proc_date.year() >= 1880 {
            adjustment.proc_date
        } else {
            adjustment.adj_date
        };
        // OD's adjustment insert also writes the service date, clinic and
        // audit columns. A linked adjustment follows the procedure's clinic;
        // a manual one follows the patient.
        let clinic_num = if adjustment.proc_num != 0 {
            procedure_clinic_num(&mut conn, adjustment.proc_num)?
        } else {
            patient_clinic_num(&mut conn, adjustment.pat_num)?
        };
        let audit_user = audit_user_num(&mut conn, self.current_user())?;
        let mut columns = "AdjDate,AdjAmt,PatNum,AdjType,ProvNum,ProcNum,AdjNote,ProcDate"
            .to_string();
        let mut placeholders = "?,?,?,?,?,?,?,?".to_string();
        let mut params: Vec<Value> = vec![
            Value::from(od_write_date(Some(adjustment.adj_date))),
            Value::from(adjustment.adj_amount),
            Value::from(adjustment.pat_num),
            Value::from(adjustment.adj_type),
            Value::from(adjustment.provider_num),
            Value::from(adjustment.proc_num),
            Value::from(adjustment.note.clone()),
            Value::from(od_write_date(Some(proc_date))),
        ];
        let base_params = params.clone();
        append_optional_column(
            &mut conn,
            "adjustment",
            "DateEntry",
            &mut columns,
            &mut placeholders,
            &mut params,
            Value::from(od_write_date(Some(adjustment.adj_date))),
        )?;
        append_optional_column(
            &mut conn,
            "adjustment",
            "ClinicNum",
            &mut columns,
            &mut placeholders,
            &mut params,
            Value::from(clinic_num),
        )?;
        append_optional_column(
            &mut conn,
            "adjustment",
            "StatementNum",
            &mut columns,
            &mut placeholders,
            &mut params,
            Value::from(0u64),
        )?;
        append_optional_column(
            &mut conn,
            "adjustment",
            "SecUserNumEntry",
            &mut columns,
            &mut placeholders,
            &mut params,
            Value::from(audit_user),
        )?;
        append_optional_column(
            &mut conn,
            "adjustment",
            "TaxTransID",
            &mut columns,
            &mut placeholders,
            &mut params,
            Value::from(0u64),
        )?;
        if adjustment.adj_num == 0 {
            let sql = format!(
                "INSERT INTO adjustment ({columns}) VALUES ({placeholders}) \
                 ON DUPLICATE KEY UPDATE AdjDate=VALUES(AdjDate),AdjAmt=VALUES(AdjAmt),\
                 PatNum=VALUES(PatNum),AdjType=VALUES(AdjType),ProvNum=VALUES(ProvNum),\
                 ProcNum=VALUES(ProcNum),AdjNote=VALUES(AdjNote),ProcDate=VALUES(ProcDate)"
            );
            conn.exec_drop(sql, params).map_err(|e| e.to_string())?;
            Ok(conn.last_insert_id() as Id)
        } else {
            let mut update_params = base_params;
            update_params.push(Value::from(adjustment.adj_num));
            let sql = "UPDATE adjustment SET AdjDate=?,AdjAmt=?,PatNum=?,AdjType=?,ProvNum=?,\
                 ProcNum=?,AdjNote=?,ProcDate=? WHERE AdjNum=?";
            // Optional columns are only set on insert in OD's update path; the
            // original values are audit facts that must not be rewritten.
            conn.exec_drop(sql, update_params).map_err(|e| e.to_string())?;
            Ok(adjustment.adj_num)
        }
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
        self.ensure_od_writes_allowed()?;
        let splits = payment.splits(allocations)?;
        let mut conn = self.conn()?;
        let audit_user = audit_user_num(&mut conn, self.current_user())?;
        let now = od_write_datetime(Some(Local::now().naive_local()));
        let clinic_num = write_clinic_num(&mut conn, 0, payment.pat_num)?;

        // Resolve optional OD columns before opening the transaction; MySQL
        // transactions cannot run information_schema checks. Each flag appends
        // one column and one value so genuine 24_3 schemas get full audit rows
        // while reduced demo schemas keep working.
        let mut payment_columns =
            "PatNum,PayDate,PayAmt,PayType,CheckNum,PayNote".to_string();
        let mut payment_placeholders = "?,?,?,?,?,?".to_string();
        let mut payment_values: Vec<(String, Value)> = Vec::new();
        for (name, value) in [
            ("BankBranch", Value::from("")),
            ("IsSplit", Value::from(1i8)),
            ("ClinicNum", Value::from(clinic_num)),
            ("DateEntry", Value::from(now.clone())),
            ("DepositNum", Value::from(0u64)),
            ("Receipt", Value::from("")),
            ("IsRecurringCC", Value::from(0i8)),
            ("SecUserNumEntry", Value::from(audit_user)),
            ("PaymentSource", Value::from(0i8)),
            ("ProcessStatus", Value::from(0i8)),
            ("RecurringChargeDate", Value::from(od_write_date(None))),
            ("ExternalId", Value::from("")),
            ("PaymentStatus", Value::from(0i8)),
            ("IsCcCompleted", Value::from(0i8)),
            ("MerchantFee", Value::from(0.0f64)),
        ] {
            if column_exists(&mut conn, "payment", name)? {
                payment_columns.push(',');
                payment_columns.push_str(name);
                payment_placeholders.push_str(",?");
                payment_values.push((name.to_string(), value));
            }
        }
        let payment_sql = format!(
            "INSERT INTO payment ({payment_columns}) VALUES ({payment_placeholders})"
        );

        let mut split_columns = "PayNum,PatNum,ProcNum,SplitAmt,DatePay,ProcDate,ProvNum,\
                                  PayPlanNum,AdjNum,UnearnedType,PayPlanChargeNum,PayPlanDebitType"
            .to_string();
        let mut split_placeholders = "?,?,?,?,?,?,?,?,?,?,?,?".to_string();
        let mut split_values: Vec<(String, Value)> = Vec::new();
        for (name, value) in [
            ("IsDiscount", Value::from(0i8)),
            ("DiscountType", Value::from(0i8)),
            ("DateEntry", Value::from(now.clone())),
            ("ClinicNum", Value::from(clinic_num)),
            ("SecUserNumEntry", Value::from(audit_user)),
            ("FSplitNum", Value::from(0u64)),
            // This port cannot reproduce CDT.dll's salted hash; the write
            // policy gate is the only mitigation, so never fabricate one.
            ("SecurityHash", Value::from("")),
        ] {
            if column_exists(&mut conn, "paysplit", name)? {
                split_columns.push(',');
                split_columns.push_str(name);
                split_placeholders.push_str(",?");
                split_values.push((name.to_string(), value));
            }
        }
        let split_sql = format!(
            "INSERT INTO paysplit ({split_columns}) VALUES ({split_placeholders})"
        );

        // One payment and all its splits must be written atomically; otherwise
        // a failed split would leave an unbalanced OD ledger.
        let mut tx = conn
            .start_transaction(mysql::TxOpts::default())
            .map_err(|e| e.to_string())?;
        let mut payment_params: Vec<Value> = vec![
            Value::from(payment.pat_num),
            Value::from(od_write_date(Some(payment.pay_date))),
            Value::from(payment.pay_amount),
            Value::from(payment.pay_type),
            Value::from(payment.check_num.clone()),
            Value::from(payment.note.clone()),
        ];
        payment_params.extend(payment_values.into_iter().map(|(_, value)| value));
        tx.exec_drop(&payment_sql, payment_params)
            .map_err(|e| e.to_string())?;
        let pay_num = tx.last_insert_id().unwrap_or(0) as Id;
        let pay_date = od_write_date(Some(payment.pay_date));
        for split in &splits {
            let mut split_params: Vec<Value> = vec![
                Value::from(pay_num),
                Value::from(split.pat_num),
                Value::from(split.proc_num),
                Value::from(split.split_amount),
                Value::from(pay_date.clone()),
                Value::from(pay_date.clone()),
                Value::from(split.provider_num),
                Value::from(split.pay_plan_num),
                Value::from(split.adj_num),
                Value::from(split.unearned_type),
                Value::from(split.pay_plan_charge_num),
                Value::from(split.pay_plan_debit_type.as_db()),
            ];
            split_params.extend(split_values.iter().map(|(_, value)| value.clone()));
            tx.exec_drop(&split_sql, split_params)
                .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(pay_num)
    }

    fn list_treat_plans(&self, pat_num: Id) -> Result<Vec<TreatPlan>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                // OD treatplan has no ProvNum column; the domain field is left unset.
                // The module grid shows Active, then Inactive, then Saved plans.
                "SELECT TreatPlanNum,PatNum,CAST(DateTP AS CHAR),Heading,TPStatus,Note FROM treatplan WHERE PatNum = ? ORDER BY FIELD(TPStatus,1,2,0),DateTP DESC,TreatPlanNum DESC",
                (Value::from(pat_num),),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (num, pat, saved, heading, status, note): (u32, u32, Option<String>, Option<String>, i8, Option<String>) = from_row(row);
                TreatPlan {
                    treat_plan_num: num,
                    pat_num: pat,
                    date_saved: datetime(saved),
                    heading: s(heading),
                    status: TreatPlanStatus::from_od(status),
                    provider_num: 0,
                    note: s(note),
                }
            })
            .collect())
    }

    fn upsert_treat_plan(&self, plan: &TreatPlan) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        // Every NOT NULL column without a default gets an explicit OD-style
        // zero value. SecDateEntry is set on insert only, like OD's DateEntry.
        let sql = "INSERT INTO treatplan \
            (TreatPlanNum,PatNum,DateTP,Heading,Note,Signature,SigIsTopaz,ResponsParty,DocNum,TPStatus,\
             SecUserNumEntry,SecDateEntry,UserNumPresenter,TPType,SignaturePractice,DateTSigned,\
             DateTPracticeSigned,SignatureText,SignaturePracticeText,MobileAppDeviceNum) \
            VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) \
            ON DUPLICATE KEY UPDATE DateTP=VALUES(DateTP),Heading=VALUES(Heading),Note=VALUES(Note),\
             TPStatus=VALUES(TPStatus),UserNumPresenter=VALUES(UserNumPresenter),TPType=VALUES(TPType)";
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        let audit_user = audit_user_num(&mut conn, self.current_user())?;
        let params: Vec<Value> = vec![
            Value::from(plan.treat_plan_num),
            Value::from(plan.pat_num),
            Value::from(od_date_param(plan.date_saved.map(|d| d.date()))),
            Value::from(plan.heading.clone()),
            Value::from(plan.note.clone()),
            Value::from(""),
            Value::from(0i32),
            Value::from(0u64),
            Value::from(0u64),
            Value::from(i32::from(plan.status.as_od())),
            Value::from(audit_user),
            Value::from(today),
            Value::from(0u64),
            Value::from(0i32),
            Value::from(""),
            Value::from("0001-01-01 00:00:00"),
            Value::from("0001-01-01 00:00:00"),
            Value::from(""),
            Value::from(""),
            Value::from(0u64),
        ];
        conn.exec_drop(sql, params).map_err(|e| e.to_string())?;
        if plan.treat_plan_num == 0 {
            Ok(conn.last_insert_id() as Id)
        } else {
            Ok(plan.treat_plan_num)
        }
    }

    fn delete_treat_plan(&self, plan_num: Id) -> Result<(), String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        // OD refuses to delete a plan that still has proctp rows; clear both
        // line tables first.
        for sql in [
            "DELETE FROM proctp WHERE TreatPlanNum = ?",
            "DELETE FROM treatplanattach WHERE TreatPlanNum = ?",
            "DELETE FROM treatplan WHERE TreatPlanNum = ?",
        ] {
            conn.exec_drop(sql, (Value::from(plan_num),))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn list_treat_plan_lines(&self, plan_num: Id) -> Result<Vec<TreatPlanLine>, String> {
        let mut conn = self.conn()?;
        let saved = treat_plan_status(&mut conn, plan_num)?
            .ok_or_else(|| format!("treatment plan {plan_num} not found"))?
            == TreatPlanStatus::Saved;
        if saved {
            let rows: Vec<mysql::Row> = conn
                .exec(
                    "SELECT ProcTPNum,TreatPlanNum,ProcNumOrig,ItemOrder,Priority,ToothNumTP,Surf,\
                     ProcCode,Descript,FeeAmt,ProvNum,CAST(DateTP AS CHAR) \
                     FROM proctp WHERE TreatPlanNum = ? ORDER BY ItemOrder",
                    (Value::from(plan_num),),
                )
                .map_err(|e| e.to_string())?;
            Ok(rows
                .into_iter()
                .map(|row| {
                    let (
                        line_num,
                        plan,
                        proc_num,
                        item_order,
                        priority,
                        tooth,
                        surf,
                        code,
                        description,
                        fee,
                        provider,
                        date,
                    ): (
                        u32,
                        u32,
                        u32,
                        i32,
                        i32,
                        Option<String>,
                        Option<String>,
                        Option<String>,
                        Option<String>,
                        f64,
                        u32,
                        Option<String>,
                    ) = from_row(row);
                    TreatPlanLine {
                        line_num,
                        treat_plan_num: plan,
                        pat_num: 0,
                        proc_num,
                        item_order,
                        priority: priority as Id,
                        tooth_num: s(tooth),
                        surf: s(surf),
                        proc_code: s(code),
                        description: s(description),
                        fee,
                        provider_num: provider,
                        date_tp: od_date(date),
                    }
                })
                .collect())
        } else {
            let rows: Vec<mysql::Row> = conn
                .exec(
                    "SELECT a.TreatPlanAttachNum,a.TreatPlanNum,a.ProcNum,a.Priority,\
                     p.ToothNum,p.Surf,p.ProcFee,p.CodeNum,p.ProvNum,CAST(p.ProcDate AS CHAR) \
                     FROM treatplanattach a LEFT JOIN procedurelog p ON p.ProcNum = a.ProcNum \
                     WHERE a.TreatPlanNum = ? ORDER BY a.TreatPlanAttachNum",
                    (Value::from(plan_num),),
                )
                .map_err(|e| e.to_string())?;
            Ok(rows
                .into_iter()
                .map(|row| {
                    let (
                        line_num,
                        plan,
                        proc_num,
                        priority,
                        tooth,
                        surf,
                        fee,
                        code_num,
                        provider,
                        date,
                    ): (
                        u32,
                        u32,
                        u32,
                        i32,
                        Option<String>,
                        Option<String>,
                        Option<f64>,
                        Option<u32>,
                        Option<u32>,
                        Option<String>,
                    ) = from_row(row);
                    TreatPlanLine {
                        line_num,
                        treat_plan_num: plan,
                        pat_num: 0,
                        proc_num,
                        item_order: line_num as i32,
                        priority: priority as Id,
                        tooth_num: s(tooth),
                        surf: s(surf),
                        // procedurelog stores a CodeNum, not the code text.
                        proc_code: code_num
                            .filter(|n| *n != 0)
                            .map(|n| format!("#{n}"))
                            .unwrap_or_default(),
                        description: format!("Procedure #{proc_num}"),
                        fee: fee.unwrap_or(0.0),
                        provider_num: provider.unwrap_or(0),
                        date_tp: od_date(date),
                    }
                })
                .collect())
        }
    }

    fn upsert_treat_plan_line(
        &self,
        plan: &TreatPlan,
        line: &TreatPlanLine,
    ) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        let saved = treat_plan_status(&mut conn, plan.treat_plan_num)?
            .ok_or_else(|| format!("treatment plan {} not found", plan.treat_plan_num))?
            == TreatPlanStatus::Saved;
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        if saved {
            let item_order = if line.line_num == 0 && line.item_order == 0 {
                // MAX() returns one row containing SQL NULL on an empty
                // table, so the column itself is Option<i32> here.
                let max: Option<Option<i32>> = conn
                    .exec_first(
                        "SELECT MAX(ItemOrder) FROM proctp WHERE TreatPlanNum = ?",
                        (Value::from(plan.treat_plan_num),),
                    )
                    .map_err(|e| e.to_string())?;
                max.flatten().unwrap_or(-1) + 1
            } else {
                line.item_order
            };
            // `proctp` has no alternate unique key, so the PK must be in the
            // INSERT for ON DUPLICATE KEY UPDATE to fire. Without it an edit
            // silently inserted a second line and returned the stale id.
            let sql = "INSERT INTO proctp \
                (ProcTPNum,TreatPlanNum,PatNum,ProcNumOrig,ItemOrder,Priority,ToothNumTP,Surf,\
                 ProcCode,Descript,FeeAmt,PriInsAmt,SecInsAmt,PatAmt,Discount,Prognosis,Dx,\
                 ProcAbbr,SecUserNumEntry,SecDateEntry,FeeAllowed,TaxAmt,ProvNum,DateTP,ClinicNum,\
                 CatPercUCR) \
                VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) \
                ON DUPLICATE KEY UPDATE TreatPlanNum=VALUES(TreatPlanNum),PatNum=VALUES(PatNum),\
                 ProcNumOrig=VALUES(ProcNumOrig),ItemOrder=VALUES(ItemOrder),\
                 Priority=VALUES(Priority),ToothNumTP=VALUES(ToothNumTP),Surf=VALUES(Surf),\
                 ProcCode=VALUES(ProcCode),Descript=VALUES(Descript),FeeAmt=VALUES(FeeAmt),\
                 FeeAllowed=VALUES(FeeAllowed),TaxAmt=VALUES(TaxAmt),ProvNum=VALUES(ProvNum),\
                 DateTP=VALUES(DateTP),ClinicNum=VALUES(ClinicNum),CatPercUCR=VALUES(CatPercUCR)";
            let audit_user = audit_user_num(&mut conn, self.current_user())?;
            let clinic_num = write_clinic_num(&mut conn, 0, plan.pat_num)?;
            let params: Vec<Value> = vec![
                Value::from(line.line_num),
                Value::from(plan.treat_plan_num),
                Value::from(plan.pat_num),
                Value::from(line.proc_num),
                Value::from(item_order),
                Value::from(line.priority as u64),
                Value::from(line.tooth_num.clone()),
                Value::from(line.surf.clone()),
                Value::from(line.proc_code.clone()),
                Value::from(line.description.clone()),
                Value::from(line.fee),
                Value::from(0.0f64),
                Value::from(0.0f64),
                Value::from(0.0f64),
                Value::from(0.0f64),
                Value::from(""),
                Value::from(""),
                Value::from(""),
                Value::from(audit_user),
                Value::from(today),
                Value::from(0.0f64),
                Value::from(0.0f64),
                Value::from(line.provider_num),
                Value::from(od_date_param(line.date_tp)),
                Value::from(clinic_num),
                Value::from(0.0f64),
            ];
            conn.exec_drop(sql, params).map_err(|e| e.to_string())?;
            if line.line_num == 0 {
                Ok(conn.last_insert_id() as Id)
            } else {
                Ok(line.line_num)
            }
        } else {
            let sql = "INSERT INTO treatplanattach (TreatPlanAttachNum,TreatPlanNum,ProcNum,Priority) \
                       VALUES (?,?,?,?) ON DUPLICATE KEY UPDATE TreatPlanNum=VALUES(TreatPlanNum),\
                       ProcNum=VALUES(ProcNum),Priority=VALUES(Priority)";
            conn.exec_drop(
                sql,
                (
                    Value::from(line.line_num),
                    Value::from(plan.treat_plan_num),
                    Value::from(line.proc_num),
                    Value::from(line.priority as u64),
                ),
            )
            .map_err(|e| e.to_string())?;
            if line.line_num == 0 {
                Ok(conn.last_insert_id() as Id)
            } else {
                Ok(line.line_num)
            }
        }
    }

    fn delete_treat_plan_line(&self, plan: &TreatPlan, line_num: Id) -> Result<(), String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        let saved = treat_plan_status(&mut conn, plan.treat_plan_num)?
            .ok_or_else(|| format!("treatment plan {} not found", plan.treat_plan_num))?
            == TreatPlanStatus::Saved;
        let (table, key) = if saved {
            ("proctp", "ProcTPNum")
        } else {
            ("treatplanattach", "TreatPlanAttachNum")
        };
        let sql = format!("DELETE FROM {table} WHERE {key} = ?");
        conn.exec_drop(&sql, (Value::from(line_num),))
            .map_err(|e| e.to_string())
    }

    fn list_definitions(&self) -> Result<Vec<Definition>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .query(
                "SELECT DefNum,Category,ItemOrder,ItemName,ItemValue,ItemColor,IsHidden \
                 FROM definition ORDER BY Category,ItemOrder,DefNum",
            )
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(map_definition).collect())
    }

    fn upsert_definition(&self, definition: &Definition) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        let params = (
            definition.category.as_od() as i32,
            definition.item_order,
            definition.item_name.clone(),
            definition.item_value.clone(),
            definition.item_color,
            i8::from(definition.is_hidden),
        );
        if definition.def_num == 0 {
            conn.exec_drop(
                "INSERT INTO definition \
                 (Category,ItemOrder,ItemName,ItemValue,ItemColor,IsHidden) \
                 VALUES (?,?,?,?,?,?)",
                params,
            )
            .map_err(|e| e.to_string())?;
            Ok(conn.last_insert_id() as Id)
        } else {
            let num = definition.def_num;
            conn.exec_drop(
                "INSERT INTO definition \
                 (DefNum,Category,ItemOrder,ItemName,ItemValue,ItemColor,IsHidden) \
                 VALUES (?,?,?,?,?,?,?) \
                 ON DUPLICATE KEY UPDATE Category=VALUES(Category),ItemOrder=VALUES(ItemOrder),\
                 ItemName=VALUES(ItemName),ItemValue=VALUES(ItemValue),ItemColor=VALUES(ItemColor),\
                 IsHidden=VALUES(IsHidden)",
                (
                    num,
                    params.0,
                    params.1,
                    params.2,
                    params.3,
                    params.4,
                    params.5,
                ),
            )
            .map_err(|e| e.to_string())?;
            Ok(num)
        }
    }

    fn delete_definition(&self, def_num: Id) -> Result<(), String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        let category: Option<i16> = conn
            .exec_first(
                "SELECT Category FROM definition WHERE DefNum = ?",
                (Value::from(def_num),),
            )
            .map_err(|e| e.to_string())?;
        conn.exec_drop(
            "DELETE FROM definition WHERE DefNum = ?",
            (Value::from(def_num),),
        )
        .map_err(|e| e.to_string())?;
        if let Some(category) = category {
            let sql = format!(
                "SELECT DefNum FROM definition WHERE Category = {} ORDER BY ItemOrder,DefNum",
                category
            );
            let remaining: Vec<u32> = conn.query(sql).map_err(|e| e.to_string())?;
            for (order, num) in remaining.into_iter().enumerate() {
                conn.exec_drop(
                    "UPDATE definition SET ItemOrder = ? WHERE DefNum = ?",
                    (order as i32, num),
                )
                .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    fn list_display_fields(&self) -> Result<Vec<DisplayField>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .query(
                "SELECT DisplayFieldNum,InternalName,ItemOrder,Description,ColumnWidth,\
                 Category,ChartViewNum,PickList,DescriptionOverride \
                 FROM displayfield ORDER BY Category,ItemOrder,DisplayFieldNum",
            )
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(map_display_field).collect())
    }

    fn upsert_display_field(&self, field: &DisplayField) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        let params = (
            field.internal_name.clone(),
            field.item_order,
            field.description.clone(),
            field.column_width,
            field.category.as_od() as i32,
            field.chart_view_num,
            field.pick_list.clone(),
            field.description_override.clone(),
        );
        if field.display_field_num == 0 {
            conn.exec_drop(
                "INSERT INTO displayfield \
                 (InternalName,ItemOrder,Description,ColumnWidth,Category,ChartViewNum,\
                  PickList,DescriptionOverride) VALUES (?,?,?,?,?,?,?,?)",
                params,
            )
            .map_err(|e| e.to_string())?;
            Ok(conn.last_insert_id() as Id)
        } else {
            let num = field.display_field_num;
            conn.exec_drop(
                "INSERT INTO displayfield \
                 (DisplayFieldNum,InternalName,ItemOrder,Description,ColumnWidth,Category,\
                  ChartViewNum,PickList,DescriptionOverride) VALUES (?,?,?,?,?,?,?,?,?) \
                 ON DUPLICATE KEY UPDATE InternalName=VALUES(InternalName),\
                 ItemOrder=VALUES(ItemOrder),Description=VALUES(Description),\
                 ColumnWidth=VALUES(ColumnWidth),Category=VALUES(Category),\
                 ChartViewNum=VALUES(ChartViewNum),PickList=VALUES(PickList),\
                 DescriptionOverride=VALUES(DescriptionOverride)",
                (
                    num,
                    params.0.clone(),
                    params.1,
                    params.2.clone(),
                    params.3,
                    params.4,
                    params.5,
                    params.6.clone(),
                    params.7.clone(),
                ),
            )
            .map_err(|e| e.to_string())?;
            Ok(num)
        }
    }

    fn replace_display_fields(
        &self,
        category: DisplayFieldCategory,
        fields: &[DisplayField],
    ) -> Result<(), String> {
        self.ensure_od_writes_allowed()?;
        let normalized: Vec<DisplayField> = fields
            .iter()
            .enumerate()
            .map(|(index, field)| DisplayField {
                display_field_num: 0,
                category,
                item_order: index as i32,
                chart_view_num: 0,
                ..field.clone()
            })
            .collect();
        // OD deletes every row for the category, then reinserts the list. An
        // exact default leaves no rows so future reads fall back to defaults.
        let rows = if display_signature(&normalized)
            == display_signature(&default_display_fields(category))
        {
            Vec::new()
        } else {
            normalized
        };
        let mut conn = self.conn()?;
        conn.exec_drop(
            "DELETE FROM displayfield WHERE Category = ?",
            (category.as_od() as i32,),
        )
        .map_err(|e| e.to_string())?;
        for field in &rows {
            conn.exec_drop(
                "INSERT INTO displayfield \
                 (InternalName,ItemOrder,Description,ColumnWidth,Category,ChartViewNum,\
                  PickList,DescriptionOverride) VALUES (?,?,?,?,?,?,?,?)",
                (
                    field.internal_name.clone(),
                    field.item_order,
                    field.description.clone(),
                    field.column_width,
                    field.category.as_od() as i32,
                    field.chart_view_num,
                    field.pick_list.clone(),
                    field.description_override.clone(),
                ),
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn list_users(&self) -> Result<Vec<Userod>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .query("SELECT UserNum,UserName,Password,UserGroupNum,EmployeeNum,ClinicNum,IsHidden FROM userod ORDER BY UserName")
            .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|row| Userod {
                user_num: cell_u32(row, 0),
                user_name: s(cell_text(row, 1)),
                password: s(cell_text(row, 2)),
                user_group_num: cell_u32(row, 3),
                employee_num: cell_u32(row, 4),
                clinic_num: cell_u32(row, 5),
                is_hidden: cell_bool(row, 6),
            })
            .collect())
    }

    fn list_prefs(&self) -> Result<Vec<Pref>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .query("SELECT PrefNum,PrefName,ValueString FROM preference ORDER BY PrefName")
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (pref_num, name, value): (u32, Option<String>, Option<String>) = from_row(row);
                Pref {
                    pref_num,
                    pref_name: s(name),
                    pref_value: s(value),
                }
            })
            .collect())
    }

    fn upsert_pref(&self, name: &str, value: &str) -> Result<(), String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        // Check existence explicitly: MySQL counts *changed* rows in
        // affected_rows(), so an UPDATE that writes the same value would report
        // zero and look like a missing row.
        let count: u64 = conn
            .exec_first("SELECT COUNT(*) FROM preference WHERE PrefName=?", (name.to_string(),))
            .map_err(|e| e.to_string())?
            .unwrap_or(0);
        if count == 0 {
            conn.exec_drop(
                "INSERT INTO preference (PrefName,ValueString) VALUES (?,?)",
                (name.to_string(), value.to_string()),
            )
            .map_err(|e| e.to_string())?;
        } else {
            conn.exec_drop(
                "UPDATE preference SET ValueString=? WHERE PrefName=?",
                (value.to_string(), name.to_string()),
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn list_group_permissions(&self) -> Result<Vec<GroupPermission>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .query("SELECT GroupPermNum,UserGroupNum,PermType,FKey FROM grouppermission")
            .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|row| GroupPermission {
                group_perm_num: cell_u32(row, 0),
                user_group_num: cell_u32(row, 1),
                perm_type: Permission::from_od(cell_i16(row, 2)),
                // OD 25.3 has no PermissionLevel column; the port keeps the
                // field for compatibility but always reads it as 0.
                permission_level: 0,
                fkey: cell_u32(row, 3),
            })
            .collect())
    }

    fn list_user_group_attaches(&self) -> Result<Vec<UserGroupAttach>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .query("SELECT UserGroupAttachNum,UserNum,UserGroupNum FROM usergroupattach")
            .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|row| UserGroupAttach {
                user_group_attach_num: cell_u32(row, 0),
                user_num: cell_u32(row, 1),
                user_group_num: cell_u32(row, 2),
            })
            .collect())
    }

    fn list_security_log(&self, limit: usize) -> Result<Vec<SecurityEvent>, String> {
        let mut conn = self.conn()?;
        let sql = format!(
            "SELECT SecurityLogNum,PermType,UserNum,CAST(LogDateTime AS CHAR),LogText,PatNum \
             FROM securitylog ORDER BY LogDateTime DESC LIMIT {}",
            limit.clamp(1, 1000)
        );
        let rows: Vec<mysql::Row> = conn.query(sql).map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|row| SecurityEvent {
                security_log_num: cell_u32(row, 0),
                perm_type: Permission::from_od(cell_i16(row, 1)),
                user_num: cell_u32(row, 2),
                log_datetime: datetime(cell_text(row, 3)).unwrap_or_default(),
                log_text: s(cell_text(row, 4)),
                pat_num: cell_u32(row, 5),
            })
            .collect())
    }

    fn append_security_log(&self, event: &SecurityEvent) -> Result<(), String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        let params: Vec<Value> = vec![
            Value::from(event.perm_type.as_od()),
            Value::from(event.user_num),
            Value::from(event.log_text.clone()),
            Value::from(event.pat_num),
        ];
        conn.exec_drop(
            "INSERT INTO securitylog \
             (PermType,UserNum,LogDateTime,LogText,PatNum,FKey,CompName,LogSource) \
             VALUES (?,?,NOW(),?,?,0,'odclinic',0)",
            params,
        )
        .map_err(|e| e.to_string())
    }

    // ------------------------------------------------------------ eServices

    fn list_reminder_rules(&self) -> Result<Vec<ReminderRule>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .query(
                "SELECT ApptReminderRuleNum,TypeCur,TSPrior,SendOrder,IsSendAll,\
                 TemplateSMS,TemplateEmailSubject,TemplateEmail,ClinicNum,\
                 DoNotSendWithin,IsEnabled FROM apptreminderrule \
                 ORDER BY ApptReminderRuleNum",
            )
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(map_reminder_rule).collect())
    }

    fn upsert_reminder_rule(&self, rule: &ReminderRule) -> Result<Id, String> {
        self.ensure_od_writes_allowed()?;
        let mut conn = self.conn()?;
        // `SendOrder` is an ordered comma-delimited preference list (e.g.
        // "0,1,2"), not a single comm type. Preserve the fallback chain while
        // making the port's edited `comm_type` the primary channel.
        let send_order = send_order_for_write(rule);
        let params: Vec<Value> = vec![
            Value::from(rule.rule_num),
            Value::from(rule.type_cur.as_od()),
            Value::from(minutes_to_ticks(rule.minutes_prior)),
            Value::from(send_order),
            Value::from(rule.sms_template.clone()),
            Value::from(rule.email_subject.clone()),
            Value::from(rule.email_body.clone()),
            Value::from(minutes_to_ticks(rule.do_not_send_within_minutes)),
            Value::from(i8::from(rule.is_enabled)),
        ];
        conn.exec_drop(
            "INSERT INTO apptreminderrule \
             (ApptReminderRuleNum,TypeCur,TSPrior,SendOrder,TemplateSMS,\
              TemplateEmailSubject,TemplateEmail,DoNotSendWithin,IsEnabled) \
             VALUES (?,?,?,?,?,?,?,?,?) \
             ON DUPLICATE KEY UPDATE TypeCur=VALUES(TypeCur),TSPrior=VALUES(TSPrior),\
              SendOrder=VALUES(SendOrder),TemplateSMS=VALUES(TemplateSMS),\
              TemplateEmailSubject=VALUES(TemplateEmailSubject),TemplateEmail=VALUES(TemplateEmail),\
              DoNotSendWithin=VALUES(DoNotSendWithin),IsEnabled=VALUES(IsEnabled)",
            params,
        )
        .map_err(|e| e.to_string())?;
        if rule.rule_num == 0 {
            Ok(conn.last_insert_id() as Id)
        } else {
            Ok(rule.rule_num)
        }
    }

    fn list_patient_contacts(&self) -> Result<Vec<PatientContact>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .query(
                "SELECT PatNum,TelegramChatId,EmailOverride,OptOutEmail,OptOutTelegram \
                 FROM odclinic_patient_contact",
            )
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(map_patient_contact).collect())
    }

    fn upsert_patient_contact(&self, contact: &PatientContact) -> Result<Id, String> {
        let mut conn = self.conn()?;
        conn.exec_drop(
            "INSERT INTO odclinic_patient_contact \
             (PatNum,TelegramChatId,EmailOverride,OptOutEmail,OptOutTelegram) \
             VALUES (?,?,?,?,?) ON DUPLICATE KEY UPDATE \
             TelegramChatId=VALUES(TelegramChatId),EmailOverride=VALUES(EmailOverride),\
             OptOutEmail=VALUES(OptOutEmail),OptOutTelegram=VALUES(OptOutTelegram)",
            (
                contact.pat_num,
                contact.telegram_chat_id.clone(),
                contact.email_override.clone(),
                i8::from(contact.opt_out_email),
                i8::from(contact.opt_out_telegram),
            ),
        )
        .map_err(|e| e.to_string())?;
        Ok(contact.pat_num)
    }

    fn list_reminder_sent(&self) -> Result<Vec<(Id, Id)>, String> {
        let mut conn = self.conn()?;
        // Port-local dedupe only. OD's apptremindersent belongs to HQ and its
        // SendStatus=1 means DoNotSend, so the port must never write it.
        let rows: Vec<mysql::Row> = conn
            .query("SELECT ApptReminderRuleNum,ApptNum FROM odclinic_reminder_sent")
            .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|row| (cell_u32(row, 0), cell_u32(row, 1)))
            .collect())
    }

    fn mark_reminder_sent(&self, rule_num: Id, apt_num: Id, pat_num: Id) -> Result<(), String> {
        // Port-local table: not gated by WritePolicy, never touches OD state.
        let mut conn = self.conn()?;
        conn.exec_drop(
            "INSERT INTO odclinic_reminder_sent \
             (ApptReminderRuleNum,ApptNum,PatNum) VALUES (?,?,?) \
             ON DUPLICATE KEY UPDATE SentAt = CURRENT_TIMESTAMP",
            (rule_num, apt_num, pat_num),
        )
        .map_err(|e| e.to_string())
    }

    fn insert_outbox(&self, message: &OutboxMessage) -> Result<Id, String> {
        let mut conn = self.conn()?;
        let params: Vec<Value> = vec![
            Value::from(message.pat_num),
            Value::from(message.apt_num),
            Value::from(message.rule_num),
            Value::from(message.reminder_type.as_od()),
            Value::from(message.transport as i8),
            Value::from(message.recipient.clone()),
            Value::from(message.subject.clone()),
            Value::from(message.body.clone()),
            Value::from(message.status.as_od()),
            Value::from(message.attempts),
            datetime_param(Some(message.created_at)),
            datetime_param(message.sent_at),
            Value::from(message.last_error.clone()),
        ];
        conn.exec_drop(
            "INSERT INTO odclinic_message_outbox \
             (PatNum,AptNum,RuleNum,ReminderType,Transport,Recipient,Subject,Body,\
              Status,Attempts,CreatedAt,SentAt,LastError) \
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params,
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_id() as Id)
    }

    fn list_outbox(&self, limit: usize) -> Result<Vec<OutboxMessage>, String> {
        let mut conn = self.conn()?;
        let sql = format!(
            "SELECT MessageNum,PatNum,AptNum,RuleNum,ReminderType,Transport,Recipient,\
             Subject,Body,Status,Attempts,CAST(CreatedAt AS CHAR),CAST(SentAt AS CHAR),LastError \
             FROM odclinic_message_outbox ORDER BY MessageNum DESC LIMIT {}",
            limit.clamp(1, 1000)
        );
        let rows: Vec<mysql::Row> = conn.query(sql).map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(map_outbox_message).collect())
    }

    fn update_outbox(
        &self,
        message_num: Id,
        status: OutboxStatus,
        attempts: u32,
        sent_at: Option<NaiveDateTime>,
        last_error: &str,
    ) -> Result<(), String> {
        let mut conn = self.conn()?;
        let sent_value = match sent_at {
            Some(dt) => Value::from(dt.format("%Y-%m-%d %H:%M:%S").to_string()),
            None => Value::NULL,
        };
        conn.exec_drop(
            "UPDATE odclinic_message_outbox \
             SET Status=?,Attempts=?,SentAt=?,LastError=? WHERE MessageNum=?",
            (
                status.as_od(),
                attempts,
                sent_value,
                last_error.to_string(),
                message_num,
            ),
        )
        .map_err(|e| e.to_string())
    }
}
