use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::github::types::{ContentType, GitHubContent};
use crate::types::RequestInfo;

pub fn format_path_for_log(path: &Path) -> String {
    if path.is_absolute() {
        return path.display().to_string();
    }

    match path.components().next() {
        Some(Component::CurDir) | Some(Component::ParentDir) | None => path.display().to_string(),
        _ => format!("./{}", path.display()),
    }
}

pub fn compute_base_and_default_output(
    request: &RequestInfo,
    treat_as_single_file: bool,
    file_path_override: Option<&str>,
) -> (PathBuf, PathBuf) {
    if treat_as_single_file {
        let path_str = file_path_override.unwrap_or(&request.path);
        let file_path = Path::new(path_str);
        let base = file_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let default_output = PathBuf::from(".");
        (normalize_base(base), default_output)
    } else {
        let trimmed = request.path.trim_matches('/');
        let base = if trimmed.is_empty() {
            PathBuf::new()
        } else {
            PathBuf::from(trimmed)
        };

        let default_output = if request.has_trailing_slash || base.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            base.file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
        };

        (normalize_base(base), default_output)
    }
}

pub fn determine_paths(request: &RequestInfo, contents: &[GitHubContent]) -> (PathBuf, PathBuf) {
    let is_single_file = contents.len() == 1 && contents[0].content_type == ContentType::File;
    compute_base_and_default_output(
        request,
        is_single_file,
        contents.first().map(|item| item.path.as_str()),
    )
}

fn normalize_base(base: PathBuf) -> PathBuf {
    if base.as_os_str().is_empty() {
        PathBuf::new()
    } else {
        base
    }
}

pub fn ensure_directory(dir: &Path) -> Result<()> {
    if dir.exists() {
        if !dir.is_dir() {
            return Err(anyhow!(
                "output path {} exists but is not a directory",
                dir.display()
            ));
        }
    } else {
        fs::create_dir_all(dir)
            .with_context(|| format!("failed to create output directory {}", dir.display()))?;
    }
    Ok(())
}

pub fn relative_path(base_path: &Path, item: &GitHubContent) -> Result<PathBuf> {
    let full_path = Path::new(&item.path);
    let mut relative = if base_path.as_os_str().is_empty() {
        full_path.to_path_buf()
    } else {
        full_path
            .strip_prefix(base_path)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| full_path.to_path_buf())
    };

    if relative.as_os_str().is_empty() {
        relative = PathBuf::from(&item.name);
    }

    let mut sanitized = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => sanitized.push(part),
            Component::CurDir => continue,
            _ => {
                return Err(anyhow!(
                    "refusing to write outside the output directory ({})",
                    item.path
                ));
            }
        }
    }

    if sanitized.as_os_str().is_empty() {
        sanitized.push(&item.name);
    }

    Ok(sanitized)
}

