use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use std::time::Duration;

use tui_big_text::{BigText, PixelSize};

use crate::{
    config::Limits,
    ipc::ModeSummary,
    tui::{
        app::{App, Screen},
        theme::*,
        view::{draw_header, fmt_limit, fmt_schedule, fmt_short, kv, picker_block},
    },
};

#[derive(Debug)]
pub struct ConfirmState {
    pub mode: ModeSummary,
    pub duration: Duration,
    pub requested: Duration,
    pub hard: bool,
    pub clamped: bool,
    pub error: Option<String>,
    pub detail: Option<crate::ipc::ModeDetailPayload>,
    /// Index into `detail.profile.site_groups` of the highlighted group.
    /// ↑/↓ moves it; shift+Enter opens the inspector modal for that group.
    pub selected_group: usize,
    /// When Some, the inspector modal is open and shows the named group's
    /// full host list. Esc closes it.
    pub expanded_group: Option<String>,
    /// Vertical scroll offset inside the inspector modal.
    pub expanded_scroll: u16,
}

impl ConfirmState {
    pub const STEP: Duration = Duration::from_secs(5 * 60);
    pub const MIN_BOUND: Duration = Duration::from_secs(5 * 60);
    pub const MAX_BOUND: Duration = Duration::from_secs(8 * 3600);

    pub fn from_mode(mode: ModeSummary, default: Duration, hard_default: bool) -> Self {
        let mut state = Self {
            requested: default,
            duration: default,
            hard: hard_default,
            clamped: false,
            error: None,
            mode,
            detail: None,
            selected_group: 0,
            expanded_group: None,
            expanded_scroll: 0,
        };
        state.reclamp();
        state
    }

    pub fn group_count(&self) -> usize {
        self.detail.as_ref().map(|d| d.profile.site_groups.len()).unwrap_or(0)
    }

    pub fn select_prev_group(&mut self) {
        let n = self.group_count();
        if n == 0 {
            return;
        }
        self.selected_group =
            if self.selected_group == 0 { n - 1 } else { self.selected_group - 1 };
    }

    pub fn select_next_group(&mut self) {
        let n = self.group_count();
        if n == 0 {
            return;
        }
        self.selected_group = (self.selected_group + 1) % n;
    }

    pub fn open_selected_group(&mut self) {
        let Some(detail) = &self.detail else { return };
        if let Some(name) = detail.profile.site_groups.get(self.selected_group) {
            self.expanded_group = Some(name.clone());
            self.expanded_scroll = 0;
        }
    }

    pub fn close_expanded(&mut self) {
        self.expanded_group = None;
        self.expanded_scroll = 0;
    }

    pub fn reclamp(&mut self) {
        let mut d = self.requested.max(Self::MIN_BOUND).min(Self::MAX_BOUND);
        let ceiling = self.effective_max();
        let mut clamped = false;
        if d > ceiling {
            d = ceiling;
            clamped = true;
        }
        if let Some(min) = self.mode.limits.min_duration {
            if d < min {
                d = min;
            }
        }
        self.duration = d;
        self.clamped = clamped;
    }

    pub fn effective_max(&self) -> Duration {
        self.mode.limits.max_duration.unwrap_or(Self::MAX_BOUND)
    }

    pub fn increment_duration(&mut self) {
        self.requested = self.requested.saturating_add(Self::STEP);
        self.reclamp();
    }

    pub fn decrement_duration(&mut self) {
        self.requested = self.requested.checked_sub(Self::STEP).unwrap_or(Self::MIN_BOUND);
        self.reclamp();
    }

    pub fn blocked_reason(&self) -> Option<String> {
        if let Some(rem) = self.mode.stats.cooldown_remaining {
            let d = fmt_short(rem);
            return Some(
                crate::i18n::t!("tui.confirm.cooldown_available_in", duration = d).to_string(),
            );
        }
        if let (Some(_cap), Some(rem)) =
            (self.mode.limits.daily_cap, self.mode.stats.daily_cap_remaining)
        {
            if rem.is_zero() {
                return Some(crate::i18n::t!("tui.confirm.daily_cap_reached").to_string());
            }
        }
        None
    }

