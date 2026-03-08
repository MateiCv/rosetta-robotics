//! `rosetta stats` subcommand -- show dataset statistics with distribution analysis.
//!
//! Reads all episodes, computes per-field min/max/mean/std, episode length
//! distribution, and displays results as a table or JSON.

use anyhow::Result;
use serde::Serialize;

use crate::output::OutputFormat;

/// Summary statistics for episode lengths across the dataset.
#[derive(Debug, Serialize)]
struct EpisodeLengthStats {
    min: usize,
    max: usize,
    mean: f64,
    std: f64,
}

/// Per-field distribution statistics (min/max/mean/std per dimension).
#[derive(Debug, Serialize)]
struct FieldStatsOutput {
    name: String,
    dims: usize,
    min: Vec<f64>,
    max: Vec<f64>,
    mean: Vec<f64>,
    std: Vec<f64>,
}

/// Complete statistics report for a dataset.
#[derive(Debug, Serialize)]
struct StatsReport {
    path: String,
    format: String,
    total_episodes: usize,
    total_frames: usize,
    episode_lengths: Option<EpisodeLengthStats>,
    fields: Vec<FieldStatsOutput>,
}

/// Streaming accumulator for per-field statistics.
///
/// Tracks count, dimensionality, min, max, sum, and sum of squares for
/// online mean/std computation (Welford-like streaming approach).
struct FieldStats {
    name: String,
    count: usize,
    dims: usize,
    min: Vec<f64>,
    max: Vec<f64>,
    sum: Vec<f64>,
    sum_sq: Vec<f64>,
}

impl FieldStats {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            count: 0,
            dims: 0,
            min: Vec::new(),
            max: Vec::new(),
            sum: Vec::new(),
            sum_sq: Vec::new(),
        }
    }

    /// Push a new frame's values into the accumulator.
    ///
    /// On first call, initializes dimensions from the input length.
    fn push(&mut self, values: &[f64]) {
        if self.count == 0 {
            // First observation: initialize accumulators.
            self.dims = values.len();
            self.min = values.to_vec();
            self.max = values.to_vec();
            self.sum = values.to_vec();
            self.sum_sq = values.iter().map(|v| v * v).collect();
        } else {
            // Update existing accumulators.
            for (i, &v) in values.iter().enumerate() {
                if i < self.dims {
                    if v < self.min[i] {
                        self.min[i] = v;
                    }
                    if v > self.max[i] {
                        self.max[i] = v;
                    }
                    self.sum[i] += v;
                    self.sum_sq[i] += v * v;
                }
            }
        }
        self.count += 1;
    }

    /// Compute the per-dimension mean.
    fn mean(&self) -> Vec<f64> {
        if self.count == 0 {
            return vec![];
        }
        self.sum.iter().map(|s| s / self.count as f64).collect()
    }

    /// Compute the per-dimension standard deviation (population std).
    fn std(&self) -> Vec<f64> {
        if self.count == 0 {
            return vec![];
        }
        let n = self.count as f64;
        self.sum
            .iter()
            .zip(self.sum_sq.iter())
            .map(|(s, sq)| {
                let mean = s / n;
                let variance = (sq / n) - (mean * mean);
                // Guard against floating-point negative variance.
                if variance > 0.0 {
                    variance.sqrt()
                } else {
                    0.0
                }
            })
            .collect()
    }

    /// Convert to the serializable output format.
    fn to_output(&self) -> FieldStatsOutput {
        FieldStatsOutput {
            name: self.name.clone(),
            dims: self.dims,
            min: self.min.clone(),
            max: self.max.clone(),
            mean: self.mean(),
            std: self.std(),
        }
    }
}

