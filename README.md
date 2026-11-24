# odclinic — a scoped Rust port of Open Dental

This is a Windows desktop application in Rust that ports selected Open Dental
modules. It is **not** a complete clone of `OpenDental.exe`; it is a working
foundation over the seven modules chosen for this port. **Imaging is excluded.**

Licence: GPL-2.0-only, matching the upstream source
(`OpenDental/opendental`, branch `24_3`).

## What the binary actually is (REA)

`C:\Program Files (x86)\Open Dental\OpenDental.exe`, SHA-256
`8e78c110e28c3052e0e83c07bf129449c2c6803a246c06aca8c75c907fe71007`:

* PE32 x86, IL-only, 32-bit-required, .NET Framework 4.8 (MVID
  `442fe367-681a-4074-b646-ba7a888adc12`).
* Complete metadata: 4,081 TypeDefs, 32,127 MethodDefs, 36,862 fields,
  985 manifest resources, 57 assembly references.
* Open Dental's own dependencies: `OpenDentBusiness`, `CodeBase`, `WpfOD`,
  `Bridges`, `DataConnectionBase` (all 25.3.48.0).
* Third-party: `MySqlConnector`, `Newtonsoft.Json`, `WebView2`, `MigraDoc`,
  `PdfSharp`, AWS SDK, `SharpDX`, `zxing`, `NHunspell`.
* 1,055 types in the `OpenDental` namespace, 122 in `OpenDental.UI`; many
  report forms (`FormRpAging`, `FormRpAppointments`, …).

REA evidence IDs used for the classification:
`ev_9385fb0b174144d74f8643f72deace7b124abcf4b9231e04899fd4a720c8b4d4`
(complete managed artifact record) and
`ev_7cf1b08b1c9864356bbdda8df3562885c8f744790d84de91c510d45b3d61df77`
(summary view). The inspection was static: nothing was loaded or executed.

### Source version gap

The newest public upstream branch is `24_3`; the installed executable is
`25.3.48.0`. This port therefore uses `opendental-source/` (branch `24_3`)
as its reference and uses REA on the 25.3 binary to verify names, dependencies
and UI structure. Where the two disagree, the binary wins and the difference
is recorded in this repository.

### Verified 25.3 differences

REA member inspection of the shipped binary (static, no execution) confirmed
the module entry points `OpenDental.FormOpenDental`, `ControlAppt`,
`ControlFamily`, `ControlAccount`, `ControlTreat` and `ControlChart` still
exist, but several UI types have moved out of the namespaces used by the
24.3 source:

| 24.3 source | 25.3 binary (REA) |
|---|---|
| `OpenDental.UI.ModuleBar` | `OpenDental.ModuleBar` |
| `OpenDental.Forms.FormReportsMore` | `OpenDental.FormReportsMore` |
| `OpenDental.Forms.FormRpAging` | `OpenDental.FormRpAging` |
| `OpenDental.Forms.FormRpAppointments` | `OpenDental.FormRpAppointments` |
| `OpenDental.Forms.FormRpProdInc` | `OpenDental.FormRpProdInc` |

`OpenDental.ControlImages` is also still present; it is excluded here by the
agreed scope, not because it disappeared. Evidence IDs:
`ev_52fd77df494c4655f74a29f41b8b1164003b309677af62c4c572ab5c2cefb13f`,
`ev_462439807e63d680764380bffd34e86e5562ee8729526bb45fc6fbc074299d6f`,
`ev_18933c49569519a634b9803488cd71f91c140f3072713a0c20c4ed9b73fd06a9`.
The Rust port uses stable module concepts rather than C# namespace names, so
this difference does not affect it; it is recorded to keep the source/version
gap explicit.

### Verified enum values and corrected bugs

Static REA reading of `OpenDentBusiness.dll` constants (no code executed)
confirmed the stored numeric codes used by this port:

| Enum | Verified values | Effect |
|---|---|---|
| `ApptStatus` | `None=0, Scheduled=1, Complete=2, UnschedList=3, ASAP=4, Broken=5, Planned=6, PtNote=7, PtNoteCompleted=8` | The port previously wrote `Broken=3` and `Planned=4`, which would corrupt appointment statuses. Now corrected. |
| `PatientGender` | `Male=0, Female=1, Unknown=2, Other=3` | `Other` is now representable instead of collapsing to `Unknown`. |
| `TreatPlanStatus` | `Saved=0, Active=1, Inactive=2` | The port had these reversed; now corrected. |
| `ProcStat` | `TP=1, C=2, EC=3, EO=4, R=5, D=6, Cn=7, TPi=8` | Replaced raw `i8` status numbers with a typed `ProcedureStatus` so balances and production still count only `C=2` procedures. |
| `EnumPermType` | `AppointmentsModule=1, FamilyModule=2, AccountModule=3, TPModule=4, ChartModule=5, Setup=8, Reports=22` | Confirmed the permission codes shown in Setup/Security. |
| `ClaimStatus` | `U, H, W, P, S, R, I` | Stored as a **one-character string** in `claim.ClaimStatus`, not a number. The port previously used an `i8`, which could never round-trip OD claims. Now a typed `ClaimStatus`. |

Evidence IDs:
`ev_1296df0d3b5ceb6cd4e8cd6d6aec0708ffc474030b94be4b239cb6bde286671c`
(`ApptStatus`, `PatientGender`, `TreatPlanStatus`),
`ev_ee243a37f6c757b1e3b4640147951b1913eea47115165b2abe2fbaee45e6f856`
(`ProcStat`), and
`ev_6d6df4f5bcd812be6d69969925edab9d6736837bd1ce10f5fca74cb2e08f8bc7`
(`EnumPermType`). Four unit tests in `od-domain` lock these mappings in place.

The billing slice was verified the same way: static REA member inspection of
`OpenDentBusiness.dll` confirmed that `Adjustment`, `PaySplit`, `Payment`,
`Claim`, `ClaimStatus`, `ClaimProc`, `Statement` and `Ledgers` all still exist in
25.3.48.0 (evidence
`ev_fc318f31f8ebbc27ada18860f5417e2658fb971dae37779767261af3a8e27e41` for the
first six and `ev_04489a2ee572e683cf519baee6d91678d056f942a678c269e2d281eb7a80d058`
for `ClaimProc`, `Statement`, `Adjustment` and `PaySplit`). Exact field names
come from the 24.3 source. The balance model follows
`Ledgers.GetAgingQueryString`: completed procedure fees + adjustments −
paysplits − received insurance payments/write-offs, with insurance estimates
shown separately and never subtracted.

The payment-plan slice was grounded in the same way: `PayPlan`, `PayPlanCharge`
and `PayPlanLink` still exist in the 25.3 binary (evidence
`ev_7ae375936c75cc6c2abdcdd03699d78dd5b17ffef51332e252ae57daad9a5dec`). Field
names and enum values come from the 24.3 `PayPlan.cs`, `PayPlanCharge.cs` and
`PayPlanLink.cs` sources. OD's `PayPlanChargeType` is `Debit=0` (charge due) and
`Credit=1` (production moved onto the plan); `PayPlanFrequency` is
`Weekly=0, EveryOtherWeek=1, OrdinalWeekday=2, Monthly=3, Quarterly=4`.
`paysplit.PayPlanNum` links a payment to its plan.

The definitions/display-fields slice was grounded in the 25.3 binary too:
`Def`, `DefCat`, `DisplayField` and `DisplayFieldCategory` are all still present
(evidence
`ev_907f3b126a98e11aca7b78fac3ecdacb859c3b893ca46030f710bc516eb049ee`,
`ev_c1d043e8e5caeb14141d1a2d2fccd1a82474403352c10670f970577c48140010`,
`ev_a9353319479707aa1cf94297215bccfc99f029229e2b410ec3c94302dde1f52b` and
`ev_291f8817f5b1144ea8fcada1e5a9d03745a7b2651c860b534822045c831db9a8`).
OD's `DefEdit=220` permission gates Definitions and `Setup=8` gates Display
Fields. `definition.ItemColor` is a signed ARGB integer, hidden definitions stay
in the table so references survive, and saving a display-field list identical to
OD's built-in defaults deliberately leaves no rows so OD falls back to its
defaults.

The clinical/waitlist slice was verified against both shipped assemblies:
`FormUnsched.Delete_Click()` in `OpenDental.exe` (evidence
`ev_1b447969417d0c4827dcf9214a61b32090ed35734f2fed0fbe5e015d85eb8320`) and the
constants/tables in `OpenDentBusiness.dll` (evidence
`ev_a14533544b373293d9714e772be249d1bbbc849f7d94526335586c909e74465e` and
`ev_5219fa2e1f69eb018654ff049d5d4d2c0175135d27681d0d584fddde51cd6471`).
Confirmed in 25.3.48.0:

