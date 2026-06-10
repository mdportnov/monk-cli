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
    onboarding::{preset_blurb, preset_label, preset_meta, PresetTier, PRESETS},
    tui::{
        app::{App, EditorState, FlashLevel, HomeState, PickerState, PresetPickerState, Screen},
        theme::*,
        view::{draw_header, fmt_short},
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
        KeyCode::Esc => {
            if !picker.filter.is_empty() {
                picker.clear_filter();
            } else {
                app.set_screen(Screen::Home(HomeState::default()));
            }
        }
        KeyCode::Char('q') => {
            app.set_screen(Screen::Home(HomeState::default()));
        }
        KeyCode::Up | KeyCode::Char('k') => picker.move_up(),
        KeyCode::Down | KeyCode::Char('j') => picker.move_down(),
        KeyCode::Char('r') => app.refresh_picker().await,
        KeyCode::Enter => app.open_confirm_from_picker().await,
        KeyCode::Char(' ') => app.open_confirm_from_picker().await,
        KeyCode::Char('n') => open_editor_new(app),
        KeyCode::Char('a') => open_preset_picker(app),
        KeyCode::Char('e') => app.open_editor_edit(),
        KeyCode::Char('d') => {
            let running = matches!(
                (&picker.current(), &app.globals.active),
                (Some(m), Some(s)) if s.profile == m.name
            );
            if running {
                app.globals.set_flash(
                    crate::i18n::t!("tui.flash.delete_running_blocked").to_string(),
                    FlashLevel::Warn,
                );
            } else if let Some(m) = picker.current() {
                picker.confirm_delete = Some(m.name.clone());
            }
        }
        KeyCode::Char('c') => app.duplicate_current_mode().await,
        KeyCode::Char(c @ '1'..='9') => {
            let idx = (c as u8 - b'1') as usize;
            app.quick_start(idx).await;
        }
        KeyCode::Backspace => {
            if !picker.filter.is_empty() {
                picker.filter.pop();
                picker.selected = 0;
            }
        }
        KeyCode::Char(c) if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' => {
            picker.filter.push(c);
            picker.selected = 0;
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
        KeyCode::Enter => app.instantiate_preset().await,
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

    let active = app.globals.active.as_ref().map(|s| (s.profile.clone(), s.remaining()));
    draw_picker_list(f, body[0], picker, active.as_ref());
    draw_picker_details(f, body[1], picker);
    let selected_is_running = matches!(
        (picker.current(), active.as_ref()),
        (Some(m), Some((name, _))) if &m.name == name
    );
    draw_picker_footer(f, outer[2], picker, selected_is_running);

    if let Some(name) = &picker.confirm_delete {
        crate::tui::view::draw_confirm_modal(
            f,
            "delete mode?",
            vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("about to remove  ", Style::default().fg(DIM)),
                    Span::styled(
                        format!("`{name}`"),
                        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(""),
                Line::from(Span::styled("this can't be undone.", Style::default().fg(DIM))),
                Line::from(""),
            ],
            "delete",
            "cancel",
            true,
        );
    }
}

pub fn draw_preset_picker(f: &mut Frame, app: &App, state: &PresetPickerState) {
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

    draw_preset_list(f, body[0], state);
    draw_preset_preview(f, body[1], state);

    let help = "↑/↓ select   ⏎ add   esc back";
    let footer = Paragraph::new(Span::styled(help, Style::default().fg(DIM)))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(DIM)));
    f.render_widget(footer, outer[2]);
}

