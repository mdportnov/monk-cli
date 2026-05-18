use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::tui::{app::App, theme::*};

pub fn draw_monk(f: &mut Frame, area: Rect, app: &App) {
    let frames = monk_frames();
    let idx = ((app.globals.frame / 6) as usize) % frames.len();
    let art = frames[idx].trim_matches('\n');

    let halo_on = (app.globals.frame / 3) % 2 == 0;
    let total_rows = art.lines().count();

    let mut lines: Vec<Line> = Vec::new();
    for (row, line) in art.lines().enumerate() {
        let is_halo_row = row < 2;
        let is_ground_row = row + 1 == total_rows;
        let is_head_row = (2..=5).contains(&row);

        let mut spans: Vec<Span> = Vec::new();
        for ch in line.chars() {
            let style = match ch {
                '*' if halo_on => Style::default().fg(GLOW).add_modifier(Modifier::BOLD),
                '*' => Style::default().fg(DIM),
                '~' => Style::default().fg(ACCENT),
                '#' => Style::default().fg(ROBE),
                '-' | '.' if is_head_row => Style::default().fg(DIM),
                _ if is_halo_row => Style::default().fg(GLOW),
                _ if is_ground_row => Style::default().fg(ACCENT),
                _ if is_head_row => Style::default().fg(SKIN),
                _ => Style::default().fg(ROBE),
            };
            spans.push(Span::styled(ch.to_string(), style));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        breath_label(app.globals.frame),
        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(" companion ", Style::default().fg(ACCENT)));

    let p = Paragraph::new(lines).alignment(Alignment::Center).block(block);
    f.render_widget(p, area);
}

pub fn breath_label(frame: u64) -> &'static str {
    match frame % 75 {
        0..=24 => "breathe in …",
        25..=34 => "hold …",
        35..=64 => "breathe out …",
        _ => "rest …",
    }
}

pub fn monk_frames() -> [&'static str; 4] {
    // Each line starts at column 0 so Paragraph::Center can align by content
    // width alone — leading whitespace would shift the figure rightward.
    [
        "\n\
·   *   ·\n\
*         *\n\
___\n\
/   \\\n\
( -.- )\n\
\\___/\n\
|_|\n\
__|_|__\n\
/       \\\n\
|   ___   |\n\
|  /   \\  |\n\
|  \\___/  |\n\
\\       /\n\
/_________\\\n\
(___________)\n\
~~~~~~~\n",
        "\n\
*    ·    *\n\
 ·       ·\n\
___\n\
/   \\\n\
( -.- )\n\
\\___/\n\
|_|\n\
__|_|__\n\
/       \\\n\
|   ___   |\n\
|  /   \\  |\n\
|  \\___/  |\n\
\\       /\n\
/_________\\\n\
(___________)\n\
~~~~~~~\n",
        "\n\
 *   ·   *\n\
·         ·\n\
___\n\
/   \\\n\
( -.- )\n\
\\___/\n\
|_|\n\
__|_|__\n\
/       \\\n\
|   ___   |\n\
|  /   \\  |\n\
|  \\___/  |\n\
\\       /\n\
/_________\\\n\
(___________)\n\
~~~~~~~\n",
        "\n\
·    *    ·\n\
 *       *\n\
___\n\
/   \\\n\
( -.- )\n\
\\___/\n\
|_|\n\
__|_|__\n\
/       \\\n\
|   ___   |\n\
|  /   \\  |\n\
|  \\___/  |\n\
\\       /\n\
/_________\\\n\
(___________)\n\
~~~~~~~\n",
    ]
}
