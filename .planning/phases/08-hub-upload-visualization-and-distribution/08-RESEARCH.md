# Phase 8: Hub Upload, Visualization, and Distribution - Research

**Researched:** 2026-02-22
**Domain:** HuggingFace Hub API (upload), Terminal TUI (visualization), Cross-platform CI/CD (distribution)
**Confidence:** MEDIUM

## Summary

Phase 8 covers three distinct domains: (1) implementing `forge hub upload` to push datasets to HuggingFace Hub, replacing the current stub, (2) building an interactive terminal dataset viewer with camera frame display, timeseries charts, and episode navigation, and (3) setting up cross-platform binary releases via GitHub Actions for 5 target platforms.

The critical finding is that the `hf-hub` Rust crate (v0.5.0) is **download-only** -- it has no upload, create_repo, or commit API. Hub upload must be implemented using direct HTTP calls to the HuggingFace Hub REST API (create repo at `POST /api/repos/create`, upload files via the commit API at `POST /api/{repo_type}s/{repo_id}/commit/{revision}`). The `ureq` HTTP client is already an indirect dependency via hf-hub. For the TUI viewer, `ratatui` (v0.29 or v0.30) with crossterm backend is the standard Rust TUI library, with `ratatui-image` providing terminal image rendering via halfblock fallback. For distribution, `cross` + GitHub Actions matrix builds with tag-triggered releases is the established pattern.

**Primary recommendation:** Implement hub upload as a custom HTTP client module using ureq directly against the HuggingFace API, build the TUI viewer with ratatui + ratatui-image, and create a release workflow using cross for Linux ARM and native builds for other targets.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| HUB-03 | Hub upload with Git LFS for large files | Custom HTTP client against HF commit API; LFS handling for files >10MB via preupload endpoint |
| HUB-05 | Upload verification (uploaded dataset is downloadable/usable) | Re-download via existing hub_client::download_dataset and inspect to verify round-trip |
| VIZ-01 | Interactive dataset viewer displaying camera frames | ratatui + ratatui-image for halfblock image rendering in terminal |
| VIZ-02 | Action/state timeseries display | ratatui Chart widget with Dataset/Line rendering for numeric field timeseries |
| VIZ-03 | Episode navigation support | ratatui event loop with crossterm key events; left/right arrow for episode switching |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| ratatui | 0.29 | Terminal UI framework | De facto Rust TUI library, 11M+ downloads, immediate-mode rendering with Chart/Table/Sparkline widgets |
| crossterm | 0.28 | Terminal backend for ratatui | Default cross-platform terminal abstraction, works on Linux/macOS/Windows |
| ratatui-image | 0.5 | Image rendering in terminal | Only ratatui image widget; halfblock fallback works on all terminals |
| ureq | 2.x | HTTP client for Hub API | Already an indirect dependency via hf-hub; sync API matches project pattern |
| cross | 0.2.x | Cross-compilation tool | Docker-based cross-compilation for Linux aarch64 from x86_64 runners |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| sha2 | 0.10 | SHA-256 hashing for LFS | Computing file hashes for Git LFS preupload protocol |
| image (already dep) | 0.25 | Image decoding for TUI | Already in workspace; decode camera frames to RGB for halfblock rendering |
| serde_json (already dep) | 1 | JSON for API requests/responses | Already in workspace; Hub API communication |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| ratatui | Direct crossterm | ratatui provides Chart, Table, Layout for free; raw crossterm is too low-level |
| ureq (sync) | reqwest (async) | ureq is already a dep via hf-hub, sync matches existing codebase; reqwest adds tokio dependency |
| ratatui-image | Custom halfblock | ratatui-image handles protocol detection and image scaling; custom would be fragile |
| cross | cargo-zigbuild | cross is more established for Linux cross-compilation; zigbuild is newer but less tested with system deps like FFmpeg |

**Installation (new dependencies):**
```toml
# In workspace Cargo.toml [workspace.dependencies]
ratatui = { version = "0.29", features = ["crossterm"] }
crossterm = "0.28"
ratatui-image = "0.5"
sha2 = "0.10"
```

## Architecture Patterns

### Recommended Project Structure
```
crates/forge-cli/src/
  commands/
    hub.rs              # Enhanced: full upload implementation
    visualize.rs        # Rewritten: interactive TUI viewer
  hub_client.rs         # Enhanced: upload_dataset(), create_repo() functions
  hub_upload.rs         # NEW: upload-specific HTTP logic (LFS, commit API)
  tui/                  # NEW: TUI module
    mod.rs              # App state, event loop
    viewer.rs           # Image/chart rendering
    widgets.rs          # Custom widgets (episode selector, frame viewer)
```

