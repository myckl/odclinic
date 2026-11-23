//! MySQL backend speaking the Open Dental table/column naming convention.
//!
//! Connect with an ODBC-style URL in `ODCLINIC_DB_URL`, for example:
//! `mysql://od_user:secret@localhost:3306/opendental`

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime};
use mysql::{Opts, Pool, PooledConn, Value, from_row, prelude::Queryable};
use od_domain::definitions::{
    default_display_fields, Definition, DefinitionCategory, DisplayField, DisplayFieldCategory,
};
use od_domain::eservices::*;
use od_domain::*;

use crate::Store;

pub struct MySqlStore {
    pool: Pool,
}

impl MySqlStore {
    pub fn new(url: &str) -> Result<Self, String> {
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
        ensure_recall_schema(&mut conn)?;
        ensure_eservices_schema(&mut conn)?;
        ensure_treat_plan_schema(&mut conn)?;
        ensure_pay_plan_schema(&mut conn)?;
        ensure_definitions_schema(&mut conn)?;
        ensure_clinical_schema(&mut conn)?;
        ensure_registry_schema(&mut conn)?;
        Ok(Self { pool })
    }

    fn conn(&self) -> Result<PooledConn, String> {
        self.pool.get_conn().map_err(|e| format!("MySQL connect failed: {e}"))
    }
}

/// The demo database may predate the recall slice. Add the two objects this
/// module needs without touching any other OD table. Safe to run on every
/// connection: both statements are idempotent.
fn ensure_recall_schema(conn: &mut PooledConn) -> Result<(), String> {
    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS recall (\
         RecallNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,\
         PatNum bigint NOT NULL DEFAULT 0,\
         DateDueCalc date NOT NULL DEFAULT '0001-01-01',\
         DateDue date NOT NULL DEFAULT '0001-01-01',\
         DatePrevious date NOT NULL DEFAULT '0001-01-01',\
         RecallInterval int NOT NULL DEFAULT 0,\
         RecallStatus bigint NOT NULL DEFAULT 0,\
         Note text NOT NULL,\
         IsDisabled tinyint NOT NULL DEFAULT 0,\
         DateTStamp timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,\
         RecallTypeNum bigint NOT NULL DEFAULT 0,\
         DisableUntilBalance double NOT NULL DEFAULT 0,\
         DisableUntilDate datetime NOT NULL DEFAULT '0001-01-01',\
         DateScheduled datetime NOT NULL DEFAULT '0001-01-01',\
         Priority tinyint NOT NULL DEFAULT 0,\
         TimePatternOverride varchar(255) NOT NULL DEFAULT '',\
         KEY (PatNum), KEY (DateDue))",
    )
    .map_err(|e| format!("could not create OD recall table: {e}"))?;

    let has_priority: Option<i64> = conn
        .query_first(
            "SELECT COUNT(*) FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'appointment' AND COLUMN_NAME = 'Priority'",
        )
        .map_err(|e| format!("could not inspect appointment table: {e}"))?;
    if has_priority.unwrap_or(0) == 0 {
        conn.query_drop("ALTER TABLE appointment ADD COLUMN Priority tinyint NOT NULL DEFAULT 0")
            .map_err(|e| format!("could not add appointment.Priority: {e}"))?;
    }
    Ok(())
}

/// eServices objects. The first two use real OD table/column names, so an
/// existing Open Dental database needs no schema change from this slice. The
/// contact and outbox tables are port-local: OD sends through HQ and has no
/// equivalent local queue.
fn ensure_eservices_schema(conn: &mut PooledConn) -> Result<(), String> {
    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS apptreminderrule (\
         ApptReminderRuleNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,\
         TypeCur tinyint NOT NULL DEFAULT 0,\
         TSPrior bigint NOT NULL DEFAULT 0,\
         SendOrder varchar(255) NOT NULL DEFAULT '0,1,2',\
         IsSendAll tinyint NOT NULL DEFAULT 0,\
         TemplateSMS text NOT NULL,\
         TemplateEmailSubject text NOT NULL,\
         TemplateEmail text NOT NULL,\
         ClinicNum bigint NOT NULL DEFAULT 0,\
         DoNotSendWithin bigint NOT NULL DEFAULT 0,\
         IsEnabled tinyint NOT NULL DEFAULT 1,\
         Language varchar(255) NOT NULL DEFAULT '',\
         DateTStamp timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP)",
    )
    .map_err(|e| format!("could not create OD apptreminderrule table: {e}"))?;

    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS apptremindersent (\
         ApptReminderSentNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,\
         PatNum bigint NOT NULL DEFAULT 0,\
         ClinicNum bigint NOT NULL DEFAULT 0,\
         SendStatus tinyint NOT NULL DEFAULT 0,\
         MessageType tinyint NOT NULL DEFAULT 0,\
         MessageFk bigint NOT NULL DEFAULT 0,\
         DateTimeEntry datetime NOT NULL DEFAULT CURRENT_TIMESTAMP,\
         DateTimeSent datetime NOT NULL DEFAULT '0001-01-01',\
         ResponseDescript text NOT NULL,\
         ApptReminderRuleNum bigint NOT NULL DEFAULT 0,\
         ApptNum bigint NOT NULL DEFAULT 0,\
         ApptDateTime datetime NOT NULL DEFAULT '0001-01-01',\
         TSPrior bigint NOT NULL DEFAULT 0,\
         KEY (ApptReminderRuleNum,ApptNum), KEY (PatNum))",
    )
    .map_err(|e| format!("could not create OD apptremindersent table: {e}"))?;

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
    Ok(())
}

