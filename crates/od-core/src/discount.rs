//! Pricing a completed procedure from an OD discount plan.
//!
//! Mirrors the upstream path `Procedures.Insert` / `Procedures.Update` →
//! `Adjustments.CreateAdjustmentForDiscountPlan` →
//! `Procedures.GetDiscountAmountForDiscountPlanAndValidate` from the public
//! 24_3 source, verified against the 25.3 assembly. The caller gathers the
//! rows; everything here is pure so the exact OD rules can be unit tested
//! without a database.
//!
//! Upstream rejects the discount (returns 0) when the procedure is outside the
//! subscription range, when a configured frequency category is already used
//! up, when no fee exists on the plan's fee schedule, or when the reduced fee
//! is not lower than the charged fee. An annual maximum then clamps the
//! remaining amount, including a partial final adjustment.

use chrono::{Datelike, Duration, NaiveDate};
use od_domain::{DiscountPlan, DiscountPlanSub, Id, Pref, Procedure, ProcedureCode};

/// OD `PrefName` values holding comma-delimited D-codes for the seven
/// frequency-limit categories, in `DiscountPlan.frequency_limits()` order.
const FREQUENCY_PREFS: [&str; 7] = [
    "DiscountPlanExamCodes",
    "DiscountPlanXrayCodes",
    "DiscountPlanProphyCodes",
    "DiscountPlanFluorideCodes",
    "DiscountPlanPerioCodes",
    "DiscountPlanLimitedCodes",
    "DiscountPlanPACodes",
];

/// Procedure-code numbers configured by the seven OD preferences. A code that
/// appears in more than one category is checked against every category, just
/// as upstream's `HasMetDiscountFrequencyLimitation` ORs the checks together.
#[derive(Clone, Debug, Default)]
pub struct FrequencyCodeSets {
    pub sets: [Vec<Id>; 7],
}

impl FrequencyCodeSets {
    /// Resolve OD's comma-delimited `D1110,D2140` preferences to `CodeNum`s.
    /// Unknown codes are ignored; the English pref names are stable in OD.
    pub fn from_prefs(prefs: &[Pref], codes: &[ProcedureCode]) -> Self {
        let mut sets: [Vec<Id>; 7] = Default::default();
        for (index, name) in FREQUENCY_PREFS.iter().enumerate() {
            let value = prefs
                .iter()
                .find(|p| p.pref_name.eq_ignore_ascii_case(name))
                .map(|p| p.pref_value.as_str())
                .unwrap_or("");
            for code in value.split(',') {
                let code = code.trim();
                if code.is_empty() {
                    continue;
                }
                if let Some(found) = codes.iter().find(|c| c.code.eq_ignore_ascii_case(code)) {
                    if !sets[index].contains(&found.code_num) {
                        sets[index].push(found.code_num);
                    }
                }
            }
        }
        Self { sets }
    }

    /// First category containing `code_num`, matching upstream's category
    /// priority. The full set is still checked by `discount_amount`.
    pub fn category_for(&self, code_num: Id) -> Option<usize> {
        self.sets.iter().position(|set| set.contains(&code_num))
    }
}

/// OD `DateTime.AddYears`, safe for 29 February (clamped to 28 February).
fn add_years(date: NaiveDate, years: i32) -> NaiveDate {
    let year = date.year() + years;
    for day in (1..=28).rev() {
        if let Some(result) = NaiveDate::from_ymd_opt(year, date.month(), day) {
            if day == date.day() || (date.day() > 28 && day == 28) {
                return result;
            }
        }
    }
    NaiveDate::from_ymd_opt(year, date.month(), 28)
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(year, 12, 31).unwrap())
}

/// Mirrors `DiscountPlanSubs.GetAnnualMaxDateEffective` /
/// `GetAnnualMaxDateTerm`. OD stores open ended subscriptions with a zero
/// date or any year before 1880; the effective month/day is then projected
/// onto the current year so the annual maximum resets each anniversary.
pub fn annual_max_range(
    effective: Option<NaiveDate>,
    term: Option<NaiveDate>,
    today: NaiveDate,
) -> (NaiveDate, NaiveDate) {
    let effective = effective.unwrap_or_else(|| NaiveDate::from_ymd_opt(1, 1, 1).unwrap());
    let start = if effective.year() < 1880 {
        add_years(effective, today.year() - effective.year())
    } else {
        effective
    };
    let end = match term {
        Some(date) if date.year() >= 1880 => date,
        _ => NaiveDate::from_ymd_opt(9999, 12, 31).unwrap(),
    };
    (start, end)
}

