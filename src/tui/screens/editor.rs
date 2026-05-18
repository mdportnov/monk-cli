use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::time::Duration;

use crate::{
    config::{Hooks, Limits, Profile},
    tui::{
        app::{App, EditorField, Screen},
        theme::*,
        view::{draw_header, picker_block},
        widgets::{MultiSelectItem, MultiSelectList, TextInput},
    },
};

#[derive(Debug)]
pub struct EditorState {
    pub original_name: Option<String>,
    pub name: TextInput,
    pub color_idx: usize,
    pub max: TextInput,
    pub min: TextInput,
    pub cooldown: TextInput,
    pub daily_cap: TextInput,
    pub panic_delay: TextInput,
    pub schedule: TextInput,
    pub sites: TextInput,
    pub hook_before: TextInput,
    pub hook_after: TextInput,
    pub apps: MultiSelectList,
    pub groups: MultiSelectList,
    pub brands: MultiSelectList,
    pub focus: EditorField,
    pub snapshot: Profile,
    pub error: Option<String>,
    /// The field that triggered `error`, if any. Used to draw a red label.
    pub error_field: Option<EditorField>,
    pub confirm_cancel: bool,
    /// When Some, the inline "add custom app" modal is open with this input.
    /// Enter commits, Esc discards.
    pub add_custom_app: Option<TextInput>,
    /// Inline error from the add-custom-app modal (e.g. invalid bundle id).
    pub add_custom_error: Option<String>,
}

pub const COLOR_PALETTE: &[(&str, &str)] = &[
    ("none", "—"),
    ("blue", "blue"),
    ("cyan", "cyan"),
    ("green", "green"),
    ("amber", "amber"),
    ("violet", "violet"),
    ("red", "red"),
];

pub fn palette_index(color: &Option<String>) -> usize {
    let key = color.as_deref().unwrap_or("none");
    COLOR_PALETTE.iter().position(|(k, _)| *k == key).unwrap_or(0)
}

pub fn palette_value(idx: usize) -> Option<String> {
    let (k, _) = COLOR_PALETTE[idx % COLOR_PALETTE.len()];
    if k == "none" {
        None
    } else {
        Some(k.to_string())
    }
}

impl EditorState {
    pub fn new_mode() -> Self {
        let (apps, groups, brands) = load_picklists(&Profile::default());
        let mut s = Self {
            original_name: None,
            name: TextInput::new(""),
            color_idx: 0,
            max: TextInput::new(""),
            min: TextInput::new(""),
            cooldown: TextInput::new(""),
            daily_cap: TextInput::new(""),
            panic_delay: TextInput::new(""),
            schedule: TextInput::new(""),
            sites: TextInput::new(""),
            hook_before: TextInput::new(""),
            hook_after: TextInput::new(""),
            apps,
            groups,
            brands,
            focus: EditorField::Name,
            snapshot: Profile::default(),
            error: None,
            error_field: None,
            confirm_cancel: false,
            add_custom_app: None,
            add_custom_error: None,
        };
        s.sync_focus();
        s
    }

    pub fn edit(name: String, profile: Profile) -> Self {
        let (apps, groups, brands) = load_picklists(&profile);
        let limits = profile.limits.clone();
        let color_idx = palette_index(&profile.color);
        let mut s = Self {
            original_name: Some(name.clone()),
            name: TextInput::new(name),
            color_idx,
            max: TextInput::new(fmt_opt_humantime(limits.max_duration)),
            min: TextInput::new(fmt_opt_humantime(limits.min_duration)),
            cooldown: TextInput::new(fmt_opt_humantime(limits.cooldown)),
            daily_cap: TextInput::new(fmt_opt_humantime(limits.daily_cap)),
            panic_delay: TextInput::new(fmt_opt_humantime(limits.panic_delay)),
            schedule: TextInput::new(format_schedule_spec(profile.schedule.as_ref())),
            sites: TextInput::new(profile.sites.join(", ")),
            hook_before: TextInput::new(profile.hooks.before.join(" && ")),
            hook_after: TextInput::new(profile.hooks.after.join(" && ")),
            apps,
            groups,
            brands,
            focus: EditorField::Name,
            snapshot: profile,
            error: None,
            error_field: None,
            confirm_cancel: false,
            add_custom_app: None,
            add_custom_error: None,
        };
        s.sync_focus();
        s
    }

