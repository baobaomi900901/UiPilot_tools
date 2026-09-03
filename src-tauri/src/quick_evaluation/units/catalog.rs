use super::{Dimension, LinearFactor, TemperatureScale, Unit, UnitDefinition};

const INCH_METERS: f64 = 0.0254;
const FOOT_METERS: f64 = INCH_METERS * 12.0;
const YARD_METERS: f64 = FOOT_METERS * 3.0;
const MILE_METERS: f64 = YARD_METERS * 1760.0;
const POUND_KILOGRAMS: f64 = 0.453_592_37;
const US_GALLON_CUBIC_METERS: f64 = 0.003_785_411_784;
const IMPERIAL_GALLON_CUBIC_METERS: f64 = 0.004_546_09;

pub(super) struct UnitSpec {
    pub(super) dimension: Dimension,
    pub(super) definition: UnitDefinition,
    pub(super) symbol: &'static str,
}

impl UnitSpec {
    const fn linear(dimension: Dimension, factor: f64, symbol: &'static str) -> Self {
        Self {
            dimension,
            definition: UnitDefinition::Linear(LinearFactor(factor)),
            symbol,
        }
    }

    const fn temperature(scale: TemperatureScale, symbol: &'static str) -> Self {
        Self {
            dimension: Dimension::Temperature,
            definition: UnitDefinition::Temperature(scale),
            symbol,
        }
    }
}

const CATALOG: [UnitSpec; 45] = [
    UnitSpec::linear(Dimension::Length, 0.001, "mm"),
    UnitSpec::linear(Dimension::Length, 0.01, "cm"),
    UnitSpec::linear(Dimension::Length, 1.0, "m"),
    UnitSpec::linear(Dimension::Length, 1000.0, "km"),
    UnitSpec::linear(Dimension::Length, INCH_METERS, "in"),
    UnitSpec::linear(Dimension::Length, FOOT_METERS, "ft"),
    UnitSpec::linear(Dimension::Length, YARD_METERS, "yd"),
    UnitSpec::linear(Dimension::Length, MILE_METERS, "mi"),
    UnitSpec::linear(Dimension::Mass, 0.000_001, "mg"),
    UnitSpec::linear(Dimension::Mass, 0.001, "g"),
    UnitSpec::linear(Dimension::Mass, 1.0, "kg"),
    UnitSpec::linear(Dimension::Mass, 1000.0, "tonne"),
    UnitSpec::linear(Dimension::Mass, POUND_KILOGRAMS / 16.0, "oz"),
    UnitSpec::linear(Dimension::Mass, POUND_KILOGRAMS, "lb"),
    UnitSpec::linear(Dimension::Mass, POUND_KILOGRAMS * 2000.0, "ton"),
    UnitSpec::linear(Dimension::Area, 0.001 * 0.001, "mm²"),
    UnitSpec::linear(Dimension::Area, 0.01 * 0.01, "cm²"),
    UnitSpec::linear(Dimension::Area, 1.0, "m²"),
    UnitSpec::linear(Dimension::Area, 1000.0 * 1000.0, "km²"),
    UnitSpec::linear(Dimension::Area, INCH_METERS * INCH_METERS, "in²"),
    UnitSpec::linear(Dimension::Area, FOOT_METERS * FOOT_METERS, "ft²"),
    UnitSpec::linear(Dimension::Area, YARD_METERS * YARD_METERS, "yd²"),
    UnitSpec::linear(Dimension::Area, MILE_METERS * MILE_METERS, "mi²"),
    UnitSpec::linear(Dimension::Area, 10_000.0, "ha"),
    UnitSpec::linear(
        Dimension::Area,
        FOOT_METERS * FOOT_METERS * 43_560.0,
        "acre",
    ),
    UnitSpec::linear(Dimension::Volume, 0.000_001, "mL"),
    UnitSpec::linear(Dimension::Volume, 0.001, "L"),
    UnitSpec::linear(Dimension::Volume, 0.01 * 0.01 * 0.01, "cm³"),
    UnitSpec::linear(Dimension::Volume, 1.0, "m³"),
    UnitSpec::linear(Dimension::Volume, US_GALLON_CUBIC_METERS / 768.0, "tsp"),
    UnitSpec::linear(Dimension::Volume, US_GALLON_CUBIC_METERS / 256.0, "tbsp"),
    UnitSpec::linear(Dimension::Volume, US_GALLON_CUBIC_METERS / 128.0, "fl oz"),
    UnitSpec::linear(Dimension::Volume, US_GALLON_CUBIC_METERS / 16.0, "cup"),
    UnitSpec::linear(Dimension::Volume, US_GALLON_CUBIC_METERS / 8.0, "pt"),
    UnitSpec::linear(Dimension::Volume, US_GALLON_CUBIC_METERS / 4.0, "qt"),
    UnitSpec::linear(Dimension::Volume, US_GALLON_CUBIC_METERS, "gal"),
    UnitSpec::linear(
        Dimension::Volume,
        INCH_METERS * INCH_METERS * INCH_METERS,
        "in³",
    ),
    UnitSpec::linear(
        Dimension::Volume,
        FOOT_METERS * FOOT_METERS * FOOT_METERS,
        "ft³",
    ),
    UnitSpec::linear(
        Dimension::Volume,
        IMPERIAL_GALLON_CUBIC_METERS / 160.0,
        "imp fl oz",
    ),
    UnitSpec::linear(
        Dimension::Volume,
        IMPERIAL_GALLON_CUBIC_METERS / 8.0,
        "imp pt",
    ),
    UnitSpec::linear(
        Dimension::Volume,
        IMPERIAL_GALLON_CUBIC_METERS / 4.0,
        "imp qt",
    ),
    UnitSpec::linear(Dimension::Volume, IMPERIAL_GALLON_CUBIC_METERS, "imp gal"),
    UnitSpec::temperature(TemperatureScale::Celsius, "°C"),
    UnitSpec::temperature(TemperatureScale::Fahrenheit, "°F"),
    UnitSpec::temperature(TemperatureScale::Kelvin, "K"),
];

