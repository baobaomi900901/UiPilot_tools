use std::{fmt, path::Path};

use super::windows_backend::{self, NativeActionError, NativeActivation};
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
    )
}

fn execute_application_with<A, L>(
    action: &ResultAction,
    mut activate: A,
    mut launch: L,
) -> Result<ApplicationActionOutcome, ApplicationActionError>
where
    A: FnMut(&Path) -> NativeActivation,
    L: FnMut(&Path) -> Result<(), NativeActionError>,
{
    let ResultAction::LaunchApplication {
        shortcut,
        executable,
        ..
    } = action;

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
        apps::windows_backend::{NativeActionError, NativeActivation},
        result_registry::ResultAction,
    };

    fn action(executable: Option<&str>) -> ResultAction {
        ResultAction::LaunchApplication {
            app_id: "app-trusted".into(),
            shortcut: PathBuf::from(r"C:\Menu\Trusted.lnk"),
            executable: executable.map(PathBuf::from),
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
        );

        assert_eq!(
            result,
            Err(ApplicationActionError::ApplicationEntryUnavailable)
        );
        assert_eq!(activation_calls.get(), 1);
        assert_eq!(launch_calls.get(), 1);
    }
}
