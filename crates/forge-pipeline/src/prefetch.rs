//! Background episode prefetch via bounded crossbeam channel.
//!
//! Overlaps episode reading/decoding with episode writing for improved
//! throughput on I/O-bound workloads. The prefetch thread is a standalone
//! `std::thread::spawn` (NOT a rayon worker) since it is I/O-bound and
//! should not consume a rayon worker slot.
//!
//! **Note:** The current `FormatReader::read_episodes()` returns
//! `Box<dyn Iterator>` (not `Send`), so the prefetch function requires
//! a `Send` iterator. The Converter collects episodes on the main thread
//! as a workaround. When the trait is updated to return `Send` iterators,
//! prefetch can be used directly with the reader output.

use crossbeam_channel::{bounded, Receiver};

use forge_core::{Episode, ForgeResult};

/// Prefetch episodes from an iterator into a bounded channel.
///
/// Spawns a background thread that reads episodes from the iterator and
/// sends them into a bounded channel. The consumer (Converter) receives
/// episodes from the channel. When the channel is full (capacity reached),
/// the producer blocks until the consumer reads -- natural backpressure.
///
/// This overlaps episode reading/decoding with episode writing, improving
/// throughput for I/O-bound workloads.
///
/// # Arguments
///
/// * `episodes` - Iterator of episodes (must be `Send` for thread transfer).
/// * `buffer_size` - Bounded channel capacity. Default should be 2 (small
///   enough to avoid memory pressure, large enough to hide I/O latency).
///
/// # Returns
///
/// A `Receiver` that yields episodes as they are decoded by the background
/// thread. The channel closes when the iterator is exhausted.
pub fn prefetch_episodes<I>(
    episodes: I,
    buffer_size: usize,
) -> Receiver<ForgeResult<Episode>>
where
    I: Iterator<Item = ForgeResult<Episode>> + Send + 'static,
{
    let (tx, rx) = bounded(buffer_size);

    std::thread::spawn(move || {
        for episode in episodes {
            if tx.send(episode).is_err() {
                break; // Consumer dropped the receiver
            }
        }
        // tx drops here, closing the channel
    });

    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::Episode;

    #[test]
    fn prefetch_delivers_all_episodes() {
        let episodes: Vec<ForgeResult<Episode>> = (0..5)
            .map(|i| Ok(Episode::new(format!("{i}"))))
            .collect();

        let rx = prefetch_episodes(episodes.into_iter(), 2);
        let received: Vec<_> = rx.into_iter().collect();
        assert_eq!(received.len(), 5);
        for (i, result) in received.into_iter().enumerate() {
            let ep = result.unwrap();
            assert_eq!(ep.episode_id, format!("{i}"));
        }
    }

    #[test]
    fn prefetch_handles_errors() {
        let episodes: Vec<ForgeResult<Episode>> = vec![
            Ok(Episode::new("0")),
            Err(forge_core::ForgeError::Video("test error".to_string())),
            Ok(Episode::new("2")),
        ];

        let rx = prefetch_episodes(episodes.into_iter(), 2);
        let received: Vec<_> = rx.into_iter().collect();
        assert_eq!(received.len(), 3);
        assert!(received[0].is_ok());
        assert!(received[1].is_err());
        assert!(received[2].is_ok());
    }
}