pub fn describe_download_target(
    output_dir: &Path,
    base_path: &Path,
    contents: &[GitHubContent],
) -> Result<String> {
    if contents.len() == 1 && contents[0].content_type == ContentType::File {
        let relative = relative_path(base_path, &contents[0])?;
        let target = output_dir.join(relative);
        Ok(format_path_for_log(&target))
    } else {
        Ok(format_path_for_log(output_dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::types::GitHubContent;
    use crate::types::RequestKind;

    fn make_file(path: &str) -> GitHubContent {
        let name = Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path)
            .to_string();
        GitHubContent {
            name,
            path: path.to_string(),
            url: format!("https://api.example.com/repos/file/{}", path),
            size: Some(42),
            download_url: Some(format!("https://raw.example.com/repos/file/{}", path)),
            content_type: ContentType::File,
            sha: Some("da39a3ee5e6b4b0d3255bfef95601890afd80709".to_string()),
        }
    }

    fn make_dir(path: &str) -> GitHubContent {
        let name = Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path)
            .to_string();
        GitHubContent {
            name,
            path: path.to_string(),
            url: format!("https://api.example.com/repos/dir/{}", path),
            size: None,
            download_url: None,
            content_type: ContentType::Dir,
            sha: None,
        }
    }

    #[test]
    fn determines_output_for_single_file() {
        let request = RequestInfo {
            owner: "foo".into(),
            repo: "bar".into(),
            branch: "main".into(),
            path: "dir/file.txt".into(),
            has_trailing_slash: false,
            kind: RequestKind::Blob,
        };
        let contents = vec![make_file("dir/file.txt")];
        let (_base, output) = determine_paths(&request, &contents);
        assert_eq!(output, PathBuf::from("."));
    }

    #[test]
    fn determine_output_for_directory_without_trailing_slash() {
        let request = RequestInfo {
            owner: "foo".into(),
            repo: "bar".into(),
            branch: "main".into(),
            path: "dir/subdir".into(),
            has_trailing_slash: false,
            kind: RequestKind::Tree,
        };
        let contents = vec![make_dir("dir/subdir")];
        let (_base, output) = determine_paths(&request, &contents);
        assert_eq!(output, PathBuf::from("subdir"));
    }

    #[test]
    fn relative_path_removes_base_prefix() {
        let base = Path::new("dir/subdir");
        let item = make_file("dir/subdir/file.txt");
        let relative = relative_path(base, &item).unwrap();
        assert_eq!(relative, PathBuf::from("file.txt"));
    }

    #[test]
    fn relative_path_rejects_traversal() {
        let base = Path::new("dir");
        let item = make_file("dir/../evil.txt");
        let err = relative_path(base, &item).unwrap_err();
        assert!(err
            .to_string()
            .contains("refusing to write outside the output directory"));
    }

    #[test]
    fn test_format_path_for_log_absolute() {
        let path = Path::new("/home/user/file.txt");
        assert_eq!(format_path_for_log(path), "/home/user/file.txt");
    }

    #[test]
    fn test_format_path_for_log_relative() {
        let path = Path::new("file.txt");
        assert_eq!(format_path_for_log(path), "./file.txt");
    }

    #[test]
    fn test_format_path_for_log_current_dir() {
        let path = Path::new("./file.txt");
        assert_eq!(format_path_for_log(path), "./file.txt");
    }

    #[test]
    fn test_format_path_for_log_parent_dir() {
        let path = Path::new("../file.txt");
        assert_eq!(format_path_for_log(path), "../file.txt");
    }

    #[test]
    fn test_compute_base_and_default_output_single_file() {
        let request = RequestInfo {
            owner: "owner".into(),
            repo: "repo".into(),
            branch: "main".into(),
            path: "path/to/file.txt".into(),
            has_trailing_slash: false,
            kind: RequestKind::Blob,
        };
        let (base, output) = compute_base_and_default_output(&request, true, Some("path/to/file.txt"));
        assert_eq!(base, PathBuf::from("path/to"));
        assert_eq!(output, PathBuf::from("."));
    }

    #[test]
    fn test_compute_base_and_default_output_directory() {
        let request = RequestInfo {
            owner: "owner".into(),
            repo: "repo".into(),
            branch: "main".into(),
            path: "path/to/dir".into(),
            has_trailing_slash: false,
            kind: RequestKind::Tree,
        };
        let (base, output) = compute_base_and_default_output(&request, false, None);
        assert_eq!(base, PathBuf::from("path/to/dir"));
        assert_eq!(output, PathBuf::from("dir"));
    }

    #[test]
    fn test_compute_base_and_default_output_directory_with_trailing_slash() {
        let request = RequestInfo {
            owner: "owner".into(),
            repo: "repo".into(),
            branch: "main".into(),
            path: "path/to/dir".into(),
            has_trailing_slash: true,
            kind: RequestKind::Tree,
        };
        let (base, output) = compute_base_and_default_output(&request, false, None);
        assert_eq!(base, PathBuf::from("path/to/dir"));
        assert_eq!(output, PathBuf::from("."));
    }

    #[test]
    fn test_compute_base_and_default_output_root() {
        let request = RequestInfo {
            owner: "owner".into(),
            repo: "repo".into(),
            branch: "main".into(),
            path: "".into(),
            has_trailing_slash: false,
            kind: RequestKind::Tree,
        };
        let (base, output) = compute_base_and_default_output(&request, false, None);
        assert_eq!(base, PathBuf::new());
        assert_eq!(output, PathBuf::from("."));
    }

    #[test]
    fn test_relative_path_empty_base() {
        let base = Path::new("");
        let item = make_file("file.txt");
        let relative = relative_path(base, &item).unwrap();
        assert_eq!(relative, PathBuf::from("file.txt"));
    }

    #[test]
    fn test_relative_path_empty_result_uses_name() {
        let base = Path::new("dir");
        let mut item = make_file("dir");
        item.name = "filename.txt".to_string();
        let relative = relative_path(base, &item).unwrap();
        assert_eq!(relative, PathBuf::from("filename.txt"));
    }

    #[test]
    fn test_relative_path_sanitizes_current_dir() {
        let base = Path::new("dir");
        let item = make_file("dir/./file.txt");
        let relative = relative_path(base, &item).unwrap();
        assert_eq!(relative, PathBuf::from("file.txt"));
    }

    #[test]
    fn test_normalize_base_empty() {
        assert_eq!(normalize_base(PathBuf::new()), PathBuf::new());
    }

    #[test]
    fn test_normalize_base_non_empty() {
        assert_eq!(normalize_base(PathBuf::from("dir")), PathBuf::from("dir"));
    }

    #[test]
    fn test_describe_download_target_single_file() {
        let output_dir = Path::new("output");
        let base_path = Path::new("dir");
        let contents = vec![make_file("dir/file.txt")];
        let result = describe_download_target(output_dir, base_path, &contents).unwrap();
        assert_eq!(result, "./output/file.txt");
    }

    #[test]
    fn test_describe_download_target_directory() {
        let output_dir = Path::new("output");
        let base_path = Path::new("dir");
        let contents = vec![make_dir("dir"), make_file("dir/file1.txt"), make_file("dir/file2.txt")];
        let result = describe_download_target(output_dir, base_path, &contents).unwrap();
        assert_eq!(result, "./output");
    }
}
