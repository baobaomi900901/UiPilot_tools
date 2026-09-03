use chrono_tz::Tz;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AliasEntry {
    pub(super) normalized_key: &'static str,
    pub(super) zone: Tz,
    pub(super) display_name: &'static str,
}

const ALIASES: &[AliasEntry] = &[
    AliasEntry {
        normalized_key: "beijing",
        zone: chrono_tz::Asia::Shanghai,
        display_name: "Beijing",
    },
    AliasEntry {
        normalized_key: "shanghai",
        zone: chrono_tz::Asia::Shanghai,
        display_name: "Shanghai",
    },
    AliasEntry {
        normalized_key: "hongkong",
        zone: chrono_tz::Asia::Hong_Kong,
        display_name: "Hong Kong",
    },
    AliasEntry {
        normalized_key: "taipei",
        zone: chrono_tz::Asia::Taipei,
        display_name: "Taipei",
    },
    AliasEntry {
        normalized_key: "tokyo",
        zone: chrono_tz::Asia::Tokyo,
        display_name: "Tokyo",
    },
    AliasEntry {
        normalized_key: "seoul",
        zone: chrono_tz::Asia::Seoul,
        display_name: "Seoul",
    },
    AliasEntry {
        normalized_key: "singapore",
        zone: chrono_tz::Asia::Singapore,
        display_name: "Singapore",
    },
    AliasEntry {
        normalized_key: "bangkok",
        zone: chrono_tz::Asia::Bangkok,
        display_name: "Bangkok",
    },
    AliasEntry {
        normalized_key: "jakarta",
        zone: chrono_tz::Asia::Jakarta,
        display_name: "Jakarta",
    },
    AliasEntry {
        normalized_key: "dubai",
        zone: chrono_tz::Asia::Dubai,
        display_name: "Dubai",
    },
    AliasEntry {
        normalized_key: "mumbai",
        zone: chrono_tz::Asia::Kolkata,
        display_name: "Mumbai",
    },
    AliasEntry {
        normalized_key: "newdelhi",
        zone: chrono_tz::Asia::Kolkata,
        display_name: "New Delhi",
    },
    AliasEntry {
        normalized_key: "london",
        zone: chrono_tz::Europe::London,
        display_name: "London",
    },
    AliasEntry {
        normalized_key: "paris",
        zone: chrono_tz::Europe::Paris,
        display_name: "Paris",
    },
    AliasEntry {
        normalized_key: "berlin",
        zone: chrono_tz::Europe::Berlin,
        display_name: "Berlin",
    },
    AliasEntry {
        normalized_key: "rome",
        zone: chrono_tz::Europe::Rome,
        display_name: "Rome",
    },
    AliasEntry {
        normalized_key: "moscow",
        zone: chrono_tz::Europe::Moscow,
        display_name: "Moscow",
    },
    AliasEntry {
        normalized_key: "newyork",
        zone: chrono_tz::America::New_York,
        display_name: "New York",
    },
    AliasEntry {
        normalized_key: "chicago",
        zone: chrono_tz::America::Chicago,
        display_name: "Chicago",
    },
    AliasEntry {
        normalized_key: "denver",
        zone: chrono_tz::America::Denver,
        display_name: "Denver",
    },
    AliasEntry {
        normalized_key: "losangeles",
        zone: chrono_tz::America::Los_Angeles,
        display_name: "Los Angeles",
    },
    AliasEntry {
        normalized_key: "sanfrancisco",
        zone: chrono_tz::America::Los_Angeles,
        display_name: "San Francisco",
    },
    AliasEntry {
        normalized_key: "toronto",
        zone: chrono_tz::America::Toronto,
        display_name: "Toronto",
    },
    AliasEntry {
        normalized_key: "vancouver",
        zone: chrono_tz::America::Vancouver,
        display_name: "Vancouver",
    },
    AliasEntry {
        normalized_key: "mexicocity",
        zone: chrono_tz::America::Mexico_City,
        display_name: "Mexico City",
    },
    AliasEntry {
        normalized_key: "saopaulo",
        zone: chrono_tz::America::Sao_Paulo,
        display_name: "Sao Paulo",
    },
    AliasEntry {
        normalized_key: "sãopaulo",
        zone: chrono_tz::America::Sao_Paulo,
        display_name: "Sao Paulo",
    },
    AliasEntry {
        normalized_key: "sydney",
        zone: chrono_tz::Australia::Sydney,
        display_name: "Sydney",
    },
    AliasEntry {
        normalized_key: "melbourne",
        zone: chrono_tz::Australia::Melbourne,
        display_name: "Melbourne",
    },
    AliasEntry {
        normalized_key: "auckland",
        zone: chrono_tz::Pacific::Auckland,
        display_name: "Auckland",
    },
    AliasEntry {
        normalized_key: "utc",
        zone: chrono_tz::UTC,
        display_name: "UTC",
    },
    AliasEntry {
        normalized_key: "gmt",
        zone: chrono_tz::GMT,
        display_name: "GMT",
    },
];

