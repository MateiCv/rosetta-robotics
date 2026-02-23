---
milestone: next
audited: 2026-02-22T00:00:00Z
status: gaps_found
scores:
  requirements: 13/20
  phases: 1/2
  integration: 13/15
  flows: 3/5
gaps:
  requirements:
    - id: "CLI-04"
      status: "partial"
      phase: "07"
      claimed_by_plans: ["07-03-PLAN.md"]
      completed_by_plans: ["07-03-SUMMARY.md"]
      verification_status: "passed"
      evidence: "filter.rs calls get_writer() unconditionally at line 37 before dry-run check at line 54; fails for read-only formats (groot) even in --dry-run mode"
    - id: "CLI-16"
      status: "partial"
      phase: "07"
      claimed_by_plans: ["07-01-PLAN.md"]
      completed_by_plans: ["07-01-SUMMARY.md"]
      verification_status: "passed"
      evidence: "convert.rs passes raw source string to Path::new() at line 57 without calling resolve_and_detect(); hf:// URLs are not resolved for forge convert"
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
  integration:
    - "convert.rs does not call resolve_and_detect(); hf:// source URLs passed directly to Path::new() → filesystem-not-found error (affects CLI-02, CLI-16)"
    - "filter.rs calls get_writer() unconditionally before dry-run check; read-only formats (groot) fail even with --dry-run (affects CLI-04)"
  flows:
    - "forge convert hf://org/repo ./out — breaks at convert.rs:57; hf:// never resolved to local cache path"
    - "forge filter ./groot-dataset ./out --dry-run — breaks at filter.rs:37; get_writer('groot') errors before dry-run logic reached"
tech_debt:
  - phase: 07-cli-configuration-and-hub-download
    items:
      - "output::print_table exported but never called — dead code, compiler warning"
      - "visualize.rs is a placeholder (intentional — Phase 8 VIZ-01 implements full TUI)"
      - "hub upload is a stub directing to Python CLI (intentional — Phase 8 implements full upload)"
      - "Field remapping only supports rename; transform values deferred with warning"
  - phase: v1.0-accumulated
    items:
      - "11 items from v1.0 audit (see .planning/milestones/v1.0-MILESTONE-AUDIT.md)"
  - phase: v2.0-accumulated
    items:
      - "7 items from v2.0 audit (see .planning/milestones/v2.0-MILESTONE-AUDIT.md)"
---

# Milestone "Next" (Phases 7-8) — Audit Report

**Audited:** 2026-02-22
**Status:** GAPS FOUND
**Scope:** Phase 7 (complete, verified), Phase 8 (not started)

---

## Scores