### Pattern 1: Hub Upload via HuggingFace HTTP Commit API
**What:** Direct HTTP calls to create repos and upload files without git/git-lfs CLI dependency.
**When to use:** Always -- the hf-hub Rust crate has no upload API.
**Architecture:**

The HuggingFace Hub provides an HTTP-based commit API that bypasses git entirely. The workflow is:

1. **Create repo:** `POST https://huggingface.co/api/repos/create` with JSON body `{"type": "dataset", "name": "repo-name", "organization": "org-name"}` and `Authorization: Bearer <token>` header.

2. **For small files (<10MB):** Include file content directly in the commit API call.

3. **For large files (>=10MB, needs LFS):**
   - Compute SHA-256 hash and file size
   - POST to LFS batch endpoint to get upload URL
   - PUT file content to the presigned upload URL
   - Reference the LFS pointer in the commit

4. **Create commit:** `POST https://huggingface.co/api/datasets/{repo_id}/commit/main` with multipart body containing file operations.

```rust
// Source: HuggingFace Hub API documentation + Python huggingface_hub source
pub struct HubUploader {
    token: String,
    endpoint: String, // default: "https://huggingface.co"
}

impl HubUploader {
    pub fn create_repo(&self, repo_id: &str, private: bool) -> Result<String> {
        let body = serde_json::json!({
            "type": "dataset",
            "name": repo_id,
            "private": private,
        });
        let resp = ureq::post(&format!("{}/api/repos/create", self.endpoint))
            .header("Authorization", &format!("Bearer {}", self.token))
            .send_json(&body)?;
        // Returns repo URL
        Ok(resp.into_json::<serde_json::Value>()?["url"].as_str().unwrap().to_string())
    }

    pub fn upload_folder(&self, repo_id: &str, local_path: &Path) -> Result<()> {
        // 1. Enumerate files
        // 2. Classify: small (<10MB) vs large (>=10MB, LFS)
        // 3. For LFS files: compute SHA-256, call batch API, upload to presigned URL
        // 4. Build commit operations list
        // 5. POST commit
        todo!()
    }
}
```

### Pattern 2: Ratatui Immediate-Mode TUI with Event Loop
**What:** Standard ratatui application pattern with crossterm backend.
**When to use:** For the interactive dataset viewer (VIZ-01, VIZ-02, VIZ-03).
**Example:**

```rust
// Source: ratatui official documentation
use ratatui::prelude::*;
use crossterm::event::{self, Event, KeyCode};

struct App {
    episodes: Vec<Episode>,
    current_episode: usize,
    current_frame: usize,
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|frame| ui(frame, app))?;
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Left => app.prev_episode(),
                KeyCode::Right => app.next_episode(),
                KeyCode::Up => app.prev_frame(),
                KeyCode::Down => app.next_frame(),
                _ => {}
            }
        }
    }
}

fn ui(frame: &mut Frame, app: &App) {
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(frame.area());

    // Left: camera frame (halfblock image)
    // Right: action/state timeseries chart
    render_camera_frame(frame, layout[0], app);
    render_timeseries(frame, layout[1], app);
}
```

### Pattern 3: GitHub Actions Release Workflow with Matrix Builds
**What:** Tag-triggered workflow that builds binaries for 5 platforms and creates a GitHub release.
**When to use:** For cross-platform distribution.

```yaml
# .github/workflows/release.yml
name: Release
on:
  push:
    tags: ['v*']

jobs:
  build:
    strategy:
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-22.04
            name: linux-x86_64
          - target: aarch64-unknown-linux-gnu
            os: ubuntu-22.04
            name: linux-aarch64
            use_cross: true
          - target: x86_64-apple-darwin
            os: macos-13
            name: macos-x86_64
          - target: aarch64-apple-darwin
            os: macos-14
            name: macos-aarch64
          - target: x86_64-pc-windows-msvc
            os: windows-latest
            name: windows-x86_64
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - name: Install system deps (FFmpeg, etc.)
        # Platform-specific FFmpeg installation
      - name: Build
        # Use cross for linux-aarch64, cargo for others
      - name: Package
        # tar.gz for Linux/macOS, zip for Windows
      - name: Upload artifact
        uses: actions/upload-artifact@v4

  release:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
```

