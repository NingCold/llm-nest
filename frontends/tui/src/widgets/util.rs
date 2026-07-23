use ratatui::layout::{Constraint, Layout, Rect};

pub fn centered_rect(r: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Length((r.height * (100 - percent_y)) / 200),
        Constraint::Length((r.height * percent_y) / 100),
        Constraint::Length((r.height * (100 - percent_y)) / 200),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Length((r.width * (100 - percent_x)) / 200),
        Constraint::Length((r.width * percent_x) / 100),
        Constraint::Length((r.width * (100 - percent_x)) / 200),
    ])
    .split(popup_layout[1])[1]
}

pub fn thinking_frame(idx: usize) -> String {
    let chars = [
        '\u{280B}', '\u{2819}', '\u{2839}', '\u{2838}', '\u{283C}', '\u{2834}', '\u{2826}',
        '\u{2827}', '\u{2807}', '\u{280F}',
    ];
    chars[idx % chars.len()].to_string()
}