| Constant | Verified value | Use in this port |
|---|---|---|
| `ApptStatus.UnschedList` | `3` | Waitlist rows (old `Broken=3` comment was wrong; `Broken=5`, `Planned=6`). |
| `DefCat.RecallUnschedStatus` | `13` | FK for `appointment.UnschedStatus`. |
| `HistAppointmentAction` | `Created=0, Changed=1, Missed=2, Cancelled=3, Deleted=4` | `histappointment` snapshots. |
| `ApptPriority` | `Normal=0, ASAP=1` | ASAP list and waitlist priority. |
| `EnumPermType` | `AppointmentCreate=25, AppointmentEdit=27, PatientCreate=106, PatientEdit=108, DefEdit=220, AppointmentDelete=237` | Action gates and audits. |

`HistAppointment.ToAppt()` copies every same-named field by reflection, so a
restore includes `UnschedStatus` and `Priority`. The `Delete_Click` IL also
settles one detail the 24.3 source made easy to miss: OD checks
`AppointmentEdit=27`, deletes the unscheduled appointments and writes
`securitylog` rows, but **also offers to copy appointment notes into an internal
Commlog** when notes exist. It never sends a patient-facing cancellation or
reconfirmation. This port keeps the direct delete, the audit and the absence of
patient messages, and deliberately omits the optional Commlog prompt.

## Architecture

| Crate | Responsibility |
|---|---|
| `od-domain` | Value types and enums (`Patient`, `Appointment`, `Procedure`, `TreatPlan`, `Userod`, …), field names mapped from OD classes. |
| `od-db` | `Store` trait, in-memory `DemoStore`, and `MySqlStore` using OD table/column names. `schema.sql` is a minimal compatible schema. |
| `od-core` | Business rules: schedule conflicts, patient balance, provider production, login and permission gates, reminder rule evaluation and pluggable local transports. |
| `od-gui` | `eframe`/`egui` desktop shell: top menu, left module bar, grid-style module views, Setup → eServices and Setup → Definitions / Display fields pages. |
| root `odclinic` | Binary entry point. |

The navigation follows `FormOpenDental`: a left `ModuleBar` (Appointments,
Family, Account, Treatment Plan, Chart, …) and a top menu with Setup and
Reports. The original `GridOD` becomes `egui::Grid` with striped rows and
selectable cells.

Schema notes that came from the source map:

* `Procedure` maps to the `procedurelog` table, not `proc`.
* Appointment duration is derived from `Pattern` (one character = one 5-minute
  increment), not stored as a length column.
* Money columns are `PayAmt`, `ProcFee`, `FeeAmt`, `AdjAmt`, `SplitAmt`,
  `InsPayAmt`, `WriteOff`; preferences use `ValueString`; treatment plans use
  `DateTP` and `TPStatus`; definitions use `definition.ItemName`, `ItemValue`,
  `ItemColor` (ARGB int) and `IsHidden`; display fields use `displayfield.InternalName`,
  `Description`, `ColumnWidth`, `Category`, `ChartViewNum`, `PickList` and
  `DescriptionOverride`.
* A payment can be split across patients/procedures, so the ledger credits
  `paysplit.SplitAmt`, never `payment.PayAmt`. `claimproc` has `InsPayEst` and
  `WriteOff` but **no `AllowedAmt`** column in 24.3/25.3.
* `patient` has `Title` but **no `Suffix`** column, and no `DateCreated` — the
  timestamp is `DateTStamp`. `Birthdate` is `NOT NULL`, so inserts without a
  birth date use Open Dental's conventional `0001-01-01`.
* `treatplan` has **no `ProvNum`** column; the provider field on the port's
  treatment-plan model is left at 0 when reading from MySQL.
* `userod.Password` is `HashType$Salt$Hash`; `grouppermission.PermType` is a
  `smallint` because codes exceed 127, and the table has **no
  `PermissionLevel`** column in the installed 25.3 demo schema. The audit
  table is `securitylog` (`LogDateTime`, `PermType`, `UserNum`, `LogText`).

## Running it

```bash
# Demo data, no database required
cargo run --release

# Against the local Open Dental demo database (MariaDB on localhost)
$env:ODCLINIC_DB_URL = "mysql://root:<password>@localhost:3306/demo"
cargo run --release
```

The URL accepts the shorthand `root:<password>@localhost:3306/demo` too. If
`ODCLINIC_DB_URL` is absent or the connection fails, the app falls back to the
seeded demo data and shows the reason in the status bar. The **Prefer demo
data** toggle in Settings overrides the database until it is turned off and the
app is restarted.

### Automated build sequence

`scripts/build_sequence.ps1` runs the standard build sequence autonomously and
stops on first failure. There is also a GitHub Actions workflow at
`.github/workflows/build.yml`: it runs the no-live-DB sequence on pushes/PRs
(`-SkipLive -SkipSmoke`), and can run live MySQL + smoke as a manual opt-in
workflow when `ODCLINIC_DB_URL` is configured as a repository secret and a MySQL
service is reachable.

```powershell
# Full sequence: workspace tests, live MySQL, check, build, smoke
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build_sequence.ps1 -Phase all

# Individual phases
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build_sequence.ps1 -Phase test
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build_sequence.ps1 -Phase live
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build_sequence.ps1 -Phase check
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build_sequence.ps1 -Phase build
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build_sequence.ps1 -Phase smoke
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build_sequence.ps1 -Phase release

# Skip optional live/smoke steps when DB is unavailable
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build_sequence.ps1 -Phase all -SkipLive -SkipSmoke
```

For live MySQL phases, it uses `ODCLINIC_DB_URL` or `MYSQL_ROOT_PASSWORD` if set;
otherwise it reads the stored local OpenChamber credential automatically. It
never prints the credential. Live MySQL tests run serialized with
`--test-threads=1`.

### Demo users

When running on the in-memory demo store, the login screen accepts
`admin` / `admin` (all modules and actions) and `aharrison` / `harrison`
(clinical modules only; no Setup, Reports or security administration). The
login screen has a **Demo credentials** button and a **Settings** escape hatch,
so the login requirement can be turned off without editing the settings file.

Create a fresh compatible database with:

```bash
mysql -u root -p < crates/od-db/schema.sql
```

⚠️ Use a copy of a database, not a live practice database: writes go straight
to the OD tables with their original column names.

### Live database verification

Sixteen ignored integration tests check the backend against a real OD schema:

```bash
# Read every table/column the port uses
$env:ODCLINIC_DB_URL = "mysql://root:<password>@localhost:3306/demo"
cargo test -p od-db --test mysql_live -- --ignored --nocapture
```

This runs `mysql_backend_reads_compatible_od_tables`,
`mysql_backend_writes_compatible_od_tables`,
`mysql_backend_supports_security_tables` and
`mysql_backend_supports_billing_tables`. The write tests create one temporary
patient, one temporary appointment and one temporary `securitylog` row, plus one
temporary `adjustment` and a `payment`/`paysplit` pair, read them back, then
delete them before asserting. Two more tests cover payment plans, multi-split
payments and statement rows; `mysql_backend_supports_recall_and_asap` writes a
temporary `recall` and ASAP-flagged `appointment`, reads both back through the
`Store` API and deletes them. `mysql_eservices_rules_outbox_and_dedupe_round_trip`
adds a temporary `apptreminderrule` rule, a port-local patient contact, a dedupe
key and an outbox row, reads each back and deletes all four.
`mysql_treatment_plans_round_trip` creates an Inactive plan and line, promotes
the plan to Active, verifies the `treatplanattach` link, saves a `proctp`
snapshot with the procedure's real fee, restores the patient's original plans
and deletes all temporary rows. `mysql_definitions_and_display_fields_round_trip`
writes a temporary `definition` row with an ARGB colour and hidden flag, reads it
back and deletes it, then replaces and restores the display fields of an
unpopulated OD category, verifying the "default list means no rows" save rule.
`mysql_waitlist_history_and_patient_corrections_round_trip` writes a temporary
`AptStatus = 3` waitlist row with a `RecallUnschedStatus` definition, snapshots it
to `histappointment`, deletes and restores it through `restore_appointment`, then
writes and reads a port-local patient correction before cleaning up all three
rows. `mysql_registry_and_clinic_card_round_trip` reads OD `disease`/`diseasedef`,
`patientnote` and `procedurelog`, then issues a temporary clinic card with a unique
authorisation marker, checks that no `statement` row was created, writes and reads
an `IsInvoice` statement, deactivates (rather than deletes) the card and cleans
both rows up. `mysql_local_reports_build_from_live_data` runs the recall/ASAP,
payment-plan and treatment-plan report builders over the real tables, and
`mysql_upstream_reports_build_from_live_data` runs the aging, payment-plan
detail, birthday, new-patient, appointments, active-patient, all four
production/income modes and the daily-payments builder; both check the CSV/HTML
exports.
`MySqlStore::new()` adds the `recall` table,
`appointment.Priority` and `appointment.UnschedStatus` columns, the
`histappointment` and port-local `odclinic_patient_correction` tables, four
eServices tables, full OD `treatplan`, `proctp` and `treatplanattach` columns,
`definition.ItemColor` plus the full `displayfield` table, the port-local
`odclinic_clinic_card` table and the `disease.PatNote`/`disease.ProbStatus`
columns idempotently so older demo databases work. URL-encode the password in
`ODCLINIC_DB_URL` if it contains `@`, `#`, `:` or other reserved characters.

