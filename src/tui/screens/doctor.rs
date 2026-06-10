use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::{
    ipc::{self, Request},
    tui::{
        app::{App, FlashLevel, HomeState, Screen},
        theme::*,
    },
};

#[derive(Debug, Default)]
pub struct DoctorState {
    pub loading: bool,
    pub report: Option<crate::doctor::Report>,
    pub order: Vec<usize>,
    pub selected: usize,
}

impl DoctorState {
    pub fn set_report(&mut self, report: crate::doctor::Report) {
        self.order = report.display_order();
        self.report = Some(report);
        self.selected = 0;
    }

    pub fn move_up(&mut self) {
        if self.order.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.order.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.order.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.order.len();
    }

    pub fn jump_to_first_failure(&mut self) {
        let Some(r) = &self.report else { return };
        for (pos, &idx) in self.order.iter().enumerate() {
            if r.checks[idx].status == crate::doctor::Status::Fail {
                self.selected = pos;
                return;
            }
        }
    }

    pub fn current(&self) -> Option<&crate::doctor::Check> {
        let r = self.report.as_ref()?;
        let idx = *self.order.get(self.selected)?;
        r.checks.get(idx)
    }
}

pub async fn handle_doctor_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => {
            app.set_screen(Screen::Home(HomeState::default()));
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if let Screen::Doctor(st) = &mut app.screen {
                st.move_up();
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Screen::Doctor(st) = &mut app.screen {
                st.move_down();
            }
        }
        KeyCode::Char('f') => {
            if let Screen::Doctor(st) = &mut app.screen {
                st.jump_to_first_failure();
            }
        }
        KeyCode::Char('r') => app.open_doctor().await,
        KeyCode::Char(c) => {
            let action = if let Screen::Doctor(st) = &app.screen {
                st.current().and_then(|ch| ch.actions.iter().find(|a| a.key == c).copied())
            } else {
                None
            };
            if let Some(action) = action {
                let kind = action.kind;
                match kind.run() {
                    Ok(msg) => app.globals.set_flash(msg, FlashLevel::Success),
                    Err(e) => app.globals.set_flash(e, FlashLevel::Error),
                }
                use crate::doctor::ActionKind;
                if matches!(kind, ActionKind::StartDaemon | ActionKind::StopDaemon) {
                    for _ in 0..20 {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        let want_up = matches!(kind, ActionKind::StartDaemon);
                        let up = ipc::send(&Request::Ping).await.is_ok();
                        if up == want_up {
                            break;
                        }
                    }
                }
                app.open_doctor().await;
            }
        }
        _ => {}
    }
}

pub fn draw_doctor(f: &mut Frame, _app: &App, st: &DoctorState) {
    use crate::doctor::Status;

    let area = f.area();
    let outer = Block::default()
        .title(" doctor ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(chunks[0]);

    if st.report.is_none() && st.loading {
        let p = Paragraph::new(crate::i18n::t!("tui.doctor.running"))
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center);
        f.render_widget(p, body[0]);
    } else if st.report.is_none() {
        let p = Paragraph::new(crate::i18n::t!("tui.doctor.no_report"))
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center);
        f.render_widget(p, body[0]);
    } else if let Some(report) = &st.report {
        let items: Vec<ListItem> = st
            .order
            .iter()
            .filter_map(|&i| report.checks.get(i))
            .map(|c| {
                let color = match c.status {
                    Status::Ok => Color::Rgb(120, 190, 120),
                    Status::Warn => GLOW,
                    Status::Fail => ALERT,
                    Status::Info => ACCENT,
                    Status::Skipped => DIM,
                };
                let mut lines = vec![Line::from(vec![
                    Span::styled(format!(" {} ", c.status.icon()), Style::default().fg(color)),
                    Span::styled(c.title.clone(), Style::default().fg(TEXT)),
                ])];
                if !c.purpose.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!("     {}", c.purpose),
                        Style::default().fg(DIM),
                    )));
                }
                ListItem::new(lines)
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::RIGHT).border_style(Style::default().fg(DIM)))
            .highlight_style(
                Style::default().bg(Color::Rgb(40, 50, 65)).add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        let mut state = ListState::default();
        state.select(Some(st.selected));
        f.render_stateful_widget(list, body[0], &mut state);

        let mut lines: Vec<Line> = Vec::new();
        if let Some(c) = st.current() {
            let color = match c.status {
                Status::Ok => Color::Rgb(120, 190, 120),
                Status::Warn => GLOW,
                Status::Fail => ALERT,
                Status::Info => ACCENT,
                Status::Skipped => DIM,
            };
            lines.push(Line::from(Span::styled(
                c.title.clone(),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )));
            if !c.purpose.is_empty() {
                lines.push(Line::from(Span::styled(
                    c.purpose.to_string(),
                    Style::default().fg(DIM),
                )));
            }
            lines.push(Line::from(Span::styled(
                format!("status: {}", c.status.label()),
                Style::default().fg(color),
            )));
            lines.push(Line::from(""));
            for line in c.detail.lines() {
                lines.push(Line::from(Span::styled(line.to_string(), Style::default().fg(TEXT))));
            }
            if !c.extras.is_empty() {
                lines.push(Line::from(""));
                for extra in &c.extras {
                    lines.push(Line::from(Span::styled(extra.clone(), Style::default().fg(DIM))));
                }
            }
            if let Some(hint) = &c.hint {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("hint: {hint}"),
                    Style::default().fg(GLOW),
                )));
            }
            if !c.actions.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    crate::i18n::t!("tui.doctor.actions"),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )));
                for a in &c.actions {
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("  [{}] ", a.key),
                            Style::default().fg(GLOW).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(a.label.to_string(), Style::default().fg(TEXT)),
                    ]));
                }
            }
        }
        let detail = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::NONE));
        let detail_area = Rect {
            x: body[1].x + 1,
            y: body[1].y,
            width: body[1].width.saturating_sub(1),
            height: body[1].height,
        };
        f.render_widget(detail, detail_area);
    }

    let footer = if let Some(report) = &st.report {
        let (ok, warn, fail) = report.summary();
        let tail = if st.loading {
            &format!("   ·   {}", crate::i18n::t!("tui.doctor.rerunning"))
        } else {
            ""
        };
        format!(
            "{ok} ok · {warn} warn · {fail} fail   ·   ↑/↓ nav · f first fail · r rerun · esc back{tail}"
        )
    } else {
        crate::i18n::t!("tui.doctor.running").to_string()
    };
    let p =
        Paragraph::new(Span::styled(footer, Style::default().fg(DIM))).alignment(Alignment::Center);
    f.render_widget(p, chunks[1]);
}