/// The one-year segment containing `reference`, clipped to the subscription.
/// Mirrors `GetDateEffectiveForAnnualDateRangeSegment` and
/// `GetDateTermForAnnualDateRangeSegment`; this is the window used for both
/// the annual maximum running total and frequency-limit counts.
pub fn annual_bucket(
    reference: NaiveDate,
    effective: Option<NaiveDate>,
    term: Option<NaiveDate>,
    today: NaiveDate,
) -> (NaiveDate, NaiveDate) {
    let (mut start, mut end) = annual_max_range(effective, term, today);
    if reference < start || reference > end {
        return (start, end);
    }
    while add_years(start, 1) <= reference {
        start = add_years(start, 1);
    }
    let segment_end = add_years(start, 1) - Duration::days(1);
    if end > segment_end {
        end = segment_end;
    }
    (start, end)
}

/// OD `Procedure.Quantity`: `BaseUnits + UnitQty`, never below one.
pub fn quantity(procedure: &Procedure) -> f64 {
    (procedure.base_units + procedure.unit_qty).max(1.0)
}

/// OD `Procedure.ProcFeeTotal`.
pub fn proc_fee_total(procedure: &Procedure) -> f64 {
    procedure.fee * quantity(procedure)
}

/// Outcome of the upstream discount computation. `amount` is always the
/// positive discount to book; callers post it as a negative `Adjustment`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DiscountResult {
    pub amount: f64,
    pub exceeds_annual_max: bool,
    pub exceeds_frequency: bool,
}

impl DiscountResult {
    const NONE: Self = Self {
        amount: 0.0,
        exceeds_annual_max: false,
        exceeds_frequency: false,
    };
}

/// Parameters gathered from the store by the caller.
#[derive(Clone, Debug)]
pub struct DiscountInput<'a> {
    pub procedure: &'a Procedure,
    pub plan: &'a DiscountPlan,
    pub subscription: &'a DiscountPlanSub,
    /// Amount from the plan's fee schedule for this code/provider, if any.
    pub discount_fee: Option<f64>,
    /// Absolute total of prior negative adjustments of the plan's type in the
    /// annual bucket (OD `Adjustments.GetTotForPatByType`).
    pub running_total: f64,
    /// The patient's other procedures, used for frequency-limit counts.
    pub history: &'a [Procedure],
    pub frequency_codes: &'a FrequencyCodeSets,
    /// `DateTime.Today` in OD's date helpers.
    pub today: NaiveDate,
}

