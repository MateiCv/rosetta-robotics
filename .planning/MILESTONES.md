# Milestones

## v1.0 Core Pipeline (Shipped: 2026-02-22)

**Phases completed:** 4 phases, 14 plans, 25 tasks
**Lines of Rust:** 9,974
**Tests:** 134 passing
**Requirements:** 34/34 satisfied
**Timeline:** 31 days (2026-01-21 → 2026-02-21)

**Key accomplishments:**
- 6-crate Cargo workspace with canonical data models (Episode, Frame, LazyArray, LazyImage), thread-safe lazy loading, CI on Linux + macOS
- Full LeRobot v3 read/write pipeline with H.264 video encoder/decoder, native Arrow Parquet I/O, format registry with auto-detection
- Parallel episode conversion via rayon work-stealing, error continuation, zero-copy Bytes video frames, deterministic mode, progress bars with memory tracking, prefetch pipeline
- 7 streaming quality metrics (LDLJ smoothness, dead actions, gripper chatter, static detection, timestamp regularity, saturation, entropy) with SIMD acceleration via pulp
- Composite quality scoring with configurable weights, early exit optimization, QualityAnalyzer with JSON export, FilterEngine with copy-only dry-run filtering

**Tech debt carried forward (11 items):**
- Field remapping stub (deferred to Phase 5)
- Video file_index=0 hardcoded in reader
- stats.json empty placeholder
- See `.planning/milestones/v1.0-MILESTONE-AUDIT.md` for full list

---