Verified 2026-11-21 against the installed MariaDB 10.11 `demo` database:
15 patients, 4 providers, 6 operatories, 3 users, 1,205 preferences,
42 appointments, 295 definitions and 45 display fields; per-patient
procedures/payments/treatment plans and all read/write checks passed, including
the waitlist → `histappointment` snapshot → delete → restore → patient-correction
round trip. This run exposed and fixed three column problems:
`patient.Suffix`, `patient.DateCreated` and `treatplan.ProvNum` do not exist in
the real OD schema, plus `patient.Birthdate` being `NOT NULL`.

The security-table pass also succeeded: 3 `usergroupattach` rows, 783
`grouppermission` rows and 3 existing `securitylog` rows were read; one new
audit row was written, read back and deleted. It exposed one more schema
difference: real `grouppermission` has **no `PermissionLevel` column**, so the
port now reads that field as 0. All three `userod.Password` values in this demo
database are blank, which is valid in Open Dental: `CheckPassword` requires an
empty input for a blank stored password.

The billing pass (same date) read 1 claim, 3 claimprocs and 0 adjustments,
paysplits and statements on the first patient; it then wrote one adjustment and
one payment/paysplit, read both back through the `Store` API and deleted them.
This confirmed `claim.ClaimStatus` is read as a string and that the real OD
`adjustment`, `payment` and `paysplit` tables accept the columns the port writes
(`AdjDate`/`AdjAmt`/`AdjNote`/`ProcDate`, `PayDate`/`PayAmt`/`PayNote`,
`DatePay`/`ProcDate`/`SplitAmt`/`ProvNum`). The port also gained new OD
permissions `PaymentCreate=15`, `PaymentEdit=16`, `AdjustmentCreate=17`,
`AdjustmentEdit=18` and `Billing=62`.

## Implemented in this foundation

* Module navigation and top menu; MySQL/demo backend selection at startup.
* **Settings screen:** 110 on/off switches covering modules, patient actions,
  appointment actions (including recalls and the ASAP list), account/billing
  display (including insurance, payment plans and automatic discount pricing),
  procedure completion, treatment-plan editing,
  reports, local eServices, login, permission enforcement and audit granularity. Choices save automatically to
  `%APPDATA%\odclinic\settings.txt` and are honoured by the menus, grids and
  reports. Settings stays reachable even when the Setup module is hidden or
  the login screen is showing.
* **Login and permissions:** passwords are verified with an exact port of
  Open Dental's `Authentication` (MD5, ECW MD5, SHA3-512 and blank-password
  behaviour). Permissions are resolved from `usergroupattach` plus the legacy
  `userod.UserGroupNum`, then used to gate modules and actions.
* **Audit trail:** logins/logouts, patient, appointment and billing changes
  write to OD's `securitylog` table; each category has its own toggle and the
  last 50 entries can be viewed in Setup.
* REA-verified status/gender/permission codes, with regression tests in `od-domain`.
* Appointments: day schedule by operatory with status colours, create/edit/
  delete, conflict detection against the same operatory or provider.
* **Recalls and ASAP list:** three tabs in Appointments. Recalls read/write the
  OD `recall` table with the editable `DateDue`, OD's packed `Interval` integer
  (years/months/weeks/days), status key, note, disabled flag and priority, sorted
  overdue-first. "Schedule" opens a prefilled appointment. The ASAP list is
  derived from `appointment.Priority = 1` with active statuses, exactly as OD's
  `RefreshASAP` query does; the appointment editor has an **ASAP priority**
  checkbox. Both lists have their own toggles and are audited via Appointment
  changes.
* Family: patient search grid, detail pane, create/edit/delete patient.
* **Integrated patient registry:** a **Patient registry** button in the
  Treatment Plan and Family patient views opens one patient-centred window with
  Overview (contact details and the Account balance), Treatments & conditions
  (OD `procedurelog`, `disease` joined to `diseasedef`, and `patientnote`), a
  clinic-card panel and an A5 invoice panel. There is deliberately **no separate
  Clinic Cards module**. A clinic card can only be issued with a billing reason,
  an authorisation code and an explicit acknowledgement that the patient needs
  it for billing; it writes only to the port-local `odclinic_clinic_card` table,
  never to `statement`, `payment` or `adjustment`, and can only be deactivated,
  never deleted. Issuing and deactivating are audited in OD `securitylog`.
  **A5 invoice** builds one line per completed procedure (tooth/site, unit
  price, discount and tax), shows total, amount paid from that patient's
  paysplits and the balance, optionally records an OD `statement` row with
  `IsInvoice = true`, and writes a real A5 PDF 1.4 file with automatic page
  breaks. The three new toggles (**Patient registry**, **Clinic cards**,
  **A5 invoice**) are on by default and honour `PatientEdit`/`Billing`
  permissions.
* **Account/billing:** OD-sourced detailed balance (charges + adjustments −
  paysplits − received insurance, estimates shown separately), ledger of
  procedures/adjustments/paysplits, read-only claim and statement views, and
  write-capable **Record adjustment** / **Record payment** dialogs gated by the
  matching OD permissions and feature toggles. Balance can show credit when a
  patient overpays, like OD. The **Insurance & claims** switch is off by
  default: with it off, claim tables are not queried, insurance totals are
  hidden and the balance is charges + adjustments − patient paysplits only.
  Turn it on to use the full OD insurance model; the code and tables are kept
  either way.
* **Payment plans:** create static or dynamic patient plans (`payplan`,
  `payplancharge`, `payplanlink`). Static plans amortise a fixed amount with
  OD's `PayPlanEdit` period rate (`APR / 100 / periods-per-year`, banker's
  rounding), down payment dated today, final remainder absorbed, and a 2,000-
  charge ceiling; "Generate charges" refuses duplicates/closed/locked plans.
  Dynamic plans link completed procedures or adjustments as production, then
  "Issue charges due" creates only periods due on or before today. Dynamic
  issuance is idempotent (existing charge dates are not re-issued), respects the
  TP option (Await completion / Treat as complete), applies APR interest to
  principal not yet charged, and carries each charge's `FKey`/`LinkType` back to
  the linked production. Record plan payments through the same allocation dialog.
  Enabled by default; toggle **Payment plans** off in Settings to hide the whole
  feature.
* **Multi-split payments:** the Record payment dialog allocates one payment
  across several procedures, payment plans or unallocated cash, with a live
  "allocated vs payment" check. It writes one `payment` row and one `paysplit`
  row per allocation in a single transaction, so a single cheque covering a
  family's treatment is recorded as OD expects. Allocation amounts must sum to
  `payment.PayAmt`.
* **Statements:** generate a patient-only statement (charges, adjustments,
  payments and per-plan balances, with no insurance columns), write a dated OD
  `statement` row and open a print-ready HTML page in the browser. **Save PDF**
  writes a real PDF 1.4 file (Helvetica, A4, automatic page breaks) to an
  editable path and opens it in the default viewer. No PDF crate is used — the
  writer lives in `od-core/src/pdf.rs`. The line total ties to the Account
  balance. Gated by **Statements in Account** and the Billing permission.
* **eServices (local reminders):** Setup → eServices has Channels, Reminder
  rules and Outbox tabs. Rules mirror OD `apptreminderrule` (type, 100 ns
  `TSPrior` ticks, `DoNotSendWithin`, `SendOrder`, templates); dedupe uses OD
  `apptremindersent`. The engine renders OD-style `{FirstName}` / `{DateTime}`
  tokens, resolves a channel (OD `CommType` → Telegram for text, SMTP for
  email), honours patient opt-outs, and writes to a local outbox instead of
  OD's HQ-side AutoComm records. **SMTP email** (`lettre`, STARTTLS optional)
  and the **Telegram Bot API** (`ureq`) deliver live; WhatsApp Cloud API,
  local GSM modem, Africa's Talking and SMS.to are selectable/planned options
  whose settings are already persisted in `%APPDATA%\odclinic\eservices.txt`.
  New rules can be edited in the GUI, "Generate reminders" evaluates today's
  schedule, and "Deliver pending" sends queued rows with per-row status,
  attempt count, receipt/error and `securitylog` audits. **Dry-run is on by
  default**, and the master switch is off until you enable it, so nothing
  leaves the machine until a channel is configured. **Send message** buttons
  also appear in Appointments/Schedule and Recalls. No OD HQ server,
  registration key or eConnector is used at any point.
