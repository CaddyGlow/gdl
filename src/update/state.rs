use std::env;
use std::fs;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use log::warn;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct UpdateState {
    pub last_check: Option<u64>,
    pub postpone_until: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateDecision {
    UpdateNow,
    Postpone,
    Discard,
}

pub fn update_state_path() -> Result<PathBuf> {
    let base = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".cache"))
        })
        .ok_or_else(|| {
            anyhow!("Unable to determine cache directory (set XDG_CACHE_HOME or HOME)")
        })?;

    let dir = base.join("gdl");
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create cache directory {}", dir.display()))?;
    Ok(dir.join("update_state.json"))
}

pub fn load_update_state(path: &Path) -> Result<UpdateState> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(UpdateState::default()),
        Err(err) => {
            return Err(anyhow!(
                "failed to open update state file {}: {}",
                path.display(),
                err
            ));
        }
    };

    match serde_json::from_reader(file) {
        Ok(state) => Ok(state),
        Err(err) => {
            warn!(
                "Unable to parse update state file {}; resetting tracking: {}",
                path.display(),
                err
            );
            Ok(UpdateState::default())
        }
    }
}

pub fn save_update_state(path: &Path, state: &UpdateState) -> Result<()> {
    let tmp_path = path.with_extension("json.tmp");
    let mut file = File::create(&tmp_path).with_context(|| {
        format!(
            "failed to create temporary update state file {}",
            tmp_path.display()
        )
    })?;
    serde_json::to_writer_pretty(&mut file, state)
        .with_context(|| format!("failed to write update state to {}", tmp_path.display()))?;

    use std::io::Write;
    file.flush()
        .with_context(|| format!("failed to flush update state file {}", tmp_path.display()))?;
    if path.exists() {
        fs::remove_file(path).with_context(|| {
            format!(
                "failed to remove existing update state file {}",
                path.display()
            )
        })?;
    }
    fs::rename(&tmp_path, path)
        .with_context(|| format!("failed to persist update state file {}", path.display()))?;
    Ok(())
}
