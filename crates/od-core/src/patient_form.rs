//! Printable patient forms built from OD-compatible patient data.
//!
//! The port does not ship OD's form designer. Instead it offers one reviewed,
//! self-describing intake/medical-history form that reads the same patient,
//! patientnote and disease rows already kept for the Family and Chart modules.
//! Nothing is written back: a form is a print/export view of existing data.

use chrono::NaiveDate;
use od_domain::{Disease, Patient, PatientNote};

/// One problem/medical line on the form.
#[derive(Clone, Debug, PartialEq)]
pub struct PatientFormProblem {
    pub name: String,
    pub status: String,
    pub date_start: NaiveDate,
    pub date_stop: Option<NaiveDate>,
    pub note: String,
}

/// A printable, self-describing snapshot of one patient's demographics and
/// medical history. Every field comes from that patient's own records; there
/// is no family/guarantor grouping and no insurance data by default.
#[derive(Clone, Debug, PartialEq)]
pub struct PatientFormDocument {
    pub pat_num: u32,
    pub patient_name: String,
    pub generated_on: NaiveDate,
    pub practice: String,
    pub birth_date: Option<NaiveDate>,
    pub gender: String,
    pub home_phone: String,
    pub mobile_phone: String,
    pub email: String,
    pub address: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub chart_number: String,
    pub medical: String,
    pub treatment: String,
    pub service: String,
    pub problems: Vec<PatientFormProblem>,
}

/// Build a form document for one patient. `diseases` may contain other
/// patients' rows; only this patient's problems are included, oldest first.
pub fn build_patient_form_document(
    patient: &Patient,
    note: Option<&PatientNote>,
    diseases: &[Disease],
    practice: &str,
    as_of: NaiveDate,
) -> PatientFormDocument {
    let mut problems: Vec<PatientFormProblem> = diseases
        .iter()
        .filter(|d| d.pat_num == patient.pat_num)
        .map(|d| PatientFormProblem {
            name: d.disease_name.clone(),
            status: d.status.label().to_string(),
            date_start: d.date_start,
            date_stop: d.date_stop,
            note: d.note.clone(),
        })
        .collect();
    problems.sort_by_key(|p| (p.date_start, p.name.clone()));

    PatientFormDocument {
        pat_num: patient.pat_num,
        patient_name: patient.display_name(),
        generated_on: as_of,
        practice: practice.to_string(),
        birth_date: patient.birth_date,
        gender: patient.gender.label().to_string(),
        home_phone: patient.home_phone.clone(),
        mobile_phone: patient.wireless_phone.clone(),
        email: patient.email.clone(),
        address: patient.address.clone(),
        city: patient.city.clone(),
        state: patient.state.clone(),
        zip: patient.zip.clone(),
        chart_number: patient.chart_number.clone(),
        medical: note.map(|n| n.medical.clone()).unwrap_or_default(),
        treatment: note.map(|n| n.treatment.clone()).unwrap_or_default(),
        service: note.map(|n| n.service.clone()).unwrap_or_default(),
        problems,
    }
}

