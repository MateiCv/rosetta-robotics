# Phase 1: Core Types and Project Scaffolding - Research

**Researched:** 2026-02-21
**Domain:** Cargo workspace scaffolding, Rust data model design, CI with FFI dependencies
**Confidence:** HIGH

## Summary

Phase 1 establishes the foundation everything else builds on: a 6-crate Cargo workspace, the canonical data models (`Episode`, `Frame`, `LazyArray`, `LazyImage`, `DatasetInfo`, `ForgeError`, `ConversionConfig`), a minimal `forge version` CLI, and a CI pipeline that validates FFmpeg and HDF5 linking on Linux and macOS. No actual format reading/writing, video processing, or quality analysis happens in this phase -- only the type contracts and build infrastructure.

The core types are well-understood from the existing Python implementation (`forge/core/models.py`, `forge/core/exceptions.py`, `forge/core/protocols.py`, `forge/config/models.py`). The Rust versions should preserve the same semantic structure but leverage Rust's ownership system for lazy loading (closures + `OnceCell` instead of Python's mutable `_cached` fields), `thiserror` for typed errors, and `serde` derive macros for config serialization. The workspace structure is standard Cargo -- no novel patterns needed.

The highest-risk item in this phase is CI validation of FFmpeg and HDF5 system library linking. FFmpeg requires `pkg-config` and system packages (`libavcodec-dev` etc. on Ubuntu, `ffmpeg` via Homebrew on macOS). HDF5 requires `libhdf5-dev` on Ubuntu and `hdf5` via Homebrew on macOS. Both must be proven to link correctly in CI before any code depends on them in Phase 2+.

**Primary recommendation:** Use Rust edition 2024 with MSRV 1.85.0, a virtual workspace with `crates/` directory, and `vergen-gitcl` for build info in the version command. Keep `forge-core` dependency-light (only `serde`, `thiserror`, `bytes`). Prove FFmpeg and HDF5 linking in CI with minimal "can it link?" test crates, not full implementations.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- User reviewed full algorithm descriptions for all format readers, writers, quality metrics, and video encoding
- Rust implementations should follow the same mathematical formulas as Python Forge for quality metrics (exact weights, thresholds, and normalization)
- H.264 encoding parameters must match Python: libx264, CRF 23, preset medium, yuv420p, GOP 12
- Core data models (Episode, Frame, LazyImage, LazyArray) should mirror Python's lazy loading pattern but leverage Rust ownership/lifetimes

### Claude's Discretion
- Binary naming and version output format
- Config format choice (YAML via serde_yaml_ng vs TOML)
- CI platform matrix and tooling (GitHub Actions vs other)
- Error type hierarchy (thiserror for library crates, anyhow at CLI boundary)
- Workspace crate organization details
- Rust edition and MSRV policy

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CORE-01 | System defines canonical Episode, Frame, LazyArray, LazyImage, and DatasetInfo data models shared across all formats | Python models.py fully analyzed -- Rust equivalents designed with ownership-aware lazy loading, serde derives, and trait-based abstractions. See Architecture Patterns section. |
| CLI-11 | `forge version` shows version, build info, and FFmpeg availability | `vergen-gitcl` crate provides git SHA, build timestamp, and cargo version at compile time. FFmpeg availability checked at runtime via `ffmpeg-next` version query or pkg-config. See Code Examples section. |
| HUB-04 | Single static binary with no runtime dependencies beyond system FFmpeg | Workspace structure with feature-gated FFI deps (HDF5, video). Binary links dynamically to system FFmpeg. No Python, no JVM, no runtime interpreter. See Standard Stack section. |
| HUB-06 | CI/CD pipeline with automated testing and binary release builds | GitHub Actions with matrix (Ubuntu 22.04/24.04, macOS 14). FFmpeg and HDF5 installed via apt/brew. Rust toolchain via `actions-rust-lang/setup-rust-toolchain`. See CI Pipeline section. |
</phase_requirements>

## Standard Stack

### Core (Phase 1 only)

