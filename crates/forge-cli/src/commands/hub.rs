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

/// Upload a dataset to HuggingFace Hub (stub -- not yet implemented).
///
/// Prints a helpful message directing the user to the Python CLI.
pub fn run_upload(path: &str, repo: &str) -> Result<()> {
    eprintln!("Hub upload is not yet implemented.");
    eprintln!("This feature is planned for the next release.");
    eprintln!("For now, use the Python huggingface_hub CLI:");
    eprintln!("  huggingface-cli upload {} {}", repo, path);
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
