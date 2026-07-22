use std::cmp::Ordering;

use super::{Application, ApplicationEntryKind};

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

fn best_match(application: &Application, query: &str) -> Option<u8> {
    match_class(&application.display_name.to_lowercase(), query)
}

pub(crate) fn rank(applications: &[Application], query: &str) -> Vec<Application> {
    if query.is_empty() {
        return Vec::new();
    }
    let query = query.to_lowercase();
    let mut matches: Vec<_> = applications
        .iter()
        .filter_map(|application| best_match(application, &query).map(|class| (application, class)))
        .collect();
    matches.sort_by(|(left, left_class), (right, right_class)| {
        left_class
            .cmp(right_class)
            .then_with(|| right.use_count.cmp(&left.use_count))
            .then_with(|| {
                left.display_name
                    .to_lowercase()
                    .cmp(&right.display_name.to_lowercase())
            })
            .then_with(|| match (left.entry_kind(), right.entry_kind()) {
                (ApplicationEntryKind::PackagedApp, ApplicationEntryKind::DesktopShortcut) => {
                    Ordering::Less
                }
                (ApplicationEntryKind::DesktopShortcut, ApplicationEntryKind::PackagedApp) => {
                    Ordering::Greater
                }
                _ => Ordering::Equal,
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

    use crate::apps::{rank, Application, ApplicationLaunchTarget};

    fn application(id: &str, name: &str, use_count: u64) -> Application {
        application_with_target(
            id,
            name,
            use_count,
            ApplicationLaunchTarget::Shortcut {
                shortcut: PathBuf::from(format!(r"C:\Menu\{id}.lnk")),
                executable: None,
            },
        )
    }

    fn application_with_target(
        id: &str,
        name: &str,
        use_count: u64,
        target: ApplicationLaunchTarget,
    ) -> Application {
        Application {
            app_id: id.into(),
            display_name: name.into(),
            target,
            icon: None,
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
            application("exact", "微信", 0),
            application("prefix", "微信开发者工具", 0),
            application("contains", "企业微信", 0),
            application("subsequence", "微型信号", 0),
            application("unrelated", "Calculator", 100),
        ];

        assert_eq!(
            titles(&rank(&applications, "微信")),
            ["微信", "微信开发者工具", "企业微信", "微型信号"],
        );
    }

    #[test]
    fn enterprise_wechat_exact_match_precedes_uninstaller_contains_match() {
        let applications = vec![
            application("main", "企业微信", 0),
            application("uninstaller", "卸载企业微信", 100),
        ];

        assert_eq!(
            ids(&rank(&applications, "企业微信")),
            ["main", "uninstaller"]
        );
    }

    #[test]
    fn application_search_only_matches_display_name() {
        let applications = vec![application("console", "Console", 0)];

        assert!(rank(&applications, "terminal").is_empty());
    }

    #[test]
    fn equal_match_class_prefers_use_count() {
        let applications = vec![
            application("higher-use", "vscode", 100),
            application("lower-use", "VSCode", 1),
        ];

        assert_eq!(
            ids(&rank(&applications, "vscode")),
            ["higher-use", "lower-use"],
        );
    }

    #[test]
    fn name_then_app_id_break_remaining_ties() {
        let applications = vec![
            application("id-z", "App Alpha", 4),
            application("id-b", "App Beta", 4),
            application("id-a", "App Beta", 4),
        ];

        assert_eq!(ids(&rank(&applications, "app")), ["id-z", "id-a", "id-b"],);
    }

    #[test]
    fn empty_query_is_empty_and_results_are_limited_to_twenty() {
        let applications: Vec<_> = (0..25)
            .map(|index| application(&format!("id-{index:02}"), &format!("App {index:02}"), 0))
            .collect();

        assert!(rank(&applications, "").is_empty());
        assert_eq!(rank(&applications, "app").len(), 20);
    }

    #[test]
    fn duplicate_display_names_keep_distinct_ids() {
        let applications = vec![
            application("first", "Console", 0),
            application("second", "Console", 0),
        ];

        let ranked = rank(&applications, "console");
        assert_eq!(ids(&ranked), ["first", "second"]);
    }

    #[test]
    fn packaged_app_only_wins_after_match_use_count_and_name_tie() {
        let desktop = application("desktop", "设置", 0);
        let packaged = application_with_target(
            "packaged",
            "设置",
            0,
            ApplicationLaunchTarget::PackagedApp {
                aumid: "family!settings".into(),
            },
        );

        assert_eq!(
            ids(&rank(&[desktop, packaged], "设置")),
            ["packaged", "desktop"]
        );
    }

    #[test]
    fn higher_use_count_still_beats_packaged_kind() {
        let desktop = application("desktop", "设置", 2);
        let packaged = application_with_target(
            "packaged",
            "设置",
            1,
            ApplicationLaunchTarget::PackagedApp {
                aumid: "family!settings".into(),
            },
        );

        assert_eq!(
            ids(&rank(&[desktop, packaged], "设置")),
            ["desktop", "packaged"]
        );
    }
}
