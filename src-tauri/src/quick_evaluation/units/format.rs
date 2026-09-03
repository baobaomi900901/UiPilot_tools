const SIGNIFICANT_DIGITS: i32 = 6;
const FIXED_MINIMUM: f64 = 0.000_001;
const FIXED_MAXIMUM: f64 = 1_000_000_000.0;

pub(super) fn format_number(value: f64) -> String {
    debug_assert!(value.is_finite());
    if value == 0.0 {
        return "0".into();
    }

    let absolute = value.abs();
    if (FIXED_MINIMUM..FIXED_MAXIMUM).contains(&absolute) {
        format_fixed(value)
    } else {
        format_scientific(value)
    }
}

fn format_fixed(value: f64) -> String {
    let exponent = value.abs().log10().floor() as i32;
    let decimal_places = (SIGNIFICANT_DIGITS - 1 - exponent).max(0) as usize;
    let rounded = if exponent >= SIGNIFICANT_DIGITS {
        let factor = 10_f64.powi(exponent - SIGNIFICANT_DIGITS + 1);
        (value / factor).round() * factor
    } else {
        let factor = 10_f64.powi(SIGNIFICANT_DIGITS - 1 - exponent);
        (value * factor).round() / factor
    };
    trim_fraction(format!("{rounded:.decimal_places$}"))
}

fn format_scientific(value: f64) -> String {
    let formatted = format!("{value:e}");
    let (_, exponent) = formatted
        .split_once('e')
        .expect("scientific formatting always has an exponent");
    let mut exponent = exponent
        .parse::<i32>()
        .expect("scientific formatting always has an integer exponent");
    let scaled = if exponent >= SIGNIFICANT_DIGITS - 1 {
        value / 10_f64.powi(exponent - SIGNIFICANT_DIGITS + 1)
    } else if exponent >= -303 {
        value * 10_f64.powi(SIGNIFICANT_DIGITS - 1 - exponent)
    } else {
        let shifted = value * 1e300;
        shifted * 10_f64.powi(SIGNIFICANT_DIGITS - 1 - (exponent + 300))
    };
    let rounded_digits = scaled.round();
    let mantissa = if rounded_digits.abs() >= 1_000_000.0 {
        exponent += 1;
        rounded_digits / 1_000_000.0
    } else {
        rounded_digits / 100_000.0
    };
    format!("{}e{exponent}", trim_fraction(format!("{mantissa:.5}")))
}

fn trim_fraction(mut value: String) -> String {
    if value.contains('.') {
        while value.ends_with('0') {
            value.pop();
        }
        if value.ends_with('.') {
            value.pop();
        }
    }
    if value == "-0" {
        "0".into()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::format_number;

    #[test]
    fn formats_fixed_and_scientific_thresholds_with_six_significant_digits() {
        for (value, expected) in [
            (0.000_001, "0.000001"),
            (0.000_000_999_999, "9.99999e-7"),
            (999_999_999.0, "1000000000"),
            (1_000_000_000.0, "1e9"),
            (1_234_565_000.0, "1.23457e9"),
            (-1_234_565_000.0, "-1.23457e9"),
            (9_999_995_000.0, "1e10"),
            (-1_234_567_000_000.0, "-1.23457e12"),
            (1e-308, "1e-308"),
            (f64::from_bits(1), "4.94066e-324"),
            (f64::MAX, "1.79769e308"),
        ] {
            assert_eq!(format_number(value), expected, "value={value:?}");
        }
    }

    #[test]
    fn rounds_ties_trims_fractional_zeroes_and_canonicalizes_negative_zero() {
        for (value, expected) in [
            (1.234_375, "1.23438"),
            (1_234_565.0, "1234570"),
            (-1_234_565.0, "-1234570"),
            (12.340_0, "12.34"),
            (1_000.0, "1000"),
            (0.0, "0"),
            (-0.0, "0"),
        ] {
            assert_eq!(format_number(value), expected, "value={value:?}");
        }
    }
}