| Library | Version | Purpose | Why Standard | Confidence |
|---------|---------|---------|--------------|------------|
| **Rust (stable)** | 1.85+ (edition 2024) | Language | Edition 2024 stable since Feb 2025 (Rust 1.85.0). Resolver v3 is the default. Async closures and other 2024 features available. | HIGH |
| **thiserror** | 2.0.x | Library error types | 609M+ total downloads. Derive macro for `std::error::Error`. Use in all library crates. Latest is 2.0.18. | HIGH |
| **anyhow** | 2.x | Application error propagation | Use only at the CLI boundary (`forge-cli`). Erases error types for convenient display. | HIGH |
| **serde** | 1.x | Serialization derives | Ubiquitous. `#[derive(Serialize, Deserialize)]` on all config and metadata types. | HIGH |
| **serde_json** | 1.x | JSON for metadata files | DatasetInfo, LeRobot metadata.json, quality reports all use JSON. | HIGH |
| **serde_yaml_ng** | 0.9.x | YAML config parsing | Maintained fork of deprecated `serde_yaml`. For ConversionConfig YAML files. | MEDIUM |
| **clap** | 4.5.x | CLI framework | Derive API with `#[command(version, propagate_version = true)]` for automatic `--version` on all subcommands. 290M+ downloads. | HIGH |
| **bytes** | 1.x | Reference-counted byte buffers | Used in `LazyImage` for zero-copy frame data. `Bytes::slice()` creates views without copying. | HIGH |
| **vergen-gitcl** | 9.x | Build info generation | Emits `VERGEN_GIT_SHA`, `VERGEN_BUILD_TIMESTAMP`, `VERGEN_CARGO_TARGET_TRIPLE` as env vars at compile time. Uses git CLI (no libgit2 dependency). MSRV: 1.88+ for v9 -- use v8.x if MSRV 1.85 is required. | MEDIUM |

### Build and CI

| Tool | Version | Purpose | Confidence |
|------|---------|---------|------------|
| **GitHub Actions** | - | CI/CD platform | Project is on GitHub. Matrix builds for Linux + macOS. | HIGH |
| **actions-rust-lang/setup-rust-toolchain** | v1 | Rust toolchain setup | Installs toolchain, configures caching, problem matchers. | HIGH |
| **AnimMouse/setup-ffmpeg** | v7 | FFmpeg installation in CI | Handles platform-specific FFmpeg installation. Alternatively, use `apt-get`/`brew` directly. | MEDIUM |
| **cargo-nextest** | latest | Test runner | Faster test execution, better output formatting, test retries. | HIGH |

### Phase 1 Dependencies NOT Needed Yet

These are confirmed in the stack research but should NOT appear in Phase 1 crate dependencies:

| Library | Phase | Why Not Phase 1 |
|---------|-------|-----------------|
| `arrow` / `parquet` | Phase 2 | No Parquet I/O in Phase 1. Types are defined abstractly. |
| `ffmpeg-next` | Phase 2 | Phase 1 only validates FFmpeg *links* in CI, not actual video code. |
| `hdf5-metno` | Phase 2+ | Phase 1 only validates HDF5 *links* in CI. |
| `rayon` | Phase 3 | No parallelism in Phase 1. |
| `ndarray` | Phase 4 | No numerical computation in Phase 1. |
| `tokio` / `reqwest` | Phase 7 | No HTTP in Phase 1. |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `vergen-gitcl` v9 | `built` crate or manual `build.rs` | `built` is simpler but less flexible. Manual `build.rs` with `env!("CARGO_PKG_VERSION")` + git command is zero-dep but more code. |
| `serde_yaml_ng` | `toml` | TOML is more Rust-idiomatic (Cargo.toml precedent). But Python Forge uses YAML configs, and user familiarity matters. Recommend: support both, default to YAML for migration ease. |
| Edition 2024 (Rust 1.85+) | Edition 2021 (Rust 1.56+) | Edition 2021 has broader toolchain support but misses resolver v3 default, async closures, and other 2024 features. Given this is a new project, no reason to use older edition. |

**Installation (Phase 1 workspace root):**
```toml
[workspace]
resolver = "3"
members = [
    "crates/forge-core",
    "crates/forge-video",
    "crates/forge-formats",
    "crates/forge-quality",
    "crates/forge-pipeline",
    "crates/forge-cli",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "MIT"
repository = "https://github.com/arpitg1304/forge"

[workspace.dependencies]
# Shared across crates -- pin versions here
thiserror = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
bytes = "1"
```

## Architecture Patterns

### Recommended Project Structure

