//! Pure functions for download strategy validation and selection.
//!
//! This module contains testable logic for determining optimal download strategies
//! based on git availability, request type, and file counts.

use crate::cli::DownloadStrategy;
use crate::types::RequestKind;

/// Validates and resolves a download strategy based on git availability and request type.
///
/// For `DownloadStrategy::Auto`, selects the most appropriate strategy:
/// - If git is available: prefers Git
/// - If git is not available and requesting a tree: prefers Zip
/// - Otherwise: prefers API
///
/// For explicit strategies, returns them unchanged.
pub fn validate_download_strategy(
    strategy: DownloadStrategy,
    git_available: bool,
    request_kind: RequestKind,
) -> DownloadStrategy {
    match strategy {
        DownloadStrategy::Auto => {
            if git_available {
                DownloadStrategy::Git
            } else if request_kind == RequestKind::Tree {
                DownloadStrategy::Zip
            } else {
                DownloadStrategy::Api
            }
        }
        s => s,
    }
}

/// Selects the optimal download strategy based on multiple factors.
///
/// Decision tree:
/// - If git is not available:
///   - If file count > 100: Zip (efficient for large file sets)
///   - Otherwise: API
/// - If git is available:
///   - If downloading whole repo: Git (most efficient)
///   - If file count > 50: Zip (efficient for medium file sets)
///   - Otherwise: API (precise control for small file sets)
pub fn select_optimal_strategy(
    git_available: bool,
    is_whole_repo: bool,
    file_count: usize,
) -> DownloadStrategy {
    if !git_available {
        if file_count > 100 {
            DownloadStrategy::Zip
        } else {
            DownloadStrategy::Api
        }
    } else if is_whole_repo {
        DownloadStrategy::Git
    } else if file_count > 50 {
        DownloadStrategy::Zip
    } else {
        DownloadStrategy::Api
    }
}

/// Determines if a request represents a whole repository download.
///
/// A path is considered a whole repo if it's empty or just a root slash.
pub fn is_whole_repo_request(path: &str) -> bool {
    path.is_empty() || path == "/"
}

/// Determines the fallback strategy order for Auto mode when git is available.
///
/// Returns strategies in order of preference: [Git, Zip, Api]
pub fn git_available_fallback_order() -> [DownloadStrategy; 3] {
    [
        DownloadStrategy::Git,
        DownloadStrategy::Zip,
        DownloadStrategy::Api,
    ]
}

/// Determines the fallback strategy order for Auto mode when git is not available
/// and requesting a whole repo.
///
/// Returns strategies in order of preference: [Zip, Api]
pub fn no_git_whole_repo_fallback_order() -> [DownloadStrategy; 2] {
    [DownloadStrategy::Zip, DownloadStrategy::Api]
}

/// Determines the fallback strategy order for Auto mode when git is not available
/// and requesting a specific path.
///
/// Returns strategies in order of preference: [Api, Zip]
pub fn no_git_specific_path_fallback_order() -> [DownloadStrategy; 2] {
    [DownloadStrategy::Api, DownloadStrategy::Zip]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_download_strategy() {
        // Auto mode selects based on git availability and request type
        assert_eq!(
            validate_download_strategy(DownloadStrategy::Auto, true, RequestKind::Tree),
            DownloadStrategy::Git
        );
        assert_eq!(
            validate_download_strategy(DownloadStrategy::Auto, false, RequestKind::Tree),
            DownloadStrategy::Zip
        );
        assert_eq!(
            validate_download_strategy(DownloadStrategy::Auto, false, RequestKind::Blob),
            DownloadStrategy::Api
        );

        // Explicit strategies are unchanged
        assert_eq!(
            validate_download_strategy(DownloadStrategy::Api, true, RequestKind::Tree),
            DownloadStrategy::Api
        );
        assert_eq!(
            validate_download_strategy(DownloadStrategy::Git, false, RequestKind::Blob),
            DownloadStrategy::Git
        );
    }

    #[test]
    fn test_select_optimal_strategy() {
        // Without git: use Zip for large file counts, API otherwise
        assert_eq!(
            select_optimal_strategy(false, false, 150),
            DownloadStrategy::Zip
        );
        assert_eq!(
            select_optimal_strategy(false, false, 50),
            DownloadStrategy::Api
        );
        assert_eq!(
            select_optimal_strategy(false, false, 101),
            DownloadStrategy::Zip
        ); // Boundary

        // With git: whole repo uses Git, partial uses Zip for many files
        assert_eq!(
            select_optimal_strategy(true, true, 1000),
            DownloadStrategy::Git
        );
        assert_eq!(
            select_optimal_strategy(true, false, 100),
            DownloadStrategy::Zip
        );
        assert_eq!(
            select_optimal_strategy(true, false, 20),
            DownloadStrategy::Api
        );
        assert_eq!(
            select_optimal_strategy(true, false, 51),
            DownloadStrategy::Zip
        ); // Boundary
    }

    #[test]
    fn test_is_whole_repo_request() {
        assert!(is_whole_repo_request(""));
        assert!(is_whole_repo_request("/"));
        assert!(!is_whole_repo_request("src"));
        assert!(!is_whole_repo_request("src/main.rs"));
    }

    #[test]
    fn test_fallback_orders() {
        // Git available: try Git, then Zip, then API
        let order = git_available_fallback_order();
        assert_eq!(
            order,
            [
                DownloadStrategy::Git,
                DownloadStrategy::Zip,
                DownloadStrategy::Api
            ]
        );

        // No git, whole repo: try Zip, then API
        let order = no_git_whole_repo_fallback_order();
        assert_eq!(order, [DownloadStrategy::Zip, DownloadStrategy::Api]);

        // No git, specific path: try API, then Zip
        let order = no_git_specific_path_fallback_order();
        assert_eq!(order, [DownloadStrategy::Api, DownloadStrategy::Zip]);
    }
}
