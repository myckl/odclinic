//! Faithful static payment-plan charge generation.
//!
//! Mirrors the 24_3 C# path `PayPlanEdit.CalcPeriodRate` →
//! `CalcNextPeriodDate` / `GetOrdinalOfMonth` → `CreateScheduleCharges` used by
//! `FormPayPlan`'s **Create Schedule** button. The caller gathers the plan and
//! its existing charges; everything here is pure so OD's exact amortisation
//! arithmetic can be unit tested without a database.
//!
//! Unlike the earlier even-split stub, APR now produces real periodic interest:
//! a charge's interest is `round(remaining principal * period rate, 2)` once its
//! due date reaches `DateInterestStart`, and its principal portion is
//! `PayAmt - interest`. The final charge absorbs whatever principal remains.
//! A non-zero `DownPayment` is issued first, dated **today** as OD does.
//!
//! Dynamic plans, production links, prepayments and the automatic issuer job
//! (`IssueChargesDueForDynamicPaymentPlans`) are deliberately out of scope.

use chrono::{Datelike, Duration, Months, NaiveDate};
use od_domain::{PayPlan, PayPlanCharge, PayPlanChargeType, PayPlanFrequency};

/// OD's `payPlanChargesCeiling` in `CreateScheduleCharges`. The ceiling keeps a
/// bad term combination (payment smaller than one period's interest) from
/// looping forever.
pub const MAX_PLAN_CHARGES: usize = 2000;

/// OD `PayPlanEdit.CalcPeriodRate`: `APR / 100 / periods-per-year`.
pub fn period_rate(apr: f64, frequency: PayPlanFrequency) -> f64 {
    if apr == 0.0 {
        return 0.0;
    }
    let periods = match frequency {
        PayPlanFrequency::Weekly => 52.0,
        PayPlanFrequency::EveryOtherWeek => 26.0,
        PayPlanFrequency::OrdinalWeekday | PayPlanFrequency::Monthly => 12.0,
        PayPlanFrequency::Quarterly => 4.0,
    };
    apr / 100.0 / periods
}

/// .NET's `Math.Round(value, 2)` uses banker's rounding by default. Using the
/// same rule matters on exact half-cent interest amounts.
pub(crate) fn round2(value: f64) -> f64 {
    (value * 100.0).round_ties_even() / 100.0
}

fn add_months(date: NaiveDate, months: i64) -> NaiveDate {
    if months < 0 {
        return date;
    }
    date.checked_add_months(Months::new(months as u32))
        .unwrap_or(date)
}

/// OD `PayPlanEdit.GetOrdinalOfMonth`: the 3rd Friday returns 3; a 5th is
/// deliberately treated as a 4th, matching upstream's day-28 check.
fn ordinal_of_month(date: NaiveDate) -> i64 {
    if (date - Duration::days(28)).month() == date.month() {
        4
    } else if (date - Duration::days(21)).month() == date.month() {
        4
    } else if (date - Duration::days(14)).month() == date.month() {
        3
    } else if (date - Duration::days(7)).month() == date.month() {
        2
    } else {
        1
    }
}

/// OD `PayPlanEdit.CalcNextPeriodDate`. `period_num` is zero-based.
pub fn next_period_date(
    first_date: NaiveDate,
    period_num: i64,
    frequency: PayPlanFrequency,
) -> NaiveDate {
    match frequency {
        PayPlanFrequency::Weekly => first_date + Duration::days(7 * period_num),
        PayPlanFrequency::EveryOtherWeek => first_date + Duration::days(14 * period_num),
        PayPlanFrequency::Monthly => add_months(first_date, period_num),
        PayPlanFrequency::Quarterly => add_months(first_date, 3 * period_num),
        PayPlanFrequency::OrdinalWeekday => {
            // "First/second/etc Mon/Tue/etc of the month" — find the first
            // matching weekday in the target month, then add whole weeks.
            let rough = add_months(first_date, period_num);
            let wanted = first_date.weekday().num_days_from_sunday() as i32;
            let first_of_month = NaiveDate::from_ymd_opt(rough.year(), rough.month(), 1)
                .unwrap_or(rough);
            let first_weekday = first_of_month.weekday().num_days_from_sunday() as i32;
            let day = if first_weekday == wanted {
                1
            } else if first_weekday < wanted {
                wanted - first_weekday + 1
            } else {
                7 - (first_weekday - wanted) + 1
            };
            let anchor = NaiveDate::from_ymd_opt(rough.year(), rough.month(), day as u32)
                .unwrap_or(first_of_month);
            anchor + Duration::days(7 * (ordinal_of_month(first_date) - 1))
        }
    }
}

fn charge(plan: &PayPlan, date: NaiveDate, principal: f64, interest: f64, note: &str) -> PayPlanCharge {
    PayPlanCharge {
        pay_plan_num: plan.pay_plan_num,
        guarantor: plan.guarantor,
        pat_num: plan.pat_num,
        charge_date: date,
        principal,
        interest,
        note: note.to_string(),
        charge_type: PayPlanChargeType::Debit,
        ..Default::default()
    }
}