pub(super) fn spec(unit: Unit) -> &'static UnitSpec {
    &CATALOG[unit as usize]
}

pub(super) fn resolve_alias(source: &str) -> Option<Unit> {
    let normalized = normalize_alias(source)?;

    let direct = match normalized.as_str() {
        "mm" | "millimeter" | "millimeters" | "millimetre" | "millimetres" => {
            Some(Unit::Millimeter)
        }
        "cm" | "centimeter" | "centimeters" | "centimetre" | "centimetres" => {
            Some(Unit::Centimeter)
        }
        "m" | "meter" | "meters" | "metre" | "metres" => Some(Unit::Meter),
        "km" | "kilometer" | "kilometers" | "kilometre" | "kilometres" => Some(Unit::Kilometer),
        "in" | "inch" | "inches" => Some(Unit::Inch),
        "ft" | "foot" | "feet" => Some(Unit::Foot),
        "yd" | "yard" | "yards" => Some(Unit::Yard),
        "mi" | "mile" | "miles" => Some(Unit::Mile),
        "mg" | "milligram" | "milligrams" => Some(Unit::Milligram),
        "g" | "gram" | "grams" => Some(Unit::Gram),
        "kg" | "kilogram" | "kilograms" => Some(Unit::Kilogram),
        "tonne" | "tonnes" | "metric ton" | "metric tons" | "metric tonne" | "metric tonnes" => {
            Some(Unit::Tonne)
        }
        "oz" | "ounce" | "ounces" => Some(Unit::Ounce),
        "lb" | "lbs" | "pound" | "pounds" => Some(Unit::Pound),
        "ton" | "tons" | "us ton" | "us tons" | "short ton" | "short tons" => Some(Unit::ShortTon),
        "ha" | "hectare" | "hectares" => Some(Unit::Hectare),
        "acre" | "acres" => Some(Unit::Acre),
        "ml" | "milliliter" | "milliliters" | "millilitre" | "millilitres" => {
            Some(Unit::Milliliter)
        }
        "l" | "liter" | "liters" | "litre" | "litres" => Some(Unit::Liter),
        "tsp" | "tsps" | "teaspoon" | "teaspoons" => Some(Unit::Teaspoon),
        "tbsp" | "tbsps" | "tablespoon" | "tablespoons" => Some(Unit::Tablespoon),
        "cup" | "cups" => Some(Unit::UsCup),
        "c" | "°c" | "celsius" | "degree celsius" | "degrees celsius" => Some(Unit::Celsius),
        "f" | "°f" | "fahrenheit" | "degree fahrenheit" | "degrees fahrenheit" => {
            Some(Unit::Fahrenheit)
        }
        "k" | "°k" | "kelvin" | "kelvins" | "degree kelvin" | "degrees kelvin" => {
            Some(Unit::Kelvin)
        }
        _ => None,
    };
    if direct.is_some() {
        return direct;
    }

    if let Some(unit) = resolve_area_alias(&normalized) {
        return Some(unit);
    }
    if let Some(unit) = resolve_cubic_alias(&normalized) {
        return Some(unit);
    }
    if let Some(unit) = resolve_volume_family(&normalized, VolumeSystem::Us) {
        return Some(unit);
    }

    let (prefix, family) = normalized.split_once(' ')?;
    let system = match prefix {
        "us" => VolumeSystem::Us,
        "imp" | "uk" | "imperial" => VolumeSystem::Imperial,
        _ => return None,
    };
    resolve_volume_family(family, system)
}