```
forge-rust-rewrite/
├── Cargo.toml                          # Virtual workspace root
├── crates/
│   ├── forge-core/                     # Phase 1: Core types (zero heavy deps)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                  # Re-exports
│   │       ├── episode.rs              # Episode struct
│   │       ├── frame.rs                # Frame struct
│   │       ├── lazy.rs                 # LazyArray, LazyImage
│   │       ├── schema.rs              # DatasetInfo, FieldSchema, CameraInfo, Dtype
│   │       ├── config.rs               # ConversionConfig, FieldMapping
│   │       └── error.rs                # ForgeError enum (thiserror)
│   │
│   ├── forge-video/                    # Phase 2: stub Cargo.toml only in Phase 1
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs                  # Empty or re-export placeholder
│   │
│   ├── forge-formats/                  # Phase 2: stub
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   │
│   ├── forge-quality/                  # Phase 4: stub
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   │
│   ├── forge-pipeline/                 # Phase 3: stub
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   │
│   └── forge-cli/                      # Phase 1: minimal binary with `forge version`
│       ├── Cargo.toml
│       ├── build.rs                    # vergen build info generation
│       └── src/
│           └── main.rs                 # clap CLI with version subcommand
│
├── .github/
│   └── workflows/
│       └── ci.yml                      # GitHub Actions CI
│
├── tests/                              # Workspace-level integration tests (future)
│
├── forge/                              # Existing Python code (kept for reference)
│
└── .gitignore                          # Updated with Rust entries
```

### Pattern 1: Ownership-Based Lazy Loading (replacing Python's mutable cache)

**What:** Python uses `_cached: NDArray | None` with `object.__setattr__` to cache lazy-loaded data in frozen dataclasses. In Rust, use `OnceCell<T>` (or `OnceLock<T>` from std) with a closure stored as `Box<dyn FnOnce() -> T + Send>` to achieve the same semantics with Rust's ownership guarantees.

**When to use:** For `LazyArray` and `LazyImage` types where data is expensive to load and should be deferred until first access.

**Why not `Arc<Mutex<Option<T>>>`:** `OnceCell` is lock-free for reads after initialization, which is critical when multiple frames in an episode are accessed during quality analysis or writing.

**Example:**
```rust
use std::cell::OnceCell;
use bytes::Bytes;

/// Lazy-loaded image frame.
/// Mirrors Python's LazyImage but uses Rust's OnceCell for thread-safe lazy init.
pub struct LazyImage {
    loader: Option<Box<dyn FnOnce() -> Result<Bytes, ForgeError> + Send>>,
    cached: OnceCell<Bytes>,
    pub height: u32,
    pub width: u32,
    pub channels: u8,
}

impl LazyImage {
    pub fn new(
        loader: impl FnOnce() -> Result<Bytes, ForgeError> + Send + 'static,
        height: u32,
        width: u32,
        channels: u8,
    ) -> Self {
        Self {
            loader: Some(Box::new(loader)),
            cached: OnceCell::new(),
            height,
            width,
            channels,
        }
    }

    /// Create from already-loaded data (no lazy loading needed).
    pub fn from_bytes(data: Bytes, height: u32, width: u32, channels: u8) -> Self {
        let cell = OnceCell::new();
        let _ = cell.set(data);
        Self {
            loader: None,
            cached: cell,
            height,
            width,
            channels,
        }
    }

    pub fn shape(&self) -> (u32, u32, u8) {
        (self.height, self.width, self.channels)
    }

    pub fn is_loaded(&self) -> bool {
        self.cached.get().is_some()
    }
}
```

**Note on thread safety:** For the parallel pipeline (Phase 3+), `LazyImage` and `LazyArray` will need `Send + Sync`. `OnceCell` from `std::cell` is NOT `Sync` -- use `std::sync::OnceLock` instead for types that cross thread boundaries. In Phase 1, define with `OnceLock` from the start to avoid refactoring later.

### Pattern 2: Typed Error Hierarchy with thiserror

**What:** Map Python's exception hierarchy (`ForgeError` -> `UnsupportedFormatError`, `FormatDetectionError`, etc.) to a Rust enum with `#[derive(thiserror::Error)]`. Each variant carries structured data (not just a string message).

**When to use:** In `forge-core` for the `ForgeError` enum. Other crates may define their own error types that convert into `ForgeError` via `#[from]`.

