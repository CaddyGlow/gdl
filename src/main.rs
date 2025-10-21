use std::env;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use log::info;
use reqwest::Client;

mod cache;
mod cli;
mod download;
mod git;
mod github;
mod http;
mod overwrite;
mod paths;
mod progress;
mod rate_limit;
mod types;
mod update;
mod utils;
mod zip;

use cache::clear_all_caches;
use cli::Cli;
use download::download_github_path;
use github::fetch_rate_limit_info;
use rate_limit::RateLimitTracker;
use update::{auto_check_for_updates, check_for_update, run_self_update};
use utils::init_logging;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let verbose = cli.verbose;
    init_logging(verbose);

    let Cli {
        urls,
        self_update,
        check_update,
        output,
        token,
        verbose: _,
        parallel,
        strategy,
        no_cache,
        clear_cache,
        force,
    } = cli;

    let token = token
        .or_else(|| env::var("GITHUB_TOKEN").ok())
        .or_else(|| env::var("GH_TOKEN").ok());

    if clear_cache {
        clear_all_caches()?;
        return Ok(());
    }

    if self_update {
        run_self_update(token.as_deref())?;
        return Ok(());
    }

    if check_update {
        check_for_update(token.as_deref())?;
        return Ok(());
    }

    auto_check_for_updates(token.as_deref())?;

    let client = Client::builder()
        .user_agent("gdl-rs (https://github.com/CaddyGlow/gdl)")
        .build()
        .context("failed to construct HTTP client")?;
    let rate_limit = Arc::new(RateLimitTracker::default());

    let parallel = parallel.max(1);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build async runtime")?;

    let rate_limit_for_runtime = Arc::clone(&rate_limit);

    runtime.block_on(async move {
        let output_ref = output.as_ref();
        let token_ref = token.as_deref();
        let rate_limit = rate_limit_for_runtime;
        for url in urls {
            download_github_path(
                &client,
                &url,
                output_ref,
                token_ref,
                parallel,
                Arc::clone(&rate_limit),
                strategy,
                no_cache,
                force,
            )
            .await?;
        }

        // Fetch and display rate limit info in verbose mode
        // Note: This endpoint does not count against your primary rate limit
        if verbose >= 1 {
            let _ = fetch_rate_limit_info(&client, token_ref).await;
        }

        Ok::<(), anyhow::Error>(())
    })?;

    info!("All downloads completed successfully.");
    Ok(())
}
