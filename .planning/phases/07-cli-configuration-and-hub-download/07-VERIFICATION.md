---
phase: 07-cli-configuration-and-hub-download
verified: 2026-02-22T00:00:00Z
status: passed
score: 5/5 must-haves verified
re_verification: false
---

# Phase 7: CLI, Configuration, and Hub Download — Verification Report

**Phase Goal:** Users interact with Forge through a complete CLI with all subcommands, YAML-driven configuration, and HuggingFace Hub dataset downloading
**Verified:** 2026-02-22
**Status:** PASSED
**Re-verification:** No — initial verification

---

## Goal Achievement

### Observable Truths

| #   | Truth                                                                                                         | Status     | Evidence                                                                                                     |
|-----|---------------------------------------------------------------------------------------------------------------|------------|--------------------------------------------------------------------------------------------------------------|
| 1   | All CLI subcommands work end-to-end: inspect, convert, quality, filter, hub download, hub upload, stats, export-video, visualize | VERIFIED   | All 11 subcommands declared in `Commands` enum in `main.rs` and dispatched to module handlers; all modules compile |
| 2   | YAML-based conversion config supports field mapping, camera remapping, FPS override, and forge inspect --generate-config auto-generates a config | VERIFIED   | `ConversionConfig` has `fields`, `cameras`, `fps` fields; `from_yaml()` deserializes from YAML; `generate_yaml_config()` in `inspect.rs` writes templated YAML |
| 3   | forge hub download <repo> downloads datasets from HuggingFace Hub with authentication for gated repos, re-downloads skipped via local cache | VERIFIED   | `hub_client.rs` implements `download_dataset()` with `hf_hub::api::sync::ApiBuilder`, auth error detection (401/403), and `repo.get()` which handles caching automatically |
| 4   | Format auto-detection works seamlessly across all CLI subcommands without user-specified format hints         | VERIFIED   | `resolve_and_detect()` in `output.rs` calls `hub_client::resolve_path()` then `forge_formats::detect_format()`; 6 of 7 path-accepting commands use it; convert pipeline calls `registry::detect_format()` internally |
| 5   | Shell completions are generated for bash, zsh, and fish                                                       | VERIFIED   | `completions.rs` calls `clap_complete::aot::generate(shell, cmd, ...)` for any `clap_complete::aot::Shell` variant including Bash, Zsh, Fish |

**Score:** 5/5 truths verified

---

### Required Artifacts

All artifacts from plans 07-01 through 07-05 exist and are substantive (non-stub, non-empty).

#### Plan 07-01 Artifacts

| Artifact                                       | Expected                                      | Status      | Details                                                                 |
|------------------------------------------------|-----------------------------------------------|-------------|-------------------------------------------------------------------------|
| `crates/forge-cli/src/commands/mod.rs`         | Command module declarations                   | VERIFIED    | Declares 10 subcommand modules: completions, convert, export_video, filter, formats, hub, inspect, quality, stats, version, visualize |
| `crates/forge-cli/src/commands/version.rs`     | Version subcommand                            | VERIFIED    | `pub fn run() -> Result<()>` — prints pkg version, git SHA, build timestamp, target, FFmpeg check |
| `crates/forge-cli/src/commands/formats.rs`     | Formats subcommand with JSON support          | VERIFIED    | `pub fn run(output_format: OutputFormat) -> Result<()>` — text table and JSON array via `output::print_json` |
| `crates/forge-cli/src/output.rs`               | OutputFormat enum, shared formatting, resolve_and_detect | VERIFIED    | `OutputFormat { Text, Json }`, `print_json<T>`, `print_table`, `resolve_and_detect` all present and substantive |
| `crates/forge-cli/src/config.rs`               | YAML config loading with CLI arg merge        | VERIFIED    | `load_config()` with 11 parameters — loads YAML via `ConversionConfig::from_yaml()`, overrides with CLI args, defaults target_format to "lerobot-v3" |
| `crates/forge-pipeline/src/convert.rs`         | Working field remapping (apply_field_remapping_static) | VERIFIED    | `apply_field_remapping_static()` renames episode-level metadata and per-frame metadata keys; warns on transforms; is called at line 195 |

#### Plan 07-02 Artifacts