**Example:**
```rust
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ForgeError {
    #[error("unsupported format: '{format_name}'")]
    UnsupportedFormat {
        format_name: String,
        available: Vec<String>,
    },

    #[error("format detection failed for path: {path}")]
    FormatDetection { path: PathBuf },

    #[error("episode '{episode_id}' not found in {path}")]
    EpisodeNotFound { episode_id: String, path: PathBuf },

    #[error("inspection failed for '{path}': {reason}")]
    Inspection { path: PathBuf, reason: String },

    #[error("validation failed: {errors:?}")]
    Validation {
        errors: Vec<String>,
        episode_id: Option<String>,
    },

    #[error("conversion failed ({source_format} -> {target_format}): {reason}")]
    Conversion {
        source_format: String,
        target_format: String,
        reason: String,
    },

    #[error("schema mismatch for field '{field_name}': expected {expected}, got {actual}")]
    Schema {
        field_name: String,
        expected: String,
        actual: String,
    },

    #[error("video error: {0}")]
    Video(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("pipeline closed unexpectedly")]
    PipelineClosed,

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Type alias for convenience.
pub type ForgeResult<T> = Result<T, ForgeError>;
```

### Pattern 3: Serde-Derived Config with YAML

**What:** Map Python's `ConversionConfig` dataclass to a Rust struct with `#[derive(Serialize, Deserialize)]`. Use `#[serde(default)]` for optional fields with defaults.

**When to use:** For `ConversionConfig` and `FieldMapping` in `forge-core/src/config.rs`.

**Example:**
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionConfig {
    pub source_format: Option<String>,
    pub target_format: Option<String>,

    #[serde(default)]
    pub cameras: HashMap<String, String>,

    #[serde(default)]
    pub fields: HashMap<String, FieldMapping>,

    pub action_field: Option<String>,
    pub state_field: Option<String>,

    pub fps: Option<f64>,
    pub robot_type: Option<String>,

    #[serde(default = "default_codec")]
    pub video_codec: String,
    #[serde(default = "default_crf")]
    pub video_crf: u8,

    #[serde(default)]
    pub fail_on_error: bool,
    #[serde(default = "default_true")]
    pub skip_existing: bool,

    #[serde(default = "default_true")]
    pub compress_videos: bool,
    #[serde(default = "default_true")]
    pub include_depth: bool,

    #[serde(default = "default_workers")]
    pub num_workers: usize,
}