    pub fn slider_fraction(&self) -> f32 {
        let max = self.effective_max().as_secs() as f32;
        let min = Self::MIN_BOUND.as_secs() as f32;
        let cur = self.duration.as_secs() as f32;
        if max <= min {
            return 1.0;
        }
        ((cur - min) / (max - min)).clamp(0.0, 1.0)
    }

    pub fn limits(&self) -> &Limits {
        &self.mode.limits
    }
}

pub async fn handle_confirm_key(app: &mut App, key: KeyEvent) {
    let mut clamped = false;
    {
        let Screen::ModeConfirm(confirm) = &mut app.screen else { return };

        // Group inspector modal is open — keys go there.
        if confirm.expanded_group.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => confirm.close_expanded(),
                KeyCode::Up | KeyCode::Char('k') => {
                    confirm.expanded_scroll = confirm.expanded_scroll.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    confirm.expanded_scroll = confirm.expanded_scroll.saturating_add(1);
                }
                KeyCode::PageUp => {
                    confirm.expanded_scroll = confirm.expanded_scroll.saturating_sub(10);
                }
                KeyCode::PageDown => {
                    confirm.expanded_scroll = confirm.expanded_scroll.saturating_add(10);
                }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.open_picker();
                return;
            }
            KeyCode::Enter => {
                app.start_from_confirm().await;
                return;
            }
            KeyCode::Char(' ') | KeyCode::Char('i') | KeyCode::Char('o') => {
                if confirm.group_count() > 0 {
                    confirm.open_selected_group();
                    return;
                }
            }
            KeyCode::Char('e') => {
                app.open_editor_from_confirm();
                return;
            }
            KeyCode::Up | KeyCode::Char('k') => confirm.select_prev_group(),
            KeyCode::Down | KeyCode::Char('j') => confirm.select_next_group(),
            KeyCode::Left | KeyCode::Char('h') => {
                confirm.decrement_duration();
                clamped = confirm.clamped;
            }
            KeyCode::Right | KeyCode::Char('l') => {
                confirm.increment_duration();
                clamped = confirm.clamped;
            }
            KeyCode::Char('H') => confirm.hard = !confirm.hard,
            _ => {}
        }
    }
    if clamped {
        app.trigger_clamp_effect();
    }
}

