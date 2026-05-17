use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::tui::{
        app::{App, Screen, LOCALES},
        theme::*,
        view::{draw_header, picker_block},
        widgets::TextInput,
    };

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsField {
    DefaultProfile,
    DefaultDuration,
    HardMode,
    Autostart,
    Locale,
    PanicDelay,
    TamperPenalty,
    Reset,
}

impl SettingsField {
    pub const ORDER: [SettingsField; 8] = [
        SettingsField::DefaultProfile,
        SettingsField::DefaultDuration,
        SettingsField::HardMode,
        SettingsField::Autostart,
        SettingsField::Locale,
        SettingsField::PanicDelay,
        SettingsField::TamperPenalty,
        SettingsField::Reset,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsField::DefaultProfile => "default profile",
            SettingsField::DefaultDuration => "default duration",
            SettingsField::HardMode => "hard mode by default",
            SettingsField::Autostart => "autostart at login",
            SettingsField::Locale => "locale",
            SettingsField::PanicDelay => "panic delay",
            SettingsField::TamperPenalty => "tamper penalty",
            SettingsField::Reset => "reset all data",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            SettingsField::DefaultProfile => "← → cycle through your modes",
            SettingsField::DefaultDuration => "e.g. 25m, 50m, 1h30m",
            SettingsField::HardMode => "space to toggle",
            SettingsField::Autostart => "space to toggle",
            SettingsField::Locale => "← → en / ru",
            SettingsField::PanicDelay => "delay before panic releases hard-mode",
            SettingsField::TamperPenalty => "time added to session on tamper",
            SettingsField::Reset => "enter to wipe config and audit log",
        }
    }
}

#[derive(Debug)]
pub struct SettingsState {
    pub original: crate::config::General,
    /// All existing profile names, in display order. Default profile is
    /// cycled by index over this list — selecting a profile that doesn't
    /// exist is no longer possible.
    pub profile_names: Vec<String>,
    pub default_profile_idx: usize,
    pub default_duration: TextInput,
    pub hard_mode: bool,
    pub autostart: bool,
    pub locale_idx: usize,
    pub panic_delay: TextInput,
    pub tamper_penalty: TextInput,
    pub focus: SettingsField,
    pub error: Option<String>,
    pub confirm_reset: bool,
}

impl SettingsState {
    pub fn from_general(g: crate::config::General, profile_names: Vec<String>) -> Self {
        let locale_idx =
            g.locale.as_deref().and_then(|l| LOCALES.iter().position(|x| *x == l)).unwrap_or(0);
        let default_profile_idx =
            profile_names.iter().position(|n| n == &g.default_profile).unwrap_or(0);
        let mut s = Self {
            profile_names,
            default_profile_idx,
            default_duration: TextInput::new(
                humantime::format_duration(g.default_duration).to_string(),
            ),
            hard_mode: g.hard_mode,
            autostart: g.autostart,
            locale_idx,
            panic_delay: TextInput::new(
                humantime::format_duration(g.panic_delay).to_string(),
            ),
            tamper_penalty: TextInput::new(
                humantime::format_duration(g.tamper_penalty).to_string(),
            ),
            focus: SettingsField::DefaultProfile,
            error: None,
            confirm_reset: false,
            original: g,
        };
        s.sync_focus();
        s
    }

    pub fn next_field(&mut self) {
        let idx = SettingsField::ORDER.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = SettingsField::ORDER[(idx + 1) % SettingsField::ORDER.len()];
        self.sync_focus();
    }

    pub fn prev_field(&mut self) {
        let idx = SettingsField::ORDER.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus =
            SettingsField::ORDER[(idx + SettingsField::ORDER.len() - 1) % SettingsField::ORDER.len()];
        self.sync_focus();
    }

    pub fn cycle_default_profile(&mut self, delta: i32) {
        if self.profile_names.is_empty() {
            return;
        }
        let n = self.profile_names.len() as i32;
        let next = (self.default_profile_idx as i32 + delta).rem_euclid(n);
        self.default_profile_idx = next as usize;
    }

    pub fn default_profile_name(&self) -> &str {
        self.profile_names
            .get(self.default_profile_idx)
            .map(String::as_str)
            .unwrap_or("")
    }

    fn sync_focus(&mut self) {
        let focus = self.focus;
        self.default_duration.focused = focus == SettingsField::DefaultDuration;
        self.panic_delay.focused = focus == SettingsField::PanicDelay;
        self.tamper_penalty.focused = focus == SettingsField::TamperPenalty;
    }

    pub fn build_general(&self) -> Result<crate::config::General, String> {
        let default_duration = humantime::parse_duration(self.default_duration.value.trim())
            .map_err(|e| format!("invalid default duration: {e}"))?;
        let panic_delay = humantime::parse_duration(self.panic_delay.value.trim())
            .map_err(|e| format!("invalid panic delay: {e}"))?;
        let tamper_penalty = humantime::parse_duration(self.tamper_penalty.value.trim())
            .map_err(|e| format!("invalid tamper penalty: {e}"))?;

        let locale = Some(LOCALES[self.locale_idx].to_string());

        Ok(crate::config::General {
            default_profile: self.default_profile_name().to_string(),
            default_duration,
            hard_mode: self.hard_mode,
            autostart: self.autostart,
            locale,
            panic_delay,
            tamper_penalty,
            ..self.original.clone()
        })
    }
}