fn default_codec() -> String { "h264".into() }
fn default_crf() -> u8 { 23 }
fn default_true() -> bool { true }
fn default_workers() -> usize { 1 }
```

### Pattern 4: Stub Crates for Workspace Compilation

**What:** Create all 6 workspace crates in Phase 1 as stubs so `cargo build --workspace` compiles. Stub crates have a `lib.rs` with a doc comment and maybe a re-export placeholder. This validates the workspace structure and dependency graph without requiring implementation.

**When to use:** Phase 1 for `forge-video`, `forge-formats`, `forge-quality`, `forge-pipeline`.

**Example (stub `forge-video/src/lib.rs`):**
```rust
//! Video encoding and decoding via FFmpeg.
//!
//! This crate wraps FFmpeg's libavcodec/libavformat for H.264 video
//! encoding and decoding. It is isolated in its own crate so that
//! FFmpeg build failures do not cascade to the rest of the workspace.
//!
//! Implementation arrives in Phase 2.
```

### Anti-Patterns to Avoid

- **Putting Arrow/Parquet types in forge-core:** The `Episode` and `Frame` types should NOT depend on `arrow` or `parquet` crates. Use generic types (`Vec<f32>`, `HashMap<String, Vec<u8>>`, `bytes::Bytes`) that can be converted to Arrow at the format boundary. This keeps `forge-core` lightweight and compilation fast.

- **Using `String` for everything:** Python uses `str` for format names, camera names, field names. In Rust, prefer newtypes or enums where the set is closed. For example, `Dtype` should be a Rust enum (as in Python), not a `String`. `FormatId` should be an enum with one variant per format.

- **Skipping `Send + Sync` bounds:** Lazy types (`LazyArray`, `LazyImage`) must be `Send` from the start. Using `std::cell::OnceCell` (not `Sync`) will require refactoring when rayon parallelism is added in Phase 3. Use `std::sync::OnceLock` from day one.

- **Over-engineering the CLI in Phase 1:** The CLI should be a single `forge version` command. Do not stub out all 10+ subcommands -- they arrive in later phases. Keep the CLI entry point minimal.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Error derive macros | Manual `impl Error for MyError` | `thiserror` 2.x | Boilerplate-free, handles `Display`, `From`, `source()` correctly |
| CLI argument parsing | Manual `std::env::args()` parsing | `clap` 4.5 derive API | Shell completions, help text, env vars, type-safe parsing for free |
| Build info (git SHA, timestamp) | Manual `build.rs` with `Command::new("git")` | `vergen-gitcl` or `built` | Handles edge cases (no git, CI environments, detached HEAD) |
| YAML parsing | Manual token parser | `serde_yaml_ng` | Correct YAML 1.2 parsing with serde integration |
| Workspace dependency management | Per-crate version pinning | `[workspace.dependencies]` | Single source of truth for shared dependency versions |

**Key insight:** Phase 1 is infrastructure. Every minute spent hand-rolling infrastructure is a minute not spent on the actual format conversion code that arrives in Phase 2.

## Common Pitfalls

### Pitfall 1: forge-core Becoming a Kitchen Sink
**What goes wrong:** Everything gets added to `forge-core` because "it's shared". Arrow types, format-specific helpers, video frame types all end up there, creating a compilation bottleneck and tight coupling.
**Why it happens:** Convenience -- it is easier to add to an existing crate than create a proper boundary.
**How to avoid:** Strict rule: `forge-core` contains ONLY data models, error types, and config types. No I/O, no format-specific logic, no heavy dependencies. Its `Cargo.toml` should have minimal deps: `serde`, `thiserror`, `bytes`, `serde_json`.
**Warning signs:** `forge-core/Cargo.toml` gains dependencies like `arrow`, `parquet`, `ffmpeg-next`, or `ndarray`.

### Pitfall 2: Wrong OnceCell for Lazy Types
**What goes wrong:** Using `std::cell::OnceCell` (not `Sync`) for `LazyArray`/`LazyImage`, then hitting compile errors when rayon tries to send episodes across threads in Phase 3.
**Why it happens:** `OnceCell` from `std::cell` is the first search result and works for single-threaded code.
**How to avoid:** Use `std::sync::OnceLock` (stabilized in Rust 1.70) from the start. The performance cost is negligible (one atomic check per access after initialization).
**Warning signs:** `LazyImage` does not implement `Send + Sync`.

### Pitfall 3: CI FFmpeg Version Mismatch
**What goes wrong:** CI installs FFmpeg 4.x (Ubuntu 22.04 default) but `ffmpeg-next` 8.0 expects FFmpeg 6.x+. Build fails with cryptic linker errors about missing symbols.
**Why it happens:** Ubuntu's apt packages lag behind upstream FFmpeg by 2-3 major versions.
**How to avoid:** Use a dedicated FFmpeg setup action (`AnimMouse/setup-ffmpeg`) or add a PPA (`ppa:savoury1/ffmpeg6`) for Ubuntu. Pin the minimum FFmpeg version in CI documentation. macOS Homebrew generally has current FFmpeg.
**Warning signs:** `undefined reference to avcodec_xxx` in CI build logs.

### Pitfall 4: .gitignore Not Updated for Rust
**What goes wrong:** `target/` directory (Rust build artifacts, can be 10+ GB) gets committed. Or worse, `Cargo.lock` is not committed for the binary crate.
**Why it happens:** The repo currently has a Python `.gitignore` with no Rust entries.
**How to avoid:** Add Rust-specific entries to `.gitignore` in Phase 1: `target/`, `*.pdb`. Commit `Cargo.lock` (this is a binary project, not a library).
**Warning signs:** `git status` shows hundreds of files in `target/`.

### Pitfall 5: Episode ID Type Mismatch
**What goes wrong:** Python uses `str` for `episode_id` (e.g., `"episode_0"`, `"000042"`). Some architecture research suggests `u64`. Using `u64` breaks string-based episode IDs from RLDS and HDF5 datasets.
**Why it happens:** Premature optimization -- `u64` is faster for indexing but loses information.
**How to avoid:** Use `String` for `episode_id` (matching Python). Add a `fn index(&self) -> Option<u64>` method that tries to parse numeric IDs when needed for indexing. This preserves compatibility with all format readers.
**Warning signs:** Format readers that need to construct non-numeric episode IDs.

### Pitfall 6: vergen v9 MSRV Conflict
**What goes wrong:** `vergen-gitcl` v9 requires MSRV 1.88, but the project targets Rust 1.85 (edition 2024 minimum).
**Why it happens:** vergen v9 was recently released with a higher MSRV than the edition 2024 minimum.
**How to avoid:** Use `vergen-gitcl` v8.x which has a lower MSRV, or bump project MSRV to 1.88+ (acceptable since the local toolchain is 1.90). Alternatively, use the `built` crate (simpler, lower MSRV) or a manual `build.rs` with `env!("CARGO_PKG_VERSION")` + git command.
**Warning signs:** `cargo build` fails with "`vergen-gitcl` requires Rust 1.88 or newer".

## Code Examples

### forge version Command Output

```rust
// crates/forge-cli/src/main.rs
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "forge",
    version,
    about = "High-performance robotics dataset format converter",
    propagate_version = true,
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show version, build info, and FFmpeg availability
    Version,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Version => print_version(),
    }
}

