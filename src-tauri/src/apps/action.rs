use std::{fmt, path::Path};

use super::{
    windows_backend::{self, NativeActionError, NativeActivation},
    ApplicationLaunchTarget,
};
use crate::result_registry::ResultAction;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum ApplicationActionOutcome {
    LaunchRequested,
    ActivationRequested,
    ActivationRefusedLaunchRequested,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ApplicationActionError {
    ApplicationEntryUnavailable,
}

impl fmt::Display for ApplicationActionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("application entry unavailable")
    }
}

impl std::error::Error for ApplicationActionError {}

impl From<NativeActionError> for ApplicationActionError {
    fn from(_: NativeActionError) -> Self {
        Self::ApplicationEntryUnavailable
    }
}

pub(crate) fn execute_application(
    action: &ResultAction,
) -> Result<ApplicationActionOutcome, ApplicationActionError> {
    execute_application_with(
        action,
        windows_backend::try_activate,
        windows_backend::launch_shortcut,
        windows_backend::launch_packaged_app,
    )
}

fn execute_application_with<A, L, P>(
    action: &ResultAction,
    mut activate: A,
    mut launch: L,
    mut launch_packaged: P,
) -> Result<ApplicationActionOutcome, ApplicationActionError>
where
    A: FnMut(&Path) -> NativeActivation,
    L: FnMut(&Path) -> Result<(), NativeActionError>,
    P: FnMut(&str) -> Result<(), NativeActionError>,
{
    let target = match action {
        ResultAction::LaunchApplication { target, .. } => target,
        ResultAction::OpenIndexedPath => {
            return Err(ApplicationActionError::ApplicationEntryUnavailable)
        }
        ResultAction::CopyText { .. } => {
            return Err(ApplicationActionError::ApplicationEntryUnavailable)
        }
    };
    let (shortcut, executable) = match target {
        ApplicationLaunchTarget::Shortcut {
            shortcut,
            executable,
        } => (shortcut, executable),
        ApplicationLaunchTarget::PackagedApp { aumid } => {
            launch_packaged(aumid)?;
            return Ok(ApplicationActionOutcome::LaunchRequested);
        }
    };

    let Some(executable) = executable else {
        launch(shortcut)?;
        return Ok(ApplicationActionOutcome::LaunchRequested);
    };

    match activate(executable) {
        NativeActivation::Activated => Ok(ApplicationActionOutcome::ActivationRequested),
        NativeActivation::Refused => {
            launch(shortcut)?;
            Ok(ApplicationActionOutcome::ActivationRefusedLaunchRequested)
        }
        NativeActivation::Unavailable | NativeActivation::Indeterminate => {
            launch(shortcut)?;
            Ok(ApplicationActionOutcome::LaunchRequested)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        path::{Path, PathBuf},
    };

    use super::{execute_application_with, ApplicationActionError, ApplicationActionOutcome};
    use crate::{
        apps::{
            windows_backend::{NativeActionError, NativeActivation},
            ApplicationLaunchTarget,
        },
        result_registry::ResultAction,
    };

    fn action(executable: Option<&str>) -> ResultAction {
        ResultAction::LaunchApplication {
            app_id: "app-trusted".into(),
            target: ApplicationLaunchTarget::Shortcut {
                shortcut: PathBuf::from(r"C:\Menu\Trusted.lnk"),
                executable: executable.map(PathBuf::from),
            },
        }
    }

    fn packaged_action(aumid: &str) -> ResultAction {
        ResultAction::LaunchApplication {
            app_id: "app-packaged".into(),
            target: ApplicationLaunchTarget::PackagedApp {
                aumid: aumid.into(),
            },
        }
    }

    #[test]
    fn action_policy_is_the_only_fallback_and_outcome_state_machine() {
        let cases = [
            (
                None,
                NativeActivation::Indeterminate,
                ApplicationActionOutcome::LaunchRequested,
                0,
                1,
            ),
            (
                Some(r"C:\Apps\Trusted.exe"),
                NativeActivation::Activated,
                ApplicationActionOutcome::ActivationRequested,
                1,
                0,
            ),
            (
                Some(r"C:\Apps\Trusted.exe"),
                NativeActivation::Refused,
                ApplicationActionOutcome::ActivationRefusedLaunchRequested,
                1,
                1,
            ),
            (
                Some(r"C:\Apps\Trusted.exe"),
                NativeActivation::Unavailable,
                ApplicationActionOutcome::LaunchRequested,
                1,
                1,
            ),
            (
                Some(r"C:\Apps\Trusted.exe"),
                NativeActivation::Indeterminate,
                ApplicationActionOutcome::LaunchRequested,
                1,
                1,
            ),
        ];

        for (executable, native, expected, expected_activations, expected_launches) in cases {
            let activation_calls = Cell::new(0);
            let launch_calls = Cell::new(0);
            let result = execute_application_with(
                &action(executable),
                |path| {
                    activation_calls.set(activation_calls.get() + 1);
                    assert_eq!(path, Path::new(r"C:\Apps\Trusted.exe"));
                    native
                },
                |path| {
                    launch_calls.set(launch_calls.get() + 1);
                    assert_eq!(path, Path::new(r"C:\Menu\Trusted.lnk"));
                    Ok(())
                },
                |_| panic!("desktop shortcut must not use packaged activation"),
            );

            assert_eq!(result, Ok(expected));
            assert_eq!(activation_calls.get(), expected_activations);
            assert_eq!(launch_calls.get(), expected_launches);
        }
    }

    #[test]
    fn action_policy_does_not_retry_a_failed_trusted_launch() {
        let activation_calls = Cell::new(0);
        let launch_calls = Cell::new(0);

        let result = execute_application_with(
            &action(Some(r"C:\Apps\Trusted.exe")),
            |_| {
                activation_calls.set(activation_calls.get() + 1);
                NativeActivation::Refused
            },
            |_| {
                launch_calls.set(launch_calls.get() + 1);
                Err(NativeActionError::ApplicationEntryUnavailable)
            },
            |_| panic!("desktop shortcut must not use packaged activation"),
        );

        assert_eq!(
            result,
            Err(ApplicationActionError::ApplicationEntryUnavailable)
        );
        assert_eq!(activation_calls.get(), 1);
        assert_eq!(launch_calls.get(), 1);
    }

    #[test]
    fn packaged_action_only_uses_aumid_activation() {
        let packaged_calls = Cell::new(0);
        let result = execute_application_with(
            &packaged_action("family!app"),
            |_| panic!("packaged app must not use process activation"),
            |_| panic!("packaged app must not launch a shortcut"),
            |aumid| {
                packaged_calls.set(packaged_calls.get() + 1);
                assert_eq!(aumid, "family!app");
                Ok(())
            },
        );

        assert_eq!(result, Ok(ApplicationActionOutcome::LaunchRequested));
        assert_eq!(packaged_calls.get(), 1);
    }

    #[test]
    fn packaged_action_failure_is_not_retried() {
        let packaged_calls = Cell::new(0);
        let result = execute_application_with(
            &packaged_action("family!app"),
            |_| panic!("packaged app must not use process activation"),
            |_| panic!("packaged app must not launch a shortcut"),
            |_| {
                packaged_calls.set(packaged_calls.get() + 1);
                Err(NativeActionError::ApplicationEntryUnavailable)
            },
        );

        assert_eq!(
            result,
            Err(ApplicationActionError::ApplicationEntryUnavailable)
        );
        assert_eq!(packaged_calls.get(), 1);
    }

    #[test]
    fn file_action_is_rejected_before_all_application_launch_paths() {
        let activation_calls = Cell::new(0);
        let shortcut_calls = Cell::new(0);
        let packaged_calls = Cell::new(0);

        let result = execute_application_with(
            &ResultAction::OpenIndexedPath,
            |_| {
                activation_calls.set(activation_calls.get() + 1);
                NativeActivation::Activated
            },
            |_| {
                shortcut_calls.set(shortcut_calls.get() + 1);
                Ok(())
            },
            |_| {
                packaged_calls.set(packaged_calls.get() + 1);
                Ok(())
            },
        );

        assert_eq!(
            result,
            Err(ApplicationActionError::ApplicationEntryUnavailable)
        );
        assert_eq!(activation_calls.get(), 0);
        assert_eq!(shortcut_calls.get(), 0);
        assert_eq!(packaged_calls.get(), 0);
    }
}
