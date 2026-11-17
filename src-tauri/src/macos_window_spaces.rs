use objc2_app_kit::NSWindowCollectionBehavior as Behavior;

pub(super) fn launcher_behavior(current: Behavior) -> Behavior {
    // AppKit roles in each group are mutually exclusive. Clear incompatible
    // roles if the native window was initialized or reconfigured with them.
    let conflicting = Behavior::MoveToActiveSpace | Behavior::FullScreenPrimary
        | Behavior::FullScreenNone | Behavior::Primary | Behavior::Auxiliary;
    (current & !conflicting) | Behavior::CanJoinAllSpaces
        | Behavior::FullScreenAuxiliary | Behavior::CanJoinAllApplications
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_can_join_another_apps_fullscreen_without_conflicting_roles() {
        for original in [Behavior::empty(), Behavior::FullScreenPrimary | Behavior::Primary,
            Behavior::FullScreenNone | Behavior::Auxiliary | Behavior::MoveToActiveSpace] {
            let result = launcher_behavior(original);
            assert!(result.contains(Behavior::CanJoinAllSpaces | Behavior::FullScreenAuxiliary
                | Behavior::CanJoinAllApplications));
            assert!(!result.intersects(Behavior::MoveToActiveSpace | Behavior::FullScreenPrimary
                | Behavior::FullScreenNone | Behavior::Primary | Behavior::Auxiliary));
        }
    }

    #[test]
    fn repeated_presentation_preserves_unrelated_window_behavior() {
        let unrelated = Behavior::Transient | Behavior::IgnoresCycle;
        let configured = launcher_behavior(unrelated);
        assert!(configured.contains(unrelated));
        assert_eq!(launcher_behavior(configured), configured);
    }
}
