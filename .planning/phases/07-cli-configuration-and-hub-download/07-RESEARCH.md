# Phase 7: CLI, Configuration, and Hub Download - Research

**Researched:** 2026-02-22
**Domain:** Rust CLI architecture (clap 4.5), YAML configuration, HuggingFace Hub integration
**Confidence:** HIGH

## Summary

Phase 7 builds the complete user-facing CLI on top of the mature library layer (Phases 1-6). The codebase already has a working `forge convert`, `forge version`, and `forge formats` in `crates/forge-cli/src/main.rs` using clap 4.5 derive API, plus comprehensive library APIs in `forge-formats` (registry, detection, readers/writers), `forge-quality` (QualityAnalyzer, FilterEngine), and `forge-pipeline` (Converter). The work is primarily wiring: creating new clap subcommands that call existing library functions and formatting output for humans and JSON.

The two genuinely new capabilities are: (1) HuggingFace Hub dataset downloading via the `hf-hub` Rust crate (file-by-file download with `RepoInfo.siblings` enumeration since no `snapshot_download` exists), and (2) YAML config generation from dataset inspection. Everything else (inspect, quality, filter, stats, export-video, visualize) has working library code that needs CLI glue.

**Primary recommendation:** Structure the CLI as a modular set of command handler files (one per subcommand) rather than expanding the current monolithic `main.rs`. Use `hf-hub` 0.5 with its sync API for Hub downloads, implementing snapshot_download manually via `info().siblings` + per-file `download()`. Generate shell completions via `clap_complete` in `build.rs`.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CLI-01 | `forge inspect <path>` displays dataset schema, cameras, fields, episode count, and stats in human-readable and JSON formats | Wire `FormatReader::inspect()` (returns `DatasetInfo`) to a new inspect subcommand. DatasetInfo already serializes to JSON via serde. Human-readable formatting follows Python CLI pattern. |
| CLI-02 | `forge convert <source> <output>` converts between any supported format pair | Already implemented in existing `main.rs`. Needs enhancement: YAML config file loading via `--config`, format override via `--source-format`, and `hf://` URL resolution. |
| CLI-03 | `forge quality <path>` runs quality analysis and displays per-episode scores | Wire `QualityAnalyzer::analyze_dataset()` to CLI. Library returns `QualityReport` with JSON export. Human output needs table formatting. |
| CLI-04 | `forge filter <path> <output>` filters episodes by quality criteria | Wire `FilterEngine::filter()` to CLI. Needs reader/writer setup from registry, dry-run mode (output=None), and threshold CLI args. |
| CLI-05 | `forge hub download <repo>` downloads datasets from HuggingFace Hub | New capability: use `hf-hub` crate. Parse `hf://` URLs, enumerate files via `RepoInfo.siblings`, download each file, cache locally. |
| CLI-06 | `forge hub upload <path> <repo>` uploads datasets to HuggingFace Hub | Phase 8 requirement per ROADMAP (HUB-03 maps to Phase 8). However listed in Phase 7 success criteria as a subcommand. Research covers stub/placeholder approach. |
| CLI-08 | `forge stats <path>` shows dataset statistics with distribution analysis | Read all episodes, compute per-field min/max/mean/std, episode length distribution. Pure Rust numeric computation. |
| CLI-09 | `forge export-video <path>` exports episodes as standalone video files | Wire `FormatReader::read_episodes()` + `forge-video` encoder to produce standalone MP4s from episode camera data. |
| CLI-10 | `forge visualize <path>` opens interactive dataset viewer | Terminal-based visualization is Phase 8 (VIZ-01). CLI-10 requirement text says "opens interactive dataset viewer". This is a stub/placeholder in Phase 7 that prints info + defers to Phase 8 for the full interactive TUI. |
| CLI-12 | YAML-based conversion config supports field mapping, camera remapping, FPS override, and quality thresholds | `ConversionConfig` already supports YAML deserialization via serde. Needs: `--config` flag on convert, CLI args override config file values. Field remapping stub needs actual implementation. |
| CLI-13 | `forge inspect --generate-config` auto-generates YAML config from dataset inspection | Inspect dataset with `FormatReader::inspect()`, extract schema/cameras/fields, emit YAML template with comments. Python CLI has a working reference implementation. |
| CLI-15 | Shell completions generated for bash, zsh, and fish | Use `clap_complete` crate in `build.rs` to generate completion scripts at compile time. |
| CLI-16 | Format auto-detection works for all 8 input formats without user-specified format hint | Already implemented in `forge-formats::detection::detect_format_scored()`. CLI must use `detect_format()` before calling readers, which `Converter::convert()` already does. Ensure all new subcommands follow this pattern. |
| HUB-01 | Download datasets from HuggingFace Hub via `hf://` URL scheme with authentication for gated repos | `hf-hub` crate supports `Api::dataset()` for dataset repos, token-based auth via `ApiBuilder::with_token()` or `HF_TOKEN` env var. |
| HUB-02 | Cache downloaded datasets locally to avoid re-downloads | `hf-hub` maintains cache compatible with Python `huggingface_hub`. Built-in skip-if-cached behavior via `ApiRepo::get()`. Cache dir configurable. |
</phase_requirements>

