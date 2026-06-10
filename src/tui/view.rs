use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use std::time::Duration;

use crate::tui::{
    app::{App, FlashLevel, Screen},
    screens,
    theme::{ACCENT, ALERT, DIM, GLOW, TEXT},
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
    let area = f.area();
    const MIN_WIDTH: u16 = 80;
    const MIN_HEIGHT: u16 = 20;

    // Guard against narrow terminals
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let message = crate::i18n::t!(
            "tui.narrow_terminal",
            min_w = MIN_WIDTH,
            min_h = MIN_HEIGHT,
            cur_w = area.width,
            cur_h = area.height
        )
        .to_string();
        let p = Paragraph::new(message)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(TEXT));
        f.render_widget(p, area);
        return;
    }

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
    // Render flash globally so actions taken on picker/editor/settings/etc.
    // give visible feedback without bouncing back to home. Home draws the
    // flash inline in its status panel; skip the toast there to avoid
    // double-rendering.
    if !matches!(app.screen, Screen::Home(_)) {
        draw_flash_toast(f, app);
    }
}

fn draw_flash_toast(f: &mut Frame, app: &App) {
    let Some(flash) = &app.globals.flash else { return };
    let color = match flash.level {
        FlashLevel::Success => GLOW,
        FlashLevel::Warn => Color::Rgb(220, 180, 90),
        FlashLevel::Error => ALERT,
        FlashLevel::Info => ACCENT,
    };
    let area = f.area();
    let label = format!("  {}  ", flash.message);
    let text_width = label.chars().count() as u16;
    let width = text_width.min(area.width.saturating_sub(4)).max(8);
    let height = 1u16;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    // Sit just above the footer.
    let y = area.y + area.height.saturating_sub(4);
    let rect = Rect { x, y, width, height };
    f.render_widget(ratatui::widgets::Clear, rect);
    let p = Paragraph::new(Span::styled(
        label,
        Style::default().fg(Color::Black).bg(color).add_modifier(Modifier::BOLD),
    ))
    .alignment(Alignment::Center);
    f.render_widget(p, rect);
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
            Line::from("  c            cancel scheduled panic"),
            Line::from("  m            open modes picker"),
            Line::from("  1..9         quick-start mode by slot"),
            Line::from("  ? · F1       toggle help"),
            Line::from("  q · esc      quit"),
        ],
        Screen::ModePicker(_) => vec![
            Line::from(Span::styled(
                "modes",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  ↑/↓ · j/k    navigate"),
            Line::from("  enter        configure & start  (running: view)"),
            Line::from("  1..9         quick-start mode by slot"),
            Line::from("  n · a · e · c · d  new · add from preset · edit · copy · delete"),
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
            Line::from("  ↑/↓ · j/k    select site group"),
            Line::from("  enter        start session"),
            Line::from("  i · o        inspect selected group"),
            Line::from("  e            edit this mode"),
            Line::from("  shift+h      toggle hard mode"),
            Line::from("  esc · q      back to picker"),
        ],
        Screen::ModeEditor(_) => vec![
            Line::from(Span::styled(
                "editor",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  ↑/↓ · tab     next / prev field"),
            Line::from("  ctrl+s          save"),
            Line::from("  ctrl+⏎          save & start"),
            Line::from("  space           toggle app/group"),
            Line::from("  enter           activate item"),
            Line::from("  esc             cancel"),
        ],
        Screen::Settings(_) => vec![
            Line::from(Span::styled(
                "settings",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  ↑/↓ · tab       next / prev field"),
            Line::from("  space           toggle on/off"),
            Line::from("  ←/→             cycle profile / locale"),
            Line::from("  ctrl+s          save"),
            Line::from("  enter           confirm reset (on reset row only)"),
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

/// Render a centered yes/no confirmation modal over the current screen.
/// Used by editor (discard unsaved changes), picker (delete mode),
/// settings (reset all data). Caller is responsible for handling the
/// y/n key events outside; this just draws.
pub fn draw_confirm_modal(
    f: &mut Frame,
    title: &str,
    body: Vec<Line<'static>>,
    yes_label: &str,
    no_label: &str,
    destructive: bool,
) {
    let area = f.area();
    let width = 56.min(area.width.saturating_sub(4));
    let height = ((body.len() as u16) + 6).min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let rect = Rect { x, y, width, height };

    f.render_widget(ratatui::widgets::Clear, rect);

    let border_color = if destructive { ALERT } else { ACCENT };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(border_color).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let layout = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Min(1),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
        ])
        .split(inner);

    let para = Paragraph::new(body).alignment(Alignment::Center).wrap(Wrap { trim: true });
    f.render_widget(para, layout[0]);

    let yes_style = Style::default().fg(border_color).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(DIM);
    let actions = Line::from(vec![
        Span::styled("y  ", yes_style),
        Span::styled(yes_label.to_string(), Style::default().fg(TEXT)),
        Span::styled("    ·    ", dim),
        Span::styled("n  ", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
        Span::styled(no_label.to_string(), Style::default().fg(TEXT)),
    ]);
    f.render_widget(Paragraph::new(actions).alignment(Alignment::Center), layout[2]);
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
        "↑/↓ move   ⏎ select   p panic   q quit   ·   stop disabled".to_string()
    } else if app.globals.active.is_some() {
        "↑/↓ move   ⏎ select   x stop   m modes   q quit".to_string()
    } else {
        "↑/↓ move   ⏎ select   s start   m modes   q quit".to_string()
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
                panic_delay: None,
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
        app.screen = Screen::ModePicker(PickerState {
            modes,
            selected: 0,
            loading: false,
            error: None,
            confirm_delete: None,
            filter: String::new(),
        });
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
    fn snapshot_preset_picker() {
        use crate::tui::app::PresetPickerState;
        let mut app = base_app();
        app.screen = Screen::PresetPicker(PresetPickerState::default());
        insta::assert_snapshot!(render(&app, 110, 40));
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