pub async fn handle_settings_key(app: &mut App, key: KeyEvent) {
    let Screen::Settings(settings) = &mut app.screen else { return };
    if settings.confirm_reset {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.reset_all().await;
                return;
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                settings.confirm_reset = false;
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc => {
            app.set_screen(Screen::Home(crate::tui::app::HomeState::default()));
        }
        KeyCode::Tab | KeyCode::Down => settings.next_field(),
        KeyCode::BackTab | KeyCode::Up => settings.prev_field(),
        KeyCode::Enter => {
            if settings.focus == SettingsField::Reset {
                settings.confirm_reset = true;
            } else {
                app.save_settings().await;
            }
        }
        _ => {
            let focus = settings.focus;
            match focus {
                SettingsField::DefaultProfile => match key.code {
                    KeyCode::Left | KeyCode::Char('h') => settings.cycle_default_profile(-1),
                    KeyCode::Right | KeyCode::Char('l') => settings.cycle_default_profile(1),
                    _ => {}
                },
                SettingsField::DefaultDuration => {
                    settings.default_duration.handle(key);
                }
                SettingsField::HardMode => {
                    if matches!(key.code, KeyCode::Char(' ')) {
                        settings.hard_mode = !settings.hard_mode;
                    }
                }
                SettingsField::Autostart => {
                    if matches!(key.code, KeyCode::Char(' ')) {
                        settings.autostart = !settings.autostart;
                    }
                }
                SettingsField::Locale => {
                    match key.code {
                        KeyCode::Left | KeyCode::Char('h') => {
                            settings.locale_idx = (settings.locale_idx + LOCALES.len() - 1) % LOCALES.len();
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            settings.locale_idx = (settings.locale_idx + 1) % LOCALES.len();
                        }
                        _ => {}
                    }
                }
                SettingsField::PanicDelay => {
                    settings.panic_delay.handle(key);
                }
                SettingsField::TamperPenalty => {
                    settings.tamper_penalty.handle(key);
                }
                SettingsField::Reset => {
                    if matches!(key.code, KeyCode::Enter) {
                        settings.confirm_reset = true;
                    }
                }
            }
            settings.error = None;
        }
    }
}

pub fn draw_settings(f: &mut Frame, app: &App, st: &SettingsState) {
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
    let mut constraints: Vec<Constraint> = fields.iter().map(|_| Constraint::Length(2)).collect();
    constraints.push(Constraint::Length(3));
    let rows =
        Layout::default().direction(Direction::Vertical).constraints(constraints).split(inner);

    for (i, field) in fields.iter().enumerate() {
        draw_settings_field(f, rows[i], st, *field);
    }

    let status_row = rows[fields.len()];
    let mut lines: Vec<Line> = Vec::new();
    if let Some(err) = &st.error {
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

    let help = "↑/↓ · tab fields   ctrl+s save   esc cancel";
    let footer = Paragraph::new(Span::styled(help, Style::default().fg(DIM)))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(DIM)));
    f.render_widget(footer, outer[2]);

    if st.confirm_reset {
        crate::tui::view::draw_confirm_modal(
            f,
            "reset all data?",
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "this wipes:",
                    Style::default().fg(TEXT),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "· all modes you've configured",
                    Style::default().fg(DIM),
                )),
                Line::from(Span::styled(
                    "· all settings",
                    Style::default().fg(DIM),
                )),
                Line::from(Span::styled(
                    "· the full session audit log",
                    Style::default().fg(DIM),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "there is no undo.",
                    Style::default().fg(ALERT).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
            ],
            "reset everything",
            "cancel",
            true,
        );
    }
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
    f.render_widget(Paragraph::new(Line::from(Span::styled(label, label_style))), rows[0]);

    let value_style = if focused { Style::default().fg(TEXT) } else { Style::default().fg(DIM) };

    match field {
        SettingsField::DefaultProfile => {
            if st.profile_names.is_empty() {
                let warn = Style::default().fg(ALERT).add_modifier(Modifier::ITALIC);
                f.render_widget(Paragraph::new(Span::styled("no modes — create one first", warn)), rows[1]);
            } else {
                let name = st.default_profile_name();
                let spans = vec![
                    Span::styled("←  ", Style::default().fg(DIM)),
                    Span::styled(
                        name.to_string(),
                        value_style.add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("  →", Style::default().fg(DIM)),
                ];
                f.render_widget(Paragraph::new(Line::from(spans)), rows[1]);
            }
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
            f.render_widget(Paragraph::new(Span::styled("⟲ wipe all data", style)), rows[1]);
        }
    }
}