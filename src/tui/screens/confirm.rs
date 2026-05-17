use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
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
        };
        state.reclamp();
        state
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
            return Some(format!("cooldown — available in {}", fmt_short(rem)));
        }
        if let (Some(_cap), Some(rem)) =
            (self.mode.limits.daily_cap, self.mode.stats.daily_cap_remaining)
        {
            if rem.is_zero() {
                return Some("daily cap reached — budget restores tomorrow".into());
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
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.open_picker().await;
                return;
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                app.start_from_confirm().await;
                return;
            }
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

    draw_confirm_details(f, body[4], confirm);

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
        Span::styled(
            format!("  {:>5}  ", fmt_short(ConfirmState::MIN_BOUND)),
            Style::default().fg(DIM),
        ),
        Span::styled(bar, Style::default().fg(ACCENT)),
        Span::styled(
            format!("  {:<5}", fmt_short(confirm.effective_max())),
            Style::default().fg(DIM),
        ),
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
        lines.push(Line::from(Span::styled(err.clone(), Style::default().fg(ALERT))));
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
    let sites = &detail.expanded_sites;
    lines.push(Line::from(vec![Span::styled(
        format!("sites · {}", sites.len()),
        Style::default().fg(ACCENT),
    )]));
    if !detail.profile.site_groups.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  groups  ", Style::default().fg(DIM)),
            Span::styled(detail.profile.site_groups.join(", "), Style::default().fg(TEXT)),
        ]));
    }
    let capacity = inner.height.saturating_sub(lines.len() as u16) as usize;
    let shown = capacity.min(sites.len());
    for host in sites.iter().take(shown) {
        let short: String = host.trim_start_matches("www.").to_string();
        lines.push(Line::from(vec![
            Span::styled("  • ", Style::default().fg(DIM)),
            Span::styled(short, Style::default().fg(TEXT)),
        ]));
    }
    if sites.len() > shown {
        lines.push(Line::from(Span::styled(
            format!("  +{} more", sites.len() - shown),
            Style::default().fg(DIM),
        )));
    }
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