use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::{
    config::{Config, Profile, Schedule, Weekday},
    daemon::scheduler,
    ipc::{self, Request, Response},
    tui::{
        app::{App, FlashLevel, HomeState, Screen},
        theme::*,
        view::{draw_footer, draw_header, fmt_short, picker_block},
        widgets::TextInput,
    },
};

const WEEKDAYS: [Weekday; 7] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
    Weekday::Sat,
    Weekday::Sun,
];
const DAY_LABELS: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

#[derive(Debug)]
pub struct ScheduleRow {
    pub name: String,
    pub profile: Profile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormField {
    Days,
    Start,
    End,
    Tz,
    Enabled,
}

impl FormField {
    const ORDER: [FormField; 5] =
        [FormField::Days, FormField::Start, FormField::End, FormField::Tz, FormField::Enabled];
}

#[derive(Debug)]
pub struct ScheduleForm {
    pub name: String,
    pub days: [bool; 7],
    pub day_cursor: usize,
    pub start: TextInput,
    pub end: TextInput,
    pub tz: TextInput,
    pub enabled: bool,
    pub focus: FormField,
    pub error: Option<String>,
}

impl ScheduleForm {
    pub fn from_profile(name: &str, profile: &Profile) -> Self {
        let (days, start, end, tz, enabled) = match &profile.schedule {
            Some(s) => {
                let mut d = [false; 7];
                for (i, wd) in WEEKDAYS.iter().enumerate() {
                    d[i] = s.days.contains(wd);
                }
                (d, s.start.clone(), s.end.clone(), s.tz.clone(), s.enabled)
            }
            None => (
                [true, true, true, true, true, false, false],
                "09:00".into(),
                "17:00".into(),
                "local".into(),
                true,
            ),
        };
        let mut form = Self {
            name: name.to_string(),
            days,
            day_cursor: 0,
            start: TextInput::new(start),
            end: TextInput::new(end),
            tz: TextInput::new(tz),
            enabled,
            focus: FormField::Days,
            error: None,
        };
        form.sync_focus();
        form
    }

    fn sync_focus(&mut self) {
        self.start.focused = self.focus == FormField::Start;
        self.end.focused = self.focus == FormField::End;
        self.tz.focused = self.focus == FormField::Tz;
    }

    fn next_field(&mut self) {
        let i = FormField::ORDER.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = FormField::ORDER[(i + 1) % FormField::ORDER.len()];
        self.sync_focus();
    }

    fn prev_field(&mut self) {
        let i = FormField::ORDER.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = FormField::ORDER[(i + FormField::ORDER.len() - 1) % FormField::ORDER.len()];
        self.sync_focus();
    }

    pub fn to_schedule(&self) -> Result<Schedule, String> {
        let days: Vec<Weekday> =
            WEEKDAYS.iter().zip(self.days.iter()).filter(|(_, on)| **on).map(|(d, _)| *d).collect();
        let tz = self.tz.value.trim();
        let sch = Schedule {
            enabled: self.enabled,
            days,
            start: self.start.value.trim().to_string(),
            end: self.end.value.trim().to_string(),
            tz: if tz.is_empty() { "local".into() } else { tz.to_string() },
        };
        sch.validate().map_err(|e| e.to_string())?;
        Ok(sch)
    }
}

#[derive(Debug, Default)]
pub struct SchedulesState {
    pub rows: Vec<ScheduleRow>,
    pub selected: usize,
    pub error: Option<String>,
    pub form: Option<ScheduleForm>,
}

impl SchedulesState {
    pub fn load() -> Self {
        match Config::load() {
            Ok(cfg) => {
                let now = chrono::Utc::now();
                let mut rows: Vec<ScheduleRow> = cfg
                    .profiles
                    .into_iter()
                    .map(|(name, profile)| ScheduleRow { name, profile })
                    .collect();
                // Scheduled-and-enabled first (soonest next window on top),
                // then disabled schedules, then profiles without one.
                rows.sort_by_key(|r| match &r.profile.schedule {
                    Some(s) if s.enabled => match scheduler::current_or_next(s, now) {
                        Some(w) => (0u8, w.start.timestamp(), r.name.clone()),
                        None => (1, 0, r.name.clone()),
                    },
                    Some(_) => (1, 0, r.name.clone()),
                    None => (2, 0, r.name.clone()),
                });
                Self { rows, ..Default::default() }
            }
            Err(e) => Self { error: Some(e.to_string()), ..Default::default() },
        }
    }

