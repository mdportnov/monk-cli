use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::tui::app::{App, HomeState, MenuItem, Screen};

const ACCENT: Color = Color::Rgb(140, 180, 220);
const DIM: Color = Color::Rgb(120, 120, 130);
const TEXT: Color = Color::Rgb(210, 210, 215);
const ROBE: Color = Color::Rgb(170, 120, 90);
const SKIN: Color = Color::Rgb(210, 180, 140);
const GLOW: Color = Color::Rgb(190, 165, 110);
const ALERT: Color = Color::Rgb(200, 90, 90);

pub fn draw(f: &mut Frame, app: &App) {
    match &app.screen {
        Screen::Home(home) => draw_home(f, app, home),
    }
}

fn draw_home(f: &mut Frame, app: &App, home: &HomeState) {
    let area = f.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(3)])
        .split(area);

    draw_header(f, outer[0], app);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(outer[1]);

    draw_menu(f, body[0], app, home);
    draw_monk(f, body[1], app);
    draw_footer(f, outer[2], app);
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let mut spans = vec![
        Span::styled("monk", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled("  silence · discipline · flow", Style::default().fg(DIM)),
    ];
    if app.globals.hard_mode.is_some() {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(
            " HARD MODE ",
            Style::default().fg(Color::Black).bg(ALERT).add_modifier(Modifier::BOLD),
        ));
    }
    let p = Paragraph::new(Line::from(spans))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(DIM)));
    f.render_widget(p, area);
}

fn draw_menu(f: &mut Frame, area: Rect, app: &App, home: &HomeState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(5)])
        .split(area);

    let items: Vec<ListItem> = MenuItem::ALL
        .iter()
        .map(|m| {
            let disabled = matches!(m, MenuItem::Stop) && app.globals.hard_mode.is_some();
            let style = if disabled {
                Style::default().fg(DIM).add_modifier(Modifier::CROSSED_OUT)
            } else {
                Style::default().fg(TEXT)
            };
            ListItem::new(Line::from(Span::styled(format!("  {}", m.label()), style)))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(DIM))
                .title(Span::styled(" menu ", Style::default().fg(ACCENT))),
        )
        .highlight_style(
            Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    let mut state = ListState::default();
    state.select(Some(home.selected));
    f.render_stateful_widget(list, chunks[0], &mut state);

    let info_lines = build_info_lines(app, home);
    let info = Paragraph::new(info_lines)
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(DIM))
                .title(Span::styled(" status ", Style::default().fg(ACCENT))),
        );
    f.render_widget(info, chunks[1]);
}

fn build_info_lines(app: &App, home: &HomeState) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();

    let daemon = if app.globals.daemon_running {
        Span::styled("running", Style::default().fg(ACCENT))
    } else {
        Span::styled("stopped", Style::default().fg(ALERT))
    };
    lines.push(Line::from(vec![
        Span::styled("daemon  ", Style::default().fg(DIM)),
        daemon,
    ]));

    match &app.globals.active {
        Some(s) => {
            let remaining = s.remaining();
            let mins = remaining.as_secs() / 60;
            let secs = remaining.as_secs() % 60;
            lines.push(Line::from(vec![
                Span::styled("session ", Style::default().fg(DIM)),
                Span::styled(s.profile.clone(), Style::default().fg(TEXT)),
                Span::raw("  "),
                Span::styled(
                    format!("{mins:02}:{secs:02}"),
                    Style::default().fg(GLOW).add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        None => lines.push(Line::from(vec![
            Span::styled("session ", Style::default().fg(DIM)),
            Span::styled("idle", Style::default().fg(TEXT)),
        ])),
    }

    if let Some(msg) = &app.globals.flash {
        lines.push(Line::from(Span::styled(msg.clone(), Style::default().fg(GLOW))));
    } else {
        lines.push(Line::from(Span::styled(
            home.selected_item().hint(),
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )));
    }

    lines
}

fn draw_monk(f: &mut Frame, area: Rect, app: &App) {
    let frames = monk_frames();
    let idx = ((app.globals.frame / 4) as usize) % frames.len();
    let art = frames[idx].trim_matches('\n');

    let halo_on = (app.globals.frame / 2) % 2 == 0;
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

fn breath_label(frame: u64) -> &'static str {
    match (frame / 4) % 4 {
        0 => "breathe in …",
        1 => "hold …",
        2 => "breathe out …",
        _ => "rest …",
    }
}

fn monk_frames() -> [&'static str; 4] {
    [
        r#"
       . * .
      *     *
         ___
        /   \
       | -.- |
        \___/
       __|_|__
      /#######\
     /## \_/ ##\
    |###########|
     \#########/
       ~~~~~~~
"#,
        r#"
      .  *  .
     *       *
         ___
        /   \
       | -.- |
        \___/
        _|_|_
       /#####\
      /## \_/ ##\
     |###########|
      \#########/
        ~~~~~
"#,
        r#"
      . * * .
     *       *
         ___
        /   \
       | -.- |
        \___/
       __|_|__
      /#######\
     /## \_/ ##\
    |###########|
     \#########/
       ~~~~~~~
"#,
        r#"
     *   *   *
      *     *
         ___
        /   \
       | -.- |
        \___/
       __|_|__
      /#######\
     /## \_/ ##\
    |###########|
     \#########/
       ~~~~~~~
"#,
    ]
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let help = if app.globals.hard_mode.is_some() {
        "↑/↓ move   ⏎ select   q quit   ·   stop disabled — use panic"
    } else {
        "↑/↓ move   ⏎ select   s start   x stop   p panic   q quit"
    };
    let p = Paragraph::new(Span::styled(help, Style::default().fg(DIM)))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(DIM)));
    f.render_widget(p, area);
}
