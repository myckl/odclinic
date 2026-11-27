//! Pluggable delivery transports.
//!
//! SMTP email, the Telegram Bot API and Africa's Talking SMS are implemented
//! in this build. The other channel names remain in `TransportKind` and in the
//! settings file so a future build can add them without migrating the database
//! or the reminder rules. No transport here contacts Open Dental HQ.

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
pub struct AfricasTalkingChannel;

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

const AFRICAS_TALKING_URL: &str = "https://sms.africastalking.com/v1/messaging";

impl Transport for AfricasTalkingChannel {
    fn kind(&self) -> TransportKind {
        TransportKind::AfricasTalking
    }

    fn name(&self) -> &'static str {
        "Africa's Talking"
    }

    fn send(&self, message: &OutboxMessage, config: &EServicesConfig) -> Result<String, String> {
        // Credentials are checked before any network call, so an unconfigured
        // channel fails offline instead of contacting the provider.
        if config.africas_talking_username.trim().is_empty() {
            return Err("Africa's Talking username is not configured".into());
        }
        if config.africas_talking_key.trim().is_empty() {
            return Err("Africa's Talking API key is not configured".into());
        }
        if message.recipient.trim().is_empty() {
            return Err("No phone number is set for this patient".into());
        }
        let text = if message.subject.is_empty() {
            message.body.clone()
        } else {
            format!("{}\n\n{}", message.subject, message.body)
        };
        let body = format!(
            "username={}&to={}&message={}",
            form_encode(config.africas_talking_username.trim()),
            form_encode(message.recipient.trim()),
            form_encode(&text),
        );
        let response = match ureq::post(AFRICAS_TALKING_URL)
            .set("Api-Key", config.africas_talking_key.trim())
            .set("Content-Type", "application/x-www-form-urlencoded")
            .send_string(&body)
        {
            Ok(response) => response,
            // ureq 2 reports HTTP errors as `Error::Status` with the response
            // attached, so cover that path as well as an Ok status check.
            Err(ureq::Error::Status(status, response)) => {
                let response_body = response.into_string().unwrap_or_default();
                return Err(format!(
                    "Africa's Talking returned HTTP {status}: {}",
                    redact_secret(&response_body, &config.africas_talking_key)
                ));
            }
            Err(err) => {
                return Err(format!(
                    "Africa's Talking request failed: {}",
                    redact_secret(&err.to_string(), &config.africas_talking_key)
                ));
            }
        };
        let status = response.status();
        let response_body = response.into_string().unwrap_or_default();
        if status >= 400 {
            return Err(format!(
                "Africa's Talking returned HTTP {status}: {}",
                redact_secret(&response_body, &config.africas_talking_key)
            ));
        }
        africas_talking_receipt(&response_body, &config.africas_talking_key)
    }
}

/// Minimal `application/x-www-form-urlencoded` escaping: unreserved bytes stay
/// as-is and everything else is percent-encoded. Keeps `+251...` numbers and
/// multi-line messages intact without pulling in another dependency.
fn form_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// Provider text is echoed in receipts and errors; never leak the API key.
fn redact_secret(text: &str, secret: &str) -> String {
    if secret.is_empty() || !text.contains(secret) {
        return text.to_string();
    }
    text.replace(secret, "[redacted]")
}

fn json_id(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(text) if !text.is_empty() => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// Reads both Africa's Talking response shapes (`recipients[0]` and
/// top-level `status` plus `data.messageId`) into a short provider receipt.
fn africas_talking_receipt(body: &str, api_key: &str) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| "Africa's Talking returned an unreadable response".to_string())?;
    let recipient = value
        .get("recipients")
        .and_then(|rows| rows.as_array())
        .or_else(|| {
            value
                .get("SMSMessageData")
                .and_then(|data| data.get("Recipients"))
                .and_then(|rows| rows.as_array())
        })
        .and_then(|rows| rows.first());
    let status = recipient
        .and_then(|row| row.get("status").or_else(|| row.get("Status")))
        .and_then(|value| value.as_str())
        .or_else(|| value.get("status").and_then(|value| value.as_str()));
    let message_id = recipient
        .and_then(|row| json_id(row.get("messageId").or_else(|| row.get("MessageId"))))
        .or_else(|| {
            value.get("data").and_then(|data| {
                json_id(data.get("messageId").or_else(|| data.get("message_id")))
            })
        })
        .or_else(|| json_id(value.get("messageId")));
    let succeeded = status.is_some_and(|status| {
        matches!(
            status.to_ascii_lowercase().as_str(),
            "success" | "sent" | "queued" | "accepted"
        )
    }) || (status.is_none() && message_id.is_some());
    if !succeeded {
        let reason = status
            .map(|status| redact_secret(status, api_key))
            .unwrap_or_else(|| "unknown status".to_string());
        return Err(format!("Africa's Talking rejected the message: {reason}"));
    }
    match message_id {
        Some(id) => Ok(format!(
            "Africa's Talking message #{}",
            redact_secret(&id, api_key)
        )),
        None => Ok("Africa's Talking accepted the message (no receipt ID)".into()),
    }
}