fn draw_preset_list(f: &mut Frame, area: Rect, state: &PresetPickerState) {
    if let Some(err) = &state.error {
        let p = Paragraph::new(Span::styled(err.clone(), Style::default().fg(ALERT)))
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center)
            .block(picker_block(" presets "));
        f.render_widget(p, area);
        return;
    }

    let mut items: Vec<ListItem> = Vec::new();
    let mut last_tier: Option<PresetTier> = None;
    let mut visual_selected: usize = 0;
    for (preset_idx, meta) in PRESETS.iter().enumerate() {
        if Some(meta.tier) != last_tier {
            let header = crate::i18n::lookup(meta.tier.label_key()).into_owned();
            items.push(ListItem::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!(" {}  {}", meta.tier.glyph(), header),
                    Style::default().fg(DIM).add_modifier(Modifier::BOLD),
                )),
            ]));
            last_tier = Some(meta.tier);
        }
        if preset_idx == state.selected {
            visual_selected = items.len();
        }
        let label = preset_label(meta.id);
        let blurb = preset_blurb(meta.id);
        let mut lines = vec![Line::from(Span::styled(
            format!("  {}", label),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ))];
        if !blurb.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("    {}", short_blurb(&blurb)),
                Style::default().fg(DIM),
            )));
        }
        items.push(ListItem::new(lines));
    }

    let list = List::new(items)
        .block(picker_block(" presets "))
        .highlight_style(Style::default().bg(Color::Rgb(35, 40, 55)).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    let mut list_state = ListState::default();
    list_state.select(Some(visual_selected));
    f.render_stateful_widget(list, area, &mut list_state);
}

fn short_blurb(full: &str) -> String {
    const MAX: usize = 70;
    if full.chars().count() <= MAX {
        return full.to_string();
    }
    let mut out: String = full.chars().take(MAX).collect();
    if let Some(idx) = out.rfind(' ') {
        out.truncate(idx);
    }
    out.push('…');
    out
}

fn draw_preset_preview(f: &mut Frame, area: Rect, state: &PresetPickerState) {
    let block = picker_block(" preview ");
    let Some(preset_name) = state.current() else {
        f.render_widget(block, area);
        return;
    };

    let Some(profile) = crate::onboarding::lookup_preset(preset_name) else {
        let p = Paragraph::new(Span::styled("failed to load preset", Style::default().fg(ALERT)))
            .alignment(Alignment::Center)
            .block(block);
        f.render_widget(p, area);
        return;
    };

    let label = preset_label(preset_name);
    let blurb = preset_blurb(preset_name);
    let tier = preset_meta(preset_name).map(|m| m.tier);

    let mut lines: Vec<Line> = Vec::new();

    let mut header_spans = vec![Span::styled(
        label.into_owned(),
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )];
    if let Some(t) = tier {
        let tier_label = crate::i18n::lookup(t.label_key()).into_owned();
        header_spans.push(Span::raw("  "));
        header_spans.push(Span::styled(
            format!("[{} {}]", t.glyph(), tier_label),
            Style::default().fg(DIM),
        ));
    }
    lines.push(Line::from(header_spans));

    if !blurb.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(blurb.into_owned(), Style::default().fg(TEXT))));
    }

    // Blocked groups in human form
    if !profile.site_groups.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "blocks",
            Style::default().fg(DIM).add_modifier(Modifier::BOLD),
        )));

        let categories: Vec<String> = group_categories(&profile.site_groups);
        let mut total_hosts = 0usize;
        if let Ok(all) = crate::sites::all_groups() {
            for g in &all {
                if profile.site_groups.iter().any(|q| q == &g.qualified()) {
                    total_hosts += g.hosts.len();
                }
            }
        }
        let joined = categories.join(", ");
        lines.push(Line::from(Span::styled(format!("  {}", joined), Style::default().fg(TEXT))));
        lines.push(Line::from(Span::styled(
            format!("  {} groups · {} hosts", profile.site_groups.len(), total_hosts),
            Style::default().fg(DIM),
        )));
    }

    // Limits in human form
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "limits",
        Style::default().fg(DIM).add_modifier(Modifier::BOLD),
    )));
    lines.push(kv("  max", &fmt_limit(profile.limits.max_duration)));
    lines.push(kv("  min", &fmt_limit(profile.limits.min_duration)));
    lines.push(kv("  cooldown", &fmt_limit(profile.limits.cooldown)));
    lines.push(kv("  day cap", &fmt_limit(profile.limits.daily_cap)));
    if let Some(pd) = profile.limits.panic_delay {
        lines.push(kv("  panic", &fmt_short(pd)));
    }

    // Custom sites (when used by a user-modified preset)
    if !profile.sites.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "extra sites",
            Style::default().fg(DIM).add_modifier(Modifier::BOLD),
        )));
        for site in profile.sites.iter().take(3) {
            lines.push(Line::from(Span::styled(format!("  {}", site), Style::default().fg(DIM))));
        }
        if profile.sites.len() > 3 {
            lines.push(Line::from(Span::styled(
                format!("  … {} more", profile.sites.len() - 3),
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "press enter to instantiate",
        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
    )));

    let p = Paragraph::new(lines).wrap(Wrap { trim: false }).block(block);
    f.render_widget(p, area);
}

