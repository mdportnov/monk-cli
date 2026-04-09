use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use std::time::Duration;

use tui_big_text::{BigText, PixelSize};

use crate::{
    ipc::ModeSummary,
    tui::app::{
        App, ConfirmState, EditorField, EditorState, HomeState, MenuItem, PickerState, Screen,
        SettingsField, SettingsState, LOCALES,
    },
};

const ACCENT: Color = Color::Rgb(140, 180, 220);
const DIM: Color = Color::Rgb(120, 120, 130);
const TEXT: Color = Color::Rgb(210, 210, 215);
const ROBE: Color = Color::Rgb(170, 120, 90);
const SKIN: Color = Color::Rgb(210, 180, 140);
const GLOW: Color = Color::Rgb(190, 165, 110);
const ALERT: Color = Color::Rgb(200, 90, 90);

pub fn draw_with_effects(f: &mut Frame, app: &mut App, dt: std::time::Duration) {
    draw(f, app);
    if let Some(effect) = app.effect.as_mut() {
        use tachyonfx::Shader;
        if effect.running() {
            use tachyonfx::EffectRenderer;
            let area = f.area();
            f.render_effect(effect, area, tachyonfx::Duration::from_millis(dt.as_millis() as u32));
        } else {
            app.effect = None;
        }
    }
}

pub fn draw(f: &mut Frame, app: &App) {
    match &app.screen {
        Screen::Home(home) => draw_home(f, app, home),
        Screen::ModePicker(picker) => draw_picker(f, app, picker),
        Screen::ModeConfirm(confirm) => draw_confirm(f, app, confirm.as_ref()),
        Screen::ModeEditor(editor) => draw_editor(f, app, editor.as_ref()),
        Screen::Settings(st) => draw_settings(f, app, st.as_ref()),
    }
    if app.globals.help_open {
        draw_help_overlay(f, app);
    }
}

