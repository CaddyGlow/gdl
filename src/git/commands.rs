//! Pure functions for building git commands.
//!
//! This module contains testable logic for constructing git command arguments
//! used throughout the application.

use std::path::Path;

/// Builds command arguments for `git init`.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use gdl::git::build_init_command;
///
/// let args = build_init_command(Path::new("/tmp/repo"));
/// assert_eq!(args, vec!["init", "/tmp/repo"]);
/// ```
pub fn build_init_command(path: &Path) -> Vec<String> {
    vec!["init".to_string(), path.to_string_lossy().to_string()]
}

/// Builds command arguments for `git sparse-checkout init`.
pub fn build_sparse_checkout_init_command() -> Vec<String> {
    vec![
        "sparse-checkout".to_string(),
        "init".to_string(),
        "--cone".to_string(),
    ]
}

/// Builds command arguments for `git sparse-checkout set`.
///
/// # Examples
///
/// ```
/// use gdl::git::build_sparse_checkout_set_command;
///
/// let paths = vec!["src".to_string(), "docs".to_string()];
/// let args = build_sparse_checkout_set_command(&paths);
/// assert!(args.contains(&"src".to_string()));
/// assert!(args.contains(&"docs".to_string()));
/// ```
pub fn build_sparse_checkout_set_command(paths: &[String]) -> Vec<String> {
    let mut cmd = vec!["sparse-checkout".to_string(), "set".to_string()];
    cmd.extend(paths.iter().cloned());
    cmd
}

/// Builds command arguments for `git fetch`.
///
/// # Examples
///
/// ```
/// use gdl::git::build_fetch_command;
///
/// let args = build_fetch_command("origin", "main", Some(1));
/// assert!(args.contains(&"--depth=1".to_string()));
/// assert!(args.contains(&"main".to_string()));
/// ```
pub fn build_fetch_command(remote: &str, branch: &str, depth: Option<u32>) -> Vec<String> {
    let mut cmd = vec!["fetch".to_string(), remote.to_string()];

    if let Some(d) = depth {
        cmd.push(format!("--depth={}", d));
    }

    cmd.push(branch.to_string());
    cmd
}

/// Builds command arguments for `git fetch` with progress tracking.
pub fn build_fetch_command_with_progress(
    remote: &str,
    branch: &str,
    depth: Option<u32>,
) -> Vec<String> {
    let mut cmd = vec![
        "fetch".to_string(),
        "--progress".to_string(),
        remote.to_string(),
    ];

    if let Some(d) = depth {
        cmd.push(format!("--depth={}", d));
    }

    cmd.push(branch.to_string());
    cmd
}

/// Builds command arguments for `git checkout`.
///
/// # Examples
///
/// ```
/// use gdl::git::build_checkout_command;
///
/// let args = build_checkout_command("main");
/// assert_eq!(args, vec!["checkout", "main"]);
/// ```
pub fn build_checkout_command(treeish: &str) -> Vec<String> {
    vec!["checkout".to_string(), treeish.to_string()]
}

/// Builds command arguments for `git clone`.
///
/// # Arguments
///
/// * `url` - The repository URL to clone from
/// * `target_dir` - The directory to clone into
/// * `branch` - The branch to clone (optional)
/// * `depth` - Clone depth for shallow clone (optional)
/// * `no_checkout` - Whether to skip checkout after cloning
///
/// # Examples
///
/// ```
/// use gdl::git::build_clone_command;
///
/// let args = build_clone_command(
///     "https://github.com/user/repo.git",
///     "/tmp/repo",
///     Some("main"),
///     Some(1),
///     false
/// );
/// assert!(args.contains(&"--depth=1".to_string()));
/// assert!(args.contains(&"--branch=main".to_string()));
/// ```
pub fn build_clone_command(
    url: &str,
    target_dir: &str,
    branch: Option<&str>,
    depth: Option<u32>,
    no_checkout: bool,
) -> Vec<String> {
    let mut cmd = vec!["clone".to_string()];

    if let Some(d) = depth {
        cmd.push(format!("--depth={}", d));
    }

    if let Some(b) = branch {
        cmd.push(format!("--branch={}", b));
    }

    if no_checkout {
        cmd.push("--no-checkout".to_string());
    }

    cmd.push(url.to_string());
    cmd.push(target_dir.to_string());
    cmd
}

