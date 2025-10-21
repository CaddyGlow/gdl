use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use log::{debug, warn};
use reqwest::header::AUTHORIZATION;
use reqwest::Client;
use serde::Deserialize;

use crate::github::types::{GitHubContent, GitTreeResponse};
use crate::rate_limit::RateLimitTracker;
use crate::types::{FileMetadata, RequestInfo};
use crate::github::types::GitTreeEntryType;

pub async fn fetch_github_contents(
    client: &Client,
    request: &RequestInfo,
    folder_path: &str,
    token: Option<&str>,
    rate_limit: Arc<RateLimitTracker>,
    no_cache: bool,
) -> Result<Vec<GitHubContent>> {
    let mut api_url = url::Url::parse(&format!(
        "https://api.github.com/repos/{}/{}/contents",
        request.owner, request.repo
    ))?;

    {
        let mut segments = api_url
            .path_segments_mut()
            .map_err(|_| anyhow!("failed to manipulate GitHub API URL"))?;
        if !folder_path.is_empty() {
            for segment in folder_path.split('/') {
                if !segment.is_empty() {
                    segments.push(segment);
                }
            }
        }
    }

    api_url
        .query_pairs_mut()
        .append_pair("ref", &request.branch);

    let mut request_builder = client.get(api_url);

    if let Some(token) = token {
        request_builder = request_builder.header(AUTHORIZATION, format!("token {}", token.trim()));
    }

    let context = format!(
        "listing contents for {}/{}:{}:{}",
        request.owner,
        request.repo,
        request.branch,
        if folder_path.is_empty() {
            "/"
        } else {
            folder_path
        }
    );

    let body = crate::http::send_github_request_cached(&request_builder, &rate_limit, &context, no_cache)
        .await
        .context("GitHub API request failed")?;

    let items: Result<Vec<GitHubContent>, _> = serde_json::from_slice(&body);
    match items {
        Ok(list) => Ok(list),
        Err(_) => {
            let single: GitHubContent =
                serde_json::from_slice(&body).context("unable to decode GitHub API response")?;
            Ok(vec![single])
        }
    }
}

pub async fn fetch_git_tree(
    client: &Client,
    request: &RequestInfo,
    token: Option<&str>,
    rate_limit: Arc<RateLimitTracker>,
    no_cache: bool,
) -> Result<GitTreeResponse> {
    let tree_spec = if request.path.is_empty() {
        request.branch.clone()
    } else {
        format!("{}:{}", request.branch, request.path)
    };

    let mut api_url = url::Url::parse("https://api.github.com/repos")
        .context("failed to construct GitHub tree URL")?;
    {
        let mut segments = api_url
            .path_segments_mut()
            .map_err(|_| anyhow!("failed to manipulate GitHub tree URL"))?;
        segments.push(&request.owner);
        segments.push(&request.repo);
        segments.push("git");
        segments.push("trees");
        segments.push(&tree_spec);
    }
    api_url.query_pairs_mut().append_pair("recursive", "1");

    let mut request_builder = client.get(api_url);
    if let Some(token) = token {
        request_builder = request_builder.header(AUTHORIZATION, format!("token {}", token.trim()));
    }

    let context = format!(
        "enumerating git tree for {}/{} ({})",
        request.owner, request.repo, tree_spec
    );
    let body = crate::http::send_github_request_cached(&request_builder, &rate_limit, &context, no_cache)
        .await
        .context("GitHub git tree request failed")?;

    let tree: GitTreeResponse =
        serde_json::from_slice(&body).context("failed to decode GitHub tree response")?;
    Ok(tree)
}

pub async fn build_file_inventory(
    client: &Client,
    request: &RequestInfo,
    token: Option<&str>,
    contents: &[GitHubContent],
    rate_limit: Arc<RateLimitTracker>,
    no_cache: bool,
) -> Result<HashMap<String, FileMetadata>> {
    use crate::github::types::ContentType;

    if contents.len() == 1 && contents[0].content_type == ContentType::File {
        let mut map = HashMap::new();
        map.insert(
            contents[0].path.clone(),
            FileMetadata {
                size: contents[0].size,
            },
        );
        return Ok(map);
    }

    let tree = fetch_git_tree(client, request, token, Arc::clone(&rate_limit), no_cache).await?;
    if tree.truncated {
        warn!(
            "GitHub tree listing for {}/{} may be incomplete (truncated).",
            request.owner, request.repo
        );
    }

    let mut files = HashMap::new();
    let base_prefix = if request.path.is_empty() {
        String::new()
    } else {
        format!("{}/", request.path)
    };

    for entry in tree.tree {
        if entry.entry_type != GitTreeEntryType::Blob {
            continue;
        }

        let full_path = if base_prefix.is_empty() {
            entry.path.trim_start_matches('/').to_string()
        } else if entry.path.is_empty() {
            request.path.clone()
        } else {
            format!("{}{}", base_prefix, entry.path.trim_start_matches('/'))
        };

        files.insert(full_path, FileMetadata { size: entry.size });
    }

    Ok(files)
}

