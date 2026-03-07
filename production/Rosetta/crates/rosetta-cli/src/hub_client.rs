//! HuggingFace Hub client for downloading datasets, resolving hf:// URLs,
//! and managing local cache paths.
//!
//! Key functions:
//! - [`is_hf_url`] -- detect whether a path string is a HuggingFace URL
//! - [`parse_hf_url`] -- parse an hf:// URL into its components
//! - [`resolve_path`] -- transparently resolve hf:// URLs or pass local paths through
//! - [`download_dataset`] -- download all files from a HuggingFace dataset repo

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Parsed components of a HuggingFace URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HfRef {
    /// The repo ID in `org/repo` format.
    pub repo_id: String,
    /// Optional revision (branch, tag, or commit SHA), e.g. `@main`.
    pub revision: Option<String>,
    /// Optional subpath within the repo to scope downloads to.
    pub subpath: Option<String>,
}

/// Returns `true` if the path looks like a HuggingFace URL.
///
/// Recognized prefixes:
/// - `hf://`
/// - `huggingface://`
/// - URLs containing `huggingface.co/datasets/`
pub fn is_hf_url(path: &str) -> bool {
    path.starts_with("hf://")
        || path.starts_with("huggingface://")
        || path.contains("huggingface.co/datasets/")
}

/// Parse a HuggingFace URL into its components.
///
/// Supported formats:
/// - `hf://org/repo`
/// - `hf://org/repo@revision`
/// - `hf://org/repo/subpath`
/// - `hf://org/repo@revision/subpath`
/// - `huggingface://org/repo`
/// - `https://huggingface.co/datasets/org/repo`
pub fn parse_hf_url(url: &str) -> Result<HfRef> {
    // Strip the scheme prefix to get the path portion.
    let remainder = if let Some(rest) = url.strip_prefix("hf://") {
        rest
    } else if let Some(rest) = url.strip_prefix("huggingface://") {
        rest
    } else if let Some(idx) = url.find("huggingface.co/datasets/") {
        &url[idx + "huggingface.co/datasets/".len()..]
    } else {
        anyhow::bail!(
            "Invalid HuggingFace URL: expected format hf://org/repo, got '{url}'"
        );
    };

    // The remainder should be: org/repo[@revision][/subpath...]
    // First split off the org/repo part.
    let slash_idx = remainder
        .find('/')
        .ok_or_else(|| anyhow::anyhow!(
            "Invalid HuggingFace URL: expected format hf://org/repo, got '{url}'"
        ))?;

    let org = &remainder[..slash_idx];
    let after_org = &remainder[slash_idx + 1..];

    // after_org is: repo[@revision][/subpath...]
    // Find the next '/' which would separate repo from subpath.
    let (repo_part, subpath) = match after_org.find('/') {
        Some(idx) => {
            let sub = &after_org[idx + 1..];
            let sub = if sub.is_empty() { None } else { Some(sub.to_string()) };
            (&after_org[..idx], sub)
        }
        None => (after_org, None),
    };

    // repo_part may contain @revision, e.g. "aloha@main"
    let (repo_name, revision) = match repo_part.find('@') {
        Some(idx) => {
            let rev = &repo_part[idx + 1..];
            let rev = if rev.is_empty() { None } else { Some(rev.to_string()) };
            (&repo_part[..idx], rev)
        }
        None => (repo_part, None),
    };

    if org.is_empty() || repo_name.is_empty() {
        anyhow::bail!(
            "Invalid HuggingFace URL: expected format hf://org/repo, got '{url}'"
        );
    }

    Ok(HfRef {
        repo_id: format!("{org}/{repo_name}"),
        revision,
        subpath,
    })
}

/// Resolve a user-provided path: if it's an hf:// URL, download the dataset
/// and return the local cache path. Otherwise, return the path as-is.
///
/// This is the single entry point for transparent hf:// handling across all
/// path-accepting commands.
pub fn resolve_path(path: &str) -> Result<PathBuf> {
    if is_hf_url(path) {
        download_dataset(path, None)
    } else {
        Ok(PathBuf::from(path))
    }
}

