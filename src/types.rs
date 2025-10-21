use std::path::PathBuf;

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
