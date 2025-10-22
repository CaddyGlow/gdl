use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_github_content_file() {
        let json = r#"{
            "name": "test.txt",
            "path": "dir/test.txt",
            "url": "https://api.github.com/repos/owner/repo/contents/dir/test.txt",
            "size": 1234,
            "download_url": "https://raw.githubusercontent.com/owner/repo/main/dir/test.txt",
            "type": "file",
            "sha": "abc123"
        }"#;

        let content: GitHubContent = serde_json::from_str(json).unwrap();
        assert_eq!(content.name, "test.txt");
        assert_eq!(content.path, "dir/test.txt");
        assert_eq!(content.size, Some(1234));
        assert_eq!(content.content_type, ContentType::File);
        assert_eq!(content.sha, Some("abc123".to_string()));
    }

    #[test]
    fn test_deserialize_github_content_dir() {
        let json = r#"{
            "name": "dirname",
            "path": "dir/dirname",
            "url": "https://api.github.com/repos/owner/repo/contents/dir/dirname",
            "type": "dir"
        }"#;

        let content: GitHubContent = serde_json::from_str(json).unwrap();
        assert_eq!(content.name, "dirname");
        assert_eq!(content.content_type, ContentType::Dir);
        assert_eq!(content.size, None);
        assert_eq!(content.download_url, None);
    }

    #[test]
    fn test_deserialize_content_type_symlink() {
        let json = r#"{"name":"link","path":"link","url":"","type":"symlink"}"#;
        let content: GitHubContent = serde_json::from_str(json).unwrap();
        assert_eq!(content.content_type, ContentType::Symlink);
    }

    #[test]
    fn test_deserialize_content_type_submodule() {
        let json = r#"{"name":"sub","path":"sub","url":"","type":"submodule"}"#;
        let content: GitHubContent = serde_json::from_str(json).unwrap();
        assert_eq!(content.content_type, ContentType::Submodule);
    }

    #[test]
    fn test_deserialize_content_type_other() {
        let json = r#"{"name":"unknown","path":"unknown","url":"","type":"unknown_type"}"#;
        let content: GitHubContent = serde_json::from_str(json).unwrap();
        assert_eq!(content.content_type, ContentType::Other);
    }

    #[test]
    fn test_deserialize_git_tree_response() {
        let json = r#"{
            "tree": [
                {"path": "file1.txt", "type": "blob", "size": 100},
                {"path": "dir1", "type": "tree", "size": null}
            ],
            "truncated": false
        }"#;

        let tree: GitTreeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(tree.tree.len(), 2);
        assert!(!tree.truncated);
        assert_eq!(tree.tree[0].path, "file1.txt");
        assert_eq!(tree.tree[0].entry_type, GitTreeEntryType::Blob);
        assert_eq!(tree.tree[0].size, Some(100));
        assert_eq!(tree.tree[1].entry_type, GitTreeEntryType::Tree);
    }

    #[test]
    fn test_deserialize_git_tree_response_truncated() {
        let json = r#"{"tree": [], "truncated": true}"#;
        let tree: GitTreeResponse = serde_json::from_str(json).unwrap();
        assert!(tree.truncated);
    }

    #[test]
    fn test_deserialize_git_tree_response_no_truncated_field() {
        let json = r#"{"tree": []}"#;
        let tree: GitTreeResponse = serde_json::from_str(json).unwrap();
        assert!(!tree.truncated); // default should be false
    }

    #[test]
    fn test_deserialize_git_tree_entry_types() {
        let blob_json = r#"{"path":"file","type":"blob","size":10}"#;
        let blob: GitTreeEntry = serde_json::from_str(blob_json).unwrap();
        assert_eq!(blob.entry_type, GitTreeEntryType::Blob);

        let tree_json = r#"{"path":"dir","type":"tree"}"#;
        let tree: GitTreeEntry = serde_json::from_str(tree_json).unwrap();
        assert_eq!(tree.entry_type, GitTreeEntryType::Tree);

        let commit_json = r#"{"path":"submodule","type":"commit"}"#;
        let commit: GitTreeEntry = serde_json::from_str(commit_json).unwrap();
        assert_eq!(commit.entry_type, GitTreeEntryType::Commit);
    }

    #[test]
    fn test_deserialize_repository_info() {
        let json = r#"{"default_branch": "main"}"#;
        let info: RepositoryInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.default_branch, "main");
    }

    #[test]
    fn test_content_type_clone() {
        let content_type = ContentType::File;
        let cloned = content_type.clone();
        assert_eq!(content_type, cloned);
    }
}