* **Treatment plans:** create/edit plans, set heading, date, note and OD
  `TPStatus` (`Saved=0`, `Active=1`, `Inactive=2`), and enforce OD's one-Active-
  plan-per-patient rule in code. "Make active"/"Make inactive" promote or
  demote a plan; "Save as snapshot" creates a separate Saved plan with copied
  lines. Procedure lines can be added, edited and removed from the plan grid.
  Active/Inactive plans write `treatplanattach` links to `procedurelog`; Saved
  snapshots write immutable `proctp` copies. Editing is gated by the Treatment
  Plan Edit feature toggle and OD `TreatPlanEdit=38` permission, with optional
  `securitylog` audits.
* **Definitions and display fields:** Setup → Definitions mirrors `FormDefinitions`:
  definitions are grouped by category (all 56 OD `DefCat` values, with the 18
  editable categories shown), sorted by `ItemOrder`, and can be added, edited,
  hidden (not deleted, so references survive), moved up/down or alphabetized.
  The grid shows the category's value label and ARGB colour where OD supports
  them. Setup → Display fields mirrors `FormDisplayFields`: pick one of the 22
  OD categories, rename or resize columns, add/remove/reorder them, restore the
  defaults, and save. Descriptions must be unique and must not match an internal
  name, matching OD's validation. Saving the built-in default list removes the
  category's rows so OD falls back to its own defaults. Definitions are gated by
  `DefEdit=220` and display fields by `Setup=8`, with an optional shared audit
  toggle. Both write the real OD `definition` and `displayfield` tables.
