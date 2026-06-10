use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use tui_big_text::{BigText, PixelSize};

use crate::tui::{
    app::{App, FlashLevel, HomeState, MenuItem, Screen},
    screens::companion::draw_monk,
    theme::*,
    view::{draw_footer, draw_header, fmt_short},
};

pub async fn handle_home_key(app: &mut App, key: KeyEvent) {
    let Screen::Home(home) = &mut app.screen else { return };
    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Esc => app.should_quit = true,
        KeyCode::Up | KeyCode::Char('k') => home.move_up(),
        KeyCode::Down | KeyCode::Char('j') => home.move_down(),
        KeyCode::Enter => activate_home(app).await,
        KeyCode::Char('m') => app.open_picker(),
        KeyCode::Char('s') => {
            home.selected = 0;
            activate_home(app).await;
        }
        KeyCode::Char('x') => {
            home.selected = 1;
            activate_home(app).await;
        }
        KeyCode::Char('p') => {
            home.selected = 2;
            activate_home(app).await;
        }
        KeyCode::Char('c') => {
            // Cancel a scheduled panic release. Only meaningful when one is
            // pending (panic_releases_at is Some). No-op otherwise.
            let scheduled =
                app.globals.hard_mode.as_ref().and_then(|h| h.panic_releases_at).is_some();
            if scheduled {
                app.cancel_panic().await;
            }
        }
        KeyCode::Char(c @ '1'..='9') => {
            let idx = (c as u8 - b'1') as usize;
            app.quick_start(idx).await;
        }
        _ => {}
    }
}

pub fn draw_home(f: &mut Frame, app: &App, home: &HomeState) {
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
        if let Some((name, at)) = &app.globals.next_scheduled {
            let remaining = (*at - chrono::Utc::now()).to_std().unwrap_or_default();
            let text = format!("next  {}  in {}", name, fmt_short(remaining));
            let line = Paragraph::new(Line::from(Span::styled(
                text,
                Style::default().fg(ACCENT).add_modifier(Modifier::ITALIC),
            )))
            .alignment(Alignment::Center);
            let y = area.y + area.height.saturating_sub(2);
            let bar = Rect { x: area.x + 1, y, width: area.width.saturating_sub(2), height: 1 };
            f.render_widget(line, bar);
        }
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
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(inner);

    let mut title_spans = vec![
        Span::styled("mode  ", Style::default().fg(DIM)),
        Span::styled(
            session.profile.clone(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
    ];
    if app.globals.hard_mode.is_some() {
        title_spans.push(Span::raw("   "));
        title_spans.push(Span::styled(
            " HARD MODE ",
            Style::default().fg(Color::White).bg(ALERT).add_modifier(Modifier::BOLD),
        ));
    }
    let title = Paragraph::new(Line::from(title_spans)).alignment(Alignment::Center);
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
            if let Some(rem) = mode.stats.cooldown_remaining {
                info_lines.push(Line::from(vec![
                    Span::styled("cooldown  ", Style::default().fg(DIM)),
                    Span::styled(fmt_short(rem).to_string(), Style::default().fg(TEXT)),
                ]));
            } else {
                info_lines.push(Line::from(vec![
                    Span::styled("cooldown  ", Style::default().fg(DIM)),
                    Span::styled(fmt_short(cd).to_string(), Style::default().fg(DIM)),
                ]));
            }
        }
    }
    let info = Paragraph::new(info_lines).alignment(Alignment::Center);
    f.render_widget(info, layout[3]);

    let footer_line = if let Some(hard) = &app.globals.hard_mode {
        if let Some(at) = hard.panic_releases_at {
            // Panic scheduled — show countdown plus `c cancel` shortcut.
            let now = chrono::Utc::now();
            let remaining = (at - now).to_std().unwrap_or(std::time::Duration::ZERO);
            Line::from(vec![
                Span::styled(
                    crate::i18n::t!("tui.home_hard.scheduled_in").to_string(),
                    Style::default().fg(DIM),
                ),
                Span::styled(
                    fmt_short(remaining),
                    Style::default().fg(GLOW).add_modifier(Modifier::BOLD),
                ),
                Span::styled("   ·   c cancel", Style::default().fg(DIM)),
            ])
        } else {
            Line::from(vec![
                Span::styled(
                    crate::i18n::t!("tui.home_hard.phrase_label").to_string(),
                    Style::default().fg(DIM),
                ),
                Span::styled(
                    hard.panic_phrase.clone(),
                    Style::default().fg(GLOW).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    crate::i18n::t!("tui.home_hard.press_p").to_string(),
                    Style::default().fg(DIM),
                ),
            ])
        }
    } else {
        Line::from(Span::styled("x end  p panic", Style::default().fg(DIM)))
    };
    let footer = Paragraph::new(footer_line).alignment(Alignment::Center);
    f.render_widget(footer, layout[5]);
}

fn draw_menu(f: &mut Frame, area: Rect, app: &App, home: &HomeState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(5)])
        .split(area);

    let has_session = app.globals.active.is_some();
    let has_hard = app.globals.hard_mode.is_some();
    let items: Vec<ListItem> = MenuItem::ALL
        .iter()
        .map(|m| {
            let disabled = match m {
                MenuItem::Stop => !has_session || has_hard,
                MenuItem::Panic => !has_hard,
                _ => false,
            };
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
        .highlight_style(Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    let mut state = ListState::default();
    state.select(Some(home.selected));
    f.render_stateful_widget(list, chunks[0], &mut state);

    let info_lines = build_info_lines(app, home);
    let info = Paragraph::new(info_lines).wrap(Wrap { trim: true }).block(
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
    lines.push(Line::from(vec![Span::styled("daemon  ", Style::default().fg(DIM)), daemon]));

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
            FlashLevel::Success => GLOW,
            FlashLevel::Warn => Color::Rgb(220, 180, 90),
            FlashLevel::Error => ALERT,
            FlashLevel::Info => ACCENT,
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

// Helper functions that will need to be accessible from the main app
async fn activate_home(app: &mut App) {
    let Screen::Home(home) = &app.screen else { return };
    let item = home.selected_item();
    match item {
        MenuItem::Start => app.do_start().await,
        MenuItem::Stop => {
            if app.globals.active.is_none() {
                app.globals.set_flash(
                    crate::i18n::t!("tui.flash.nothing_to_stop").to_string(),
                    FlashLevel::Warn,
                );
            } else if app.globals.hard_mode.is_some() {
                app.globals.set_flash(
                    crate::i18n::t!("tui.flash.hard_stop_denied").to_string(),
                    FlashLevel::Error,
                );
            } else {
                app.do_stop().await;
            }
        }
        MenuItem::Panic => {
            if app.globals.hard_mode.is_none() {
                app.globals.set_flash(
                    crate::i18n::t!("tui.flash.no_hard_session").to_string(),
                    FlashLevel::Warn,
                );
            } else {
                app.do_panic().await;
            }
        }
        MenuItem::Profiles => app.open_picker(),
        MenuItem::Settings => app.open_settings().await,
        MenuItem::Doctor => app.open_doctor().await,
        MenuItem::Quit => app.should_quit = true,
    }
}
