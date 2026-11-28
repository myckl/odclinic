//! Local reminder rule engine.
//!
//! Mirrors the shape of OD's `apptreminderrule` / `apptremindersent` but runs
//! entirely on this machine. The HQ-side AutoComm path in Open Dental is
//! replaced by the local outbox in `od-db`.

use chrono::{Datelike, NaiveDateTime};
use od_domain::eservices::*;
use od_domain::{
    Appointment, AppointmentStatus, Clinic, Id, Operatory, Patient, Procedure, ProcedureStatus,
    Provider,
};

/// Everything the rule engine needs, gathered once by the caller.
pub struct ReminderContext<'a> {
    pub rules: &'a [ReminderRule],
    pub appointments: &'a [Appointment],
    /// Completed procedures are the time anchor for OD GeneralMessage
    /// post-treatment follow-ups.
    pub procedures: &'a [Procedure],
    pub patients: &'a [Patient],
    pub contacts: &'a [PatientContact],
    pub providers: &'a [Provider],
    pub clinics: &'a [Clinic],
    pub operatories: &'a [Operatory],
    pub config: &'a EServicesConfig,
    pub now: NaiveDateTime,
    /// Rule types enabled by the corresponding feature toggles.
    pub allow_types: &'a [ReminderType],
    /// When true, generated rows are marked DryRun instead of Pending.
    pub dry_run: bool,
}

/// Replace OD-style `{Token}` placeholders with patient and appointment data.
pub fn render_template(
    template: &str,
    patient: &Patient,
    appointment: &Appointment,
    providers: &[Provider],
    clinics: &[Clinic],
    operatories: &[Operatory],
    now: NaiveDateTime,
) -> String {
    let provider = providers
        .iter()
        .find(|p| p.provider_num == appointment.provider_num)
        .map(|p| format!("Dr. {}", p.l_name))
        .unwrap_or_else(|| "your provider".into());
    let clinic = clinics
        .iter()
        .find(|c| c.clinic_num == appointment.clinic_num)
        .map(|c| c.description.clone())
        .unwrap_or_default();
    let operatory = operatories
        .iter()
        .find(|o| o.operatory_num == appointment.operatory_num)
        .map(|o| o.description.clone())
        .unwrap_or_default();

    let replacements = [
        ("{FirstName}", patient.f_name.clone()),
        ("{LastName}", patient.l_name.clone()),
        ("{PatientName}", format!("{} {}", patient.f_name, patient.l_name)),
        ("{Date}", appointment.apt_datetime.format("%a, %b %d").to_string()),
        ("{Time}", appointment.apt_datetime.format("%H:%M").to_string()),
        (
            "{DateTime}",
            appointment
                .apt_datetime
                .format("%a, %b %d at %H:%M")
                .to_string(),
        ),
        ("{Provider}", provider),
        ("{Clinic}", clinic),
        ("{Operatory}", operatory),
        ("{Today}", now.format("%b %d, %Y").to_string()),
    ];
    let mut text = template.to_string();
    for (token, value) in replacements {
        text = text.replace(token, &value);
    }
    text
}

/// Which transport a rule resolves to for a patient. OD's `CommType::Text`
/// prefers Telegram and falls back to Africa's Talking SMS; `Preferred` keeps
/// the existing Telegram → SMTP order and adds SMS as a last resort. Email
/// rules stay on SMTP. Other transports can take over by changing this one
/// function.
pub fn resolve_transport(
    comm_type: CommType,
    email: Option<&str>,
    telegram_chat_id: &str,
    phone: Option<&str>,
    config: &EServicesConfig,
) -> Option<TransportKind> {
    let telegram_ready = config.is_enabled(TransportKind::Telegram)
        && !telegram_chat_id.trim().is_empty();
    let smtp_ready = config.is_enabled(TransportKind::Smtp)
        && email.is_some_and(|value| !value.trim().is_empty());
    let africas_talking_ready = config.readiness(TransportKind::AfricasTalking)
        == TransportReadiness::Ready
        && phone.is_some_and(|value| !value.trim().is_empty());

    match comm_type {
        CommType::Text => {
            if telegram_ready {
                Some(TransportKind::Telegram)
            } else if africas_talking_ready {
                Some(TransportKind::AfricasTalking)
            } else {
                None
            }
        }
        CommType::Email | CommType::SecureEmail => smtp_ready.then_some(TransportKind::Smtp),
        CommType::Preferred => {
            if telegram_ready {
                Some(TransportKind::Telegram)
            } else if smtp_ready {
                Some(TransportKind::Smtp)
            } else if africas_talking_ready {
                Some(TransportKind::AfricasTalking)
            } else {
                None
            }
        }
    }
}

