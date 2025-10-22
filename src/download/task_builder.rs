//! Pure functions for building download tasks.
//!
//! This module contains testable logic for constructing download tasks from
//! GitHub content items and filtering downloadable content.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::github::types::{ContentType, GitHubContent};
use crate::paths::relative_path;
use crate::types::{DownloadTask, FileMetadata};

/// Builds a download task from a GitHub content item.
///
/// Calculates the target path and retrieves file size from metadata.
///
/// # Arguments
///
/// * `item` - The GitHub content item (must be a file)
/// * `base_path` - The base path to calculate relative paths from
/// * `output_dir` - The output directory for downloaded files
/// * `file_metadata` - Map of file paths to their metadata (for size information)
///
/// # Returns
///
/// A `DownloadTask` ready for execution
pub fn build_download_task(
    item: &GitHubContent,
    base_path: &Path,
    output_dir: &Path,
    file_metadata: &HashMap<String, FileMetadata>,
) -> Result<DownloadTask> {
    let relative = relative_path(base_path, item)?;
    let target_path = output_dir.join(&relative);
    let size = file_metadata.get(&item.path).and_then(|meta| meta.size);

    Ok(DownloadTask {
        item: item.clone(),
        target_path,
        size,
    })
}

/// Builds download tasks from a list of GitHub content items.
///
/// Only processes items of type `File`. Directories and other types are ignored.
///
/// # Arguments
///
/// * `contents` - List of GitHub content items
/// * `base_path` - The base path to calculate relative paths from
/// * `output_dir` - The output directory for downloaded files
/// * `file_metadata` - Map of file paths to their metadata
///
/// # Returns
///
/// A vector of `DownloadTask` instances
pub fn build_download_tasks(
    contents: &[GitHubContent],
    base_path: &Path,
    output_dir: &Path,
    file_metadata: &HashMap<String, FileMetadata>,
) -> Result<Vec<DownloadTask>> {
    contents
        .iter()
        .filter(|item| item.content_type == ContentType::File)
        .map(|item| build_download_task(item, base_path, output_dir, file_metadata))
        .collect()
}

/// Filters downloadable items from a list of GitHub content.
///
/// Returns only items of type `File`.
pub fn filter_downloadable_items(contents: &[GitHubContent]) -> Vec<&GitHubContent> {
    contents
        .iter()
        .filter(|item| item.content_type == ContentType::File)
        .collect()
}

/// Filters directory items from a list of GitHub content.
///
/// Returns only items of type `Dir`.
pub fn filter_directory_items(contents: &[GitHubContent]) -> Vec<&GitHubContent> {
    contents
        .iter()
        .filter(|item| item.content_type == ContentType::Dir)
        .collect()
}

/// Categorizes GitHub content items by type.
///
/// Returns a tuple of (files, directories, skipped) where:
/// - files: items of type File
/// - directories: items of type Dir
/// - skipped: items of other types (Symlink, Submodule, Other)
pub fn categorize_content_items(
    contents: Vec<GitHubContent>,
) -> (Vec<GitHubContent>, Vec<GitHubContent>, Vec<GitHubContent>) {
    let mut files = Vec::new();
    let mut directories = Vec::new();
    let mut skipped = Vec::new();

    for item in contents {
        match item.content_type {
            ContentType::File => files.push(item),
            ContentType::Dir => directories.push(item),
            ContentType::Symlink | ContentType::Submodule | ContentType::Other => {
                skipped.push(item)
            }
        }
    }

    (files, directories, skipped)
}

/// Checks if a content item should be skipped during download.
///
/// Returns true for Symlink, Submodule, or Other content types.
pub fn should_skip_item(item: &GitHubContent) -> bool {
    matches!(
        item.content_type,
        ContentType::Symlink | ContentType::Submodule | ContentType::Other
    )
}

/// Calculates the target file path for a download item.
///
/// # Arguments
///
/// * `item` - The GitHub content item
/// * `base_path` - The base path to calculate relative paths from
/// * `output_dir` - The output directory
///
/// # Returns
///
/// The complete target path for the downloaded file
pub fn calculate_target_path(
    item: &GitHubContent,
    base_path: &Path,
    output_dir: &Path,
) -> Result<PathBuf> {
    let relative = relative_path(base_path, item)
        .with_context(|| format!("failed to calculate relative path for {}", item.path))?;
    Ok(output_dir.join(&relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_file_item(path: &str) -> GitHubContent {
        GitHubContent {
            name: path.split('/').last().unwrap_or(path).to_string(),
            path: path.to_string(),
            url: format!("https://api.github.com/test/{}", path),
            size: Some(1024),
            download_url: Some(format!("https://raw.githubusercontent.com/test/{}", path)),
            content_type: ContentType::File,
            sha: Some("abc123".to_string()),
        }
    }

    fn create_test_dir_item(path: &str) -> GitHubContent {
        GitHubContent {
            name: path.to_string(),
            path: path.to_string(),
            url: format!("https://api.github.com/test/{}", path),
            size: None,
            download_url: None,
            content_type: ContentType::Dir,
            sha: None,
        }
    }

    fn create_test_symlink_item() -> GitHubContent {
        GitHubContent {
            name: "link".to_string(),
            path: "link".to_string(),
            url: "https://api.github.com/test/link".to_string(),
            size: None,
            download_url: None,
            content_type: ContentType::Symlink,
            sha: None,
        }
    }

    #[test]
    fn test_build_download_task() {
        let item = create_test_file_item("src/main.rs");
        let mut metadata = HashMap::new();
        metadata.insert("src/main.rs".to_string(), FileMetadata { size: Some(1024) });

        let task =
            build_download_task(&item, Path::new("src"), Path::new("output"), &metadata).unwrap();
        assert_eq!(task.target_path, PathBuf::from("output/main.rs"));
        assert_eq!(task.size, Some(1024));

        // Without metadata
        let task = build_download_task(
            &item,
            Path::new("src"),
            Path::new("output"),
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(task.size, None);
    }

    #[test]
    fn test_build_download_tasks() {
        let items = vec![
            create_test_file_item("src/main.rs"),
            create_test_dir_item("src/tests"),
            create_test_file_item("src/lib.rs"),
        ];

        let tasks = build_download_tasks(
            &items,
            Path::new("src"),
            Path::new("output"),
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(tasks.len(), 2); // Only files, directories filtered out
    }

    #[test]
    fn test_filter_items() {
        let items = vec![
            create_test_file_item("file1.txt"),
            create_test_dir_item("dir1"),
            create_test_file_item("file2.txt"),
            create_test_symlink_item(),
        ];

        // Filter files
        let files = filter_downloadable_items(&items);
        assert_eq!(files.len(), 2);

        // Filter directories
        let dirs = filter_directory_items(&items);
        assert_eq!(dirs.len(), 1);
    }

    #[test]
    fn test_categorize_content_items() {
        let items = vec![
            create_test_file_item("file.txt"),
            create_test_dir_item("dir"),
            create_test_symlink_item(),
        ];

        let (files, dirs, skipped) = categorize_content_items(items);
        assert_eq!(files.len(), 1);
        assert_eq!(dirs.len(), 1);
        assert_eq!(skipped.len(), 1);
    }

    #[test]
    fn test_should_skip_item() {
        assert!(!should_skip_item(&create_test_file_item("file.txt")));
        assert!(!should_skip_item(&create_test_dir_item("dir")));
        assert!(should_skip_item(&create_test_symlink_item()));
    }
}