    pub fn input_mut(&mut self, f: EditorField) -> Option<&mut TextInput> {
        match f {
            EditorField::Name => Some(&mut self.name),
            EditorField::Max => Some(&mut self.max),
            EditorField::Min => Some(&mut self.min),
            EditorField::Cooldown => Some(&mut self.cooldown),
            EditorField::DailyCap => Some(&mut self.daily_cap),
            EditorField::PanicDelay => Some(&mut self.panic_delay),
            EditorField::Schedule => Some(&mut self.schedule),
            EditorField::Sites => Some(&mut self.sites),
            EditorField::HookBefore => Some(&mut self.hook_before),
            EditorField::HookAfter => Some(&mut self.hook_after),
            _ => None,
        }
    }

    pub fn next_field(&mut self) {
        self.clear_picker_filters();
        let idx = EditorField::ORDER.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = EditorField::ORDER[(idx + 1) % EditorField::ORDER.len()];
        self.sync_focus();
    }

    pub fn prev_field(&mut self) {
        self.clear_picker_filters();
        let idx = EditorField::ORDER.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus =
            EditorField::ORDER[(idx + EditorField::ORDER.len() - 1) % EditorField::ORDER.len()];
        self.sync_focus();
    }

    fn clear_picker_filters(&mut self) {
        self.apps.filter_clear();
        self.groups.filter_clear();
        self.brands.filter_clear();
    }

    fn sync_focus(&mut self) {
        let focus = self.focus;
        for f in EditorField::ORDER {
            let is = f == focus;
            if let Some(inp) = self.input_mut(f) {
                inp.focused = is;
            }
        }
    }

    pub fn build_profile(
        &self,
    ) -> std::result::Result<(String, Profile), (Option<EditorField>, String)> {
        let name = self.name.value.trim().to_string();
        if name.is_empty() {
            return Err((Some(EditorField::Name), "name is required".into()));
        }
        if name.chars().count() > 30 {
            return Err((Some(EditorField::Name), "name must be ≤ 30 chars".into()));
        }
        let max_duration =
            parse_opt_humantime(&self.max).map_err(|e| (Some(EditorField::Max), e))?;
        let min_duration =
            parse_opt_humantime(&self.min).map_err(|e| (Some(EditorField::Min), e))?;
        let cooldown =
            parse_opt_humantime(&self.cooldown).map_err(|e| (Some(EditorField::Cooldown), e))?;
        let daily_cap =
            parse_opt_humantime(&self.daily_cap).map_err(|e| (Some(EditorField::DailyCap), e))?;
        let panic_delay = parse_opt_humantime(&self.panic_delay)
            .map_err(|e| (Some(EditorField::PanicDelay), e))?;
        let limits = Limits { max_duration, min_duration, cooldown, daily_cap, panic_delay };
        if let (Some(mn), Some(mx)) = (limits.min_duration, limits.max_duration) {
            if mn > mx {
                return Err((Some(EditorField::Min), "min must be ≤ max".into()));
            }
        }
        let sites: Vec<String> = self
            .sites
            .value
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        let hooks = Hooks {
            before: split_cmds(&self.hook_before.value),
            after: split_cmds(&self.hook_after.value),
        };
        let schedule = parse_schedule_spec(&self.schedule.value)
            .map_err(|e| (Some(EditorField::Schedule), e.to_string()))?;
        let profile = Profile {
            sites,
            site_groups: self.groups.selected_ids(),
            brands: self.brands.selected_ids(),
            apps: self.apps.selected_ids(),
            allow: self.snapshot.allow.clone(),
            hooks,
            limits,
            color: palette_value(self.color_idx),
            schedule,
        };
        Ok((name, profile))
    }