    pub fn move_up(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        self.selected = if self.selected == 0 { self.rows.len() - 1 } else { self.selected - 1 };
    }

    pub fn move_down(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.rows.len();
    }

    pub fn current(&self) -> Option<&ScheduleRow> {
        self.rows.get(self.selected)
    }
}

pub async fn handle_schedules_key(app: &mut App, key: KeyEvent) {
    let Screen::Schedules(state) = &mut app.screen else { return };

    if state.form.is_some() {
        handle_form_key(app, key).await;
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.set_screen(Screen::Home(HomeState::default()));
        }
        KeyCode::Up | KeyCode::Char('k') => state.move_up(),
        KeyCode::Down | KeyCode::Char('j') => state.move_down(),
        KeyCode::Char('r') => {
            let selected = state.selected;
            let mut fresh = SchedulesState::load();
            fresh.selected = selected.min(fresh.rows.len().saturating_sub(1));
            app.screen = Screen::Schedules(fresh);
        }
        KeyCode::Char(' ') => toggle_selected(app).await,
        KeyCode::Enter | KeyCode::Char('e') => {
            if let Some(row) = state.current() {
                state.form = Some(ScheduleForm::from_profile(&row.name, &row.profile));
            }
        }
        KeyCode::Char('d') => remove_selected(app).await,
        _ => {}
    }
}

async fn handle_form_key(app: &mut App, key: KeyEvent) {
    let Screen::Schedules(state) = &mut app.screen else { return };
    let Some(form) = &mut state.form else { return };

    // Save from any field with ctrl+s.
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('s')) {
        submit_form(app).await;
        return;
    }

    match key.code {
        KeyCode::Esc => {
            state.form = None;
        }
        KeyCode::Enter => {
            submit_form(app).await;
        }
        KeyCode::Tab | KeyCode::Down => form.next_field(),
        KeyCode::BackTab | KeyCode::Up => form.prev_field(),
        _ => match form.focus {
            FormField::Days => match key.code {
                KeyCode::Left | KeyCode::Char('h') => {
                    form.day_cursor = (form.day_cursor + 6) % 7;
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    form.day_cursor = (form.day_cursor + 1) % 7;
                }
                KeyCode::Char(' ') => {
                    form.days[form.day_cursor] = !form.days[form.day_cursor];
                }
                KeyCode::Char(c @ '1'..='7') => {
                    let i = (c as u8 - b'1') as usize;
                    form.days[i] = !form.days[i];
                }
                KeyCode::Char('w') => form.days = [true, true, true, true, true, false, false],
                KeyCode::Char('a') => form.days = [true; 7],
                KeyCode::Char('n') => form.days = [false, false, false, false, false, true, true],
                _ => {}
            },
            FormField::Enabled => {
                if matches!(key.code, KeyCode::Char(' ')) {
                    form.enabled = !form.enabled;
                }
            }
            FormField::Start => {
                form.start.handle(key);
            }
            FormField::End => {
                form.end.handle(key);
            }
            FormField::Tz => {
                form.tz.handle(key);
            }
        },
    }
}

async fn submit_form(app: &mut App) {
    let (name, schedule) = {
        let Screen::Schedules(state) = &mut app.screen else { return };
        let Some(form) = &mut state.form else { return };
        match form.to_schedule() {
            Ok(s) => (form.name.clone(), s),
            Err(e) => {
                form.error = Some(e);
                return;
            }
        }
    };
    save_schedule(app, &name, Some(schedule)).await;
}

