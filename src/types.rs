use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    Tree,
    Blob,
}

#[derive(Debug, Clone)]
pub struct RequestInfo {
    pub owner: String,
    pub repo: String,
    pub branch: String,
    pub path: String,
    pub has_trailing_slash: bool,
    pub kind: RequestKind,
}

#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub size: Option<u64>,
}

#[derive(Debug)]
pub struct DownloadTask {
    pub item: crate::github::types::GitHubContent,
    pub target_path: PathBuf,
    pub size: Option<u64>,
}

#[derive(Debug)]
pub struct FileCopyTask {
    pub item_path: String,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub size: Option<u64>,
}

impl crate::overwrite::TargetPath for DownloadTask {
    fn path(&self) -> &Path {
        &self.target_path
    }

    fn size(&self) -> u64 {
        self.size.unwrap_or(0)
    }
}

impl crate::overwrite::TargetPath for FileCopyTask {
    fn path(&self) -> &Path {
        &self.target_path
    }

    fn size(&self) -> u64 {
        self.size.unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overwrite::TargetPath;
    use std::path::PathBuf;

    #[test]
    fn test_request_kind_equality() {
        assert_eq!(RequestKind::Tree, RequestKind::Tree);
        assert_eq!(RequestKind::Blob, RequestKind::Blob);
        assert_ne!(RequestKind::Tree, RequestKind::Blob);
    }

    #[test]
    fn test_download_task_target_path() {
        use crate::github::types::{ContentType, GitHubContent};

        let task = DownloadTask {
            item: GitHubContent {
                name: "test.txt".to_string(),
                path: "dir/test.txt".to_string(),
                url: "https://example.com".to_string(),
                size: Some(100),
                download_url: Some("https://example.com/download".to_string()),
                content_type: ContentType::File,
                sha: Some("abc123".to_string()),
            },
            target_path: PathBuf::from("output/test.txt"),
            size: Some(100),
        };

        assert_eq!(task.path(), Path::new("output/test.txt"));
        assert_eq!(task.size(), 100);
    }

    #[test]
    fn test_download_task_size_none() {
        use crate::github::types::{ContentType, GitHubContent};

        let task = DownloadTask {
            item: GitHubContent {
                name: "test.txt".to_string(),
                path: "dir/test.txt".to_string(),
                url: "https://example.com".to_string(),
                size: None,
                download_url: Some("https://example.com/download".to_string()),
                content_type: ContentType::File,
                sha: None,
            },
            target_path: PathBuf::from("output/test.txt"),
            size: None,
        };

        assert_eq!(task.size(), 0);
    }

    #[test]
    fn test_file_copy_task_target_path() {
        let task = FileCopyTask {
            item_path: "src/file.txt".to_string(),
            source_path: PathBuf::from("temp/file.txt"),
            target_path: PathBuf::from("dest/file.txt"),
            size: Some(200),
        };

        assert_eq!(task.path(), Path::new("dest/file.txt"));
        assert_eq!(task.size(), 200);
    }

    #[test]
    fn test_file_copy_task_size_none() {
        let task = FileCopyTask {
            item_path: "src/file.txt".to_string(),
            source_path: PathBuf::from("temp/file.txt"),
            target_path: PathBuf::from("dest/file.txt"),
            size: None,
        };

        assert_eq!(task.size(), 0);
    }

    #[test]
    fn test_request_info_clone() {
        let request = RequestInfo {
            owner: "owner".to_string(),
            repo: "repo".to_string(),
            branch: "main".to_string(),
            path: "src/main.rs".to_string(),
            has_trailing_slash: false,
            kind: RequestKind::Blob,
        };

        let cloned = request.clone();
        assert_eq!(cloned.owner, "owner");
        assert_eq!(cloned.repo, "repo");
        assert_eq!(cloned.branch, "main");
        assert_eq!(cloned.path, "src/main.rs");
        assert!(!cloned.has_trailing_slash);
        assert_eq!(cloned.kind, RequestKind::Blob);
    }

    #[test]
    fn test_file_metadata_clone() {
        let metadata = FileMetadata { size: Some(1024) };
        let cloned = metadata.clone();
        assert_eq!(cloned.size, Some(1024));
    }
}