| Category | Score | Details |
|----------|-------|---------|
| Requirements | 13/20 | 13 fully satisfied, 2 partial (integration gaps), 5 unsatisfied (Phase 8) |
| Phases | 1/2 | Phase 7 verified and passed; Phase 8 not started |
| Integration | 13/15 | 2 cross-module wiring gaps found in Phase 7 |
| E2E Flows | 3/5 | 3 complete flows, 2 broken (convert hf://, filter dry-run on read-only) |

---

## Phase Verification Summary

### Phase 7: CLI, Configuration, and Hub Download — PASSED

- **VERIFICATION.md:** 5/5 observable truths verified
- **Plans:** 5/5 completed with atomic commits
- **Requirements:** 15/15 claimed satisfied (2 have integration gaps found post-verification)
- **Compilation:** `cargo build -p forge-cli` passes (1 warning: unused print_table)
- **Tests:** 387 total (21 unit tests in Phase 7 code)

### Phase 8: Hub Upload, Visualization, and Distribution — NOT STARTED

- **VERIFICATION.md:** Missing (no phase directory)
- **Plans:** 0/? (TBD)
- **Requirements:** 5 unsatisfied (HUB-03, HUB-05, VIZ-01, VIZ-02, VIZ-03)

---

## Requirements Coverage (3-Source Cross-Reference)

### Phase 7 Requirements

| REQ-ID | Description | VERIFICATION.md | SUMMARY Frontmatter | ROADMAP Phase | Final Status |
|--------|-------------|----------------|---------------------|---------------|--------------|
| CLI-01 | forge inspect with text/JSON output | passed | 07-02 | 7 | **satisfied** |
| CLI-02 | forge convert with YAML config | passed | 07-02 | 7 | **satisfied** |
| CLI-03 | forge quality analysis | passed | 07-03 | 7 | **satisfied** |
| CLI-04 | forge filter with dry-run | passed | 07-03 | 7 | **partial** |
| CLI-05 | forge hub download | passed | 07-04 | 7 | **satisfied** |
| CLI-06 | Auth for gated repos | passed | 07-04 | 7 | **satisfied** |
| CLI-08 | forge stats per-field distribution | passed | 07-03 | 7 | **satisfied** |
| CLI-09 | forge export-video MP4 export | passed | 07-05 | 7 | **satisfied** |
| CLI-10 | forge visualize exists | passed | 07-05 | 7 | **satisfied** |
| CLI-12 | Modular CLI one-file-per-subcommand | passed | 07-01 | 7 | **satisfied** |
| CLI-13 | inspect --generate-config | passed | 07-02 | 7 | **satisfied** |
| CLI-15 | Shell completions bash/zsh/fish | passed | 07-05 | 7 | **satisfied** |
| CLI-16 | All commands use resolve_and_detect | passed | 07-01 | 7 | **partial** |
| HUB-01 | Hub download dataset files | passed | 07-04 | 7 | **satisfied** |
| HUB-02 | Re-download skip via cache | passed | 07-04 | 7 | **satisfied** |

### Phase 8 Requirements (Not Started)

| REQ-ID | Description | VERIFICATION.md | SUMMARY Frontmatter | ROADMAP Phase | Final Status |
|--------|-------------|----------------|---------------------|---------------|--------------|
| HUB-03 | Hub upload with Git LFS | missing | missing | 8 | **unsatisfied** |
| HUB-05 | Upload verification | missing | missing | 8 | **unsatisfied** |
| VIZ-01 | Interactive dataset viewer | missing | missing | 8 | **unsatisfied** |
| VIZ-02 | Action/state timeseries | missing | missing | 8 | **unsatisfied** |
| VIZ-03 | Episode navigation | missing | missing | 8 | **unsatisfied** |

### Orphaned Requirements

Phase 8 requirements (HUB-03, HUB-05, VIZ-01, VIZ-02, VIZ-03) are assigned in ROADMAP.md but have no phase directory, no VERIFICATION.md, and no SUMMARY.md entries. These are orphaned — assigned but never verified.

---

## Integration Gaps

### 1. `forge convert` bypasses resolve_and_detect (CLI-02, CLI-16)

**File:** `crates/forge-cli/src/commands/convert.rs` line 57
**Issue:** `converter.convert(Path::new(source), ...)` passes the raw source string as a filesystem path without calling `crate::output::resolve_and_detect(source)`. The `Converter::convert()` method calls `registry::detect_format()` internally, which probes the filesystem — but `hub_client::resolve_path()` is never called.
**Impact:** `forge convert hf://lerobot/aloha ./out` produces a filesystem-not-found error. Users must first run `forge hub download` separately and pass the local cache path.
**Fix:** Add `resolve_and_detect` call before constructing the path, or pre-resolve hf:// URLs in convert.rs.

### 2. `forge filter` acquires writer before dry-run check (CLI-04)

**File:** `crates/forge-cli/src/commands/filter.rs` line 37
**Issue:** `get_writer(&format_name)` is called unconditionally. For read-only formats (groot), this returns `ForgeError::UnsupportedFormat` before the dry-run check at line 54.
**Impact:** `forge filter ./groot-dataset ./out --dry-run` errors immediately even though no writing would occur.
**Fix:** Move writer acquisition inside the non-dry-run branch, or use a no-op writer for dry-run mode.

---

## E2E Flow Verification

| Flow | Status | Break Point |
|------|--------|-------------|
| `forge inspect <path>` → review → `forge convert <path> <out>` | **COMPLETE** | — |
| `forge hub download repo` → `forge inspect $(result)` → `forge convert $(result) ./out` | **COMPLETE** | — (using local cache path, not hf:// inline) |
| `forge inspect <path> --generate-config ./my.yaml` → edit → `forge convert <path> <out> --config ./my.yaml` | **COMPLETE** | — |
| `forge convert hf://org/repo ./out` (inline hf:// URL) | **BROKEN** | convert.rs:57 — hf:// never resolved |
| `forge filter ./groot-dataset ./out --dry-run` (read-only format) | **BROKEN** | filter.rs:37 — get_writer fails for groot |

---

## Tech Debt

### Phase 7 Items

| Item | Severity | Notes |
|------|----------|-------|
| `output::print_table` dead code | Low | Exported but never called; compiler warning |
| `visualize.rs` placeholder | Info | Intentional — Phase 8 VIZ-01 implements full TUI |
| Hub upload stub | Info | Intentional — Phase 8 implements full upload |
| Field remapping transform deferred | Low | Only rename supported; transform values log warning |

### Accumulated from Previous Milestones

- **v1.0:** 11 items (see `.planning/milestones/v1.0-MILESTONE-AUDIT.md`)
- **v2.0:** 7 items (see `.planning/milestones/v2.0-MILESTONE-AUDIT.md`)

---

## Human Verification Required

1. **Hub download against real HuggingFace dataset** — `forge hub download lerobot/aloha`
2. **forge export-video with real dataset** — requires actual image frames and system FFmpeg
3. **Shell completions integration** — requires shell configuration testing

---

_Audited: 2026-02-22_
_Auditor: Claude (gsd milestone audit)_