pub fn draw_confirm(f: &mut Frame, app: &App, confirm: &ConfirmState) {
    let area = f.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(3)])
        .split(area);

    draw_header(f, outer[0], app);

    let is_running = matches!(
        &app.globals.active,
        Some(s) if s.profile == confirm.mode.name
    );
    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(8),
            Constraint::Length(if is_running { 0 } else { 2 }),
            Constraint::Length(2),
            Constraint::Min(4),
        ])
        .split(outer[1]);

    let title_line = if is_running {
        let remaining = app.globals.active.as_ref().map(|s| s.remaining()).unwrap_or_default();
        Line::from(vec![
            Span::styled(
                crate::i18n::t!("tui.confirm.running_badge").to_string(),
                Style::default().fg(ALERT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                confirm.mode.name.clone(),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                {
                    let d = fmt_short(remaining);
                    crate::i18n::t!("tui.confirm.time_left", duration = d).to_string()
                },
                Style::default().fg(DIM),
            ),
        ])
    } else {
        let chip_label = if confirm.hard { " HARD " } else { " SOFT " };
        let chip_style = if confirm.hard {
            Style::default().fg(Color::Black).bg(ALERT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
        };
        Line::from(vec![
            Span::styled(
                crate::i18n::t!("tui.confirm.start_prefix").to_string(),
                Style::default().fg(DIM),
            ),
            Span::styled(
                confirm.mode.name.clone(),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw("   "),
            Span::styled(chip_label, chip_style),
            Span::styled("  H toggle", Style::default().fg(DIM)),
        ])
    };
    let title = Paragraph::new(title_line).alignment(Alignment::Center);
    f.render_widget(title, body[0]);

    let timer_duration = if is_running {
        app.globals.active.as_ref().map(|s| s.remaining()).unwrap_or(confirm.duration)
    } else {
        confirm.duration
    };
    let secs = timer_duration.as_secs();
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    let timer_text = format!("{h:02}:{m:02}:{s:02}");
    let timer_color = if !is_running && confirm.clamped { ALERT } else { GLOW };
    let big = BigText::builder()
        .pixel_size(PixelSize::Quadrant)
        .style(Style::default().fg(timer_color))
        .alignment(Alignment::Center)
        .lines(vec![Line::from(timer_text)])
        .build();
    f.render_widget(big, body[1]);

    if !is_running {
        let blocked = confirm.blocked_reason().is_some();
        draw_duration_slider(f, body[2], confirm, blocked);
    }

    let hints = Paragraph::new(build_confirm_status(confirm, is_running))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(hints, body[3]);

    draw_confirm_details(f, body[4], confirm);

    let has_groups = confirm.group_count() > 0;
    let help = if is_running {
        if has_groups {
            crate::i18n::t!("tui.confirm.footer_running").to_string()
        } else {
            crate::i18n::t!("tui.confirm.footer_running_no_groups").to_string()
        }
    } else if confirm.blocked_reason().is_some() {
        crate::i18n::t!("tui.confirm.footer_blocked").to_string()
    } else if has_groups {
        crate::i18n::t!("tui.confirm.footer_default").to_string()
    } else {
        crate::i18n::t!("tui.confirm.footer_no_groups").to_string()
    };
    let footer = Paragraph::new(Span::styled(help, Style::default().fg(DIM)))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(DIM)));
    f.render_widget(footer, outer[2]);

    if let Some(name) = &confirm.expanded_group {
        draw_group_inspector(f, name, confirm.expanded_scroll);
    }
}

fn draw_group_inspector(f: &mut Frame, group_id: &str, scroll: u16) {
    let area = f.area();
    let width = 64.min(area.width.saturating_sub(4));
    let height = 24.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let rect = Rect { x, y, width, height };

    f.render_widget(ratatui::widgets::Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            format!(" {group_id} "),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let hosts = crate::sites::all_groups()
        .ok()
        .and_then(|all| all.into_iter().find(|g| g.qualified() == group_id))
        .map(|g| g.hosts)
        .unwrap_or_default();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let header = Paragraph::new(Span::styled(
        crate::i18n::t!("tui.confirm.hosts_count", count = hosts.len()).to_string(),
        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
    ))
    .alignment(Alignment::Center);
    f.render_widget(header, chunks[0]);

    let lines: Vec<Line> = hosts
        .iter()
        .map(|h| {
            Line::from(vec![
                Span::styled("  • ", Style::default().fg(DIM)),
                Span::styled(h.clone(), Style::default().fg(TEXT)),
            ])
        })
        .collect();

    // Clamp scroll so we can't page past the end.
    let visible = chunks[1].height as usize;
    let max_scroll = lines.len().saturating_sub(visible) as u16;
    let scroll = scroll.min(max_scroll);

    f.render_widget(Paragraph::new(lines).scroll((scroll, 0)), chunks[1]);

    let hint = Paragraph::new(Span::styled(
        crate::i18n::t!("tui.confirm.inspector_hint").to_string(),
        Style::default().fg(DIM),
    ))
    .alignment(Alignment::Center);
    f.render_widget(hint, chunks[2]);
}

fn draw_duration_slider(f: &mut Frame, area: Rect, confirm: &ConfirmState, blocked: bool) {
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
    let bar_color = if blocked { DIM } else { ACCENT };
    let line = Line::from(vec![
        Span::styled(
            format!("  {:>5}  ", fmt_short(ConfirmState::MIN_BOUND)),
            Style::default().fg(DIM),
        ),
        Span::styled(bar, Style::default().fg(bar_color)),
        Span::styled(
            format!("  {:<5}", fmt_short(confirm.effective_max())),
            Style::default().fg(DIM),
        ),
    ]);
    f.render_widget(Paragraph::new(line).alignment(Alignment::Center), area);
}

fn build_confirm_status(confirm: &ConfirmState, is_running: bool) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();
    if is_running {
        let hint = if confirm.group_count() > 0 {
            crate::i18n::t!("tui.confirm.running_inspect_hint").to_string()
        } else {
            crate::i18n::t!("tui.confirm.running_inspect_hint_no_groups").to_string()
        };
        lines.push(Line::from(Span::styled(
            hint,
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )));
        return lines;
    }
    if confirm.clamped {
        let m = fmt_short(confirm.effective_max());
        let msg = crate::i18n::t!("tui.confirm.clamped_to_max", max = m).to_string();
        lines.push(Line::from(Span::styled(
            msg,
            Style::default().fg(ALERT).add_modifier(Modifier::BOLD),
        )));
    }
    if let Some(reason) = confirm.blocked_reason() {
        lines.push(Line::from(Span::styled(
            reason,
            Style::default().fg(ALERT).add_modifier(Modifier::BOLD),
        )));
    } else if let Some(err) = &confirm.error {
        lines.push(Line::from(Span::styled(err.clone(), Style::default().fg(ALERT))));
    } else if confirm.hard {
        lines.push(Line::from(Span::styled(
            crate::i18n::t!("tui.confirm.hard_mode_note").to_string(),
            Style::default().fg(GLOW).add_modifier(Modifier::BOLD),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            crate::i18n::t!("tui.confirm.soft_mode_note").to_string(),
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )));
    }
    lines
}

