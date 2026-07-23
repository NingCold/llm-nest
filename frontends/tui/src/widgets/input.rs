use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::{Color, Style},
    widgets::{Block, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, InputMode};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let input = Paragraph::new(app.input.as_str())
        .block(
            Block::bordered()
                .title(" Input ")
                .border_style(match app.input_mode {
                    InputMode::Insert => Style::default().fg(Color::Yellow),
                    InputMode::Normal => Style::default().fg(Color::DarkGray),
                }),
        )
        .style(match app.input_mode {
            InputMode::Insert => Style::default().fg(Color::White),
            InputMode::Normal => Style::default().fg(Color::DarkGray),
        });

    f.render_widget(input, area);

    if app.input_mode == InputMode::Insert {
        let prefix = &app.input[..app.cursor];
        let cursor_width = prefix.width();
        f.set_cursor_position(Position::new(area.x + cursor_width as u16 + 1, area.y + 1));
    }
}
