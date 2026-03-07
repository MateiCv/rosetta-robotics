# Rosetta Robotics

**Any format in, any format out.** The universal robotics dataset converter for Rust and Python.

```
cargo install rosetta-robotics        pip install rosetta-robotics
```

```
                    +-------------+
  HDF5      -----\ |             | /-----      LeRobot v3
  Zarr      -----\ |             | /-----      LeRobot v2
  RLDS      -----\ |   ROSETTA   | /-----      HDF5
  ROS Bag   -----\ |             | /-----      Zarr
  LeRobot   -----/ |  convert    | \-----      RLDS
  GR00T     -----/ |  inspect    | \-----      ROS Bag
  RoboDM    -----/ |  quality    | \-----      RoboDM
                    +-------------+
```

## Supported Formats

| Format | Read | Write | Description |
|--------|------|-------|-------------|
| LeRobot v3 | Yes | Yes | Parquet + MP4 video |
| LeRobot v2 | Yes | Yes | Per-episode Parquet + MP4 |
| GR00T | Yes | - | NVIDIA Isaac / GR00T |
| HDF5 | Yes | Yes | robomimic, ACT, ALOHA |
| Zarr | Yes | Yes | Diffusion Policy, UMI |
| RLDS | Yes | Yes | TFRecord, Open X-Embodiment |
| ROS Bag | Yes | Yes | ROS 1 bags, ROS 2 MCAP |
| RoboDM | Yes | Yes | Berkeley .vla |

---

## Installation

**Rust (CLI)**
```bash
cargo install --git https://github.com/verlet-robotics/rosetta
```

**Rust (Library)** — add to your `Cargo.toml`:
```toml
[dependencies]
rosetta-core = { git = "https://github.com/verlet-robotics/rosetta" }
rosetta-formats = { git = "https://github.com/verlet-robotics/rosetta" }
rosetta-pipeline = { git = "https://github.com/verlet-robotics/rosetta" }
rosetta-quality = { git = "https://github.com/verlet-robotics/rosetta" }
```

