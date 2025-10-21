use std::path::Path;
use std::process::{Command as StdCommand, Stdio};

use anyhow::{anyhow, Context, Result};

pub fn git_available() -> bool {
    StdCommand::new("git")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub fn ensure_git_available() -> Result<()> {
    if git_available() {
        Ok(())
    } else {
        Err(anyhow!(
            "git executable not found in PATH; install git or choose `--strategy api`"
        ))
    }
}

pub fn run_git_command(
    args: &[&str],
    workdir: Option<&Path>,
    redacted_indices: &[usize],
) -> Result<()> {
    let mut cmd = StdCommand::new("git");
    cmd.args(args);
    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }
    cmd.env("GIT_TERMINAL_PROMPT", "0");

    let command_display = format_git_command(args, redacted_indices);
    let output = cmd
        .output()
        .with_context(|| format!("failed to execute git {}", command_display))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            String::new()
        };
        let detail = if message.is_empty() {
            "no additional output".to_string()
        } else {
            message
        };
        return Err(anyhow!(
            "git {} exited with status {}: {}",
            command_display,
            output.status,
            detail
        ));
    }

    Ok(())
}

fn format_git_command(args: &[&str], redacted_indices: &[usize]) -> String {
    args.iter()
        .enumerate()
        .map(|(idx, arg)| {
            if redacted_indices.contains(&idx) {
                "<redacted>"
            } else {
                arg
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
