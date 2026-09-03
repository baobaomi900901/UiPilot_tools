mod catalog;

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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum UnitClassification {
    NoMatch,
    Invalid(UnitFailure),
    Ready(UnitIntent),
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