fn normalize_alias(source: &str) -> Option<String> {
    let source = source.trim_matches(' ');
    if source.is_empty()
        || source
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() && byte != b' ')
    {
        return None;
    }

    let mut normalized = String::with_capacity(source.len());
    let mut previous_was_space = false;
    for character in source.chars() {
        if character == ' ' {
            if !previous_was_space {
                normalized.push(' ');
            }
            previous_was_space = true;
        } else {
            normalized.push(character.to_ascii_lowercase());
            previous_was_space = false;
        }
    }
    Some(normalized)
}

fn resolve_area_alias(source: &str) -> Option<Unit> {
    for suffix in ["²", "^2", "2"] {
        if let Some(base) = source.strip_suffix(suffix) {
            return area_from_abbreviation(base);
        }
    }
    if let Some(base) = source.strip_prefix("sq") {
        if !base.is_empty() && !base.starts_with(' ') {
            if let Some(unit) = area_from_abbreviation(base) {
                return Some(unit);
            }
        }
    }
    if let Some(base) = source.strip_prefix("sq ") {
        return area_from_length_alias(base);
    }
    source
        .strip_prefix("square ")
        .and_then(area_from_length_alias)
}

fn area_from_abbreviation(source: &str) -> Option<Unit> {
    match source {
        "mm" => Some(Unit::SquareMillimeter),
        "cm" => Some(Unit::SquareCentimeter),
        "m" => Some(Unit::SquareMeter),
        "km" => Some(Unit::SquareKilometer),
        "in" => Some(Unit::SquareInch),
        "ft" => Some(Unit::SquareFoot),
        "yd" => Some(Unit::SquareYard),
        "mi" => Some(Unit::SquareMile),
        _ => None,
    }
}

