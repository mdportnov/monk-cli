use crossterm::event::{KeyCode, KeyEvent};
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
    tui::{
        app::{App, EditorState, HomeState, PickerState, PresetPickerState, Screen},
        view::{draw_header, fmt_short},
        theme::*,
    },
};

pub async fn handle_picker_key(app: &mut App, key: KeyEvent) {
    let Screen::ModePicker(picker) = &mut app.screen else { return };

    // Two-step delete: if a confirmation is in flight, intercept all keys
    // here so navigation can't bypass the prompt.
    if picker.confirm_delete.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                picker.confirm_delete = None;
                app.delete_current_mode().await;
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                picker.confirm_delete = None;
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.set_screen(Screen::Home(HomeState::default()));
        }
        KeyCode::Up | KeyCode::Char('k') => picker.move_up(),
        KeyCode::Down | KeyCode::Char('j') => picker.move_down(),
        KeyCode::Char('r') => app.refresh_picker().await,
        KeyCode::Enter | KeyCode::Char(' ') => app.open_confirm_from_picker().await,
        KeyCode::Char('n') => open_editor_new(app),
        KeyCode::Char('a') => open_preset_picker(app),
        KeyCode::Char('e') => app.open_editor_edit(),
        KeyCode::Char('d') => {
            if let Some(m) = picker.current() {
                picker.confirm_delete = Some(m.name.clone());
            }
        }
        _ => {}
    }
}

pub async fn handle_preset_picker_key(app: &mut App, key: KeyEvent) {
    let Screen::PresetPicker(state) = &mut app.screen else { return };
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.open_picker();
        }
        KeyCode::Up | KeyCode::Char('k') => state.move_up(),
        KeyCode::Down | KeyCode::Char('j') => state.move_down(),
        KeyCode::Enter | KeyCode::Char(' ') => app.instantiate_preset().await,
        _ => {}
    }
}

pub fn draw_picker(f: &mut Frame, app: &App, picker: &PickerState) {
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

pub fn draw_preset_picker(f: &mut Frame, app: &App, state: &PresetPickerState) {
    let area = f.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(3)])
        .split(area);

    draw_header(f, outer[0], app);

    let body = outer[1];
    let centered = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(60),
            Constraint::Percentage(20),
        ])
        .split(body);

    if let Some(err) = &state.error {
        let p = Paragraph::new(Span::styled(err.clone(), Style::default().fg(ALERT)))
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center)
            .block(picker_block(" presets "));
        f.render_widget(p, centered[1]);
    } else {
        let items: Vec<ListItem> = state
            .names
            .iter()
            .map(|name| {
                let label = crate::i18n::t!("onboarding.preset", preset = name);
                ListItem::new(Line::from(Span::styled(format!("  {}", label), Style::default().fg(TEXT))))
            })
            .collect();

        let list = List::new(items)
            .block(picker_block(" presets "))
            .highlight_style(Style::default().bg(Color::Rgb(35, 40, 55)).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");

        let mut list_state = ListState::default();
        list_state.select(Some(state.selected));
        f.render_stateful_widget(list, centered[1], &mut list_state);
    }

    let help = "↑/↓ select   ⏎ add   esc back";
    let footer = Paragraph::new(Span::styled(help, Style::default().fg(DIM)))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(DIM)));
    f.render_widget(footer, outer[2]);
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
                "press n for a fresh mode · a to start from a preset",
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            )),
        ];
        let p = Paragraph::new(lines).alignment(Alignment::Center).block(picker_block(" modes "));
        f.render_widget(p, area);
        return;
    }

    let items: Vec<ListItem> =
        picker.modes.iter().map(|m| ListItem::new(render_mode_row(m))).collect();

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

    let mut spans = vec![Span::styled("  ".to_string(), Style::default())];
    spans.push(Span::styled(icon.to_string(), Style::default().fg(icon_color)));
    spans.push(Span::raw(" "));
    spans.push(Span::styled(m.name.clone(), name_style));

    if m.is_default {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("★", Style::default().fg(Color::Rgb(220, 180, 90))));
    }

    let line_width = spans.iter().map(|s| s.content.chars().count()).sum::<usize>();
    let status_width = status_text.chars().count();
    let padding = 45_usize.saturating_sub(line_width).saturating_sub(status_width);

    if padding > 0 {
        spans.push(Span::raw(" ".repeat(padding)));
    }
    spans.push(Span::styled(status_text, Style::default().fg(DIM)));

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
    ("●", ACCENT, "ready".to_string())
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
    Line::from(Span::styled(s.to_string(), Style::default().fg(DIM).add_modifier(Modifier::ITALIC)))
}

fn fmt_limit(d: Option<Duration>) -> String {
    match d {
        Some(v) => fmt_short(v),
        None => "—".into(),
    }
}

fn picker_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(title.to_string(), Style::default().fg(ACCENT)))
}

fn draw_picker_footer(f: &mut Frame, area: Rect, picker: &PickerState) {
    let (help, color) = if let Some(name) = &picker.confirm_delete {
        (
            format!("delete `{name}`?   y  yes   n  cancel"),
            ALERT,
        )
    } else {
        (
            "↑/↓ select   ⏎ configure   n new   a preset   e edit   d delete   r refresh   esc back"
                .to_string(),
            DIM,
        )
    };
    let style = Style::default().fg(color);
    let style = if picker.confirm_delete.is_some() {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    };
    let p = Paragraph::new(Span::styled(help, style))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(DIM)));
    f.render_widget(p, area);
}

// Helper functions
fn open_editor_new(app: &mut App) {
    app.set_screen(Screen::ModeEditor(Box::new(EditorState::new_mode())));
}

fn open_preset_picker(app: &mut App) {
    app.set_screen(Screen::PresetPicker(PresetPickerState::default()));
}

