//! Pluggable delivery transports.
//!
//! Only SMTP email and the Telegram Bot API are implemented in this build.
//! The other channel names remain in `TransportKind` and in the settings file
//! so a future build can add them without migrating the database or the
//! reminder rules. No transport here contacts Open Dental HQ.

use od_domain::eservices::{EServicesConfig, OutboxMessage, TransportKind};

use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::Tls;
use lettre::{Message, SmtpTransport, Transport as LettreTransport};

/// A channel that can deliver an outbox row.
pub trait Transport: Send + Sync {
    fn kind(&self) -> TransportKind;
    fn name(&self) -> &'static str;
    fn send(&self, message: &OutboxMessage, config: &EServicesConfig) -> Result<String, String>;
}

pub struct SmtpChannel;
pub struct TelegramChannel;

impl Transport for SmtpChannel {
    fn kind(&self) -> TransportKind {
        TransportKind::Smtp
    }

    fn name(&self) -> &'static str {
        "SMTP email"
    }

    fn send(&self, message: &OutboxMessage, config: &EServicesConfig) -> Result<String, String> {
        if config.smtp_host.trim().is_empty() {
            return Err("SMTP host is not configured".into());
        }
        if config.smtp_from.trim().is_empty() {
            return Err("SMTP from address is not configured".into());
        }
        let email = Message::builder()
            .from(
                config
                    .smtp_from
                    .parse()
                    .map_err(|e| format!("Invalid SMTP from address: {e}"))?,
            )
            .to(message
                .recipient
                .parse()
                .map_err(|e| format!("Invalid recipient '{}': {e}", message.recipient))?)
            .subject(&message.subject)
            .header(ContentType::TEXT_PLAIN)
            .body(message.body.clone())
            .map_err(|e| format!("Could not build email: {e}"))?;

        let mut builder = SmtpTransport::relay(&config.smtp_host)
            .map_err(|e| format!("Could not configure SMTP relay: {e}"))?
            .port(config.smtp_port);
        if !config.smtp_user.is_empty() || !config.smtp_password.is_empty() {
            builder = builder.credentials(Credentials::new(
                config.smtp_user.clone(),
                config.smtp_password.clone(),
            ));
        }
        if !config.smtp_use_tls {
            builder = builder.tls(Tls::None);
        }
        let mailer = builder.build();
        let response = mailer
            .send(&email)
            .map_err(|e| format!("SMTP send failed: {e}"))?;
        Ok(format!("SMTP accepted: {response:?}"))
    }
}

impl Transport for TelegramChannel {
    fn kind(&self) -> TransportKind {
        TransportKind::Telegram
    }

    fn name(&self) -> &'static str {
        "Telegram Bot API"
    }

    fn send(&self, message: &OutboxMessage, config: &EServicesConfig) -> Result<String, String> {
        if config.telegram_bot_token.trim().is_empty() {
            return Err("Telegram bot token is not configured".into());
        }
        if message.recipient.trim().is_empty() {
            return Err("No Telegram chat ID is set".into());
        }
        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage",
            config.telegram_bot_token
        );
        let text = if message.subject.is_empty() {
            message.body.clone()
        } else {
            format!("{}\n\n{}", message.subject, message.body)
        };
        let response = ureq::post(&url)
            .set("Content-Type", "application/json")
            .send_json(serde_json::json!({
                "chat_id": message.recipient,
                "text": text,
            }))
            .map_err(|e| format!("Telegram request failed: {e}"))?;
        let status = response.status();
        let body = response.into_string().unwrap_or_default();
        if status >= 400 {
            return Err(format!("Telegram returned HTTP {status}: {body}"));
        }
        let message_id = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("result")?.get("message_id")?.as_u64())
            .unwrap_or(0);
        Ok(format!("Telegram message #{message_id}"))
    }
}