fn print_version() {
    println!("forge {}", env!("CARGO_PKG_VERSION"));

    // Build info from vergen (set in build.rs)
    if let Some(sha) = option_env!("VERGEN_GIT_SHA") {
        println!("git commit: {}", &sha[..8]);
    }
    if let Some(ts) = option_env!("VERGEN_BUILD_TIMESTAMP") {
        println!("built: {}", ts);
    }
    if let Some(target) = option_env!("VERGEN_CARGO_TARGET_TRIPLE") {
        println!("target: {}", target);
    }

    // FFmpeg availability (runtime check)
    // In Phase 1 this is a placeholder. Phase 2 adds actual FFmpeg probing.
    println!("ffmpeg: checking...");
    match std::process::Command::new("ffmpeg").arg("-version").output() {
        Ok(output) => {
            let version_line = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = version_line.lines().next() {
                println!("ffmpeg: {}", line);
            }
        }
        Err(_) => {
            println!("ffmpeg: not found (required for video operations)");
        }
    }
}
```

### build.rs for Version Info

```rust
// crates/forge-cli/build.rs
// Option A: Using vergen-gitcl (if MSRV allows)
use vergen_gitcl::{BuildBuilder, CargoBuilder, Emitter, GitclBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let build = BuildBuilder::default().build_timestamp(true).build()?;
    let cargo = CargoBuilder::default().target_triple(true).build()?;
    let gitcl = GitclBuilder::default().sha(true).build()?;

    Emitter::default()
        .add_instructions(&build)?
        .add_instructions(&cargo)?
        .add_instructions(&gitcl)?
        .emit()?;

    Ok(())
}

// Option B: Manual build.rs (zero dependencies, works with any MSRV)
// fn main() {
//     let output = std::process::Command::new("git")
//         .args(["rev-parse", "--short", "HEAD"])
//         .output();
//     if let Ok(output) = output {
//         let sha = String::from_utf8_lossy(&output.stdout);
//         println!("cargo:rustc-env=GIT_SHA={}", sha.trim());
//     }
//     println!("cargo:rustc-env=BUILD_DATE={}", chrono::Utc::now().format("%Y-%m-%d"));
// }
```

### Dtype Enum (Mapping from Python)

```rust
// crates/forge-core/src/schema.rs
use serde::{Deserialize, Serialize};

/// Data types supported by Forge.
/// Maps directly to Python Forge's Dtype enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dtype {
    Float32,
    Float64,
    Int32,
    Int64,
    Uint8,
    Bool,
    String,
}

/// Schema for a single field in the dataset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldSchema {
    pub name: String,
    pub shape: Vec<usize>,
    pub dtype: Dtype,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Metadata about a camera stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraInfo {
    pub name: String,
    pub height: u32,
    pub width: u32,
    #[serde(default = "default_channels")]
    pub channels: u8,
    #[serde(default = "default_encoding")]
    pub encoding: String,
}

