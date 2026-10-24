//! Checking and writing the generated README.

use super::generate::Generated;
use crate::Error;
use std::path::Path;

/// What happened to the README.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The README already matched; nothing was written.
    UpToDate,
    /// The README was rewritten.
    Updated,
}

/// Verifies that the README is fresh. Never writes.
///
/// Fails with [`Error::CheckReadmeMismatch`] if the README differs from the generated content
/// (including line terminators), or with [`Error::CheckHasWarnings`] if transforms emitted
/// warnings and `fail_on_warnings` is set.
pub fn check(generated: &Generated, fail_on_warnings: bool) -> Result<Outcome, Error> {
    if !generated.is_up_to_date() {
        return Err(Error::CheckReadmeMismatch(generated.readme_path.clone()));
    }
    if fail_on_warnings && !generated.warnings.is_empty() {
        return Err(Error::CheckHasWarnings);
    }
    Ok(Outcome::UpToDate)
}

/// Writes the README if it changed.
///
/// Like cargo-rdme, refuses to overwrite a README that has uncommitted changes or is not tracked
/// by git, unless `force` is set. Outside a git repository the README is always written.
pub fn write(generated: &Generated, force: bool) -> Result<Outcome, Error> {
    if generated.is_up_to_date() {
        return Ok(Outcome::UpToDate);
    }
    if !force && git_is_current(&generated.readme_path) == Some(false) {
        return Err(Error::ReadmeNotUpdatedUncommittedChanges(
            generated.readme_path.clone(),
        ));
    }

    std::fs::write(&generated.readme_path, &generated.rendered).map_err(|source| {
        Error::WriteReadme {
            path: generated.readme_path.clone(),
            source,
        }
    })?;

    Ok(Outcome::Updated)
}

/// Whether `path` is tracked by git and has no staged or unstaged changes.
///
/// Returns `None` when this cannot be determined (for example, outside a repository).
#[must_use]
pub fn git_is_current(path: &Path) -> Option<bool> {
    let path = dunce::canonicalize(path).ok()?;
    let repo = gix::discover(path.parent()?).ok()?;
    let workdir = dunce::canonicalize(repo.workdir()?).ok()?;
    let relative = path.strip_prefix(&workdir).ok()?;

    // Git paths always use `/`.
    let git_path = relative
        .components()
        .map(|c| c.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()?
        .join("/");

    let index = repo.index_or_empty().ok()?;
    if index.entry_by_path(git_path.as_str().into()).is_none() {
        // Untracked or ignored: treat as not current, so it is not silently overwritten.
        return Some(false);
    }

    // `:(top)` makes the pathspec relative to the repository root rather than the process's
    // working directory.
    let pathspec = format!(":(top,literal){git_path}");
    let changes: Result<Vec<_>, _> = repo
        .status(gix::progress::Discard)
        .ok()?
        .into_iter([pathspec.as_str().into()])
        .ok()?
        .collect();

    Some(changes.ok()?.is_empty())
}