/// Build OD's static charge schedule, including the down payment.
///
/// When `date_interest_start` is absent the first payment date is used, so an
/// APR always takes effect from the first periodic charge. Invalid terms (a
/// period payment too small to reduce principal) return no charges at all,
/// mirroring upstream's "clear all debits and mark terms invalid" branch.
pub fn generate_plan_charges(plan: &PayPlan, today: NaiveDate) -> Vec<PayPlanCharge> {
    let count = plan.number_of_payments;
    if count <= 0 || plan.completed_amt <= 0.0 {
        return Vec::new();
    }
    let period_payment = if plan.pay_amt > 0.0 {
        plan.pay_amt
    } else {
        plan.completed_amt / count as f64
    };
    let rate = period_rate(plan.apr, plan.charge_frequency);
    let first_payment = plan.date_pay_plan_start.unwrap_or(plan.pay_plan_date);
    let interest_start = plan.date_interest_start.unwrap_or(first_payment);
    let down = plan.down_payment.max(0.0).min(plan.completed_amt);

    let mut remaining = plan.completed_amt - down;
    let mut previous = remaining;
    let mut charges = Vec::new();
    if down > 0.0 {
        charges.push(charge(plan, today, round2(down), 0.0, "Down payment"));
    }
    let down_count = charges.len();

    let mut period = 0_i64;
    while remaining > 0.0 && charges.len() - down_count < MAX_PLAN_CHARGES {
        // A negative/zero principal would increase the balance. OD clears all
        // debits and flags the terms invalid in this case.
        if remaining > previous + 1e-9 || period_payment <= 0.0 {
            return Vec::new();
        }
        previous = remaining;

        let date = next_period_date(first_payment, period, plan.charge_frequency);
        let interest = if date >= interest_start {
            round2(remaining * rate)
        } else {
            0.0
        };
        let mut principal = period_payment - interest;
        let number = charges.len() - down_count;
        if (count > 0 && number as i64 == count as i64 - 1)
            || (remaining + interest <= period_payment)
        {
            principal = remaining;
        }
        if principal <= 0.0 {
            return Vec::new();
        }
        remaining -= principal;
        charges.push(charge(
            plan,
            date,
            round2(principal),
            round2(interest),
            &format!("Plan charge {} of {}", number + 1, count),
        ));
        period += 1;
    }
    charges
}

