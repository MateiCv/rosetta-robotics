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

use rosetta_core::{Episode, RosettaResult};

/// Generic prefetch: spawn a background thread that pushes items from an
/// iterator into a bounded crossbeam channel. Returns a Receiver the
/// consumer can iterate over.
///
/// When the channel is full (capacity reached), the producer blocks until
/// the consumer reads -- natural backpressure. The channel closes when the
/// iterator is exhausted or the consumer drops the receiver.
///
/// # Arguments
///
/// * `items` - Iterator of items (must be `Send` for thread transfer).
/// * `buffer_size` - Bounded channel capacity. Use 2 for episode-level
///   prefetch (small enough to avoid memory pressure, large enough to
///   hide I/O latency).
///
/// # Returns
///
/// A `Receiver` that yields items as they are produced by the background
/// thread. The channel closes when the iterator is exhausted.
pub fn prefetch_items<T, I>(
    items: I,
    buffer_size: usize,
) -> Receiver<T>
where
    T: Send + 'static,
    I: Iterator<Item = T> + Send + 'static,
{
    let (tx, rx) = bounded(buffer_size);

    std::thread::spawn(move || {
        for item in items {
            if tx.send(item).is_err() {
                break; // Consumer dropped the receiver
            }
        }
        // tx drops here, closing the channel
    });

    rx
}

/// Prefetch episodes from an iterator into a bounded channel.
///
/// Convenience wrapper around [`prefetch_items`] for the common case of
/// prefetching `RosettaResult<Episode>` items. Spawns a background thread
/// that reads episodes and sends them into a bounded channel, overlapping
/// episode reading/decoding with episode writing.
///
/// # Arguments
///
/// * `episodes` - Iterator of episodes (must be `Send` for thread transfer).
/// * `buffer_size` - Bounded channel capacity. Default should be 2.
///
/// # Returns
///
/// A `Receiver` that yields episodes as they are decoded by the background
/// thread. The channel closes when the iterator is exhausted.
pub fn prefetch_episodes<I>(
    episodes: I,
    buffer_size: usize,
) -> Receiver<RosettaResult<Episode>>
where
    I: Iterator<Item = RosettaResult<Episode>> + Send + 'static,
{
    prefetch_items(episodes, buffer_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosetta_core::Episode;

    #[test]
    fn prefetch_delivers_all_episodes() {
        let episodes: Vec<RosettaResult<Episode>> = (0..5)
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
        let episodes: Vec<RosettaResult<Episode>> = vec![
            Ok(Episode::new("0")),
            Err(rosetta_core::RosettaError::Video("test error".to_string())),
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