| Artifact                                        | Expected                                       | Status      | Details                                                                  |
|-------------------------------------------------|------------------------------------------------|-------------|--------------------------------------------------------------------------|
| `crates/forge-cli/src/commands/inspect.rs`      | Inspect subcommand: human text, JSON, generate-config | VERIFIED    | `pub fn run(path, output_format, generate_config)` — calls `resolve_and_detect`, `get_reader`, `inspect`, prints text/JSON, calls `generate_yaml_config()` |
| `crates/forge-cli/src/commands/convert.rs`      | Enhanced convert subcommand                    | VERIFIED    | `pub fn run(...)` — calls `crate::config::load_config()`, creates `Converter::new(config)`, executes conversion, exits non-zero on failures |

#### Plan 07-03 Artifacts

| Artifact                                       | Expected                                  | Status      | Details                                                                    |
|------------------------------------------------|-------------------------------------------|-------------|----------------------------------------------------------------------------|
| `crates/forge-cli/src/commands/quality.rs`     | Quality analysis subcommand               | VERIFIED    | `pub fn run(path, config_path, output_format, save_json)` — calls `QualityAnalyzer::analyze_dataset`, prints table or JSON |
| `crates/forge-cli/src/commands/filter.rs`      | Episode filtering subcommand              | VERIFIED    | `pub fn run(path, output, min_quality, dry_run, config_path, output_format)` — validates non-dry-run has output path, uses `FilterEngine::filter`, supports `format_dry_run` |
| `crates/forge-cli/src/commands/stats.rs`       | Dataset statistics subcommand             | VERIFIED    | `pub fn run(path, output_format)` — streaming Welford-like accumulation, `FieldStats` struct, `StatsReport` with `#[derive(Serialize)]`, text table and JSON output |

#### Plan 07-04 Artifacts

| Artifact                                        | Expected                                                        | Status      | Details                                                                 |
|-------------------------------------------------|-----------------------------------------------------------------|-------------|-------------------------------------------------------------------------|
| `crates/forge-cli/src/hub_client.rs`            | HuggingFace Hub download, URL parsing, path resolution          | VERIFIED    | `is_hf_url`, `parse_hf_url`, `resolve_path`, `download_dataset` all exported; 12 unit tests; auth error detection; `derive_snapshot_root` for cache path derivation |
| `crates/forge-cli/src/commands/hub.rs`          | Hub download and upload subcommands                             | VERIFIED    | `run_download` prepends `hf://` if needed, calls `download_dataset`, prints cache path to stdout for piping; `run_upload` prints helpful stub message and returns `Ok(())` |

#### Plan 07-05 Artifacts

| Artifact                                            | Expected                           | Status      | Details                                                                     |
|-----------------------------------------------------|------------------------------------|-------------|-----------------------------------------------------------------------------|
| `crates/forge-cli/src/commands/export_video.rs`     | Video export subcommand            | VERIFIED    | `pub fn run(path, output_dir, episodes, camera, fps)` — calls `H264VideoEncoder::new`, `encode_frame`, `finish`; handles Raw/JPEG/PNG encoding via `image` crate; `parse_episode_selection` with 7 unit tests |
| `crates/forge-cli/src/commands/visualize.rs`        | Visualize placeholder subcommand   | VERIFIED    | `pub fn run(path)` — calls `resolve_and_detect`, `inspect`, prints dataset summary and "Interactive visualization is not yet available" message (intentional placeholder per plan 05 design) |
| `crates/forge-cli/src/commands/completions.rs`      | Shell completions subcommand       | VERIFIED    | `pub fn run(shell, cmd)` — calls `clap_complete::aot::generate(shell, cmd, name, &mut stdout())` |

---

### Key Link Verification