/// Treatment-plan storage. OD keeps Saved plans as immutable `proctp` copies
/// and Active/Inactive plans as `treatplanattach` links to `procedurelog`.
/// Everything here is an OD table name; the extra columns are the NOT NULL
/// bookkeeping fields the 24.3 CRUD layer writes on every insert.
fn ensure_treat_plan_schema(conn: &mut PooledConn) -> Result<(), String> {
    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS treatplan (\
         TreatPlanNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,\
         PatNum bigint NOT NULL DEFAULT 0,\
         DateTP date NOT NULL DEFAULT '0001-01-01',\
         Heading varchar(255) NOT NULL DEFAULT '',\
         Note text NOT NULL,\
         Signature text NOT NULL,\
         SigIsTopaz tinyint NOT NULL DEFAULT 0,\
         ResponsParty bigint NOT NULL DEFAULT 0,\
         DocNum bigint NOT NULL DEFAULT 0,\
         TPStatus tinyint NOT NULL DEFAULT 0,\
         SecUserNumEntry bigint NOT NULL DEFAULT 0,\
         SecDateEntry date NOT NULL DEFAULT '0001-01-01',\
         SecDateTEdit timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,\
         UserNumPresenter bigint NOT NULL DEFAULT 0,\
         TPType tinyint NOT NULL DEFAULT 0,\
         SignaturePractice text NOT NULL,\
         DateTSigned datetime NOT NULL DEFAULT '0001-01-01 00:00:00',\
         DateTPracticeSigned datetime NOT NULL DEFAULT '0001-01-01 00:00:00',\
         SignatureText varchar(255) NOT NULL DEFAULT '',\
         SignaturePracticeText varchar(255) NOT NULL DEFAULT '',\
         MobileAppDeviceNum bigint NOT NULL DEFAULT 0,\
         KEY (PatNum))",
    )
    .map_err(|e| format!("could not create OD treatplan table: {e}"))?;

    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS proctp (\
         ProcTPNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,\
         TreatPlanNum bigint NOT NULL DEFAULT 0,\
         PatNum bigint NOT NULL DEFAULT 0,\
         ProcNumOrig bigint NOT NULL DEFAULT 0,\
         ItemOrder int NOT NULL DEFAULT 0,\
         Priority bigint NOT NULL DEFAULT 0,\
         ToothNumTP varchar(255) NOT NULL DEFAULT '',\
         Surf varchar(255) NOT NULL DEFAULT '',\
         ProcCode varchar(15) DEFAULT NULL,\
         Descript varchar(255) NOT NULL DEFAULT '',\
         FeeAmt double NOT NULL DEFAULT 0,\
         PriInsAmt double NOT NULL DEFAULT 0,\
         SecInsAmt double NOT NULL DEFAULT 0,\
         PatAmt double NOT NULL DEFAULT 0,\
         Discount double NOT NULL DEFAULT 0,\
         Prognosis text NOT NULL,\
         Dx text NOT NULL,\
         ProcAbbr varchar(255) NOT NULL DEFAULT '',\
         SecUserNumEntry bigint NOT NULL DEFAULT 0,\
         SecDateEntry date NOT NULL DEFAULT '0001-01-01',\
         SecDateTEdit timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,\
         FeeAllowed double NOT NULL DEFAULT 0,\
         TaxAmt double NOT NULL DEFAULT 0,\
         ProvNum bigint NOT NULL DEFAULT 0,\
         DateTP date NOT NULL DEFAULT '0001-01-01',\
         ClinicNum bigint NOT NULL DEFAULT 0,\
         CatPercUCR double NOT NULL DEFAULT 0,\
         KEY (TreatPlanNum))",
    )
    .map_err(|e| format!("could not create OD proctp table: {e}"))?;

    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS treatplanattach (\
         TreatPlanAttachNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,\
         TreatPlanNum bigint NOT NULL DEFAULT 0,\
         ProcNum bigint NOT NULL DEFAULT 0,\
         Priority bigint NOT NULL DEFAULT 0,\
         KEY (TreatPlanNum), KEY (ProcNum))",
    )
    .map_err(|e| format!("could not create OD treatplanattach table: {e}"))?;

    // The old port-local schema.sql created smaller versions of these tables.
    // Add any missing OD columns so an existing demo database still works.
    ensure_columns(
        conn,
        "treatplan",
        &[
            ("DateTP", "date NOT NULL DEFAULT '0001-01-01'"),
            ("Heading", "varchar(255) NOT NULL DEFAULT ''"),
            ("Note", "text NOT NULL"),
            ("Signature", "text NOT NULL"),
            ("SigIsTopaz", "tinyint NOT NULL DEFAULT 0"),
            ("ResponsParty", "bigint NOT NULL DEFAULT 0"),
            ("DocNum", "bigint NOT NULL DEFAULT 0"),
            ("TPStatus", "tinyint NOT NULL DEFAULT 0"),
            ("SecUserNumEntry", "bigint NOT NULL DEFAULT 0"),
            ("SecDateEntry", "date NOT NULL DEFAULT '0001-01-01'"),
            ("UserNumPresenter", "bigint NOT NULL DEFAULT 0"),
            ("TPType", "tinyint NOT NULL DEFAULT 0"),
            ("SignaturePractice", "text NOT NULL"),
            ("DateTSigned", "datetime NOT NULL DEFAULT '0001-01-01 00:00:00'"),
            ("DateTPracticeSigned", "datetime NOT NULL DEFAULT '0001-01-01 00:00:00'"),
            ("SignatureText", "varchar(255) NOT NULL DEFAULT ''"),
            ("SignaturePracticeText", "varchar(255) NOT NULL DEFAULT ''"),
            ("MobileAppDeviceNum", "bigint NOT NULL DEFAULT 0"),
        ],
    )?;
    ensure_columns(
        conn,
        "proctp",
        &[
            ("ProcNumOrig", "bigint NOT NULL DEFAULT 0"),
            ("ItemOrder", "int NOT NULL DEFAULT 0"),
            ("Priority", "bigint NOT NULL DEFAULT 0"),
            ("ToothNumTP", "varchar(255) NOT NULL DEFAULT ''"),
            ("Surf", "varchar(255) NOT NULL DEFAULT ''"),
            ("ProcCode", "varchar(15) DEFAULT NULL"),
            ("Descript", "varchar(255) NOT NULL DEFAULT ''"),
            ("PriInsAmt", "double NOT NULL DEFAULT 0"),
            ("SecInsAmt", "double NOT NULL DEFAULT 0"),
            ("PatAmt", "double NOT NULL DEFAULT 0"),
            ("Discount", "double NOT NULL DEFAULT 0"),
            ("Prognosis", "text NOT NULL"),
            ("Dx", "text NOT NULL"),
            ("ProcAbbr", "varchar(255) NOT NULL DEFAULT ''"),
            ("SecUserNumEntry", "bigint NOT NULL DEFAULT 0"),
            ("SecDateEntry", "date NOT NULL DEFAULT '0001-01-01'"),
            ("FeeAllowed", "double NOT NULL DEFAULT 0"),
            ("TaxAmt", "double NOT NULL DEFAULT 0"),
            ("ProvNum", "bigint NOT NULL DEFAULT 0"),
            ("DateTP", "date NOT NULL DEFAULT '0001-01-01'"),
            ("ClinicNum", "bigint NOT NULL DEFAULT 0"),
            ("CatPercUCR", "double NOT NULL DEFAULT 0"),
        ],
    )?;
    ensure_columns(
        conn,
        "treatplanattach",
        &[
            ("TreatPlanNum", "bigint NOT NULL DEFAULT 0"),
            ("ProcNum", "bigint NOT NULL DEFAULT 0"),
            ("Priority", "bigint NOT NULL DEFAULT 0"),
        ],
    )?;
    Ok(())
}