fn draw_help_overlay(f: &mut Frame, app: &App) {
    let area = f.area();
    let lines: Vec<Line> = match &app.screen {
        Screen::Home(_) => vec![
            Line::from(Span::styled("home", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from("  ↑/↓ · j/k    navigate menu"),
            Line::from("  enter        activate item"),
            Line::from("  s · x · p    start · stop · panic"),
            Line::from("  m            open modes picker"),
            Line::from("  1..9         quick-start mode by slot"),
            Line::from("  ?            toggle help"),
            Line::from("  q · esc      quit"),
        ],
        Screen::ModePicker(_) => vec![
            Line::from(Span::styled("modes", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from("  ↑/↓ · j/k    navigate"),
            Line::from("  enter        configure & start"),
            Line::from("  n · e · d    new · edit · delete"),
            Line::from("  r            refresh"),
            Line::from("  esc · q      back to home"),
        ],
        Screen::ModeConfirm(_) => vec![
            Line::from(Span::styled("confirm", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from("  ←/→ · h/l    adjust duration (5m steps)"),
            Line::from("  shift+h      toggle hard mode"),
            Line::from("  enter        start session"),
            Line::from("  esc · q      back to picker"),
        ],
        Screen::ModeEditor(_) => vec![
            Line::from(Span::styled("editor", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from("  tab/shift-tab   next / prev field"),
            Line::from("  ctrl+s          save"),
            Line::from("  space           toggle app/group"),
            Line::from("  esc             cancel"),
        ],
        Screen::Settings(_) => vec![
            Line::from(Span::styled("settings", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from("  tab/shift-tab   next / prev field"),
            Line::from("  space           toggle on/off"),
            Line::from("  ←/→             cycle locale"),
            Line::from("  ctrl+s          save"),
            Line::from("  esc             cancel"),
        ],
    };
    let width = 44.min(area.width.saturating_sub(4));
    let height = (lines.len() as u16 + 4).min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let rect = Rect { x, y, width, height };
    f.render_widget(ratatui::widgets::Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .title(" help ");
    let para = Paragraph::new(lines)
        .block(block)
        .style(Style::default().fg(TEXT))
        .wrap(Wrap { trim: false });
    f.render_widget(para, rect);
}

fn draw_editor(f: &mut Frame, app: &App, editor: &EditorState) {
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

    draw_editor_fields(f, body[0], editor);
    draw_editor_aux(f, body[1], editor);

    let help = if editor.confirm_cancel {
        "discard unsaved changes?   y  yes   n  keep editing"
    } else {
        "tab/shift-tab fields   ctrl+s save   esc cancel"
    };
    let footer = Paragraph::new(Span::styled(help, Style::default().fg(DIM)))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(DIM)));
    f.render_widget(footer, outer[2]);
}

fn draw_editor_fields(f: &mut Frame, area: Rect, editor: &EditorState) {
    let block = picker_block(" edit mode ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let scalar_fields = [
        EditorField::Name,
        EditorField::Color,
        EditorField::Max,
        EditorField::Min,
        EditorField::Cooldown,
        EditorField::DailyCap,
        EditorField::Sites,
        EditorField::HookBefore,
        EditorField::HookAfter,
    ];
    let mut constraints: Vec<Constraint> =
        scalar_fields.iter().map(|_| Constraint::Length(2)).collect();
    constraints.push(Constraint::Length(3));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    for (i, field) in scalar_fields.iter().enumerate() {
        draw_editor_field(f, rows[i], editor, *field);
    }

    let status_row = rows[scalar_fields.len()];
    let mut lines: Vec<Line> = Vec::new();
    if let Some(err) = &editor.error {
        lines.push(Line::from(Span::styled(
            err.clone(),
            Style::default().fg(ALERT).add_modifier(Modifier::BOLD),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            editor.focus.help(),
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )));
    }
    if editor.is_dirty() {
        lines.push(Line::from(Span::styled(
            "● unsaved changes",
            Style::default().fg(GLOW),
        )));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), status_row);
}

fn draw_editor_field(f: &mut Frame, area: Rect, editor: &EditorState, field: EditorField) {
    let focused = editor.focus == field;
    let label_style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DIM)
    };
    let prefix = if focused { "▶ " } else { "  " };
    let label = format!("{prefix}{:<14}", field.label());
    let rows = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(label.len() as u16 + 1), Constraint::Min(5)])
        .split(area);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(label, label_style))),
        rows[0],
    );

    if field == EditorField::Color {
        draw_color_swatch(f, rows[1], editor, focused);
        return;
    }
    let input = match field {
        EditorField::Name => &editor.name,
        EditorField::Max => &editor.max,
        EditorField::Min => &editor.min,
        EditorField::Cooldown => &editor.cooldown,
        EditorField::DailyCap => &editor.daily_cap,
        EditorField::Sites => &editor.sites,
        EditorField::HookBefore => &editor.hook_before,
        EditorField::HookAfter => &editor.hook_after,
        _ => return,
    };
    let style = if focused {
        Style::default().fg(TEXT)
    } else {
        Style::default().fg(DIM)
    };
    let buf = f.buffer_mut();
    input.render(rows[1], buf, style);
}

pub fn palette_color(key: &str) -> Color {
    match key {
        "blue" => Color::Rgb(110, 160, 230),
        "cyan" => Color::Rgb(90, 200, 210),
        "green" => Color::Rgb(120, 200, 130),
        "amber" => Color::Rgb(220, 180, 90),
        "violet" => Color::Rgb(180, 140, 220),
        "red" => Color::Rgb(220, 110, 110),
        _ => DIM,
    }
}

fn draw_color_swatch(f: &mut Frame, area: Rect, editor: &EditorState, focused: bool) {
    use crate::tui::app::COLOR_PALETTE;
    let mut spans: Vec<Span> = Vec::new();
    for (i, (key, label)) in COLOR_PALETTE.iter().enumerate() {
        let is_sel = i == editor.color_idx;
        let col = palette_color(key);
        let mut style = Style::default().fg(col);
        if is_sel {
            style = style.add_modifier(Modifier::BOLD | Modifier::REVERSED);
        } else if !focused {
            style = style.add_modifier(Modifier::DIM);
        }
        spans.push(Span::styled(format!(" {label} "), style));
        spans.push(Span::raw(" "));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_editor_aux(f: &mut Frame, area: Rect, editor: &EditorState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    let apps_block = picker_block(" blocked apps ");
    let apps_focused = editor.focus == EditorField::Apps;
    editor.apps.render(chunks[0], f.buffer_mut(), apps_block, apps_focused);

    let groups_block = picker_block(" site groups ");
    let groups_focused = editor.focus == EditorField::Groups;
    editor.groups.render(chunks[1], f.buffer_mut(), groups_block, groups_focused);
}

fn draw_confirm(f: &mut Frame, app: &App, confirm: &ConfirmState) {
    let area = f.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(3)])
        .split(area);

    draw_header(f, outer[0], app);

    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(8),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(4),
        ])
        .split(outer[1]);

    let title = Paragraph::new(Line::from(vec![
        Span::styled("start  ", Style::default().fg(DIM)),
        Span::styled(
            confirm.mode.name.clone(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
    ]))
    .alignment(Alignment::Center);
    f.render_widget(title, body[0]);

    let secs = confirm.duration.as_secs();
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    let timer_text = format!("{h:02}:{m:02}:{s:02}");
    let big = BigText::builder()
        .pixel_size(PixelSize::Quadrant)
        .style(Style::default().fg(if confirm.clamped { ALERT } else { GLOW }))
        .alignment(Alignment::Center)
        .lines(vec![Line::from(timer_text)])
        .build();
    f.render_widget(big, body[1]);

    draw_duration_slider(f, body[2], confirm);

    let hints = Paragraph::new(build_confirm_status(confirm))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(hints, body[3]);

    let details = draw_confirm_details(confirm);
    f.render_widget(details, body[4]);

    let help = if confirm.blocked_reason().is_some() {
        "←/→ duration   esc back   ·   start blocked"
    } else {
        "←/→ duration   shift+H hard   ⏎ start   esc back"
    };
    let footer = Paragraph::new(Span::styled(help, Style::default().fg(DIM)))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(DIM)));
    f.render_widget(footer, outer[2]);
}