/// Mirrors `Procedures.GetDiscountAmountForDiscountPlanAndValidate` plus the
/// fee computation in `GetDiscountAmountForDiscountPlan`.
pub fn discount_amount(input: &DiscountInput<'_>) -> DiscountResult {
    let proc = input.procedure;
    let sub = input.subscription;
    let (range_start, range_end) =
        annual_max_range(sub.date_effective, sub.date_term, input.today);
    if proc.proc_date < range_start || proc.proc_date > range_end {
        return DiscountResult::NONE;
    }
    let (bucket_start, bucket_end) =
        annual_bucket(proc.proc_date, sub.date_effective, sub.date_term, input.today);

    // Upstream checks frequency before it looks up a fee. A limit of -1 is
    // unrestricted; the count only includes completed procedures, as OD's
    // GetCompletedForDateRange only returns those.
    let limits = input.plan.frequency_limits();
    let mut exceeds_frequency = false;
    for (category, limit) in limits.iter().copied().enumerate() {
        if limit < 0 {
            continue;
        }
        let codes = &input.frequency_codes.sets[category];
        if codes.is_empty() || !codes.contains(&proc.code_num) {
            continue;
        }
        let count = input
            .history
            .iter()
            .filter(|p| {
                p.proc_num != proc.proc_num
                    && p.proc_status.is_completed()
                    && codes.contains(&p.code_num)
                    && p.proc_date >= bucket_start
                    && p.proc_date <= bucket_end
            })
            .count() as i32;
        if count >= limit {
            exceeds_frequency = true;
            break;
        }
    }
    if exceeds_frequency {
        return DiscountResult {
            amount: 0.0,
            exceeds_annual_max: false,
            exceeds_frequency: true,
        };
    }

    // OD treats a missing fee or the sentinel -1 as "no discount".
    let Some(discount_fee) = input.discount_fee.filter(|fee| *fee >= 0.0) else {
        return DiscountResult::NONE;
    };
    let units = quantity(proc);
    let estimated = proc_fee_total(proc) - discount_fee * units;
    if estimated <= 0.0 {
        return DiscountResult::NONE;
    }

    if input.plan.annual_max < 0.0 {
        return DiscountResult {
            amount: estimated,
            ..DiscountResult::NONE
        };
    }
    if input.running_total > input.plan.annual_max {
        return DiscountResult {
            amount: 0.0,
            exceeds_annual_max: true,
            exceeds_frequency: false,
        };
    }
    if input.running_total + estimated > input.plan.annual_max {
        return DiscountResult {
            amount: (input.plan.annual_max - input.running_total).max(0.0),
            exceeds_annual_max: true,
            exceeds_frequency: false,
        };
    }
    DiscountResult {
        amount: estimated,
        ..DiscountResult::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use od_domain::{ProcedureStatus, Pref};

    fn day(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn proc(fee: f64, units: f64, date: NaiveDate, code: Id) -> Procedure {
        Procedure {
            proc_num: 10,
            pat_num: 1,
            code_num: code,
            proc_date: date,
            proc_status: ProcedureStatus::Complete,
            fee,
            unit_qty: units,
            provider_num: 1,
            ..Default::default()
        }
    }

    fn plan(annual_max: f64) -> DiscountPlan {
        DiscountPlan {
            discount_plan_num: 5,
            def_num: 1,
            annual_max,
            ..Default::default()
        }
    }

    fn sub(effective: NaiveDate) -> DiscountPlanSub {
        DiscountPlanSub {
            discount_sub_num: 7,
            discount_plan_num: 5,
            pat_num: 1,
            date_effective: Some(effective),
            ..Default::default()
        }
    }

    #[test]
    fn annual_range_uses_current_year_for_open_ended_subscriptions() {
        // A real effective date is used verbatim; only OD's zero-style dates
        // are projected onto the current year.
        let (start, end) = annual_max_range(Some(day(2024, 6, 15)), None, day(2026, 8, 1));
        assert_eq!(start, day(2024, 6, 15));
        assert_eq!(end, day(9999, 12, 31));

        let zero_date = NaiveDate::from_ymd_opt(1, 6, 15).unwrap();
        let (start, end) = annual_max_range(Some(zero_date), None, day(2026, 8, 1));
        assert_eq!(start, day(2026, 6, 15));
        assert_eq!(end, day(9999, 12, 31));

        let (start, end) = annual_max_range(None, Some(day(2025, 1, 31)), day(2026, 8, 1));
        assert_eq!(start, day(2026, 1, 1));
        assert_eq!(end, day(2025, 1, 31));
    }

    #[test]
    fn annual_bucket_is_one_year_and_rolls_forward() {
        let effective = Some(day(2024, 6, 15));
        let (start, end) = annual_bucket(day(2026, 8, 1), effective, None, day(2026, 8, 1));
        assert_eq!(start, day(2026, 6, 15));
        assert_eq!(end, day(2027, 6, 14));

        let (start, end) = annual_bucket(day(2026, 1, 2), effective, None, day(2026, 8, 1));
        assert_eq!(start, day(2025, 6, 15));
        assert_eq!(end, day(2026, 6, 14));
    }

    #[test]
    fn discount_uses_reduced_fee_and_quantity() {
        // Upstream vector: usual fee 100, plan fee 95, one unit -> 5.
        let p = proc(100.0, 1.0, day(2026, 8, 1), 1);
        let input = DiscountInput {
            procedure: &p,
            plan: &plan(-1.0),
            subscription: &sub(day(2024, 1, 1)),
            discount_fee: Some(95.0),
            running_total: 0.0,
            history: &[],
            frequency_codes: &FrequencyCodeSets::default(),
            today: day(2026, 8, 1),
        };
        assert_eq!(discount_amount(&input), DiscountResult {
            amount: 5.0,
            ..DiscountResult::NONE
        });

        // Two units double both the charged fee and the discount.
        let p2 = proc(100.0, 2.0, day(2026, 8, 1), 1);
        let input2 = DiscountInput { procedure: &p2, ..input.clone() };
        assert_eq!(discount_amount(&input2).amount, 10.0);
    }

    #[test]
    fn no_discount_outside_range_or_without_a_cheaper_fee() {
        let p = proc(100.0, 1.0, day(2026, 8, 1), 1);
        let mut input = DiscountInput {
            procedure: &p,
            plan: &plan(-1.0),
            subscription: &sub(day(2024, 1, 1)),
            discount_fee: Some(95.0),
            running_total: 0.0,
            history: &[],
            frequency_codes: &FrequencyCodeSets::default(),
            today: day(2026, 8, 1),
        };
        let active = sub(day(2024, 1, 1));
        let ended = DiscountPlanSub {
            date_term: Some(day(2026, 7, 31)),
            ..active.clone()
        };
        input.subscription = &ended;
        assert_eq!(discount_amount(&input).amount, 0.0);

        input.subscription = &active;
        input.discount_fee = Some(120.0);
        assert_eq!(discount_amount(&input).amount, 0.0);
        input.discount_fee = None;
        assert_eq!(discount_amount(&input).amount, 0.0);
    }

    #[test]
    fn annual_max_clamps_and_then_blocks() {
        let p = proc(100.0, 1.0, day(2026, 8, 1), 1);
        let mut input = DiscountInput {
            procedure: &p,
            plan: &plan(25.0),
            subscription: &sub(day(2024, 1, 1)),
            discount_fee: Some(70.0), // estimated discount 30
            running_total: 0.0,
            history: &[],
            frequency_codes: &FrequencyCodeSets::default(),
            today: day(2026, 8, 1),
        };
        assert_eq!(discount_amount(&input), DiscountResult {
            amount: 25.0,
            exceeds_annual_max: true,
            exceeds_frequency: false,
        });
        input.running_total = 20.0;
        assert_eq!(discount_amount(&input).amount, 5.0);
        input.running_total = 25.0;
        assert_eq!(discount_amount(&input).amount, 0.0);
    }

    #[test]
    fn frequency_limit_blocks_when_category_is_used_up() {
        let date = day(2026, 8, 1);
        let p = proc(100.0, 1.0, date, 1);
        let mut plan = plan(-1.0);
        plan.exam_freq_limit = 2;
        let history = vec![
            { let mut h = proc(100.0, 1.0, date, 1); h.proc_num = 1; h },
            { let mut h = proc(100.0, 1.0, date, 1); h.proc_num = 2; h },
        ];
        let prefs = vec![Pref {
            pref_num: 1,
            pref_name: "DiscountPlanExamCodes".into(),
            pref_value: "D9999,D1110".into(),
        }];
        let codes = vec![ProcedureCode {
            code_num: 1,
            code: "D1110".into(),
            ..Default::default()
        }];
        let sets = FrequencyCodeSets::from_prefs(&prefs, &codes);
        assert_eq!(sets.sets[0], vec![1]);
        let mut planned = history.clone();
        planned[0].proc_status = ProcedureStatus::TreatmentPlanned;
        let input = DiscountInput {
            procedure: &p,
            plan: &plan,
            subscription: &sub(day(2024, 1, 1)),
            discount_fee: Some(95.0),
            running_total: 0.0,
            history: &history,
            frequency_codes: &sets,
            today: date,
        };
        assert_eq!(discount_amount(&input), DiscountResult {
            amount: 0.0,
            exceeds_annual_max: false,
            exceeds_frequency: true,
        });

        // A planned procedure does not count toward the limit.
        let input2 = DiscountInput { history: &planned, ..input };
        assert_eq!(discount_amount(&input2).amount, 5.0);
    }
}