/// Telegram chat IDs and Africa's Talking numbers both use the one existing
/// "opt out of text messages" flag.
fn is_text_transport(transport: TransportKind) -> bool {
    matches!(transport, TransportKind::Telegram | TransportKind::AfricasTalking)
}

fn fallback_body(rule: &ReminderRule) -> &'static str {
    match rule.type_cur {
        ReminderType::ConfirmationFutureDay => "Please confirm your upcoming appointment at {Clinic} on {DateTime}. Reply to this message to confirm.",
        ReminderType::Arrival => "You have an appointment at {Clinic} today at {Time}. Please let us know when you arrive.",
        ReminderType::ScheduleThankYou | ReminderType::NewPatientThankYou | ReminderType::GeneralMessage => {
            "Thank you for visiting {Clinic} on {DateTime}. We hope you have a smooth recovery."
        }
        _ => "This is a reminder for your appointment at {Clinic} on {DateTime}.",
    }
}

fn default_subject(rule: &ReminderRule) -> &'static str {
    match rule.type_cur {
        ReminderType::ConfirmationFutureDay => "Appointment confirmation",
        ReminderType::ScheduleThankYou
        | ReminderType::NewPatientThankYou
        | ReminderType::GeneralMessage => "Thank you",
        _ => "Appointment reminder",
    }
}

/// Shared recipient/template selection for both the upcoming-appointment rules
/// and the completed-procedure follow-up rules.
fn candidate_message(
    rule: &ReminderRule,
    patient: &Patient,
    appointment: &Appointment,
    ctx: &ReminderContext<'_>,
) -> Option<OutboxMessage> {
    let contact = ctx
        .contacts
        .iter()
        .find(|c| c.pat_num == patient.pat_num)
        .cloned()
        .unwrap_or_else(|| PatientContact {
            pat_num: patient.pat_num,
            ..Default::default()
        });
    if contact.opt_out_email && contact.opt_out_telegram {
        return None;
    }

    let effective_email = if contact.email_override.trim().is_empty() {
        patient.email.trim()
    } else {
        contact.email_override.trim()
    };
    let transport = resolve_transport(
        rule.comm_type,
        Some(effective_email),
        &contact.telegram_chat_id,
        Some(&patient.home_phone),
        ctx.config,
    )?;
    if transport.is_email() && contact.opt_out_email {
        return None;
    }
    if is_text_transport(transport) && contact.opt_out_telegram {
        return None;
    }

    let recipient = match transport {
        TransportKind::Smtp => effective_email.to_string(),
        TransportKind::Telegram => contact.telegram_chat_id.trim().to_string(),
        TransportKind::AfricasTalking => patient.home_phone.trim().to_string(),
        _ => String::new(),
    };
    if recipient.is_empty() {
        return None;
    }

    let template = if transport.is_email() {
        &rule.email_body
    } else {
        &rule.sms_template
    };
    let chosen = if template.trim().is_empty() {
        fallback_body(rule)
    } else {
        template.as_str()
    };
    let body = render_template(
        chosen,
        patient,
        appointment,
        ctx.providers,
        ctx.clinics,
        ctx.operatories,
        ctx.now,
    );
    let subject = render_template(
        if rule.email_subject.trim().is_empty() {
            default_subject(rule)
        } else {
            &rule.email_subject
        },
        patient,
        appointment,
        ctx.providers,
        ctx.clinics,
        ctx.operatories,
        ctx.now,
    );

    Some(OutboxMessage {
        message_num: 0,
        pat_num: patient.pat_num,
        apt_num: appointment.apt_num,
        rule_num: rule.rule_num,
        reminder_type: rule.type_cur,
        transport,
        recipient,
        subject,
        body,
        status: if ctx.dry_run {
            OutboxStatus::DryRun
        } else {
            OutboxStatus::Pending
        },
        attempts: 0,
        created_at: ctx.now,
        sent_at: None,
        last_error: String::new(),
    })
}