fn draw_duration_slider(f: &mut Frame, area: Rect, confirm: &ConfirmState) {
    let width = area.width.saturating_sub(20).max(10) as usize;
    let frac = confirm.slider_fraction();
    let pos = ((width as f32) * frac).round() as usize;
    let mut bar = String::with_capacity(width);
    for i in 0..width {
        if i == pos {
            bar.push('●');
        } else {
            bar.push('─');
        }
    }
    let line = Line::from(vec![
        Span::styled(format!("  {:>5}  ", fmt_short(ConfirmState::MIN_BOUND)), Style::default().fg(DIM)),
        Span::styled(bar, Style::default().fg(ACCENT)),
        Span::styled(format!("  {:<5}", fmt_short(confirm.effective_max())), Style::default().fg(DIM)),
    ]);
    f.render_widget(Paragraph::new(line).alignment(Alignment::Center), area);
}

fn build_confirm_status(confirm: &ConfirmState) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();
    if confirm.clamped {
        lines.push(Line::from(Span::styled(
            format!("clamped to mode max ({})", fmt_short(confirm.effective_max())),
            Style::default().fg(ALERT).add_modifier(Modifier::BOLD),
        )));
    }
    if let Some(reason) = confirm.blocked_reason() {
        lines.push(Line::from(Span::styled(
            reason,
            Style::default().fg(ALERT).add_modifier(Modifier::BOLD),
        )));
    } else if let Some(err) = &confirm.error {
        lines.push(Line::from(Span::styled(
            err.clone(),
            Style::default().fg(ALERT),
        )));
    } else if confirm.hard {
        lines.push(Line::from(Span::styled(
            "hard mode — cannot stop early, panic phrase only",
            Style::default().fg(GLOW).add_modifier(Modifier::BOLD),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "soft mode — stop anytime",
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )));
    }
    lines
}

