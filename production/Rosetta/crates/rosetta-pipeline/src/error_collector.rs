//! Thread-safe error collection for parallel episode processing.
//!
//! [`ErrorCollector`] gathers [`EpisodeFailure`] records from multiple rayon
//! worker threads during parallel conversion. When `fail_on_error` is enabled,
//! an [`AtomicBool`] signals all workers to stop on the first failure.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use rosetta_core::EpisodeFailure;

/// Thread-safe collector for episode processing failures.
///
/// Used during parallel conversion to gather errors from multiple worker
/// threads. Supports atomic early exit when `fail_on_error` is set.
pub struct ErrorCollector {
    failures: Mutex<Vec<EpisodeFailure>>,
    should_stop: AtomicBool,
    fail_on_error: bool,
}

impl ErrorCollector {
    /// Create a new error collector.
    ///
    /// If `fail_on_error` is true, the first recorded failure will signal
    /// all workers to stop via [`should_stop`](Self::should_stop).
    pub fn new(fail_on_error: bool) -> Self {
        Self {
            failures: Mutex::new(Vec::new()),
            should_stop: AtomicBool::new(false),
            fail_on_error,
        }
    }

    /// Record a failure for the given episode. If `fail_on_error` is set,
    /// signals all workers to stop via AtomicBool.
    pub fn record(
        &self,
        episode_index: usize,
        error: &rosetta_core::RosettaError,
        affected_files: Vec<PathBuf>,
    ) {
        // Build context chain from error source chain.
        let mut context = Vec::new();
        let mut current: Option<&dyn std::error::Error> = std::error::Error::source(error);
        while let Some(cause) = current {
            context.push(cause.to_string());
            current = cause.source();
        }

        let failure = EpisodeFailure {
            episode_index,
            error: error.to_string(),
            context,
            affected_files,
        };
        self.failures.lock().unwrap().push(failure);
        if self.fail_on_error {
            self.should_stop.store(true, Ordering::Release);
        }
    }

    /// Check whether workers should stop (called in par_iter closure).
    pub fn should_stop(&self) -> bool {
        self.should_stop.load(Ordering::Acquire)
    }

    /// Consume the collector and return all failures sorted by episode index.
    pub fn into_failures(self) -> Vec<EpisodeFailure> {
        let mut failures = self.failures.into_inner().unwrap();
        failures.sort_by_key(|f| f.episode_index);
        failures
    }

    /// Return count of recorded failures.
    pub fn failure_count(&self) -> usize {
        self.failures.lock().unwrap().len()
    }
}

/// Print a structured failure summary to stderr.
///
/// Formats each failure with its error message, causal chain, and affected
/// files in a human-readable format suitable for CLI output.
pub fn print_failure_summary(failures: &[EpisodeFailure]) {
    eprintln!("\n--- Failure Summary ---");
    eprintln!("{} episodes failed:\n", failures.len());
    for f in failures {
        eprintln!("  Episode {}: {}", f.episode_index, f.error);
        for ctx in &f.context {
            eprintln!("    caused by: {ctx}");
        }
        for file in &f.affected_files {
            eprintln!("    affected: {}", file.display());
        }
        eprintln!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosetta_core::RosettaError;

    #[test]
    fn error_collector_records_failures() {
        let collector = ErrorCollector::new(false);
        let err = RosettaError::Video("test error".to_string());
        collector.record(2, &err, vec![PathBuf::from("file.mp4")]);
        collector.record(0, &err, vec![]);

        assert_eq!(collector.failure_count(), 2);
        assert!(!collector.should_stop());

        let failures = collector.into_failures();
        assert_eq!(failures.len(), 2);
        // Should be sorted by episode index.
        assert_eq!(failures[0].episode_index, 0);
        assert_eq!(failures[1].episode_index, 2);
        assert_eq!(failures[1].affected_files.len(), 1);
    }

    #[test]
    fn error_collector_signals_stop_on_fail_on_error() {
        let collector = ErrorCollector::new(true);
        assert!(!collector.should_stop());

        let err = RosettaError::Video("fatal".to_string());
        collector.record(0, &err, vec![]);

        assert!(collector.should_stop());
    }

    #[test]
    fn error_collector_empty() {
        let collector = ErrorCollector::new(false);
        assert_eq!(collector.failure_count(), 0);
        let failures = collector.into_failures();
        assert!(failures.is_empty());
    }
}