    pub fn is_dirty(&self) -> bool {
        match self.build_profile() {
            Ok((name, p)) => {
                if self.original_name.as_deref() != Some(name.as_str())
                    && self.original_name.is_some()
                {
                    return true;
                }
                if self.original_name.is_none() {
                    return true;
                }
                !profile_eq(&p, &self.snapshot)
            }
            Err(_) => true,
        }
    }
}

pub async fn handle_editor_key(app: &mut App, key: KeyEvent) {
    let Screen::ModeEditor(ed) = &mut app.screen else { return };

    // Add-custom modal: while open, all keys go to it. The kind of thing
    // being added depends on which field the modal was triggered from
    // (Apps → bundle id, Sites → hostname).
    if ed.add_custom_app.is_some() {
        let key_code = key.code;
        let focus = ed.focus;
        // Borrow the input mutably only for handle/clone.
        let input = ed.add_custom_app.as_mut().unwrap();
        match key_code {
            KeyCode::Esc => {
                ed.add_custom_app = None;
                ed.add_custom_error = None;
            }
            KeyCode::Enter => {
                let raw = input.value.trim().to_string();
                match focus {
                    EditorField::Apps => {
                        if raw.is_empty() {
                            ed.add_custom_error = Some("bundle id is required".into());
                        } else if !looks_like_bundle_id(&raw) {
                            ed.add_custom_error =
                                Some("expected reverse-DNS form, e.g. com.example.app".into());
                        } else {
                            ed.apps.add_item_selected(MultiSelectItem::new(
                                raw.clone(),
                                format!("(custom) [{raw}]"),
                            ));
                            ed.add_custom_app = None;
                            ed.add_custom_error = None;
                        }
                    }
                    EditorField::Sites => match crate::sites::sanitize_host(&raw) {
                        Some(host) => {
                            let current = ed.sites.value.trim();
                            if current.is_empty() {
                                ed.sites.value = host;
                            } else {
                                ed.sites.value = format!("{current}, {host}");
                            }
                            ed.sites.cursor = ed.sites.value.chars().count();
                            ed.add_custom_app = None;
                            ed.add_custom_error = None;
                        }
                        None => {
                            ed.add_custom_error = Some(
                                "not a valid hostname (e.g. example.com or news.bbc.co.uk)".into(),
                            );
                        }
                    },
                    _ => {
                        ed.add_custom_app = None;
                        ed.add_custom_error = None;
                    }
                }
            }
            _ => {
                input.handle(key);
                ed.add_custom_error = None;
            }
        }
        return;
    }

    if ed.confirm_cancel {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.open_picker();
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                ed.confirm_cancel = false;
            }
            _ => {}
        }
        return;
    }

    // `+` on Apps or Sites field opens an inline "add custom" modal.
    // `+` is not a valid character in either bundle ids or hostnames, so
    // intercepting it before TextInput / MultiSelectList sees it is safe.
    if matches!(key.code, KeyCode::Char('+'))
        && matches!(ed.focus, EditorField::Apps | EditorField::Sites)
    {
        let mut input = TextInput::new("");
        input.focused = true;
        ed.add_custom_app = Some(input);
        ed.add_custom_error = None;
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('s') => {
                app.save_editor().await;
                return;
            }
            KeyCode::Enter => {
                app.save_editor_and_open_confirm().await;
                return;
            }
            _ => {}
        }
    }
    // ↑/↓ move between fields for everything that isn't a multi-select
    // (Apps/Groups/Brands swallow vertical arrows for their own list nav).
    let focus = ed.focus;
    let is_multiselect =
        matches!(focus, EditorField::Apps | EditorField::Groups | EditorField::Brands);
    if !is_multiselect {
        match key.code {
            KeyCode::Up => {
                ed.prev_field();
                return;
            }
            KeyCode::Down => {
                ed.next_field();
                return;
            }
            _ => {}
        }
    }
    match key.code {
        KeyCode::Esc => {
            let consumed = match ed.focus {
                EditorField::Apps => ed.apps.handle(key),
                EditorField::Groups => ed.groups.handle(key),
                EditorField::Brands => ed.brands.handle(key),
                _ => false,
            };
            if !consumed {
                if ed.is_dirty() {
                    ed.confirm_cancel = true;
                } else {
                    app.open_picker();
                }
            }
        }
        KeyCode::Tab => ed.next_field(),
        KeyCode::BackTab => ed.prev_field(),
        _ => {
            let focus = ed.focus;
            match focus {
                EditorField::Apps => {
                    ed.apps.handle(key);
                }
                EditorField::Groups => {
                    ed.groups.handle(key);
                }
                EditorField::Brands => {
                    ed.brands.handle(key);
                }
                EditorField::Color => {
                    let n = COLOR_PALETTE.len();
                    match key.code {
                        KeyCode::Left | KeyCode::Char('h') => {
                            ed.color_idx = (ed.color_idx + n - 1) % n;
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            ed.color_idx = (ed.color_idx + 1) % n;
                        }
                        _ => {}
                    }
                }
                _ => {
                    if let Some(input) = ed.input_mut(focus) {
                        input.handle(key);
                    }
                }
            }
            ed.error = None;
            ed.error_field = None;
        }
    }
}