fn draw_confirm_details(f: &mut Frame, area: Rect, confirm: &ConfirmState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(28),
            Constraint::Percentage(40),
            Constraint::Percentage(32),
        ])
        .split(area);

    f.render_widget(build_contract_panel(confirm), cols[0]);
    draw_blocklist_panel(f, cols[1], confirm);
    draw_usage_panel(f, cols[2], confirm);
}

fn build_contract_panel(confirm: &ConfirmState) -> Paragraph<'static> {
    let limits = confirm.limits();
    let mut lines: Vec<Line> = vec![
        kv("max", &fmt_limit(limits.max_duration)),
        kv("min", &fmt_limit(limits.min_duration)),
        kv("cooldown", &fmt_limit(limits.cooldown)),
        kv("daily cap", &fmt_limit(limits.daily_cap)),
        Line::from(""),
        kv("used today", &fmt_short(confirm.mode.stats.used_24h)),
    ];
    if let Some(rem) = confirm.mode.stats.daily_cap_remaining {
        lines.push(kv("budget left", &fmt_short(rem)));
    }
    if let Some(detail) = &confirm.detail {
        lines.push(Line::from(""));
        lines.push(kv("sessions 14d", &detail.total_sessions_7d.to_string()));
        lines.push(kv("total 14d", &fmt_short(detail.total_duration_7d)));
        if let Some(sch) = &detail.profile.schedule {
            lines.push(Line::from(""));
            lines.push(kv("schedule", &fmt_schedule(sch)));
        }
    }
    Paragraph::new(lines).wrap(Wrap { trim: false }).block(picker_block(" contract "))
}