/// The channels this build can actually call.
pub fn installed_transports() -> Vec<Box<dyn Transport>> {
    vec![
        Box::new(SmtpChannel),
        Box::new(TelegramChannel),
        Box::new(AfricasTalkingChannel),
    ]
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
    } else if message.transport == TransportKind::AfricasTalking {
        !config.africas_talking_username.is_empty() && !config.africas_talking_key.is_empty()
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

/// Explains a transport in the Setup screen. Planned channels say what they
/// would need; live channels say where to configure them.
pub fn planned_note(kind: TransportKind) -> &'static str {
    match kind {
        TransportKind::WhatsApp => "Meta Cloud API: 1,000 free service messages/month; needs a Meta Business account and a dedicated business number.",
        TransportKind::GsmModem => "A local USB GSM modem with an Ethio Telecom or Safaricom Ethiopia SIM works without internet.",
        TransportKind::AfricasTalking => "Live SMS: enter the Africa's Talking username and API key above, then tick the channel to enable it.",
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
    fn installed_channels_include_smtp_telegram_and_africas_talking() {
        let kinds: Vec<_> = installed_transports().iter().map(|t| t.kind()).collect();
        assert_eq!(
            kinds,
            vec![
                TransportKind::Smtp,
                TransportKind::Telegram,
                TransportKind::AfricasTalking
            ]
        );
    }

    #[test]
    fn africas_talking_without_credentials_fails_before_network() {
        let mut config = EServicesConfig::default();
        config.set_enabled(TransportKind::AfricasTalking, true);
        let message = message(TransportKind::AfricasTalking);
        assert_eq!(
            deliver(&message, &config).unwrap_err(),
            "Africa's Talking username is not configured"
        );
        config.africas_talking_username = "clinic".into();
        assert_eq!(
            deliver(&message, &config).unwrap_err(),
            "Africa's Talking API key is not configured"
        );
    }

    #[test]
    fn preview_does_not_need_a_configured_channel() {
        let preview = delivery_preview(&message(TransportKind::Telegram), &EServicesConfig::default());
        assert!(preview.starts_with("DRY RUN"));
        assert!(preview.contains("someone@example.test"));
        assert!(preview.contains("configured: no"));
    }

    #[test]
    fn africas_talking_preview_shows_configuration_without_secrets() {
        let message = message(TransportKind::AfricasTalking);
        let preview = delivery_preview(&message, &EServicesConfig::default());
        assert!(preview.starts_with("DRY RUN — Africa's Talking"));
        assert!(preview.contains("configured: no"));

        let mut config = EServicesConfig::default();
        config.africas_talking_username = "clinic".into();
        config.africas_talking_key = "TOPSECRETKEY".into();
        let preview = delivery_preview(&message, &config);
        assert!(preview.contains("configured: yes"));
        assert!(!preview.contains("TOPSECRETKEY"));
    }

    #[test]
    fn africas_talking_receipt_reads_both_response_shapes() {
        assert_eq!(
            africas_talking_receipt(
                r#"{"recipients":[{"status":"Success","messageId":"AT-1"}]}"#,
                "TOPSECRETKEY"
            )
            .unwrap(),
            "Africa's Talking message #AT-1"
        );
        assert_eq!(
            africas_talking_receipt(
                r#"{"SMSMessageData":{"Recipients":[{"status":"Sent","messageId":"AT-2"}]}}"#,
                "TOPSECRETKEY"
            )
            .unwrap(),
            "Africa's Talking message #AT-2"
        );
        assert_eq!(
            africas_talking_receipt(
                r#"{"status":"success","data":{"messageId":"AT-3"}}"#,
                "TOPSECRETKEY"
            )
            .unwrap(),
            "Africa's Talking message #AT-3"
        );
    }

    #[test]
    fn africas_talking_receipt_handles_success_without_id_and_failures() {
        let receipt =
            africas_talking_receipt(r#"{"recipients":[{"status":"Success"}]}"#, "TOPSECRETKEY")
                .unwrap();
        assert_eq!(receipt, "Africa's Talking accepted the message (no receipt ID)");

        let error =
            africas_talking_receipt(r#"{"recipients":[{"status":"Failed"}]}"#, "TOPSECRETKEY")
                .unwrap_err();
        assert!(error.contains("rejected"));
        assert!(!error.contains("TOPSECRETKEY"));

        let error = africas_talking_receipt(
            r#"{"recipients":[{"status":"TOPSECRETKEY"}]}"#,
            "TOPSECRETKEY",
        )
        .unwrap_err();
        assert!(!error.contains("TOPSECRETKEY"));

        assert!(africas_talking_receipt("not json", "TOPSECRETKEY")
            .unwrap_err()
            .contains("unreadable"));
    }

    #[test]
    fn africas_talking_form_encoding_escapes_phone_and_newlines() {
        let encoded = form_encode("+251 91 234 5678\n\nHello");
        assert!(encoded.starts_with("%2B251%2091"));
        assert!(encoded.contains("%0A%0A"));
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('\n'));
    }
}
