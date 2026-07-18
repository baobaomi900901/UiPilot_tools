use super::Application;

#[derive(Clone, Copy)]
struct Match {
    class: u8,
    alias: bool,
}

fn match_class(value: &str, query: &str) -> Option<u8> {
    if value == query {
        Some(0)
    } else if value.starts_with(query) {
        Some(1)
    } else if value.contains(query) {
        Some(2)
    } else if query
        .chars()
        .try_fold(value.chars(), |mut remaining, expected| {
            remaining
                .find(|candidate| *candidate == expected)
                .map(|_| remaining)
        })
        .is_some()
    {
        Some(3)
    } else {
        None
    }
}

fn best_match(application: &Application, query: &str) -> Option<Match> {
    std::iter::once((application.display_name.as_str(), false))
        .chain(
            application
                .aliases
                .iter()
                .map(|alias| (alias.as_str(), true)),
        )
        .filter_map(|(value, alias)| {
            match_class(&value.to_lowercase(), query).map(|class| Match { class, alias })
        })
        .min_by_key(|matched| (matched.class, !matched.alias))
}

pub(crate) fn rank(applications: &[Application], query: &str) -> Vec<Application> {
    if query.is_empty() {
        return Vec::new();
    }
    let query = query.to_lowercase();
    let mut matches: Vec<_> = applications
        .iter()
        .filter_map(|application| {
            best_match(application, &query).map(|matched| (application, matched))
        })
        .collect();
    matches.sort_by(|(left, left_match), (right, right_match)| {
        left_match
            .class
            .cmp(&right_match.class)
            .then_with(|| right_match.alias.cmp(&left_match.alias))
            .then_with(|| right.use_count.cmp(&left.use_count))
            .then_with(|| {
                left.display_name
                    .to_lowercase()
                    .cmp(&right.display_name.to_lowercase())
            })
            .then_with(|| left.app_id.cmp(&right.app_id))
    });
    matches
        .into_iter()
        .take(20)
        .map(|(application, _)| application.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::apps::{rank, Application};

    fn application(id: &str, name: &str, aliases: &[&str], use_count: u64) -> Application {
        Application {
            app_id: id.into(),
            display_name: name.into(),
            shortcut: PathBuf::from(format!(r"C:\Menu\{id}.lnk")),
            executable: None,
            icon: None,
            aliases: aliases.iter().map(|alias| (*alias).into()).collect(),
            use_count,
        }
    }

    fn titles(applications: &[Application]) -> Vec<&str> {
        applications
            .iter()
            .map(|application| application.display_name.as_str())
            .collect()
    }

    fn ids(applications: &[Application]) -> Vec<&str> {
        applications
            .iter()
            .map(|application| application.app_id.as_str())
            .collect()
    }

    #[test]
    fn exact_prefix_contains_and_subsequence_are_ordered() {
        let applications = vec![
            application("exact", "微信", &[], 0),
            application("prefix", "微信开发者工具", &[], 0),
            application("contains", "企业微信", &[], 0),
            application("subsequence", "微型信号", &[], 0),
            application("unrelated", "Calculator", &[], 100),
        ];

        assert_eq!(
            titles(&rank(&applications, "微信")),
            ["微信", "微信开发者工具", "企业微信", "微型信号"],
        );
    }

    #[test]
    fn equal_match_class_prefers_alias_then_use_count() {
        let applications = vec![
            application("display", "vscode", &[], 100),
            application("alias", "Visual Studio Code", &["vscode"], 0),
            application("lower-use", "VSCode", &[], 1),
        ];

        assert_eq!(
            ids(&rank(&applications, "vscode")),
            ["alias", "display", "lower-use"],
        );
        assert_eq!(
            titles(&rank(&applications, "vscode"))[0],
            "Visual Studio Code"
        );
    }

    #[test]
    fn name_then_app_id_break_remaining_ties() {
        let applications = vec![
            application("id-z", "App Alpha", &[], 4),
            application("id-b", "App Beta", &[], 4),
            application("id-a", "App Beta", &[], 4),
        ];

        assert_eq!(ids(&rank(&applications, "app")), ["id-z", "id-a", "id-b"],);
    }

    #[test]
    fn empty_query_is_empty_and_results_are_limited_to_twenty() {
        let applications: Vec<_> = (0..25)
            .map(|index| {
                application(
                    &format!("id-{index:02}"),
                    &format!("App {index:02}"),
                    &[],
                    0,
                )
            })
            .collect();

        assert!(rank(&applications, "").is_empty());
        assert_eq!(rank(&applications, "app").len(), 20);
    }

    #[test]
    fn duplicate_display_names_keep_distinct_ids_and_aliases() {
        let applications = vec![
            application("first", "Console", &["terminal"], 0),
            application("second", "Console", &["shell"], 0),
        ];

        let ranked = rank(&applications, "console");
        assert_eq!(ids(&ranked), ["first", "second"]);
        assert_ne!(ranked[0].aliases, ranked[1].aliases);
    }
}