async fn toggle_selected(app: &mut App) {
    let (name, schedule) = {
        let Screen::Schedules(state) = &app.screen else { return };
        let Some(row) = state.current() else { return };
        let Some(sch) = &row.profile.schedule else { return };
        let mut sch = sch.clone();
        sch.enabled = !sch.enabled;
        (row.name.clone(), sch)
    };
    save_schedule(app, &name, Some(schedule)).await;
}

async fn remove_selected(app: &mut App) {
    let name = {
        let Screen::Schedules(state) = &app.screen else { return };
        let Some(row) = state.current() else { return };
        if row.profile.schedule.is_none() {
            return;
        }
        row.name.clone()
    };
    save_schedule(app, &name, None).await;
}

/// Persist a schedule change for `name` via the daemon and reload the list.
async fn save_schedule(app: &mut App, name: &str, schedule: Option<Schedule>) {
    let profile = {
        let Screen::Schedules(state) = &app.screen else { return };
        let Some(row) = state.rows.iter().find(|r| r.name == name) else { return };
        let mut p = row.profile.clone();
        p.schedule = schedule;
        p
    };

    match ipc::send(&Request::SaveMode { name: name.to_string(), profile: Box::new(profile) }).await
    {
        Ok(Response::Ok) => {
            app.globals.set_flash(
                crate::i18n::t!("tui.schedules.saved", profile = name).to_string(),
                FlashLevel::Success,
            );
            app.kick_refresh();
            let mut fresh = SchedulesState::load();
            fresh.selected = fresh.rows.iter().position(|r| r.name == name).unwrap_or(0);
            app.screen = Screen::Schedules(fresh);
        }
        Ok(Response::Error { message }) => set_error(app, message),
        Ok(_) => {}
        Err(e) => set_error(app, e.to_string()),
    }
}

fn set_error(app: &mut App, message: String) {
    let Screen::Schedules(state) = &mut app.screen else { return };
    match &mut state.form {
        Some(form) => form.error = Some(message),
        None => state.error = Some(message),
    }
}

pub fn draw_schedules(f: &mut Frame, app: &App, state: &SchedulesState) {
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

    draw_list(f, body[0], state);
    match &state.form {
        Some(form) => draw_form(f, body[1], form),
        None => draw_details(f, body[1], state),
    }

    draw_footer(f, outer[2], app);
}

fn schedule_status(profile: &Profile, now: chrono::DateTime<chrono::Utc>) -> (String, Color) {
    let Some(sch) = &profile.schedule else {
        return (crate::i18n::t!("tui.schedules.none").to_string(), DIM);
    };
    if !sch.enabled {
        return (crate::i18n::t!("tui.schedules.disabled").to_string(), DIM);
    }
    match scheduler::current_or_next(sch, now) {
        Some(w) if w.contains(now) => {
            let until = w.end.with_timezone(&chrono::Local).format("%H:%M").to_string();
            (crate::i18n::t!("tui.schedules.active_until", time = until).to_string(), GLOW)
        }
        Some(w) => {
            let in_ = fmt_short((w.start - now).to_std().unwrap_or_default());
            (crate::i18n::t!("tui.schedules.next_in", remaining = in_).to_string(), ACCENT)
        }
        None => (crate::i18n::t!("tui.schedules.disabled").to_string(), DIM),
    }
}