/// The payment-plan report filters patient plans from insurance plans using
/// OD's `payplan.PlanNum`; amortised charge generation reads
/// `DateInterestStart` and dynamic plans read `DynamicPayPlanTPOption`. Older
/// demo databases may not have those columns yet. `payplancharge.FKey` and
/// `.LinkType` carry the dynamic link onto each issued charge.
fn ensure_pay_plan_schema(conn: &mut PooledConn) -> Result<(), String> {
    ensure_columns(
        conn,
        "payplan",
        &[
            ("PlanNum", "bigint NOT NULL DEFAULT 0"),
            ("DateInterestStart", "date DEFAULT NULL"),
            ("DynamicPayPlanTPOption", "tinyint NOT NULL DEFAULT 0"),
        ],
    )?;
    ensure_columns(
        conn,
        "payplancharge",
        &[
            ("FKey", "bigint NOT NULL DEFAULT 0"),
            ("LinkType", "tinyint NOT NULL DEFAULT 0"),
        ],
    )?;
    ensure_columns(
        conn,
        "paysplit",
        &[
            ("PayPlanChargeNum", "bigint NOT NULL DEFAULT 0"),
            ("PayPlanDebitType", "int NOT NULL DEFAULT 0"),
        ],
    )
}

/// Definitions and display fields use OD's own table names and columns. The
/// demo database may predate `ItemColor` or have no `displayfield` table at all,
/// so both are brought up to the 24.3 shape without touching existing rows.
fn ensure_definitions_schema(conn: &mut PooledConn) -> Result<(), String> {
    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS definition (\
         DefNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,\
         Category int NOT NULL DEFAULT 0,\
         ItemOrder int NOT NULL DEFAULT 0,\
         ItemName varchar(255) NOT NULL DEFAULT '',\
         ItemValue varchar(255) NOT NULL DEFAULT '',\
         ItemColor int NOT NULL DEFAULT 0,\
         IsHidden tinyint NOT NULL DEFAULT 0,\
         KEY (Category), KEY (Category,ItemOrder))",
    )
    .map_err(|e| format!("could not create OD definition table: {e}"))?;
    ensure_columns(conn, "definition", &[("ItemColor", "int NOT NULL DEFAULT 0")])?;

    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS displayfield (\
         DisplayFieldNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,\
         InternalName varchar(255) NOT NULL DEFAULT '',\
         ItemOrder int NOT NULL DEFAULT 0,\
         Description varchar(255) NOT NULL DEFAULT '',\
         ColumnWidth int NOT NULL DEFAULT 0,\
         Category int NOT NULL DEFAULT 0,\
         ChartViewNum bigint NOT NULL DEFAULT 0,\
         PickList text NOT NULL,\
         DescriptionOverride varchar(255) NOT NULL DEFAULT '',\
         KEY (Category), KEY (ChartViewNum))",
    )
    .map_err(|e| format!("could not create OD displayfield table: {e}"))?;
    Ok(())
}

