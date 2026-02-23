//! TUI rendering: camera frame display, timeseries charts, status bar.
//!
//! Stub implementation -- will be completed in Task 2.

use super::App;

/// Render the full TUI interface.
pub fn render_ui(frame: &mut ratatui::Frame, _app: &App) {
    let area = frame.area();
    let placeholder = ratatui::widgets::Paragraph::new("Loading viewer...");
    frame.render_widget(placeholder, area);
}