/// The channels this build can actually call.
pub fn installed_transports() -> Vec<Box<dyn Transport>> {
    vec![Box::new(SmtpChannel), Box::new(TelegramChannel)]
}

/// Deliver one outbox row using the channel recorded on it. Returns a short
/// provider receipt suitable for `securitylog` / the outbox `last_error` field.
pub fn deliver(message: &OutboxMessage, config: &EServicesConfig) -> Result<String, String> {
    if !message.transport.implemented() {
        return Err(format!(
            "{} is a planned option and is not implemented in this build",
            message.transport.label()
        ));
    }
    if !config.is_enabled(message.transport) {
        return Err(format!(
            "{} is not enabled in eServices settings",
            message.transport.label()
        ));
    }
    let channel = installed_transports()
        .into_iter()
        .find(|t| t.kind() == message.transport)
        .ok_or_else(|| "Transport is unavailable".to_string())?;
    channel.send(message, config)
}

/// A safe preview used by dry-run mode and the Setup screen.
pub fn delivery_preview(message: &OutboxMessage, config: &EServicesConfig) -> String {
    let name = match message.transport {
        TransportKind::Smtp => "SMTP email",
        TransportKind::Telegram => "Telegram",
        _ => message.transport.label(),
    };
    let configured = if message.transport == TransportKind::Smtp {
        !config.smtp_host.is_empty()
    } else if message.transport == TransportKind::Telegram {
        !config.telegram_bot_token.is_empty()
    } else {
        false
    };
    format!(
        "DRY RUN — {name} (configured: {}) to {}\nSubject: {}\n{}",
        if configured { "yes" } else { "no" },
        message.recipient,
        message.subject,
        message.body
    )
}

/// Explains the channels that are selectable but not yet wired up.
pub fn planned_note(kind: TransportKind) -> &'static str {
    match kind {
        TransportKind::WhatsApp => "Meta Cloud API: 1,000 free service messages/month; needs a Meta Business account and a dedicated business number.",
        TransportKind::GsmModem => "A local USB GSM modem with an Ethio Telecom or Safaricom Ethiopia SIM works without internet.",
        TransportKind::AfricasTalking => "Bulk SMS is live in Ethiopia. Pricing must be confirmed with the provider; a test sandbox is available.",
        TransportKind::SmsTo => "Pay-as-you-go SMS fallback. Confirm the current Ethiopia rate after signing in.",
        TransportKind::Smtp | TransportKind::Telegram => "Implemented in this build.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_domain::eservices::{OutboxStatus, ReminderType};

    fn message(kind: TransportKind) -> OutboxMessage {
        OutboxMessage {
            transport: kind,
            recipient: "someone@example.test".into(),
            subject: "Reminder".into(),
            body: "See you soon".into(),
            status: OutboxStatus::Pending,
            reminder_type: ReminderType::Reminder,
            created_at: chrono::DateTime::from_timestamp(0, 0)
                .unwrap()
                .naive_utc(),
            ..Default::default()
        }
    }

    #[test]
    fn deliver_rejects_planned_and_disabled_channels_without_network() {
        let config = EServicesConfig::default();
        assert!(deliver(&message(TransportKind::WhatsApp), &config)
            .unwrap_err()
            .contains("planned option"));
        assert!(deliver(&message(TransportKind::Smtp), &config)
            .unwrap_err()
            .contains("not enabled"));
    }

    #[test]
    fn installed_channels_are_only_smtp_and_telegram() {
        let kinds: Vec<_> = installed_transports().iter().map(|t| t.kind()).collect();
        assert_eq!(kinds, vec![TransportKind::Smtp, TransportKind::Telegram]);
    }

    #[test]
    fn preview_does_not_need_a_configured_channel() {
        let preview = delivery_preview(&message(TransportKind::Telegram), &EServicesConfig::default());
        assert!(preview.starts_with("DRY RUN"));
        assert!(preview.contains("someone@example.test"));
        assert!(preview.contains("configured: no"));
    }
}
