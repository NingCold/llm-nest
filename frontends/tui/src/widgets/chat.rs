use chat::Role;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph},
};

use crate::app::{App, CachedLine};
use crate::widgets::util::thinking_frame;

pub fn render(f: &mut Frame, area: Rect, app: &mut App) {
    let content_width = area.width.saturating_sub(2);
    if app.render_cache.generation != app.layout_generation
        || app.render_cache.width != content_width
    {
        app.render_cache
            .rebuild(&app.messages, content_width, app.layout_generation);
    }

    let mut lines: Vec<Line> = Vec::with_capacity(app.render_cache.lines.len());

    for (idx, cl) in app.render_cache.lines.iter().enumerate() {
        match cl {
            CachedLine::Role(role) => {
                let style = match role {
                    Role::User => Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                    Role::Assistant => Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                    _ => Style::default().fg(Color::Gray),
                };
                let label = match role {
                    Role::User => "You:",
                    Role::Assistant => "Assistant:",
                    _ => "System:",
                };
                lines.push(Line::from(Span::styled(label, style)));
            }
            CachedLine::Content(text) => {
                if let Some(role_idx) = app.render_cache.last_assistant_line
                    && app.waiting
                    && idx == role_idx + 1
                {
                    let elapsed = app
                        .thinking_start
                        .map(|t| t.elapsed().as_secs_f64())
                        .unwrap_or(0.0);
                    lines.push(Line::from(Span::styled(
                        format!(
                            " {} Thinking \u{00b7} {:.1}s",
                            thinking_frame(app.thinking_tick),
                            elapsed
                        ),
                        Style::default().fg(Color::Gray),
                    )));
                } else {
                    lines.push(Line::from(Span::raw(text.clone())));
                }
            }
            CachedLine::Spacer => {
                lines.push(Line::from(""));
            }
        }
    }

    let total = app.render_cache.lines.len();
    let visible = (area.height.max(2) - 2) as usize;
    let max_scroll = total.saturating_sub(visible);
    if app.scroll > max_scroll {
        app.scroll = max_scroll;
    }
    let scroll_from_top = total.saturating_sub(visible + app.scroll);

    let para = Paragraph::new(Text::from(lines))
        .block(
            Block::bordered()
                .title(" Chat ")
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .scroll((scroll_from_top as u16, 0));

    f.render_widget(para, area);
}
