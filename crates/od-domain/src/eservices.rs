//! Local eServices: reminder rules, the dedupe log and the delivery outbox.
//!
//! Open Dental keeps the *rules* in local tables (`apptreminderrule`) but
//! performs the actual sending on its HQ servers. The shipped 25.3 assembly
//! confirms this: `EServiceSignal` is documented as a workstation-to-HQ
//! communication, `WebServiceMainHQProxy` posts setup payloads and registration
//! keys, and the `AutoComm` objects carry HQ-generated IDs and message FKs.
//!
//! This port keeps an OD-compatible rule shape so the data is familiar, but
//! replaces the whole HQ path with a local outbox and pluggable transports.
//! No registration key and no Open Dental server are involved.

use std::path::PathBuf;

use chrono::NaiveDateTime;

use crate::Id;

/// OD `ApptReminderType`. Values were taken from the 24.3 C# source and are
/// still present in the 25.3 assembly.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(i8)]
pub enum ReminderType {
    #[default]
    Reminder = 0,
    ConfirmationFutureDay = 1,
    Arrival = 5,
    Birthday = 6,
    GeneralMessage = 7,
    WebSchedRecall = 8,
}

impl ReminderType {
    pub const ALL: [Self; 6] = [
        Self::Reminder,
        Self::ConfirmationFutureDay,
        Self::Arrival,
        Self::Birthday,
        Self::GeneralMessage,
        Self::WebSchedRecall,
    ];

    pub fn from_od(value: i8) -> Self {
        match value {
            1 => Self::ConfirmationFutureDay,
            5 => Self::Arrival,
            6 => Self::Birthday,
            7 => Self::GeneralMessage,
            8 => Self::WebSchedRecall,
            _ => Self::Reminder,
        }
    }

    pub fn as_od(self) -> i8 {
        self as i8
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Reminder => "Appointment reminder",
            Self::ConfirmationFutureDay => "Future-day confirmation",
            Self::Arrival => "Arrival message",
            Self::Birthday => "Birthday message",
            Self::GeneralMessage => "General message",
            Self::WebSchedRecall => "WebSched recall",
        }
    }
}

/// OD `CommType`: how a rule prefers to reach the patient.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(i8)]
pub enum CommType {
    #[default]
    Preferred = 0,
    Text = 1,
    Email = 2,
    SecureEmail = 3,
}

impl CommType {
    pub const ALL: [Self; 4] = [Self::Preferred, Self::Text, Self::Email, Self::SecureEmail];

    pub fn from_od(value: i8) -> Self {
        match value {
            1 => Self::Text,
            2 => Self::Email,
            3 => Self::SecureEmail,
            _ => Self::Preferred,
        }
    }

    pub fn as_od(self) -> i8 {
        self as i8
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Preferred => "Preferred channel",
            Self::Text => "Text message",
            Self::Email => "Email",
            Self::SecureEmail => "Secure email",
        }
    }
}

/// A delivery channel. SMTP, Telegram and Africa's Talking are implemented in
/// this build; the rest are kept as first-class settings so they can be wired
/// up later without changing the rule engine, database layer or GUI.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(i8)]
pub enum TransportKind {
    #[default]
    Smtp = 0,
    Telegram = 1,
    WhatsApp = 2,
    GsmModem = 3,
    AfricasTalking = 4,
    SmsTo = 5,
}

impl TransportKind {
    pub const ALL: [Self; 6] = [
        Self::Smtp,
        Self::Telegram,
        Self::WhatsApp,
        Self::GsmModem,
        Self::AfricasTalking,
        Self::SmsTo,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Smtp => "smtp",
            Self::Telegram => "telegram",
            Self::WhatsApp => "whatsapp",
            Self::GsmModem => "gsm",
            Self::AfricasTalking => "africastalking",
            Self::SmsTo => "smsto",
        }
    }

    pub fn from_config_str(value: &str) -> Option<Self> {
        let trimmed = value.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|k| k.as_str() == trimmed)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Smtp => "SMTP email",
            Self::Telegram => "Telegram Bot API",
            Self::WhatsApp => "WhatsApp Cloud API",
            Self::GsmModem => "Local GSM modem",
            Self::AfricasTalking => "Africa's Talking",
            Self::SmsTo => "SMS.to",
        }
    }

    /// Only these transports can actually deliver in this build.
    pub fn implemented(self) -> bool {
        matches!(self, Self::Smtp | Self::Telegram | Self::AfricasTalking)
    }

    pub fn is_email(self) -> bool {
        matches!(self, Self::Smtp)
    }
}