/// Why an existing plan must not be given another generated schedule.
///
/// This is the local guard for OD's `PayPlanCharges.Sync` warning: repeating
/// "Generate charges" would otherwise append a duplicate set of debits, which
/// is especially harmful on a live billing database.
pub fn charge_generation_block(
    plan: &PayPlan,
    existing: &[PayPlanCharge],
) -> Option<&'static str> {
    if plan.is_closed {
        return Some("This payment plan is closed.");
    }
    if plan.is_locked {
        return Some("This payment plan is locked.");
    }
    if existing.iter().any(|c| {
        c.pay_plan_num == plan.pay_plan_num && c.charge_type == PayPlanChargeType::Debit
    }) {
        return Some("Charges have already been generated for this plan.");
    }
    if plan.number_of_payments <= 0 {
        return Some("Set a payment count before generating charges.");
    }
    if plan.completed_amt <= 0.0 {
        return Some("Set a plan amount before generating charges.");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn amortized_plan() -> PayPlan {
        PayPlan {
            pay_plan_num: 7,
            pat_num: 3,
            guarantor: 3,
            pay_plan_date: date(2026, 1, 1),
            date_pay_plan_start: Some(date(2026, 1, 1)),
            completed_amt: 1000.0,
            pay_amt: 250.0,
            number_of_payments: 5,
            apr: 12.0,
            charge_frequency: PayPlanFrequency::Monthly,
            ..Default::default()
        }
    }

    #[test]
    fn period_rate_follows_od_divisors() {
        assert_eq!(period_rate(12.0, PayPlanFrequency::Monthly), 0.01);
        assert_eq!(period_rate(12.0, PayPlanFrequency::Quarterly), 0.03);
        assert_eq!(period_rate(12.0, PayPlanFrequency::Weekly), 12.0 / 100.0 / 52.0);
        assert_eq!(period_rate(12.0, PayPlanFrequency::EveryOtherWeek), 12.0 / 100.0 / 26.0);
        assert_eq!(period_rate(12.0, PayPlanFrequency::OrdinalWeekday), 0.01);
        assert_eq!(period_rate(0.0, PayPlanFrequency::Monthly), 0.0);
    }

    #[test]
    fn next_period_date_matches_upstream_month_week_and_ordinal_steps() {
        let first = date(2026, 1, 15);
        assert_eq!(next_period_date(first, 0, PayPlanFrequency::Monthly), first);
        assert_eq!(next_period_date(first, 2, PayPlanFrequency::Monthly), date(2026, 3, 15));
        assert_eq!(next_period_date(first, 2, PayPlanFrequency::Quarterly), date(2026, 7, 15));
        assert_eq!(next_period_date(first, 3, PayPlanFrequency::Weekly), date(2026, 2, 5));
        assert_eq!(
            next_period_date(first, 2, PayPlanFrequency::EveryOtherWeek),
            date(2026, 2, 12)
        );
        // 15 Jan 2026 is the 3rd Thursday; the 3rd Thursday of March is the 19th.
        assert_eq!(
            next_period_date(first, 2, PayPlanFrequency::OrdinalWeekday),
            date(2026, 3, 19)
        );
        // Month-end dates clamp like .NET's DateTime.AddMonths.
        assert_eq!(
            next_period_date(date(2026, 1, 31), 1, PayPlanFrequency::Monthly),
            date(2026, 2, 28)
        );
    }

    #[test]
    fn amortization_matches_upstream_payplan_edit_vectors() {
        let charges = generate_plan_charges(&amortized_plan(), date(2026, 1, 1));
        let pairs: Vec<(f64, f64)> = charges
            .iter()
            .map(|c| (round2(c.principal), round2(c.interest)))
            .collect();
        // PayPlanEditTests.PayPlanEdit_GetListExpectedCharges_CorrectInterestCalculatedAfterDirectOverpayment
        assert_eq!(
            pairs,
            vec![
                (240.0, 10.0),
                (242.40, 7.60),
                (244.82, 5.18),
                (247.27, 2.73),
                (25.51, 0.26),
            ]
        );
        let principal: f64 = charges.iter().map(|c| c.principal).sum();
        assert_eq!(round2(principal), 1000.0);
        assert_eq!(charges.len(), 5);
        assert!(charges.iter().all(|c| c.charge_type == PayPlanChargeType::Debit));
    }

    #[test]
    fn down_payment_is_dated_today_and_reduces_financed_principal() {
        let mut plan = amortized_plan();
        plan.down_payment = 100.0;
        let charges = generate_plan_charges(&plan, date(2026, 5, 20));
        assert_eq!(charges[0].note, "Down payment");
        assert_eq!(charges[0].charge_date, date(2026, 5, 20));
        assert_eq!(charges[0].principal, 100.0);
        assert_eq!(charges[0].interest, 0.0);
        // Remaining 900 at 1% gives 9.00 interest on the first period.
        assert_eq!(charges[1].principal, 241.0);
        assert_eq!(charges[1].interest, 9.0);
        let total: f64 = charges.iter().map(|c| c.principal).sum();
        assert_eq!(round2(total), 1000.0);
    }

    #[test]
    fn interest_starts_only_at_the_configured_date() {
        let mut plan = amortized_plan();
        plan.date_interest_start = Some(date(2026, 3, 1));
        let charges = generate_plan_charges(&plan, date(2026, 1, 1));
        assert_eq!(charges[0].interest, 0.0);
        assert_eq!(charges[0].principal, 250.0);
        assert_eq!(charges[1].interest, 0.0);
        // 500.00 remaining at 1% from March onwards.
        assert_eq!(charges[2].interest, 5.0);
    }

    #[test]
    fn zero_apr_reproduces_even_charges_with_final_remainder() {
        let mut plan = amortized_plan();
        plan.apr = 0.0;
        plan.pay_amt = 150.0;
        plan.number_of_payments = 4;
        plan.completed_amt = 600.0;
        let charges = generate_plan_charges(&plan, date(2026, 1, 1));
        assert_eq!(charges.len(), 4);
        assert!(charges.iter().all(|c| c.interest == 0.0));
        assert!(charges.iter().take(3).all(|c| c.principal == 150.0));
        assert_eq!(charges[3].principal, 150.0);
        assert_eq!(charges[0].charge_date, date(2026, 1, 1));
        assert_eq!(charges[3].charge_date, date(2026, 4, 1));
    }

    #[test]
    fn invalid_terms_and_payment_smaller_than_interest_produce_no_charges() {
        let mut plan = amortized_plan();
        plan.pay_amt = 1.0; // less than one period's interest
        assert!(generate_plan_charges(&plan, date(2026, 1, 1)).is_empty());
        let mut bad = amortized_plan();
        bad.completed_amt = 0.0;
        assert!(generate_plan_charges(&bad, date(2026, 1, 1)).is_empty());
    }

    #[test]
    fn generation_is_blocked_for_closed_locked_or_already_generated_plans() {
        let plan = amortized_plan();
        let existing = generate_plan_charges(&plan, date(2026, 1, 1));
        assert!(charge_generation_block(&plan, &[]).is_none());
        assert!(charge_generation_block(&plan, &existing).is_some());

        let mut closed = plan.clone();
        closed.is_closed = true;
        assert!(charge_generation_block(&closed, &[]).is_some());
        let mut locked = plan.clone();
        locked.is_locked = true;
        assert!(charge_generation_block(&locked, &[]).is_some());

        let mut empty = plan;
        empty.number_of_payments = 0;
        assert!(charge_generation_block(&empty, &[]).is_some());
    }
}
