use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command as StdCommand, Stdio};

use anyhow::{Context, Result, anyhow};
use indicatif::ProgressBar;
use regex::Regex;

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

/// Run git command with progress tracking by parsing stderr output
pub fn run_git_with_progress(
    args: &[&str],
    workdir: Option<&Path>,
    redacted_indices: &[usize],
    progress_bar: &ProgressBar,
) -> Result<()> {
    let mut cmd = StdCommand::new("git");
    cmd.args(args);
    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let command_display = format_git_command(args, redacted_indices);

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to execute git {}", command_display))?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("failed to capture stderr"))?;

    // Parse git progress output and collect error messages
    let reader = BufReader::new(stderr);
    let error_messages = parse_git_progress(reader, progress_bar);

    let status = child
        .wait()
        .with_context(|| format!("failed to wait for git {}", command_display))?;

    if !status.success() {
        let detail = if error_messages.is_empty() {
            "no additional output".to_string()
        } else {
            error_messages.join("\n")
        };
        return Err(anyhow!(
            "git {} exited with status {}: {}",
            command_display,
            status,
            detail
        ));
    }

    Ok(())
}

/// Parse git progress output and update progress bar
/// Returns a vector of non-progress messages (usually errors)
fn parse_git_progress<R: BufRead>(reader: R, progress_bar: &ProgressBar) -> Vec<String> {
    // Regex patterns for different git progress messages
    let enumerating_re = Regex::new(r"remote: Enumerating objects: (\d+)").unwrap();
    let counting_re = Regex::new(r"remote: Counting objects:\s+(\d+)% \((\d+)/(\d+)\)").unwrap();
    let compressing_re =
        Regex::new(r"remote: Compressing objects:\s+(\d+)% \((\d+)/(\d+)\)").unwrap();
    let receiving_re = Regex::new(r"Receiving objects:\s+(\d+)% \((\d+)/(\d+)\)").unwrap();
    let resolving_re = Regex::new(r"Resolving deltas:\s+(\d+)% \((\d+)/(\d+)\)").unwrap();

    let mut error_messages = Vec::new();

    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim();

        // Skip empty lines
        if line.is_empty() {
            continue;
        }

        // Match different progress patterns
        let is_progress = if let Some(caps) = enumerating_re.captures(line) {
            if let Ok(count) = caps[1].parse::<u64>() {
                progress_bar.set_message(format!("Enumerating objects: {}", count));
            }
            true
        } else if let Some(caps) = counting_re.captures(line) {
            if let (Ok(pct), Ok(current), Ok(total)) = (
                caps[1].parse::<u64>(),
                caps[2].parse::<u64>(),
                caps[3].parse::<u64>(),
            ) {
                progress_bar.set_message(format!("Counting objects: {}%", pct));
                progress_bar.set_length(total);
                progress_bar.set_position(current);
            }
            true
        } else if let Some(caps) = compressing_re.captures(line) {
            if let (Ok(pct), Ok(current), Ok(total)) = (
                caps[1].parse::<u64>(),
                caps[2].parse::<u64>(),
                caps[3].parse::<u64>(),
            ) {
                progress_bar.set_message(format!("Compressing objects: {}%", pct));
                progress_bar.set_length(total);
                progress_bar.set_position(current);
            }
            true
        } else if let Some(caps) = receiving_re.captures(line) {
            if let (Ok(pct), Ok(current), Ok(total)) = (
                caps[1].parse::<u64>(),
                caps[2].parse::<u64>(),
                caps[3].parse::<u64>(),
            ) {
                progress_bar.set_message(format!("Receiving objects: {}%", pct));
                progress_bar.set_length(total);
                progress_bar.set_position(current);
            }
            true
        } else if let Some(caps) = resolving_re.captures(line) {
            if let (Ok(pct), Ok(current), Ok(total)) = (
                caps[1].parse::<u64>(),
                caps[2].parse::<u64>(),
                caps[3].parse::<u64>(),
            ) {
                progress_bar.set_message(format!("Resolving deltas: {}%", pct));
                progress_bar.set_length(total);
                progress_bar.set_position(current);
            }
            true
        } else {
            false
        };

        // If this line wasn't a progress message, it might be an error
        if !is_progress {
            error_messages.push(line.to_string());
        }
    }

    error_messages
}