fn area_from_length_alias(source: &str) -> Option<Unit> {
    match source {
        "mm" | "millimeter" | "millimeters" | "millimetre" | "millimetres" => {
            Some(Unit::SquareMillimeter)
        }
        "cm" | "centimeter" | "centimeters" | "centimetre" | "centimetres" => {
            Some(Unit::SquareCentimeter)
        }
        "m" | "meter" | "meters" | "metre" | "metres" => Some(Unit::SquareMeter),
        "km" | "kilometer" | "kilometers" | "kilometre" | "kilometres" => {
            Some(Unit::SquareKilometer)
        }
        "in" | "inch" | "inches" => Some(Unit::SquareInch),
        "ft" | "foot" | "feet" => Some(Unit::SquareFoot),
        "yd" | "yard" | "yards" => Some(Unit::SquareYard),
        "mi" | "mile" | "miles" => Some(Unit::SquareMile),
        _ => None,
    }
}

fn resolve_cubic_alias(source: &str) -> Option<Unit> {
    for suffix in ["³", "^3", "3"] {
        if let Some(base) = source.strip_suffix(suffix) {
            return cubic_from_abbreviation(base);
        }
    }
    source
        .strip_prefix("cubic ")
        .and_then(cubic_from_length_alias)
}

fn cubic_from_abbreviation(source: &str) -> Option<Unit> {
    match source {
        "cm" => Some(Unit::CubicCentimeter),
        "m" => Some(Unit::CubicMeter),
        "in" => Some(Unit::CubicInch),
        "ft" => Some(Unit::CubicFoot),
        _ => None,
    }
}