/// Waitlist and correction history. `appointment.UnschedStatus` and
/// `histappointment` are real OD columns/tables; the patient correction table
/// is port-local because Open Dental has no patient history table.
fn ensure_clinical_schema(conn: &mut PooledConn) -> Result<(), String> {
    ensure_columns(conn, "appointment", &[("UnschedStatus", "bigint NOT NULL DEFAULT 0")])?;

    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS histappointment (\
         HistApptNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,\
         HistUserNum bigint NOT NULL DEFAULT 0,\
         HistDateTStamp datetime NOT NULL,\
         HistApptAction int NOT NULL DEFAULT 0,\
         AptNum bigint NOT NULL DEFAULT 0,\
         PatNum bigint NOT NULL DEFAULT 0,\
         AptStatus tinyint NOT NULL DEFAULT 0,\
         Pattern varchar(255) NOT NULL DEFAULT '',\
         TimeLocked tinyint NOT NULL DEFAULT 0,\
         Op bigint NOT NULL DEFAULT 0,\
         Note varchar(255) NOT NULL DEFAULT '',\
         ProvNum bigint NOT NULL DEFAULT 0,\
         ClinicNum bigint NOT NULL DEFAULT 0,\
         AptDateTime datetime NOT NULL DEFAULT '0001-01-01',\
         UnschedStatus bigint NOT NULL DEFAULT 0,\
         Priority tinyint NOT NULL DEFAULT 0,\
         KEY (AptNum), KEY (HistDateTStamp))",
    )
    .map_err(|e| format!("could not create OD histappointment table: {e}"))?;
    ensure_columns(
        conn,
        "histappointment",
        &[
            ("HistUserNum", "bigint NOT NULL DEFAULT 0"),
            ("HistDateTStamp", "datetime NOT NULL DEFAULT '0001-01-01'"),
            ("HistApptAction", "int NOT NULL DEFAULT 0"),
            ("UnschedStatus", "bigint NOT NULL DEFAULT 0"),
            ("Priority", "tinyint NOT NULL DEFAULT 0"),
        ],
    )?;

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
    Ok(())
}

/// The patient registry reads OD's real `disease`, `diseasedef`, `patientnote`
/// and `procedurelog` tables. Only the clinic-card table belongs to this port;
/// creating it idempotently is safe because OD has no equivalent table.
fn ensure_registry_schema(conn: &mut PooledConn) -> Result<(), String> {
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
    // Older demo databases may predate these OD clinical columns. They are
    // additive, so upgrading the demo database in place is safe.
    ensure_columns(
        conn,
        "disease",
        &[
            ("PatNote", "text"),
            ("ProbStatus", "tinyint NOT NULL DEFAULT 0"),
        ],
    )
}

