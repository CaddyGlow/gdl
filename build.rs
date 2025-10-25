use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    // Re-run if Git metadata changes.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");

    let manifest_version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());
    let describe = git_output(["describe", "--tags", "--dirty", "--always"]);
    let commit = git_output(["rev-parse", "--short", "HEAD"]);
    let branch = git_output(["rev-parse", "--abbrev-ref", "HEAD"]);

    let declared_version = manifest_declared_version().unwrap_or_else(|| manifest_version.clone());

    // Note: We don't validate version matches tag here because during release workflow,
    // we bump the version first, then create the tag. The validation would prevent commits.
    // Instead, rely on the release script to ensure version and tag are in sync.

    let pkg_version = declared_version.clone();

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

    println!("cargo:rustc-env=GHDL_VERSION={version}");
    println!("cargo:rustc-env=GHDL_LONG_VERSION={long_version}");
    println!("cargo:rustc-env=CARGO_PKG_VERSION={pkg_version}");
}

fn git_output<const N: usize>(args: [&str; N]) -> Option<String> {
    if !Path::new(".git").exists() {
        return None;
    }

    let output = Command::new("git").args(args).output().ok()?;
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

fn manifest_declared_version() -> Option<String> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").ok()?;
    let manifest_path = Path::new(&manifest_dir).join("Cargo.toml");
    let contents = std::fs::read_to_string(manifest_path).ok()?;

    let mut in_package_section = false;
    for line in contents.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') {
            in_package_section = trimmed == "[package]";
            continue;
        }

        if !in_package_section {
            continue;
        }

        let mut parts = trimmed.splitn(2, '=');
        let key = parts.next()?.trim();
        if key != "version" {
            continue;
        }

        let value = parts.next()?.split('#').next()?.trim();
        if let Some(stripped) = value.strip_prefix('"') {
            if let Some(stripped) = stripped.strip_suffix('"') {
                return Some(stripped.to_string());
            }
        }

        return None;
    }

    None
}