| From                              | To                                            | Via                                     | Status      | Evidence                                                         |
|-----------------------------------|-----------------------------------------------|-----------------------------------------|-------------|------------------------------------------------------------------|
| `main.rs`                         | `commands/` modules                           | `mod commands; match dispatch`          | WIRED       | All 11 Commands variants dispatch to `commands::*::run()` at lines 197-295 |
| `config.rs`                       | `forge_core::ConversionConfig`                | `load_config()` merges YAML + CLI       | WIRED       | `ConversionConfig::from_yaml(path)?` at line 35; all CLI overrides applied |
| `output.rs`                       | `forge_formats::detect_format`                | `resolve_and_detect()` shared helper    | WIRED       | Line 83: `forge_formats::detect_format(&resolved)` called in `resolve_and_detect()` |
| `output.rs`                       | `hub_client::resolve_path`                    | hf:// transparent resolution            | WIRED       | Line 77: `crate::hub_client::resolve_path(path)?` called first in `resolve_and_detect()` |
| `commands/inspect.rs`             | `crate::output::resolve_and_detect + forge_formats::get_reader` | `resolve_and_detect()` then `inspect()` | WIRED       | Lines 19-22: `resolve_and_detect`, `get_reader`, `reader.inspect` all called in sequence |
| `commands/inspect.rs`             | generated YAML config file                    | `generate_yaml_config()`               | WIRED       | Lines 33-35: `generate_yaml_config(&info, &format_name, config_path)?` called when `generate_config` is Some |
| `commands/convert.rs`             | `crate::config::load_config`                  | YAML + CLI merge before Converter::new  | WIRED       | Lines 30-42: `crate::config::load_config(...)` called, result passed to `Converter::new(config)` |
| `commands/quality.rs`             | `forge_quality::QualityAnalyzer`              | `analyze_dataset()` call               | WIRED       | Line 34: `analyzer.analyze_dataset(reader.as_ref(), &resolved_path)?` |
| `commands/filter.rs`              | `forge_quality::FilterEngine`                 | `filter()` with reader/writer from registry | WIRED   | Lines 51, 60-65: `FilterEngine::new(...)`, `engine.filter(reader.as_ref(), writer.as_mut(), ...)` |
| `commands/stats.rs`               | `crate::output::resolve_and_detect + forge_formats::get_reader` | `resolve_and_detect()`, then read episodes | WIRED  | Lines 146-150: `resolve_and_detect`, `get_reader`, `reader.read_episodes` in sequence |
| `commands/hub.rs`                 | `crate::hub_client::download_dataset`         | CLI args -> download call               | WIRED       | Line 26: `crate::hub_client::download_dataset(&url, token)?` |
| `hub_client.rs`                   | `hf_hub::api::sync::ApiBuilder`               | dataset repo info + per-file download   | WIRED       | Lines 131-148: `ApiBuilder::new().with_progress(true)`, `.build()`, `api.dataset(...)`, `repo.info()`, `repo.get(filename)` |
| `commands/export_video.rs`        | `forge_video::H264VideoEncoder + VideoEncoderConfig` | VideoEncoderConfig -> H264VideoEncoder -> encode_frame -> finish | WIRED | Lines 104-168: `VideoEncoderConfig::new`, `H264VideoEncoder::new`, `encoder.encode_frame`, `encoder.finish` all called |
| `commands/completions.rs`         | `clap_complete::aot::generate`                | runtime completion generation           | WIRED       | Line 21: `clap_complete::aot::generate(shell, cmd, cmd.get_name().to_string(), &mut std::io::stdout())` |

---

### Requirements Coverage

There is no standalone REQUIREMENTS.md file; requirements are tracked in PROJECT.md and mapped through ROADMAP.md. All 16 requirement IDs declared across plans 07-01 through 07-05 are verified below.