/// Builds command arguments for `git clone` with progress tracking.
pub fn build_clone_command_with_progress(
    url: &str,
    target_dir: &str,
    branch: Option<&str>,
    depth: Option<u32>,
    no_checkout: bool,
) -> Vec<String> {
    let mut cmd = vec!["clone".to_string(), "--progress".to_string()];

    if let Some(d) = depth {
        cmd.push(format!("--depth={}", d));
    }

    if let Some(b) = branch {
        cmd.push(format!("--branch={}", b));
    }

    if no_checkout {
        cmd.push("--no-checkout".to_string());
    }

    cmd.push(url.to_string());
    cmd.push(target_dir.to_string());
    cmd
}

/// Builds command arguments for `git remote add`.
pub fn build_remote_add_command(name: &str, url: &str) -> Vec<String> {
    vec![
        "remote".to_string(),
        "add".to_string(),
        name.to_string(),
        url.to_string(),
    ]
}

/// Builds command arguments for `git config`.
pub fn build_config_command(key: &str, value: &str) -> Vec<String> {
    vec!["config".to_string(), key.to_string(), value.to_string()]
}

/// Validates that a URL appears to be a valid git URL.
///
/// Accepts URLs starting with:
/// - `https://`
/// - `git@` (SSH format)
/// - `ssh://`
///
/// # Examples
///
/// ```
/// use gdl::git::is_valid_git_url;
///
/// assert!(is_valid_git_url("https://github.com/user/repo"));
/// assert!(is_valid_git_url("git@github.com:user/repo.git"));
/// assert!(is_valid_git_url("ssh://git@github.com/user/repo.git"));
/// assert!(!is_valid_git_url("ftp://example.com"));
/// assert!(!is_valid_git_url("not-a-url"));
/// ```
pub fn is_valid_git_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("git@") || url.starts_with("ssh://")
}

/// Converts a GitHub HTTPS URL to a git URL (adding .git suffix if missing).
///
/// # Examples
///
/// ```
/// use gdl::git::github_url_to_git_url;
///
/// assert_eq!(
///     github_url_to_git_url("https://github.com/user/repo"),
///     "https://github.com/user/repo.git"
/// );
/// assert_eq!(
///     github_url_to_git_url("https://github.com/user/repo.git"),
///     "https://github.com/user/repo.git"
/// );
/// ```
pub fn github_url_to_git_url(https_url: &str) -> String {
    if !https_url.contains("github.com") {
        return https_url.to_string();
    }

    let trimmed = https_url.trim_end_matches('/');
    if trimmed.ends_with(".git") {
        trimmed.to_string()
    } else {
        format!("{}.git", trimmed)
    }
}

/// Constructs a GitHub repository git URL from owner and repo name.
///
/// # Examples
///
/// ```
/// use gdl::git::build_github_repo_url;
///
/// assert_eq!(
///     build_github_repo_url("torvalds", "linux"),
///     "https://github.com/torvalds/linux.git"
/// );
/// ```
pub fn build_github_repo_url(owner: &str, repo: &str) -> String {
    format!("https://github.com/{}/{}.git", owner, repo)
}