### Anti-Patterns to Avoid
- **Do NOT shell out to git/git-lfs for uploads:** The requirement is a single binary with no runtime dependencies. Use HTTP API directly.
- **Do NOT use tokio/async for hub upload:** The codebase is synchronous. ureq's sync API matches the existing pattern.
- **Do NOT render images pixel-by-pixel:** Use ratatui-image's halfblock renderer which handles scaling, aspect ratio, and color mapping.
- **Do NOT statically link FFmpeg into release binaries:** FFmpeg is a system dependency. Document it as a runtime requirement. Binaries should dynamically link to system FFmpeg.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Terminal image rendering | Custom halfblock/braille renderer | ratatui-image | Handles protocol detection, image scaling, color quantization, aspect ratio |
| Terminal UI layout | Manual ANSI escape code management | ratatui + crossterm | Layout engine, widget library, event handling built-in |
| SHA-256 for LFS | Custom hash implementation | sha2 crate | Cryptographic correctness, SIMD-optimized |
| Cross-compilation | Custom Docker containers | cross tool | Pre-built Docker images for all common targets |
| Release artifact packaging | Custom shell scripts | GitHub Actions upload-artifact + softprops/action-gh-release | Standard pattern, handles asset naming and checksums |
| Timeseries chart rendering | Custom ASCII chart | ratatui::widgets::Chart | Native support for multiple datasets, axes, labels |

**Key insight:** The upload implementation is the one area where custom code is necessary, because the hf-hub crate lacks upload support. But even there, the HTTP protocol is well-documented and straightforward -- create repo, upload files, create commit.

## Common Pitfalls

### Pitfall 1: HuggingFace LFS File Size Threshold
**What goes wrong:** Uploading large files (Parquet, MP4) without LFS causes HTTP 413 or silent data truncation.
**Why it happens:** HuggingFace Hub requires Git LFS for files >10MB. The HTTP commit API rejects large inline files.
**How to avoid:** Check file size before upload. For files >=10MB, use the LFS batch API to get a presigned upload URL, upload the file there, then reference the LFS pointer in the commit.
**Warning signs:** Upload succeeds but file is truncated, or API returns 413/422 error.

### Pitfall 2: HuggingFace API Authentication Flow
**What goes wrong:** Token not found, wrong token scope, or 401/403 errors during upload.
**Why it happens:** Upload requires write-scoped tokens. Read-only tokens work for download but fail for create_repo/commit.
**How to avoid:** Validate token has write scope early. Support both `--token` CLI flag and `HF_TOKEN` env var. Provide clear error messages distinguishing "no token" from "wrong permissions."
**Warning signs:** Download works but upload fails with 401/403.

### Pitfall 3: Terminal Alternate Screen and Signal Handling
**What goes wrong:** TUI crashes and leaves terminal in raw mode, corrupting the user's shell.
**Why it happens:** Panic or early exit without restoring terminal state.
**How to avoid:** Use `std::panic::set_hook` to restore terminal on panic. Use `scopeguard` or Drop impl to ensure cleanup. ratatui's `init()` / `restore()` pattern handles this.
**Warning signs:** After Ctrl+C or error, terminal echoing is broken, cursor is invisible.

### Pitfall 4: FFmpeg Dynamic Linking in Release Binaries
**What goes wrong:** Release binary crashes with "libavcodec not found" on user's machine.
**Why it happens:** Binary dynamically links to system FFmpeg, which may not be installed or may be a different version.
**How to avoid:** Document FFmpeg as a runtime requirement. Include installation instructions per platform. Consider feature-gating FFmpeg-dependent features.
**Warning signs:** Binary works in CI but fails on fresh user machines.

### Pitfall 5: Cross-Compilation with System Library Dependencies
**What goes wrong:** `cross` build fails because FFmpeg headers/libs are not available in the Docker container.
**Why it happens:** cross's default Docker images do not include FFmpeg development libraries.
**How to avoid:** Use custom cross Docker images (Cross.toml `[target.aarch64-unknown-linux-gnu] image = "custom-image"`) or install FFmpeg dev libs in a `pre-build` script. For macOS, use native builds on macOS runners (GitHub provides both x86_64 and aarch64 macOS runners).
**Warning signs:** Build succeeds locally but fails in CI for specific targets.

### Pitfall 6: Large Dataset Upload Timeouts
**What goes wrong:** Upload of large datasets (many Parquet files + MP4s) times out or OOMs.
**Why it happens:** Trying to upload all files in a single HTTP request, or loading all file content into memory at once.
**How to avoid:** Upload files individually or in small batches. Stream file content from disk rather than loading into memory. Use progress bars to show upload status.
**Warning signs:** Upload hangs or consumes excessive memory for large datasets.

## Code Examples

Verified patterns from official sources:

