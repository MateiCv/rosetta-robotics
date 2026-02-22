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


## v2.0 Format Breadth (Shipped: 2026-02-22)

**Phases completed:** 2 phases, 12 plans
**Lines of Rust:** 24,628 (+14,654 from v1.0)
**Tests:** 366 passing
**Requirements:** 14/14 satisfied (READ-01..09, WRIT-02..07)
**Timeline:** 2 days (2026-02-21 → 2026-02-22)

**Key accomplishments:**
- 8 format readers (RLDS/TFRecord, LeRobot v2, HDF5, Zarr, ROS bag, RoboDM, GR00T) with scored auto-detection engine
- 6 format writers (RLDS, HDF5, Zarr, ROS bag/MCAP, LeRobot v2, RoboDM) for full bidirectional conversion
- Scored format auto-detection: confidence-ranked identification from filesystem structure alone, no user hints needed
- Round-trip integration tests verified for all 7 writable formats (GR00T read-only)
- Zero external Python dependencies: manual TFRecord, CDR, EBML parsers instead of Python ecosystem crates

**Tech debt carried forward (7 items):**
- Python interop validation deferred (tensorflow_datasets, h5py, zarr, ROS 2 tools)
- stats.json empty placeholder in LeRobot v2/v3 writers
- RoboDM small image heuristic misclassification
- zarrs pinned to 0.22 for MSRV compatibility
- HDF5 requires system libHDF5 (excluded from default features)
- See `.planning/milestones/v2.0-MILESTONE-AUDIT.md` for full list

**Scope note:** CLI, Hub integration, visualization, and distribution (Phases 7-8, 20 requirements) deferred to next milestone.

---