fn draw_list(f: &mut Frame, area: Rect, state: &SchedulesState) {
    let title = format!(" {} ", crate::i18n::t!("tui.schedules.title"));
    if let Some(err) = &state.error {
        let p = Paragraph::new(Span::styled(err.clone(), Style::default().fg(ALERT)))
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center)
            .block(picker_block(&title));
        f.render_widget(p, area);
        return;
    }
    if state.rows.is_empty() {
        let p = Paragraph::new(Span::styled(
            crate::i18n::t!("tui.schedules.empty"),
            Style::default().fg(DIM),
        ))
        .alignment(Alignment::Center)
        .block(picker_block(&title));
        f.render_widget(p, area);
        return;
    }

    let now = chrono::Utc::now();
    let items: Vec<ListItem> = state
        .rows
        .iter()
        .map(|row| {
            let (status, color) = schedule_status(&row.profile, now);
            let summary = match &row.profile.schedule {
                Some(s) => crate::tui::view::fmt_schedule(s),
                None => "—".to_string(),
            };
            let clock = if row.profile.schedule.as_ref().is_some_and(|s| s.enabled) {
                "⏰ "
            } else {
                "   "
            };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(clock.to_string(), Style::default().fg(ACCENT)),
                    Span::styled(
                        row.name.clone(),
                        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(status, Style::default().fg(color)),
                ]),
                Line::from(Span::styled(format!("     {summary}"), Style::default().fg(DIM))),
            ])
        })
        .collect();

    let list = List::new(items)
        .block(picker_block(&title))
        .highlight_style(Style::default().bg(Color::Rgb(35, 40, 55)).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    let mut list_state = ListState::default();
    list_state.select(Some(state.selected));
    f.render_stateful_widget(list, area, &mut list_state);
}