/// OD `apptreminderrule`. `TSPrior` and `DoNotSendWithin` are .NET `TimeSpan`
/// values, stored as 100-nanosecond ticks.
#[derive(Clone, Debug)]
pub struct ReminderRule {
    pub rule_num: Id,
    pub type_cur: ReminderType,
    /// Minutes before the appointment (converted to/from OD ticks at the DB
    /// boundary). Kept as minutes here so the rule engine stays readable.
    pub minutes_prior: i32,
    /// OD `DoNotSendWithin`, in minutes. A candidate is skipped when the
    /// appointment is closer than this.
    pub do_not_send_within_minutes: i32,
    pub send_order: String,
    pub is_enabled: bool,
    pub comm_type: CommType,
    pub sms_template: String,
    pub email_subject: String,
    pub email_body: String,
}

impl Default for ReminderRule {
    fn default() -> Self {
        Self {
            rule_num: 0,
            type_cur: ReminderType::Reminder,
            minutes_prior: 1440,
            do_not_send_within_minutes: 60,
            send_order: "0,1,2".into(),
            is_enabled: true,
            comm_type: CommType::Preferred,
            sms_template: String::new(),
            email_subject: String::new(),
            email_body: String::new(),
        }
    }
}

/// Per-patient channel data that OD does not store in `patient`. Kept in a
/// small local table so the MySQL backend never has to alter an OD table for
/// Telegram chat IDs or opt-outs.
#[derive(Clone, Debug, Default)]
pub struct PatientContact {
    pub pat_num: Id,
    pub telegram_chat_id: String,
    /// Empty means "use patient.Email".
    pub email_override: String,
    pub opt_out_email: bool,
    pub opt_out_telegram: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(i8)]
pub enum OutboxStatus {
    #[default]
    Pending = 0,
    Sent = 1,
    Failed = 2,
    /// No transport was configured or the patient opted out.
    Skipped = 3,
    /// Rendered but not delivered because dry-run is on.
    DryRun = 4,
}

impl OutboxStatus {
    pub fn from_od(value: i8) -> Self {
        match value {
            1 => Self::Sent,
            2 => Self::Failed,
            3 => Self::Skipped,
            4 => Self::DryRun,
            _ => Self::Pending,
        }
    }

    pub fn as_od(self) -> i8 {
        self as i8
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Sent => "Sent",
            Self::Failed => "Failed",
            Self::Skipped => "Skipped",
            Self::DryRun => "Dry run",
        }
    }
}

/// One local delivery attempt. This is the port's replacement for OD's HQ-side
/// AutoComm records, so all message IDs are generated locally.
#[derive(Clone, Debug, Default)]
pub struct OutboxMessage {
    pub message_num: Id,
    pub pat_num: Id,
    pub apt_num: Id,
    pub rule_num: Id,
    pub reminder_type: ReminderType,
    pub transport: TransportKind,
    pub recipient: String,
    pub subject: String,
    pub body: String,
    pub status: OutboxStatus,
    pub attempts: u32,
    pub created_at: NaiveDateTime,
    pub sent_at: Option<NaiveDateTime>,
    pub last_error: String,
}

/// Credentials and endpoints for every known transport. Unimplemented
/// transports still get config keys so their details can be entered ahead of
/// time and picked up by a future build without a settings-file migration.
#[derive(Clone, Debug, PartialEq)]
pub struct EServicesConfig {
    pub enabled_transports: Vec<TransportKind>,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: String,
    pub smtp_password: String,
    pub smtp_from: String,
    /// `true` uses STARTTLS; `false` is for a local relay on port 25/587.
    pub smtp_use_tls: bool,
    pub telegram_bot_token: String,
    /// Default/staff chat. A patient-level Telegram chat ID takes precedence.
    pub telegram_chat_id: String,
    pub whatsapp_token: String,
    pub whatsapp_phone_id: String,
    pub gsm_device: String,
    pub africas_talking_username: String,
    pub africas_talking_key: String,
    pub sms_to_key: String,
}

impl Default for EServicesConfig {
    fn default() -> Self {
        Self {
            enabled_transports: Vec::new(),
            smtp_host: String::new(),
            smtp_port: 587,
            smtp_user: String::new(),
            smtp_password: String::new(),
            smtp_from: String::new(),
            smtp_use_tls: true,
            telegram_bot_token: String::new(),
            telegram_chat_id: String::new(),
            whatsapp_token: String::new(),
            whatsapp_phone_id: String::new(),
            gsm_device: String::new(),
            africas_talking_username: String::new(),
            africas_talking_key: String::new(),
            sms_to_key: String::new(),
        }
    }
}

