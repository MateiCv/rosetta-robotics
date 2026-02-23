//! TUI rendering: camera frame display, timeseries charts, status bar.
//!
//! Camera images are rendered using Unicode halfblock characters, where each
//! character cell represents two vertical pixels (top pixel as foreground color,
//! bottom pixel as background color, using the upper-half-block character).
//! This works in all terminals without requiring Sixel or Kitty protocol support.

use forge_core::lazy::ImageEncoding;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, Paragraph},
    Frame,
};

use super::App;

/// Colors used for timeseries dimensions.
const SERIES_COLORS: [Color; 6] = [
    Color::Cyan,
    Color::Yellow,
    Color::Green,
    Color::Magenta,
    Color::Red,
    Color::Blue,
];

/// Render the full TUI interface: camera frame, timeseries chart, and status bar.
pub fn render_ui(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Main vertical split: top content (90%) + status bar (3 lines).
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(area);

    // Top area: camera frame (left 50%) + timeseries chart (right 50%).
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[0]);

    // Render each panel.
    render_camera_panel(frame, app, content_chunks[0]);
    render_timeseries_panel(frame, app, content_chunks[1]);
    render_status_bar(frame, app, main_chunks[1]);
}

/// Render the camera image panel using halfblock characters.
fn render_camera_panel(frame: &mut Frame, app: &App, area: Rect) {
    let camera_name = if app.camera_names.is_empty() {
        "(no cameras)"
    } else {
        &app.camera_names[app.current_camera]
    };

    let block = Block::default()
        .title(format!(" Camera: {} ", camera_name))
        .borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.camera_names.is_empty() {
        let msg = Paragraph::new("No camera streams in this dataset.");
        frame.render_widget(msg, inner);
        return;
    }

    let current_frame = app.current_frame_ref();
    let cam_name = &app.camera_names[app.current_camera];

    let lazy_img = match current_frame.images.get(cam_name) {
        Some(img) => img,
        None => {
            let msg = Paragraph::new(format!("No image for camera '{}' at this frame.", cam_name));
            frame.render_widget(msg, inner);
            return;
        }
    };

    // Load and decode the image bytes.
    let bytes = match lazy_img.load() {
        Ok(b) => b,
        Err(e) => {
            let msg = Paragraph::new(format!("Error loading image: {e}"));
            frame.render_widget(msg, inner);
            return;
        }
    };

    let width = lazy_img.width;
    let height = lazy_img.height;
    let encoding = lazy_img.encoding;

    // Decode to RGB pixel data.
    let rgb_data: Vec<u8> = match decode_to_rgb(bytes, width, height, lazy_img.channels, encoding) {
        Ok(data) => data,
        Err(e) => {
            let msg = Paragraph::new(format!("Error decoding image: {e}"));
            frame.render_widget(msg, inner);
            return;
        }
    };

    // Render as halfblock characters.
    let lines = render_halfblocks(&rgb_data, width, height, inner.width as u32, inner.height as u32);
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Decode image bytes to raw RGB based on encoding type.
fn decode_to_rgb(
    bytes: &[u8],
    width: u32,
    height: u32,
    channels: u8,
    encoding: ImageEncoding,
) -> Result<Vec<u8>, String> {
    match encoding {
        ImageEncoding::Raw if channels == 3 => Ok(bytes.to_vec()),
        ImageEncoding::Jpeg | ImageEncoding::Png => {
            let decoded = image::load_from_memory(bytes)
                .map_err(|e| format!("{:?} decode failed: {e}", encoding))?
                .to_rgb8();
            Ok(decoded.into_raw())
        }
        _ => {
            // For Raw with channels != 3, or CompressedBytes, try the image crate.
            image::load_from_memory(bytes)
                .map(|img| img.to_rgb8().into_raw())
                .or_else(|_| {
                    // Last resort: if it's raw data with enough bytes, try direct conversion.
                    if channels == 3 && bytes.len() == (width * height * 3) as usize {
                        Ok(bytes.to_vec())
                    } else {
                        Err(format!(
                            "Cannot decode {:?} image ({}x{}, {} channels, {} bytes)",
                            encoding,
                            width,
                            height,
                            channels,
                            bytes.len()
                        ))
                    }
                })
        }
    }
}

/// Render RGB pixel data as halfblock characters.
///
/// Each terminal cell displays two vertical pixels: the top pixel is the
/// foreground color and the bottom pixel is the background color, using
/// the Unicode upper-half-block character (U+2580).
///
/// The image is downsampled with nearest-neighbor to fit the available area.
fn render_halfblocks<'a>(
    rgb: &[u8],
    img_w: u32,
    img_h: u32,
    term_w: u32,
    term_h: u32,
) -> Vec<Line<'a>> {
    if img_w == 0 || img_h == 0 || term_w == 0 || term_h == 0 {
        return vec![Line::from("(empty image)")];
    }

    // Each terminal row displays 2 pixel rows, so available pixel height = term_h * 2.
    let avail_px_h = term_h * 2;

    // Calculate scale factor to fit image in the available area.
    let scale_x = img_w as f64 / term_w as f64;
    let scale_y = img_h as f64 / avail_px_h as f64;
    let scale = f64::max(scale_x, scale_y);

    // Output dimensions in terminal cells.
    let out_w = ((img_w as f64 / scale).ceil() as u32).min(term_w);
    let out_h_px = ((img_h as f64 / scale).ceil() as u32).min(avail_px_h);
    let out_h_cells = out_h_px.div_ceil(2);

    let stride = (img_w * 3) as usize;

    let get_pixel = |px: u32, py: u32| -> (u8, u8, u8) {
        // Nearest-neighbor sample from the source image.
        let sx = ((px as f64 * scale) as u32).min(img_w - 1);
        let sy = ((py as f64 * scale) as u32).min(img_h - 1);
        let offset = (sy as usize) * stride + (sx as usize) * 3;
        if offset + 2 < rgb.len() {
            (rgb[offset], rgb[offset + 1], rgb[offset + 2])
        } else {
            (0, 0, 0)
        }
    };

    let mut lines = Vec::with_capacity(out_h_cells as usize);

    for row in 0..out_h_cells {
        let top_py = row * 2;
        let bot_py = row * 2 + 1;

        let mut spans = Vec::with_capacity(out_w as usize);
        for col in 0..out_w {
            let (tr, tg, tb) = get_pixel(col, top_py);
            let fg = Color::Rgb(tr, tg, tb);

            if bot_py < out_h_px {
                let (br, bg, bb) = get_pixel(col, bot_py);
                let bg_color = Color::Rgb(br, bg, bb);
                spans.push(Span::styled(
                    "\u{2580}", // Upper half block
                    Style::default().fg(fg).bg(bg_color),
                ));
            } else {
                // Odd last row: only top pixel, use black background.
                spans.push(Span::styled(
                    "\u{2580}",
                    Style::default().fg(fg).bg(Color::Black),
                ));
            }
        }
        lines.push(Line::from(spans));
    }

    lines
}