/// Generate candidate outbox rows. `is_sent(rule_num, apt_num)` is supplied by
/// the DB layer and implements the OD dedupe check. Upcoming rules use
/// appointment time; post-treatment `GeneralMessage` rules use the procedure's
/// `DateComplete` and a positive `TSPrior` measured in minutes after treatment.
pub fn due_messages(
    ctx: &ReminderContext<'_>,
    is_sent: impl Fn(Id, Id) -> bool,
) -> Vec<OutboxMessage> {
    let mut messages = Vec::new();

    for rule in ctx.rules {
        if !rule.is_enabled || !ctx.allow_types.contains(&rule.type_cur) {
            continue;
        }
        // ScheduleThankYou/NewPatientThankYou need OD's appointment-completion
        // flags; they stay in the editor but are not generated here. Birthday,
        // WebSched and portal rules are HQ-owned or need data this port lacks.
        if !matches!(
            rule.type_cur,
            ReminderType::Reminder
                | ReminderType::ConfirmationFutureDay
                | ReminderType::Arrival
                | ReminderType::GeneralMessage
        ) {
            continue;
        }

        if rule.type_cur == ReminderType::GeneralMessage {
            for procedure in ctx.procedures {
                if procedure.proc_status != ProcedureStatus::Complete
                    || procedure.apt_num == 0
                    || is_sent(rule.rule_num, procedure.apt_num)
                {
                    continue;
                }
                let Some(completed) = procedure.date_complete else {
                    continue;
                };
                let elapsed = (ctx.now - completed).num_minutes();
                if elapsed < rule.minutes_prior as i64
                    || (rule.do_not_send_within_minutes > 0
                        && elapsed < rule.do_not_send_within_minutes as i64)
                {
                    continue;
                }
                let Some(patient) = ctx
                    .patients
                    .iter()
                    .find(|p| p.pat_num == procedure.pat_num)
                else {
                    continue;
                };
                let appointment = ctx
                    .appointments
                    .iter()
                    .find(|a| a.apt_num == procedure.apt_num)
                    .cloned()
                    .unwrap_or_else(|| Appointment {
                        apt_num: procedure.apt_num,
                        pat_num: procedure.pat_num,
                        apt_datetime: completed,
                        provider_num: procedure.provider_num,
                        ..Default::default()
                    });
                if let Some(message) = candidate_message(rule, patient, &appointment, ctx) {
                    messages.push(message);
                }
            }
            continue;
        }

        for appointment in ctx.appointments {
            if !matches!(
                appointment.apt_status,
                AppointmentStatus::Scheduled | AppointmentStatus::Planned
            ) {
                continue;
            }
            if is_sent(rule.rule_num, appointment.apt_num) {
                continue;
            }

            let diff_minutes = (appointment.apt_datetime - ctx.now).num_minutes();
            if diff_minutes > rule.minutes_prior as i64
                || diff_minutes < rule.do_not_send_within_minutes as i64
            {
                continue;
            }

            let Some(patient) = ctx
                .patients
                .iter()
                .find(|p| p.pat_num == appointment.pat_num)
            else {
                continue;
            };
            if let Some(message) = candidate_message(rule, patient, appointment, ctx) {
                messages.push(message);
            }
        }
    }

    messages.sort_by_key(|m| (m.apt_num, m.rule_num, m.transport as i8));
    messages
}

