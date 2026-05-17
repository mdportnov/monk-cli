use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use std::time::Duration;


use crate::tui::{
    app::{App, Screen},
    screens,
    theme::{ACCENT, ALERT, DIM, TEXT},
};

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
        Screen::Home(home) => screens::home::draw_home(f, app, home),
        Screen::ModePicker(picker) => screens::picker::draw_picker(f, app, picker),
        Screen::ModeConfirm(confirm) => screens::confirm::draw_confirm(f, app, confirm.as_ref()),
        Screen::ModeEditor(editor) => screens::editor::draw_editor(f, app, editor.as_ref()),
        Screen::Settings(st) => screens::settings::draw_settings(f, app, st.as_ref()),
        Screen::Doctor(st) => screens::doctor::draw_doctor(f, app, st.as_ref()),
        Screen::PresetPicker(state) => screens::picker::draw_preset_picker(f, app, state),
        Screen::Panic(st) => {
            // Render the home screen behind the modal so context remains
            // visible (timer, blocklist, etc.).
            let home = crate::tui::app::HomeState::default();
            screens::home::draw_home(f, app, &home);
            screens::panic::draw_panic(f, app, st.as_ref());
        }
    }
    if app.globals.help_open {
        draw_help_overlay(f, app);
    }
}

fn draw_help_overlay(f: &mut Frame, app: &App) {
    let area = f.area();
    let lines: Vec<Line> = match &app.screen {
        Screen::Home(_) => vec![
            Line::from(Span::styled(
                "home",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
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
            Line::from(Span::styled(
                "modes",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  ↑/↓ · j/k    navigate"),
            Line::from("  enter        configure & start"),
            Line::from("  n · a · e · d  new · add from preset · edit · delete"),
            Line::from("  r            refresh"),
            Line::from("  esc · q      back to home"),
        ],
        Screen::ModeConfirm(_) => vec![
            Line::from(Span::styled(
                "confirm",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  ←/→ · h/l    adjust duration (5m steps)"),
            Line::from("  shift+h      toggle hard mode"),
            Line::from("  enter        start session"),
            Line::from("  esc · q      back to picker"),
        ],
        Screen::ModeEditor(_) => vec![
            Line::from(Span::styled(
                "editor",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  tab/shift-tab   next / prev field"),
            Line::from("  ctrl+s          save"),
            Line::from("  space · enter   toggle app/group"),
            Line::from("  esc             cancel"),
        ],
        Screen::Settings(_) => vec![
            Line::from(Span::styled(
                "settings",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  tab/shift-tab   next / prev field"),
            Line::from("  space           toggle on/off"),
            Line::from("  ←/→             cycle locale"),
            Line::from("  ctrl+s          save"),
            Line::from("  esc             cancel"),
        ],
        Screen::Doctor(_) => vec![
            Line::from(Span::styled(
                "doctor",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  ↑/↓ · j/k    navigate checks"),
            Line::from("  f            jump to first failure"),
            Line::from("  r            rerun"),
            Line::from("  esc · q      back to home"),
        ],
        Screen::Panic(_) => vec![
            Line::from(Span::styled(
                "panic",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  type phrase    confirm release"),
            Line::from("  enter          submit"),
            Line::from("  esc            cancel"),
        ],
        Screen::PresetPicker(_) => vec![
            Line::from(Span::styled(
                "presets",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  ↑/↓ · j/k    navigate presets"),
            Line::from("  enter        prefill editor with preset"),
            Line::from("  esc · q      back to picker"),
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



pub fn kv(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<10} "), Style::default().fg(DIM)),
        Span::styled(value.to_string(), Style::default().fg(TEXT)),
    ])
}

pub fn fmt_schedule(s: &crate::config::Schedule) -> String {
    use crate::config::Weekday::*;
    let order = [Mon, Tue, Wed, Thu, Fri, Sat, Sun];
    let labels = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
    let days: String = order
        .iter()
        .zip(labels.iter())
        .filter(|(d, _)| s.days.contains(d))
        .map(|(_, l)| *l)
        .collect::<Vec<_>>()
        .join(",");
    let suffix = if s.enabled { "" } else { " (off)" };
    format!("{} {}–{}{}", days, s.start, s.end, suffix)
}

pub fn fmt_limit(d: Option<Duration>) -> String {
    match d {
        Some(v) => fmt_short(v),
        None => "—".into(),
    }
}

pub fn picker_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(title.to_string(), Style::default().fg(ACCENT)))
}


pub fn draw_header(f: &mut Frame, area: Rect, app: &App) {
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


pub fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
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
            has_schedule: false,
        }
    }

    fn base_app() -> App {
        App {
            globals: Globals {
                daemon_running: true,
                frame: 0,
                cached_modes: vec![sample_mode("deepwork"), sample_mode("reading")],
                ..Default::default()
            },
            ..Default::default()
        }
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
        app.screen =
            Screen::ModePicker(PickerState { modes, selected: 0, loading: false, error: None });
        insta::assert_snapshot!(render(&app, 100, 30));
    }

    #[test]
    fn snapshot_confirm() {
        let mut app = base_app();
        let confirm =
            ConfirmState::from_mode(sample_mode("deepwork"), Duration::from_secs(50 * 60), false);
        app.screen = Screen::ModeConfirm(Box::new(confirm));
        insta::assert_snapshot!(render(&app, 100, 30));
    }

    #[test]
    fn snapshot_editor_new() {
        let mut app = base_app();
        let mut editor = EditorState::new_mode();
        editor.apps.items.clear();
        editor.groups.items.clear();
        editor.brands.items.clear();
        app.screen = Screen::ModeEditor(Box::new(editor));
        insta::assert_snapshot!(render(&app, 100, 30));
    }

    #[test]
    fn flash_levels_all_render() {
        for level in [FlashLevel::Info, FlashLevel::Success, FlashLevel::Warn, FlashLevel::Error] {
            let mut app = base_app();
            app.screen = Screen::Home(HomeState::default());
            app.globals.flash =
                Some(Flash { message: format!("{level:?} message"), level, expires_at: 100 });
            let _ = render(&app, 90, 24);
        }
    }
}