fn default_channels() -> u8 { 3 }
fn default_encoding() -> String { "rgb".into() }
```

### CI Pipeline (GitHub Actions)

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [main, master]
  pull_request:

env:
  CARGO_TERM_COLOR: always

jobs:
  build:
    strategy:
      matrix:
        include:
          - os: ubuntu-22.04
            name: Linux
          - os: macos-14
            name: macOS (ARM)
    runs-on: ${{ matrix.os }}
    name: Build (${{ matrix.name }})

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: stable
          components: rustfmt, clippy

      - name: Install FFmpeg (Ubuntu)
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            libavcodec-dev libavformat-dev libavutil-dev \
            libavfilter-dev libavdevice-dev libswscale-dev \
            libswresample-dev pkg-config

      - name: Install FFmpeg (macOS)
        if: runner.os == 'macOS'
        run: brew install ffmpeg pkg-config

      - name: Install HDF5 (Ubuntu)
        if: runner.os == 'Linux'
        run: sudo apt-get install -y libhdf5-dev

      - name: Install HDF5 (macOS)
        if: runner.os == 'macOS'
        run: brew install hdf5

      - name: Check formatting
        run: cargo fmt --all -- --check

      - name: Clippy lints
        run: cargo clippy --workspace --all-targets -- -D warnings

      - name: Build workspace
        run: cargo build --workspace

      - name: Run tests
        run: cargo test --workspace

      - name: Verify forge version
        run: cargo run -p forge-cli -- version
```

## CI Pipeline

### Platform Matrix

| Runner | OS | FFmpeg Install | HDF5 Install | Notes |
|--------|----|----------------|--------------|-------|
| `ubuntu-22.04` | Ubuntu 22.04 LTS | `apt-get install libavcodec-dev libavformat-dev libavutil-dev libswscale-dev pkg-config` | `apt-get install libhdf5-dev` | Default Ubuntu runner. FFmpeg 4.4 via apt (may need PPA for 6.x+). |
| `macos-14` | macOS 14 Sonoma (ARM) | `brew install ffmpeg pkg-config` | `brew install hdf5` | Apple Silicon runner. Homebrew has current FFmpeg. |

### FFmpeg Version Concern

Ubuntu 22.04's default `libavcodec-dev` provides FFmpeg 4.4.x. The `ffmpeg-next` 8.0 crate supports FFmpeg 4.x through 8.x, so this should work. However, if the crate requires FFmpeg 6.x+, options are:
1. Use the `AnimMouse/setup-ffmpeg` action to install a newer version
2. Add PPA: `sudo add-apt-repository ppa:savoury1/ffmpeg6`
3. Use Ubuntu 24.04 which ships FFmpeg 6.1

**Recommendation:** Start with Ubuntu 22.04 default packages. If linking fails, upgrade to Ubuntu 24.04 or add the PPA.

### HDF5 Linking Validation

In Phase 1, HDF5 is not used by any crate code. The CI step installs the system library to validate that it is available for Phase 2+ when `forge-formats` will depend on `hdf5-metno`. The validation is: system packages install successfully and `pkg-config --libs hdf5` returns a valid linker path.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `resolver = "2"` (Rust 2021) | `resolver = "3"` (Rust 2024) | Feb 2025 (Rust 1.85) | Resolver v3 is MSRV-aware by default. New workspace projects should use edition 2024. |
| `serde_yaml` (dtolnay) | `serde_yaml_ng` | 2024 | Original crate deprecated. `serde_yml` also deprecated due to unsoundness (RUSTSEC-2025-0068). |
| `hdf5` (aldanor) | `hdf5-metno` | 2023 | Original maintainer inactive. metno fork is actively maintained. |
| `vergen` v8 (single crate) | `vergen-gitcl` v9 (split crates) | 2025 | v9 splits into `vergen`, `vergen-git2`, `vergen-gitcl`, `vergen-gix`. MSRV increased to 1.88. |
| Manual `.gitignore` | `cargo init` template | Always | Cargo generates standard Rust `.gitignore` entries. |

**Deprecated/outdated:**
- `serde_yaml` (dtolnay): Archived, no longer maintained. Use `serde_yaml_ng`.
- `serde_yml`: Archived due to unsoundness (RUSTSEC-2025-0068). Use `serde_yaml_ng`.
- `hdf5` (aldanor): Abandoned. Use `hdf5-metno`.
- `actions-rs/*` GitHub Actions: Largely unmaintained. Use `actions-rust-lang/setup-rust-toolchain` instead.

## Open Questions

