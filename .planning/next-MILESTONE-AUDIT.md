---
milestone: next
audited: 2026-02-23T00:00:00Z
status: gaps_found
scores:
  requirements: 15/20
  phases: 2/3
  integration: 15/15
  flows: 8/10
gaps:
  requirements:
    - id: "HUB-03"
      status: "unsatisfied"
      phase: "08"
      claimed_by_plans: []
      completed_by_plans: []
      verification_status: "orphaned"
      evidence: "Phase 8 not started; no phase directory, no plans, no implementation"
    - id: "HUB-05"
      status: "unsatisfied"
      phase: "08"
      claimed_by_plans: []
      completed_by_plans: []
      verification_status: "orphaned"
      evidence: "Phase 8 not started; no phase directory, no plans, no implementation"
    - id: "VIZ-01"
      status: "unsatisfied"
      phase: "08"
      claimed_by_plans: []
      completed_by_plans: []
      verification_status: "orphaned"
      evidence: "Phase 8 not started; no phase directory, no plans, no implementation"
    - id: "VIZ-02"
      status: "unsatisfied"
      phase: "08"
      claimed_by_plans: []
      completed_by_plans: []
      verification_status: "orphaned"
      evidence: "Phase 8 not started; no phase directory, no plans, no implementation"
    - id: "VIZ-03"
      status: "unsatisfied"
      phase: "08"
      claimed_by_plans: []
      completed_by_plans: []
      verification_status: "orphaned"
      evidence: "Phase 8 not started; no phase directory, no plans, no implementation"
  integration: []
  flows:
    - "forge hub upload <path> <repo> — stub only; prints message directing to Python CLI (Phase 8)"
    - "forge visualize <path> — placeholder only; prints dataset summary and 'not yet available' (Phase 8)"
tech_debt:
  - phase: 07-cli-configuration-and-hub-download
    items:
      - "visualize.rs is a placeholder (intentional — Phase 8 VIZ-01 implements full TUI)"
      - "hub upload is a stub directing to Python CLI (intentional — Phase 8 implements full upload)"
      - "Field remapping only supports rename; transform values deferred with warning"
      - "forge convert hf:// has no --token CLI flag; users must use HF_TOKEN env var (UX inconsistency with forge hub download)"
  - phase: v1.0-accumulated
    items:
      - "11 items from v1.0 audit (see .planning/milestones/v1.0-MILESTONE-AUDIT.md)"
  - phase: v2.0-accumulated
    items:
      - "7 items from v2.0 audit (see .planning/milestones/v2.0-MILESTONE-AUDIT.md)"
resolved_since_last_audit:
  - id: "CLI-04"
    was: "partial"
    now: "satisfied"
    resolved_by: "Phase 7.1 — NopWriter for dry-run mode in filter.rs"
  - id: "CLI-16"
    was: "partial"
    now: "satisfied"
    resolved_by: "Phase 7.1 — hub_client::resolve_path() before Converter::convert() in convert.rs"
  - item: "output::print_table dead code"
    was: "tech_debt"
    now: "resolved"
    resolved_by: "Phase 7.1 — function removed entirely"
---

# Milestone "Next" (Phases 7-8) — Audit Report

**Audited:** 2026-02-23
**Status:** GAPS FOUND
**Scope:** Phase 7 (complete, verified), Phase 7.1 (complete, verified — gap closure), Phase 8 (not started)
**Previous audit:** 2026-02-22 (13/20 requirements, 2 integration gaps — now resolved)

---

## Scores

| Category | Score | Details |
|----------|-------|---------|
| Requirements | 15/20 | 15 fully satisfied (up from 13), 5 unsatisfied (Phase 8 not started) |
| Phases | 2/3 | Phase 7 + 7.1 verified and passed; Phase 8 not started |
| Integration | 15/15 | All cross-module wiring verified; 0 gaps (down from 2) |
| E2E Flows | 8/10 | 8 complete flows, 2 stubs/placeholders (Phase 8 features) |

---

## Changes Since Previous Audit (2026-02-22)

Phase 7.1 gap closure resolved all issues found in the previous audit:

| Issue | Previous Status | Current Status | Resolution |
|-------|----------------|----------------|------------|
| CLI-04: filter dry-run on read-only formats | partial | **satisfied** | NopWriter in filter.rs defers get_writer() to non-dry-run branch |
| CLI-16: convert hf:// URL resolution | partial | **satisfied** | hub_client::resolve_path() called before Converter::convert() |
| print_table dead code warning | tech_debt | **resolved** | Function removed entirely from output.rs |
| Integration gaps | 2 | **0** | Both wiring issues fixed and confirmed by integration checker |
| Broken E2E flows | 2 | **0** | `forge convert hf://...` and `forge filter --dry-run` both work |

