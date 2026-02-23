//! YAML configuration loading with CLI argument override.
//!
//! Loads a base [`ConversionConfig`] from an optional YAML file, then
//! applies CLI argument overrides. CLI arguments only override when
//! explicitly set (using `Option<T>` to distinguish).

use std::path::Path;

use anyhow::Result;
use forge_core::ConversionConfig;

/// Load a conversion config from an optional YAML file and merge CLI overrides.
///
/// If `config_path` is `Some`, the YAML file is loaded as the base config.
/// Otherwise, `ConversionConfig::default()` is used. CLI arguments override
/// config values only when explicitly provided (non-None / true for flags).
///
/// The `cameras` parameter extends (does not replace) the config's camera
/// mappings -- CLI camera mappings are added on top of YAML cameras.
#[allow(clippy::too_many_arguments)]
pub fn load_config(
    config_path: Option<&Path>,
    cli_source_format: Option<&str>,
    cli_target_format: Option<&str>,
    cli_fps: Option<f64>,
    cli_cameras: &[(String, String)],
    cli_fail_on_error: bool,
    cli_deterministic: bool,
    cli_num_workers: Option<usize>,
    cli_quiet: bool,
    cli_json: bool,
    cli_verbose: bool,
) -> Result<ConversionConfig> {
    let mut config = if let Some(path) = config_path {
        ConversionConfig::from_yaml(path)?
    } else {
        ConversionConfig::default()
    };

    // CLI args override config (only when explicitly set).
    if let Some(fmt) = cli_source_format {
        config.source_format = Some(fmt.to_string());
    }
    if let Some(fmt) = cli_target_format {
        config.target_format = Some(fmt.to_string());
    }
    if let Some(fps) = cli_fps {
        config.fps = Some(fps);
    }

    // Cameras extend (don't replace).
    for (src, dst) in cli_cameras {
        config.cameras.insert(src.clone(), dst.clone());
    }

    // Boolean flags override when true (flag presence means true).
    if cli_fail_on_error {
        config.fail_on_error = true;
    }
    if cli_deterministic {
        config.deterministic = true;
    }
    if let Some(workers) = cli_num_workers {
        config.num_workers = workers;
    }
    if cli_quiet {
        config.quiet = true;
    }
    if cli_json {
        config.json_output = true;
    }
    if cli_verbose {
        config.verbose = true;
    }

    // Default target format if not set anywhere.
    if config.target_format.is_none() {
        config.target_format = Some("lerobot-v3".to_string());
    }

    Ok(config)
}
