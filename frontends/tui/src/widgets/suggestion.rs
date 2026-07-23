use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph},
};

use crate::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let prefix = app.input.trim().to_string();
    let matched: Vec<&str> = app
        .command_suggestions
        .iter()
        .filter(|cmd| cmd.starts_with(&prefix))
        .copied()
        .collect();

    if matched.is_empty() {
        return;
    }

    let items: Vec<Line> = matched
        .iter()
        .map(|cmd| Line::from(Span::styled(*cmd, Style::default().fg(Color::Yellow))))
        .collect();

    let height = matched.len() as u16 + 2;
    let sug_area = Rect::new(area.x, area.y, area.width.min(32), height.min(area.height));
    let para = Paragraph::new(Text::from(items))
        .block(
            Block::bordered()
                .title(" Commands ")
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .style(Style::default().bg(Color::Black));

    f.render_widget(para, sug_area);
}