1. **vergen version vs MSRV tradeoff**
   - What we know: `vergen-gitcl` v9 requires MSRV 1.88, project MSRV is 1.85 (edition 2024). Local toolchain is 1.90 so it compiles locally. CI installs `stable` which is currently 1.90+.
   - What's unclear: Whether to pin MSRV at 1.85 (edition minimum) or bump to 1.88+ (to use latest vergen).
   - Recommendation: Use MSRV 1.85 in `Cargo.toml` for maximum compatibility, but use `vergen-gitcl` v8.x (or the manual `build.rs` approach) to stay within that MSRV. Alternatively, set `rust-version = "1.88"` since there is no practical reason someone would build this project on Rust 1.85-1.87.

2. **FFmpeg version in Ubuntu 22.04 CI**
   - What we know: Ubuntu 22.04 ships FFmpeg 4.4. `ffmpeg-next` 8.0 claims support for FFmpeg 4.x through 8.x.
   - What's unclear: Whether all required symbols from `ffmpeg-next` are available in FFmpeg 4.4 specifically.
   - Recommendation: Try Ubuntu 22.04 default packages first. If linking fails, move to Ubuntu 24.04 (FFmpeg 6.1) or use `AnimMouse/setup-ffmpeg`.

3. **Config format: YAML vs TOML vs both**
   - What we know: Python Forge uses YAML configs. TOML is the Rust ecosystem convention. `serde_yaml_ng` is MEDIUM confidence (maintained fork of deprecated crate).
   - What's unclear: Whether to support both formats or pick one.
   - Recommendation: Default to YAML for user migration ease. Implement TOML support as a stretch goal. The `ConversionConfig` struct is format-agnostic via serde -- supporting both is trivial once serde derives are in place.

## Sources

### Primary (HIGH confidence)
- Python Forge source code: `forge/core/models.py`, `forge/core/exceptions.py`, `forge/core/protocols.py`, `forge/config/models.py` -- canonical data model definitions
- [Announcing Rust 1.85.0 and Rust 2024](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/) -- Edition 2024 stable, resolver v3 default
- [Cargo Workspaces](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html) -- Official Rust book workspace documentation
- [thiserror on crates.io](https://crates.io/crates/thiserror) -- v2.0.18, 609M downloads
- [clap derive tutorial](https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html) -- Subcommands, version propagation

### Secondary (MEDIUM confidence)
- [vergen-gitcl on GitHub](https://github.com/rustyhorde/vergen) -- v9.x build info, MSRV 1.88
- [AnimMouse/setup-ffmpeg](https://github.com/AnimMouse/setup-ffmpeg) -- GitHub Actions FFmpeg setup
- [actions-rust-lang/setup-rust-toolchain](https://github.com/actions-rust-lang/setup-rust-toolchain) -- Modern Rust CI setup
- [hdf5-metno on GitHub](https://github.com/metno/hdf5-rust) -- HDF5 Rust bindings, apt/brew installation
- [ffmpeg-next on crates.io](https://crates.io/crates/ffmpeg-next) -- v8.0, FFmpeg 4.x-8.x support claimed
- [serde_yaml_ng on crates.io](https://crates.io/crates/serde_yaml_ng) -- v0.9.x, maintained fork

### Tertiary (LOW confidence)
- [RUSTSEC-2025-0068](https://rustsec.org/advisories/RUSTSEC-2025-0068.html) -- serde_yml unsoundness advisory (verified, but advisory content not directly fetched)
- Ubuntu 22.04 FFmpeg 4.4 version -- based on general knowledge of Ubuntu LTS package versions, not directly verified against CI runner image manifest

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- All Phase 1 dependencies (`thiserror`, `serde`, `clap`, `bytes`) are battle-tested with hundreds of millions of downloads
- Architecture: HIGH -- Cargo workspace structure follows official Rust documentation. Data model mapping from Python is straightforward.
- CI pipeline: MEDIUM-HIGH -- GitHub Actions Rust CI is well-documented. FFmpeg/HDF5 apt/brew installation is standard. Only uncertainty is FFmpeg version compatibility on Ubuntu 22.04.
- Pitfalls: HIGH -- Identified from direct analysis of Python source code (episode ID types), Rust ecosystem knowledge (OnceCell vs OnceLock), and CI experience (FFmpeg version skew)

**Research date:** 2026-02-21
**Valid until:** 2026-04-21 (60 days -- Phase 1 stack is stable, no fast-moving dependencies)