/// Download all files from a HuggingFace dataset repository to the local cache.
///
/// Uses the `hf-hub` crate's sync API with `ureq`. Files that are already cached
/// are skipped automatically by `repo.get()`.
///
/// If `token` is provided, it overrides the default authentication. Otherwise,
/// `hf-hub` reads `HF_TOKEN` env var and `~/.cache/huggingface/token` automatically.
///
/// Returns the path to the snapshot root directory containing all downloaded files.
pub fn download_dataset(url: &str, token: Option<&str>) -> Result<PathBuf> {
    let hf_ref = parse_hf_url(url)?;

    // Build the API, optionally with an explicit token.
    let mut builder = hf_hub::api::sync::ApiBuilder::new()
        .with_progress(true);
    if let Some(t) = token {
        builder = builder.with_token(Some(t.to_string()));
    }
    let api = builder
        .build()
        .context("failed to initialize HuggingFace Hub API")?;

    let repo = if let Some(ref rev) = hf_ref.revision {
        api.repo(hf_hub::Repo::with_revision(
            hf_ref.repo_id.clone(),
            hf_hub::RepoType::Dataset,
            rev.clone(),
        ))
    } else {
        api.dataset(hf_ref.repo_id.clone())
    };

    // Fetch repo metadata to enumerate files.
    let info = repo.info().map_err(|e| {
        let msg = format!("{e}");
        if msg.contains("401")
            || msg.contains("403")
            || msg.contains("Unauthorized")
            || msg.contains("Forbidden")
        {
            anyhow::anyhow!(
                "Authentication required for '{}'. Set HF_TOKEN environment variable \
                 or run `huggingface-cli login`.\nOriginal error: {e}",
                hf_ref.repo_id
            )
        } else {
            anyhow::anyhow!("Failed to fetch repo info for '{}': {e}", hf_ref.repo_id)
        }
    })?;

    // Collect file list, optionally filtering by subpath.
    let files: Vec<String> = info
        .siblings
        .iter()
        .map(|s| s.rfilename.clone())
        .filter(|name| {
            if let Some(ref sub) = hf_ref.subpath {
                name.starts_with(sub)
            } else {
                true
            }
        })
        .collect();

    eprintln!(
        "Downloading {} files from {}",
        files.len(),
        hf_ref.repo_id
    );

    let mut snapshot_root = None;
    for (i, filename) in files.iter().enumerate() {
        eprintln!("  [{}/{}] {}", i + 1, files.len(), filename);
        let local_path = repo
            .get(filename)
            .with_context(|| format!("failed to download {filename}"))?;
        if snapshot_root.is_none() {
            snapshot_root = Some(derive_snapshot_root(&local_path, filename));
        }
    }

    snapshot_root
        .ok_or_else(|| anyhow::anyhow!("no files found in repo {}", hf_ref.repo_id))
}

/// Derive the snapshot root directory from a downloaded file's absolute path
/// and its relative name within the repo.
///
/// `file_path` is something like:
///   `~/.cache/huggingface/hub/datasets--org--repo/snapshots/{sha}/path/to/file`
/// `relative_name` is `path/to/file`.
///
/// We pop enough components from file_path to strip the relative name and arrive
/// at the snapshot root.
fn derive_snapshot_root(file_path: &Path, relative_name: &str) -> PathBuf {
    let depth = relative_name.matches('/').count() + 1;
    let mut root = file_path.to_path_buf();
    for _ in 0..depth {
        root.pop();
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_hf_url_hf_prefix() {
        assert!(is_hf_url("hf://lerobot/aloha"));
    }

    #[test]
    fn test_is_hf_url_huggingface_prefix() {
        assert!(is_hf_url("huggingface://lerobot/aloha"));
    }

    #[test]
    fn test_is_hf_url_full_https() {
        assert!(is_hf_url(
            "https://huggingface.co/datasets/lerobot/aloha"
        ));
    }

    #[test]
    fn test_is_hf_url_local_path() {
        assert!(!is_hf_url("/local/path"));
        assert!(!is_hf_url("relative/path"));
        assert!(!is_hf_url("./data"));
    }

    #[test]
    fn test_parse_hf_url_simple() {
        let r = parse_hf_url("hf://lerobot/aloha").unwrap();
        assert_eq!(
            r,
            HfRef {
                repo_id: "lerobot/aloha".to_string(),
                revision: None,
                subpath: None,
            }
        );
    }

    #[test]
    fn test_parse_hf_url_with_revision() {
        let r = parse_hf_url("hf://lerobot/aloha@main").unwrap();
        assert_eq!(
            r,
            HfRef {
                repo_id: "lerobot/aloha".to_string(),
                revision: Some("main".to_string()),
                subpath: None,
            }
        );
    }

    #[test]
    fn test_parse_hf_url_with_subpath() {
        let r = parse_hf_url("hf://lerobot/aloha/data/train").unwrap();
        assert_eq!(
            r,
            HfRef {
                repo_id: "lerobot/aloha".to_string(),
                revision: None,
                subpath: Some("data/train".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_hf_url_with_revision_and_subpath() {
        let r = parse_hf_url("hf://lerobot/aloha@v2/data/train").unwrap();
        assert_eq!(
            r,
            HfRef {
                repo_id: "lerobot/aloha".to_string(),
                revision: Some("v2".to_string()),
                subpath: Some("data/train".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_hf_url_huggingface_prefix() {
        let r = parse_hf_url("huggingface://lerobot/aloha").unwrap();
        assert_eq!(r.repo_id, "lerobot/aloha");
    }

    #[test]
    fn test_parse_hf_url_https() {
        let r = parse_hf_url("https://huggingface.co/datasets/lerobot/aloha").unwrap();
        assert_eq!(r.repo_id, "lerobot/aloha");
    }

    #[test]
    fn test_parse_hf_url_no_slash() {
        let err = parse_hf_url("hf://just_a_name").unwrap_err();
        assert!(err.to_string().contains("expected format hf://org/repo"));
    }

    #[test]
    fn test_derive_snapshot_root_simple() {
        let path = PathBuf::from("/cache/snapshots/abc123/config.json");
        let root = derive_snapshot_root(&path, "config.json");
        assert_eq!(root, PathBuf::from("/cache/snapshots/abc123"));
    }

    #[test]
    fn test_derive_snapshot_root_nested() {
        let path = PathBuf::from("/cache/snapshots/abc123/data/train/0000.parquet");
        let root = derive_snapshot_root(&path, "data/train/0000.parquet");
        assert_eq!(root, PathBuf::from("/cache/snapshots/abc123"));
    }

    #[test]
    fn test_resolve_path_local() {
        // Local paths are returned as-is (no existence check in resolve_path).
        let p = resolve_path("/some/local/path").unwrap();
        assert_eq!(p, PathBuf::from("/some/local/path"));
    }
}
