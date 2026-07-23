use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::Text,
    widgets::Paragraph,
};

use crate::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let mode = match app.input_mode {
        crate::app::InputMode::Normal => "NORMAL",
        crate::app::InputMode::Insert => "INSERT",
    };
    let text = format!(
        " {} | draw={} | mode={} | {}",
        mode,
        app.draw_count,
        { if app.waiting { "WAIT" } else { "IDLE" } },
        app.status
    );
    let para = Paragraph::new(Text::raw(text)).style(Style::default().fg(Color::Gray));
    f.render_widget(para, area);
}