**Python** — requires a [Rust toolchain](https://rustup.rs):
```bash
pip install git+https://github.com/verlet-robotics/rosetta
```

---

## CLI Usage

### `rosetta formats`

Lists all supported dataset formats and whether they support reading, writing, or both.

```bash
rosetta formats
rosetta formats --json    # output as JSON
```

### `rosetta inspect`

Shows dataset metadata without loading all data — format, episode count, frame count, camera resolutions, field schemas (state/action dimensions and dtypes), and FPS.

```bash
rosetta inspect /path/to/dataset
rosetta inspect /path/to/dataset --json
rosetta inspect /path/to/dataset --generate-config config.yaml   # generate a conversion config template
```

### `rosetta convert`

Converts a dataset from one format to another. Auto-detects the source format. Runs in parallel with memory-aware throttling.

```bash
rosetta convert /path/to/input /path/to/output --target-format lerobot-v3
rosetta convert input/ output/ --source-format hdf5 --target-format zarr
rosetta convert input/ output/ --target-format rlds --config conversion.yaml   # field/camera remapping
rosetta convert input/ output/ --target-format lerobot-v3 --deterministic      # bit-identical output
rosetta convert input/ output/ --target-format lerobot-v3 --threads 4          # limit parallelism
```

### `rosetta quality`

Runs quality analysis on a dataset. Computes per-episode scores across 7 metrics: smoothness, dead actions, static detection, gripper chatter, timestamp regularity, action saturation, and action entropy. Returns a weighted composite score (0-1) for each episode.

```bash
rosetta quality /path/to/dataset
rosetta quality /path/to/dataset --json
rosetta quality /path/to/dataset --save-json report.json
rosetta quality /path/to/dataset --config quality.yaml   # custom metric weights
```

### `rosetta filter`

Filters episodes by quality score. Copies only episodes that meet the threshold to the output, leaving the source unchanged.

```bash
rosetta filter /path/to/dataset /path/to/output --min-quality 0.7
rosetta filter /path/to/dataset --dry-run --min-quality 0.5   # preview without writing
rosetta filter /path/to/dataset /path/to/output --config filter.yaml
```

### `rosetta stats`

Shows dataset statistics with distribution analysis — frame counts, action/state ranges, image dimensions, and per-episode breakdowns.

```bash
rosetta stats /path/to/dataset
rosetta stats /path/to/dataset --json
```

### `rosetta export-video`

Exports episodes as standalone MP4 video files from camera image data.

```bash
rosetta export-video /path/to/dataset --output ./videos
rosetta export-video /path/to/dataset --episodes 0-10 --camera top --fps 30
```

### `rosetta hub`

Download and upload datasets from HuggingFace Hub.

```bash
rosetta hub download lerobot/pusht_v2 --output ./data
rosetta hub upload /path/to/dataset username/my-dataset --token $HF_TOKEN --private
```

### `rosetta visualize`

Opens an interactive terminal UI to browse episodes and frames, view camera images, and inspect state/action values.

```bash
rosetta visualize /path/to/dataset
```

### `rosetta completions`

Generates shell completion scripts for tab-completion of commands, flags, and arguments.

```bash
rosetta completions bash >> ~/.bashrc
rosetta completions zsh >> ~/.zfunc/_rosetta
rosetta completions fish > ~/.config/fish/completions/rosetta.fish
```

---

## Python Usage

```python
import rosetta_robotics

# List all supported formats and their read/write capabilities
for fmt in rosetta_robotics.formats():
    print(fmt.name, fmt.can_read, fmt.can_write)

# Auto-detect the format of a dataset on disk
fmt = rosetta_robotics.detect_format("/path/to/dataset")

# Inspect dataset metadata without loading all data
info = rosetta_robotics.inspect("/path/to/dataset")
print(info.format, info.total_episodes, info.total_frames, info.fps)
for cam in info.cameras:
    print(cam.name, cam.height, cam.width)
for field in info.fields:
    print(field.name, field.shape, field.dtype)

# Read all episodes and access frame data
episodes = rosetta_robotics.read_episodes("/path/to/dataset")
for ep in episodes:
    print(ep.episode_id, ep.num_frames)
    frame = ep.frame(0)
    print(frame.state)       # joint positions as list[float]
    print(frame.action)      # actions as list[float]
    print(frame.image_names) # available camera names

    # Access raw image bytes and dimensions
    raw = frame.image_bytes("top")
    h, w, c = frame.image_shape("top")

# Convert a dataset from one format to another
result = rosetta_robotics.convert(
    "/path/to/input",
    "/path/to/output",
    target_format="lerobot-v3",
    deterministic=True,     # bit-identical output
    num_workers=4,          # limit parallelism
)
print(f"Converted {result.episodes_converted} episodes, {result.total_frames} frames")
if result.errors:
    print("Errors:", result.errors)

# Run quality analysis on a dataset
report = rosetta_robotics.analyze_quality("/path/to/dataset")
print(f"Overall score: {report.overall_score:.2f}")
print(f"Recommendations: {report.recommendations}")
for eq in report.per_episode:
    print(f"  {eq.episode_id}: score={eq.composite_score}, flags={eq.flags}")
```

---

## Rust Usage

```rust
use std::path::Path;

fn main() -> rosetta_core::RosettaResult<()> {
    // List all supported formats
    for fmt in rosetta_formats::all_formats() {
        println!("{}: read={}, write={}", fmt.name, fmt.can_read, fmt.can_write);
    }

    // Auto-detect format and inspect metadata
    let path = Path::new("/path/to/dataset");
    let format_name = rosetta_formats::detect_format(path)?;
    let reader = rosetta_formats::get_reader(format_name)?;
    let info = reader.inspect(path)?;
    println!("{} episodes, {} frames", info.total_episodes, info.total_frames);

    // Read episodes
    let output = reader.read_episodes(path)?;
    for episode in &output.data {
        println!("Episode {}: {} frames", episode.episode_id, episode.frame_count());
        let frame = &episode.frames[0];

        // Access state/action (lazy-loaded)
        if let Some(state) = &frame.state {
            let values = state.load()?;
            println!("  state: {:?}", &values[..5.min(values.len())]);
        }
        if let Some(action) = &frame.action {
            let values = action.load()?;
            println!("  action: {:?}", &values[..5.min(values.len())]);
        }

        // Access images (lazy-loaded)
        for name in frame.image_names() {
            let img = &frame.images[name];
            let (h, w, c) = img.shape();
            println!("  camera '{}': {}x{}x{}", name, h, w, c);
        }
    }

    // Convert between formats
    let config = rosetta_core::ConversionConfig {
        target_format: Some("lerobot-v3".into()),
        deterministic: true,
        ..Default::default()
    };
    let converter = rosetta_pipeline::Converter::new(config);
    let result = converter.convert(
        Path::new("/path/to/input"),
        Path::new("/path/to/output"),
    )?;
    println!("Converted {} episodes ({} frames)", result.episodes_converted, result.total_frames);

    // Quality analysis
    let analyzer = rosetta_quality::QualityAnalyzer::new(rosetta_quality::QualityConfig::default());
    let reader = rosetta_formats::get_reader(format_name)?;
    let report = analyzer.analyze_dataset(reader.as_ref(), path)?;
    println!("Overall quality: {:.2}", report.overall_score);

    Ok(())
}
```

---

MIT License - Verlet Robotics
