use super::MAX_QUERY_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NumberGrammar {
    Number,
    NonNegativeInteger,
}

pub(crate) fn canonicalize_number(source: &str, grammar: NumberGrammar) -> Option<String> {
    if source.is_empty() || source.len() > MAX_QUERY_BYTES {
        return None;
    }

    let bytes = source.as_bytes();
    let mut index = 0;
    if matches!(bytes.first(), Some(b'+') | Some(b'-')) {
        if grammar == NumberGrammar::NonNegativeInteger {
            return None;
        }
        index += 1;
    }

    let integer_start = index;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    let integer_digits = index - integer_start;
    let mut grouped = false;
    if bytes.get(index) == Some(&b',') {
        if !(1..=3).contains(&integer_digits) {
            return None;
        }
        grouped = true;
        while bytes.get(index) == Some(&b',') {
            index += 1;
            let group_start = index;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
            if index - group_start != 3 {
                return None;
            }
        }
    }

    let has_integer = integer_digits > 0;
    let mut has_decimal = false;
    if bytes.get(index) == Some(&b'.') {
        if grammar == NumberGrammar::NonNegativeInteger {
            return None;
        }
        has_decimal = true;
        index += 1;
        let fraction_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if !has_integer && fraction_start == index {
            return None;
        }
    }

    if index != bytes.len() || (!has_integer && !has_decimal) {
        return None;
    }

    let canonical = if grouped {
        source
            .chars()
            .filter(|character| *character != ',')
            .collect()
    } else {
        source.to_owned()
    };
    canonical
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .map(|_| canonical)
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_number, NumberGrammar};

    #[test]
    fn canonicalizes_every_approved_number_shape() {
        for (source, expected) in [
            ("12", "12"),
            ("-40", "-40"),
            ("+40", "+40"),
            (".5", ".5"),
            ("-.5", "-.5"),
            ("1.", "1."),
            ("1.25", "1.25"),
            ("1,000.5", "1000.5"),
            ("-1,000.5", "-1000.5"),
            ("1,000.", "1000."),
            ("12,345,678", "12345678"),
            ("0001", "0001"),
        ] {
            assert_eq!(
                canonicalize_number(source, NumberGrammar::Number).as_deref(),
                Some(expected),
                "source={source}"
            );
        }
    }

    #[test]
    fn rejects_non_english_or_malformed_number_shapes() {
        for source in [
            "", "+", "-", ".", "1e6", "1,5", "1,23", "12,34", "1234,567", "1,0000", "1,000,00",
            "1.000,5", "NaN", "nan", "inf", "Infinity", "∞", "１２", "١٢", "0x10", " 1", "1 ",
            "1_000", "++1", "--1", "1..0",
        ] {
            assert_eq!(
                canonicalize_number(source, NumberGrammar::Number),
                None,
                "source={source}"
            );
        }
    }

    #[test]
    fn enforces_finite_and_bounded_number_literals() {
        let finite_at_limit = "0".repeat(400);
        let overflowing_at_limit = "9".repeat(400);
        let beyond_limit = "0".repeat(401);

        assert_eq!(
            canonicalize_number(&finite_at_limit, NumberGrammar::Number).as_deref(),
            Some(finite_at_limit.as_str())
        );
        assert_eq!(
            canonicalize_number(&overflowing_at_limit, NumberGrammar::Number),
            None
        );
        assert_eq!(
            canonicalize_number(&beyond_limit, NumberGrammar::Number),
            None
        );
    }

    #[test]
    fn integer_subset_accepts_only_unsigned_integer_forms() {
        for (source, expected) in [
            ("0", "0"),
            ("12", "12"),
            ("1,000", "1000"),
            ("0001", "0001"),
        ] {
            assert_eq!(
                canonicalize_number(source, NumberGrammar::NonNegativeInteger).as_deref(),
                Some(expected),
                "source={source}"
            );
        }

        for source in ["+1", "-1", ".5", "1.0", "1.", "1e3"] {
            assert_eq!(
                canonicalize_number(source, NumberGrammar::NonNegativeInteger),
                None,
                "source={source}"
            );
        }
    }
}