fn cubic_from_length_alias(source: &str) -> Option<Unit> {
    match source {
        "cm" | "centimeter" | "centimeters" | "centimetre" | "centimetres" => {
            Some(Unit::CubicCentimeter)
        }
        "m" | "meter" | "meters" | "metre" | "metres" => Some(Unit::CubicMeter),
        "in" | "inch" | "inches" => Some(Unit::CubicInch),
        "ft" | "foot" | "feet" => Some(Unit::CubicFoot),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum VolumeSystem {
    Us,
    Imperial,
}

fn resolve_volume_family(source: &str, system: VolumeSystem) -> Option<Unit> {
    match (source, system) {
        ("fl oz" | "floz" | "fluid ounce" | "fluid ounces", VolumeSystem::Us) => {
            Some(Unit::UsFluidOunce)
        }
        ("pt" | "pint" | "pints", VolumeSystem::Us) => Some(Unit::UsPint),
        ("qt" | "quart" | "quarts", VolumeSystem::Us) => Some(Unit::UsQuart),
        ("gal" | "gallon" | "gallons", VolumeSystem::Us) => Some(Unit::UsGallon),
        ("fl oz" | "floz" | "fluid ounce" | "fluid ounces", VolumeSystem::Imperial) => {
            Some(Unit::ImperialFluidOunce)
        }
        ("pt" | "pint" | "pints", VolumeSystem::Imperial) => Some(Unit::ImperialPint),
        ("qt" | "quart" | "quarts", VolumeSystem::Imperial) => Some(Unit::ImperialQuart),
        ("gal" | "gallon" | "gallons", VolumeSystem::Imperial) => Some(Unit::ImperialGallon),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::super::{
        classify, Dimension, LinearFactor, TemperatureScale, Unit, UnitClassification,
        UnitDefinition, UnitFailure, UnitIntent,
    };
    use super::resolve_alias;

    fn assert_aliases(unit: Unit, aliases: &[&str]) {
        for alias in aliases {
            assert_eq!(resolve_alias(alias), Some(unit), "alias={alias}");
        }
    }

    fn factor(unit: Unit) -> f64 {
        let UnitDefinition::Linear(LinearFactor(value)) = unit.definition() else {
            panic!("expected linear definition for {unit:?}");
        };
        value
    }

    fn assert_close(actual: f64, expected: f64) {
        let scale = actual.abs().max(expected.abs()).max(1.0);
        assert!(
            (actual - expected).abs() <= f64::EPSILON * scale * 8.0,
            "actual={actual:?}, expected={expected:?}"
        );
    }

    fn ready(source: &str) -> UnitIntent {
        let UnitClassification::Ready(intent) = classify(source) else {
            panic!("expected ready unit intent for {source}");
        };
        intent
    }

    #[test]
    fn catalog_is_closed_and_every_canonical_symbol_round_trips() {
        assert_eq!(Unit::ALL.len(), 45);
        let mut symbols = HashSet::new();

        for &unit in Unit::ALL {
            assert!(symbols.insert(unit.symbol()), "duplicate symbol: {unit:?}");
            assert_eq!(resolve_alias(unit.symbol()), Some(unit), "unit={unit:?}");
            match unit.definition() {
                UnitDefinition::Linear(LinearFactor(value)) => {
                    assert!(value.is_finite() && value > 0.0, "unit={unit:?}");
                    assert_ne!(unit.dimension(), Dimension::Temperature);
                }
                UnitDefinition::Temperature(_) => {
                    assert_eq!(unit.dimension(), Dimension::Temperature);
                }
            }
        }
    }

    #[test]
    fn resolves_length_and_mass_names_plurals_and_spellings() {
        for (unit, aliases) in [
            (
                Unit::Millimeter,
                &[
                    "mm",
                    "millimeter",
                    "millimeters",
                    "millimetre",
                    "millimetres",
                ][..],
            ),
            (
                Unit::Centimeter,
                &[
                    "cm",
                    "centimeter",
                    "centimeters",
                    "centimetre",
                    "centimetres",
                ],
            ),
            (Unit::Meter, &["m", "meter", "meters", "metre", "metres"]),
            (
                Unit::Kilometer,
                &["km", "kilometer", "kilometers", "kilometre", "kilometres"],
            ),
            (Unit::Inch, &["in", "inch", "inches"]),
            (Unit::Foot, &["ft", "foot", "feet"]),
            (Unit::Yard, &["yd", "yard", "yards"]),
            (Unit::Mile, &["mi", "mile", "miles"]),
            (Unit::Milligram, &["mg", "milligram", "milligrams"]),
            (Unit::Gram, &["g", "gram", "grams"]),
            (Unit::Kilogram, &["kg", "kilogram", "kilograms"]),
            (
                Unit::Tonne,
                &[
                    "tonne",
                    "tonnes",
                    "metric ton",
                    "metric tons",
                    "metric tonne",
                    "metric tonnes",
                ],
            ),
            (Unit::Ounce, &["oz", "ounce", "ounces"]),
            (Unit::Pound, &["lb", "lbs", "pound", "pounds"]),
            (
                Unit::ShortTon,
                &[
                    "ton",
                    "tons",
                    "us ton",
                    "us tons",
                    "short ton",
                    "short tons",
                ],
            ),
        ] {
            assert_aliases(unit, aliases);
        }
    }

    #[test]
    fn resolves_every_area_exponent_sq_square_and_name_form() {
        for (unit, abbreviation, names) in [
            (
                Unit::SquareMillimeter,
                "mm",
                &["millimeter", "millimeters", "millimetre", "millimetres"][..],
            ),
            (
                Unit::SquareCentimeter,
                "cm",
                &["centimeter", "centimeters", "centimetre", "centimetres"],
            ),
            (
                Unit::SquareMeter,
                "m",
                &["meter", "meters", "metre", "metres"],
            ),
            (
                Unit::SquareKilometer,
                "km",
                &["kilometer", "kilometers", "kilometre", "kilometres"],
            ),
            (Unit::SquareInch, "in", &["inch", "inches"]),
            (Unit::SquareFoot, "ft", &["foot", "feet"]),
            (Unit::SquareYard, "yd", &["yard", "yards"]),
            (Unit::SquareMile, "mi", &["mile", "miles"]),
        ] {
            for alias in [
                format!("{abbreviation}²"),
                format!("{abbreviation}^2"),
                format!("{abbreviation}2"),
                format!("sq {abbreviation}"),
                format!("sq{abbreviation}"),
                format!("square {abbreviation}"),
            ] {
                assert_eq!(resolve_alias(&alias), Some(unit), "alias={alias}");
            }
            for name in names {
                for alias in [format!("square {name}"), format!("sq {name}")] {
                    assert_eq!(resolve_alias(&alias), Some(unit), "alias={alias}");
                }
            }
        }

        assert_aliases(Unit::Hectare, &["ha", "hectare", "hectares"]);
        assert_aliases(Unit::Acre, &["acre", "acres"]);
    }

    #[test]
    fn resolves_metric_and_us_volume_names_and_cubic_forms() {
        for (unit, aliases) in [
            (
                Unit::Milliliter,
                &[
                    "ml",
                    "milliliter",
                    "milliliters",
                    "millilitre",
                    "millilitres",
                ][..],
            ),
            (Unit::Liter, &["l", "liter", "liters", "litre", "litres"]),
            (Unit::Teaspoon, &["tsp", "teaspoon", "teaspoons"]),
            (Unit::Tablespoon, &["tbsp", "tablespoon", "tablespoons"]),
            (
                Unit::UsFluidOunce,
                &["fl oz", "floz", "fluid ounce", "fluid ounces"],
            ),
            (Unit::UsCup, &["cup", "cups"]),
            (Unit::UsPint, &["pt", "pint", "pints"]),
            (Unit::UsQuart, &["qt", "quart", "quarts"]),
            (Unit::UsGallon, &["gal", "gallon", "gallons"]),
        ] {
            assert_aliases(unit, aliases);
        }

        for (unit, abbreviation, names) in [
            (
                Unit::CubicCentimeter,
                "cm",
                &["centimeter", "centimeters", "centimetre", "centimetres"][..],
            ),
            (
                Unit::CubicMeter,
                "m",
                &["meter", "meters", "metre", "metres"],
            ),
            (Unit::CubicInch, "in", &["inch", "inches"]),
            (Unit::CubicFoot, "ft", &["foot", "feet"]),
        ] {
            for alias in [
                format!("{abbreviation}³"),
                format!("{abbreviation}^3"),
                format!("{abbreviation}3"),
                format!("cubic {abbreviation}"),
            ] {
                assert_eq!(resolve_alias(&alias), Some(unit), "alias={alias}");
            }
            for name in names {
                let alias = format!("cubic {name}");
                assert_eq!(resolve_alias(&alias), Some(unit), "alias={alias}");
            }
        }
    }

    #[test]
    fn resolves_temperature_degree_and_full_name_forms_case_insensitively() {
        for (unit, aliases) in [
            (
                Unit::Celsius,
                &["c", "°c", "celsius", "degree celsius", "degrees celsius"][..],
            ),
            (
                Unit::Fahrenheit,
                &[
                    "f",
                    "°f",
                    "fahrenheit",
                    "degree fahrenheit",
                    "degrees fahrenheit",
                ],
            ),
            (
                Unit::Kelvin,
                &[
                    "k",
                    "°k",
                    "kelvin",
                    "kelvins",
                    "degree kelvin",
                    "degrees kelvin",
                ],
            ),
        ] {
            assert_aliases(unit, aliases);
        }

        assert_eq!(resolve_alias("°C"), Some(Unit::Celsius));
        assert_eq!(resolve_alias("FAHRENHEIT"), Some(Unit::Fahrenheit));
        assert_eq!(resolve_alias("KelVins"), Some(Unit::Kelvin));
    }

    #[test]
    fn defaults_volume_and_ton_to_us_and_limits_explicit_regional_prefixes() {
        for (aliases, us, imperial) in [
            (
                &["fl oz", "floz", "fluid ounce", "fluid ounces"][..],
                Unit::UsFluidOunce,
                Unit::ImperialFluidOunce,
            ),
            (&["pt", "pint", "pints"], Unit::UsPint, Unit::ImperialPint),
            (
                &["qt", "quart", "quarts"],
                Unit::UsQuart,
                Unit::ImperialQuart,
            ),
            (
                &["gal", "gallon", "gallons"],
                Unit::UsGallon,
                Unit::ImperialGallon,
            ),
        ] {
            for alias in aliases {
                assert_eq!(resolve_alias(alias), Some(us), "alias={alias}");
                assert_eq!(resolve_alias(&format!("us {alias}")), Some(us));
                for prefix in ["imp", "uk", "imperial"] {
                    assert_eq!(
                        resolve_alias(&format!("{prefix} {alias}")),
                        Some(imperial),
                        "prefix={prefix}, alias={alias}"
                    );
                }
            }
        }

        assert_eq!(resolve_alias("ton"), Some(Unit::ShortTon));
        assert_eq!(resolve_alias("tonne"), Some(Unit::Tonne));
        assert_eq!(resolve_alias("oz"), Some(Unit::Ounce));
        assert_eq!(resolve_alias("fl oz"), Some(Unit::UsFluidOunce));
        assert_eq!(resolve_alias("cup"), Some(Unit::UsCup));
    }

    #[test]
    fn rejects_undefined_cups_prefix_pairs_and_malformed_dimensions() {
        for alias in [
            "metric cup",
            "japanese cup",
            "legal cup",
            "us cup",
            "imp cup",
            "uk cup",
            "imperial cup",
            "imp tbsp",
            "uk l",
            "imperial oz",
            "us in",
            "metric gal",
            "usgal",
            "impgal",
            "mm^3",
            "mm3",
            "l²",
            "m^4",
            "square liter",
            "cubic yard",
            "°m",
        ] {
            assert_eq!(resolve_alias(alias), None, "alias={alias}");
        }
    }

    #[test]
    fn catalog_factors_derive_from_the_approved_exact_anchors() {
        assert_close(factor(Unit::Inch), 0.0254);
        assert_close(factor(Unit::Foot), factor(Unit::Inch) * 12.0);
        assert_close(factor(Unit::Yard), factor(Unit::Foot) * 3.0);
        assert_close(factor(Unit::Mile), factor(Unit::Yard) * 1760.0);

        assert_close(factor(Unit::Pound), 0.453_592_37);
        assert_close(factor(Unit::Ounce), factor(Unit::Pound) / 16.0);
        assert_close(factor(Unit::ShortTon), factor(Unit::Pound) * 2000.0);
        assert_close(factor(Unit::Tonne), 1000.0);

        assert_close(factor(Unit::SquareFoot), factor(Unit::Foot).powi(2));
        assert_close(factor(Unit::Acre), factor(Unit::SquareFoot) * 43_560.0);
        assert_close(factor(Unit::Hectare), 10_000.0);

        assert_close(factor(Unit::Liter), 0.001);
        assert_close(factor(Unit::Milliliter), 0.000_001);
        assert_close(factor(Unit::CubicCentimeter), factor(Unit::Milliliter));
        assert_close(factor(Unit::UsGallon), 0.003_785_411_784);
        assert_close(factor(Unit::UsQuart), factor(Unit::UsGallon) / 4.0);
        assert_close(factor(Unit::UsPint), factor(Unit::UsGallon) / 8.0);
        assert_close(factor(Unit::UsCup), factor(Unit::UsGallon) / 16.0);
        assert_close(factor(Unit::UsFluidOunce), factor(Unit::UsGallon) / 128.0);
        assert_close(factor(Unit::Tablespoon), factor(Unit::UsGallon) / 256.0);
        assert_close(factor(Unit::Teaspoon), factor(Unit::UsGallon) / 768.0);
        assert_close(factor(Unit::ImperialGallon), 0.004_546_09);
        assert_close(
            factor(Unit::ImperialFluidOunce),
            factor(Unit::ImperialGallon) / 160.0,
        );

        assert_eq!(
            Unit::Celsius.definition(),
            UnitDefinition::Temperature(TemperatureScale::Celsius)
        );
        assert_eq!(
            Unit::Fahrenheit.definition(),
            UnitDefinition::Temperature(TemperatureScale::Fahrenheit)
        );
        assert_eq!(
            Unit::Kelvin.definition(),
            UnitDefinition::Temperature(TemperatureScale::Kelvin)
        );
    }

    #[test]
    fn parses_attached_or_spaced_numbers_and_preserves_compound_unit_names() {
        for (source, value, source_unit, target_unit) in [
            ("1000mm to in", "1000", Unit::Millimeter, Unit::Inch),
            (
                "1000 millimeters to inches",
                "1000",
                Unit::Millimeter,
                Unit::Inch,
            ),
            ("-40°F TO Celsius", "-40", Unit::Fahrenheit, Unit::Celsius),
            (".5 L to US FL OZ", ".5", Unit::Liter, Unit::UsFluidOunce),
            (
                "1,000.5 fluid ounces to litres",
                "1000.5",
                Unit::UsFluidOunce,
                Unit::Liter,
            ),
            (
                "1 metric ton to short tons",
                "1",
                Unit::Tonne,
                Unit::ShortTon,
            ),
            (
                "1 mm^2 to mi2",
                "1",
                Unit::SquareMillimeter,
                Unit::SquareMile,
            ),
            ("2cubic feet to m³", "2", Unit::CubicFoot, Unit::CubicMeter),
            (
                "3 imp gal to uk pints",
                "3",
                Unit::ImperialGallon,
                Unit::ImperialPint,
            ),
        ] {
            let intent = ready(source);
            assert_eq!(intent.canonical_value, value, "source={source}");
            assert_eq!(intent.source, source_unit, "source={source}");
            assert_eq!(intent.target, target_unit, "source={source}");
        }
    }

    #[test]
    fn falls_through_until_to_is_complete_and_one_unit_is_known() {
        for source in [
            "",
            "convert this",
            "1 m",
            "1 oz",
            "1 gal",
            "ton",
            "fl oz",
            "1 m to",
            "to ft",
            "100 widgets to gadgets",
            "one thing to another",
            "1m toto ft",
            "1m\tto\tft",
        ] {
            assert_eq!(
                classify(source),
                UnitClassification::NoMatch,
                "source={source}"
            );
        }

        assert_eq!(
            classify(&format!("1 m to {}", "x".repeat(401))),
            UnitClassification::NoMatch
        );
    }

    #[test]
    fn complete_owned_queries_return_typed_operand_and_unknown_unit_failures() {
        for source in [
            "m to ft",
            "1e6 m to ft",
            "NaN m to ft",
            "1,5 m to ft",
            "1\tm to ft",
        ] {
            assert_eq!(
                classify(source),
                UnitClassification::Invalid(UnitFailure::InvalidNumber),
                "source={source}"
            );
        }

        for source in [
            "1 parsec to m",
            "1 m to parsec",
            "1 to L",
            "1 imp cup to ml",
            "1 ml to metric cup",
            "1 uk tbsp to l",
            "1 imperial oz to lb",
            "1 mm^3 to l",
            "1 m^4 to ft",
        ] {
            assert_eq!(
                classify(source),
                UnitClassification::Invalid(UnitFailure::UnknownUnit),
                "source={source}"
            );
        }
    }

    #[test]
    fn complete_owned_queries_reject_cross_dimension_pairs_before_conversion() {
        for source in ["1 m to kg", "1 oz to fl oz", "1 L to m²", "32 C to ft"] {
            assert_eq!(
                classify(source),
                UnitClassification::Invalid(UnitFailure::DimensionMismatch),
                "source={source}"
            );
        }
    }
}
