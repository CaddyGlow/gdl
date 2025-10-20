use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    // Re-run if Git metadata changes.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");

    let pkg_version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());

    let describe = git_output(["describe", "--tags", "--dirty", "--always"]);
    let commit = git_output(["rev-parse", "--short", "HEAD"]);
    let branch = git_output(["rev-parse", "--abbrev-ref", "HEAD"]);

    let version = if let Some(ref describe) = describe {
        format!("{pkg_version} ({describe})")
    } else if let Some(ref commit) = commit {
        format!("{pkg_version} ({commit})")
    } else {
        pkg_version.clone()
    };

    let mut long_lines = vec![format!("version: {pkg_version}")];
    if let Some(describe) = describe {
        long_lines.push(format!("git describe: {describe}"));
    }
    if let Some(commit) = commit {
        long_lines.push(format!("commit: {commit}"));
    }
    if let Some(branch) = branch {
        long_lines.push(format!("branch: {branch}"));
    }

    let long_version = long_lines.join("\n");

    println!("cargo:rustc-env=GDL_VERSION={version}");
    println!("cargo:rustc-env=GDL_LONG_VERSION={long_version}");
}

fn git_output<const N: usize>(args: [&str; N]) -> Option<String> {
    if !Path::new(".git").exists() {
        return None;
    }

    let output = Command::new("git").args(&args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}
