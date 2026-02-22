# Phase 1: Core Types and Project Scaffolding - Context

**Gathered:** 2026-02-21
**Status:** Ready for planning

<domain>
## Phase Boundary

Establish the Cargo workspace with 6 crates (forge-core, forge-video, forge-formats, forge-quality, forge-pipeline, forge-cli), define canonical data models (Episode, Frame, LazyArray, LazyImage, DatasetInfo, ForgeError, ConversionConfig), set up CI pipeline validating FFmpeg and HDF5 linking on Linux and macOS, and produce a minimal `forge version` binary.

</domain>

<decisions>
## Implementation Decisions

### Conversion Algorithm Reference
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

</decisions>

<specifics>
## Specific Ideas

- Research recommends 6-crate workspace: forge-core (zero deps) -> forge-video + forge-quality + forge-formats -> forge-pipeline -> forge-cli
- enum_dispatch for format plugins (compile-time, ~10x faster than trait objects)
- Channel-sink pattern for parallel read / sequential write
- thiserror for library crates, anyhow only at CLI boundary
- pulp for stable SIMD (not std::simd which is nightly-only)
- serde_yaml_ng for YAML (serde_yaml deprecated, serde_yml has security advisory)

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 01-core-types-and-project-scaffolding*
*Context gathered: 2026-02-21*