fn group_categories(qualified: &[String]) -> Vec<String> {
    use std::collections::BTreeSet;
    let mut cats: BTreeSet<String> = BTreeSet::new();
    for q in qualified {
        if let Some((_, id)) = q.split_once('.') {
            cats.insert(id.to_string());
        }
    }
    cats.into_iter().collect()
}

fn draw_picker_list(
    f: &mut Frame,
    area: Rect,
    picker: &PickerState,
    active: Option<&(String, Duration)>,
) {
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

    // Show filter status in title
    let title = if picker.filter.is_empty() {
        " modes ".to_string()
    } else {
        format!(" modes · filter: {} ", picker.filter)
    };

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
        let p = Paragraph::new(lines).alignment(Alignment::Center).block(picker_block(&title));
        f.render_widget(p, area);
        return;
    }

    let filtered_modes = picker.filtered_modes();

    if filtered_modes.is_empty() {
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "no matches",
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "press esc to clear filter · backspace to edit",
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            )),
        ];
        let p = Paragraph::new(lines).alignment(Alignment::Center).block(picker_block(&title));
        f.render_widget(p, area);
        return;
    }

    let items: Vec<ListItem> = filtered_modes
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let running_remaining =
                active.and_then(|(name, rem)| if *name == m.name { Some(*rem) } else { None });
            let slot = if i < 9 { Some(i + 1) } else { None };
            ListItem::new(render_mode_row(m, running_remaining, slot))
        })
        .collect();

    let list = List::new(items)
        .block(picker_block(&title))
        .highlight_style(Style::default().bg(Color::Rgb(35, 40, 55)).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    let mut state = ListState::default();
    state.select(Some(picker.selected));
    f.render_stateful_widget(list, area, &mut state);
}

fn render_mode_row(
    m: &ModeSummary,
    running_remaining: Option<Duration>,
    slot: Option<usize>,
) -> Line<'static> {
    let (icon, icon_color, status_text) = status_signal(m, running_remaining);
    let name_style = if m.is_default {
        Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(TEXT)
    };

    let slot_text = match slot {
        Some(n) => format!("{n} "),
        None => "  ".to_string(),
    };
    let mut spans = vec![Span::styled(slot_text, Style::default().fg(DIM))];
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

fn status_signal(
    m: &ModeSummary,
    running_remaining: Option<Duration>,
) -> (&'static str, Color, String) {
    if let Some(rem) = running_remaining {
        return ("●", GLOW, format!("running · {} left", fmt_short(rem)));
    }
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

fn draw_picker_footer(f: &mut Frame, area: Rect, picker: &PickerState, selected_is_running: bool) {
    let help = if !picker.filter.is_empty() {
        "↑/↓ select · ⏎ start · type to filter · backspace edit · esc clear filter"
    } else if selected_is_running {
        "↑/↓ · ⏎ view · type filter · 1..9 quick · n new · a preset · e edit · c copy · r refresh · esc back"
    } else {
        "↑/↓ · ⏎ start · type filter · 1..9 quick · n new · a preset · e edit · c copy · d del · r refresh · esc"
    };
    let p = Paragraph::new(Span::styled(help, Style::default().fg(DIM)))
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
