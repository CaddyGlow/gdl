use std::path::PathBuf;

use clap::{ArgAction, Parser, ValueEnum};

pub const VERSION: &str = env!("GDL_VERSION");
pub const LONG_VERSION: &str = env!("GDL_LONG_VERSION");
pub const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum DownloadStrategy {
    /// Use the GitHub REST API for downloads.
    Api,
    /// Use git sparse checkout to retrieve content.
    Git,
    /// Try the REST API first, then fall back to git if needed.
    Auto,
}

#[derive(Parser, Debug)]
#[command(
    author,
    version = VERSION,
    long_version = LONG_VERSION,
    about = "Download files or directories from a GitHub repository using the REST API or git."
)]
pub struct Cli {
    /// GitHub folder URLs to download from (e.g. https://github.com/owner/repo/tree/branch/path)
    #[arg(
        value_name = "URL",
        num_args = 1..,
        required_unless_present_any = ["self_update", "check_update", "clear_cache"]
    )]
    pub urls: Vec<String>,

    /// Update gdl to the latest release and exit
    #[arg(long)]
    pub self_update: bool,

    /// Check for a newer gdl release and exit without installing it
    #[arg(long)]
    pub check_update: bool,

    /// Output directory to place the downloaded files (defaults depend on the request)
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// GitHub personal access token (falls back to GITHUB_TOKEN or GH_TOKEN env vars)
    #[arg(long)]
    pub token: Option<String>,

    /// Increase logging verbosity (-v for debug, -vv for trace)
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
    pub verbose: u8,

    /// Maximum number of files to download concurrently
    #[arg(long, value_name = "N", default_value_t = 4)]
    pub parallel: usize,

    /// Preferred download strategy (`api`, `git`, or `auto`)
    #[arg(long, value_enum, default_value_t = DownloadStrategy::Auto)]
    pub strategy: DownloadStrategy,

    /// Disable HTTP response caching and download resume
    #[arg(long)]
    pub no_cache: bool,

    /// Clear all cached data and exit
    #[arg(long)]
    pub clear_cache: bool,
}