pub fn parse_github_url(raw_url: &str) -> Result<RequestInfo> {
    use crate::types::RequestKind;

    let parsed = url::Url::parse(raw_url).context("invalid GitHub URL")?;
    let has_trailing_slash = raw_url.ends_with('/');

    let segments: Vec<_> = parsed
        .path_segments()
        .ok_or_else(|| anyhow!("GitHub URL is missing path segments"))?
        .collect();

    if segments.len() < 5 || (segments[2] != "tree" && segments[2] != "blob") {
        return Err(anyhow!(
            "URL must include /tree/ or /blob/ with a branch and path component"
        ));
    }

    let mut kind = if segments[2] == "tree" {
        RequestKind::Tree
    } else {
        RequestKind::Blob
    };

    let owner = segments[0].to_string();
    let repo = segments[1].to_string();
    let branch = segments[3].to_string();
    let raw_path = segments[4..].join("/");
    let path = raw_path.trim_matches('/').to_string();

    if path.is_empty() && kind == RequestKind::Blob {
        kind = RequestKind::Tree;
    }

    Ok(RequestInfo {
        owner,
        repo,
        branch,
        path,
        has_trailing_slash,
        kind,
    })
}

#[derive(Debug, Deserialize)]
struct RateLimitResource {
    limit: u64,
    remaining: u64,
    used: u64,
    reset: u64,
}

#[derive(Debug, Deserialize)]
struct RateLimitResponse {
    resources: RateLimitResources,
}

#[derive(Debug, Deserialize)]
struct RateLimitResources {
    core: RateLimitResource,
}

/// Fetch rate limit information from the GitHub API
/// Note: This endpoint does not count against your primary rate limit
pub async fn fetch_rate_limit_info(client: &Client, token: Option<&str>) -> Result<()> {
    let mut request = client.get("https://api.github.com/rate_limit");

    if let Some(token) = token {
        request = request.header(AUTHORIZATION, format!("token {}", token.trim()));
    }

    let response = request
        .send()
        .await
        .context("failed to fetch rate limit information")?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "rate limit check failed with status: {}",
            response.status()
        ));
    }

    let rate_limit: RateLimitResponse = response
        .json()
        .await
        .context("failed to parse rate limit response")?;

    let core = &rate_limit.resources.core;
    let reset_time = UNIX_EPOCH + Duration::from_secs(core.reset);
    let eta = reset_time
        .duration_since(SystemTime::now())
        .ok()
        .map(|d| format!("in {}s", d.as_secs()))
        .unwrap_or_else(|| "now".to_string());

    debug!(
        "GitHub API rate limit: {}/{} remaining (used: {}, resets {})",
        core.remaining, core.limit, core.used, eta
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RequestKind;

    #[test]
    fn parses_tree_url_with_trailing_slash() {
        let info = parse_github_url("https://github.com/foo/bar/tree/main/path/to/dir/").unwrap();

        assert_eq!(info.owner, "foo");
        assert_eq!(info.repo, "bar");
        assert_eq!(info.branch, "main");
        assert_eq!(info.path, "path/to/dir");
        assert!(info.has_trailing_slash);
        assert_eq!(info.kind, RequestKind::Tree);
    }

    #[test]
    fn blob_url_without_path_defaults_to_tree_root() {
        let info = parse_github_url("https://github.com/foo/bar/blob/main/").unwrap();

        assert_eq!(info.owner, "foo");
        assert_eq!(info.repo, "bar");
        assert_eq!(info.branch, "main");
        assert_eq!(info.path, "");
        assert!(info.has_trailing_slash);
        assert_eq!(info.kind, RequestKind::Tree);
    }

    #[test]
    fn rejects_invalid_github_url() {
        let err = parse_github_url("https://github.com/foo/bar").unwrap_err();
        assert!(
            err.to_string()
                .contains("URL must include /tree/ or /blob/"),
            "{}",
            err
        );
    }
}