### HuggingFace Hub Commit API (from Python huggingface_hub docs)
```
# The commit API workflow:
# 1. Create repo (if needed)
POST https://huggingface.co/api/repos/create
Content-Type: application/json
Authorization: Bearer {token}
{"type": "dataset", "name": "{org}/{repo}", "private": false}

# 2. For LFS files, get upload URL
POST https://huggingface.co/api/datasets/{repo_id}/lfs/objects/batch
Content-Type: application/json
Authorization: Bearer {token}
{"operation": "upload", "objects": [{"oid": "{sha256}", "size": {size}}]}

# 3. Upload LFS file to presigned URL
PUT {presigned_url}
Content-Type: application/octet-stream
{file_bytes}

# 4. Create commit with file operations
POST https://huggingface.co/api/datasets/{repo_id}/commit/main
Authorization: Bearer {token}
# Multipart body with operations
```

### Ratatui Chart Widget for Timeseries (from ratatui docs)
```rust
// Source: https://docs.rs/ratatui/latest/ratatui/widgets/struct.Chart.html
use ratatui::widgets::{Chart, Dataset, Axis, Block, Borders};
use ratatui::style::{Color, Style};

let action_data: Vec<(f64, f64)> = episode.frames.iter()
    .enumerate()
    .filter_map(|(i, f)| {
        f.action.as_ref()
            .and_then(|a| a.load().ok())
            .map(|vals| (i as f64, vals[0]))  // First dimension
    })
    .collect();

let datasets = vec![
    Dataset::default()
        .name("action[0]")
        .marker(ratatui::symbols::Marker::Braille)
        .style(Style::default().fg(Color::Cyan))
        .data(&action_data),
];

let chart = Chart::new(datasets)
    .block(Block::default().title("Action Timeseries").borders(Borders::ALL))
    .x_axis(Axis::default().title("Frame").bounds([0.0, max_frame as f64]))
    .y_axis(Axis::default().title("Value").bounds([min_val, max_val]));

frame.render_widget(chart, area);
```

### ratatui-image Halfblock Rendering (from ratatui-image docs)
```rust
// Source: https://github.com/benjajaja/ratatui-image
use ratatui_image::{Image, Resize, picker::Picker};

// Initialize picker (detects terminal graphics protocol)
let mut picker = Picker::from_termios()?;
picker.guess_protocol();  // Detects sixel/kitty/halfblock

// Load image from raw RGB bytes
let dyn_image = image::DynamicImage::ImageRgb8(
    image::RgbImage::from_raw(width, height, rgb_bytes).unwrap()
);

// Create ratatui-image widget
let image_state = picker.new_resize_protocol(dyn_image);

// Render in the TUI
frame.render_stateful_widget(
    StatefulImage::new(None),
    image_area,
    &mut image_state,
);
```

