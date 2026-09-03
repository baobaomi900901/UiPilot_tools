mod catalog;
mod format;

use time::OffsetDateTime;

use super::{
    normalize_query,
    number::{canonicalize_number, NumberGrammar},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Dimension {
    Length,
    Mass,
    Area,
    Volume,
    Temperature,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LinearFactor(pub(crate) f64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TemperatureScale {
    Celsius,
    Fahrenheit,
    Kelvin,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum UnitDefinition {
    Linear(LinearFactor),
    Temperature(TemperatureScale),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub(crate) enum Unit {
    Millimeter,
    Centimeter,
    Meter,
    Kilometer,
    Inch,
    Foot,
    Yard,
    Mile,
    Milligram,
    Gram,
    Kilogram,
    Tonne,
    Ounce,
    Pound,
    ShortTon,
    SquareMillimeter,
    SquareCentimeter,
    SquareMeter,
    SquareKilometer,
    SquareInch,
    SquareFoot,
    SquareYard,
    SquareMile,
    Hectare,
    Acre,
    Milliliter,
    Liter,
    CubicCentimeter,
    CubicMeter,
    Teaspoon,
    Tablespoon,
    UsFluidOunce,
    UsCup,
    UsPint,
    UsQuart,
    UsGallon,
    CubicInch,
    CubicFoot,
    ImperialFluidOunce,
    ImperialPint,
    ImperialQuart,
    ImperialGallon,
    Celsius,
    Fahrenheit,
    Kelvin,
}

impl Unit {
    #[cfg(test)]
    pub(crate) const ALL: &'static [Self] = &[
        Self::Millimeter,
        Self::Centimeter,
        Self::Meter,
        Self::Kilometer,
        Self::Inch,
        Self::Foot,
        Self::Yard,
        Self::Mile,
        Self::Milligram,
        Self::Gram,
        Self::Kilogram,
        Self::Tonne,
        Self::Ounce,
        Self::Pound,
        Self::ShortTon,
        Self::SquareMillimeter,
        Self::SquareCentimeter,
        Self::SquareMeter,
        Self::SquareKilometer,
        Self::SquareInch,
        Self::SquareFoot,
        Self::SquareYard,
        Self::SquareMile,
        Self::Hectare,
        Self::Acre,
        Self::Milliliter,
        Self::Liter,
        Self::CubicCentimeter,
        Self::CubicMeter,
        Self::Teaspoon,
        Self::Tablespoon,
        Self::UsFluidOunce,
        Self::UsCup,
        Self::UsPint,
        Self::UsQuart,
        Self::UsGallon,
        Self::CubicInch,
        Self::CubicFoot,
        Self::ImperialFluidOunce,
        Self::ImperialPint,
        Self::ImperialQuart,
        Self::ImperialGallon,
        Self::Celsius,
        Self::Fahrenheit,
        Self::Kelvin,
    ];

    pub(crate) fn dimension(self) -> Dimension {
        catalog::spec(self).dimension
    }

    pub(crate) fn definition(self) -> UnitDefinition {
        catalog::spec(self).definition
    }

    pub(crate) fn symbol(self) -> &'static str {
        catalog::spec(self).symbol
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UnitIntent {
    pub(crate) canonical_value: String,
    pub(crate) source: Unit,
    pub(crate) target: Unit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UnitFailure {
    InvalidNumber,
    UnknownUnit,
    DimensionMismatch,
    BelowAbsoluteZero,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum UnitClassification {
    NoMatch,
    Invalid(UnitFailure),
    Ready(UnitIntent),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UnitOutput {
    pub(crate) title: String,
    pub(crate) subtitle: Option<String>,
    pub(crate) copy_text: String,
    pub(crate) refresh_at_utc: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UnitInvalid {
    pub(crate) reason: UnitFailure,
    pub(crate) title: String,
    pub(crate) explanation: String,
}

pub(crate) fn classify(source: &str) -> UnitClassification {
    let Some(normalized) = normalize_query(source) else {
        return UnitClassification::NoMatch;
    };
    let Some((source_operand, target_alias)) = split_conversion(&normalized) else {
        return UnitClassification::NoMatch;
    };

    let parsed_source = parse_source_operand(source_operand);
    let target = catalog::resolve_alias(target_alias);
    let source_is_known = matches!(
        parsed_source,
        SourceOperand::Ready { .. } | SourceOperand::KnownUnitInvalidNumber
    );
    if !source_is_known && target.is_none() {
        return UnitClassification::NoMatch;
    }

    let (canonical_value, source) = match parsed_source {
        SourceOperand::Ready {
            canonical_value,
            unit,
        } => (canonical_value, unit),
        SourceOperand::KnownUnitInvalidNumber | SourceOperand::InvalidNumber => {
            return UnitClassification::Invalid(UnitFailure::InvalidNumber);
        }
        SourceOperand::UnknownUnit => {
            return UnitClassification::Invalid(UnitFailure::UnknownUnit);
        }
    };
    let Some(target) = target else {
        return UnitClassification::Invalid(UnitFailure::UnknownUnit);
    };
    if source.dimension() != target.dimension() {
        return UnitClassification::Invalid(UnitFailure::DimensionMismatch);
    }

    UnitClassification::Ready(UnitIntent {
        canonical_value,
        source,
        target,
    })
}

pub(crate) fn evaluate(intent: UnitIntent) -> Result<UnitOutput, UnitInvalid> {
    if intent.source.dimension() != intent.target.dimension() {
        return Err(failure(UnitFailure::DimensionMismatch));
    }

    let source_definition = intent.source.definition();
    let target_definition = intent.target.definition();
    if let UnitDefinition::Temperature(source) = source_definition {
        if decimal_below_absolute_zero(&intent.canonical_value, source) {
            return Err(below_absolute_zero());
        }
    }

    let value = intent
        .canonical_value
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| failure(UnitFailure::InvalidNumber))?;
    let converted = match (source_definition, target_definition) {
        (UnitDefinition::Linear(source), UnitDefinition::Linear(target)) => {
            value * (source.0 / target.0)
        }
        (UnitDefinition::Temperature(source), UnitDefinition::Temperature(target)) => {
            convert_temperature(value, source, target)?
        }
        _ => return Err(failure(UnitFailure::DimensionMismatch)),
    };
    if !converted.is_finite() {
        return Err(failure(UnitFailure::InvalidNumber));
    }

    let title = format!(
        "{} {}",
        format::format_number(converted),
        intent.target.symbol()
    );
    Ok(UnitOutput {
        subtitle: Some(format!(
            "{} {} · {}",
            intent.canonical_value,
            intent.source.symbol(),
            dimension_label(intent.source.dimension())
        )),
        copy_text: title.clone(),
        title,
        refresh_at_utc: None,
    })
}

fn decimal_below_absolute_zero(source: &str, scale: TemperatureScale) -> bool {
    let Some(magnitude) = source.strip_prefix('-') else {
        return false;
    };
    match scale {
        TemperatureScale::Celsius => decimal_magnitude_exceeds(magnitude, "273.15"),
        TemperatureScale::Fahrenheit => decimal_magnitude_exceeds(magnitude, "459.67"),
        TemperatureScale::Kelvin => magnitude
            .bytes()
            .any(|byte| byte.is_ascii_digit() && byte != b'0'),
    }
}

fn decimal_magnitude_exceeds(value: &str, limit: &str) -> bool {
    let (value_integer, value_fraction) = value.split_once('.').unwrap_or((value, ""));
    let (limit_integer, limit_fraction) = limit.split_once('.').unwrap_or((limit, ""));
    let value_integer = value_integer.trim_start_matches('0');
    let limit_integer = limit_integer.trim_start_matches('0');

    match value_integer.len().cmp(&limit_integer.len()) {
        std::cmp::Ordering::Greater => return true,
        std::cmp::Ordering::Less => return false,
        std::cmp::Ordering::Equal => {}
    }
    match value_integer.cmp(limit_integer) {
        std::cmp::Ordering::Greater => return true,
        std::cmp::Ordering::Less => return false,
        std::cmp::Ordering::Equal => {}
    }

    let digits = value_fraction.len().max(limit_fraction.len());
    for index in 0..digits {
        let value_digit = value_fraction
            .as_bytes()
            .get(index)
            .copied()
            .unwrap_or(b'0');
        let limit_digit = limit_fraction
            .as_bytes()
            .get(index)
            .copied()
            .unwrap_or(b'0');
        match value_digit.cmp(&limit_digit) {
            std::cmp::Ordering::Greater => return true,
            std::cmp::Ordering::Less => return false,
            std::cmp::Ordering::Equal => {}
        }
    }
    false
}

pub(crate) fn failure(reason: UnitFailure) -> UnitInvalid {
    let (title, explanation) = match reason {
        UnitFailure::InvalidNumber => ("数值无效", "请输入有效的有限数值"),
        UnitFailure::UnknownUnit => ("无法识别单位", "请检查源单位和目标单位"),
        UnitFailure::DimensionMismatch => ("无法转换单位", "源单位与目标单位的维度不同"),
        UnitFailure::BelowAbsoluteZero => ("温度低于绝对零度", "输入温度不能低于绝对零度"),
    };
    UnitInvalid {
        reason,
        title: title.into(),
        explanation: explanation.into(),
    }
}

fn convert_temperature(
    value: f64,
    source: TemperatureScale,
    target: TemperatureScale,
) -> Result<f64, UnitInvalid> {
    let minimum = match source {
        TemperatureScale::Celsius => -273.15,
        TemperatureScale::Fahrenheit => -459.67,
        TemperatureScale::Kelvin => 0.0,
    };
    if value < minimum {
        return Err(below_absolute_zero());
    }

    Ok(match (source, target) {
        (TemperatureScale::Celsius, TemperatureScale::Celsius)
        | (TemperatureScale::Fahrenheit, TemperatureScale::Fahrenheit)
        | (TemperatureScale::Kelvin, TemperatureScale::Kelvin) => value,
        (TemperatureScale::Celsius, TemperatureScale::Fahrenheit) => value * 9.0 / 5.0 + 32.0,
        (TemperatureScale::Fahrenheit, TemperatureScale::Celsius) => (value - 32.0) * 5.0 / 9.0,
        (TemperatureScale::Celsius, TemperatureScale::Kelvin) => value + 273.15,
        (TemperatureScale::Kelvin, TemperatureScale::Celsius) => value - 273.15,
        (TemperatureScale::Fahrenheit, TemperatureScale::Kelvin) => (value + 459.67) * 5.0 / 9.0,
        (TemperatureScale::Kelvin, TemperatureScale::Fahrenheit) => value * 9.0 / 5.0 - 459.67,
    })
}

fn below_absolute_zero() -> UnitInvalid {
    failure(UnitFailure::BelowAbsoluteZero)
}

fn dimension_label(dimension: Dimension) -> &'static str {
    match dimension {
        Dimension::Length => "长度换算",
        Dimension::Mass => "质量换算",
        Dimension::Area => "面积换算",
        Dimension::Volume => "体积换算",
        Dimension::Temperature => "温度换算",
    }
}

fn split_conversion(source: &str) -> Option<(&str, &str)> {
    let bytes = source.as_bytes();
    for index in 1..bytes.len().saturating_sub(2) {
        if bytes[index - 1] == b' '
            && bytes[index..index + 2].eq_ignore_ascii_case(b"to")
            && bytes[index + 2] == b' '
        {
            let left = &source[..index - 1];
            let right = &source[index + 3..];
            if !left.is_empty() && !right.is_empty() {
                return Some((left, right));
            }
        }
    }
    None
}

enum SourceOperand {
    Ready { canonical_value: String, unit: Unit },
    KnownUnitInvalidNumber,
    InvalidNumber,
    UnknownUnit,
}

fn parse_source_operand(source: &str) -> SourceOperand {
    for boundary in source
        .char_indices()
        .map(|(index, _)| index)
        .skip(1)
        .chain(std::iter::once(source.len()))
    {
        let value_source = &source[..boundary];
        let unit_source = source[boundary..]
            .strip_prefix(' ')
            .unwrap_or(&source[boundary..]);
        if unit_source.is_empty() {
            continue;
        }
        let Some(unit) = catalog::resolve_alias(unit_source) else {
            continue;
        };
        if let Some(canonical_value) = canonicalize_number(value_source, NumberGrammar::Number) {
            return SourceOperand::Ready {
                canonical_value,
                unit,
            };
        }
    }

    if catalog::resolve_alias(source).is_some() {
        return SourceOperand::KnownUnitInvalidNumber;
    }

    if let Some((value_source, unit_source)) = source.split_once(' ') {
        if canonicalize_number(value_source, NumberGrammar::Number).is_some() {
            return SourceOperand::UnknownUnit;
        }
        return if catalog::resolve_alias(unit_source).is_some() {
            SourceOperand::KnownUnitInvalidNumber
        } else {
            SourceOperand::InvalidNumber
        };
    }

    for boundary in source
        .char_indices()
        .map(|(index, _)| index)
        .skip(1)
        .chain(std::iter::once(source.len()))
    {
        let value_source = &source[..boundary];
        let unit_source = &source[boundary..];
        if !unit_source.is_empty()
            && catalog::resolve_alias(unit_source).is_some()
            && looks_like_invalid_number(value_source)
        {
            return SourceOperand::KnownUnitInvalidNumber;
        }
    }

    for boundary in source
        .char_indices()
        .map(|(index, _)| index)
        .skip(1)
        .chain(std::iter::once(source.len()))
    {
        let value_source = &source[..boundary];
        if canonicalize_number(value_source, NumberGrammar::Number).is_some() {
            return SourceOperand::UnknownUnit;
        }
    }

    SourceOperand::InvalidNumber
}

fn looks_like_invalid_number(source: &str) -> bool {
    source
        .as_bytes()
        .first()
        .is_some_and(|byte| matches!(byte, b'+' | b'-' | b'.') || byte.is_ascii_digit())
        && source.bytes().all(|byte| {
            byte.is_ascii_digit()
                || matches!(byte, b'+' | b'-' | b'.' | b',' | b'e' | b'E')
                || byte.is_ascii_whitespace()
        })
}

#[cfg(test)]
mod tests {
    use super::{classify, evaluate, failure, Unit, UnitClassification, UnitFailure, UnitIntent};

    fn evaluate_ready(source: &str) -> super::UnitOutput {
        let UnitClassification::Ready(intent) = classify(source) else {
            panic!("expected ready unit intent for {source}");
        };
        evaluate(intent).unwrap()
    }

    #[test]
    fn converts_every_linear_anchor_through_its_dimension_base_unit() {
        for (source, expected_title) in [
            ("1 in to m", "0.0254 m"),
            ("0.0254 m to in", "1 in"),
            ("1 ft to in", "12 in"),
            ("1 yd to ft", "3 ft"),
            ("1 mi to yd", "1760 yd"),
            ("100 cm to m", "1 m"),
            ("3 ft to yd", "1 yd"),
            ("1000 mm to in", "39.3701 in"),
            ("1 lb to kg", "0.453592 kg"),
            ("0.45359237 kg to lb", "1 lb"),
            ("16 oz to lb", "1 lb"),
            ("1 ton to lb", "2000 lb"),
            ("1 tonne to kg", "1000 kg"),
            ("1 tonne to ton", "1.10231 ton"),
            ("1 ton to tonne", "0.907185 tonne"),
            ("1 m² to cm²", "10000 cm²"),
            ("1 ft² to in²", "144 in²"),
            ("1 yd² to ft²", "9 ft²"),
            ("1 acre to ft²", "43560 ft²"),
            ("1 ha to m²", "10000 m²"),
            ("1 mi² to acre", "640 acre"),
            ("1 L to mL", "1000 mL"),
            ("1 mL to cm³", "1 cm³"),
            ("1 gal to L", "3.78541 L"),
            ("1 L to gal", "0.264172 gal"),
            ("1 imp gal to L", "4.54609 L"),
            ("1 L to imp gal", "0.219969 imp gal"),
            ("1 cup to mL", "236.588 mL"),
            ("4 qt to gal", "1 gal"),
            ("8 pt to gal", "1 gal"),
            ("16 cup to gal", "1 gal"),
            ("128 fl oz to gal", "1 gal"),
            ("256 tbsp to gal", "1 gal"),
            ("768 tsp to gal", "1 gal"),
            ("1 gal to tsp", "768 tsp"),
            ("4 imp qt to imp gal", "1 imp gal"),
            ("8 imp pt to imp gal", "1 imp gal"),
            ("160 imp fl oz to imp gal", "1 imp gal"),
            ("1 in³ to mL", "16.3871 mL"),
            ("1 ft³ to in³", "1728 in³"),
        ] {
            assert_eq!(
                evaluate_ready(source).title,
                expected_title,
                "source={source}"
            );
        }
    }

    #[test]
    fn distinguishes_us_and_imperial_volume_mass_and_fluid_ounce_semantics() {
        assert_eq!(evaluate_ready("1 gal to L").title, "3.78541 L");
        assert_eq!(evaluate_ready("1 imp gal to L").title, "4.54609 L");
        assert_eq!(evaluate_ready("1 cup to mL").title, "236.588 mL");
        assert_eq!(evaluate_ready("1 ton to tonne").title, "0.907185 tonne");
        assert_eq!(evaluate_ready("1 tonne to ton").title, "1.10231 ton");

        let UnitClassification::Invalid(reason) = classify("1 oz to fl oz") else {
            panic!("mass ounce and fluid ounce must be a dimension mismatch");
        };
        let invalid = failure(reason);
        assert_eq!(invalid.reason, UnitFailure::DimensionMismatch);
        assert_eq!(invalid.title, "无法转换单位");
        assert_eq!(invalid.explanation, "源单位与目标单位的维度不同");
    }

    #[test]
    fn converts_all_six_temperature_directions_and_allows_absolute_zero_edges() {
        for (source, expected_title) in [
            ("0 C to F", "32 °F"),
            ("32 F to C", "0 °C"),
            ("0 C to K", "273.15 K"),
            ("273.15 K to C", "0 °C"),
            ("32 F to K", "273.15 K"),
            ("273.15 K to F", "32 °F"),
            ("-273.15 C to K", "0 K"),
            ("-459.67 F to K", "0 K"),
            ("0 K to C", "-273.15 °C"),
        ] {
            assert_eq!(
                evaluate_ready(source).title,
                expected_title,
                "source={source}"
            );
        }
    }

    #[test]
    fn rejects_each_temperature_scale_below_its_exact_absolute_zero_boundary() {
        for source in [
            "-273.150001 C to K",
            "-459.670001 F to C",
            "-0.000001 K to F",
        ] {
            let UnitClassification::Ready(intent) = classify(source) else {
                panic!("expected parsed temperature intent for {source}");
            };
            let invalid = evaluate(intent).unwrap_err();
            assert_eq!(
                invalid.reason,
                UnitFailure::BelowAbsoluteZero,
                "source={source}"
            );
            assert_eq!(invalid.title, "温度低于绝对零度", "source={source}");
            assert_eq!(
                invalid.explanation, "输入温度不能低于绝对零度",
                "source={source}"
            );
        }
    }

    #[test]
    fn rejects_sub_boundary_decimals_that_round_or_underflow_at_f64_precision() {
        let tiny_negative_kelvin = format!("-0.{}1 K to C", "0".repeat(386));
        assert_eq!(tiny_negative_kelvin.len(), 397);

        for source in [
            "-273.150000000000001 C to K".to_owned(),
            "-459.67000000000001 F to K".to_owned(),
            tiny_negative_kelvin,
        ] {
            let UnitClassification::Ready(intent) = classify(&source) else {
                panic!("expected parsed temperature intent for {source}");
            };
            let invalid = evaluate(intent).unwrap_err();
            assert_eq!(
                invalid.reason,
                UnitFailure::BelowAbsoluteZero,
                "source={source}"
            );
        }
    }

    #[test]
    fn exact_decimal_zero_and_absolute_zero_forms_remain_valid() {
        for (source, expected_title) in [
            ("-273.150000000000000 C to K", "0 K"),
            ("-459.67000000000000 F to K", "0 K"),
            ("-.0 K to C", "-273.15 °C"),
            ("+0 K to C", "-273.15 °C"),
            ("-000,000.000 K to C", "-273.15 °C"),
        ] {
            assert_eq!(
                evaluate_ready(source).title,
                expected_title,
                "source={source}"
            );
        }
    }

    #[test]
    fn rejects_cross_dimension_intents_even_if_constructed_after_classification() {
        let invalid = evaluate(UnitIntent {
            canonical_value: "1".into(),
            source: Unit::Kilogram,
            target: Unit::Liter,
        })
        .unwrap_err();

        assert_eq!(invalid, failure(UnitFailure::DimensionMismatch));
    }

    #[test]
    fn same_unit_and_same_system_conversions_use_the_common_precision_path() {
        assert_eq!(evaluate_ready("1.234567 m to m").title, "1.23457 m");
        assert_eq!(evaluate_ready("3 ft to yd").title, "1 yd");
        assert_eq!(evaluate_ready("8 cup to fl oz").title, "64 fl oz");
    }

    #[test]
    fn successful_outputs_match_the_shared_copy_and_chinese_dimension_contract() {
        for (source, expected_subtitle) in [
            ("1000 mm to in", "1000 mm · 长度换算"),
            ("1 kg to lb", "1 kg · 质量换算"),
            ("1 m² to ft²", "1 m² · 面积换算"),
            ("1 L to gal", "1 L · 体积换算"),
            ("32 F to C", "32 °F · 温度换算"),
        ] {
            let output = evaluate_ready(source);
            assert_eq!(output.subtitle.as_deref(), Some(expected_subtitle));
            assert_eq!(output.copy_text, output.title);
            assert_eq!(output.refresh_at_utc, None);
        }
    }

    #[test]
    fn every_owned_invalid_classification_matches_the_shared_failure_contract() {
        for (reason, expected_title, expected_explanation) in [
            (
                UnitFailure::InvalidNumber,
                "数值无效",
                "请输入有效的有限数值",
            ),
            (
                UnitFailure::UnknownUnit,
                "无法识别单位",
                "请检查源单位和目标单位",
            ),
            (
                UnitFailure::DimensionMismatch,
                "无法转换单位",
                "源单位与目标单位的维度不同",
            ),
            (
                UnitFailure::BelowAbsoluteZero,
                "温度低于绝对零度",
                "输入温度不能低于绝对零度",
            ),
        ] {
            let invalid = failure(reason);
            assert_eq!(invalid.reason, reason);
            assert_eq!(invalid.title, expected_title);
            assert_eq!(invalid.explanation, expected_explanation);
        }
    }
}
