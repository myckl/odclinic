//! Read-only patient badges for the picker, family grid and registry banner.
//!
//! OD stores the new-patient flag on `appointment.IsNewPatient` while its
//! New Patients report derives the same idea from the first completed
//! procedure. This view accepts either signal and resolves referral names from
//! `refattach` rows of type `RefFrom` (1), the same rows Referral Analysis
//! counts. Nothing here writes to an OD table.

use std::collections::HashMap;

use chrono::{Duration, NaiveDate, NaiveDateTime};
use od_domain::{Id, Patient, Procedure, ProcedureStatus, Referral, RefAttach, Appointment};

/// Badge data for one patient. `labels` is ready to render in a compact grid.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PatientBadge {
    pub new_patient: bool,
    pub referral: String,
    pub labels: Vec<String>,
}

/// Build a patient-number-keyed badge map. `today` bounds the derived
/// new-patient window: a first completed procedure in the last 90 days counts
/// as new even if the source appointment did not carry the OD flag.
pub fn patient_badges(
    patients: &[Patient],
    appointments: &[Appointment],
    procedures: &[Procedure],
    refattaches: &[RefAttach],
    referrals: &[Referral],
    today: NaiveDateTime,
) -> HashMap<Id, PatientBadge> {
    let cutoff = (today - Duration::days(90)).date();
    let mut badges = HashMap::new();

    for patient in patients {
        let pat_num = patient.pat_num;
        let flagged_new = appointments
            .iter()
            .any(|apt| apt.pat_num == pat_num && apt.is_new_patient);
        let first_complete = procedures
            .iter()
            .filter(|proc| {
                proc.pat_num == pat_num && proc.proc_status == ProcedureStatus::Complete
            })
            .map(|proc| proc.proc_date)
            .min();
        let new_patient = flagged_new || first_complete.is_some_and(|date| date >= cutoff);

        let referral = refattaches
            .iter()
            .filter(|attach| attach.pat_num == pat_num && attach.ref_type == 1)
            .filter_map(|attach| referrals.iter().find(|r| r.referral_num == attach.referral_num))
            .map(|referral| {
                let name = [referral.title.as_str(), &referral.f_name, &referral.l_name]
                    .into_iter()
                    .filter(|part| !part.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                if name.trim().is_empty() {
                    format!("Source #{}", referral.referral_num)
                } else {
                    name
                }
            })
            .next()
            .unwrap_or_default();

        let mut labels = Vec::new();
        if new_patient {
            labels.push("New".to_string());
        }
        if !referral.is_empty() {
            labels.push(format!("Ref: {referral}"));
        }
        badges.insert(
            pat_num,
            PatientBadge {
                new_patient,
                referral,
                labels,
            },
        );
    }

    badges
}

/// Date-only form used by callers that already have a NaiveDate.
pub fn patient_badges_on_date(
    patients: &[Patient],
    appointments: &[Appointment],
    procedures: &[Procedure],
    refattaches: &[RefAttach],
    referrals: &[Referral],
    today: NaiveDate,
) -> HashMap<Id, PatientBadge> {
    let start = today.and_hms_opt(0, 0, 0).unwrap_or_default();
    patient_badges(
        patients,
        appointments,
        procedures,
        refattaches,
        referrals,
        start,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_domain::ProcedureStatus;

    fn patient(num: Id) -> Patient {
        Patient {
            pat_num: num,
            f_name: "Ada".into(),
            l_name: "Tester".into(),
            ..Default::default()
        }
    }

    #[test]
    fn badges_use_appointment_flag_and_first_completed_procedure() {
        let today = chrono::DateTime::from_timestamp(1_767_000_000, 0)
            .unwrap()
            .naive_utc();
        let patients = vec![patient(1), patient(2), patient(3)];
        let appointments = vec![Appointment {
            pat_num: 2,
            is_new_patient: true,
            ..Default::default()
        }];
        let procedures = vec![Procedure {
            pat_num: 3,
            proc_status: ProcedureStatus::Complete,
            proc_date: (today - Duration::days(10)).date(),
            ..Default::default()
        }];
        let badges = patient_badges(
            &patients,
            &appointments,
            &procedures,
            &[],
            &[],
            today,
        );
        assert!(!badges[&1].new_patient);
        assert!(badges[&2].new_patient);
        assert!(badges[&3].new_patient);
        assert_eq!(badges[&2].labels, vec!["New"]);
    }

    #[test]
    fn referral_badge_uses_ref_from_rows_only() {
        let today = chrono::DateTime::from_timestamp(1_767_000_000, 0)
            .unwrap()
            .naive_utc();
        let patients = vec![patient(1)];
        let attaches = vec![
            RefAttach {
                pat_num: 1,
                referral_num: 7,
                ref_type: 1,
                ..Default::default()
            },
            RefAttach {
                pat_num: 1,
                referral_num: 8,
                ref_type: 0,
                ..Default::default()
            },
        ];
        let referrals = vec![
            Referral {
                referral_num: 7,
                title: "Dr.".into(),
                f_name: "Grace".into(),
                l_name: "Hopper".into(),
                ..Default::default()
            },
            Referral {
                referral_num: 8,
                f_name: "Hidden".into(),
                l_name: "Source".into(),
                ..Default::default()
            },
        ];
        let badges = patient_badges(&patients, &[], &[], &attaches, &referrals, today);
        assert_eq!(badges[&1].referral, "Dr. Grace Hopper");
        assert_eq!(badges[&1].labels, vec!["Ref: Dr. Grace Hopper"]);
    }
}
