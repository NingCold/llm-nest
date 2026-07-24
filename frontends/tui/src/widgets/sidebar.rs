use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, List, ListItem},
};

use crate::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .sessions
        .iter()
        .map(|(id, title)| {
            let marker = app
                .cur_session
                .as_ref()
                .map_or(" ", |c| if c == id { "\u{25b8} " } else { "  " });
            let label = if title.is_empty() {
                format!("{}{}", marker, &id.to_string()[..8])
            } else {
                format!("{}{}", marker, title)
            };
            ListItem::new(label)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::bordered()
                .title(" Sessions ")
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_widget(list, area);
}