---

## Phase Verification Summary

### Phase 7: CLI, Configuration, and Hub Download — PASSED

- **VERIFICATION.md:** 5/5 observable truths verified
- **Plans:** 5/5 completed with atomic commits
- **Requirements:** 15/15 satisfied (CLI-04 and CLI-16 gaps fixed by 7.1)
- **Compilation:** `cargo build -p forge-cli` passes with 0 warnings, 0 errors
- **Tests:** 387 total (21 unit tests in Phase 7 code)

### Phase 7.1: CLI Integration Fixes — PASSED

- **VERIFICATION.md:** 3/3 observable truths verified
- **Plans:** 1/1 completed with 3 atomic commits
- **Requirements:** CLI-04 and CLI-16 confirmed satisfied
- **Compilation:** Zero warnings after print_table removal
- **Tests:** All 21 forge-cli tests pass, 99 forge-quality tests pass

### Phase 8: Hub Upload, Visualization, and Distribution — NOT STARTED

- **VERIFICATION.md:** Missing (no phase directory)
- **Plans:** 0/? (TBD)
- **Requirements:** 5 unsatisfied (HUB-03, HUB-05, VIZ-01, VIZ-02, VIZ-03)

---

## Requirements Coverage (3-Source Cross-Reference)

### Phase 7 + 7.1 Requirements

| REQ-ID | Description | VERIFICATION.md | SUMMARY Frontmatter | ROADMAP Phase | Final Status |
|--------|-------------|----------------|---------------------|---------------|--------------|
| CLI-01 | forge inspect with text/JSON output | passed (07) | 07-02 | 7 | **satisfied** |
| CLI-02 | forge convert with YAML config | passed (07) | 07-02 | 7 | **satisfied** |
| CLI-03 | forge quality analysis | passed (07) | 07-03 | 7 | **satisfied** |
| CLI-04 | forge filter with dry-run | passed (07, 07.1) | 07-03, 07.1-01 | 7, 7.1 | **satisfied** |
| CLI-05 | forge hub download | passed (07) | 07-04 | 7 | **satisfied** |
| CLI-06 | Auth for gated repos | passed (07) | 07-04 | 7 | **satisfied** |
| CLI-08 | forge stats per-field distribution | passed (07) | 07-03 | 7 | **satisfied** |
| CLI-09 | forge export-video MP4 export | passed (07) | 07-05 | 7 | **satisfied** |
| CLI-10 | forge visualize exists (placeholder) | passed (07) | 07-05 | 7 | **satisfied** |
| CLI-12 | Modular CLI one-file-per-subcommand | passed (07) | 07-01 | 7 | **satisfied** |
| CLI-13 | inspect --generate-config | passed (07) | 07-02 | 7 | **satisfied** |
| CLI-15 | Shell completions bash/zsh/fish | passed (07) | 07-05 | 7 | **satisfied** |
| CLI-16 | All commands use resolve_and_detect / hf:// resolution | passed (07, 07.1) | 07-01, 07.1-01 | 7, 7.1 | **satisfied** |
| HUB-01 | Hub download dataset files | passed (07) | 07-04 | 7 | **satisfied** |
| HUB-02 | Re-download skip via cache | passed (07) | 07-04 | 7 | **satisfied** |

### Phase 8 Requirements (Not Started)

| REQ-ID | Description | VERIFICATION.md | SUMMARY Frontmatter | ROADMAP Phase | Final Status |
|--------|-------------|----------------|---------------------|---------------|--------------|
| HUB-03 | Hub upload with Git LFS | missing | missing | 8 | **unsatisfied** |
| HUB-05 | Upload verification | missing | missing | 8 | **unsatisfied** |
| VIZ-01 | Interactive dataset viewer | missing | missing | 8 | **unsatisfied** |
| VIZ-02 | Action/state timeseries | missing | missing | 8 | **unsatisfied** |
| VIZ-03 | Episode navigation | missing | missing | 8 | **unsatisfied** |

### Orphaned Requirements

Phase 8 requirements (HUB-03, HUB-05, VIZ-01, VIZ-02, VIZ-03) are assigned in ROADMAP.md but have no phase directory, no VERIFICATION.md, and no SUMMARY.md entries. These are orphaned — assigned but never verified. This is expected since Phase 8 has not been started.

---

## Integration Verification

### Phase 7.1 Gap Closure — Confirmed Fixed

