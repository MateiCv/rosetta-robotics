# Phase 5: Remaining Format Readers - Context

**Gathered:** 2026-02-22
**Status:** Ready for planning

<domain>
## Phase Boundary

Read datasets from 7 new formats (RLDS/TFRecord, LeRobot v2, HDF5, Zarr, ROS bags, RoboDM, GR00T) and implement format auto-detection that identifies all 8 input formats from filesystem structure and file headers without user hints. Each reader converts source data into Forge's internal representation for downstream conversion or analysis.

</domain>

<decisions>
## Implementation Decisions

### Auto-detection behavior
- Best-guess with warning when format can't be confidently identified (proceed with most likely match, don't hard-fail)
- Warn about ambiguity when multiple formats match — show confidence ranking, proceed with top pick
- Use filesystem structure AND file headers for detection (not filesystem only) — read first few bytes/metadata keys to confirm format
- `--format` override flag always respected — if user specifies a format, use that reader unconditionally without second-guessing

### Data fidelity & mapping
- Preserve everything — carry forward all fields including ones Forge doesn't understand, as opaque metadata. Nothing is lost during reading.
- Keep original camera names as-is from the source format. No normalization. User can remap later via YAML config (Phase 7).
- Preserve original timestamp form — keep whatever the source format uses (Unix time, step indices, etc). Don't normalize to a universal scheme. Original semantics preserved.
- Pass through image encoding as-is — don't decode compressed frames (JPEG, PNG, video) during reading. Only decode when needed downstream (visualization, format conversion requiring raw pixels).

### Validation strictness
- Lenient by default — best-effort reading, skip broken episodes, warn about issues, read what's usable
- Summary at end — collect all warnings and show a summary after reading completes (e.g., "47/50 episodes read, 3 skipped (truncated data)") rather than per-episode warnings
- `--strict` flag available — turns all warnings into errors for CI/pipeline use cases
- Union schema for inconsistent episodes — merge all schemas, missing fields become null/default. Every episode has the superset of all fields.

### Format priorities
- All 7 formats equally important — no priority ordering. Implement in whatever order makes technical sense.
- Both ROS 1 (.bag) and ROS 2 (MCAP) reading supported — legacy ROS 1 datasets are still common in the wild

### Claude's Discretion
- Implementation order of the 7 format readers (technical dependencies may dictate grouping)
- Selection of real-world validation datasets for each format (research popular datasets in each format)
- Internal reader architecture and trait design
- Compression handling strategy per format (Blosc for Zarr, etc.)

</decisions>

<specifics>
## Specific Ideas

- Each format reader must be validated against real-world datasets from the community (not just synthetic test fixtures). Claude to research and select 2-3 appropriate validation targets per format.
- Auto-detection should be a scored/ranked system — each format detector returns a confidence score, and the system picks the highest while warning about close alternatives.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 05-remaining-format-readers*
*Context gathered: 2026-02-22*