## Standard Stack

### Core (Already in Workspace)

| Library | Version | Purpose | Already Used |
|---------|---------|---------|--------------|
| clap | 4.5 (derive) | CLI argument parsing, subcommand routing | Yes - workspace dep |
| serde | 1.x | Serialization for config and output | Yes - workspace dep |
| serde_yaml_ng | 0.9 | YAML config file loading | Yes - workspace dep |
| serde_json | 1.x | JSON output formatting | Yes - workspace dep |
| anyhow | 1.x | CLI error handling | Yes - forge-cli dep |
| indicatif | 0.18 | Progress bars (existing) | Yes - workspace dep |

### New Dependencies

| Library | Version | Purpose | Why Standard | Confidence |
|---------|---------|---------|--------------|------------|
| clap_complete | 4.5 | Shell completion generation (bash, zsh, fish) | Official clap companion crate, same version line | HIGH |
| hf-hub | 0.5 | HuggingFace Hub file download, caching, authentication | Official HuggingFace Rust client. Cache-compatible with Python huggingface_hub. Sync API via `ureq` (no tokio needed). | HIGH |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| hf-hub (sync/ureq) | hf-hub (tokio) | Async could be faster for parallel downloads, but adds tokio runtime to the binary. Sync is simpler, sufficient for sequential file downloads. Use sync unless download speed is a measured bottleneck. |
| hf-hub | reqwest + manual HF API | Full control but reinvents caching, etag checking, auth token handling. Not worth it. |
| clap_complete build.rs | Runtime `--completions` flag | Build.rs approach generates files at compile time for packaging. Runtime approach is simpler for users (`forge completions bash`). **Recommend runtime approach** as a subcommand since it requires no build-time file management. |
| Plain println! for tables | comfy-table or tabled | Adding a table formatting crate adds polish but also a dependency. For Phase 7, formatted println! with fixed-width columns (as existing `print_formats` does) is sufficient. Consider adding later. |

### Installation

```toml
# In workspace Cargo.toml [workspace.dependencies]:
clap_complete = "4.5"
hf-hub = { version = "0.5", default-features = false, features = ["ureq"] }

# In crates/forge-cli/Cargo.toml [dependencies]:
clap_complete.workspace = true
hf-hub.workspace = true
forge-quality.workspace = true
```

Note: `hf-hub` default features include `tokio` + `native-tls`. Use `default-features = false, features = ["ureq"]` to get sync-only API without pulling in the tokio runtime, keeping the binary lean.

## Architecture Patterns

### Recommended CLI Module Structure

```
crates/forge-cli/
├── build.rs                  # Existing: git SHA, timestamp. Add: clap_complete generation
├── Cargo.toml
└── src/
    ├── main.rs               # Cli struct, Commands enum, main() dispatch
    ├── commands/
    │   ├── mod.rs             # pub mod for each command
    │   ├── convert.rs         # Enhanced convert (YAML config, hf:// resolution)
    │   ├── inspect.rs         # CLI-01, CLI-13 (--generate-config)
    │   ├── quality.rs         # CLI-03 (quality analysis)
    │   ├── filter.rs          # CLI-04 (episode filtering)
    │   ├── stats.rs           # CLI-08 (dataset statistics)
    │   ├── export_video.rs    # CLI-09 (video export)
    │   ├── visualize.rs       # CLI-10 (placeholder/basic info)
    │   ├── hub.rs             # CLI-05, CLI-06, HUB-01, HUB-02 (download/upload)
    │   ├── completions.rs     # CLI-15 (shell completions)
    │   ├── version.rs         # Existing version command (moved here)
    │   └── formats.rs         # Existing formats command (moved here)
    ├── config.rs              # CLI-12: YAML config loading + CLI arg merge logic
    ├── hub_client.rs          # HuggingFace Hub download/cache logic
    └── output.rs              # Shared output formatting (human vs JSON)
```