fn draw_blocklist_panel(f: &mut Frame, area: Rect, confirm: &ConfirmState) {
    let block = picker_block(" blocked ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(detail) = &confirm.detail else {
        let p = Paragraph::new(Span::styled(
            "loading…",
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        ));
        f.render_widget(p, inner);
        return;
    };

    let mut lines: Vec<Line> = Vec::new();
    let apps = &detail.profile.apps;
    lines.push(Line::from(vec![Span::styled(
        format!("apps · {}", apps.len()),
        Style::default().fg(ACCENT),
    )]));
    if apps.is_empty() {
        lines.push(Line::from(Span::styled(
            "  —",
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        )));
    } else {
        for a in apps.iter().take(6) {
            let name: String = a.clone();
            lines.push(Line::from(vec![
                Span::styled("  • ", Style::default().fg(DIM)),
                Span::styled(name, Style::default().fg(TEXT)),
            ]));
        }
        if apps.len() > 6 {
            lines.push(Line::from(Span::styled(
                format!("  +{} more", apps.len() - 6),
                Style::default().fg(DIM),
            )));
        }
    }
    lines.push(Line::from(""));

    // Groups — show each as a collapsed entry with host count. Loading the
    // full registry per render is fine: ~590 entries, parsed via include_str!
    // from a static TOML, costs <1ms on warm cache and we render at ~8 fps.
    let groups = &detail.profile.site_groups;
    if !groups.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(format!("site groups · {}", groups.len()), Style::default().fg(ACCENT)),
            Span::styled(
                "    ↑/↓ select · i inspect",
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            ),
        ]));
        let registry = crate::sites::all_groups().ok();
        for (i, q) in groups.iter().enumerate() {
            let size = registry
                .as_ref()
                .and_then(|all| all.iter().find(|g| &g.qualified() == q))
                .map(|g| g.hosts.len())
                .unwrap_or(0);
            let is_selected = i == confirm.selected_group;
            let bullet = if is_selected { "▶ " } else { "  • " };
            let bullet_style = if is_selected {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            };
            let name_style = if is_selected {
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT)
            };
            lines.push(Line::from(vec![
                Span::styled(bullet, bullet_style),
                Span::styled(q.clone(), name_style),
                Span::styled(format!("  ({size} hosts)"), Style::default().fg(DIM)),
            ]));
        }
        lines.push(Line::from(""));
    }

    // Custom sites — only what the user explicitly added on top of groups.
    let custom = &detail.profile.sites;
    if !custom.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            format!("custom sites · {}", custom.len()),
            Style::default().fg(ACCENT),
        )]));
        let used = lines.len() as u16;
        let capacity = inner.height.saturating_sub(used + 2) as usize;
        let shown = capacity.min(custom.len());
        for host in custom.iter().take(shown) {
            let short = host.trim_start_matches("www.").to_string();
            lines.push(Line::from(vec![
                Span::styled("  • ", Style::default().fg(DIM)),
                Span::styled(short, Style::default().fg(TEXT)),
            ]));
        }
        if custom.len() > shown {
            lines.push(Line::from(Span::styled(
                format!("  +{} more", custom.len() - shown),
                Style::default().fg(DIM),
            )));
        }
        lines.push(Line::from(""));
    }

    // Footer: total blocked hosts after group expansion.
    lines.push(Line::from(Span::styled(
        format!("total · {} hosts", detail.expanded_sites.len()),
        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
    )));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_usage_panel(f: &mut Frame, area: Rect, confirm: &ConfirmState) {
    let block = picker_block(" usage · 14d ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(detail) = &confirm.detail else {
        let p = Paragraph::new(Span::styled(
            "loading…",
            Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
        ));
        f.render_widget(p, inner);
        return;
    };

    if detail.usage.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "no history",
                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
            )),
            inner,
        );
        return;
    }

    let max_secs = detail.usage.iter().map(|d| d.total.as_secs()).max().unwrap_or(0).max(1);
    let bar_area_height = inner.height.saturating_sub(2);
    let mut lines: Vec<Line> = Vec::new();
    for day in &detail.usage {
        let secs = day.total.as_secs();
        let frac = secs as f64 / max_secs as f64;
        let bar_width = inner.width.saturating_sub(14) as usize;
        let filled = ((bar_width as f64) * frac).round() as usize;
        let bar: String = "█".repeat(filled);
        let pad: String = "·".repeat(bar_width.saturating_sub(filled));
        let color = if secs == 0 { DIM } else { ACCENT };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<5} ", day.date), Style::default().fg(DIM)),
            Span::styled(bar, Style::default().fg(color)),
            Span::styled(pad, Style::default().fg(DIM)),
            Span::styled(format!(" {:>5}", fmt_short(day.total)), Style::default().fg(TEXT)),
        ]));
        if lines.len() as u16 >= bar_area_height {
            break;
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
}