fn draw_details(f: &mut Frame, area: Rect, state: &SchedulesState) {
    let title = format!(" {} ", crate::i18n::t!("tui.schedules.details"));
    let block = picker_block(&title);
    let Some(row) = state.current() else {
        f.render_widget(block, area);
        return;
    };

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            row.name.clone(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    match &row.profile.schedule {
        Some(sch) => {
            lines.push(Line::from(vec![
                Span::styled("⏰ ", Style::default().fg(ACCENT)),
                Span::styled(
                    crate::tui::view::fmt_schedule(sch),
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ),
            ]));
            let tz_label = if sch.tz == "local" {
                crate::i18n::t!("tui.schedules.tz_local").to_string()
            } else {
                sch.tz.clone()
            };
            lines.push(Line::from(Span::styled(
                format!("tz: {tz_label}"),
                Style::default().fg(DIM),
            )));
            lines.push(Line::from(""));

            // Preview the next few windows so a misconfigured schedule is
            // visible before it fires.
            let now = chrono::Utc::now();
            let mut probe = now;
            let mut shown = 0;
            if sch.enabled {
                lines.push(Line::from(Span::styled(
                    crate::i18n::t!("tui.schedules.upcoming"),
                    Style::default().fg(DIM).add_modifier(Modifier::BOLD),
                )));
            }
            while shown < 3 {
                let Some(w) = scheduler::current_or_next(sch, probe) else { break };
                let start_local = w.start.with_timezone(&chrono::Local);
                let end_local = w.end.with_timezone(&chrono::Local);
                let marker = if w.contains(now) { "● " } else { "  " };
                lines.push(Line::from(Span::styled(
                    format!(
                        "{marker}{} – {}",
                        start_local.format("%a %d %b %H:%M"),
                        end_local.format("%H:%M")
                    ),
                    if w.contains(now) {
                        Style::default().fg(GLOW)
                    } else {
                        Style::default().fg(TEXT)
                    },
                )));
                probe = w.end + chrono::Duration::minutes(1);
                shown += 1;
            }
        }
        None => {
            lines.push(Line::from(Span::styled(
                crate::i18n::t!("tui.schedules.none_hint"),
                Style::default().fg(DIM),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        crate::i18n::t!("tui.schedules.list_keys"),
        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
    )));

    let p = Paragraph::new(lines).wrap(Wrap { trim: false }).block(block);
    f.render_widget(p, area);
}

fn draw_form(f: &mut Frame, area: Rect, form: &ScheduleForm) {
    let title = format!(" {} ", crate::i18n::t!("tui.schedules.form_title", profile = form.name));
    let block = picker_block(&title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // days label
            Constraint::Length(2), // day cells
            Constraint::Length(1), // start
            Constraint::Length(1), // end
            Constraint::Length(1), // tz
            Constraint::Length(1), // enabled
            Constraint::Length(1), // spacer
            Constraint::Min(2),    // error / hints
        ])
        .split(inner);

    // Days: label on its own line, seven toggle cells below — the cells
    // alone are 34 columns, which is exactly the inner width at the 80-col
    // terminal minimum, so they can't share a line with the label.
    let days_focused = form.focus == FormField::Days;
    f.render_widget(
        Paragraph::new(Line::from(field_label(
            &crate::i18n::t!("tui.schedules.field_days"),
            days_focused,
        ))),
        rows[0],
    );
    let mut day_spans: Vec<Span> = Vec::new();
    for (i, label) in DAY_LABELS.iter().enumerate() {
        let on = form.days[i];
        let mut style = if on {
            Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };
        if days_focused && i == form.day_cursor {
            style = style.add_modifier(Modifier::UNDERLINED | Modifier::BOLD);
        }
        day_spans.push(Span::styled(format!(" {label} "), style));
        if i < DAY_LABELS.len() - 1 {
            day_spans.push(Span::raw(" "));
        }
    }
    f.render_widget(Paragraph::new(Line::from(day_spans)), rows[1]);

    draw_input_row(
        f,
        rows[2],
        &crate::i18n::t!("tui.schedules.field_start"),
        &form.start,
        form.focus == FormField::Start,
    );
    draw_input_row(
        f,
        rows[3],
        &crate::i18n::t!("tui.schedules.field_end"),
        &form.end,
        form.focus == FormField::End,
    );
    draw_input_row(
        f,
        rows[4],
        &crate::i18n::t!("tui.schedules.field_tz"),
        &form.tz,
        form.focus == FormField::Tz,
    );

    let enabled_focused = form.focus == FormField::Enabled;
    let toggle = if form.enabled {
        Span::styled(
            format!(" {} ", crate::i18n::t!("tui.schedules.enabled_on")),
            Style::default().fg(Color::Black).bg(GLOW).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            format!(" {} ", crate::i18n::t!("tui.schedules.enabled_off")),
            Style::default().fg(TEXT).bg(Color::Rgb(60, 60, 70)),
        )
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            field_label(&crate::i18n::t!("tui.schedules.field_enabled"), enabled_focused),
            toggle,
        ])),
        rows[5],
    );

    let mut tail: Vec<Line> = Vec::new();
    if let Some(err) = &form.error {
        tail.push(Line::from(Span::styled(
            err.clone(),
            Style::default().fg(ALERT).add_modifier(Modifier::BOLD),
        )));
    }
    let hint = match form.focus {
        FormField::Days => crate::i18n::t!("tui.schedules.hint_days").to_string(),
        FormField::Tz => crate::i18n::t!("tui.schedules.hint_tz").to_string(),
        FormField::Enabled => crate::i18n::t!("tui.schedules.hint_enabled").to_string(),
        _ => crate::i18n::t!("tui.schedules.hint_time").to_string(),
    };
    tail.push(Line::from(Span::styled(
        hint,
        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
    )));
    tail.push(Line::from(Span::styled(
        crate::i18n::t!("tui.schedules.form_keys"),
        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
    )));
    f.render_widget(Paragraph::new(tail).wrap(Wrap { trim: true }), rows[7]);
}

fn field_label(label: &str, focused: bool) -> Span<'static> {
    let prefix = if focused { "▶ " } else { "  " };
    let style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DIM)
    };
    Span::styled(format!("{prefix}{label:<10}"), style)
}

fn draw_input_row(f: &mut Frame, area: Rect, label: &str, input: &TextInput, focused: bool) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(13), Constraint::Min(5)])
        .split(area);
    f.render_widget(Paragraph::new(Line::from(field_label(label, focused))), cols[0]);
    let style = if focused { Style::default().fg(TEXT) } else { Style::default().fg(DIM) };
    input.render(cols[1], f.buffer_mut(), style);
}