### Cross-Platform Release Workflow (from GitHub Actions best practices)
```yaml
# Source: https://dzfrias.dev/blog/deploy-rust-cross-platform-github-actions/
# Triggered on version tag push
name: Release
on:
  push:
    tags: ['v*']

permissions:
  contents: write

jobs:
  build:
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-22.04
          - target: aarch64-unknown-linux-gnu
            os: ubuntu-22.04
          - target: x86_64-apple-darwin
            os: macos-13
          - target: aarch64-apple-darwin
            os: macos-14
          - target: x86_64-pc-windows-msvc
            os: windows-latest
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions-rust-lang/setup-rust-toolchain@v1
      - name: Install cross (Linux aarch64 only)
        if: matrix.target == 'aarch64-unknown-linux-gnu'
        run: cargo install cross --locked
      - name: Build
        run: |
          if [ "${{ matrix.target }}" = "aarch64-unknown-linux-gnu" ]; then
            cross build --release --target ${{ matrix.target }} -p forge-cli
          else
            cargo build --release --target ${{ matrix.target }} -p forge-cli
          fi
      - name: Package
        run: |
          cd target/${{ matrix.target }}/release
          tar czf forge-${{ github.ref_name }}-${{ matrix.target }}.tar.gz forge
      - uses: softprops/action-gh-release@v2
        with:
          files: target/${{ matrix.target }}/release/forge-*.tar.gz
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Git LFS for Hub uploads | Xet storage (chunk-based dedup) | May 2025 | New HF repos default to Xet; LFS still supported for compatibility |
| tui-rs (unmaintained) | ratatui (community fork) | 2023 | ratatui is actively maintained, tui-rs is archived |
| Manual release scripts | GitHub Actions with matrix builds | 2023+ | Automated, reproducible, multi-platform |
| ratatui 0.26 single crate | ratatui 0.30 workspace | Late 2025 | Modular architecture, better compile times |

**Deprecated/outdated:**
- tui-rs: Archived, replaced by ratatui
- HuggingFace Git LFS for new repos: New repos default to Xet storage, but Git LFS API still works and should be used for compatibility

**Note on Xet vs LFS:** While HuggingFace is moving to Xet storage for new repos (since May 2025), the LFS-based commit API still works and is far simpler to implement. The xet-core Rust library exists but is primarily designed as a Python binding. For our Rust CLI, using the standard LFS-based commit API is the pragmatic choice -- it covers 100% of use cases and avoids adding the complex xet-core dependency.

## Open Questions

1. **HuggingFace commit API exact multipart format**
   - What we know: The commit endpoint is POST to `/api/datasets/{repo_id}/commit/{revision}`. Python library uses multipart/form-data.
   - What's unclear: Exact multipart field names and format for file operations (add/delete). The OpenAPI spec at `huggingface.co/.well-known/openapi.json` should have this, but was not fully parseable during research.
   - Recommendation: Inspect the Python `huggingface_hub` source code's `_prepare_commit_payload()` method during implementation, or use `curl -v` to trace a real upload from the Python CLI.

2. **FFmpeg availability in cross Docker images**
   - What we know: cross's default Docker images have minimal packages. FFmpeg dev headers are needed for forge-video.
   - What's unclear: Whether a `pre-build` hook or custom Dockerfile is needed for the aarch64 Linux target.
   - Recommendation: Test cross build first; if it fails, create a custom Cross.toml with a Dockerfile that adds FFmpeg dev libs. Alternatively, for release builds, consider making forge-video an optional feature that can be disabled if FFmpeg is not available.

3. **ratatui-image version compatibility with ratatui 0.29**
   - What we know: ratatui-image 0.5.x exists and supports ratatui.
   - What's unclear: Exact version compatibility matrix between ratatui 0.29 and ratatui-image.
   - Recommendation: Check ratatui-image Cargo.toml for its ratatui dependency version during implementation. If version mismatch, either upgrade ratatui to 0.30 or pin ratatui-image to a compatible version.

4. **Windows FFmpeg distribution**
   - What we know: Windows does not have a system package manager for FFmpeg.
   - What's unclear: Whether to bundle FFmpeg DLLs with the Windows binary, link statically, or require user to install FFmpeg separately.
   - Recommendation: Require FFmpeg in PATH on Windows (matching the documentation-based approach). Provide installation instructions pointing to gyan.dev/ffmpeg or chocolatey.

## Sources

### Primary (HIGH confidence)
- [HuggingFace Hub upload documentation](https://huggingface.co/docs/huggingface_hub/guides/upload) - Upload workflow, LFS handling, commit API
- [ratatui GitHub](https://github.com/ratatui/ratatui) - TUI framework, version 0.29/0.30
- [ratatui-image GitHub](https://github.com/benjajaja/ratatui-image) - Terminal image rendering widget
- [hf-hub crate docs](https://docs.rs/hf-hub/latest/hf_hub/) - Confirmed download-only, no upload API
- [Existing codebase](./crates/forge-cli/src/) - hub_client.rs, hub.rs, visualize.rs stubs

### Secondary (MEDIUM confidence)
- [Cross-platform Rust GitHub Actions blog](https://dzfrias.dev/blog/deploy-rust-cross-platform-github-actions/) - Release workflow patterns
- [houseabsolute/actions-rust-cross](https://github.com/houseabsolute/actions-rust-cross) - Cross-compilation GitHub Action
- [HuggingFace Hub API docs](https://huggingface.co/docs/hub/api) - REST API reference (moved to OpenAPI playground)
- [taiki-e/upload-rust-binary-action](https://github.com/taiki-e/upload-rust-binary-action) - Alternative release action

### Tertiary (LOW confidence)
- HuggingFace commit API exact multipart format -- inferred from Python source, not directly verified against API spec
- Xet storage details -- new default but LFS still supported; not directly tested from Rust

## Metadata

**Confidence breakdown:**
- Hub Upload: MEDIUM - HF commit API is well-documented in Python, but exact Rust implementation needs tracing the Python source for multipart format details
- TUI Visualization: HIGH - ratatui is extremely well-documented, ratatui-image is the standard solution for terminal images
- Distribution: HIGH - GitHub Actions matrix builds for Rust is a thoroughly documented pattern with many examples
- Pitfalls: HIGH - Common issues are well-known in the Rust and HuggingFace ecosystems

**Research date:** 2026-02-22
**Valid until:** 2026-03-22 (30 days -- stable domain)