**CLI-04 (filter dry-run):** `NopWriter` in `filter.rs` (lines 15-38) implements all 3 `FormatWriter` methods. Deferred writer branch (lines 66-74) only calls `get_writer()` when `dry_run == false`. Doubly guarded — `FilterEngine::filter()` also skips writer calls when `output=None`. Integration checker confirmed wiring end-to-end.

**CLI-16 (convert hf:// resolution):** `convert.rs` line 52 calls `hub_client::resolve_path(source)` before `Converter::convert()`. All 7 path-accepting commands now resolve hf:// URLs transparently (6 via `resolve_and_detect()`, 1 via direct `resolve_path()`). Integration checker confirmed all 7 callsites wired.

### Cross-Phase Wiring

All 15 cross-module connections verified by integration checker:
- 11 subcommand dispatches from `main.rs` to `commands/*.rs`
- YAML config loading chain: `main.rs` → `config::load_config()` → `ConversionConfig::from_yaml()`
- Hub resolution chain: all path commands → `resolve_and_detect()` → `hub_client::resolve_path()` → `download_dataset()`
- Quality pipeline: `quality.rs` → `QualityAnalyzer`, `filter.rs` → `FilterEngine`, `stats.rs` → streaming accumulators

### Integration Notes (Non-Blocking)

- `forge convert hf://...` has no `--token` CLI flag — users must use `HF_TOKEN` env var. The env var fallback works (hf-hub library reads it automatically), but UX is inconsistent with `forge hub download --token`. Low severity.

---

## E2E Flow Verification

| Flow | Status | Notes |
|------|--------|-------|
| `forge inspect <path>` → text/JSON/generate-config | ✓ COMPLETE | CLI-01, CLI-13 |
| `forge convert <src> <out> --config config.yaml` | ✓ COMPLETE | CLI-02 |
| `forge convert hf://org/repo ./out` (inline hf://) | ✓ COMPLETE | CLI-16 — **fixed in 7.1** |
| `forge hub download repo` → `forge convert $(result) ./out` | ✓ COMPLETE | HUB-01, CLI-02 |
| `forge quality <path> --save-json` | ✓ COMPLETE | CLI-03 |
| `forge filter <path> <out> --dry-run` (read-only format) | ✓ COMPLETE | CLI-04 — **fixed in 7.1** |
| `forge stats <path> --json` | ✓ COMPLETE | CLI-08 |
| `forge export-video <path> --output <dir>` | ✓ COMPLETE | CLI-09 |
| `forge hub upload <path> <repo>` | ○ STUB | Phase 8 (HUB-03) |
| `forge visualize <path>` | ○ PLACEHOLDER | Phase 8 (VIZ-01) |

---

## Tech Debt

### Phase 7 Items

| Item | Severity | Notes |
|------|----------|-------|
| `visualize.rs` placeholder | Info | Intentional — Phase 8 VIZ-01 implements full TUI |
| Hub upload stub | Info | Intentional — Phase 8 implements full upload |
| Field remapping transform deferred | Low | Only rename supported; transform values log warning |
| convert hf:// no --token flag | Low | Uses HF_TOKEN env var; inconsistent with hub download UX |

### Resolved Since Last Audit

| Item | Was | Now |
|------|-----|-----|
| `output::print_table` dead code | tech_debt (compiler warning) | **Resolved** — removed in Phase 7.1 |
| CLI-04 filter dry-run on read-only | partial (integration gap) | **Resolved** — NopWriter in Phase 7.1 |
| CLI-16 convert hf:// resolution | partial (integration gap) | **Resolved** — resolve_path in Phase 7.1 |

### Accumulated from Previous Milestones

- **v1.0:** 11 items (see `.planning/milestones/v1.0-MILESTONE-AUDIT.md`)
- **v2.0:** 7 items (see `.planning/milestones/v2.0-MILESTONE-AUDIT.md`)

---

## Human Verification Required

1. **Hub download against real HuggingFace dataset** — `forge hub download lerobot/aloha`
2. **forge convert hf:// end-to-end** — `forge convert hf://lerobot/aloha_sim_insertion_scripted ./out-test`
3. **forge filter --dry-run on groot dataset** — requires a real groot-format dataset
4. **forge export-video with real dataset** — requires actual image frames and system FFmpeg
5. **Shell completions integration** — requires shell configuration testing

---

## Build and Test Status

- `cargo build -p forge-cli` — **0 warnings, 0 errors**
- `cargo test -p forge-cli` — **21/21 passed**
- `cargo test -p forge-quality` — **99/99 passed**
- `cargo test --workspace` — **360+ passed, 0 failed**

---

_Audited: 2026-02-23_
_Auditor: Claude (gsd milestone audit)_
_Previous audit: 2026-02-22 (13/20 → 15/20 requirements)_
