#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FilePathKind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileExecutionOutcome {
    FileRevealRequested,
    FolderOpenRequested,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileExecutionError {
    SearchUnavailable,
    Stale,
    NotFound,
    OpenFailed,
}

pub(crate) mod windows;
