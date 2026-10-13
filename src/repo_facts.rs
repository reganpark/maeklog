use crate::model::RepoSnapshot;
use crate::process::run_with_timeout;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const GIT_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_GIT_OUTPUT: usize = 128 * 1024;
const MAX_CHANGED_FILES: usize = 50;

#[must_use]
pub fn collect(cwd: &str) -> RepoSnapshot {
    let Some(root) = git(cwd, &["rev-parse", "--show-toplevel"]) else {
        return RepoSnapshot::default();
    };
    let branch = git(cwd, &["branch", "--show-current"]).filter(|value| !value.is_empty());
    let head = git(cwd, &["rev-parse", "HEAD"]).filter(|value| !value.is_empty());
    let remote = git(cwd, &["config", "--get", "remote.origin.url"])
        .filter(|value| !value.is_empty())
        .map(|value| sanitize_remote(&value));
    let status = git(
        cwd,
        &["status", "--porcelain=v1", "--untracked-files=normal"],
    )
    .unwrap_or_default();
    let mut changed: Vec<String> = status
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| truncate(line, 400))
        .collect();
    let changed_file_count = changed.len();
    let truncated = changed.len() > MAX_CHANGED_FILES;
    changed.truncate(MAX_CHANGED_FILES);

    RepoSnapshot {
        available: true,
        root: Some(root),
        branch,
        head,
        remote,
        changed_files: changed,
        changed_file_count,
        truncated,
    }
}

fn git(cwd: &str, arguments: &[&str]) -> Option<String> {
    if !Path::new(cwd).is_dir() {
        return None;
    }
    let mut command = Command::new("git");
    command.args(arguments).current_dir(cwd);
    let result = run_with_timeout(&mut command, None, GIT_TIMEOUT, MAX_GIT_OUTPUT).ok()?;
    result.success.then(|| result.stdout.trim().to_string())
}

fn sanitize_remote(remote: &str) -> String {
    let Some(scheme_end) = remote.find("://") else {
        return remote.to_string();
    };
    let authority_start = scheme_end + 3;
    let Some(relative_at) = remote[authority_start..].find('@') else {
        return remote.to_string();
    };
    let host_start = authority_start + relative_at + 1;
    format!("{}{}", &remote[..authority_start], &remote[host_start..])
}

fn truncate(value: &str, maximum_chars: usize) -> String {
    let mut characters = value.chars();
    let prefix: String = characters.by_ref().take(maximum_chars).collect();
    if characters.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_http_credentials() {
        assert_eq!(
            sanitize_remote("https://secret@example.com/org/repo.git"),
            "https://example.com/org/repo.git"
        );
        assert_eq!(
            sanitize_remote("git@github.com:org/repo.git"),
            "git@github.com:org/repo.git"
        );
    }
}