/// A ready-made "send now" message for the selected patient/appointment. Used
/// by the GUI buttons; it bypasses rule timing but still honours the outbox,
/// opt-outs and dry-run flag.
pub fn manual_message(
    patient: &Patient,
    appointment: Option<&Appointment>,
    contact: &PatientContact,
    config: &EServicesConfig,
    transport: TransportKind,
    text: &str,
    dry_run: bool,
    now: NaiveDateTime,
) -> Result<OutboxMessage, String> {
    if !config.is_enabled(transport) {
        return Err(format!("{} is not enabled in eServices settings", transport.label()));
    }
    if contact.opt_out_email && transport.is_email() {
        return Err("This patient opted out of email".into());
    }
    if contact.opt_out_telegram && is_text_transport(transport) {
        return Err("This patient opted out of text messages".into());
    }
    let recipient = match transport {
        TransportKind::Smtp => {
            let email = if contact.email_override.trim().is_empty() {
                &patient.email
            } else {
                &contact.email_override
            };
            email.trim().to_string()
        }
        TransportKind::Telegram => contact.telegram_chat_id.trim().to_string(),
        TransportKind::AfricasTalking => patient.home_phone.trim().to_string(),
        _ => return Err(format!("{} is not implemented in this build", transport.label())),
    };
    if recipient.is_empty() {
        return Err("No recipient address is set for this patient and transport".into());
    }
    let (subject, body) = if let Some(apt) = appointment {
        (
            format!("Appointment at {} on {}", apt.apt_datetime.format("%b %d"), apt.apt_datetime.format("%H:%M")),
            format!(
                "Hello {},\n\n{}",
                patient.f_name,
                text.replace("{DateTime}", &apt.apt_datetime.format("%a, %b %d at %H:%M").to_string())
            ),
        )
    } else {
        ("Clinic message".to_string(), format!("Hello {},\n\n{}", patient.f_name, text))
    };
    Ok(OutboxMessage {
        message_num: 0,
        pat_num: patient.pat_num,
        apt_num: appointment.map(|a| a.apt_num).unwrap_or(0),
        rule_num: 0,
        reminder_type: ReminderType::GeneralMessage,
        transport,
        recipient,
        subject,
        body,
        status: if dry_run { OutboxStatus::DryRun } else { OutboxStatus::Pending },
        attempts: 0,
        created_at: now,
        sent_at: None,
        last_error: String::new(),
    })
}

