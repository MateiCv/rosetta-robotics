//! HuggingFace Hub upload client for pushing datasets via the HF HTTP API.
//!
//! Uploads files using the HuggingFace commit API, routing large files
//! (>=10 MB) through LFS presigned upload URLs and committing small files
//! inline via multipart form data.
//!
//! Key types:
//! - [`HubUploader`] -- main client struct (synchronous, uses ureq)
//! - [`FileEntry`] -- metadata for a file to upload
//! - [`LfsPointer`] -- LFS pointer info after presigned upload

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// Files at or above this size (10 MB) are uploaded via LFS presigned URLs.
const LFS_THRESHOLD: u64 = 10 * 1024 * 1024;

/// Metadata for a single file to upload.
#[derive(Debug, Clone)]
struct FileEntry {
    /// Path relative to the dataset root (forward-slash separated).
    relative_path: String,
    /// Absolute path on disk.
    absolute_path: PathBuf,
    /// File size in bytes.
    size: u64,
}

/// LFS pointer info returned after a presigned upload completes.
#[derive(Debug, Clone)]
struct LfsPointer {
    /// Path relative to the dataset root.
    relative_path: String,
    /// SHA-256 hex digest of the file content.
    oid: String,
    /// File size in bytes.
    size: u64,
}

/// Synchronous HTTP client for uploading datasets to HuggingFace Hub.
pub struct HubUploader {
    token: String,
    endpoint: String,
}

impl HubUploader {
    /// Create a new uploader targeting the default HuggingFace endpoint.
    pub fn new(token: String) -> Self {
        Self {
            token,
            endpoint: "https://huggingface.co".to_string(),
        }
    }

    /// Create or ensure the existence of a dataset repository on HuggingFace.
    ///
    /// A 409 Conflict response (repo already exists) is treated as success.
    /// Returns an error for authentication failures (401/403).
    pub fn create_repo(&self, repo_id: &str, private: bool) -> Result<()> {
        let url = format!("{}/api/repos/create", self.endpoint);

        let body = serde_json::json!({
            "type": "dataset",
            "name": repo_id,
            "private": private,
        });

        let resp = ureq::post(&url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .send_json(&body);

        match resp {
            Ok(_) => Ok(()),
            Err(ureq::Error::Status(409, _)) => {
                // Repository already exists -- that's fine.
                Ok(())
            }
            Err(ureq::Error::Status(401 | 403, _)) => {
                anyhow::bail!(
                    "Upload requires a write-scoped token. \
                     Get one at: https://huggingface.co/settings/tokens"
                );
            }
            Err(e) => {
                anyhow::bail!("Failed to create repo '{repo_id}': {e}");
            }
        }
    }

    /// Upload an entire folder to a HuggingFace dataset repository.
    ///
    /// 1. Classifies files into small (inline) and large (LFS).
    /// 2. Uploads large files via LFS presigned URLs.
    /// 3. Commits all files (small inline + LFS pointers) via the commit API.
    pub fn upload_folder(&self, repo_id: &str, local_path: &Path) -> Result<()> {
        let (small_files, large_files) = Self::classify_files(local_path)?;
        let total = small_files.len() + large_files.len();
        let lfs_count = large_files.len();

        eprintln!("{total} files ({lfs_count} via LFS)");

        // Upload large files via LFS first.
        let lfs_pointers = self.upload_lfs_files(repo_id, &large_files)?;

        // Build and send the commit.
        self.create_commit(repo_id, &small_files, &lfs_pointers)?;

        eprintln!("Upload complete: {repo_id}");
        Ok(())
    }

    /// Walk a directory recursively and split files into small and large buckets.
    ///
    /// Hidden files/directories (names starting with `.`) are skipped.
    fn classify_files(base: &Path) -> Result<(Vec<FileEntry>, Vec<FileEntry>)> {
        let mut small = Vec::new();
        let mut large = Vec::new();

        Self::walk_dir(base, base, &mut small, &mut large)?;

        Ok((small, large))
    }

    /// Recursive directory walker for [`classify_files`].
    fn walk_dir(
        root: &Path,
        current: &Path,
        small: &mut Vec<FileEntry>,
        large: &mut Vec<FileEntry>,
    ) -> Result<()> {
        let entries = fs::read_dir(current)
            .with_context(|| format!("failed to read directory: {}", current.display()))?;

        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // Skip hidden files and directories.
            if name_str.starts_with('.') {
                continue;
            }

            let path = entry.path();
            let file_type = entry.file_type()?;

            if file_type.is_dir() {
                Self::walk_dir(root, &path, small, large)?;
            } else if file_type.is_file() {
                let meta = entry.metadata()?;
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");

                let fe = FileEntry {
                    relative_path: relative,
                    absolute_path: path,
                    size: meta.len(),
                };

                if fe.size >= LFS_THRESHOLD {
                    large.push(fe);
                } else {
                    small.push(fe);
                }
            }
        }

        Ok(())
    }

