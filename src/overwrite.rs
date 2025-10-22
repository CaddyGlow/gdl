use std::io::{self, Write};
use std::path::Path;

use anyhow::{Result, anyhow};
use atty::Stream;
use log::{debug, warn};

/// Check if we should proceed with downloading files that might overwrite existing ones
pub fn check_overwrite_permission(
    files_to_download: &[(impl AsRef<Path>, u64)],
    force: bool,
) -> Result<()> {
    // Find files that already exist
    let existing_files: Vec<_> = files_to_download
        .iter()
        .filter_map(|(path, _size)| {
            let path = path.as_ref();
            if path.exists() { Some(path) } else { None }
        })
        .collect();

    if existing_files.is_empty() {
        debug!("No existing files will be overwritten");
        return Ok(());
    }

    // If force flag is set, proceed without prompting
    if force {
        debug!(
            "Force flag set, overwriting {} existing file(s)",
            existing_files.len()
        );
        return Ok(());
    }

    // Check if we're in a TTY (interactive terminal)
    let is_tty = atty::is(Stream::Stdout) && atty::is(Stream::Stdin);

    if !is_tty {
        // Not in a TTY, fail with error
        return Err(anyhow!(
            "Refusing to overwrite {} existing file(s) in non-interactive mode. \
             Use --force to override.",
            existing_files.len()
        ));
    }

    // In a TTY, prompt the user
    prompt_user_for_overwrite(&existing_files)
}

fn prompt_user_for_overwrite(existing_files: &[&Path]) -> Result<()> {
    // Log warning for tracking
    warn!(
        "{} existing file(s) will be overwritten if user confirms",
        existing_files.len()
    );

    // Display prompt to user (not through logger)
    eprintln!(
        "\n⚠  The following {} file(s) already exist:",
        existing_files.len()
    );
    for path in existing_files.iter().take(10) {
        eprintln!("  - {}", path.display());
    }
    if existing_files.len() > 10 {
        eprintln!("  ... and {} more", existing_files.len() - 10);
    }

    eprint!("\nOverwrite these file(s)? [y/N]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let answer = input.trim().to_lowercase();
    if answer == "y" || answer == "yes" {
        Ok(())
    } else {
        Err(anyhow!("Download cancelled by user"))
    }
}

/// Check a single file for overwrite permission
#[allow(dead_code)]
pub fn check_single_file_overwrite(path: &Path, force: bool) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    if force {
        debug!("Force flag set, overwriting {}", path.display());
        return Ok(());
    }

    let is_tty = atty::is(Stream::Stdout) && atty::is(Stream::Stdin);

    if !is_tty {
        return Err(anyhow!(
            "File {} already exists. Use --force to override.",
            path.display()
        ));
    }

    eprint!("File {} already exists. Overwrite? [y/N]: ", path.display());
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let answer = input.trim().to_lowercase();
    if answer == "y" || answer == "yes" {
        Ok(())
    } else {
        Err(anyhow!("Download cancelled by user"))
    }
}

/// Collect paths that will be created by the download
pub fn collect_target_paths<T: TargetPath>(tasks: &[T]) -> Vec<(&Path, u64)> {
    tasks
        .iter()
        .map(|task| (task.path(), task.size()))
        .collect()
}

pub trait TargetPath {
    fn path(&self) -> &Path;
    fn size(&self) -> u64;
}