pub fn draw_editor(f: &mut Frame, app: &App, editor: &EditorState) {
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

    let help = "↑/↓ · tab fields   ctrl+s save   ctrl+⏎ save & start   esc cancel";
    let footer = Paragraph::new(Span::styled(help, Style::default().fg(DIM)))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(DIM)));
    f.render_widget(footer, outer[2]);

    if editor.confirm_cancel {
        crate::tui::view::draw_confirm_modal(
            f,
            "discard changes?",
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "you have unsaved edits to this mode.",
                    Style::default().fg(TEXT),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "discard them and go back to the picker?",
                    Style::default().fg(DIM),
                )),
                Line::from(""),
            ],
            "discard",
            "keep editing",
            true,
        );
    }

    if let Some(input) = &editor.add_custom_app {
        let (title, hint) = match editor.focus {
            EditorField::Sites => {
                (" add custom site ", "hostname (e.g. example.com · news.bbc.co.uk)")
            }
            _ => (" add custom app ", "bundle id (e.g. com.example.app · proc.exe)"),
        };
        draw_add_custom_modal(f, title, hint, input, editor.add_custom_error.as_deref());
    }
}

fn draw_add_custom_modal(
    f: &mut Frame,
    title: &str,
    hint: &str,
    input: &TextInput,
    error: Option<&str>,
) {
    let area = f.area();
    let width = 56.min(area.width.saturating_sub(4));
    let height = 11u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let rect = Rect { x, y, width, height };

    f.render_widget(ratatui::widgets::Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            title.to_string(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let hint_para = Paragraph::new(Span::styled(hint.to_string(), Style::default().fg(DIM)))
        .alignment(Alignment::Center);
    f.render_widget(hint_para, layout[0]);

    input.render(layout[1], f.buffer_mut(), Style::default().fg(TEXT));

    if let Some(err) = error {
        let err_line = Paragraph::new(Span::styled(err.to_string(), Style::default().fg(ALERT)))
            .alignment(Alignment::Center);
        f.render_widget(err_line, layout[3]);
    }

    let actions = Paragraph::new(Span::styled("enter add · esc cancel", Style::default().fg(DIM)))
        .alignment(Alignment::Center);
    f.render_widget(actions, layout[4]);
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
        EditorField::Schedule,
        EditorField::Sites,
        EditorField::HookBefore,
        EditorField::HookAfter,
    ];
    let mut constraints: Vec<Constraint> =
        scalar_fields.iter().map(|_| Constraint::Length(2)).collect();
    constraints.push(Constraint::Length(3));
    let rows =
        Layout::default().direction(Direction::Vertical).constraints(constraints).split(inner);

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
        lines.push(Line::from(Span::styled("● unsaved changes", Style::default().fg(GLOW))));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), status_row);
}