/// Print-ready HTML. The heading records the patient number and generation
/// date so a filed copy stays self-describing.
pub fn render_patient_form_html(doc: &PatientFormDocument) -> String {
    let field = |label: &str, value: &str| {
        format!(
            "<div class=\"field\"><span class=\"label\">{}</span><span class=\"value\">{}</span></div>",
            crate::escape_html(label),
            crate::escape_html(value)
        )
    };
    let problems = if doc.problems.is_empty() {
        "<p class=\"muted\">No problems recorded.</p>".to_string()
    } else {
        let rows: String = doc
            .problems
            .iter()
            .map(|p| {
                format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    crate::escape_html(&p.name),
                    crate::escape_html(&p.status),
                    p.date_start.format("%Y-%m-%d"),
                    p.date_stop
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .unwrap_or_default(),
                    crate::escape_html(&p.note)
                )
            })
            .collect();
        format!(
            "<table><thead><tr><th>Problem</th><th>Status</th><th>Start</th><th>End</th><th>Note</th></tr></thead><tbody>{rows}</tbody></table>"
        )
    };
    let title = format!(
        "Patient form - {} (#{}) - {}",
        doc.patient_name,
        doc.pat_num,
        doc.generated_on.format("%Y-%m-%d")
    );
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title>\
<style>body{{font-family:Arial,sans-serif;margin:24px;color:#222}}h1{{font-size:20px}}h2{{font-size:15px;margin-top:24px;border-bottom:1px solid #bbb;padding-bottom:4px}}\
.grid{{display:grid;grid-template-columns:repeat(2,minmax(220px,1fr));gap:6px 24px}}.field{{display:flex;gap:8px}}\
.label{{font-weight:bold;min-width:110px}}.muted{{color:#777}}table{{border-collapse:collapse;width:100%}}th,td{{border:1px solid #bbb;padding:5px;text-align:left;vertical-align:top}}\
textarea{{width:100%;min-height:54px}}button{{margin-bottom:16px}}@media print{{button{{display:none}}}}</style></head><body>\
<button onclick=\"window.print()\">Print form</button>\
<h1>{}</h1><p class=\"muted\">{} · generated {}</p>\
<h2>Demographics</h2><div class=\"grid\">{}</div>\
<h2>Medical history</h2>{}<h2>Notes</h2>\
<div class=\"field\"><span class=\"label\">Medical</span><span class=\"value\">{}</span></div>\
<div class=\"field\"><span class=\"label\">Treatment</span><span class=\"value\">{}</span></div>\
<div class=\"field\"><span class=\"label\">Service</span><span class=\"value\">{}</span></div>\
<h2>Signature</h2><p>Patient / guardian: ____________________________  Date: ____________</p>\
</body></html>",
        crate::escape_html(&title),
        crate::escape_html(&title),
        crate::escape_html(&doc.practice),
        doc.generated_on.format("%Y-%m-%d"),
        [
            field("Patient", &doc.patient_name),
            field("Patient #", &doc.pat_num.to_string()),
            field("Birth date", &doc.birth_date.map(|d| d.to_string()).unwrap_or_default()),
            field("Gender", &doc.gender),
            field("Home phone", &doc.home_phone),
            field("Mobile phone", &doc.mobile_phone),
            field("Email", &doc.email),
            field("Chart number", &doc.chart_number),
            field("Address", &doc.address),
            field("City", &doc.city),
            field("State", &doc.state),
            field("Zip", &doc.zip),
        ]
        .join(""),
        problems,
        crate::escape_html(&doc.medical),
        crate::escape_html(&doc.treatment),
        crate::escape_html(&doc.service),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_domain::{Disease, Gender, Patient, PatientNote, ProblemStatus};

    fn document() -> PatientFormDocument {
        let patient = Patient {
            pat_num: 7,
            f_name: "Ada".into(),
            l_name: "<Lovelace>".into(),
            birth_date: NaiveDate::from_ymd_opt(1990, 2, 3),
            gender: Gender::Female,
            home_phone: "+251 11 000 000".into(),
            ..Default::default()
        };
        let note = PatientNote {
            pat_num: 7,
            medical: "No known allergies".into(),
            ..Default::default()
        };
        let diseases = vec![
            Disease {
                pat_num: 8,
                disease_name: "Other patient".into(),
                date_start: NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
                ..Default::default()
            },
            Disease {
                pat_num: 7,
                disease_name: "Caries".into(),
                status: ProblemStatus::Active,
                date_start: NaiveDate::from_ymd_opt(2024, 5, 1).unwrap(),
                note: "Upper left".into(),
                ..Default::default()
            },
        ];
        build_patient_form_document(
            &patient,
            Some(&note),
            &diseases,
            "Highland Dental",
            NaiveDate::from_ymd_opt(2026, 11, 26).unwrap(),
        )
    }

    #[test]
    fn patient_form_includes_only_this_patients_data() {
        let doc = document();
        assert_eq!(doc.pat_num, 7);
        assert_eq!(doc.patient_name, "<Lovelace>, Ada");
        assert_eq!(doc.problems.len(), 1);
        assert_eq!(doc.problems[0].name, "Caries");
        assert_eq!(doc.problems[0].status, "Active");
        assert_eq!(doc.medical, "No known allergies");
    }

    #[test]
    fn patient_form_html_is_self_describing_and_escapes_text() {
        let html = render_patient_form_html(&document());
        assert!(html.contains("Patient form - &lt;Lovelace&gt;, Ada (#7) - 2026-11-26"));
        assert!(html.contains("Highland Dental"));
        assert!(html.contains("No known allergies"));
        assert!(html.contains("Upper left"));
        assert!(html.contains("window.print()"));
        assert!(!html.contains("Other patient"));
        assert!(!html.contains("<Lovelace>, Ada"));
    }
}