### Pattern 1: Command Handler Pattern

**What:** Each subcommand is a function in its own module that takes parsed clap args and returns `anyhow::Result<()>`.
**When to use:** Every subcommand.
**Example:**

```rust
// src/commands/inspect.rs
use std::path::Path;
use anyhow::Result;
use forge_formats::{detect_format, get_reader};
use crate::output::OutputFormat;

pub fn run(path: &str, output_format: OutputFormat, generate_config: Option<&Path>) -> Result<()> {
    let resolved = crate::hub_client::resolve_path(path)?;
    let format_name = detect_format(&resolved)?;
    let reader = get_reader(format_name)?;
    let info = reader.inspect(&resolved)?;

    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&info)?);
        }
        OutputFormat::Text => {
            print_human_readable(&info);
        }
    }

    if let Some(config_path) = generate_config {
        generate_yaml_config(&info, config_path)?;
    }

    Ok(())
}
```

### Pattern 2: hf:// URL Resolution

**What:** All path-accepting commands transparently resolve `hf://` URLs to local cached paths before proceeding.
**When to use:** Every command that takes a dataset path.
**Example:**

```rust
// src/hub_client.rs
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use hf_hub::api::sync::Api;

pub fn resolve_path(path: &str) -> Result<PathBuf> {
    if is_hf_url(path) {
        download_dataset(path)
    } else {
        Ok(PathBuf::from(path))
    }
}

pub fn is_hf_url(path: &str) -> bool {
    path.starts_with("hf://") || path.starts_with("huggingface://")
        || path.contains("huggingface.co/datasets/")
}

pub fn download_dataset(url: &str) -> Result<PathBuf> {
    let ref_ = parse_hf_url(url)?;
    let api = Api::new().context("failed to initialize HuggingFace Hub API")?;
    let repo = api.dataset(ref_.repo_id);

    // Get file list from repo info
    let info = repo.info().context("failed to fetch repo info")?;
    let siblings = info.siblings;

    // Download each file (hf-hub handles caching automatically)
    for sibling in &siblings {
        repo.get(&sibling.rfilename)
            .with_context(|| format!("failed to download {}", sibling.rfilename))?;
    }

    // Return the cache directory path
    // hf-hub stores files in ~/.cache/huggingface/hub/datasets--{org}--{repo}/snapshots/{sha}/
    // The first downloaded file's parent gives us the snapshot dir
    let first_file = repo.get(&siblings[0].rfilename)?;
    Ok(first_file.parent().unwrap().to_path_buf())
}
```

### Pattern 3: CLI Arg Override of YAML Config

