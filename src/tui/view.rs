use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use std::time::Duration;

use crate::{
    ipc::ModeSummary,
    tui::app::{App, HomeState, MenuItem, PickerState, Screen},
};

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
        Screen::ModePicker(picker) => draw_picker(f, app, picker),
    }
}

fn draw_picker(f: &mut Frame, app: &App, picker: &PickerState) {
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

    draw_picker_list(f, body[0], picker);
    draw_picker_details(f, body[1], picker);
    draw_picker_footer(f, outer[2], picker);
}

fn draw_picker_list(f: &mut Frame, area: Rect, picker: &PickerState) {
    if picker.loading {
        let p = Paragraph::new("loading modes…")
            .alignment(Alignment::Center)
            .block(picker_block(" modes "));
        f.render_widget(p, area);
        return;
    }
    if let Some(err) = &picker.error {
        let p = Paragraph::new(Span::styled(err.clone(), Style::default().fg(ALERT)))
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center)
            .block(picker_block(" modes "));
        f.render_widget(p, area);
        return;
    }
    if picker.modes.is_empty() {
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "no modes yet",
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "press n to create your first (soon)",
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            )),
        ];
        let p = Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(picker_block(" modes "));
        f.render_widget(p, area);
        return;
    }

    let items: Vec<ListItem> = picker
        .modes
        .iter()
        .map(|m| ListItem::new(render_mode_row(m)))
        .collect();

    let list = List::new(items)
        .block(picker_block(" modes "))
        .highlight_style(Style::default().bg(Color::Rgb(35, 40, 55)).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    let mut state = ListState::default();
    state.select(Some(picker.selected));
    f.render_stateful_widget(list, area, &mut state);
}

fn render_mode_row(m: &ModeSummary) -> Line<'static> {
    let (icon, icon_color, status_text) = status_signal(m);
    let name_style = if m.is_default {
        Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(TEXT)
    };
    let usage = format_usage(m);
    let mut spans = vec![
        Span::styled(format!(" {icon} "), Style::default().fg(icon_color)),
        Span::styled(format!("{:<16}", truncate(&m.name, 16)), name_style),
        Span::raw(" "),
        Span::styled(format!("{usage:<18}"), Style::default().fg(DIM)),
        Span::styled(format!(" {status_text}"), Style::default().fg(icon_color)),
    ];
    if m.is_default {
        spans.push(Span::styled(" ·default", Style::default().fg(ACCENT)));
    }
    Line::from(spans)
}

fn status_signal(m: &ModeSummary) -> (&'static str, Color, String) {
    if m.stats.cooldown_remaining.is_some() {
        return (
            "◐",
            Color::Rgb(210, 180, 90),
            format!("cool {}", fmt_short(m.stats.cooldown_remaining.unwrap_or_default())),
        );
    }
    if let (Some(cap), Some(rem)) = (m.limits.daily_cap, m.stats.daily_cap_remaining) {
        if rem.is_zero() && !cap.is_zero() {
            return ("◌", ALERT, "capped".to_string());
        }
    }
    ("●", Color::Rgb(120, 200, 140), "ready".to_string())
}

fn format_usage(m: &ModeSummary) -> String {
    match m.limits.daily_cap {
        Some(cap) => format!("{} / {}", fmt_short(m.stats.used_24h), fmt_short(cap)),
        None => {
            if m.stats.used_24h.is_zero() {
                "—".to_string()
            } else {
                format!("{} today", fmt_short(m.stats.used_24h))
            }
        }
    }
}

fn fmt_short(d: Duration) -> String {
    let secs = d.as_secs();
    if secs == 0 {
        return "0".into();
    }
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 && m > 0 {
        format!("{h}h{m:02}")
    } else if h > 0 {
        format!("{h}h")
    } else if m > 0 {
        format!("{m}m")
    } else {
        format!("{secs}s")
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

fn draw_picker_details(f: &mut Frame, area: Rect, picker: &PickerState) {
    let block = picker_block(" details ");
    let Some(m) = picker.current() else {
        f.render_widget(block, area);
        return;
    };

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            m.name.clone(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    lines.push(kv("apps", &m.blocked_apps.to_string()));
    lines.push(kv("sites", &m.blocked_sites.to_string()));
    lines.push(kv("groups", &m.blocked_groups.to_string()));
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled(
        "limits",
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )));
    lines.push(kv("max", &fmt_limit(m.limits.max_duration)));
    lines.push(dim_line("  ceiling — even you can't override"));
    lines.push(kv("min", &fmt_limit(m.limits.min_duration)));
    lines.push(dim_line("  shorter doesn't count as a session"));
    lines.push(kv("cooldown", &fmt_limit(m.limits.cooldown)));
    lines.push(dim_line("  protects against compulsive restart"));
    lines.push(kv("daily cap", &fmt_limit(m.limits.daily_cap)));
    lines.push(dim_line("  daily focus budget"));
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled(
        "today",
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )));
    lines.push(kv("used", &fmt_short(m.stats.used_24h)));
    if let Some(rem) = m.stats.cooldown_remaining {
        lines.push(kv("cool", &fmt_short(rem)));
    }
    if let Some(rem) = m.stats.daily_cap_remaining {
        lines.push(kv("left", &fmt_short(rem)));
    }

    let p = Paragraph::new(lines).wrap(Wrap { trim: false }).block(block);
    f.render_widget(p, area);
}

fn kv(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<10} "), Style::default().fg(DIM)),
        Span::styled(value.to_string(), Style::default().fg(TEXT)),
    ])
}

fn dim_line(s: &str) -> Line<'static> {
    Line::from(Span::styled(
        s.to_string(),
        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
    ))
}

fn fmt_limit(d: Option<Duration>) -> String {
    match d {
        Some(v) => fmt_short(v),
        None => "—".into(),
    }
}

fn picker_block(title: &'static str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(title, Style::default().fg(ACCENT)))
}

fn draw_picker_footer(f: &mut Frame, area: Rect, _picker: &PickerState) {
    let help = "↑/↓ select   r refresh   esc back";
    let p = Paragraph::new(Span::styled(help, Style::default().fg(DIM)))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(DIM)));
    f.render_widget(p, area);
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
