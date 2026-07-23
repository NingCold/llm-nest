use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Paragraph},
};

use crate::widgets::util::centered_rect;

pub fn render(f: &mut Frame, area: Rect, _app: &crate::app::App) {
    let help_text = vec![
        Line::from(""),
        Line::from(" LLM Nest TUI - Help").style(Style::default().add_modifier(Modifier::BOLD)),
        Line::from(""),
        Line::from(" Mode Switching:"),
        Line::from("   i                     Enter Insert mode"),
        Line::from("   Esc                   Enter Normal mode"),
        Line::from(""),
        Line::from(" In Insert Mode:"),
        Line::from("   Enter                 Send message / execute command"),
        Line::from("   /new                  Create new session"),
        Line::from("   /switch <id|title>    Switch session"),
        Line::from("   /rename <title>       Rename current session"),
        Line::from("   /delete <id>          Delete session"),
        Line::from("   /list                 List sessions"),
        Line::from("   /help                 Toggle help"),
        Line::from("   /quit                 Quit"),
        Line::from(""),
        Line::from(" In Normal Mode:"),
        Line::from("   q                     Quit"),
        Line::from("   h                     Toggle help"),
        Line::from("   j / Down / ScrollDown  Scroll down"),
        Line::from("   k / Up / ScrollUp      Scroll up"),
        Line::from(""),
        Line::from(" Press h or /help to close"),
    ];

    let para = Paragraph::new(Text::from(help_text))
        .block(
            Block::bordered()
                .title(" Help ")
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Left);

    let a = centered_rect(area, 50, 60);
    f.render_widget(para, a);
}