/// Run the stats subcommand.
///
/// Reads all episodes, computes per-field statistics and episode length
/// distribution, and prints results as text or JSON.
pub fn run(path: &str, output_format: OutputFormat) -> Result<()> {
    let (resolved_path, format_name) = crate::output::resolve_and_detect(path)?;
    let reader = rosetta_formats::get_reader(&format_name)?;

    // Skip image loading — stats only needs numeric action/state fields.
    let opts = rosetta_formats::ReadOptions { skip_images: true };
    let read_output = reader.read_episodes_with_options(&resolved_path, &opts)?;
    let episodes = &read_output.data;

    let total_episodes = episodes.len();
    let mut total_frames = 0usize;
    let mut episode_lengths: Vec<usize> = Vec::new();

    // Accumulators for numeric fields.
    let mut action_stats = FieldStats::new("action");
    let mut state_stats = FieldStats::new("state");

    for episode in episodes {
        let ep_frames = episode.frames.len();
        total_frames += ep_frames;
        episode_lengths.push(ep_frames);

        for frame in &episode.frames {
            // Accumulate action data.
            if let Some(ref action) = frame.action {
                if let Ok(data) = action.load() {
                    action_stats.push(data);
                }
            }

            // Accumulate state data.
            if let Some(ref state) = frame.state {
                if let Ok(data) = state.load() {
                    state_stats.push(data);
                }
            }
        }
    }

    // Compute episode length statistics.
    let ep_len_stats = if episode_lengths.is_empty() {
        None
    } else {
        let min = *episode_lengths.iter().min().unwrap();
        let max = *episode_lengths.iter().max().unwrap();
        let sum: usize = episode_lengths.iter().sum();
        let mean = sum as f64 / episode_lengths.len() as f64;
        let variance = episode_lengths
            .iter()
            .map(|&l| {
                let diff = l as f64 - mean;
                diff * diff
            })
            .sum::<f64>()
            / episode_lengths.len() as f64;
        let std = if variance > 0.0 {
            variance.sqrt()
        } else {
            0.0
        };

        Some(EpisodeLengthStats {
            min,
            max,
            mean,
            std,
        })
    };

    // Collect field stats (only include fields that had data).
    let mut fields = Vec::new();
    if action_stats.count > 0 {
        fields.push(action_stats.to_output());
    }
    if state_stats.count > 0 {
        fields.push(state_stats.to_output());
    }

    let report = StatsReport {
        path: path.to_string(),
        format: format_name.clone(),
        total_episodes,
        total_frames,
        episode_lengths: ep_len_stats,
        fields,
    };

    match output_format {
        OutputFormat::Json => {
            crate::output::print_json(&report)?;
        }
        OutputFormat::Text => {
            println!("Dataset Statistics: {}", path);
            println!("Format: {}", format_name);
            println!("Episodes: {}", total_episodes);
            println!("Total frames: {}", total_frames);

            if let Some(ref el) = report.episode_lengths {
                println!();
                println!("Episode Lengths:");
                println!(
                    "  Min: {}  Max: {}  Mean: {:.1}  Std: {:.1}",
                    el.min, el.max, el.mean, el.std
                );
            }

            if report.fields.is_empty() {
                println!();
                println!("No numeric fields found.");
            } else {
                println!();
                println!("Field Statistics:");
                println!(
                    "  {:<14} {:<6} {:<12} {:<12} {:<12} {:<12}",
                    "Field", "Dims", "Min", "Max", "Mean", "Std"
                );
                println!("  {}", "-".repeat(68));

                for field in &report.fields {
                    // Show summary: min of all dims, max of all dims, mean of dim 0, std of dim 0.
                    // For compact display, show range across all dimensions.
                    let global_min = field
                        .min
                        .iter()
                        .cloned()
                        .fold(f64::INFINITY, f64::min);
                    let global_max = field
                        .max
                        .iter()
                        .cloned()
                        .fold(f64::NEG_INFINITY, f64::max);
                    let mean_avg = if field.mean.is_empty() {
                        0.0
                    } else {
                        field.mean.iter().sum::<f64>() / field.mean.len() as f64
                    };
                    let std_avg = if field.std.is_empty() {
                        0.0
                    } else {
                        field.std.iter().sum::<f64>() / field.std.len() as f64
                    };

                    println!(
                        "  {:<14} {:<6} {:<12.3} {:<12.3} {:<12.3} {:<12.3}",
                        field.name, field.dims, global_min, global_max, mean_avg, std_avg
                    );
                }
            }
        }
    }

    Ok(())
}