/// Redacts sensitive parts of git command arguments for logging.
///
/// Replaces arguments at the specified indices with "<redacted>".
///
/// # Examples
///
/// ```
/// use gdl::git::redact_command_args;
///
/// let args = vec!["clone", "https://user:token@github.com/repo"];
/// let redacted = redact_command_args(&args, &[1]);
/// assert_eq!(redacted, vec!["clone", "<redacted>"]);
/// ```
pub fn redact_command_args(args: &[&str], redacted_indices: &[usize]) -> Vec<String> {
    args.iter()
        .enumerate()
        .map(|(idx, arg)| {
            if redacted_indices.contains(&idx) {
                "<redacted>".to_string()
            } else {
                (*arg).to_string()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_build_basic_commands() {
        assert_eq!(
            build_init_command(Path::new("/tmp/repo")),
            vec!["init", "/tmp/repo"]
        );
        assert_eq!(
            build_sparse_checkout_init_command(),
            vec!["sparse-checkout", "init", "--cone"]
        );
        assert_eq!(build_checkout_command("main"), vec!["checkout", "main"]);
        assert_eq!(
            build_remote_add_command("origin", "https://github.com/user/repo.git"),
            vec![
                "remote",
                "add",
                "origin",
                "https://github.com/user/repo.git"
            ]
        );
        assert_eq!(
            build_config_command("user.name", "John Doe"),
            vec!["config", "user.name", "John Doe"]
        );
    }

    #[test]
    fn test_build_sparse_checkout_set_command() {
        assert_eq!(
            build_sparse_checkout_set_command(&vec!["src".to_string()]),
            vec!["sparse-checkout", "set", "src"]
        );
        assert_eq!(
            build_sparse_checkout_set_command(&vec!["src".to_string(), "docs".to_string()]),
            vec!["sparse-checkout", "set", "src", "docs"]
        );
    }

    #[test]
    fn test_build_fetch_command() {
        let cmd = build_fetch_command("origin", "main", Some(1));
        assert!(cmd.contains(&"--depth=1".to_string()));
        assert_eq!(
            build_fetch_command("origin", "main", None),
            vec!["fetch", "origin", "main"]
        );

        let cmd_progress = build_fetch_command_with_progress("origin", "main", Some(1));
        assert!(cmd_progress.contains(&"--progress".to_string()));
    }

    #[test]
    fn test_build_clone_command() {
        // Minimal
        assert_eq!(
            build_clone_command(
                "https://github.com/user/repo.git",
                "/tmp/repo",
                None,
                None,
                false
            ),
            vec!["clone", "https://github.com/user/repo.git", "/tmp/repo"]
        );

        // With options
        let cmd = build_clone_command(
            "https://github.com/user/repo.git",
            "/tmp/repo",
            Some("main"),
            Some(1),
            true,
        );
        assert!(cmd.contains(&"--depth=1".to_string()));
        assert!(cmd.contains(&"--branch=main".to_string()));
        assert!(cmd.contains(&"--no-checkout".to_string()));
    }

    #[test]
    fn test_is_valid_git_url() {
        // Valid URLs
        assert!(is_valid_git_url("https://github.com/user/repo"));
        assert!(is_valid_git_url("git@github.com:user/repo.git"));
        assert!(is_valid_git_url("ssh://git@github.com/user/repo.git"));

        // Invalid URLs
        assert!(!is_valid_git_url("ftp://example.com"));
        assert!(!is_valid_git_url("not-a-url"));
    }

    #[test]
    fn test_github_url_to_git_url() {
        assert_eq!(
            github_url_to_git_url("https://github.com/user/repo"),
            "https://github.com/user/repo.git"
        );
        assert_eq!(
            github_url_to_git_url("https://github.com/user/repo.git"),
            "https://github.com/user/repo.git"
        );
        assert_eq!(
            github_url_to_git_url("https://github.com/user/repo/"),
            "https://github.com/user/repo.git"
        );
        assert_eq!(
            github_url_to_git_url("https://gitlab.com/user/repo"),
            "https://gitlab.com/user/repo"
        ); // Non-GitHub unchanged
    }

    #[test]
    fn test_build_github_repo_url() {
        assert_eq!(
            build_github_repo_url("torvalds", "linux"),
            "https://github.com/torvalds/linux.git"
        );
    }

    #[test]
    fn test_redact_command_args() {
        let args = vec!["clone", "https://user:token@github.com/repo"];
        assert_eq!(
            redact_command_args(&args, &[]),
            vec!["clone", "https://user:token@github.com/repo"]
        );
        assert_eq!(
            redact_command_args(&args, &[1]),
            vec!["clone", "<redacted>"]
        );
    }
}
