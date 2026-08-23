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
    // 当前会话的模型（provider/model@effort）；会话切换后缓存未就绪时显示 "-"
    let model_label = match &app.model {
        Some(m) => {
            let effort = m.reasoning_effort.map(|e| e.as_wire()).unwrap_or("-");
            format!("{}/{}@{}", m.provider, m.model, effort)
        }
        None => "-".to_string(),
    };
    let text = format!(
        " {} | model: {} | draw={} | mode={} | {}",
        mode,
        model_label,
        app.draw_count,
        { if app.waiting { "WAIT" } else { "IDLE" } },
        app.status
    );
    let para = Paragraph::new(Text::raw(text)).style(Style::default().fg(Color::Gray));
    f.render_widget(para, area);
}