/// Short day/month text used by the GUI summary.
pub fn reminder_day_label(date: NaiveDateTime) -> String {
    format!("{} {:02}/{:02}", date.weekday(), date.month(), date.day())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use od_domain::{Gender, RecallPriority};

    fn test_timestamp() -> NaiveDateTime {
        chrono::DateTime::from_timestamp(1_767_000_000, 0)
            .unwrap()
            .naive_utc()
    }

    fn patient(num: Id) -> Patient {
        Patient {
            pat_num: num,
            f_name: "Marta".into(),
            l_name: "Alvarez".into(),
            email: "marta@example.test".into(),
            birth_date: Some(chrono::NaiveDate::from_ymd_opt(1979, 3, 12).unwrap()),
            gender: Gender::Female,
            ..Default::default()
        }
    }

    fn appointment(num: Id, pat: Id, when: NaiveDateTime) -> Appointment {
        Appointment {
            apt_num: num,
            pat_num: pat,
            apt_datetime: when,
            apt_status: AppointmentStatus::Scheduled,
            priority: RecallPriority::Normal,
            ..Default::default()
        }
    }

    fn context<'a>(
        rules: &'a [ReminderRule],
        appointments: &'a [Appointment],
        patients: &'a [Patient],
        contacts: &'a [PatientContact],
        config: &'a EServicesConfig,
        now: NaiveDateTime,
        dry_run: bool,
        allow: &'a [ReminderType],
    ) -> ReminderContext<'a> {
        context_with_procedures(
            rules,
            appointments,
            &[],
            patients,
            contacts,
            config,
            now,
            dry_run,
            allow,
        )
    }

    fn context_with_procedures<'a>(
        rules: &'a [ReminderRule],
        appointments: &'a [Appointment],
        procedures: &'a [Procedure],
        patients: &'a [Patient],
        contacts: &'a [PatientContact],
        config: &'a EServicesConfig,
        now: NaiveDateTime,
        dry_run: bool,
        allow: &'a [ReminderType],
    ) -> ReminderContext<'a> {
        ReminderContext {
            rules,
            appointments,
            procedures,
            patients,
            contacts,
            providers: &[],
            clinics: &[],
            operatories: &[],
            config,
            now,
            allow_types: allow,
            dry_run,
        }
    }

    #[test]
    fn templates_render_od_style_tokens() {
        let p = patient(1);
        let a = appointment(1, 1, test_timestamp());
        let text = render_template(
            "Hi {FirstName} {LastName}, {DateTime} with {Provider} at {Clinic}",
            &p,
            &a,
            &[],
            &[],
            &[],
            a.apt_datetime,
        );
        assert!(text.starts_with("Hi Marta Alvarez,"));
        assert!(!text.contains("{DateTime}"));
        assert!(text.contains("your provider"));
    }

    #[test]
    fn due_messages_respect_lead_time_and_do_not_send_within() {
        let now = test_timestamp();
        let rule = ReminderRule {
            rule_num: 1,
            minutes_prior: 1440,
            do_not_send_within_minutes: 60,
            email_body: "Reminder for {PatientName} on {Date}".into(),
            ..Default::default()
        };
        let patients = vec![patient(1)];
        let contacts = vec![PatientContact {
            pat_num: 1,
            email_override: String::new(),
            ..Default::default()
        }];
        let mut config = EServicesConfig::default();
        config.set_enabled(TransportKind::Smtp, true);
        let appointments = vec![
            appointment(1, 1, now + Duration::minutes(600)),   // in window
            appointment(2, 1, now + Duration::minutes(30)),    // too close
            appointment(3, 1, now + Duration::minutes(3000)),  // too far
        ];
        let rules = [rule];
        let ctx = context(
            &rules,
            &appointments,
            &patients,
            &contacts,
            &config,
            now,
            false,
            &[ReminderType::Reminder],
        );
        let rows = due_messages(&ctx, |_, _| false);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].apt_num, 1);
        assert_eq!(rows[0].transport, TransportKind::Smtp);
        assert_eq!(rows[0].recipient, "marta@example.test");
        assert_eq!(rows[0].status, OutboxStatus::Pending);
        assert!(rows[0].body.contains("Marta Alvarez"));
    }

    #[test]
    fn general_message_waits_72_hours_after_completion_and_dedupes() {
        let now = test_timestamp();
        let rule = ReminderRule {
            rule_num: 4,
            type_cur: ReminderType::GeneralMessage,
            minutes_prior: 4320,
            comm_type: CommType::Email,
            email_body: "Thanks {PatientName} for your visit on {Date}".into(),
            ..Default::default()
        };
        let patients = vec![patient(1)];
        let contacts = vec![PatientContact {
            pat_num: 1,
            ..Default::default()
        }];
        let mut config = EServicesConfig::default();
        config.set_enabled(TransportKind::Smtp, true);
        let appointments = vec![appointment(9, 1, now - Duration::days(3))];
        let procedures = vec![
            Procedure {
                proc_num: 1,
                pat_num: 1,
                apt_num: 9,
                proc_status: ProcedureStatus::Complete,
                date_complete: Some(now - Duration::minutes(4320)),
                provider_num: 3,
                ..Default::default()
            },
            Procedure {
                proc_num: 2,
                pat_num: 1,
                apt_num: 10,
                proc_status: ProcedureStatus::Complete,
                date_complete: Some(now - Duration::minutes(4319)),
                ..Default::default()
            },
            Procedure {
                proc_num: 3,
                pat_num: 1,
                apt_num: 11,
                proc_status: ProcedureStatus::TreatmentPlanned,
                date_complete: Some(now - Duration::days(10)),
                ..Default::default()
            },
        ];
        let rules = [rule];
        let ctx = context_with_procedures(
            &rules,
            &appointments,
            &procedures,
            &patients,
            &contacts,
            &config,
            now,
            false,
            &[ReminderType::GeneralMessage],
        );
        let rows = due_messages(&ctx, |_, _| false);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].apt_num, 9);
        assert_eq!(rows[0].reminder_type, ReminderType::GeneralMessage);
        assert!(rows[0].body.contains("Marta Alvarez"));
        assert_eq!(
            due_messages(&ctx, |rule, apt| rule == 4 && apt == 9).len(),
            0
        );
    }

    #[test]
    fn sent_log_suppresses_duplicate_reminders() {
        let now = test_timestamp();
        let rule = ReminderRule {
            rule_num: 7,
            minutes_prior: 1440,
            ..Default::default()
        };
        let appointments = vec![appointment(1, 1, now + Duration::minutes(600))];
        let patients = vec![patient(1)];
        let contacts = vec![PatientContact {
            pat_num: 1,
            email_override: "other@example.test".into(),
            ..Default::default()
        }];
        let mut config = EServicesConfig::default();
        config.set_enabled(TransportKind::Smtp, true);
        let rules = [rule];
        let ctx = context(
            &rules,
            &appointments,
            &patients,
            &contacts,
            &config,
            now,
            false,
            &[ReminderType::Reminder],
        );
        assert_eq!(due_messages(&ctx, |_, _| false).len(), 1);
        assert_eq!(due_messages(&ctx, |rule, apt| rule == 7 && apt == 1).len(), 0);
    }

    #[test]
    fn telegram_is_preferred_when_a_chat_id_exists() {
        let now = test_timestamp();
        let rule = ReminderRule {
            rule_num: 1,
            minutes_prior: 1440,
            sms_template: "Hi {FirstName}, appointment {DateTime}".into(),
            ..Default::default()
        };
        let appointments = vec![appointment(1, 1, now + Duration::minutes(600))];
        let patients = vec![patient(1)];
        let contacts = vec![PatientContact {
            pat_num: 1,
            telegram_chat_id: "123456789".into(),
            ..Default::default()
        }];
        let mut config = EServicesConfig::default();
        config.set_enabled(TransportKind::Telegram, true);
        let rules = [rule];
        let ctx = context(
            &rules,
            &appointments,
            &patients,
            &contacts,
            &config,
            now,
            true,
            &[ReminderType::Reminder],
        );
        let rows = due_messages(&ctx, |_, _| false);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].transport, TransportKind::Telegram);
        assert_eq!(rows[0].recipient, "123456789");
        assert_eq!(rows[0].status, OutboxStatus::DryRun);
    }

    #[test]
    fn text_routes_fall_back_to_africas_talking_without_changing_precedence() {
        let mut config = EServicesConfig::default();
        config.set_enabled(TransportKind::AfricasTalking, true);
        config.africas_talking_username = "clinic".into();
        config.africas_talking_key = "test-key".into();
        let phone = Some("+251911234567");

        assert_eq!(
            resolve_transport(CommType::Text, Some("marta@example.test"), "", phone, &config),
            Some(TransportKind::AfricasTalking)
        );
        assert_eq!(
            resolve_transport(CommType::Preferred, Some("marta@example.test"), "", phone, &config),
            Some(TransportKind::AfricasTalking)
        );
        assert_eq!(
            resolve_transport(CommType::Email, Some("marta@example.test"), "", phone, &config),
            None,
            "email rules must not resolve to an SMS channel"
        );
        assert_eq!(
            resolve_transport(CommType::Text, None, "", None, &config),
            None,
            "a patient without a phone number cannot receive SMS"
        );

        config.set_enabled(TransportKind::Telegram, true);
        assert_eq!(
            resolve_transport(CommType::Text, None, "123456789", phone, &config),
            Some(TransportKind::Telegram),
            "Telegram keeps priority for text when its chat ID exists"
        );

        config.set_enabled(TransportKind::Smtp, true);
        assert_eq!(
            resolve_transport(CommType::Preferred, Some("marta@example.test"), "", phone, &config),
            Some(TransportKind::Smtp),
            "SMTP keeps priority over SMS for preferred contacts"
        );
    }

    #[test]
    fn due_messages_can_use_africas_talking_for_text_reminders() {
        let now = test_timestamp();
        let rule = ReminderRule {
            rule_num: 1,
            minutes_prior: 1440,
            comm_type: CommType::Text,
            sms_template: "Hi {FirstName}, appointment {DateTime}".into(),
            ..Default::default()
        };
        let appointments = vec![appointment(1, 1, now + Duration::minutes(600))];
        let patients = vec![Patient {
            home_phone: "+251911234567".into(),
            ..patient(1)
        }];
        let contacts = vec![PatientContact {
            pat_num: 1,
            ..Default::default()
        }];
        let mut config = EServicesConfig::default();
        config.set_enabled(TransportKind::AfricasTalking, true);
        config.africas_talking_username = "clinic".into();
        config.africas_talking_key = "test-key".into();
        let rules = [rule];
        let ctx = context(
            &rules,
            &appointments,
            &patients,
            &contacts,
            &config,
            now,
            true,
            &[ReminderType::Reminder],
        );
        let rows = due_messages(&ctx, |_, _| false);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].transport, TransportKind::AfricasTalking);
        assert_eq!(rows[0].recipient, "+251911234567");
        assert!(rows[0].body.starts_with("Hi Marta"));
        assert_eq!(rows[0].status, OutboxStatus::DryRun);
    }

    #[test]
    fn africas_talking_honours_text_opt_out_and_manual_send() {
        let now = test_timestamp();
        let patient = Patient {
            home_phone: "+251911234567".into(),
            ..patient(1)
        };
        let mut contact = PatientContact {
            pat_num: 1,
            ..Default::default()
        };
        let mut config = EServicesConfig::default();
        config.set_enabled(TransportKind::AfricasTalking, true);

        let message = manual_message(
            &patient,
            None,
            &contact,
            &config,
            TransportKind::AfricasTalking,
            "See you soon",
            true,
            now,
        )
        .expect("manual SMS should queue with a phone number");
        assert_eq!(message.recipient, "+251911234567");
        assert_eq!(message.transport, TransportKind::AfricasTalking);
        assert_eq!(message.status, OutboxStatus::DryRun);

        contact.opt_out_telegram = true;
        assert!(manual_message(
            &patient,
            None,
            &contact,
            &config,
            TransportKind::AfricasTalking,
            "See you soon",
            true,
            now,
        )
        .unwrap_err()
        .contains("opted out"));
    }

    #[test]
    fn opt_out_and_missing_transport_produce_no_messages() {
        let now = test_timestamp();
        let rule = ReminderRule {
            rule_num: 1,
            minutes_prior: 1440,
            ..Default::default()
        };
        let appointments = vec![appointment(1, 1, now + Duration::minutes(600))];
        let patients = vec![patient(1)];
        let contacts = vec![PatientContact {
            pat_num: 1,
            opt_out_email: true,
            opt_out_telegram: true,
            ..Default::default()
        }];
        let config = EServicesConfig::default();
        let rules = [rule];
        let ctx = context(
            &rules,
            &appointments,
            &patients,
            &contacts,
            &config,
            now,
            false,
            &[ReminderType::Reminder],
        );
        assert!(due_messages(&ctx, |_, _| false).is_empty());
    }
}