#[cfg(test)]
pub(super) fn catalog() -> &'static [AliasEntry] {
    ALIASES
}

pub(super) fn normalize_alias(source: &str) -> String {
    source
        .chars()
        .filter(|character| !matches!(character, ' ' | '-' | '_'))
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

pub(super) fn resolve_alias(source: &str) -> Option<AliasEntry> {
    let normalized = normalize_alias(source);
    ALIASES
        .iter()
        .find(|entry| entry.normalized_key == normalized)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::{catalog, normalize_alias, resolve_alias};

    #[test]
    fn every_approved_alias_resolves_to_one_canonical_zone_and_display_name() {
        for (alias, expected_zone, expected_display) in [
            ("Beijing", "Asia/Shanghai", "Beijing"),
            ("Shanghai", "Asia/Shanghai", "Shanghai"),
            ("Hong Kong", "Asia/Hong_Kong", "Hong Kong"),
            ("Taipei", "Asia/Taipei", "Taipei"),
            ("Tokyo", "Asia/Tokyo", "Tokyo"),
            ("Seoul", "Asia/Seoul", "Seoul"),
            ("Singapore", "Asia/Singapore", "Singapore"),
            ("Bangkok", "Asia/Bangkok", "Bangkok"),
            ("Jakarta", "Asia/Jakarta", "Jakarta"),
            ("Dubai", "Asia/Dubai", "Dubai"),
            ("Mumbai", "Asia/Kolkata", "Mumbai"),
            ("New Delhi", "Asia/Kolkata", "New Delhi"),
            ("London", "Europe/London", "London"),
            ("Paris", "Europe/Paris", "Paris"),
            ("Berlin", "Europe/Berlin", "Berlin"),
            ("Rome", "Europe/Rome", "Rome"),
            ("Moscow", "Europe/Moscow", "Moscow"),
            ("New York", "America/New_York", "New York"),
            ("Chicago", "America/Chicago", "Chicago"),
            ("Denver", "America/Denver", "Denver"),
            ("Los Angeles", "America/Los_Angeles", "Los Angeles"),
            ("San Francisco", "America/Los_Angeles", "San Francisco"),
            ("Toronto", "America/Toronto", "Toronto"),
            ("Vancouver", "America/Vancouver", "Vancouver"),
            ("Mexico City", "America/Mexico_City", "Mexico City"),
            ("Sao Paulo", "America/Sao_Paulo", "Sao Paulo"),
            ("São Paulo", "America/Sao_Paulo", "Sao Paulo"),
            ("Sydney", "Australia/Sydney", "Sydney"),
            ("Melbourne", "Australia/Melbourne", "Melbourne"),
            ("Auckland", "Pacific/Auckland", "Auckland"),
            ("UTC", "UTC", "UTC"),
            ("GMT", "GMT", "GMT"),
        ] {
            for variant in [
                alias.to_owned(),
                alias.to_ascii_uppercase(),
                alias.replace(' ', ""),
                alias.replace(' ', "-"),
                alias.replace(' ', "_"),
            ] {
                let resolved = resolve_alias(&variant)
                    .unwrap_or_else(|| panic!("alias={alias}, variant={variant}"));
                assert_eq!(
                    resolved.zone.name(),
                    expected_zone,
                    "alias={alias}, variant={variant}"
                );
                assert_eq!(
                    resolved.display_name, expected_display,
                    "alias={alias}, variant={variant}"
                );
            }
        }
    }

    #[test]
    fn alias_normalization_ignores_ascii_case_spaces_hyphens_and_underscores() {
        for alias in ["NewYork", "NEW YORK", "new-york", "new_york", "nEw-_ YoRk"] {
            let resolved = resolve_alias(alias).unwrap_or_else(|| panic!("alias={alias}"));
            assert_eq!(resolved.zone.name(), "America/New_York", "alias={alias}");
            assert_eq!(resolved.display_name, "New York", "alias={alias}");
        }
        assert_eq!(normalize_alias("SãO-_ PAULO"), "sãopaulo");
    }

    #[test]
    fn catalog_has_no_duplicate_normalized_keys_and_keeps_both_sao_spellings_explicit() {
        let entries = catalog();
        let keys = entries
            .iter()
            .map(|entry| entry.normalized_key)
            .collect::<Vec<_>>();
        let unique = keys
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(keys.len(), unique.len());
        assert!(entries
            .iter()
            .all(|entry| normalize_alias(entry.normalized_key) == entry.normalized_key));
        assert!(keys.contains(&"saopaulo"));
        assert!(keys.contains(&"sãopaulo"));
    }
}
