use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui::app::App;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(3)])
        .split(area);

    let mut title_spans = vec![
        Span::styled("monk", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Span::raw("  silence, discipline, flow"),
    ];
    if app.hard_mode.is_some() {
        title_spans.push(Span::raw("  "));
        title_spans.push(Span::styled(
            "HARD MODE",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }
    let title = Paragraph::new(Line::from(title_spans))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(title, chunks[0]);

    let body = match &app.active {
        Some(s) => {
            let remaining = s.remaining();
            let mins = remaining.as_secs() / 60;
            let secs = remaining.as_secs() % 60;
            format!("{}\n\n{:02}:{:02}", s.profile, mins, secs)
        }
        None => "no active session".to_string(),
    };
    let p = Paragraph::new(body)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    f.render_widget(p, chunks[1]);

    let help_text = if app.hard_mode.is_some() {
        "q quit  •  stop disabled (hard mode) — use `monk panic`"
    } else {
        "q quit  •  s start  •  x stop"
    };
    let help = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(help, chunks[2]);
}