/// Render the timeseries chart panel (action or state data).
fn render_timeseries_panel(frame: &mut Frame, app: &App, area: Rect) {
    let episode = app.current_episode_ref();
    let frame_count = episode.frames.len();

    if frame_count == 0 {
        let block = Block::default()
            .title(" Timeseries ")
            .borders(Borders::ALL);
        let msg = Paragraph::new("No frames in this episode.").block(block);
        frame.render_widget(msg, area);
        return;
    }

    // Collect action data across all frames. Fall back to state if no actions.
    let (series_data, series_label) = collect_timeseries(episode);

    if series_data.is_empty() {
        let block = Block::default()
            .title(" Timeseries ")
            .borders(Borders::ALL);
        let msg = Paragraph::new("No numeric data available.").block(block);
        frame.render_widget(msg, area);
        return;
    }

    // Compute axis bounds.
    let x_max = frame_count as f64;
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;
    for series in &series_data {
        for &(_, y) in series {
            if y.is_finite() {
                y_min = y_min.min(y);
                y_max = y_max.max(y);
            }
        }
    }
    // Add a small margin.
    let y_range = y_max - y_min;
    if y_range.abs() < 1e-10 {
        y_min -= 1.0;
        y_max += 1.0;
    } else {
        y_min -= y_range * 0.05;
        y_max += y_range * 0.05;
    }

    // Build ratatui datasets (limit to 6 dimensions to avoid clutter).
    let datasets: Vec<Dataset> = series_data
        .iter()
        .enumerate()
        .take(6)
        .map(|(i, data)| {
            Dataset::default()
                .name(format!("{}[{}]", series_label, i))
                .marker(symbols::Marker::Braille)
                .style(Style::default().fg(SERIES_COLORS[i % SERIES_COLORS.len()]))
                .data(data)
        })
        .collect();

    // Current frame marker as the chart title.
    let title = format!(
        " {} (frame {}/{}) ",
        series_label,
        app.current_frame + 1,
        frame_count
    );

    let chart = Chart::new(datasets)
        .block(Block::default().title(title).borders(Borders::ALL))
        .x_axis(
            Axis::default()
                .title("Frame")
                .bounds([0.0, x_max])
                .labels(vec![
                    Line::from("0"),
                    Line::from(format!("{}", frame_count)),
                ]),
        )
        .y_axis(
            Axis::default()
                .title("Value")
                .bounds([y_min, y_max])
                .labels(vec![
                    Line::from(format!("{:.2}", y_min)),
                    Line::from(format!("{:.2}", y_max)),
                ]),
        );

    frame.render_widget(chart, area);
}

