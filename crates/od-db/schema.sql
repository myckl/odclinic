-- Minimal Open Dental-compatible schema for the scoped Rust port.
-- Column names follow Open Dental 24.3 (see DocumentationBuilder/OpenDentalDocumentation.xml).
-- This is a subset: it is enough for the seven in-scope modules, not a full OD database.

CREATE TABLE IF NOT EXISTS clinic (
  ClinicNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  Description varchar(255) NOT NULL DEFAULT '',
  Abbr varchar(255) NOT NULL DEFAULT '',
  ItemOrder int NOT NULL DEFAULT 0,
  IsHidden tinyint NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS provider (
  ProvNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  Abbr varchar(255) NOT NULL DEFAULT '',
  LName varchar(255) NOT NULL DEFAULT '',
  FName varchar(255) NOT NULL DEFAULT '',
  IsHidden tinyint NOT NULL DEFAULT 0,
  ItemOrder int NOT NULL DEFAULT 0,
  HourlyProdGoalAmt double NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS operatory (
  OperatoryNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  OpName varchar(255) NOT NULL DEFAULT '',
  Abbrev varchar(255) NOT NULL DEFAULT '',
  ClinicNum bigint NOT NULL DEFAULT 0,
  ItemOrder int NOT NULL DEFAULT 0,
  IsHidden tinyint NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS patient (
  PatNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  LName varchar(255) NOT NULL DEFAULT '',
  FName varchar(255) NOT NULL DEFAULT '',
  MiddleI varchar(255) NOT NULL DEFAULT '',
  Preferred varchar(255) NOT NULL DEFAULT '',
  Title varchar(255) NOT NULL DEFAULT '',
  Birthdate date NOT NULL DEFAULT '0001-01-01',
  Gender tinyint NOT NULL DEFAULT 2,
  SSN varchar(255) NOT NULL DEFAULT '',
  Guarantor bigint NOT NULL DEFAULT 0,
  PriProv bigint NOT NULL DEFAULT 0,
  SecProv bigint NOT NULL DEFAULT 0,
  FeeSched bigint NOT NULL DEFAULT 0,
  ClinicNum bigint NOT NULL DEFAULT 0,
  HmPhone varchar(255) NOT NULL DEFAULT '',
  WkPhone varchar(255) NOT NULL DEFAULT '',
  WirelessPhone varchar(255) NOT NULL DEFAULT '',
  Email varchar(255) NOT NULL DEFAULT '',
  Address varchar(255) NOT NULL DEFAULT '',
  Address2 varchar(255) NOT NULL DEFAULT '',
  City varchar(255) NOT NULL DEFAULT '',
  State varchar(255) NOT NULL DEFAULT '',
  Zip varchar(255) NOT NULL DEFAULT '',
  EstBalance double NOT NULL DEFAULT 0,
  BalTotal double NOT NULL DEFAULT 0,
  BillingType bigint NOT NULL DEFAULT 0,
  ChartNumber varchar(255) NOT NULL DEFAULT '',
  MedicaidID varchar(255) NOT NULL DEFAULT '',
  PatStatus tinyint NOT NULL DEFAULT 0,
  DateTStamp timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  County varchar(255) NOT NULL DEFAULT '',
  SiteNum bigint NOT NULL DEFAULT 0,
  GradeLevel tinyint NOT NULL DEFAULT 0,
  Urgency tinyint NOT NULL DEFAULT 0,
  PlannedIsDone tinyint NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS appointment (
  AptNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  AptDateTime datetime NOT NULL,
  AptStatus tinyint NOT NULL DEFAULT 1,
  Priority tinyint NOT NULL DEFAULT 0,
  -- FK to definition.DefNum in DefCat.RecallUnschedStatus (OD's unscheduled
  -- list / waitlist status). Only used when AptStatus = 3.
  UnschedStatus bigint NOT NULL DEFAULT 0,
  Pattern varchar(255) NOT NULL DEFAULT '',
  Op bigint NOT NULL DEFAULT 0,
  ProvNum bigint NOT NULL DEFAULT 0,
  ProvHyg bigint NOT NULL DEFAULT 0,
  ClinicNum bigint NOT NULL DEFAULT 0,
  Note text NOT NULL,
  TimeLocked tinyint NOT NULL DEFAULT 0,
  IsNewPatient tinyint NOT NULL DEFAULT 0,
  -- FK to definition.DefNum in DefCat.ApptConfirmed (2). Zero = unconfirmed.
  Confirmed bigint NOT NULL DEFAULT 0,
  ProcDescript varchar(255) NOT NULL DEFAULT '',
  DateTimeArrived datetime NOT NULL DEFAULT '0001-01-01',
  DateTimeSeated datetime NOT NULL DEFAULT '0001-01-01',
  DateTimeDismissed datetime NOT NULL DEFAULT '0001-01-01',
  ItemOrderPlanned int NOT NULL DEFAULT 0,
  KEY (AptDateTime), KEY (PatNum), KEY (Op)
);

-- OD histappointment: a snapshot of the previous appointment state. This is
-- what makes a wrong time edit or a mistaken waitlist removal restorable.
-- Only the columns this port reads/writes are listed.
CREATE TABLE IF NOT EXISTS histappointment (
  HistApptNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  HistUserNum bigint NOT NULL DEFAULT 0,
  HistDateTStamp datetime NOT NULL,
  HistApptAction int NOT NULL DEFAULT 0,
  AptNum bigint NOT NULL DEFAULT 0,
  PatNum bigint NOT NULL DEFAULT 0,
  AptStatus tinyint NOT NULL DEFAULT 0,
  Pattern varchar(255) NOT NULL DEFAULT '',
  TimeLocked tinyint NOT NULL DEFAULT 0,
  Op bigint NOT NULL DEFAULT 0,
  Note varchar(255) NOT NULL DEFAULT '',
  ProvNum bigint NOT NULL DEFAULT 0,
  ClinicNum bigint NOT NULL DEFAULT 0,
  AptDateTime datetime NOT NULL DEFAULT '0001-01-01',
  UnschedStatus bigint NOT NULL DEFAULT 0,
  Priority tinyint NOT NULL DEFAULT 0,
  KEY (AptNum), KEY (HistDateTStamp)
);

-- Field names from Open Dental 24.3 Recall.cs. DateDueCalc and DatePrevious are
-- calculated by OD; DateDue is the user-editable field used by recall reports.
CREATE TABLE IF NOT EXISTS recall (
  RecallNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  DateDueCalc date NOT NULL DEFAULT '0001-01-01',
  DateDue date NOT NULL DEFAULT '0001-01-01',
  DatePrevious date NOT NULL DEFAULT '0001-01-01',
  RecallInterval int NOT NULL DEFAULT 0,
  RecallStatus bigint NOT NULL DEFAULT 0,
  Note text NOT NULL,
  IsDisabled tinyint NOT NULL DEFAULT 0,
  DateTStamp timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  RecallTypeNum bigint NOT NULL DEFAULT 0,
  DisableUntilBalance double NOT NULL DEFAULT 0,
  DisableUntilDate datetime NOT NULL DEFAULT '0001-01-01',
  DateScheduled datetime NOT NULL DEFAULT '0001-01-01',
  Priority tinyint NOT NULL DEFAULT 0,
  TimePatternOverride varchar(255) NOT NULL DEFAULT '',
  KEY (PatNum), KEY (DateDue)
);

CREATE TABLE IF NOT EXISTS procedurecode (
  CodeNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  ProcCode varchar(255) NOT NULL DEFAULT '',
  Descript varchar(255) NOT NULL DEFAULT '',
  AbbrDesc varchar(255) NOT NULL DEFAULT '',
  ProcTime varchar(255) NOT NULL DEFAULT '',
  ProcCat bigint NOT NULL DEFAULT 0,
  KEY (ProcCode)
);

-- Prescriptions (FormRpPrescriptions reads Patient/RxDate/Drug/Sig/Disp/Prov).
-- The remaining columns are OD's ePrescribe/bookkeeping fields and are written
-- with conventional zero values when this port inserts a row.
CREATE TABLE IF NOT EXISTS rxpat (
  RxNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  RxDate date NOT NULL DEFAULT '0000-00-00',
  Drug varchar(255) NOT NULL DEFAULT '',
  Sig varchar(255) NOT NULL DEFAULT '',
  Disp varchar(255) NOT NULL DEFAULT '',
  Refills varchar(30) NOT NULL DEFAULT '',
  ProvNum bigint NOT NULL DEFAULT 0,
  Notes varchar(255) NOT NULL DEFAULT '',
  PharmacyNum bigint NOT NULL DEFAULT 0,
  IsControlled tinyint NOT NULL DEFAULT 0,
  DateTStamp timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  SendStatus tinyint NOT NULL DEFAULT 0,
  RxCui bigint NOT NULL DEFAULT 0,
  DosageCode varchar(255) NOT NULL DEFAULT '',
  ErxGuid varchar(40) NOT NULL DEFAULT '',
  IsErxOld tinyint NOT NULL DEFAULT 0,
  ErxPharmacyInfo varchar(255) NOT NULL DEFAULT '',
  IsProcRequired tinyint NOT NULL DEFAULT 0,
  ProcNum bigint NOT NULL DEFAULT 0,
  DaysOfSupply double DEFAULT NULL,
  PatientInstruction text NOT NULL,
  ClinicNum bigint NOT NULL DEFAULT 0,
  UserNum bigint NOT NULL DEFAULT 0,
  RxType tinyint NOT NULL DEFAULT 0,
  KEY (PatNum), KEY (RxDate), KEY (Drug(100))
);

-- Referral analysis (FormRpReferralAnalysis reads referral + refattach). The
-- live OD schema has more bookkeeping columns; these are the ones this port
-- reads, with OD's names and defaults so a demo database stays compatible.
CREATE TABLE IF NOT EXISTS referral (
  ReferralNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  LName varchar(255) NOT NULL DEFAULT '',
  FName varchar(255) NOT NULL DEFAULT '',
  MName varchar(255) NOT NULL DEFAULT '',
  SSN varchar(9) NOT NULL DEFAULT '',
  UsingTIN tinyint NOT NULL DEFAULT 0,
  Specialty bigint NOT NULL DEFAULT 0,
  ST varchar(50) NOT NULL DEFAULT '',
  Telephone varchar(100) NOT NULL DEFAULT '',
  Address varchar(255) NOT NULL DEFAULT '',
  Address2 varchar(255) NOT NULL DEFAULT '',
  City varchar(100) NOT NULL DEFAULT '',
  Zip varchar(50) NOT NULL DEFAULT '',
  Note text,
  Phone2 varchar(100) NOT NULL DEFAULT '',
  IsHidden tinyint NOT NULL DEFAULT 0,
  NotPerson tinyint NOT NULL DEFAULT 0,
  Title varchar(100) NOT NULL DEFAULT '',
  EMail varchar(255) NOT NULL DEFAULT '',
  PatNum bigint NOT NULL DEFAULT 0,
  IsDoctor tinyint NOT NULL DEFAULT 0,
  DateTStamp timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  IsPreferred tinyint NOT NULL DEFAULT 0,
  BusinessName varchar(255) NOT NULL DEFAULT '',
  KEY (LName), KEY (Specialty)
);

CREATE TABLE IF NOT EXISTS refattach (
  RefAttachNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  ReferralNum bigint NOT NULL DEFAULT 0,
  PatNum bigint NOT NULL DEFAULT 0,
  ItemOrder int NOT NULL DEFAULT 0,
  RefDate date DEFAULT NULL,
  RefType tinyint NOT NULL DEFAULT 0,
  RefToStatus tinyint NOT NULL DEFAULT 0,
  Note text,
  IsTransitionOfCare tinyint NOT NULL DEFAULT 0,
  ProcNum bigint NOT NULL DEFAULT 0,
  ProvNum bigint NOT NULL DEFAULT 0,
  DateTStamp timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  KEY (ReferralNum), KEY (PatNum), KEY (RefType)
);

CREATE TABLE IF NOT EXISTS fee (
  FeeNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  Amount double NOT NULL DEFAULT 0,
  FeeSched bigint NOT NULL DEFAULT 0,
  CodeNum bigint NOT NULL DEFAULT 0,
  ClinicNum bigint NOT NULL DEFAULT 0,
  ProvNum bigint NOT NULL DEFAULT 0
);

-- Open Dental maps Procedure to the "procedurelog" table via [CrudTable].
CREATE TABLE IF NOT EXISTS procedurelog (
  ProcNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  AptNum bigint NOT NULL DEFAULT 0,
  ProvNum bigint NOT NULL DEFAULT 0,
  CodeNum bigint NOT NULL DEFAULT 0,
  ClinicNum bigint NOT NULL DEFAULT 0,
  ProcDate date NOT NULL,
  ProcFee double NOT NULL DEFAULT 0,
  ProcStatus tinyint NOT NULL DEFAULT 0,
  ToothNum varchar(255) NOT NULL DEFAULT '',
  Surf varchar(255) NOT NULL DEFAULT '',
  ToothRange varchar(255) NOT NULL DEFAULT '',
  MedicalCode varchar(255) NOT NULL DEFAULT '',
  SnomedBodySite varchar(255) NOT NULL DEFAULT '',
  UnitQty double NOT NULL DEFAULT 0,
  BaseUnits double NOT NULL DEFAULT 0,
  -- OD's Procedure.Insert sets DateComplete and stores the booked discount so
  -- GetDiscountPlanProc can total it without re-reading adjustments. OD 24_3
  -- declares DateComplete as a DATE; the store also parses a datetime when a
  -- non-OD schema supplies one.
  DateComplete date NOT NULL DEFAULT '0001-01-01',
  DiscountPlanAmt double NOT NULL DEFAULT 0,
  KEY (PatNum), KEY (ProcDate)
);

-- OD writes all of these NOT NULL bookkeeping columns on insert. The port
-- supplies zero values for the fields it does not expose in the UI.
CREATE TABLE IF NOT EXISTS treatplan (
  TreatPlanNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  DateTP date NOT NULL DEFAULT '0001-01-01',
  Heading varchar(255) NOT NULL DEFAULT '',
  Note text NOT NULL,
  Signature text NOT NULL,
  SigIsTopaz tinyint NOT NULL DEFAULT 0,
  ResponsParty bigint NOT NULL DEFAULT 0,
  DocNum bigint NOT NULL DEFAULT 0,
  TPStatus tinyint NOT NULL DEFAULT 0,
  SecUserNumEntry bigint NOT NULL DEFAULT 0,
  SecDateEntry date NOT NULL DEFAULT '0001-01-01',
  SecDateTEdit timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  UserNumPresenter bigint NOT NULL DEFAULT 0,
  TPType tinyint NOT NULL DEFAULT 0,
  SignaturePractice text NOT NULL,
  DateTSigned datetime NOT NULL DEFAULT '0001-01-01 00:00:00',
  DateTPracticeSigned datetime NOT NULL DEFAULT '0001-01-01 00:00:00',
  SignatureText varchar(255) NOT NULL DEFAULT '',
  SignaturePracticeText varchar(255) NOT NULL DEFAULT '',
  MobileAppDeviceNum bigint NOT NULL DEFAULT 0,
  KEY (PatNum)
);

-- Immutable copies of procedures on a Saved (TPStatus=0) plan.
CREATE TABLE IF NOT EXISTS proctp (
  ProcTPNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  TreatPlanNum bigint NOT NULL DEFAULT 0,
  PatNum bigint NOT NULL DEFAULT 0,
  ProcNumOrig bigint NOT NULL DEFAULT 0,
  ItemOrder int NOT NULL DEFAULT 0,
  Priority bigint NOT NULL DEFAULT 0,
  ToothNumTP varchar(255) NOT NULL DEFAULT '',
  Surf varchar(255) NOT NULL DEFAULT '',
  ProcCode varchar(15) DEFAULT NULL,
  Descript varchar(255) NOT NULL DEFAULT '',
  FeeAmt double NOT NULL DEFAULT 0,
  PriInsAmt double NOT NULL DEFAULT 0,
  SecInsAmt double NOT NULL DEFAULT 0,
  PatAmt double NOT NULL DEFAULT 0,
  Discount double NOT NULL DEFAULT 0,
  Prognosis text NOT NULL,
  Dx text NOT NULL,
  ProcAbbr varchar(255) NOT NULL DEFAULT '',
  SecUserNumEntry bigint NOT NULL DEFAULT 0,
  SecDateEntry date NOT NULL DEFAULT '0001-01-01',
  SecDateTEdit timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  FeeAllowed double NOT NULL DEFAULT 0,
  TaxAmt double NOT NULL DEFAULT 0,
  ProvNum bigint NOT NULL DEFAULT 0,
  DateTP date NOT NULL DEFAULT '0001-01-01',
  ClinicNum bigint NOT NULL DEFAULT 0,
  CatPercUCR double NOT NULL DEFAULT 0,
  KEY (TreatPlanNum)
);

-- Active/Inactive plans link directly to procedurelog rows instead of proctp.
CREATE TABLE IF NOT EXISTS treatplanattach (
  TreatPlanAttachNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  TreatPlanNum bigint NOT NULL DEFAULT 0,
  ProcNum bigint NOT NULL DEFAULT 0,
  Priority bigint NOT NULL DEFAULT 0,
  KEY (TreatPlanNum), KEY (ProcNum)
);

CREATE TABLE IF NOT EXISTS payment (
  PayNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  PayDate date NOT NULL,
  PayAmt double NOT NULL DEFAULT 0,
  PayType bigint NOT NULL DEFAULT 0,
  CheckNum varchar(255) NOT NULL DEFAULT '',
  PayNote text NOT NULL,
  ClinicNum bigint NOT NULL DEFAULT 0,
  KEY (PatNum)
);

CREATE TABLE IF NOT EXISTS paysplit (
  SplitNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  PayNum bigint NOT NULL DEFAULT 0,
  ProcNum bigint NOT NULL DEFAULT 0,
  SplitAmt double NOT NULL DEFAULT 0,
  DatePay date NOT NULL,
  ProcDate date NOT NULL DEFAULT '2000-01-01',
  ProvNum bigint NOT NULL DEFAULT 0,
  PayPlanNum bigint NOT NULL DEFAULT 0,
  AdjNum bigint NOT NULL DEFAULT 0,
  UnearnedType bigint NOT NULL DEFAULT 0,
  PayPlanChargeNum bigint NOT NULL DEFAULT 0,
  PayPlanDebitType int NOT NULL DEFAULT 0,
  KEY (PatNum), KEY (PayNum)
);

CREATE TABLE IF NOT EXISTS adjustment (
  AdjNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  AdjDate date NOT NULL,
  AdjAmt double NOT NULL DEFAULT 0,
  PatNum bigint NOT NULL DEFAULT 0,
  AdjType bigint NOT NULL DEFAULT 0,
  ProvNum bigint NOT NULL DEFAULT 0,
  AdjNote text NOT NULL,
  ProcDate date NOT NULL,
  ProcNum bigint NOT NULL DEFAULT 0,
  KEY (PatNum), KEY (ProcNum)
);

CREATE TABLE IF NOT EXISTS insplan (
  PlanNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  CarrierNum bigint NOT NULL DEFAULT 0,
  GroupName varchar(255) NOT NULL DEFAULT '',
  GroupNum varchar(255) NOT NULL DEFAULT '',
  FeeSched bigint NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS inssub (
  InsSubNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PlanNum bigint NOT NULL DEFAULT 0,
  Subscriber bigint NOT NULL DEFAULT 0,
  SubscriberID varchar(255) NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS patplan (
  PatPlanNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  InsSubNum bigint NOT NULL DEFAULT 0,
  Ordinal tinyint NOT NULL DEFAULT 1,
  KEY (PatNum)
);

CREATE TABLE IF NOT EXISTS claim (
  ClaimNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  PlanNum bigint NOT NULL DEFAULT 0,
  ClaimStatus varchar(255) NOT NULL DEFAULT '',
  DateSent date NOT NULL DEFAULT '0001-01-01',
  ClaimFee double NOT NULL DEFAULT 0,
  ClaimIdentifier varchar(255) NOT NULL DEFAULT '',
  KEY (PatNum)
);

CREATE TABLE IF NOT EXISTS claimproc (
  ClaimProcNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  ClaimNum bigint NOT NULL DEFAULT 0,
  ProcNum bigint NOT NULL DEFAULT 0,
  PlanNum bigint NOT NULL DEFAULT 0,
  Status tinyint NOT NULL DEFAULT 0,
  FeeBilled double NOT NULL DEFAULT 0,
  InsPayAmt double NOT NULL DEFAULT 0,
  CopayAmt double NOT NULL DEFAULT 0,
  WriteOff double NOT NULL DEFAULT 0,
  InsPayEst double NOT NULL DEFAULT 0,
  WriteOffEst double NOT NULL,
  WriteOffEstOverride double NOT NULL,
  DateSuppReceived date NOT NULL DEFAULT '0001-01-01',
  ClaimPaymentNum bigint NOT NULL DEFAULT 0,
  KEY (ClaimNum),
  KEY (DateSuppReceived)
);

CREATE TABLE IF NOT EXISTS statement (
  StatementNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  DateSent date NOT NULL DEFAULT '0001-01-01',
  Note text NOT NULL,
  IsInvoice tinyint NOT NULL DEFAULT 0,
  KEY (PatNum)
);

CREATE TABLE IF NOT EXISTS payplan (
  PayPlanNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  Guarantor bigint NOT NULL DEFAULT 0,
  PayPlanDate date NOT NULL,
  APR double NOT NULL DEFAULT 0,
  Note text NOT NULL,
  CompletedAmt double NOT NULL DEFAULT 0,
  PaySchedule tinyint NOT NULL DEFAULT 0,
  NumberOfPayments int NOT NULL DEFAULT 0,
  PayAmt double NOT NULL DEFAULT 0,
  DownPayment double NOT NULL DEFAULT 0,
  IsClosed tinyint NOT NULL DEFAULT 0,
  ChargeFrequency tinyint NOT NULL DEFAULT 3,
  DatePayPlanStart date NOT NULL DEFAULT '0001-01-01',
  DateInterestStart date DEFAULT NULL,
  IsDynamic tinyint NOT NULL DEFAULT 0,
  DynamicPayPlanTPOption tinyint NOT NULL DEFAULT 0,
  IsLocked tinyint NOT NULL DEFAULT 0,
  KEY (PatNum), KEY (Guarantor)
);

CREATE TABLE IF NOT EXISTS payplancharge (
  PayPlanChargeNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PayPlanNum bigint NOT NULL DEFAULT 0,
  Guarantor bigint NOT NULL DEFAULT 0,
  PatNum bigint NOT NULL DEFAULT 0,
  ChargeDate date NOT NULL,
  Principal double NOT NULL DEFAULT 0,
  Interest double NOT NULL DEFAULT 0,
  Note text NOT NULL,
  ProvNum bigint NOT NULL DEFAULT 0,
  ProcNum bigint NOT NULL DEFAULT 0,
  ChargeType tinyint NOT NULL DEFAULT 0,
  FKey bigint NOT NULL DEFAULT 0,
  LinkType tinyint NOT NULL DEFAULT 0,
  KEY (PayPlanNum), KEY (ChargeDate)
);

CREATE TABLE IF NOT EXISTS toothinitial (
  ToothInitialNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  ToothNum varchar(255) NOT NULL DEFAULT '',
  InitialType tinyint NOT NULL DEFAULT 0,
  Movement float NOT NULL DEFAULT 0,
  KEY (PatNum)
);

CREATE TABLE IF NOT EXISTS disease (
  DiseaseNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  DiseaseDefNum bigint NOT NULL DEFAULT 0,
  PatNote text NOT NULL,
  ProbStatus tinyint NOT NULL DEFAULT 0,
  DateStart date NOT NULL DEFAULT '0001-01-01',
  DateStop date NOT NULL DEFAULT '0001-01-01',
  KEY (PatNum)
);

CREATE TABLE IF NOT EXISTS diseasedef (
  DiseaseDefNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  DiseaseName varchar(255) NOT NULL DEFAULT '',
  ICD9Code varchar(64) NOT NULL DEFAULT '',
  Icd10Code varchar(64) NOT NULL DEFAULT '',
  SnomedCode varchar(64) NOT NULL DEFAULT '',
  ItemOrder int NOT NULL DEFAULT 0,
  IsHidden tinyint NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS patientnote (
  PatNum bigint NOT NULL PRIMARY KEY,
  Medical text NOT NULL,
  Treatment text NOT NULL,
  Service text NOT NULL,
  MedicalComp text NOT NULL,
  FamFinancial text NOT NULL
);

CREATE TABLE IF NOT EXISTS medicationpat (
  MedicationPatNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  MedicationNum bigint NOT NULL DEFAULT 0,
  DateStart date NOT NULL DEFAULT '0001-01-01',
  PatNote text NOT NULL,
  KEY (PatNum)
);

CREATE TABLE IF NOT EXISTS vitalsign (
  VitalsignNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  DateTaken date NOT NULL,
  Height float NOT NULL DEFAULT 0,
  Weight float NOT NULL DEFAULT 0,
  BpSystolic int NOT NULL DEFAULT 0,
  BpDiastolic int NOT NULL DEFAULT 0,
  Pulse int NOT NULL DEFAULT 0,
  KEY (PatNum)
);

CREATE TABLE IF NOT EXISTS usergroup (
  UserGroupNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  Description varchar(255) NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS userod (
  UserNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  UserName varchar(255) NOT NULL DEFAULT '',
  Password varchar(255) NOT NULL DEFAULT '',
  UserGroupNum bigint NOT NULL DEFAULT 0,
  EmployeeNum bigint NOT NULL DEFAULT 0,
  ClinicNum bigint NOT NULL DEFAULT 0,
  IsHidden tinyint NOT NULL DEFAULT 0
);

-- Multiple groups per user (the current OD security model).
CREATE TABLE IF NOT EXISTS usergroupattach (
  UserGroupAttachNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  UserNum bigint NOT NULL DEFAULT 0,
  UserGroupNum bigint NOT NULL DEFAULT 0,
  KEY (UserNum)
);

-- EnumPermType codes exceed 127, so PermType must be smallint.
CREATE TABLE IF NOT EXISTS grouppermission (
  GroupPermNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  UserGroupNum bigint NOT NULL DEFAULT 0,
  PermType smallint NOT NULL DEFAULT 0,
  FKey bigint NOT NULL DEFAULT 0,
  NewerDays int NOT NULL DEFAULT 0,
  KEY (UserGroupNum)
);

-- Append-only audit trail; LogDateTime is set by the server.
CREATE TABLE IF NOT EXISTS securitylog (
  SecurityLogNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PermType smallint NOT NULL DEFAULT 0,
  UserNum bigint NOT NULL DEFAULT 0,
  LogDateTime datetime NOT NULL DEFAULT CURRENT_TIMESTAMP,
  LogText text NOT NULL,
  PatNum bigint NOT NULL DEFAULT 0,
  FKey bigint NOT NULL DEFAULT 0,
  CompName varchar(255) NOT NULL DEFAULT '',
  LogSource tinyint NOT NULL DEFAULT 0,
  KEY (LogDateTime)
);

CREATE TABLE IF NOT EXISTS preference (
  PrefNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PrefName varchar(255) NOT NULL DEFAULT '',
  ValueString varchar(255) NOT NULL DEFAULT '',
  KEY (PrefName)
);

CREATE TABLE IF NOT EXISTS definition (
  DefNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  Category int NOT NULL DEFAULT 0,
  ItemOrder int NOT NULL DEFAULT 0,
  ItemName varchar(255) NOT NULL DEFAULT '',
  ItemValue varchar(255) NOT NULL DEFAULT '',
  ItemColor int NOT NULL DEFAULT 0,
  IsHidden tinyint NOT NULL DEFAULT 0,
  KEY (Category),
  KEY (Category, ItemOrder)
);

CREATE TABLE IF NOT EXISTS displayfield (
  DisplayFieldNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  InternalName varchar(255) NOT NULL DEFAULT '',
  ItemOrder int NOT NULL DEFAULT 0,
  Description varchar(255) NOT NULL DEFAULT '',
  ColumnWidth int NOT NULL DEFAULT 0,
  Category int NOT NULL DEFAULT 0,
  ChartViewNum bigint NOT NULL DEFAULT 0,
  PickList text NOT NULL,
  DescriptionOverride varchar(255) NOT NULL DEFAULT '',
  KEY (Category),
  KEY (ChartViewNum)
);

CREATE TABLE IF NOT EXISTS displayreport (
  DisplayReportNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  InternalName varchar(255) NOT NULL DEFAULT '',
  Description varchar(255) NOT NULL DEFAULT '',
  Category int NOT NULL DEFAULT 0,
  ItemOrder int NOT NULL DEFAULT 0,
  IsHidden tinyint NOT NULL DEFAULT 0
);

-- eServices rules and dedupe log. OD keeps the rules locally but sends through
-- HQ; this port uses the same tables for rules/dedupe and adds a local outbox.
CREATE TABLE IF NOT EXISTS apptreminderrule (
  ApptReminderRuleNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  TypeCur tinyint NOT NULL DEFAULT 0,
  TSPrior bigint NOT NULL DEFAULT 0,
  SendOrder varchar(255) NOT NULL DEFAULT '0,1,2',
  IsSendAll tinyint NOT NULL DEFAULT 0,
  TemplateSMS text NOT NULL,
  TemplateEmailSubject text NOT NULL,
  TemplateEmail text NOT NULL,
  ClinicNum bigint NOT NULL DEFAULT 0,
  DoNotSendWithin bigint NOT NULL DEFAULT 0,
  IsEnabled tinyint NOT NULL DEFAULT 1,
  Language varchar(255) NOT NULL DEFAULT '',
  DateTStamp timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS apptremindersent (
  ApptReminderSentNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  ClinicNum bigint NOT NULL DEFAULT 0,
  SendStatus tinyint NOT NULL DEFAULT 0,
  MessageType tinyint NOT NULL DEFAULT 0,
  MessageFk bigint NOT NULL DEFAULT 0,
  DateTimeEntry datetime NOT NULL DEFAULT CURRENT_TIMESTAMP,
  DateTimeSent datetime NOT NULL DEFAULT '0001-01-01',
  ResponseDescript text NOT NULL,
  ApptReminderRuleNum bigint NOT NULL DEFAULT 0,
  ApptNum bigint NOT NULL DEFAULT 0,
  ApptDateTime datetime NOT NULL DEFAULT '0001-01-01',
  TSPrior bigint NOT NULL DEFAULT 0,
  KEY (ApptReminderRuleNum,ApptNum), KEY (PatNum)
);

-- Port-local channel data and delivery queue (no OD equivalent: OD sends at HQ).
CREATE TABLE IF NOT EXISTS odclinic_patient_contact (
  PatNum bigint NOT NULL PRIMARY KEY,
  TelegramChatId varchar(255) NOT NULL DEFAULT '',
  EmailOverride varchar(255) NOT NULL DEFAULT '',
  OptOutEmail tinyint NOT NULL DEFAULT 0,
  OptOutTelegram tinyint NOT NULL DEFAULT 0
);

-- Port-local reminder dedupe. OD's apptremindersent is HQ-owned state where
-- SendStatus=1 means DoNotSend; writing generic dedupe rows there would silence
-- real OD reminders. Keep this port's delivery history out of OD's table.
CREATE TABLE IF NOT EXISTS odclinic_reminder_sent (
  SentNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  ApptReminderRuleNum bigint NOT NULL DEFAULT 0,
  ApptNum bigint NOT NULL DEFAULT 0,
  PatNum bigint NOT NULL DEFAULT 0,
  SentAt datetime NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE KEY (ApptReminderRuleNum, ApptNum), KEY (PatNum)
);

-- Port-local patient correction snapshots. OD has no patient-history table.
CREATE TABLE IF NOT EXISTS odclinic_patient_correction (
  CorrectionNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  CorrectedAt datetime NOT NULL,
  UserNum bigint NOT NULL DEFAULT 0,
  Reason varchar(500) NOT NULL DEFAULT '',
  BeforeData text NOT NULL,
  KEY (PatNum)
);

-- Port-local clinic cards. Issuing one never writes a statement, payment or
-- adjustment, so a card cannot create unwanted financial detail. Cards are
-- deactivated, never deleted, so the issuing history survives.
CREATE TABLE IF NOT EXISTS odclinic_clinic_card (
  CardNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  CardNumber varchar(64) NOT NULL DEFAULT '',
  IssuedAt date NOT NULL,
  IssuedBy bigint NOT NULL DEFAULT 0,
  Reason varchar(32) NOT NULL DEFAULT 'corporate_account',
  Authorization varchar(128) NOT NULL DEFAULT '',
  Note varchar(500) NOT NULL DEFAULT '',
  IsActive tinyint NOT NULL DEFAULT 1,
  KEY (PatNum)
);

-- Port-local appointment call log. OD commlog has no appointment FK, so the
-- confirmation/call history stays in this side table and never mutates OD rows.
CREATE TABLE IF NOT EXISTS odclinic_appointment_call_log (
  CallLogNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  AptNum bigint NOT NULL DEFAULT 0,
  PatNum bigint NOT NULL DEFAULT 0,
  CommDateTime datetime NOT NULL,
  CommType bigint NOT NULL DEFAULT 0,
  CommMode tinyint NOT NULL DEFAULT 0,
  SentOrReceived tinyint NOT NULL DEFAULT 0,
  Note text NOT NULL,
  UserNum bigint NOT NULL DEFAULT 0,
  KEY (AptNum), KEY (PatNum)
);

CREATE TABLE IF NOT EXISTS odclinic_message_outbox (
  MessageNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  AptNum bigint NOT NULL DEFAULT 0,
  RuleNum bigint NOT NULL DEFAULT 0,
  ReminderType tinyint NOT NULL DEFAULT 0,
  Transport tinyint NOT NULL DEFAULT 0,
  Recipient varchar(511) NOT NULL DEFAULT '',
  Subject text NOT NULL,
  Body text NOT NULL,
  Status tinyint NOT NULL DEFAULT 0,
  Attempts int NOT NULL DEFAULT 0,
  CreatedAt datetime NOT NULL DEFAULT CURRENT_TIMESTAMP,
  SentAt datetime NULL DEFAULT NULL,
  LastError text NOT NULL,
  KEY (Status), KEY (PatNum), KEY (AptNum)
);

-- Procedure notes (OD procnote), read by the Procedure Notes report.
CREATE TABLE IF NOT EXISTS procnote (
  ProcNoteNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  ProcNum bigint NOT NULL DEFAULT 0,
  EntryDateTime datetime NOT NULL,
  UserNum bigint NOT NULL DEFAULT 0,
  Note text NOT NULL,
  Signature text NOT NULL,
  KEY (ProcNum), KEY (PatNum)
);

-- Chart of accounts and double-entry journal, read by the three accounting
-- reports. AcctType: 0 Asset, 1 Liability, 2 Equity, 3 Income, 4 Expense.
CREATE TABLE IF NOT EXISTS account (
  AccountNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  Description varchar(255) NOT NULL DEFAULT '',
  AcctType tinyint NOT NULL DEFAULT 0,
  IsRetainedEarnings tinyint NOT NULL DEFAULT 0,
  Inactive tinyint NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS journalentry (
  JournalEntryNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  AccountNum bigint NOT NULL DEFAULT 0,
  DateDisplayed date NOT NULL,
  DebitAmt double NOT NULL DEFAULT 0,
  CreditAmt double NOT NULL DEFAULT 0,
  Memo text NOT NULL,
  Splits text NOT NULL,
  CheckNumber varchar(255) NOT NULL DEFAULT '',
  TransactionNum bigint NOT NULL DEFAULT 0,
  KEY (AccountNum), KEY (DateDisplayed)
);

-- ---------------------------------------------------------------------------
-- Report fidelity tranche 3: source tables for the production-goal, payment-
-- plan-link, claim-snapshot, discount-plan and public-health reports. These are
-- read-only in the port; the columns match OD 24.3/25.3.

CREATE TABLE IF NOT EXISTS schedule (
  ScheduleNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  SchedDate date NOT NULL,
  StartTime time NOT NULL,
  StopTime time NOT NULL,
  SchedType tinyint unsigned NOT NULL DEFAULT 0,
  ProvNum bigint NOT NULL DEFAULT 0,
  BlockoutType bigint NOT NULL DEFAULT 0,
  Note text NOT NULL,
  Status tinyint unsigned NOT NULL DEFAULT 0,
  EmployeeNum bigint NOT NULL DEFAULT 0,
  ClinicNum bigint NOT NULL DEFAULT 0,
  KEY (SchedDate), KEY (ProvNum)
);

CREATE TABLE IF NOT EXISTS scheduleop (
  ScheduleOpNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  ScheduleNum bigint NOT NULL DEFAULT 0,
  OperatoryNum bigint NOT NULL DEFAULT 0,
  KEY (ScheduleNum)
);

CREATE TABLE IF NOT EXISTS payplanlink (
  PayPlanLinkNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PayPlanNum bigint NOT NULL DEFAULT 0,
  LinkType tinyint NOT NULL DEFAULT 0,
  FKey bigint NOT NULL DEFAULT 0,
  AmountOverride double NOT NULL DEFAULT 0,
  KEY (PayPlanNum)
);

CREATE TABLE IF NOT EXISTS claimsnapshot (
  ClaimSnapshotNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  ProcNum bigint NOT NULL DEFAULT 0,
  ClaimType varchar(255) NOT NULL DEFAULT '',
  Writeoff double NOT NULL DEFAULT 0,
  InsPayEst double NOT NULL DEFAULT 0,
  Fee double NOT NULL DEFAULT 0,
  DateTEntry datetime NOT NULL,
  ClaimProcNum bigint NOT NULL DEFAULT 0,
  SnapshotTrigger tinyint NOT NULL DEFAULT 0,
  KEY (ProcNum), KEY (ClaimProcNum)
);

CREATE TABLE IF NOT EXISTS discountplan (
  DiscountPlanNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  Description varchar(255) NOT NULL DEFAULT '',
  FeeSchedNum bigint NOT NULL DEFAULT 0,
  DefNum bigint NOT NULL DEFAULT 0,
  IsHidden tinyint NOT NULL DEFAULT 0,
  PlanNote text NOT NULL,
  ExamFreqLimit int NOT NULL DEFAULT -1,
  XrayFreqLimit int NOT NULL DEFAULT -1,
  ProphyFreqLimit int NOT NULL DEFAULT -1,
  FluorideFreqLimit int NOT NULL DEFAULT -1,
  PerioFreqLimit int NOT NULL DEFAULT -1,
  LimitedExamFreqLimit int NOT NULL DEFAULT -1,
  PAFreqLimit int NOT NULL DEFAULT -1,
  AnnualMax double NOT NULL DEFAULT -1
);

CREATE TABLE IF NOT EXISTS discountplansub (
  DiscountSubNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  DiscountPlanNum bigint NOT NULL DEFAULT 0,
  PatNum bigint NOT NULL DEFAULT 0,
  DateEffective date DEFAULT NULL,
  DateTerm date DEFAULT NULL,
  SubNote text NOT NULL,
  KEY (DiscountPlanNum), KEY (PatNum)
);

CREATE TABLE IF NOT EXISTS screengroup (
  ScreenGroupNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  Description varchar(255) NOT NULL DEFAULT '',
  SGDate date DEFAULT NULL,
  ProvName varchar(255) NOT NULL DEFAULT '',
  ProvNum bigint NOT NULL DEFAULT 0,
  PlaceService tinyint NOT NULL DEFAULT 0,
  County varchar(255) NOT NULL DEFAULT '',
  GradeSchool varchar(255) NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS screen (
  ScreenNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  Gender tinyint unsigned NOT NULL DEFAULT 0,
  RaceOld tinyint NOT NULL DEFAULT 0,
  GradeLevel tinyint NOT NULL DEFAULT 0,
  Age tinyint unsigned NOT NULL DEFAULT 0,
  Urgency tinyint NOT NULL DEFAULT 0,
  HasCaries tinyint unsigned NOT NULL DEFAULT 0,
  NeedsSealants tinyint unsigned NOT NULL DEFAULT 0,
  CariesExperience tinyint unsigned NOT NULL DEFAULT 0,
  EarlyChildCaries tinyint unsigned NOT NULL DEFAULT 0,
  ExistingSealants tinyint unsigned NOT NULL DEFAULT 0,
  MissingAllTeeth tinyint unsigned NOT NULL DEFAULT 0,
  Birthdate date DEFAULT NULL,
  ScreenGroupNum bigint NOT NULL DEFAULT 0,
  ScreenGroupOrder smallint unsigned NOT NULL DEFAULT 0,
  Comments varchar(255) NOT NULL DEFAULT '',
  ScreenPatNum bigint NOT NULL DEFAULT 0,
  KEY (ScreenGroupNum)
);

CREATE TABLE IF NOT EXISTS site (
  SiteNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  Description varchar(255) NOT NULL DEFAULT '',
  Note text NOT NULL,
  Address varchar(100) NOT NULL DEFAULT '',
  Address2 varchar(100) NOT NULL DEFAULT '',
  City varchar(100) NOT NULL DEFAULT '',
  State varchar(100) NOT NULL DEFAULT '',
  Zip varchar(100) NOT NULL DEFAULT '',
  ProvNum bigint NOT NULL DEFAULT 0,
  PlaceService tinyint NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS county (
  CountyNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  CountyName varchar(255) NOT NULL DEFAULT '',
  CountyCode varchar(255) NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS patientrace (
  PatientRaceNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  PatNum bigint NOT NULL DEFAULT 0,
  Race tinyint NOT NULL DEFAULT 0,
  CdcrecCode varchar(255) NOT NULL DEFAULT '',
  KEY (PatNum)
);

CREATE TABLE IF NOT EXISTS carrier (
  CarrierNum bigint NOT NULL AUTO_INCREMENT PRIMARY KEY,
  CarrierName varchar(255) NOT NULL DEFAULT '',
  Address varchar(255) NOT NULL DEFAULT '',
  Address2 varchar(255) NOT NULL DEFAULT '',
  City varchar(255) NOT NULL DEFAULT '',
  State varchar(255) NOT NULL DEFAULT '',
  Zip varchar(255) NOT NULL DEFAULT '',
  Phone varchar(255) NOT NULL DEFAULT '',
  IsHidden tinyint NOT NULL DEFAULT 0
);