fn draw_editor_field(f: &mut Frame, area: Rect, editor: &EditorState, field: EditorField) {
    let focused = editor.focus == field;
    let is_error = editor.error_field == Some(field);
    let label_style = if is_error {
        Style::default().fg(ALERT).add_modifier(Modifier::BOLD)
    } else if focused {
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

    f.render_widget(Paragraph::new(Line::from(Span::styled(label, label_style))), rows[0]);

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
        EditorField::PanicDelay => &editor.panic_delay,
        EditorField::Schedule => &editor.schedule,
        EditorField::Sites => &editor.sites,
        EditorField::HookBefore => &editor.hook_before,
        EditorField::HookAfter => &editor.hook_after,
        _ => return,
    };
    let style = if focused { Style::default().fg(TEXT) } else { Style::default().fg(DIM) };
    let buf = f.buffer_mut();
    input.render(rows[1], buf, style);
}

fn draw_color_swatch(f: &mut Frame, area: Rect, editor: &EditorState, focused: bool) {
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
        .constraints([
            Constraint::Percentage(40),
            Constraint::Percentage(30),
            Constraint::Percentage(30),
        ])
        .split(area);

    let brands_title = picker_title("brand presets", &editor.brands.filter);
    let brands_focused = editor.focus == EditorField::Brands;
    editor.brands.render(chunks[0], f.buffer_mut(), picker_block(&brands_title), brands_focused);

    let apps_title = picker_title("blocked apps", &editor.apps.filter);
    let apps_focused = editor.focus == EditorField::Apps;
    editor.apps.render(chunks[1], f.buffer_mut(), picker_block(&apps_title), apps_focused);

    let groups_title = picker_title("site groups", &editor.groups.filter);
    let groups_focused = editor.focus == EditorField::Groups;
    editor.groups.render(chunks[2], f.buffer_mut(), picker_block(&groups_title), groups_focused);
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

fn picker_title(label: &str, filter: &str) -> String {
    if filter.is_empty() {
        format!(" {label} ")
    } else {
        format!(" {label} · /{filter} ")
    }
}

fn fmt_opt_humantime(d: Option<Duration>) -> String {
    match d {
        Some(v) => humantime::format_duration(v).to_string(),
        None => String::new(),
    }
}

fn parse_opt_humantime(input: &TextInput) -> std::result::Result<Option<Duration>, String> {
    let raw = input.value.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    humantime::parse_duration(raw).map(Some).map_err(|e| format!("invalid duration: {e}"))
}

/// Cheap sanity check for a macOS/Linux app identifier (reverse-DNS like
/// `com.spotify.client`). We don't reject Windows-exe basenames here on
/// purpose — those can have any shape; if the user knows what they're doing
/// they can put one in. We just want to reject empty / accidental input.
fn looks_like_bundle_id(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty()
        && !s.contains(char::is_whitespace)
        && (s.contains('.') || s.contains('-') || s.ends_with(".exe"))
}

fn split_cmds(raw: &str) -> Vec<String> {
    raw.split("&&").map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

fn profile_eq(a: &Profile, b: &Profile) -> bool {
    a.sites == b.sites
        && a.site_groups == b.site_groups
        && a.brands == b.brands
        && a.apps == b.apps
        && a.allow == b.allow
        && a.hooks.before == b.hooks.before
        && a.hooks.after == b.hooks.after
        && a.limits.max_duration == b.limits.max_duration
        && a.limits.min_duration == b.limits.min_duration
        && a.limits.cooldown == b.limits.cooldown
        && a.limits.daily_cap == b.limits.daily_cap
        && a.color == b.color
        && a.schedule == b.schedule
}

pub fn format_schedule_spec(s: Option<&crate::config::Schedule>) -> String {
    let Some(s) = s else { return String::new() };
    use crate::config::Weekday::*;
    let order = [Mon, Tue, Wed, Thu, Fri, Sat, Sun];
    let labels = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
    let days: String = order
        .iter()
        .zip(labels.iter())
        .filter(|(d, _)| s.days.contains(d))
        .map(|(_, l)| *l)
        .collect::<Vec<_>>()
        .join(",");
    let prefix = if s.enabled { "" } else { "off " };
    let tz = if s.tz == "local" { String::new() } else { format!(" {}", s.tz) };
    format!("{prefix}{days} {}-{}{tz}", s.start, s.end)
}

pub fn parse_schedule_spec(
    raw: &str,
) -> std::result::Result<Option<crate::config::Schedule>, String> {
    use crate::config::{Schedule, Weekday};
    let s = raw.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let mut tokens: Vec<&str> = s.split_whitespace().collect();
    let enabled = if tokens.first().is_some_and(|t| t.eq_ignore_ascii_case("off")) {
        tokens.remove(0);
        false
    } else {
        true
    };
    if tokens.len() < 2 {
        return Err("expected `<days> <start>-<end> [tz]`".into());
    }
    let days_tok = tokens.remove(0);
    let window = tokens.remove(0);
    let tz = tokens.first().map(|s| (*s).to_string()).unwrap_or_else(|| "local".into());

    let (start, end) =
        window.split_once('-').ok_or_else(|| "window must be HH:MM-HH:MM".to_string())?;
    let parse_day = |t: &str| -> std::result::Result<Weekday, String> {
        Ok(match t.to_ascii_lowercase().as_str() {
            "mon" | "mo" => Weekday::Mon,
            "tue" | "tu" => Weekday::Tue,
            "wed" | "we" => Weekday::Wed,
            "thu" | "th" => Weekday::Thu,
            "fri" | "fr" => Weekday::Fri,
            "sat" | "sa" => Weekday::Sat,
            "sun" | "su" => Weekday::Sun,
            other => return Err(format!("unknown day `{other}`")),
        })
    };
    let order = [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
        Weekday::Sun,
    ];
    let mut days: Vec<Weekday> = Vec::new();
    let lower = days_tok.to_ascii_lowercase();
    match lower.as_str() {
        "daily" | "everyday" | "all" => days.extend(order),
        "weekdays" | "workdays" => days.extend(&order[..5]),
        "weekends" => days.extend(&order[5..]),
        _ => {
            for part in lower.split(',') {
                if let Some((a, b)) = part.split_once('-') {
                    let from = parse_day(a)?;
                    let to = parse_day(b)?;
                    let i = order.iter().position(|d| *d == from).unwrap();
                    let j = order.iter().position(|d| *d == to).unwrap();
                    if i <= j {
                        days.extend(&order[i..=j]);
                    } else {
                        return Err("day range must be ascending".into());
                    }
                } else {
                    days.push(parse_day(part)?);
                }
            }
        }
    }
    days.sort_by_key(|d| order.iter().position(|x| x == d).unwrap());
    days.dedup();
    let sch = Schedule { enabled, days, start: start.into(), end: end.into(), tz };
    sch.validate().map_err(|e| e.to_string())?;
    Ok(Some(sch))
}

fn load_picklists(profile: &Profile) -> (MultiSelectList, MultiSelectList, MultiSelectList) {
    let mut apps_items: Vec<MultiSelectItem> = crate::apps::load_or_scan(false)
        .map(|cache| {
            cache
                .apps
                .into_iter()
                .map(|a| MultiSelectItem::new(a.id.clone(), format!("{} [{}]", a.label, a.id)))
                .collect()
        })
        .unwrap_or_default();
    // Preserve custom bundle ids from the saved profile that the cache
    // doesn't know about (added previously via the inline "+ add custom"
    // flow, or hand-edited in the toml). Otherwise saving would silently
    // drop them on the next edit.
    for id in &profile.apps {
        if !apps_items.iter().any(|i| i.id == *id) {
            apps_items.push(MultiSelectItem::new(id.clone(), format!("(custom) [{id}]")));
        }
    }
    let groups_items = crate::sites::all_groups()
        .map(|gs| {
            gs.into_iter()
                .map(|g| {
                    MultiSelectItem::new(
                        g.qualified(),
                        format!("{} ({} hosts)", g.qualified(), g.hosts.len()),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let brands_items = crate::brands::all_brands()
        .map(|bs| {
            bs.into_iter()
                .map(|b| {
                    MultiSelectItem::new(
                        b.qualified(),
                        format!(
                            "{} {} [{}]",
                            b.icon.as_deref().unwrap_or("●"),
                            b.name,
                            b.qualified()
                        ),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    (
        MultiSelectList::new(apps_items, &profile.apps),
        MultiSelectList::new(groups_items, &profile.site_groups),
        MultiSelectList::new(brands_items, &profile.brands),
    )
}
