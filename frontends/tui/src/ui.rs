use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};

use crate::app::App;
use crate::widgets;

pub fn ui(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);

    let middle = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Min(1)])
        .split(chunks[0]);

    widgets::sidebar::render(f, middle[0], app);
    widgets::chat::render(f, middle[1], app);
    widgets::input::render(f, chunks[1], app);
    widgets::status::render(f, chunks[2], app);

    if app.show_help {
        widgets::help::render(f, area, app);
    }

    if app.show_suggestions {
        widgets::suggestion::render(f, chunks[1], app);
    }
}