* **Discount plans:** Setup → Discount plans edits the real OD `discountplan`
  and `discountplansub` tables. A plan carries a reduced `FeeSchedNum`, an
  adjustment-type `DefNum`, a note, an annual maximum and a hidden flag; a
  subscription links one patient to a plan with inclusive effective/term dates
  (a blank term means OD's open-ended zero date, so `IsValidForDate` stays true).
  Subscriptions can be ended (sets `DateTerm`, never deletes, so historic
  adjustments still resolve) and reopened (clears the term). Plans, fee
  schedules and adjustment types are picked from the loaded OD tables, and the
  Discount Plan report gains a description filter. This is the patient-facing
  pricing path for insurance-free practices, so it is on by default and works
  with **Insurance & claims** off; it is gated by `DiscountPlans` and OD
  `Setup=8`, with optional `AuditBillingChanges` audits.
  Plans also carry OD's seven frequency-limit fields and the annual maximum
  (`-1` = unlimited), all persisted to the real table and editable in Setup.
* **Procedure completion and automatic discount pricing:** Chart now creates
  planned procedures (date, CDT code, fee, tooth and units) via `New procedure`
  and offers `Complete` on each row. Completion follows OD's
  `Procedures.Insert`/`FormProcEditUpdate` → `Adjustments.CreateAdjustmentForDiscountPlan`
  path: it finds the patient's active subscription, checks the inclusive
  effective/term range, applies the seven frequency limits from the
  `DiscountPlan*Codes` preferences, prices the procedure from the plan's fee
  schedule (`ProcFeeTotal − reduced fee × Quantity`), clamps it to the annual
  maximum in the subscription's anniversary bucket, and posts the difference as
  a negative `adjustment` of the plan's adjustment type with `ProcDate` set to
  the service date. It then stores `procedurelog.DiscountPlanAmt` and sets
  `ProcStatus = Complete` plus OD's `DateComplete`, auditing both writes with
  `AdjustmentCreate=17` and `ChartModule=5`. Gated by `ProcedureCompletion` and
  `DiscountPlanAutoAdjust` (both default on); posting requires OD
  `AdjustmentCreate` when permissions are enforced. The pure rules live in
  `od-core::discount` and are covered by unit tests, a DemoStore round trip and
  a serialized live-MySQL test. Provider-level and practice-default fee-schedule
  fallbacks are approximated by searching the loaded schedules because the port
  does not model `provider.FeeSched`.
* **Clinical workflow and waitlist:** a Waitlist tab in Appointments shows OD's
  unscheduled list (`AptStatus = 3`) with an editable `RecallUnschedStatus`
  definition. Rows can be added, edited, scheduled into a real appointment or
  removed. Removal mirrors `FormUnsched.Delete_Click()`: it audits with
  `AppointmentEdit=27` and deletes the unscheduled row, sending no patient-facing
  cancellation or reconfirmation (OD's optional Commlog note prompt is omitted).
  Appointment times and lengths are strictly validated instead of silently
  defaulting. **Appointment history** snapshots every create/change/miss/
  cancel/delete to the real OD `histappointment` table and restores a snapshot
  field-for-field via `HistAppointment.ToAppt()` semantics, so mistaken removals
  can be undone. **Patient correction history** keeps a before-image, reason and
  timestamp in the port-local `odclinic_patient_correction` table with rollback,
  because OD has no patient-history table. All three features have their own
  toggles (`WaitlistEnabled`, `AppointmentHistory`, `PatientCorrectionHistory`,
  on by default) and honour the matching OD permissions.
* Chart: universal-numbering tooth chart and procedure history.
* Reports: patient list, production by provider, collections, schedule, plus
  three locally useful slices — **recall/ASAP due list**, **payment-plan
  balances** and **treatment-plan summary**. Collections credits paysplits rather
  than payment totals, so a payment split across patients is not counted twice.
  The recall list uses OD's editable `DateDue` (falling back to `DateDueCalc`)
  and sorts overdue patients first; ASAP rows come from
  `appointment.Priority = 1`. Payment-plan balances reuse the Account module's
  `pay_plan_balance` calculation so the report cannot disagree with the ledger,
  and treatment-plan summaries read the same `treatplan`/`treatplanattach`/`proctp`
  lines as the Treatment Plan module. Each report exports RFC-4180 CSV and a
  printable HTML table, gated by its own toggle. The grid headers are clickable
  for numeric-aware sorting (totals sort as numbers, labels as text) and exports
  follow the displayed order. A practice name field is printed on every export
  and, along with the last export path per report, is persisted in
  `%APPDATA%\odclinic\reports.txt`. The dedicated audit/export paths — account
  audit bundle, filtered account ledger, payment-plan stale-resolved audit and
  security-log extract — are saved there too, with patient-scoped exports keyed
  by `PatNum` so two accounts keep separate filing locations across restarts.
* Upstream `FormRp*` tranche 1: four reports ported from the public 24_3 C#
  source with OD's own column layout, each with its own toggle (on by default):
  * **Patient aging** (`FormRpAging` / `Ledgers.GetAgingQueryString`): balances
    in `0-30`, `31-60`, `61-90` and `> 90` buckets, credits applied to the oldest
    debt first, and a total row. Charges after the as-of date are excluded, so a
    past date is reproducible. Its "Age of Account" radios (Any / Over 30 / Over
    60 / Over 90), negative-balance checkboxes, "Last Pay Date" and "Age
    write-off estimates" columns match the original dialog. The `-W/O Change`,
    `-Ins Est` and `=Patient` columns only appear when **Insurance & claims** is
    on (write-offs additionally need the age-write-offs option).
  * **Payment plans** (`FormRpPayPlans` / `RpPayPlan`): provider, guarantor,
    principal, accumulated interest, paid, balance, due now and balance not due,
    using the upstream accumulation formulas. The original options are present:
    plan type (Both / Patient / Insurance), Hide completed plans, Show Family
    Balance and "Limit to plans created in date range" (on by default). A
    stale-resolved charge count and the staff-entered resolution reasons are
    shown per plan (toggleable) and flow into the CSV/HTML export/print path.
    "Only plans with stale-resolved charges (audit view)" narrows the report to
    that subset and forces the reasons column on; the **Filtered audit export**
    toolbar saves only those plan rows as CSV or HTML with a
    `pay-plan-stale-resolved-<date>` default filename, ready to file alongside
    the security-log extract. Patient plans are read from `payplan.PlanNum = 0`;
    insurance plans are hidden entirely while **Insurance & claims** is off.
  * **Birthdays** (`FormRpBirthday`): OD's `LName/FName/Preferred/Address/Add2/
    City/State/Zip/Birthdate/Age` columns, comparing month/day so the birth year
    is ignored and ranges wrapping the new year still work, with a "Next Month"
    shortcut and age as of the report date.
  * **New patients** (`FormRpNewPatients`): a patient is new when their *first
    completed procedure* falls in the range (as upstream defines it), with OD's
    `#/Date/LName/FName/Referral/Production Fee` columns. "Include address
    information" and "Exclude patients with no production" mirror the original;
    since this port does not store referrals or preferred names, those columns
    are blank.
  Every option above lives in the "Report options (as in Open Dental)" panel.
  The practice name and export paths stay practice-wide, while each option set
  is saved per signed-in user in `reports.txt` (as `user_options.user-<UserNum>`
  or `user_options.local`) and restored on restart or login/logout, so two staff
  members can keep different report defaults without overwriting each other.
  Options from an older shared installation become the first user's starting
  set rather than being discarded.
  These reports use a variable-column `ReportTable` model; the local three-column
  reports are wrapped into it, so the CSV header stays
  `Practice,Report,Item,Value,Detail` for them while upstream exports keep OD's
  column names. All four sort by any column and print the practice name.
* Upstream `FormRp*` tranche 2: six more insurance-free reports ported from the
  24_3 C# source, each with its own toggle (on by default):
  * **Appointments** (`FormRpAppointments` / `RpAppointments`): OD's
    `Date/PatNum/Patient/Age/Time/Length/Description/Home Ph./Work Ph./Cell Ph.`
    columns, one row per booking. Planned and unscheduled-list appointments are
    always excluded, "Note" appointments are opt-in, and the date range uses
    OD's inclusive upper day. The "Appointment Date Created" radio adds the
    `DateCreated` column; this port has no `SecDateTEntry`, so that column falls
    back to the appointment date. WebSched filters and clinic splits are not
    applicable to this local-first port. Length is `Pattern` characters × 5
    minutes, as upstream.
  * **Active patients** (`FormRpActivePatients` / `RpActivePatients`): one row per
    patient with a completed procedure in the range, with OD's
    `Name/Provider/Address/Address2/City/State/Zip/Carrier` columns and a
    "Patient Count" summary under `Carrier`. The "All providers" checkbox and
    provider picker mirror the upstream list. This report does not expose
    upstream's billing-type, patient-status and clinic filters (those fields
    are stored on the patient but are only filtered by Custom aging); `Address2`
    and `Carrier` are therefore blank (insurance is off by default).
  * **Production & income** (`FormRpProdInc`): all four upstream modes —
    **Daily** (one row per procedure, adjustment, paysplit or insurance
    transaction), **Monthly** (one row per day with transactions, including
    OD's `Sched` and `Unearned Pt Income` columns), **Annual** (one row per
    month, labelled `MMM yyyy`) and **Provider** (totals grouped by provider) —
    with OD's headers and total rows in the upstream order
    (`Date/Day/Production/Sched/Adj/[Write-off]/Tot Prod/Pt Income/
    [Unearned Pt Income]/[Ins Income]/Tot Income/Total Pt Income`). Production
    uses the stored `procedurelog.ProcFee`, adjustments come from
    `adjustment.AdjAmt` and patient income from paysplits. `Sched` is priced
    from scheduled appointments joined to `TreatmentPlanned` procedures by
    `procedurelog.AptNum` and bucketed on the **appointment date** (OD's
    `DtimeToDate(AptDateTime)`), not the procedure date. Unearned income comes
    from `paysplit.UnearnedType != 0`: it appears in its own column in Monthly
    and Annual, folds into the single Pt Income detail column in Daily (OD's
    `UnearnedIncome + PayAmt`), and is excluded entirely when the **Include
    Unearned Pt Income** checkbox is cleared. Unearned types whose
    `PaySplitUnearnedType` definition (category 29) has a nonblank `ItemValue`
    are hidden, as upstream's `GetHiddenUnearnedDefNums` does. Insurance income
    and write-off columns appear only when **Insurance & claims** is on; with
    it off the totals use the reduced patient-only formulas. Two limits remain:
    production sums the stored `ProcFee` instead of upstream's
    `ProcFee x (UnitQty + BaseUnits)` (those columns are read from
    `procedurelog` for Referral Analysis but are not applied here), and this
    report always uses OD's "procedure date" write-off mode, attributing
    insurance rows to their procedure's date; the alternate write-off-date
    modes are exposed only on Custom aging below.
  * **Daily payments** (`FormRpPaySheet` / `RpPaySheet` patient-payment half):
    OD's `Date/Paying Patient/Provider/Check#/Amount` columns, grouped under
    payment-type headings with a `Total Patient Payments:` row. Amounts are the
    sum of `paysplit.SplitAmt` and provider comes from each split's `ProvNum`,
    so one check split across providers produces one row per provider
    ("Show splits by provider separately" is on by default). All/single
    provider and all/single payment type filters, plus the Check/Patient
    "Group By" radio, are ported. The insurance-payment section is not: it
    needs claim-payment headers, carriers and check numbers this port does not
    store. Clinics, CareCredit/PayConnect merchant fees, online-payment
    splitting and unearned prepayments are likewise omitted because the
    matching columns are not stored.
  * **Daily adjustments** (`FormRpAdjSheet` / `RpAdjSheet`): OD's
    `Date/Patient Name/Prov/AdjustmentType/Note/Amount` columns, one flat row per
    adjustment in the inclusive `AdjDate` range, ordered by date with no totals
    or footers. `Amount` is the stored signed `adjustment.AdjAmt` verbatim. The
    report uses OD's `INNER JOIN definition`/`patient` and `LEFT JOIN provider`,
    so adjustments with an unresolved patient or adjustment type are dropped and
    a missing provider prints a blank abbreviation. All/single provider and
    all/single adjustment-type filters are ported; "All" includes hidden
    `DefCat.AdjTypes` definitions even though the picker lists only visible
    ones. Clinics are omitted because this port has none.
  * **Finance charges** (`FormRpFinanceCharge` / `RpFinanceCharge`): OD's
    `PatNum/Patient Name/Preferred Name/Amount` columns, one row per
    finance-charge adjustment in the inclusive date range, ordered by patient
    last name, first name and then amount descending. `Amount` is the signed
    `adjustment.AdjAmt`. OD reads the charge type and last-run date from
    practice preferences and filters on the patient's primary provider; this
    port offers an adjustment-type picker (with "All" as the default) and an
    all/single primary-provider filter. `Preferred Name` is blank and the
    billing-type filter is omitted because this port does not store patient
    nicknames or billing types.
  All six share the same options panel, persistence, click-to-sort grid and
  CSV/HTML exports as tranche 1.
* Upstream `FormRp*` appointment reports: three more insurance-free reports from
  the 24_3 C# source, each with its own toggle (on by default):
  * **Broken appointments** (`FormRpBrokenAppointments` / `RpBrokenAppointments`):
    all three upstream modes — **By appointment status** (`AptStatus = Broken`,
    with `AptDate/Patient/Doctor/Hygienist/IsHyg` columns), **By broken-appointment
    adjustment** (`Date/Provider/Patient/Amount/Note`) and **By ADA code**
    (`Date/Provider/[Code]/Patient/Fee`, filtering `ProcStatus = Complete` for
    D9986 missed / D9987 cancelled / both). Each mode carries upstream's footer
    rows (`Total Broken Appointments:`, plus the amount/fee total). The provider
    and date filters and the adjustment-type picker are ported. OD reads the mode
    from practice preferences and offers a multi-select of positive adjustment
    types; here the mode is a radio group and one type (or All, meaning every
    type including zero/negative) is selectable. Clinics are omitted (this port
    has none), and `Hygienist`/`IsHyg` are blank/False because `ProvHyg` and
    `IsHygieneApt` are not stored.
  * **Confirmations** (`FormRpConfirm`): the upstream field-selection report —
    the same 15 standard fields (selected by default) and 25 "other" fields from
    the original two-list picker, emitted as one column per selected field over
    appointments in the date range. Upstream scopes rows to the confirmation
    list's selected `AptNum`s; this port uses the range. `AptDateTime` comes from
    the appointment, every other field from the raw `patient.<Field>` value;
    unmodelled fields (Preferred, Address2, WkPhone, billing type, notes,
    insurance carriers, student/school data, guarantor, secondary provider,
    image folder) are blank rather than invented. `Salutation` uses the stored
    patient title.
  * **Recall field selection** (`FormRpRecall`, distinct from the local
    Recall & ASAP due list): the upstream 16 standard fields (selected by
    default) and 21 "other" fields over recalls due in the date range.
    `RecallInterval` is emitted with upstream's `MonthInterval` alias and the
    exact `ROUND(RecallInterval / 65536)` formula. Upstream scopes rows to the
    recall list's selected `RecallNum`s; this port uses the range. Insurance
    carrier/relationship fields are commented out upstream and are blank here.
  These three share the same options panel, persistence, click-to-sort grid and
  CSV/HTML exports as the earlier upstream reports. **Routing slips**
  (`FormRpRouting`) are ported in the second tranche below and export their OD
  fields as a grid. **Web Sched appointments** (`FormWebSchedAppts`) are **not**
  ported because they need WebSched provenance logs, `ProvHyg` and `WkPhone`,
  which this local-first port does not store. Monthly production goal
  (`FormRpProdGoal`) is also ported in the second tranche below and reads OD's
  `schedule`, `scheduleop` and `provider.HourlyProdGoalAmt` tables.
* Upstream `FormRp*` procedure reports: two more insurance-free reports from the
  24_3 C# source, each with its own toggle (on by default):
  * **Daily procedures** (`FormRpProcSheet` / `RpProcSheet`): both upstream
    layouts — **Individual** (one row per completed procedure with OD's
    `Date/Patient Name/Code/Tooth/Area/Description/Prov/Fee` columns, ordered by
    date then patient then code then tooth) and **Grouped by code** (`Category/
    Code/Description/Quantity/Average Fee/Total Fees`, with category names from
    `DefCat.ProcCodeCats` and ordered by category item order then code). Fees are
    the stored `procedurelog.ProcFee` verbatim and the all/single provider and
    procedure-code substring filters are ported. The "Area" column is blank
    because tooth surfaces (`procedurelog.Surf`) are not stored. Clinics,
    capitation write-offs (claims are off by default) and row-level clinic
    numbers are omitted. Uncategorised codes are dropped in the grouped layout, matching
    upstream's `INNER JOIN definition` on `procedurecode.ProcCat`.
  * **Procedure codes** (`FormRpProcCodes` / `RpProcCodes`): every CDT code with
    its `Code/Desc/Abbr/Fee` from a chosen `feesched`, optionally with the
    `Category` column first (the **Categories** layout, ordered by `ProcCat`
    definition FK then code) instead of the default **Code** layout (ordered by
    code). OD's `-1` fee sentinel and codes with no fee row are hidden unless
    **Show blank fees** is checked. The fee schedule and provider pickers are
    ported; the clinic selector is fixed to OD's default clinic (0) because
    this port has none. `SuppressIfDuplicate` on the printed Category column is
    a print-layout flag, so every tabular/CSV row keeps its category name, and
    the fee uses a plain two-decimal format instead of .NET's culture-specific
    `"n"` thousands separator.
  * Daily procedures and procedure codes share the same options panel,
    persistence, click-to-sort grid and CSV/HTML exports as the earlier
    upstream reports.
  * **Prescriptions** (`FormRpPrescriptions` / `RpPrescriptions`): prescriptions
    from OD's `rxpat` table with the upstream column set (`Patient Name`, `Date`,
    `Drug Name`, `Directions`, `Dispense`, `Prov Name`) and a case-insensitive
    prefix filter on either the patient's last name (the "Patient" radio, the
    default) or the drug name (the "Drug" radio). Rows with no matching patient
    or provider are omitted, matching upstream's two `INNER JOIN`s, and ordering
    follows the original (`LName, FName, RxDate` for Patient, `LName, Drug, RxDate`
    for Drug). The shared report date range is ignored because the original form
    has no date filter. Prescription inserts are read-only in this port; OD's
    ePrescribe/bookkeeping columns are represented in the schema but not exposed.
  * **Referral analysis** (`FormRpReferralAnalysis` / `RpReferralAnalysis`):
    referring sources ranked by distinct `refattach` patients (`RefType =
    RefFrom` only) and the production those patients generated. Production is
    upstream's `ProcFee x (UnitQty + BaseUnits)`, restricted to completed
    procedures whose CDT code is not D9986/D9987. The date range and provider
    multi-select are ported, along with **Include address information** (Title,
    Address, Add2, City, State, Zip and Specialty from `DefCat.ProviderSpecialties`)
    and **Only patients new in this range**, which uses upstream's
    `HAVING MIN(ProcDate) BETWEEN ...` rule. Referral and refattach rows are
    read from OD-compatible `referral` / `refattach` tables; this port never
    creates them.
  * **Referrals** (`FormRpReferrals` / `RpReferrals`): the upstream field-selection
    query builder over OD's `referral` table. Pick any of the 19 raw columns
    (Last/First/Middle Name, Title, Address/Add2, City, State, Zip, Telephone,
    Phone2, Email, Note, SSN, UsingTIN, IsHidden, NotPerson, ReferralNum and
    Specialty) with **All**/**None** controls, and stack WHERE prerequisites
    joined by AND. Conditions are `LIKE`, `=`, `>`, `<`, `>=`, `<=` and `<>` with
    upstream's case-insensitive SQL matching (`%`/`_` wildcards stay active);
    numeric columns compare as numbers and varchar columns as strings. The
    Specialty dropdown uses `DefCat.ProviderSpecialties` and filters on the raw
    DefNum, while IsHidden/NotPerson/UsingTIN use False/True (`0`/`1`) exactly
    as upstream's dropdowns do. Values render raw, as the original `SELECT`
    does — booleans as `0`/`1` and Specialty as the numeric definition FK, not
    its name. The shared report date range is ignored because the original form
    has no date filter. Referral rows are read-only in this port.
  * **Custom aging** (`FormRpCustomAging` / `RpCustomAging`): patient balances in
    the 0-30/31-60/61-90/over-90 columns, built from the transaction types you
    select. The non-insurance upstream types are ported — Procedure Fees
    (`ProcFee x (UnitQty + BaseUnits)` on completed procedures), Adjustments,
    Pay Plan Charges (debits), Pay Plan Production (credits) and Patient Payments
    (`paysplit.SplitAmt`) — together with **Age of Account**, the three
    **Negative Balances** modes, **Age Credits**, provider and clinic
    multi-selects and **Exclude bad addresses** (blank Zip). As with upstream,
    credits are applied oldest-first unless Age Credits keeps them in their own
    bucket; a patient's plan payments only enter the report when both plan
    transaction types are selected; and the report refuses to run when no
    transaction type is ticked. All four upstream insurance types are ported
    and gated by the **Insurance & claims** toggle (off by default in this
    insurance-free domain): `InsPay` (`claimproc.InsPayAmt` for statuses
    1/4/5/7), `InsEst` (`InsPayEst` for status 0), `WriteOff` (the live
    `WriteOff` for paid statuses) and `WriteoffEst` (status 0, or a received
    claim dated after the as-of date). Actual write-offs age by **procedure
    date** by default; the **claim payment date** mode splits the amount using
    `claimsnapshot.Writeoff` (a `-1` nulls to 0) on the procedure date and the
    remaining delta on `DateSuppReceived`. Because this port has no
    `claimproc.DateCP`, `DateSuppReceived` (falling back to the procedure date)
    stands in for OD's insurance payment date. **Family** grouping aggregates
    every member's transactions under `patient.Guarantor`; provider, clinic,
    billing-type and patient-status filters apply to that grouped row. `Billing
    Types` (none selected = all), **Exclude inactive** (`PatStatus != 2`) and
    **Exclude archive** (`PatStatus != 3`) mirror upstream's final WHERE.
    Rows are ordered by `LName, FName` and a `Totals:` footer closes the grid,
    as in the original.
  * **Patients** (`FormRpPatients`): the upstream raw-SQL field-selection
    builder over OD's `patient` table. Pick any of the 20 supported columns —
    the identifying/contact fields this port stores, plus `First Complete Proc`
    and `Last Complete Proc` (upstream's correlated `MIN`/`MAX(ProcDate)` for
    completed procedures) and `RecallStatus` (the raw definition number) — with
    **All**/**None** controls, and stack WHERE prerequisites joined by AND with
    `LIKE`, `=`, `>`, `<`, `>=`, `<=` or `<>`. Conditions follow upstream's SQL
    semantics: `LIKE` wraps the value as `%value%` with case-insensitive
    `%`/`_` wildcards, numeric columns (`PatNum`, `Gender`, `PriProv`,
    `ClinicNum`, `RecallStatus`) compare as numbers and the rest as strings.
    Insurance and aging-balance columns (`InsEst`, `BalTotal`, the `Bal_x`
    buckets), carrier/relationship filters and columns with no field in this
    port's patient model (`WkPhone`, `SSN`, `Address2`, `BillingType`, …) are not
    offered. The original form has no date filter, so the shared report range is
    ignored, and rows are ordered by surname, forename and `PatNum` for a stable
    export. Upstream's comma joins would drop patients with no recall or no
    procedure; this port keeps them with blank derived values.
  * **Second upstream tranche (22 more `FormRp*` ports, 49 report kinds total).**
    All are pure builders in `od-core/src/reports_upstream.rs` wrapped in the
    same variable-column `ReportTable` as the first tranche, with their own
    default-on feature toggle and saved options in `reports.txt`:
    * **Accounting** — Balance Sheet (`FormRpAccountingBalanceSheet`), General
      Ledger (`FormRpAccountingGenLedg`) and Profit & Loss
      (`FormRpAccountingProfitLoss`) read OD's `account` and `journalentry`
      tables. They use upstream's sign rule (Asset/Expense debit-positive,
      Liability/Equity/Income credit-positive), fold prior-year income/expense
      into retained earnings and derive current-year net income — never stored.
      The Balance Sheet uses the range's end date; the Ledger shows opening and
      closing balances per account. Those reports are no longer read-only: the
      Account ledger's adjustment **and payment** rows have a **Post to ledger**
      action, and the accounting reports offer **Post all unposted sources** to
      book every charge, payment and adjustment in the report range in one run
      (toggle `ledger_journal_posting`, on by default, plus the OD *Adjustment
      Create* permission). Each posting writes one balanced `journalentry`
      transaction — a debit and a credit of the source amount's absolute value
      sharing a `TransactionNum` and date — through
      `Store::post_journal_transaction`, which inserts both lines atomically in
      MySQL. The batch path uses `Store::post_journal_batch` to write every
      source's pair in a single DB transaction, so a failed run can never leave a
      partial posting. Debit/credit accounts default from the chart by direction
      (a completed charge or positive adjustment debits A/R and credits income;
      a write-off debits expense and credits A/R; a collection debits cash and
      credits A/R; a refund reverses that) and can be overridden — the batch
      window exposes all four pairs and previews each line with its type, date,
      source amount and accounts before anything is written. A payment's cheque
      number is carried onto both lines. Charge amounts are the procedure fee —
      the same figure the production reports use — and each pair is dated to the
      procedure. The lines are previewed before confirmation, the patient balance is unchanged,
      and each posting writes an `Adjustment Create` security-log entry so the
      source record stays traceable from the ledger to the accounting reports.
      Each memo carries a stable `[Adj #… pat #…]`, `[Pay #… pat #…]` or
      `[Proc #… pat #…]` reference that is re-attached even when staff edit the
      note, so an already-posted record shows **Posted #⟨transaction⟩** instead
      of the button (and is skipped by the batch, which is rebuilt from fresh
      data on confirm) — no duplicate journal pair can be booked (pre-reference
      memos are still recognised). The batch writes one audit line per touched
      patient. The **Journal posting
      audit** report lists every charge, payment and adjustment in a date range
      with its posting status and transaction number, plus posted/unposted totals
      per source, so an unposted record cannot be silently left out of the
      accounting reports. It shares the `ledger_journal_posting` toggle. The
      **Ledger tie-out** report is the period-close check: it compares completed
      procedure fees with the A/R charge debits, payments with cash debits, and
      signed adjustments with their A/R movement, then reconciles the expected
      A/R movement (charges + adjustments − collections) with every A/R journal
      line in the range. Each check shows the clinical figure, the journal
      figure, the variance and an explicit `OK` or `OUT OF BALANCE` status, so a
      missing posting, a duplicate or an unsourced manual journal entry cannot
      hide behind a report that merely looks complete. It also shows the A/R
      closing balance, the paysplit collections used by the Collections report,
      and an unposted-source count. Together these turn period close into one
      reviewed run: **Post all unposted sources**, then re-run the tie-out until
      every check reads `OK`. On by default with the same feature toggle.
      Closing is now an enforced gate, not just a report: once the tie-out
      reconciles, **Close period** signs off the month shown in the range. It
      requires a reason and the OD *Adjustment Create* permission, persists the
      lock in the `preference` table (`AccountingClosedPeriods`) so it survives
      a restart, and writes a `Close`/`Reopen accounting period` entry to the
      security log (always audited, independent of the audit-log toggle). While
      a month is closed, every posting window refuses sources dated in that
      month and the batch window excludes them with a visible count — a
      backdated charge, payment or adjustment cannot silently undo the
      signed-off period. **Reopen** (same permission, with its own reason, also
      audited) unlocks the month for a late correction; close it again with a
      new reason when finished. Closed months in the current report range are
      flagged above the report. The **Period close history** report is the
      read-only sign-off register: every audited close and reopen is listed
      with its timestamp, month, action, the acting user resolved from `userod`
      (`Local user` for the implicit login-free session, `User #…` when the
      record is missing), reason, the reason a reopen superseded, and that
      month's *current* posting lock. Case-insensitive **user**, **month** and
      **action** filters narrow the register; every export contains exactly the
      filtered rows shown on screen. Those three filters are saved per signed-in
      user in `reports.txt` and restored on restart or login/logout. It reads
      only the security log and the
      persisted preference — there is nothing to edit — and ignores the report
      date range so no sign-off can be missed. It uses the
      `period-close-history` export stem and shares the `ledger_journal_posting`
      toggle.
    * **Hidden pay splits** (`FormRpHiddenPaySplits`) — unearned deposits whose
      `paysplit.UnearnedType` is a hidden definition (category 29 with a
      non-empty `ItemValue`, *not* `Definition.IsHidden`). Refunds can be
      excluded.
    * **Unearned income** (`FormRpUnearnedIncome`) — Line Item, Accounts,
      Allocation and Net tabs over unearned splits. The Allocation tab compares
      prepayments with remaining treatment-planned production; Net shows the
      unallocated remainder. Zero rows can be excluded per tab.
    * **Payment plan charge applications / refunds & transfers** — auditable report
      with pay date, patient, plan, charge, due date, debit type, amount, payment,
      unapplied plan amount, and note. A “Only refunds/transfers” filter isolates
      negative/refund splits and plan-credit resolutions (balanced zero-amount
      transfers). Notes include transfer direction (e.g. `Plan #X -> account/unallocated`)
      for audit clarity. The Account ledger has a “Show refunds/transfers only” filter
      and quick “Refund/transfer” action on plan-payment rows; plan summary also offers
      a pre-filled “Refund/transfer...” action with a transfer-target picker (unallocated
      patient account or specific procedure), and the selected target is included in the
      pre-filled note/audit text. Account ledger includes transfer-history filters
      (refund/transfer only, plan, target, optional date range), a clear-filters action,
      transfer-target tooltips, and “Export filtered ledger” (CSV/HTML, respecting current
      filters) with Copy CSV/Open export folder to locate/audit resolved credits without
      leaving Account. Record Payment
      supports explicit refund/transfer workflows
      with required notes. Unapplied plan credit can be resolved to
      patient account and, if clean, the plan can be closed; closing is blocked while
      due/future/unapplied amounts remain.
    * **Stale charge resolution and patient-scoped account audit** — dynamic plan
      charges that no longer match the expected schedule are previewed with their
      evidence before **Resolve stale charges (safe)** is allowed. Staff must enter a
      non-empty reason; safe charges are zeroed and annotated
      `[Stale resolved <as of>: <reason>]` rather than deleted, and the reason is
      written to the audit log. Account plan history shows resolved rows and their
      reasons on hover. **Export account audit** bundles that one patient's resolved
      charge reasons and matching security-log rows into a single CSV or HTML file
      (default `account-audit-patient-<PatNum>.csv`), so it can be filed against the
      patient/account record without associating two separate exports. A preview
      lists the exact rows before saving; **Refresh log** widens the in-memory log
      window, and CSV/HTML/Copy CSV/Open export folder actions mirror the other
      report exports. The chosen path is remembered per patient in `reports.txt`,
      so reopening the account returns to the same filing location. Security-log
      rows are included only when the audit-log feature and `AUDIT_TRAIL`
      permission are enabled; otherwise the bundle records that omission
      explicitly instead of silently dropping them. For practice-wide
      filing, the Payment plans report's **Filtered audit export** and the
      Setup/Security log's filtered export remain available and carry the same
      reason text.
    * **Overpaid procedures** (`FormRpProcOverpaid`) — completed procedures
      where patient payments plus insurance exceed the fee, with an option to
      keep only overpayments that involved insurance.
    * **Receivables breakdown** (`FormRpReceivablesBreakdown`) — patient
      balances aged 0-30/31-60/61-90/over-90 by billing type. The insurance
      estimate column appears only when **Insurance** is enabled, so the
      Ethiopia-default configuration sees patient receivables only.
    * **Service-date view** (`FormRpServiceDateView`) — charges, payments,
      insurance write-offs and the resulting balance per procedure service date.
    * **Procedure notes** (`FormRpProcNote`) — latest notes still containing
      `[Prompt:"…"]` or empty-quote placeholders, optionally adding same-day
      patients with no note and unsigned notes; group by procedure, patient or
      date+patient. D9986/D9987 broken codes are excluded by default.
    * **Dental sealant measure** (`FormRpDentalSealantMeasure`) — per-provider
      UDS numerator/denominator for children aged 6-9 in the reporting year,
      counting sealants on first molars (14, 3, 19, 30) and the listed
      CDT/medical/SNOMED codes.
    * **Discount plans** (`FormRpDiscountPlan`) — subscribers from OD's
      `discountplan`/`discountplansub` joined to `feesched` and `definition`
      names, with a description-substring filter. Plans and subscriptions are
      editable in Setup → Discount plans, so the grid reflects live edits.
    * **Raw public-health extracts** (`FormRpPHRawPop`, `FormRpPHRawScreen`) —
      fed from OD's `screen`, `screengroup`, `site`, `county` and `patientrace`
      tables. Race and grade values are emitted as their raw numeric codes.
      These support tables stay read-only in this port. `NumberBroken` counts
      broken appointments rather than OD's broken-appointment adjustment types.
    * **Overcharged payment plans** (`FormRpDPPOvercharged`) — dynamic plans
      whose debits exceed the patient's portion, reported per `payplanlink`
      row. Procedure portions use `ProcFee * GREATEST(1, BaseUnits + UnitQty)`
      plus procedure adjustments minus insurance, honour `AmountOverride`, and
      compare against debit `payplancharge`s grouped by `FKey`/`LinkType`.
      Insurance follows upstream's paid/estimated status sets exactly: paid
      claims add `InsPayAmt + WriteOff`, estimated ones add `InsPayEst` plus
      `WriteOffEstOverride`, `WriteOffEst` or zero (in that order, with `-1`
      meaning blank). Legacy plans without links keep the completed-amount
      approximation.
    * **Production goal** (`FormRpProdGoal`) — per-day production, scheduled
      value, adjustments and write-offs. The goal column comes from OD's
      `schedule`/`scheduleop` rows (open provider blocks only, overlapping
      operatory time de-duplicated) multiplied by
      `provider.HourlyProdGoalAmt`. Practice-level holidays and closed/blockout
      rows contribute zero hours, as upstream does. The goal is excluded from
      `Tot Prod`.
    * **Provider payroll** (`FormRpProviderPayroll`) — per-provider/day
      production, write-offs and patient/insurance income. Following upstream,
      `claimsnapshot.Writeoff` (with `-1` nullified to zero) is booked as a
      negative estimate on the procedure's completion date, and the difference
      between that estimate and `claimproc.WriteOff` plus insurance income is
      booked on `claimproc.DateSuppReceived`. Claim procs with no snapshot
      contribute a zero estimate, so past reports cannot be rewritten by later
      `claimproc` edits. Received/supplemental rows with a `ClaimPaymentNum`
      count as insurance income; rows with `ClaimPaymentNum = 0` go to
      "Ins Not Final", following upstream's two income tables.
    * **Presented treatment production** (`FormRpPresentedTreatmentProduction`)
      and **plan presentation statistics**
      (`FormRpTreatPlanPresentationStatistics`) — net production of completed
      procedures that appeared on a saved treatment plan, and planned /
      scheduled / completed counts and gross or net amounts per presenter.
      OD's separate `UserNumPresenter` is mapped to the plan's provider.
    * **Print-oriented reports** — routing slips (`FormRpRouting`), laser
      labels (`FormRpLaserLabels`), statements (`FormRpStatement`) and the
      debug-only print preview (`FormRpPrintPreview`). OD lays these out with
      absolute sheet positions; the port exports their data fields as a grid
      and uses the existing simplified PDF renderer for print. Laser labels
      have all four upstream tabs (patient, insurance company, custom and
      birthday); the insurance tab reads OD's `carrier` table in
      `CarrierName + Address + City` order with no `IsHidden` filter, exactly as
      upstream, and prints `Address2` as a second line when present. Its
      "Single Company" and "Range of Companies" modes are exposed: single
      matches `CarrierName + Address` and honours a copy count, range filters
      that name/address key between start and end carriers and sorts by the
      full upstream key. Empty range bounds mean "unbounded" in the port.
  * **Third upstream tranche (two further `FormRp*` ports, 51 report kinds
    total).** Both are pure builders in `od-core/src/reports_upstream.rs` with
    their own default-on feature toggle and saved options in `reports.txt`:
    * **Patient portion uncollected** (`FormRpPatPortionUncollected`) — completed
      procedures with a positive patient balance in the date range, with OD's
      `Date/Patient Name/Procedure/Fee/Patient/Adjustment/Patient Paid/Uncollected`
      columns ordered by `ProcDate`, `LName`, `FName`, `ProcCode`. This port
      follows upstream's netting exactly: `Fee` is reduced by the cap-complete
      write-off (claim status 7), `Adjustment` is `SUM(AdjAmt)` minus paysplits
      tied to an adjustment, and the insurance estimate uses `InsPayEst` for
      unreceived claims, `InsPayAmt` for received/supplemental ones and `WriteOff`
      for every status except 7 and 9. Hidden unearned paysplits are excluded,
      ordinary splits are not date-filtered, only rows with `Uncollected > 0.005`
      are kept, and there is no total row. When **Insurance & claims** is off,
      every insurance term is zero so the Ethiopia-default configuration sees
      patient payments only.
    * **Treatment finder** (`FormRpTreatmentFinder`) — a "who needs to be
      called" list of patients with treatment planned in the date range and a
      positive `AmtPlanned = ProcFee x (UnitQty + BaseUnits)`, grouped per patient.
      Patients with a future `Scheduled` appointment are left off by default
      (opt back in with **Include patients with a future appointment**), and OD's
      provider (patient primary or treating), billing type, clinic and code-range
      filters are ported. `Contact` falls back from email to `Hm:`/`Cell:`/`Wk:`
      phone because `PreferRecallMethod` is not modelled. The `Annual Max`,
      `Amt Used`, `Amt Pend`, `Amt Rem` and `Insurance Carrier` columns appear
      only when **Insurance & claims** is on; used/pending come from `ClaimProc`
      and the carrier from `Carrier`, but annual maximums need OD's `benefit`
      rows which this port does not model, so that column reads `0.00` and the
      "above amount" filter is inert (documented fidelity gap). `Procedure.DateTP`
      is not stored separately, so `ProcDate` is used as the treatment-plan date.