    /// Compute the SHA-256 digest of a file, reading in 8 KB chunks.
    fn compute_sha256(path: &Path) -> Result<String> {
        let mut file =
            fs::File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 8192];

        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }

        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Upload large files via LFS presigned URLs.
    ///
    /// For each file:
    /// 1. Request a presigned upload URL via the LFS batch API.
    /// 2. PUT the file bytes to the presigned URL.
    /// 3. Return an [`LfsPointer`] for use in the commit payload.
    fn upload_lfs_files(
        &self,
        repo_id: &str,
        files: &[FileEntry],
    ) -> Result<Vec<LfsPointer>> {
        let mut pointers = Vec::with_capacity(files.len());

        for (i, file) in files.iter().enumerate() {
            let size_mb = file.size as f64 / (1024.0 * 1024.0);
            eprintln!(
                "  Uploading (LFS) [{}/{}] {} ({:.1} MB)",
                i + 1,
                files.len(),
                file.relative_path,
                size_mb
            );

            let oid = Self::compute_sha256(&file.absolute_path)?;

            // Request presigned upload URL.
            let batch_url = format!(
                "{}/api/datasets/{}/lfs/objects/batch",
                self.endpoint, repo_id
            );

            let batch_body = serde_json::json!({
                "operation": "upload",
                "objects": [{
                    "oid": oid,
                    "size": file.size,
                }],
            });

            let batch_resp: serde_json::Value = ureq::post(&batch_url)
                .set("Authorization", &format!("Bearer {}", self.token))
                .send_json(&batch_body)
                .with_context(|| {
                    format!("LFS batch request failed for {}", file.relative_path)
                })?
                .into_json()
                .context("failed to parse LFS batch response")?;

            // Extract the upload URL from the response.
            let upload_href = batch_resp["objects"][0]["actions"]["upload"]["href"]
                .as_str()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "LFS batch response missing upload URL for {}. Response: {}",
                        file.relative_path,
                        batch_resp
                    )
                })?;

            // Upload the file bytes via PUT.
            let file_bytes = fs::read(&file.absolute_path)
                .with_context(|| format!("cannot read {}", file.absolute_path.display()))?;

            ureq::put(upload_href)
                .set("Content-Type", "application/octet-stream")
                .send_bytes(&file_bytes)
                .with_context(|| {
                    format!("LFS upload failed for {}", file.relative_path)
                })?;

            pointers.push(LfsPointer {
                relative_path: file.relative_path.clone(),
                oid,
                size: file.size,
            });
        }

        Ok(pointers)
    }

    /// Create a commit on HuggingFace Hub containing all files.
    ///
    /// Small files are included inline in the multipart payload.
    /// Large files are referenced as LFS pointers.
    fn create_commit(
        &self,
        repo_id: &str,
        small_files: &[FileEntry],
        lfs_pointers: &[LfsPointer],
    ) -> Result<()> {
        let commit_url = format!(
            "{}/api/datasets/{}/commit/main",
            self.endpoint, repo_id
        );

        // Build the operations list for the header.
        let mut operations: Vec<serde_json::Value> = Vec::new();

        for file in small_files {
            operations.push(serde_json::json!({
                "key": "file",
                "path": file.relative_path,
                "encoding": "raw",
            }));
        }

        for ptr in lfs_pointers {
            operations.push(serde_json::json!({
                "key": "lfsFile",
                "path": ptr.relative_path,
                "algo": "sha256",
                "oid": ptr.oid,
                "size": ptr.size,
            }));
        }

        let header_json = serde_json::json!({
            "summary": "Upload dataset via rosetta CLI",
            "files": operations,
        });

        // Build multipart body manually.
        // The HF commit API expects:
        //   Part 1: header JSON
        //   Part N: one part per regular file with its content
        let boundary = format!("rosetta-upload-{}", std::process::id());
        let mut body = Vec::new();

        // Header part.
        body.extend_from_slice(
            format!(
                "--{boundary}\r\n\
                 Content-Disposition: form-data; name=\"header\"\r\n\
                 Content-Type: application/json\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(header_json.to_string().as_bytes());
        body.extend_from_slice(b"\r\n");

        // File content parts for small files.
        for file in small_files {
            let content = fs::read(&file.absolute_path)
                .with_context(|| format!("cannot read {}", file.absolute_path.display()))?;

            body.extend_from_slice(
                format!(
                    "--{boundary}\r\n\
                     Content-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\n\
                     Content-Type: application/octet-stream\r\n\r\n",
                    file.relative_path
                )
                .as_bytes(),
            );
            body.extend_from_slice(&content);
            body.extend_from_slice(b"\r\n");
        }

        // Closing boundary.
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

        let content_type = format!("multipart/form-data; boundary={boundary}");

        let resp = ureq::post(&commit_url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Content-Type", &content_type)
            .send_bytes(&body);

        match resp {
            Ok(_) => Ok(()),
            Err(ureq::Error::Status(code, resp)) => {
                let body_text = resp.into_string().unwrap_or_default();
                anyhow::bail!(
                    "Commit failed (HTTP {code}) for '{repo_id}': {body_text}"
                );
            }
            Err(e) => {
                anyhow::bail!("Commit request failed for '{repo_id}': {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_classify_files_small_only() {
        let dir = tempfile::tempdir().unwrap();

        // Create a few small files.
        fs::write(dir.path().join("a.txt"), "hello").unwrap();
        fs::write(dir.path().join("b.json"), r#"{"k":"v"}"#).unwrap();

        let (small, large) = HubUploader::classify_files(dir.path()).unwrap();
        assert_eq!(small.len(), 2);
        assert!(large.is_empty());
    }

    #[test]
    fn test_classify_files_skips_hidden() {
        let dir = tempfile::tempdir().unwrap();

        fs::write(dir.path().join("visible.txt"), "yes").unwrap();
        fs::write(dir.path().join(".hidden"), "no").unwrap();

        let hidden_dir = dir.path().join(".git");
        fs::create_dir(&hidden_dir).unwrap();
        fs::write(hidden_dir.join("config"), "skip").unwrap();

        let (small, large) = HubUploader::classify_files(dir.path()).unwrap();
        assert_eq!(small.len(), 1);
        assert_eq!(small[0].relative_path, "visible.txt");
        assert!(large.is_empty());
    }

    #[test]
    fn test_classify_files_nested_directories() {
        let dir = tempfile::tempdir().unwrap();

        let sub = dir.path().join("data").join("train");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("0000.parquet"), "data").unwrap();
        fs::write(dir.path().join("config.json"), "{}").unwrap();

        let (small, _large) = HubUploader::classify_files(dir.path()).unwrap();
        assert_eq!(small.len(), 2);

        let paths: Vec<&str> = small.iter().map(|f| f.relative_path.as_str()).collect();
        assert!(paths.contains(&"data/train/0000.parquet"));
        assert!(paths.contains(&"config.json"));
    }

    #[test]
    fn test_classify_files_lfs_threshold() {
        let dir = tempfile::tempdir().unwrap();

        // Create a file exactly at the LFS threshold (should be classified as large).
        let big_path = dir.path().join("big.bin");
        let mut f = fs::File::create(&big_path).unwrap();
        let chunk = vec![0u8; 1024 * 1024]; // 1 MB
        for _ in 0..10 {
            f.write_all(&chunk).unwrap();
        }
        drop(f);

        // Create a file just below the threshold (should be classified as small).
        let small_path = dir.path().join("small.bin");
        let mut f = fs::File::create(&small_path).unwrap();
        for _ in 0..9 {
            f.write_all(&chunk).unwrap();
        }
        f.write_all(&vec![0u8; 1024 * 1024 - 1]).unwrap(); // 9 MB + (1 MB - 1 byte)
        drop(f);

        let (small, large) = HubUploader::classify_files(dir.path()).unwrap();
        assert_eq!(large.len(), 1, "file at exactly 10 MB should be LFS");
        assert_eq!(large[0].relative_path, "big.bin");
        assert_eq!(small.len(), 1, "file below 10 MB should be small");
        assert_eq!(small[0].relative_path, "small.bin");
    }

    #[test]
    fn test_compute_sha256_known() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        fs::write(&path, "hello world\n").unwrap();

        let hash = HubUploader::compute_sha256(&path).unwrap();

        // SHA-256 of "hello world\n".
        assert_eq!(
            hash,
            "a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447"
        );
    }

    #[test]
    fn test_compute_sha256_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        fs::write(&path, "").unwrap();

        let hash = HubUploader::compute_sha256(&path).unwrap();

        // SHA-256 of empty string.
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