impl EServicesConfig {
    pub fn is_enabled(&self, kind: TransportKind) -> bool {
        self.enabled_transports.contains(&kind)
    }

    pub fn set_enabled(&mut self, kind: TransportKind, enabled: bool) {
        if enabled && !self.is_enabled(kind) {
            self.enabled_transports.push(kind);
        } else if !enabled {
            self.enabled_transports.retain(|k| *k != kind);
        }
    }

    /// What the Setup screen shows next to each transport.
    pub fn readiness(&self, kind: TransportKind) -> TransportReadiness {
        if !kind.implemented() {
            return TransportReadiness::Planned;
        }
        if !self.is_enabled(kind) {
            return TransportReadiness::Disabled;
        }
        let ready = match kind {
            TransportKind::Smtp => {
                !self.smtp_host.is_empty() && !self.smtp_from.is_empty()
            }
            TransportKind::Telegram => {
                !self.telegram_bot_token.is_empty() && !self.telegram_chat_id.is_empty()
            }
            TransportKind::AfricasTalking => {
                !self.africas_talking_username.is_empty() && !self.africas_talking_key.is_empty()
            }
            _ => false,
        };
        if ready {
            TransportReadiness::Ready
        } else {
            TransportReadiness::NeedsConfig
        }
    }

    pub fn to_text(&self) -> String {
        let transports = self
            .enabled_transports
            .iter()
            .map(|k| k.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let lines = vec![
            format!("enabled_transports={transports}"),
            format!("smtp_host={}", self.smtp_host),
            format!("smtp_port={}", self.smtp_port),
            format!("smtp_user={}", self.smtp_user),
            format!("smtp_password={}", self.smtp_password),
            format!("smtp_from={}", self.smtp_from),
            format!("smtp_use_tls={}", self.smtp_use_tls),
            format!("telegram_bot_token={}", self.telegram_bot_token),
            format!("telegram_chat_id={}", self.telegram_chat_id),
            format!("whatsapp_token={}", self.whatsapp_token),
            format!("whatsapp_phone_id={}", self.whatsapp_phone_id),
            format!("gsm_device={}", self.gsm_device),
            format!("africas_talking_username={}", self.africas_talking_username),
            format!("africas_talking_key={}", self.africas_talking_key),
            format!("sms_to_key={}", self.sms_to_key),
        ];
        format!("{}\n", lines.join("\n"))
    }

    pub fn from_text(text: &str) -> Self {
        let mut config = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "enabled_transports" => {
                    config.enabled_transports = value
                        .split(',')
                        .filter_map(TransportKind::from_config_str)
                        .collect();
                }
                "smtp_host" => config.smtp_host = value.into(),
                "smtp_port" => {
                    config.smtp_port = value.parse().unwrap_or(config.smtp_port);
                }
                "smtp_user" => config.smtp_user = value.into(),
                "smtp_password" => config.smtp_password = value.into(),
                "smtp_from" => config.smtp_from = value.into(),
                "smtp_use_tls" => {
                    config.smtp_use_tls = matches!(value, "1" | "true" | "True" | "TRUE");
                }
                "telegram_bot_token" => config.telegram_bot_token = value.into(),
                "telegram_chat_id" => config.telegram_chat_id = value.into(),
                "whatsapp_token" => config.whatsapp_token = value.into(),
                "whatsapp_phone_id" => config.whatsapp_phone_id = value.into(),
                "gsm_device" => config.gsm_device = value.into(),
                "africas_talking_username" => config.africas_talking_username = value.into(),
                "africas_talking_key" => config.africas_talking_key = value.into(),
                "sms_to_key" => config.sms_to_key = value.into(),
                _ => {}
            }
        }
        config
    }

    pub fn load() -> Self {
        match config_path("eservices.txt") {
            Some(path) => std::fs::read_to_string(path)
                .map(|text| Self::from_text(&text))
                .unwrap_or_default(),
            None => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = config_path("eservices.txt")
            .ok_or_else(|| "Could not find a settings folder".to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Could not create settings folder: {e}"))?;
        }
        std::fs::write(path, self.to_text())
            .map_err(|e| format!("Could not save eServices settings: {e}"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportReadiness {
    Ready,
    NeedsConfig,
    Disabled,
    Planned,
}

impl TransportReadiness {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::NeedsConfig => "Needs configuration",
            Self::Disabled => "Off",
            Self::Planned => "Available as an option (not in this build)",
        }
    }
}

fn config_path(name: &str) -> Option<PathBuf> {
    if let Ok(appdata) = std::env::var("APPDATA") {
        if !appdata.trim().is_empty() {
            return Some(PathBuf::from(appdata).join("odclinic").join(name));
        }
    }
    std::env::current_dir()
        .ok()
        .map(|dir| dir.join(format!("odclinic-{name}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn od_enum_values_match_source() {
        assert_eq!(ReminderType::Reminder.as_od(), 0);
        assert_eq!(ReminderType::ConfirmationFutureDay.as_od(), 1);
        assert_eq!(ReminderType::Arrival.as_od(), 5);
        assert_eq!(ReminderType::GeneralMessage.as_od(), 7);
        assert_eq!(ReminderType::from_od(8), ReminderType::WebSchedRecall);
        assert_eq!(CommType::Preferred.as_od(), 0);
        assert_eq!(CommType::from_od(3), CommType::SecureEmail);
        assert_eq!(OutboxStatus::from_od(4), OutboxStatus::DryRun);
    }

    #[test]
    fn transport_kind_tokens_round_trip_and_implemented_set() {
        assert_eq!(TransportKind::AfricasTalking as i8, 4);
        for kind in TransportKind::ALL {
            assert_eq!(TransportKind::from_config_str(kind.as_str()), Some(kind));
            assert_eq!(
                TransportKind::from_config_str(&kind.as_str().to_ascii_uppercase()),
                Some(kind)
            );
        }
        for kind in [
            TransportKind::Smtp,
            TransportKind::Telegram,
            TransportKind::AfricasTalking,
        ] {
            assert!(kind.implemented(), "{} should be implemented", kind.label());
        }
        for kind in [
            TransportKind::WhatsApp,
            TransportKind::GsmModem,
            TransportKind::SmsTo,
        ] {
            assert!(!kind.implemented(), "{} should still be planned", kind.label());
        }
    }

    #[test]
    fn config_round_trips_all_transport_options() {
        let mut config = EServicesConfig::default();
        for kind in TransportKind::ALL {
            config.set_enabled(kind, true);
        }
        config.smtp_host = "smtp.example.test".into();
        config.smtp_port = 2525;
        config.telegram_bot_token = "123:abc".into();
        config.africas_talking_username = "clinic".into();
        config.africas_talking_key = "test-key".into();
        let text = config.to_text();
        let loaded = EServicesConfig::from_text(&text);
        assert_eq!(loaded, config);
        assert_eq!(loaded.enabled_transports.len(), 6);
        assert_eq!(loaded.africas_talking_username, "clinic");
        assert_eq!(loaded.africas_talking_key, "test-key");
        assert_eq!(loaded.readiness(TransportKind::Smtp), TransportReadiness::NeedsConfig);
        assert_eq!(
            loaded.readiness(TransportKind::AfricasTalking),
            TransportReadiness::Ready
        );
        assert_eq!(loaded.readiness(TransportKind::WhatsApp), TransportReadiness::Planned);
    }

    #[test]
    fn africas_talking_readiness_requires_both_credentials() {
        let mut config = EServicesConfig::default();
        config.set_enabled(TransportKind::AfricasTalking, true);
        assert_eq!(
            config.readiness(TransportKind::AfricasTalking),
            TransportReadiness::NeedsConfig
        );
        config.africas_talking_username = "clinic".into();
        assert_eq!(
            config.readiness(TransportKind::AfricasTalking),
            TransportReadiness::NeedsConfig
        );
        config.africas_talking_key = "test-key".into();
        assert_eq!(
            config.readiness(TransportKind::AfricasTalking),
            TransportReadiness::Ready
        );
        config.set_enabled(TransportKind::AfricasTalking, false);
        assert_eq!(
            config.readiness(TransportKind::AfricasTalking),
            TransportReadiness::Disabled
        );
    }

    #[test]
    fn only_implemented_transports_can_be_ready() {
        let mut config = EServicesConfig::default();
        config.set_enabled(TransportKind::Smtp, true);
        config.smtp_host = "localhost".into();
        config.smtp_from = "clinic@example.test".into();
        assert_eq!(config.readiness(TransportKind::Smtp), TransportReadiness::Ready);
        config.set_enabled(TransportKind::Smtp, false);
        assert_eq!(config.readiness(TransportKind::Smtp), TransportReadiness::Disabled);
    }
}