* Setup/Security: preferences, users, effective permissions, permission-code
  reference and the security log. The log has a case-insensitive filter across
  details, permission and user number, so a stale-charge resolution reason can
  be found without scanning raw rows. The filtered view can be exported as CSV
  or HTML; with a patient-number filter the default filename includes that
  number, ready to file against the patient/account record. User and audit views
  require the relevant Open Dental permission when enforcement is on.

## Not ported yet

The original contains roughly 32,000 methods, so the following remain to be
built once the foundation is validated: claim creation/submission and
adjudication, the remaining eServices transports (WhatsApp Cloud API, GSM
modem, Africa's Talking, SMS.to) and patient forms. Implemented reports are the
local slices (recall/ASAP, payment-plan balances, treatment-plan summaries)
plus 44 upstream `FormRp*` ports across four fidelity tranches — 51 report kinds in total
(the first tranche through Patients, plus the accounting, ledger, clinical,
production, plan, public-health, print-oriented and payment/unscheduled-treatment
reports listed above).
Fourteen upstream forms remain deliberately skipped: twelve insurance/claims-
bound reports (off by default for the Ethiopia domain, where patient payments
and payment plans are primary) and two Arizona-specific reports. The local
reminder slice is implemented, but OD's HQ/eConnector delivery path is
deliberately not ported.
Account/billing reads claims, writes
adjustments and payments with one or many paysplits, and generates printable
statements; it does not yet create or submit claims.
Only the first, most-used slice of these behaviours is represented here.

## Reference material

* Installed binary: `C:\Program Files (x86)\Open Dental\OpenDental.exe`
* Documentation: https://www.opendental.com/site/documentation.html
* Quick manual: https://www.opendental.com/site/quickmanual.html
* API specification: https://www.opendental.com/site/apispecification.html
* Upstream source: https://github.com/OpenDental/opendental (branch `24_3`)
* Column reference: `opendental-source/DocumentationBuilder/OpenDentalDocumentation.xml`
