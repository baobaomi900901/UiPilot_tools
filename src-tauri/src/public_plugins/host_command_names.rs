//! Host-reserved launch names shared by inventory validation and dispatch.
pub(super) fn host_command_names() -> impl Iterator<Item = &'static str> {
    [
        "find",
        "settings",
        "plugins",
        "screenshot",
        "quicklinks",
        "web-search",
    ]
    .into_iter()
}