/// Add an OD column that an older demo database may not have yet. Table and
/// column names are internal constants, so interpolating the definition is safe.
fn ensure_columns(
    conn: &mut PooledConn,
    table: &str,
    columns: &[(&str, &str)],
) -> Result<(), String> {
    for (name, definition) in columns {
        let found: Option<i64> = conn
            .exec_first(
                "SELECT COUNT(*) FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? AND COLUMN_NAME = ?",
                (table, *name),
            )
            .map_err(|e| format!("could not inspect {table}.{name}: {e}"))?;
        if found.unwrap_or(0) == 0 {
            let sql = format!("ALTER TABLE {table} ADD COLUMN {name} {definition}");
            if let Err(err) = conn.query_drop(&sql) {
                // Two connections can both observe COUNT=0 and race to ADD
                // COLUMN (parallel app startup or concurrent tests). MySQL and
                // MariaDB report a duplicate column as error 1060 / SQLSTATE
                // 42S21; that means the column now exists, which is what we
                // wanted. Any other error is still fatal.
                let duplicate = matches!(&err, mysql::Error::MySqlError(e) if e.code == 1060);
                if !duplicate {
                    return Err(format!("could not add {table}.{name}: {err}"));
                }
            }
        }
    }
    Ok(())
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

/// The subset of OD `appointment` columns this port reads and writes.
/// `UnschedStatus` is the waitlist status definition FK (DefCat 13).
const APPOINTMENT_COLUMNS: &str = "AptNum,PatNum,CAST(AptDateTime AS CHAR),AptStatus,Op,ProvNum,ClinicNum,Note,Pattern,TimeLocked,Priority,UnschedStatus";

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

fn map_appointment(row: mysql::Row) -> Appointment {
    let (
        apt_num,
        pat_num,
        apt_datetime,
        apt_status,
        operatory_num,
        provider_num,
        clinic_num,
        note,
        pattern,
        time_locked,
        priority,
        unsched_status,
    ): (
        u32,
        u32,
        Option<String>,
        i8,
        u32,
        u32,
        u32,
        Option<String>,
        Option<String>,
        i8,
        i8,
        Option<i64>,
    ) = from_row(row);
    let pattern = s(pattern);
    let apt_length = pattern_minutes(&pattern, 30);
    Appointment {
        apt_num,
        pat_num,
        apt_datetime: datetime(apt_datetime).unwrap_or_default(),
        apt_length,
        apt_status: AppointmentStatus::from_od(apt_status),
        operatory_num,
        provider_num,
        clinic_num,
        notes: s(note),
        pattern,
        is_time_locked: time_locked != 0,
        priority: RecallPriority::from_od(priority),
        waitlist_status: unsched_status.unwrap_or(0) as Id,
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
        let mut conn = self.conn()?;
        let sql = "INSERT INTO patient (PatNum,LName,FName,MiddleI,Title,Birthdate,Gender,HmPhone,WirelessPhone,Email,Address,City,State,Zip,ClinicNum,PriProv) \
                   VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) \
                   ON DUPLICATE KEY UPDATE LName=VALUES(LName),FName=VALUES(FName),MiddleI=VALUES(MiddleI),Title=VALUES(Title),Birthdate=VALUES(Birthdate),Gender=VALUES(Gender),HmPhone=VALUES(HmPhone),WirelessPhone=VALUES(WirelessPhone),Email=VALUES(Email),Address=VALUES(Address),City=VALUES(City),State=VALUES(State),Zip=VALUES(Zip),ClinicNum=VALUES(ClinicNum),PriProv=VALUES(PriProv)";
        let num = if patient.pat_num == 0 {
            // Let MySQL assign the auto-increment key.
            0
        } else {
            patient.pat_num
        };
        // OD's patient.Birthdate is NOT NULL; use its conventional zero date
        // when the caller did not provide one.
        let birth = patient
            .birth_date
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "0001-01-01".to_string());
        let params: Vec<Value> = vec![
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
            Value::from(patient.city.clone()),
            Value::from(patient.state.clone()),
            Value::from(patient.zip.clone()),
            Value::from(patient.clinic_num),
            Value::from(patient.primary_provider_num),
        ];
        conn.exec_drop(sql, params).map_err(|e| e.to_string())?;
        if patient.pat_num == 0 {
            let last: u64 = conn.last_insert_id();
            Ok(last as Id)
        } else {
            Ok(patient.pat_num)
        }
    }

    fn delete_patient(&self, pat_num: Id) -> Result<(), String> {
        let mut conn = self.conn()?;
        conn.exec_drop("DELETE FROM patient WHERE PatNum = ?", (Value::from(pat_num),))
            .map_err(|e| e.to_string())
    }

    fn list_appointments(
        &self,
        from: NaiveDateTime,
        to: NaiveDateTime,
    ) -> Result<Vec<Appointment>, String> {
        let mut conn = self.conn()?;
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT AptNum,PatNum,CAST(AptDateTime AS CHAR),AptStatus,Op,ProvNum,ClinicNum,Note,Pattern,TimeLocked,Priority,UnschedStatus \
                 FROM appointment WHERE AptDateTime >= ? AND AptDateTime < ? ORDER BY AptDateTime,Op",
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
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT AptNum,PatNum,CAST(AptDateTime AS CHAR),AptStatus,Op,ProvNum,ClinicNum,Note,Pattern,TimeLocked,Priority,UnschedStatus FROM appointment WHERE AptNum = ?",
                (Value::from(apt_num),),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(map_appointment).next())
    }

    fn upsert_appointment(&self, apt: &Appointment) -> Result<Id, String> {
        let mut conn = self.conn()?;
        let sql = "INSERT INTO appointment (AptNum,PatNum,AptDateTime,AptStatus,Op,ProvNum,ClinicNum,Note,Pattern,TimeLocked,Priority,UnschedStatus) \
                   VALUES (?,?,?,?,?,?,?,?,?,?,?,?) \
                   ON DUPLICATE KEY UPDATE PatNum=VALUES(PatNum),AptDateTime=VALUES(AptDateTime),AptStatus=VALUES(AptStatus),Op=VALUES(Op),ProvNum=VALUES(ProvNum),ClinicNum=VALUES(ClinicNum),Note=VALUES(Note),Pattern=VALUES(Pattern),TimeLocked=VALUES(TimeLocked),Priority=VALUES(Priority),UnschedStatus=VALUES(UnschedStatus)";
        let num = if apt.apt_num == 0 { 0 } else { apt.apt_num };
        let pattern = if apt.pattern.is_empty() {
            "X".repeat((apt.apt_length / 5).max(1) as usize)
        } else {
            apt.pattern.clone()
        };
        let params: Vec<Value> = vec![
            Value::from(num),
            Value::from(apt.pat_num),
            Value::from(apt.apt_datetime.format("%Y-%m-%d %H:%M:%S").to_string()),
            Value::from(apt.apt_status.as_od()),
            Value::from(apt.operatory_num),
            Value::from(apt.provider_num),
            Value::from(apt.clinic_num),
            Value::from(apt.notes.clone()),
            Value::from(pattern),
            Value::from(i8::from(apt.is_time_locked)),
            Value::from(apt.priority.as_od()),
            Value::from(apt.waitlist_status),
        ];
        conn.exec_drop(sql, params).map_err(|e| e.to_string())?;
        if apt.apt_num == 0 {
            Ok(conn.last_insert_id() as Id)
        } else {
            Ok(apt.apt_num)
        }
    }

    fn delete_appointment(&self, apt_num: Id) -> Result<(), String> {
        let mut conn = self.conn()?;
        conn.exec_drop("DELETE FROM appointment WHERE AptNum = ?", (Value::from(apt_num),))
            .map_err(|e| e.to_string())
    }

    fn list_waitlist_appointments(&self) -> Result<Vec<Appointment>, String> {
        let mut conn = self.conn()?;
        let sql = format!(
            "SELECT {APPOINTMENT_COLUMNS} FROM appointment \
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
        let params: Vec<Value> = vec![
            Value::from(num),
            Value::from(change.user_num),
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
        let mut conn = self.conn()?;
        let sql = "INSERT INTO recall \
                   (RecallNum,PatNum,DateDueCalc,DateDue,DatePrevious,RecallInterval,RecallStatus,\
                    Note,IsDisabled,DateScheduled,Priority) \
                   VALUES (?,?,?,?,?,?,?,?,?,?,?) \
                   ON DUPLICATE KEY UPDATE PatNum=VALUES(PatNum),DateDueCalc=VALUES(DateDueCalc),\
                    DateDue=VALUES(DateDue),DatePrevious=VALUES(DatePrevious),\
                    RecallInterval=VALUES(RecallInterval),RecallStatus=VALUES(RecallStatus),\
                    Note=VALUES(Note),IsDisabled=VALUES(IsDisabled),\
                    DateScheduled=VALUES(DateScheduled),Priority=VALUES(Priority)";
        let num = if recall.recall_num == 0 { 0 } else { recall.recall_num };
        let params: Vec<Value> = vec![
            Value::from(num),
            Value::from(recall.pat_num),
            Value::from(od_date_param(recall.date_due_calc)),
            Value::from(od_date_param(recall.date_due)),
            Value::from(od_date_param(recall.date_previous)),
            Value::from(recall.recall_interval),
            Value::from(recall.recall_status),
            Value::from(recall.note.clone()),
            Value::from(i8::from(recall.is_disabled)),
            Value::from(od_date_param(recall.date_scheduled)),
            Value::from(recall.priority.as_od()),
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
        let rows: Vec<mysql::Row> = conn
            .query(
                "SELECT AptNum,PatNum,CAST(AptDateTime AS CHAR),AptStatus,Op,ProvNum,ClinicNum,\
                 Note,Pattern,TimeLocked,Priority,UnschedStatus FROM appointment \
                 WHERE Priority = 1 AND AptStatus IN (1,6,3,2) ORDER BY AptDateTime",
            )
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
        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT ProcNum,PatNum,CodeNum,COALESCE(AptNum,0),CAST(ProcDate AS CHAR),ProcStatus,ToothNum,ProvNum,ProcFee, \
                 CAST(COALESCE(UnitQty,0) AS DOUBLE),CAST(COALESCE(BaseUnits,0) AS DOUBLE), \
                 COALESCE(Surf,''),COALESCE(ToothRange,''),COALESCE(MedicalCode,''),COALESCE(SnomedBodySite,'') \
                 FROM procedurelog WHERE PatNum = ? ORDER BY ProcDate DESC",
                (Value::from(pat_num),),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| Procedure {
                proc_num: cell_u32(&row, 0),
                pat_num: cell_u32(&row, 1),
                code_num: cell_u32(&row, 2),
                apt_num: cell_u32(&row, 3),
                proc_date: date(cell_text(&row, 4)).unwrap_or_default(),
                proc_status: ProcedureStatus::from_od(cell_i16(&row, 5) as i8),
                tooth_num: s(cell_text(&row, 6)),
                provider_num: cell_u32(&row, 7),
                fee: cell_f64(&row, 8),
                unit_qty: cell_f64(&row, 9),
                base_units: cell_f64(&row, 10),
                surf: s(cell_text(&row, 11)),
                tooth_range: s(cell_text(&row, 12)),
                medical_code: s(cell_text(&row, 13)),
                snomed_body_site: s(cell_text(&row, 14)),
                description_override: String::new(),
            })
            .collect())
    }

    fn insert_procedure(&self, procedure: &Procedure) -> Result<Id, String> {
        let mut conn = self.conn()?;
        // Mirrors the OD columns set by Procedures.Insert for a new planned
        // procedure. OD assigns the key and stores DateTime.MinValue
        // (0001-01-01) in DateComplete until the procedure moves to Complete.
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
                Value::from(0u32),
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
                "SELECT CodeNum,ProcCode,Descript,AbbrDesc,CAST(0 AS DOUBLE),ProcCat FROM procedurecode ORDER BY ProcCode",
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let (code_num, code, code_name, abbreviation, fee, category_num): (
                    u32,
                    String,
                    String,
                    String,
                    f64,
                    u32,
                ) = from_row(row);
                ProcedureCode {
                    code_num,
                    code,
                    code_name,
                    abbreviation,
                    fee,
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
        // A journal transaction is only meaningful as a whole: inserting one
        // side of the pair would make Balance Sheet and P&L disagree.
        let mut tx = conn
            .start_transaction(mysql::TxOpts::default())
            .map_err(|e| e.to_string())?;
        for entry in entries {
            tx.exec_drop(
                "INSERT INTO journalentry \
                 (AccountNum,DateDisplayed,DebitAmt,CreditAmt,Memo,Splits,CheckNumber,TransactionNum) \
                 VALUES (?,?,?,?,?,?,?,?)",
                (
                    entry.account_num,
                    entry.date_displayed.format("%Y-%m-%d").to_string(),
                    entry.debit_amt,
                    entry.credit_amt,
                    &entry.memo,
                    &entry.splits,
                    &entry.check_number,
                    entry.transaction_num,
                ),
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(transaction_num)
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
        let mut conn = self.conn()?;
        conn.exec_drop(
            "DELETE FROM payplanlink WHERE PayPlanLinkNum=?",
            (link_num,),
        )
        .map_err(|e| e.to_string())
    }

    fn delete_pay_plan_charge(&self, charge_num: Id) -> Result<(), String> {
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
        let mut conn = self.conn()?;
        // OD stores an unsent statement as the sentinel date 0001-01-01.
        let date_sent = statement
            .date_sent
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "0001-01-01".to_string());
        conn.exec_drop(
            "INSERT INTO statement (PatNum,DateSent,Note,IsInvoice) VALUES (?,?,?,?)",
            (
                statement.pat_num,
                date_sent,
                statement.note.clone(),
                statement.is_invoice as i8,
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
                 FROM payplan WHERE PatNum = ? OR Guarantor = ? ORDER BY PayPlanDate DESC",
                (Value::from(pat_num), Value::from(pat_num)),
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
                 WHERE pp.PatNum = ? OR pp.Guarantor = ? ORDER BY pc.ChargeDate",
                (Value::from(pat_num), Value::from(pat_num)),
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
        let mut conn = self.conn()?;
        let params = vec![
            Value::from(plan.pat_num),
            Value::from(plan.guarantor),
            Value::from(plan.pay_plan_date.format("%Y-%m-%d").to_string()),
            Value::from(plan.apr),
            Value::from(plan.note.clone()),
            Value::from(plan.completed_amt),
            Value::from(plan.pay_schedule.as_od()),
            Value::from(plan.number_of_payments),
            Value::from(plan.pay_amt),
            Value::from(plan.down_payment),
            Value::from(i8::from(plan.is_closed)),
            Value::from(plan.charge_frequency.as_od()),
            Value::from(
                plan.date_pay_plan_start
                    .map(|d| d.format("%Y-%m-%d").to_string())
                    .unwrap_or_default(),
            ),
            Value::from(
                plan.date_interest_start
                    .map(|d| d.format("%Y-%m-%d").to_string())
                    .unwrap_or_default(),
            ),
            Value::from(i8::from(plan.is_dynamic)),
            Value::from(plan.dynamic_pay_plan_tp_option.as_od()),
            Value::from(i8::from(plan.is_locked)),
        ];
        if plan.pay_plan_num == 0 {
            conn.exec_drop(
                "INSERT INTO payplan \
                 (PatNum,Guarantor,PayPlanDate,APR,Note,CompletedAmt,PaySchedule,NumberOfPayments,PayAmt,DownPayment,IsClosed,ChargeFrequency,DatePayPlanStart,DateInterestStart,IsDynamic,DynamicPayPlanTPOption,IsLocked) \
                 VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                params,
            )
            .map_err(|e| e.to_string())?;
            Ok(conn.last_insert_id() as Id)
        } else {
            let mut update_params = params;
            update_params.push(Value::from(plan.pay_plan_num));
            conn.exec_drop(
                "UPDATE payplan SET PatNum=?,Guarantor=?,PayPlanDate=?,APR=?,Note=?,CompletedAmt=?,PaySchedule=?,NumberOfPayments=?,PayAmt=?,DownPayment=?,IsClosed=?,ChargeFrequency=?,DatePayPlanStart=?,DateInterestStart=?,IsDynamic=?,DynamicPayPlanTPOption=?,IsLocked=? \
                 WHERE PayPlanNum=?",
                update_params,
            )
            .map_err(|e| e.to_string())?;
            Ok(plan.pay_plan_num)
        }
    }

    fn upsert_pay_plan_charge(&self, charge: &PayPlanCharge) -> Result<Id, String> {
        let mut conn = self.conn()?;
        // OD upsert semantics: PayPlanChargeNum=0 inserts, otherwise update the
        // existing charge in place. This matters for safe stale resolution,
        // where we zero/annotate a previously issued charge without deleting it.
        if charge.pay_plan_charge_num == 0 {
            conn.exec_drop(
                "INSERT INTO payplancharge \
                 (PayPlanNum,Guarantor,PatNum,ChargeDate,Principal,Interest,Note,ProvNum,ProcNum,ChargeType,FKey,LinkType) \
                 VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
                (
                    charge.pay_plan_num,
                    charge.guarantor,
                    charge.pat_num,
                    charge.charge_date.format("%Y-%m-%d").to_string(),
                    charge.principal,
                    charge.interest,
                    charge.note.clone(),
                    charge.provider_num,
                    charge.proc_num,
                    charge.charge_type.as_od(),
                    charge.fkey,
                    charge.link_type,
                ),
            )
            .map_err(|e| e.to_string())?;
            Ok(conn.last_insert_id() as Id)
        } else {
            conn.exec_drop(
                "UPDATE payplancharge SET \
                 PayPlanNum=?,Guarantor=?,PatNum=?,ChargeDate=?,Principal=?,Interest=?,Note=?,ProvNum=?,ProcNum=?,ChargeType=?,FKey=?,LinkType=? \
                 WHERE PayPlanChargeNum=?",
                mysql::Params::Positional(vec![
                    Value::from(charge.pay_plan_num),
                    Value::from(charge.guarantor),
                    Value::from(charge.pat_num),
                    Value::from(charge.charge_date.format("%Y-%m-%d").to_string()),
                    Value::from(charge.principal),
                    Value::from(charge.interest),
                    Value::from(charge.note.clone()),
                    Value::from(charge.provider_num),
                    Value::from(charge.proc_num),
                    Value::from(charge.charge_type.as_od()),
                    Value::from(charge.fkey),
                    Value::from(charge.link_type),
                    Value::from(charge.pay_plan_charge_num),
                ]),
            )
            .map_err(|e| e.to_string())?;
            Ok(charge.pay_plan_charge_num)
        }
    }

    fn upsert_adjustment(&self, adjustment: &Adjustment) -> Result<Id, String> {
        let mut conn = self.conn()?;
        // OD stores the service date on ProcDate. Manual entries in this port
        // leave proc_date at its zero value, so keep the previous behaviour
        // of using AdjDate for them.
        let proc_date = if adjustment.proc_date.year() >= 1880 {
            adjustment.proc_date
        } else {
            adjustment.adj_date
        };
        // OD's upsert semantics: AdjNum=0 inserts, otherwise update in place.
        // A plain INSERT with an explicit key would not update linked production.
        if adjustment.adj_num == 0 {
            conn.exec_drop(
                "INSERT INTO adjustment (AdjDate,AdjAmt,PatNum,AdjType,ProvNum,ProcNum,AdjNote,ProcDate) VALUES (?,?,?,?,?,?,?,?)",
                (
                    adjustment.adj_date.format("%Y-%m-%d").to_string(),
                    adjustment.adj_amount,
                    adjustment.pat_num,
                    adjustment.adj_type,
                    adjustment.provider_num,
                    adjustment.proc_num,
                    adjustment.note.clone(),
                    proc_date.format("%Y-%m-%d").to_string(),
                ),
            )
            .map_err(|e| e.to_string())?;
            Ok(conn.last_insert_id() as Id)
        } else {
            conn.exec_drop(
                "UPDATE adjustment SET AdjDate=?,AdjAmt=?,PatNum=?,AdjType=?,ProvNum=?,ProcNum=?,AdjNote=?,ProcDate=? WHERE AdjNum=?",
                (
                    adjustment.adj_date.format("%Y-%m-%d").to_string(),
                    adjustment.adj_amount,
                    adjustment.pat_num,
                    adjustment.adj_type,
                    adjustment.provider_num,
                    adjustment.proc_num,
                    adjustment.note.clone(),
                    proc_date.format("%Y-%m-%d").to_string(),
                    adjustment.adj_num,
                ),
            )
            .map_err(|e| e.to_string())?;
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
        let splits = payment.splits(allocations)?;
        let mut conn = self.conn()?;
        // One payment and all its splits must be written atomically; otherwise
        // a failed split would leave an unbalanced OD ledger.
        let mut tx = conn
            .start_transaction(mysql::TxOpts::default())
            .map_err(|e| e.to_string())?;
        tx.exec_drop(
            "INSERT INTO payment (PatNum,PayDate,PayAmt,PayType,CheckNum,PayNote) VALUES (?,?,?,?,?,?)",
            (
                payment.pat_num,
                payment.pay_date.format("%Y-%m-%d").to_string(),
                payment.pay_amount,
                payment.pay_type,
                payment.check_num.clone(),
                payment.note.clone(),
            ),
        )
        .map_err(|e| e.to_string())?;
        let pay_num = tx.last_insert_id().unwrap_or(0) as Id;
        let pay_date = payment.pay_date.format("%Y-%m-%d").to_string();
        for split in &splits {
            tx.exec_drop(
                "INSERT INTO paysplit (PayNum,PatNum,ProcNum,SplitAmt,DatePay,ProcDate,ProvNum,PayPlanNum,AdjNum,UnearnedType,PayPlanChargeNum,PayPlanDebitType) VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
                (
                    pay_num,
                    split.pat_num,
                    split.proc_num,
                    split.split_amount,
                    pay_date.clone(),
                    pay_date.clone(),
                    split.provider_num,
                    split.pay_plan_num,
                    split.adj_num,
                    split.unearned_type,
                    split.pay_plan_charge_num,
                    split.pay_plan_debit_type.as_db(),
                ),
            )
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
            Value::from(0u64),
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
            let sql = "INSERT INTO proctp \
                (TreatPlanNum,PatNum,ProcNumOrig,ItemOrder,Priority,ToothNumTP,Surf,ProcCode,Descript,\
                 FeeAmt,PriInsAmt,SecInsAmt,PatAmt,Discount,Prognosis,Dx,ProcAbbr,SecUserNumEntry,\
                 SecDateEntry,FeeAllowed,TaxAmt,ProvNum,DateTP,ClinicNum,CatPercUCR) \
                VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) \
                ON DUPLICATE KEY UPDATE ProcNumOrig=VALUES(ProcNumOrig),ToothNumTP=VALUES(ToothNumTP),\
                 Surf=VALUES(Surf),ProcCode=VALUES(ProcCode),Descript=VALUES(Descript),\
                 FeeAmt=VALUES(FeeAmt),ProvNum=VALUES(ProvNum),DateTP=VALUES(DateTP)";
            let params: Vec<Value> = vec![
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
                Value::from(0u64),
                Value::from(today),
                Value::from(0.0f64),
                Value::from(0.0f64),
                Value::from(line.provider_num),
                Value::from(od_date_param(line.date_tp)),
                Value::from(0u64),
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
                       VALUES (?,?,?,?) ON DUPLICATE KEY UPDATE ProcNum=VALUES(ProcNum),Priority=VALUES(Priority)";
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
        let mut conn = self.conn()?;
        let params: Vec<Value> = vec![
            Value::from(rule.rule_num),
            Value::from(rule.type_cur.as_od()),
            Value::from(minutes_to_ticks(rule.minutes_prior)),
            Value::from(rule.comm_type.as_od().to_string()),
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
        let rows: Vec<mysql::Row> = conn
            .query("SELECT ApptReminderRuleNum,ApptNum FROM apptremindersent")
            .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|row| (cell_u32(row, 0), cell_u32(row, 1)))
            .collect())
    }

    fn mark_reminder_sent(&self, rule_num: Id, apt_num: Id, pat_num: Id) -> Result<(), String> {
        let mut conn = self.conn()?;
        conn.exec_drop(
            "INSERT INTO apptremindersent \
             (PatNum,ClinicNum,SendStatus,MessageType,MessageFk,ApptReminderRuleNum,ApptNum,TSPrior) \
             VALUES (?,0,1,0,0,?,?,0)",
            (pat_num, rule_num, apt_num),
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