fn draw_confirm_details(confirm: &ConfirmState) -> Paragraph<'static> {
    let limits = confirm.limits();
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "contract",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        kv("max", &fmt_limit(limits.max_duration)),
        kv("min", &fmt_limit(limits.min_duration)),
        kv("cooldown", &fmt_limit(limits.cooldown)),
        kv("daily cap", &fmt_limit(limits.daily_cap)),
        Line::from(""),
        kv("used today", &fmt_short(confirm.mode.stats.used_24h)),
        kv(
            "blocks",
            &format!(
                "{} apps · {} sites · {} groups",
                confirm.mode.blocked_apps, confirm.mode.blocked_sites, confirm.mode.blocked_groups
            ),
        ),
    ];
    if let Some(rem) = confirm.mode.stats.daily_cap_remaining {
        lines.push(kv("budget left", &fmt_short(rem)));
    }
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(picker_block(" contract "))
}

pub fn fmt_short(d: Duration) -> String {
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
    if app.globals.active.is_some() {
        draw_session_card(f, body[1], app);
    } else {
        draw_monk(f, body[1], app);
    }
    draw_footer(f, outer[2], app);
}

fn draw_session_card(f: &mut Frame, area: Rect, app: &App) {
    let Some(session) = &app.globals.active else {
        draw_monk(f, area, app);
        return;
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(" session ", Style::default().fg(ACCENT)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(4),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Min(2),
        ])
        .split(inner);

    let title = Paragraph::new(Line::from(vec![
        Span::styled("mode  ", Style::default().fg(DIM)),
        Span::styled(
            session.profile.clone(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        if app.globals.hard_mode.is_some() {
            Span::styled("  HARD", Style::default().fg(ALERT).add_modifier(Modifier::BOLD))
        } else {
            Span::raw("")
        },
    ]))
    .alignment(Alignment::Center);
    f.render_widget(title, layout[0]);

    let remaining = session.remaining();
    let secs = remaining.as_secs();
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    let timer = format!("{h:02}:{m:02}:{s:02}");
    let big = BigText::builder()
        .pixel_size(PixelSize::Quadrant)
        .style(Style::default().fg(GLOW))
        .alignment(Alignment::Center)
        .lines(vec![Line::from(timer)])
        .build();
    f.render_widget(big, layout[1]);

    let total = session.duration.as_secs().max(1);
    let elapsed = total.saturating_sub(secs);
    let pct = ((elapsed as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as u16;
    let gauge = ratatui::widgets::Gauge::default()
        .gauge_style(Style::default().fg(ACCENT))
        .percent(pct)
        .label(Span::styled(
            format!("{pct}%"),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ));
    f.render_widget(gauge, layout[2]);

    let mut info_lines: Vec<Line> = Vec::new();
    if let Some(mode) = &app.globals.active_mode {
        if let Some(cap) = mode.limits.daily_cap {
            let used = mode.stats.used_24h;
            info_lines.push(Line::from(vec![
                Span::styled("today  ", Style::default().fg(DIM)),
                Span::styled(
                    format!("{} / {}", fmt_short(used), fmt_short(cap)),
                    Style::default().fg(TEXT),
                ),
            ]));
        }
        if let Some(cd) = mode.limits.cooldown {
            info_lines.push(Line::from(vec![
                Span::styled("cooldown  ", Style::default().fg(DIM)),
                Span::styled(fmt_short(cd), Style::default().fg(TEXT)),
            ]));
        }
    }
    if info_lines.is_empty() {
        info_lines.push(Line::from(Span::styled(
            "no limits configured",
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )));
    }
    let info = Paragraph::new(info_lines).alignment(Alignment::Center);
    f.render_widget(info, layout[3]);

    let action = if app.globals.hard_mode.is_some() {
        "p  panic — delayed release"
    } else {
        "x  stop   ·   p  panic"
    };
    let actions = Paragraph::new(Span::styled(
        action,
        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
    ))
    .alignment(Alignment::Center);
    f.render_widget(actions, layout[4]);
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

    if let Some(f) = &app.globals.flash {
        let color = match f.level {
            crate::tui::app::FlashLevel::Success => GLOW,
            crate::tui::app::FlashLevel::Warn => Color::Rgb(220, 180, 90),
            crate::tui::app::FlashLevel::Error => ALERT,
            crate::tui::app::FlashLevel::Info => ACCENT,
        };
        lines.push(Line::from(Span::styled(
            f.message.clone(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));
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

fn draw_settings(f: &mut Frame, app: &App, st: &SettingsState) {
    let area = f.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(3)])
        .split(area);

    draw_header(f, outer[0], app);

    let block = picker_block(" settings ");
    let inner = block.inner(outer[1]);
    f.render_widget(block, outer[1]);

    let fields = SettingsField::ORDER;
    let mut constraints: Vec<Constraint> =
        fields.iter().map(|_| Constraint::Length(2)).collect();
    constraints.push(Constraint::Length(3));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    for (i, field) in fields.iter().enumerate() {
        draw_settings_field(f, rows[i], st, *field);
    }

    let status_row = rows[fields.len()];
    let mut lines: Vec<Line> = Vec::new();
    if st.confirm_reset {
        lines.push(Line::from(Span::styled(
            "wipe config and audit log?   y  yes   n  cancel",
            Style::default().fg(ALERT).add_modifier(Modifier::BOLD),
        )));
    } else if let Some(err) = &st.error {
        lines.push(Line::from(Span::styled(
            err.clone(),
            Style::default().fg(ALERT).add_modifier(Modifier::BOLD),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            st.focus.help(),
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), status_row);

    let help = "tab/shift-tab fields   ctrl+s save   esc cancel";
    let footer = Paragraph::new(Span::styled(help, Style::default().fg(DIM)))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(DIM)));
    f.render_widget(footer, outer[2]);
}

fn draw_settings_field(f: &mut Frame, area: Rect, st: &SettingsState, field: SettingsField) {
    let focused = st.focus == field;
    let label_style = if field == SettingsField::Reset {
        if focused {
            Style::default().fg(ALERT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(ALERT)
        }
    } else if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DIM)
    };
    let prefix = if focused { "▶ " } else { "  " };
    let label = format!("{prefix}{:<22}", field.label());
    let rows = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(label.len() as u16 + 1), Constraint::Min(5)])
        .split(area);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(label, label_style))),
        rows[0],
    );

    let value_style = if focused {
        Style::default().fg(TEXT)
    } else {
        Style::default().fg(DIM)
    };

    match field {
        SettingsField::DefaultProfile => {
            st.default_profile.render(rows[1], f.buffer_mut(), value_style);
        }
        SettingsField::DefaultDuration => {
            st.default_duration.render(rows[1], f.buffer_mut(), value_style);
        }
        SettingsField::PanicDelay => {
            st.panic_delay.render(rows[1], f.buffer_mut(), value_style);
        }
        SettingsField::TamperPenalty => {
            st.tamper_penalty.render(rows[1], f.buffer_mut(), value_style);
        }
        SettingsField::HardMode => {
            let text = if st.hard_mode { "[x] on" } else { "[ ] off" };
            f.render_widget(Paragraph::new(Span::styled(text, value_style)), rows[1]);
        }
        SettingsField::Autostart => {
            let text = if st.autostart { "[x] on" } else { "[ ] off" };
            f.render_widget(Paragraph::new(Span::styled(text, value_style)), rows[1]);
        }
        SettingsField::Locale => {
            let mut spans: Vec<Span> = Vec::new();
            for (i, l) in LOCALES.iter().enumerate() {
                let mut style = value_style;
                if i == st.locale_idx {
                    style = style.add_modifier(Modifier::BOLD | Modifier::REVERSED);
                }
                spans.push(Span::styled(format!(" {l} "), style));
                spans.push(Span::raw(" "));
            }
            f.render_widget(Paragraph::new(Line::from(spans)), rows[1]);
        }
        SettingsField::Reset => {
            let style = if focused {
                Style::default().fg(ALERT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(ALERT)
            };
            f.render_widget(
                Paragraph::new(Span::styled("⟲ wipe all data", style)),
                rows[1],
            );
        }
    }
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

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use crate::audit::stats::ModeStats;
    use crate::config::Limits;
    use crate::ipc::ModeSummary;
    use crate::tui::app::{
        App, ConfirmState, EditorState, Flash, FlashLevel, Globals, HomeState, PickerState, Screen,
    };
    use ratatui::{backend::TestBackend, Terminal};

    fn render(app: &App, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn sample_mode(name: &str) -> ModeSummary {
        ModeSummary {
            name: name.into(),
            color: None,
            blocked_apps: 1,
            blocked_sites: 1,
            blocked_groups: 1,
            limits: Limits {
                max_duration: Some(Duration::from_secs(2 * 3600)),
                min_duration: Some(Duration::from_secs(15 * 60)),
                cooldown: Some(Duration::from_secs(30 * 60)),
                daily_cap: Some(Duration::from_secs(4 * 3600)),
            },
            stats: ModeStats {
                used_24h: Duration::from_secs(45 * 60),
                last_completed_at: None,
                cooldown_remaining: None,
                daily_cap_remaining: Some(Duration::from_secs(3 * 3600 + 15 * 60)),
            },
            is_default: true,
        }
    }

    fn base_app() -> App {
        let mut app = App::default();
        app.globals = Globals {
            daemon_running: true,
            frame: 0,
            cached_modes: vec![sample_mode("deepwork"), sample_mode("reading")],
            ..Default::default()
        };
        app
    }

    #[test]
    fn snapshot_home() {
        let mut app = base_app();
        app.screen = Screen::Home(HomeState::default());
        app.globals.flash = Some(Flash {
            message: "started `deepwork`".into(),
            level: FlashLevel::Success,
            expires_at: 100,
        });
        insta::assert_snapshot!(render(&app, 90, 28));
    }

    #[test]
    fn snapshot_home_help_overlay() {
        let mut app = base_app();
        app.screen = Screen::Home(HomeState::default());
        app.globals.help_open = true;
        insta::assert_snapshot!(render(&app, 90, 28));
    }

    #[test]
    fn snapshot_picker() {
        let mut app = base_app();
        let modes = app.globals.cached_modes.clone();
        app.screen = Screen::ModePicker(PickerState {
            modes,
            selected: 0,
            loading: false,
            error: None,
        });
        insta::assert_snapshot!(render(&app, 100, 30));
    }

    #[test]
    fn snapshot_confirm() {
        let mut app = base_app();
        let confirm = ConfirmState::from_mode(
            sample_mode("deepwork"),
            Duration::from_secs(50 * 60),
            false,
        );
        app.screen = Screen::ModeConfirm(Box::new(confirm));
        insta::assert_snapshot!(render(&app, 100, 30));
    }

    #[test]
    fn snapshot_editor_new() {
        let mut app = base_app();
        app.screen = Screen::ModeEditor(Box::new(EditorState::new_mode()));
        insta::assert_snapshot!(render(&app, 100, 30));
    }

    #[test]
    fn flash_levels_all_render() {
        for level in [FlashLevel::Info, FlashLevel::Success, FlashLevel::Warn, FlashLevel::Error] {
            let mut app = base_app();
            app.screen = Screen::Home(HomeState::default());
            app.globals.flash = Some(Flash {
                message: format!("{level:?} message"),
                level,
                expires_at: 100,
            });
            let _ = render(&app, 90, 24);
        }
    }
}