/// Collect timeseries data from an episode.
///
/// Returns `(series_data, label)` where `series_data` is a Vec of per-dimension
/// `(frame_idx, value)` pairs, and `label` is "Action" or "State".
fn collect_timeseries(
    episode: &forge_core::Episode,
) -> (Vec<Vec<(f64, f64)>>, &'static str) {
    // Try actions first, then fall back to state.
    if let Some(series) = try_collect_arrays(episode, true) {
        if !series.is_empty() {
            return (series, "Action");
        }
    }
    if let Some(series) = try_collect_arrays(episode, false) {
        if !series.is_empty() {
            return (series, "State");
        }
    }
    (Vec::new(), "")
}

/// Try to collect array data from all frames in an episode.
///
/// If `use_action` is true, collects action arrays; otherwise collects state arrays.
fn try_collect_arrays(
    episode: &forge_core::Episode,
    use_action: bool,
) -> Option<Vec<Vec<(f64, f64)>>> {
    let mut dim_count = 0usize;
    let mut all_values: Vec<Vec<(f64, f64)>> = Vec::new();

    for (frame_idx, f) in episode.frames.iter().enumerate() {
        let lazy = if use_action {
            f.action.as_ref()
        } else {
            f.state.as_ref()
        };

        let lazy = match lazy {
            Some(l) => l,
            None => continue,
        };

        let values = match lazy.load() {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Initialize dimension vectors on first successful load.
        if dim_count == 0 {
            dim_count = values.len();
            all_values = (0..dim_count).map(|_| Vec::new()).collect();
        }

        for (dim, &val) in values.iter().enumerate().take(dim_count) {
            if dim < all_values.len() {
                all_values[dim].push((frame_idx as f64, val));
            }
        }
    }

    if all_values.is_empty() {
        None
    } else {
        Some(all_values)
    }
}

/// Render the status bar at the bottom of the screen.
fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let episode_info = format!(
        "Episode {}/{} ({}) | Frame {}/{} | Camera: {}",
        app.current_episode + 1,
        app.episode_count(),
        app.current_episode_ref().episode_id,
        app.current_frame + 1,
        app.frame_count(),
        if app.camera_names.is_empty() {
            "(none)"
        } else {
            &app.camera_names[app.current_camera]
        },
    );

    let nav_help = "[<-/->] Episode  [Up/Dn] Frame  [Tab] Camera  [q] Quit";

    let status = Paragraph::new(vec![
        Line::from(Span::styled(
            episode_info,
            Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            nav_help,
            Style::default()
                .fg(Color::Gray)
                .bg(Color::DarkGray),
        )),
    ])
    .block(Block::default().borders(Borders::TOP));

    frame.render_widget(status, area);
}