**What:** YAML config provides defaults; explicit CLI flags override them. Unset CLI flags do not clobber config file values.
**When to use:** `forge convert --config config.yaml --fps 60` (fps=60 overrides config file's fps).
**Example:**

```rust
// src/config.rs
use forge_core::ConversionConfig;
use std::path::Path;
use anyhow::Result;

pub fn load_config(
    config_path: Option<&Path>,
    cli_target_format: Option<&str>,
    cli_fps: Option<f64>,
    cli_cameras: &[(String, String)],
    cli_fail_on_error: bool,
    // ... other CLI overrides
) -> Result<ConversionConfig> {
    let mut config = if let Some(path) = config_path {
        ConversionConfig::from_yaml(path)?
    } else {
        ConversionConfig::default()
    };

    // CLI args override config (only when explicitly set)
    if let Some(fmt) = cli_target_format {
        config.target_format = Some(fmt.to_string());
    }
    if let Some(fps) = cli_fps {
        config.fps = Some(fps);
    }
    if cli_fail_on_error {
        config.fail_on_error = true;
    }
    for (src, dst) in cli_cameras {
        config.cameras.insert(src.clone(), dst.clone());
    }

    // Default target format if not set anywhere
    if config.target_format.is_none() {
        config.target_format = Some("lerobot-v3".to_string());
    }

    Ok(config)
}
```

### Pattern 4: Output Format Toggle

**What:** All data-producing commands support `--json` flag for machine-readable output and default to human-readable text.
**When to use:** inspect, quality, filter (dry-run), stats, formats.
**Example:**

```rust
// src/output.rs
pub enum OutputFormat {
    Text,
    Json,
}

impl OutputFormat {
    pub fn from_json_flag(json: bool) -> Self {
        if json { Self::Json } else { Self::Text }
    }
}
```

### Anti-Patterns to Avoid

- **Monolithic main.rs:** Do not add all subcommand logic to `main.rs`. It is already 197 lines. Extract to modules.
- **Duplicating detection/resolution logic:** Every command that takes a path should go through the same `resolve_path()` + `detect_format()` pipeline. Do not repeat this in each handler.
- **Blocking on tokio for Hub downloads:** The CLI is sync. Using `hf-hub` with `ureq` feature keeps it sync. Do not pull in tokio runtime.
- **Hardcoding format strings:** Use `FormatCapability` and registry functions, not hardcoded format name strings.
- **Ignoring existing field remapping stub:** `apply_field_remapping_static()` in `convert.rs` is a documented no-op (tech debt item). Phase 7 CLI-12 requires field mapping to work. This stub must be implemented.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| HuggingFace Hub download + caching | Custom HTTP client + cache dir management | `hf-hub` crate | Handles etag-based caching, symlinks, auth tokens, cache compatibility with Python huggingface_hub |
| Shell completion scripts | Manual bash/zsh/fish completion files | `clap_complete` | Generates from the clap Command definition automatically, stays in sync with CLI changes |
| YAML parsing | Custom config file parser | `serde_yaml_ng` (already in workspace) | Handles all YAML edge cases, already used for ConversionConfig and QualityConfig |
| HF URL parsing | Regex-heavy URL parser | Simple prefix-based parser (< 50 lines) | The URL format is simple (`hf://org/repo`, `hf://org/repo@rev`). The Python implementation is < 100 lines. Don't overcomplicate. |
| Progress bars | Custom terminal progress | `indicatif` (already in workspace) | Already used in forge-pipeline, battle-tested |

**Key insight:** Phase 7 is a wiring phase. The library layer (Phases 1-6) has all the heavy lifting. The CLI's job is argument parsing, path resolution, library calls, and output formatting. Resist the temptation to add complex logic to command handlers.

## Common Pitfalls

### Pitfall 1: hf-hub Has No snapshot_download

**What goes wrong:** Developer assumes `hf-hub` has a Python-equivalent `snapshot_download()` that downloads an entire repo in one call. It does not exist. Only per-file `get()`/`download()` methods are available.
**Why it happens:** Python's `huggingface_hub.snapshot_download()` is the standard way to download datasets. Developers expect the Rust crate to have the same API.
**How to avoid:** Implement snapshot download manually: call `repo.info()` to get `RepoInfo.siblings` (file list), then iterate and call `repo.get(filename)` for each file. The `get()` method handles caching automatically.
**Warning signs:** Looking for a `snapshot` or `download_all` method in hf-hub docs.

### Pitfall 2: hf-hub Cache Directory Structure

**What goes wrong:** After downloading files individually via `repo.get()`, the code cannot reconstruct the dataset directory structure because hf-hub stores files in a flat symlink-based cache.
**Why it happens:** `hf-hub` uses the same cache structure as Python: `~/.cache/huggingface/hub/datasets--{org}--{repo}/snapshots/{sha}/path/to/file`. Files are stored by blob hash and symlinked into the snapshot directory. The `get()` method returns the path to the symlink target, not the snapshot path.
**How to avoid:** After downloading, construct the expected directory path from the cache structure, or use the path of the first downloaded file to derive the snapshot root. Alternatively, set a custom `cache_dir` via `ApiBuilder::with_cache_dir()` and organize files in a Forge-specific cache layout.
**Warning signs:** Downloaded file paths don't form a valid dataset directory.

### Pitfall 3: CLI Arg vs Config File Precedence

**What goes wrong:** Config file values overwrite explicit CLI arguments, or CLI defaults (like `fail_on_error: false`) overwrite config file values.
**Why it happens:** clap's default values are always present. You cannot distinguish "user passed --fps 30" from "fps defaulted to None" unless you use `Option<T>` for all config-overrideable fields.
**How to avoid:** Use `Option<T>` for all CLI args that can be set in config files. Only override config values when the CLI arg is `Some(value)`. The existing `ConversionConfig` already uses `Option` for `fps`, `source_format`, `target_format`, etc.
**Warning signs:** Test a YAML config with `fps: 60` and verify `forge convert --config config.yaml` uses 60, not a hardcoded default.

### Pitfall 4: Field Remapping is a No-Op

**What goes wrong:** CLI-12 requires field mapping to work in conversion, but `apply_field_remapping_static()` in `forge-pipeline/src/convert.rs` is a documented no-op stub.
**Why it happens:** Field remapping was deferred from Phase 2 as tech debt (item #1 in v1.0 audit). Camera remapping works; field remapping does not.
**How to avoid:** Implement actual field remapping logic as part of CLI-12 work. The `ConversionConfig.fields` HashMap already has the right shape (`HashMap<String, FieldMapping>` with source/target/transform). The implementation needs to rename frame fields and apply basic transforms.
**Warning signs:** Config file specifies `fields:` section but converted output has original field names.

### Pitfall 5: Quality/Filter Commands Need Reader Setup

**What goes wrong:** The `QualityAnalyzer::analyze_dataset()` and `FilterEngine::filter()` require a `&dyn FormatReader` and `&mut dyn FormatWriter`. Developers forget to detect format and get reader/writer from registry.
**Why it happens:** The library API is flexible (takes trait objects). The CLI must wire the concrete reader/writer.
**How to avoid:** Follow the pattern: detect_format -> get_reader -> call library. For filter with output, also get_writer for the same format (filter preserves format).
**Warning signs:** "unsupported format" errors when running quality/filter commands.

### Pitfall 6: Shell Completions via build.rs vs Runtime

**What goes wrong:** Using `clap_complete::generate_to` in `build.rs` generates completion files at compile time, but they end up in `OUT_DIR` (the target directory), not in a user-accessible location.
**Why it happens:** build.rs output goes to the build directory, not the source tree.
**How to avoid:** Use the runtime approach: add a `forge completions <shell>` subcommand that prints the completion script to stdout. Users pipe it to the appropriate location (`forge completions bash > ~/.local/share/bash-completion/completions/forge`). This is the standard clap approach.
**Warning signs:** Completion files generated but not installable.

### Pitfall 7: HuggingFace Authentication for Gated Repos

**What goes wrong:** `forge hub download` fails on gated/private datasets with an unhelpful error.
**Why it happens:** Gated repos require authentication. The `hf-hub` crate looks for `HF_TOKEN` env var or `~/.cache/huggingface/token` file. If neither exists, the request fails.
**How to avoid:** Catch the auth error specifically (hf-hub returns distinct error for 401/403 responses) and print a helpful message: "This dataset requires authentication. Run `huggingface-cli login` or set HF_TOKEN environment variable."
**Warning signs:** Generic HTTP error without guidance on authentication.

## Code Examples

### Shell Completions via Runtime Subcommand

```rust
// Source: clap_complete docs
use clap::{Command, value_parser};
use clap_complete::aot::{generate, Shell};
use std::io;

// In Commands enum:
/// Generate shell completions
Completions {
    /// Shell to generate completions for
    #[arg(value_parser = value_parser!(Shell))]
    shell: Shell,
}

// In handler:
fn print_completions(shell: Shell, cmd: &mut Command) {
    generate(shell, cmd, cmd.get_name().to_string(), &mut io::stdout());
}
```

### HuggingFace Hub Dataset Download

```rust
// Source: hf-hub 0.5 API
use hf_hub::api::sync::Api;
use hf_hub::Repo;
use hf_hub::RepoType;

fn download_hf_dataset(repo_id: &str, token: Option<&str>) -> anyhow::Result<std::path::PathBuf> {
    let mut builder = hf_hub::api::sync::ApiBuilder::new();
    if let Some(t) = token {
        builder = builder.with_token(Some(t.to_string()));
    }
    let api = builder.build()?;
    let repo = api.dataset(repo_id.to_string());

    // Enumerate files
    let info = repo.info()?;
    let files: Vec<String> = info.siblings
        .iter()
        .map(|s| s.rfilename.clone())
        .collect();

    eprintln!("Downloading {} files from {}", files.len(), repo_id);

    // Download each file (get() skips if cached)
    let mut root = None;
    for filename in &files {
        let local_path = repo.get(filename)?;
        if root.is_none() {
            // Derive snapshot root from first file
            root = Some(derive_snapshot_root(&local_path, filename));
        }
    }

    root.ok_or_else(|| anyhow::anyhow!("no files found in repo {}", repo_id))
}
```

### YAML Config Generation from Inspection

```rust
// Source: Forge Python CLI reference implementation
fn generate_yaml_config(info: &DatasetInfo, output: &Path) -> anyhow::Result<()> {
    let mut lines = vec![
        format!("# Forge conversion configuration"),
        format!("# Generated from: {}", info.path.display()),
        format!("# Source format: {}", info.format),
        String::new(),
        "target_format: lerobot-v3".to_string(),
        String::new(),
    ];

    // FPS
    match info.fps {
        Some(fps) => lines.push(format!("fps: {fps}")),
        None => lines.push("# fps: 30  # Specify FPS (required if not in source)".to_string()),
    }

    // Robot type
    match &info.robot_type {
        Some(rt) => lines.push(format!("robot_type: {rt}")),
        None => lines.push("# robot_type: unknown  # Specify robot type".to_string()),
    }
    lines.push(String::new());

    // Camera mappings
    if !info.cameras.is_empty() {
        lines.push("cameras:".to_string());
        for cam in &info.cameras {
            lines.push(format!("  {}: {}", cam.name, cam.name));
        }
        lines.push(String::new());
    }

    // Field mappings
    if !info.fields.is_empty() {
        lines.push("# fields:".to_string());
        for field in &info.fields {
            lines.push(format!("#   {}: {}", field.name, field.name));
        }
        lines.push(String::new());
    }

    // Behavior
    lines.push("fail_on_error: false".to_string());
    lines.push("skip_existing: true".to_string());

    std::fs::write(output, lines.join("\n") + "\n")?;
    Ok(())
}
```

### Stats Computation (Pure Rust, No NumPy Equivalent Needed)

```rust
// Streaming per-field statistics without loading all data into memory
struct FieldStats {
    count: usize,
    min: Vec<f64>,
    max: Vec<f64>,
    sum: Vec<f64>,
    sum_sq: Vec<f64>,  // For variance/std computation
}

impl FieldStats {
    fn push(&mut self, values: &[f64]) {
        if self.count == 0 {
            let n = values.len();
            self.min = values.to_vec();
            self.max = values.to_vec();
            self.sum = values.to_vec();
            self.sum_sq = values.iter().map(|v| v * v).collect();
        } else {
            for (i, &v) in values.iter().enumerate() {
                self.min[i] = self.min[i].min(v);
                self.max[i] = self.max[i].max(v);
                self.sum[i] += v;
                self.sum_sq[i] += v * v;
            }
        }
        self.count += 1;
    }

    fn mean(&self) -> Vec<f64> {
        self.sum.iter().map(|s| s / self.count as f64).collect()
    }

    fn std(&self) -> Vec<f64> {
        let n = self.count as f64;
        self.sum.iter().zip(self.sum_sq.iter())
            .map(|(s, sq)| ((sq / n) - (s / n).powi(2)).max(0.0).sqrt())
            .collect()
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| hf-hub 0.3 (limited API) | hf-hub 0.5 (RepoInfo, dataset support) | 2024-2025 | Stable sync API for datasets, authentication, caching |
| clap 3.x builder API | clap 4.5 derive API | 2023 | Cleaner subcommand definitions, better error messages |
| Manual completion scripts | clap_complete 4.5 | 2023+ | Auto-generated from Command definition |
| Python subprocess for HF download | Native Rust hf-hub | 2024 | No Python runtime needed, consistent caching |

**Deprecated/outdated:**
- `structopt` crate: Superseded by clap derive. Do not use.
- `hf-hub` 0.3: API changed significantly. Use 0.5+.
- clap `App` builder: Use `Command` (renamed in clap 4.x).

## Open Questions

1. **hf-hub snapshot root directory derivation**
   - What we know: `repo.get(filename)` returns the absolute path to the cached file. Files are organized under `~/.cache/huggingface/hub/datasets--{org}--{repo}/snapshots/{commit_sha}/...`
   - What's unclear: Whether the path layout is guaranteed stable across hf-hub versions. Whether `repo.info()` provides the snapshot root directly.
   - Recommendation: Parse the returned path to derive the snapshot root. Strip the relative filename from the absolute path. Add a test that verifies the structure.

2. **CLI-06 (hub upload) scope**
   - What we know: ROADMAP assigns HUB-03 (upload) to Phase 8, but Phase 7 success criteria mention `forge hub upload` as a subcommand.
   - What's unclear: Whether Phase 7 needs a working upload or just a stub command.
   - Recommendation: Implement the subcommand skeleton that errors with "not yet implemented -- coming in next release". This satisfies the success criterion "all CLI subcommands work end-to-end" at the subcommand-exists level. Full upload logic is Phase 8.

3. **CLI-10 (visualize) scope**
   - What we know: VIZ-01 (interactive viewer) is Phase 8. CLI-10 says "opens interactive dataset viewer".
   - What's unclear: How much visualization to implement in Phase 7.
   - Recommendation: Phase 7 `forge visualize` prints dataset info (like inspect) and states "Interactive viewer requires Phase 8 features. Use `forge inspect` for dataset details." This keeps the subcommand present without pulling in TUI dependencies.

4. **Field remapping implementation depth**
   - What we know: `apply_field_remapping_static()` is a no-op stub. CLI-12 requires field mapping. `ConversionConfig.fields` has `FieldMapping { source, target, transform }`.
   - What's unclear: What transforms should be supported? The Python version mentions "normalize", "clip".
   - Recommendation: Implement field renaming (source -> target) as the baseline. Defer transforms to a future phase. Document supported transforms as "rename only" in the YAML config comments.

## Sources

### Primary (HIGH confidence)
- Codebase inspection: `crates/forge-cli/src/main.rs` (existing CLI), `crates/forge-core/src/config.rs` (ConversionConfig), `crates/forge-formats/src/registry.rs` (format registry), `crates/forge-quality/src/analyzer.rs` (QualityAnalyzer), `crates/forge-quality/src/filter.rs` (FilterEngine)
- [clap_complete docs](https://docs.rs/clap_complete/4.5.55/clap_complete/) - Shell completion generation API
- [hf-hub GitHub README](https://github.com/huggingface/hf-hub) - Rust HuggingFace Hub client
- [hf-hub docs.rs API](https://docs.rs/hf-hub/0.5.0/hf_hub/) - Full API documentation

### Secondary (MEDIUM confidence)
- [hf-hub crates.io](https://crates.io/crates/hf-hub) - Version and feature flag information
- Python Forge `forge/cli.py` - Reference CLI implementation (inspect, convert, quality, filter, stats, export-video, visualize, hub commands)
- Python Forge `forge/hub/download.py` - Reference Hub download implementation (snapshot_download wrapper, cache dir, auth handling)
- Python Forge `forge/hub/url.py` - Reference hf:// URL parsing

### Tertiary (LOW confidence)
- hf-hub internal API stability (may change between 0.5 and 1.0)
- Exact cache directory layout assumptions (derived from Python huggingface_hub compatibility claims)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - clap 4.5 and hf-hub 0.5 are well-documented with stable APIs for the features we need
- Architecture: HIGH - The library layer is complete and tested. CLI wiring patterns are straightforward.
- Pitfalls: HIGH - Identified from direct codebase analysis (no-op stubs, missing snapshot_download) and verified against hf-hub docs
- Hub integration: MEDIUM - hf-hub 0.5 API is functional but pre-1.0. Cache path derivation needs validation.

**Research date:** 2026-02-22
**Valid until:** 2026-03-22 (30 days -- stable domain, pre-1.0 hf-hub may change)