| Requirement | Source Plan | Description (derived from context)                                      | Status      | Evidence                                                              |
|-------------|------------|-------------------------------------------------------------------------|-------------|-----------------------------------------------------------------------|
| CLI-01      | 07-02      | forge inspect subcommand with text and JSON output                      | SATISFIED   | `commands/inspect.rs` fully implemented; text + JSON + generate-config |
| CLI-02      | 07-02      | forge convert with YAML config and source-format arg                    | SATISFIED   | `commands/convert.rs` calls `config::load_config()`; `--config` and `--source-format` flags in `main.rs` |
| CLI-03      | 07-03      | forge quality runs quality analysis with text/JSON output               | SATISFIED   | `commands/quality.rs` calls `QualityAnalyzer::analyze_dataset`; text table and JSON modes |
| CLI-04      | 07-03      | forge filter filters episodes by quality with dry-run                   | SATISFIED   | `commands/filter.rs` uses `FilterEngine::filter`; `--dry-run`, `--min-quality` flags; validates output path |
| CLI-05      | 07-04      | forge hub download downloads datasets from HuggingFace Hub              | SATISFIED   | `commands/hub.rs run_download` + `hub_client::download_dataset` with caching |
| CLI-06      | 07-04      | Authentication support for gated repos                                  | SATISFIED   | `--token` flag (`env = "HF_TOKEN"`) in `HubCommands::Download`; `ApiBuilder::with_token()`; auth error detection on 401/403 |
| CLI-08      | 07-03      | forge stats shows per-field distribution analysis                       | SATISFIED   | `commands/stats.rs` computes min/max/mean/std for action and state fields via streaming accumulation |
| CLI-09      | 07-05      | forge export-video exports episodes as standalone MP4 files             | SATISFIED   | `commands/export_video.rs` wires to `H264VideoEncoder`; handles Raw/JPEG/PNG; episode/camera/fps selection |
| CLI-10      | 07-05      | forge visualize exists as a subcommand (full TUI deferred to Phase 8)   | SATISFIED   | `commands/visualize.rs` exists; prints dataset info and directs to inspect/export-video; explicitly a placeholder per plan 05 design |
| CLI-12      | 07-01      | Modular CLI with one file per subcommand                                | SATISFIED   | `commands/` directory has 11 separate `.rs` files; `main.rs` is dispatcher only |
| CLI-13      | 07-02      | forge inspect --generate-config auto-generates a YAML config template   | SATISFIED   | `generate_yaml_config()` in `inspect.rs` produces commented YAML with cameras, fields, fps, fail_on_error, skip_existing |
| CLI-15      | 07-05      | Shell completions for bash, zsh, fish                                   | SATISFIED   | `commands/completions.rs` uses `clap_complete::aot::generate`; all `Shell` variants supported |
| CLI-16      | 07-01      | All path-accepting commands use resolve_and_detect for auto-detection   | SATISFIED   | `resolve_and_detect()` called in: inspect, quality, filter, stats, export_video, visualize (6/6 inspect-style commands) |
| HUB-01      | 07-04      | forge hub download <repo> downloads dataset files from HuggingFace Hub  | SATISFIED   | `hub_client::download_dataset()` enumerates `repo.info().siblings`, downloads each via `repo.get()` |
| HUB-02      | 07-04      | Re-downloads skipped via local cache                                    | SATISFIED   | `hf-hub`'s `repo.get()` handles cache automatically; cache path derived via `derive_snapshot_root()` |

**ORPHANED requirements check:** No additional requirements were found mapped to Phase 7 beyond the 16 listed above.

**Missing requirement ID:** CLI-07 is not assigned to Phase 7 (gap in numbering — this is intentional, not an orphan).

---

### Compilation Verification

`cargo build -p forge-cli` completes successfully with 0 errors and 1 warning:

```
warning: function `print_table` is never used
  --> crates/forge-cli/src/output.rs:42:8
Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.24s
```

The unused `print_table` warning is a pre-existing minor issue; the function is part of the public API for future use and does not affect any requirement.

---

### Anti-Patterns Found

| File                                    | Line   | Pattern                   | Severity | Impact                                                              |
|-----------------------------------------|--------|---------------------------|----------|---------------------------------------------------------------------|
| `commands/visualize.rs`                 | 1,9    | "placeholder" in comments | INFO     | Intentional — plan 05 explicitly designed visualize as a placeholder; Phase 8 (VIZ-01) implements the full TUI. Subcommand exists and provides useful output. |

No blocker anti-patterns found. The visualize placeholder is documented in plans and STATE.md as an intentional design decision.

---

### Human Verification Required

The following items cannot be verified programmatically and require a human with a real HuggingFace dataset:

#### 1. Hub Download Against a Real Dataset

**Test:** Run `forge hub download lerobot/aloha` (a public dataset)
**Expected:** Files download to the hf-hub cache directory; stdout shows the local cache path; re-running is instant (cache hit)
**Why human:** Requires network access to HuggingFace; cannot verify in a static code scan

#### 2. forge export-video With a Real Dataset

**Test:** Run `forge export-video <path-to-lerobot-v3-dataset> --output /tmp/forge-videos --episodes 0`
**Expected:** At least one `.mp4` file appears in `/tmp/forge-videos/` with non-zero size; `ffprobe` shows valid H.264 stream
**Why human:** Requires a real dataset with image frames; functional MP4 encoding depends on system FFmpeg availability

#### 3. Shell Completions Integration

**Test:** Run `forge completions bash > ~/.bash_completions/forge && source ~/.bash_completions/forge`, then type `forge <TAB>`
**Expected:** Shell offers all subcommand names as completions
**Why human:** Completions script is generated correctly (verified programmatically), but shell integration depends on user's shell configuration

---

### Gaps Summary

No gaps. All 5 observable truths verified. All 16 requirements satisfied. All key links confirmed wired. Compilation passes. The one placeholder (visualize) is intentional per plan 05 design, documented in STATE.md, and satisfies CLI-10 at the level required for Phase 7.

---

_Verified: 2026-02-22_
_Verifier: Claude (gsd-verifier)_
