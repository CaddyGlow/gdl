use std::env;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, anyhow};
use log::{debug, info};
use self_update::backends::github;
use self_update::update::ReleaseUpdate;
use self_update::version;

use super::prompt::prompt_for_update;
use super::state::{UpdateDecision, load_update_state, save_update_state, update_state_path};
use crate::utils::{system_time_from_secs, system_time_to_secs};

const GITHUB_OWNER: &str = "CaddyGlow";
const GITHUB_REPO: &str = "ghdl";
const BIN_NAME: &str = "ghdl";
const UPDATE_CHECK_INTERVAL_SECS: u64 = 60 * 60;
const POSTPONE_DURATION_SECS: u64 = 24 * 60 * 60;

pub fn run_self_update(token: Option<&str>) -> Result<()> {
    if skip_self_update() {
        info!("Skipping self-update because GHDL_SKIP_SELF_UPDATE is set");
        return Ok(());
    }

    let updater = build_updater(token)?;
    let status = updater
        .update()
        .context("failed to download and install the latest ghdl release")?;

    if status.updated() {
        info!("Updated ghdl to version {}", status.version());
    } else {
        info!("ghdl is already up to date (current: {})", status.version());
    }

    Ok(())
}

pub fn check_for_update(token: Option<&str>) -> Result<()> {
    if skip_self_update() {
        info!("Skipping update check because GHDL_SKIP_SELF_UPDATE is set");
        return Ok(());
    }

    let updater = build_updater(token)?;
    let latest = updater
        .get_latest_release()
        .context("failed to fetch latest ghdl release information")?;
    let current_version = updater.current_version();

    if version::bump_is_greater(&current_version, &latest.version)
        .context("failed to compare semantic versions")?
    {
        info!(
            "A newer ghdl release is available: {} (current: {})",
            latest.version, current_version
        );
    } else {
        info!(
            "ghdl is already at the latest version ({})",
            current_version
        );
    }

    Ok(())
}

fn build_updater(token: Option<&str>) -> Result<Box<dyn ReleaseUpdate>> {
    let install_path = current_bin_dir()?;
    let mut builder = github::Update::configure();

    builder
        .repo_owner(GITHUB_OWNER)
        .repo_name(GITHUB_REPO)
        .bin_name(BIN_NAME)
        .bin_install_path(&install_path)
        .target(self_update::get_target())
        .show_download_progress(true)
        .no_confirm(true)
        .current_version(crate::cli::PKG_VERSION);

    if let Some(token) = token {
        if !token.trim().is_empty() {
            builder.auth_token(token.trim());
        }
    }

    builder
        .build()
        .context("failed to configure self-update for ghdl")
}

fn current_bin_dir() -> Result<PathBuf> {
    let exe = env::current_exe().context("unable to locate current executable path")?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow!("unable to determine install directory for ghdl"))?;
    Ok(dir.to_path_buf())
}

fn skip_self_update() -> bool {
    env::var("GHDL_SKIP_SELF_UPDATE").is_ok()
}

pub async fn auto_check_for_updates(token: Option<&str>) -> Result<()> {
    // Run the blocking self_update operations in a background thread
    let token_owned = token.map(|s| s.to_string());

    tokio::task::spawn_blocking(move || {
        let token_ref = token_owned.as_deref();

        if skip_self_update() {
            return Ok(());
        }

        let state_path = update_state_path()?;
        let mut state = load_update_state(&state_path)?;
        let now = SystemTime::now();

        if let Some(postpone_until_secs) = state.postpone_until {
            let postpone_until = system_time_from_secs(postpone_until_secs);
            if postpone_until > now {
                debug!(
                    "Skipping update check because it was postponed until {:?}",
                    postpone_until
                );
                return Ok(());
            }
            state.postpone_until = None;
        }

        if let Some(last_check_secs) = state.last_check {
            let last_check = system_time_from_secs(last_check_secs);
            let elapsed = match now.duration_since(last_check) {
                Ok(duration) => duration,
                Err(_) => Duration::from_secs(UPDATE_CHECK_INTERVAL_SECS),
            };

            if elapsed < Duration::from_secs(UPDATE_CHECK_INTERVAL_SECS) {
                debug!(
                    "Skipping update check; last check was {:?} seconds ago",
                    elapsed.as_secs()
                );
                return Ok(());
            }
        }

        let updater = build_updater(token_ref)?;
        let latest = updater
            .get_latest_release()
            .context("failed to fetch latest ghdl release information")?;
        let current_version = updater.current_version();
        let now_secs = system_time_to_secs(now);

        let is_newer = version::bump_is_greater(&current_version, &latest.version)
            .context("failed to compare semantic versions")?;

        if !is_newer {
            state.last_check = Some(now_secs);
            state.postpone_until = None;
            save_update_state(&state_path, &state)?;
            return Ok(());
        }

        if !atty::is(atty::Stream::Stdin) || !atty::is(atty::Stream::Stdout) {
            info!(
                "A newer ghdl release is available: {} (current: {}), but cannot prompt in non-interactive mode",
                latest.version, current_version
            );
            state.last_check = Some(now_secs);
            state.postpone_until = None;
            save_update_state(&state_path, &state)?;
            return Ok(());
        }

        println!(
            "A newer ghdl release is available: {} (current: {}).",
            latest.version, current_version
        );

        let decision = prompt_for_update()?;

        match decision {
            UpdateDecision::UpdateNow => {
                state.last_check = Some(now_secs);
                state.postpone_until = None;
                save_update_state(&state_path, &state)?;
                run_self_update(token_ref)?;
            }
            UpdateDecision::Postpone => {
                state.last_check = Some(now_secs);
                state.postpone_until = Some(now_secs + POSTPONE_DURATION_SECS);
                save_update_state(&state_path, &state)?;
                info!("Postponed update check for 24 hours.");
            }
            UpdateDecision::Discard => {
                state.last_check = Some(now_secs);
                state.postpone_until = None;
                save_update_state(&state_path, &state)?;
            }
        }

        Ok(())
    })
    .await
    .context("update check task panicked")?
}
