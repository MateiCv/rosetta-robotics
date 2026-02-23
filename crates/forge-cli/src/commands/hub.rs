//! HuggingFace Hub subcommands: download and upload.

use std::path::Path;

use anyhow::Result;

/// Download a dataset from HuggingFace Hub.
///
/// If `repo` is not already an hf:// URL, `hf://` is prepended automatically.
/// The downloaded dataset is cached locally via hf-hub. If `output` is provided,
/// the cache path is printed to stderr and the dataset is noted at that output
/// location. If `output` is None, the cache path is printed to stdout (suitable
/// for piping: `forge inspect $(forge hub download lerobot/aloha)`).
pub fn run_download(
    repo: &str,
    output: Option<&Path>,
    token: Option<&str>,
) -> Result<()> {
    // Ensure the input is an hf:// URL.
    let url = if crate::hub_client::is_hf_url(repo) {
        repo.to_string()
    } else {
        format!("hf://{repo}")
    };

    let cached_path = crate::hub_client::download_dataset(&url, token)?;

    if let Some(out) = output {
        // Copy from cache to the user-specified output directory.
        copy_dir_recursive(&cached_path, out)?;
        eprintln!("Dataset copied to: {}", out.display());
        eprintln!("  (cached at: {})", cached_path.display());
    } else {
        // Print cache path to stdout for piping.
        println!("{}", cached_path.display());
    }

    // Print summary to stderr.
    let file_count = count_files(&cached_path);
    let total_size = dir_size(&cached_path);
    eprintln!(
        "Summary: {} files, {:.1} MB",
        file_count,
        total_size as f64 / (1024.0 * 1024.0)
    );

    Ok(())
}

/// Upload a dataset to HuggingFace Hub.
///
/// Validates the local path, requires authentication, creates the repo
/// (idempotent), and uploads all files with LFS for large files (>=10 MB).
pub fn run_upload(path: &str, repo: &str, token: Option<&str>, private: bool) -> Result<()> {
    // Validate local path exists and is a directory.
    let local_path = std::path::Path::new(path);
    if !local_path.exists() {
        anyhow::bail!("Dataset path does not exist: {path}");
    }
    if !local_path.is_dir() {
        anyhow::bail!("Dataset path is not a directory: {path}");
    }

    // Require a token for upload.
    let token = token.ok_or_else(|| {
        anyhow::anyhow!(
            "Upload requires authentication. Set HF_TOKEN environment variable \
             or pass --token <token>.\n\
             Get a write-scoped token at: https://huggingface.co/settings/tokens"
        )
    })?;

    let uploader = crate::hub_upload::HubUploader::new(token.to_string());

    // Create repo (idempotent -- 409 is handled as success).
    eprintln!("Creating repository: {repo}");
    uploader.create_repo(repo, private)?;

    // Upload all files.
    eprintln!("Uploading dataset from: {path}");
    uploader.upload_folder(repo, local_path)?;

    // Print verification hint.
    eprintln!();
    eprintln!("Verify: forge hub download {repo}");
    eprintln!("   URL: https://huggingface.co/datasets/{repo}");

    Ok(())
}

/// Recursively copy a directory tree from `src` to `dst`.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            // Follow symlinks (hf-hub uses symlinks in its cache).
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

/// Count files recursively in a directory.
fn count_files(path: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                count += count_files(&entry.path());
            } else {
                count += 1;
            }
        }
    }
    count
}

/// Calculate total size of files in a directory (recursively).
fn dir_size(path: &Path) -> u64 {
    let mut size = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                size += dir_size(&entry.path());
            } else {
                size += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    size
}
