use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct GitHubContent {
    pub name: String,
    pub path: String,
    pub url: String,
    pub size: Option<u64>,
    #[serde(rename = "download_url")]
    pub download_url: Option<String>,
    #[serde(rename = "type")]
    pub content_type: ContentType,
    pub sha: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GitTreeResponse {
    pub tree: Vec<GitTreeEntry>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Deserialize)]
pub struct GitTreeEntry {
    pub path: String,
    #[serde(rename = "type")]
    pub entry_type: GitTreeEntryType,
    pub size: Option<u64>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GitTreeEntryType {
    Blob,
    Tree,
    Commit,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    File,
    Dir,
    Symlink,
    Submodule,
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct RepositoryInfo {
    pub default_branch: String,
}
