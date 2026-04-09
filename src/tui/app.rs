use std::{io, time::Duration};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    config::{Config, Limits},
    ipc::{self, HardModeInfo, ModeSummary, Request, Response},
    session::Session,
    Result,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    Start,
    Stop,
    Panic,
    Profiles,
    Doctor,
    Quit,
}

impl MenuItem {
    pub const ALL: [MenuItem; 6] = [
        MenuItem::Start,
        MenuItem::Stop,
        MenuItem::Panic,
        MenuItem::Profiles,
        MenuItem::Doctor,
        MenuItem::Quit,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MenuItem::Start => "Start session",
            MenuItem::Stop => "Stop session",
            MenuItem::Panic => "Panic escape",
            MenuItem::Profiles => "Modes",
            MenuItem::Doctor => "Doctor",
            MenuItem::Quit => "Quit",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            MenuItem::Start => "begin a focus session with the default mode",
            MenuItem::Stop => "end the active session (soft mode only)",
            MenuItem::Panic => "request a delayed hard-mode escape",
            MenuItem::Profiles => "list configured modes",
            MenuItem::Doctor => "check environment and daemon health",
            MenuItem::Quit => "leave the TUI",
        }
    }
}

#[derive(Debug, Default)]
pub struct HomeState {
    pub selected: usize,
}

impl HomeState {
    pub fn move_up(&mut self) {
        if self.selected == 0 {
            self.selected = MenuItem::ALL.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        self.selected = (self.selected + 1) % MenuItem::ALL.len();
    }

    pub fn selected_item(&self) -> MenuItem {
        MenuItem::ALL[self.selected]
    }
}

#[derive(Debug, Default)]
pub struct PickerState {
    pub modes: Vec<ModeSummary>,
    pub selected: usize,
    pub loading: bool,
    pub error: Option<String>,
}

impl PickerState {
    pub fn move_up(&mut self) {
        if self.modes.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.modes.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.modes.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.modes.len();
    }

    pub fn current(&self) -> Option<&ModeSummary> {
        self.modes.get(self.selected)
    }
}

#[derive(Debug)]
pub struct ConfirmState {
    pub mode: ModeSummary,
    pub duration: Duration,
    pub requested: Duration,
    pub hard: bool,
    pub clamped: bool,
    pub error: Option<String>,
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

    pub fn inc(&mut self) {
        self.requested = self.requested.saturating_add(Self::STEP);
        self.reclamp();
    }

    pub fn dec(&mut self) {
        self.requested = self
            .requested
            .checked_sub(Self::STEP)
            .unwrap_or(Self::MIN_BOUND);
        self.reclamp();
    }

    pub fn blocked_reason(&self) -> Option<String> {
        if let Some(rem) = self.mode.stats.cooldown_remaining {
            return Some(format!("cooldown — available in {}", super::view::fmt_short(rem)));
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

#[derive(Debug)]
pub enum Screen {
    Home(HomeState),
    ModePicker(PickerState),
    ModeConfirm(Box<ConfirmState>),
}

impl Default for Screen {
    fn default() -> Self {
        Screen::Home(HomeState::default())
    }
}

#[derive(Debug, Default)]
pub struct Globals {
    pub active: Option<Session>,
    pub hard_mode: Option<HardModeInfo>,
    pub daemon_running: bool,
    pub flash: Option<String>,
    pub frame: u64,
}

#[derive(Debug, Default)]
pub struct App {
    pub screen: Screen,
    pub globals: Globals,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        let _ = Config::load();
        Self::default()
    }

    pub async fn refresh(&mut self) {
        match ipc::send(&Request::Status).await {
            Ok(Response::Status { active, hard_mode, .. }) => {
                self.globals.daemon_running = true;
                self.globals.active = active.map(|b| *b);
                self.globals.hard_mode = hard_mode.map(|b| *b);
            }
            _ => {
                self.globals.daemon_running = false;
                self.globals.active = None;
                self.globals.hard_mode = None;
            }
        }
    }

    pub async fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        match &self.screen {
            Screen::Home(_) => self.handle_home_key(key).await,
            Screen::ModePicker(_) => self.handle_picker_key(key).await,
            Screen::ModeConfirm(_) => self.handle_confirm_key(key).await,
        }
    }

    async fn handle_home_key(&mut self, key: KeyEvent) {
        let Screen::Home(home) = &mut self.screen else { return };
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Up | KeyCode::Char('k') => home.move_up(),
            KeyCode::Down | KeyCode::Char('j') => home.move_down(),
            KeyCode::Enter | KeyCode::Char(' ') => self.activate_home().await,
            KeyCode::Char('m') => self.open_picker().await,
            KeyCode::Char('s') => {
                home.selected = 0;
                self.activate_home().await;
            }
            KeyCode::Char('x') => {
                home.selected = 1;
                self.activate_home().await;
            }
            KeyCode::Char('p') => {
                home.selected = 2;
                self.activate_home().await;
            }
            _ => {}
        }
    }

    async fn handle_picker_key(&mut self, key: KeyEvent) {
        let Screen::ModePicker(picker) = &mut self.screen else { return };
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.screen = Screen::Home(HomeState::default());
            }
            KeyCode::Up | KeyCode::Char('k') => picker.move_up(),
            KeyCode::Down | KeyCode::Char('j') => picker.move_down(),
            KeyCode::Char('r') => self.refresh_picker().await,
            KeyCode::Enter | KeyCode::Char(' ') => self.open_confirm_from_picker(),
            _ => {}
        }
    }

    fn open_confirm_from_picker(&mut self) {
        let Screen::ModePicker(picker) = &self.screen else { return };
        let Some(mode) = picker.current().cloned() else { return };
        let cfg = Config::load().ok();
        let default_dur = cfg
            .as_ref()
            .map(|c| c.general.default_duration)
            .unwrap_or(Duration::from_secs(25 * 60));
        let hard = cfg.as_ref().map(|c| c.general.hard_mode).unwrap_or(false);
        self.screen = Screen::ModeConfirm(Box::new(ConfirmState::from_mode(mode, default_dur, hard)));
    }

    async fn handle_confirm_key(&mut self, key: KeyEvent) {
        let Screen::ModeConfirm(confirm) = &mut self.screen else { return };
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.open_picker().await,
            KeyCode::Left | KeyCode::Char('h') => confirm.dec(),
            KeyCode::Right | KeyCode::Char('l') => confirm.inc(),
            KeyCode::Char('H') => confirm.hard = !confirm.hard,
            KeyCode::Enter | KeyCode::Char(' ') => self.start_from_confirm().await,
            _ => {}
        }
    }

    async fn start_from_confirm(&mut self) {
        let Screen::ModeConfirm(confirm) = &mut self.screen else { return };
        if let Some(reason) = confirm.blocked_reason() {
            confirm.error = Some(reason);
            return;
        }
        let req = Request::Start {
            profile: confirm.mode.name.clone(),
            duration: confirm.duration,
            hard_mode: confirm.hard,
            reason: None,
        };
        match ipc::send(&req).await {
            Ok(Response::Session(s)) => {
                self.globals.flash = Some(format!("started `{}`", s.profile));
                self.screen = Screen::Home(HomeState::default());
            }
            Ok(Response::Error { message }) => {
                if let Screen::ModeConfirm(c) = &mut self.screen {
                    c.error = Some(message);
                }
            }
            Ok(_) => {
                if let Screen::ModeConfirm(c) = &mut self.screen {
                    c.error = Some("unexpected response".into());
                }
            }
            Err(e) => {
                if let Screen::ModeConfirm(c) = &mut self.screen {
                    c.error = Some(e.to_string());
                }
            }
        }
    }

    async fn open_picker(&mut self) {
        self.screen = Screen::ModePicker(PickerState { loading: true, ..Default::default() });
        self.refresh_picker().await;
    }

    async fn refresh_picker(&mut self) {
        let result = ipc::send(&Request::ListModes).await;
        let Screen::ModePicker(picker) = &mut self.screen else { return };
        picker.loading = false;
        match result {
            Ok(Response::Modes(modes)) => {
                picker.error = None;
                picker.modes = modes;
                if picker.selected >= picker.modes.len() {
                    picker.selected = 0;
                }
            }
            Ok(Response::Error { message }) => picker.error = Some(message),
            Ok(_) => picker.error = Some("unexpected response".into()),
            Err(e) => picker.error = Some(e.to_string()),
        }
    }

    async fn activate_home(&mut self) {
        let Screen::Home(home) = &self.screen else { return };
        match home.selected_item() {
            MenuItem::Start => self.do_start().await,
            MenuItem::Stop => self.do_stop().await,
            MenuItem::Panic => self.do_panic().await,
            MenuItem::Profiles => self.open_picker().await,
            MenuItem::Doctor => {
                self.globals.flash =
                    Some("run `monk doctor` in a shell for the full report".into());
            }
            MenuItem::Quit => self.should_quit = true,
        }
    }

    async fn do_start(&mut self) {
        let cfg = match Config::load() {
            Ok(c) => c,
            Err(e) => {
                self.globals.flash = Some(format!("config error: {e}"));
                return;
            }
        };
        let req = Request::Start {
            profile: cfg.general.default_profile.clone(),
            duration: cfg.general.default_duration,
            hard_mode: cfg.general.hard_mode,
            reason: None,
        };
        match ipc::send(&req).await {
            Ok(Response::Session(s)) => {
                self.globals.flash = Some(format!("started `{}`", s.profile));
            }
            Ok(Response::Error { message }) => self.globals.flash = Some(message),
            Ok(_) => self.globals.flash = Some("unexpected response".into()),
            Err(e) => self.globals.flash = Some(e.to_string()),
        }
    }

    async fn do_stop(&mut self) {
        match ipc::send(&Request::Stop { id: None }).await {
            Ok(Response::Session(s)) => {
                self.globals.flash = Some(format!("stopped `{}`", s.profile))
            }
            Ok(Response::HardModeActive(_)) => {
                self.globals.flash = Some("hard mode active — stop denied".into())
            }
            Ok(Response::Error { message }) => self.globals.flash = Some(message),
            Ok(_) => self.globals.flash = Some("nothing to stop".into()),
            Err(e) => self.globals.flash = Some(e.to_string()),
        }
    }

    async fn do_panic(&mut self) {
        match ipc::send(&Request::Panic { phrase: String::new(), cancel: false }).await {
            Ok(Response::PanicScheduled(info)) => {
                self.globals.flash = Some(match info.panic_releases_at {
                    Some(at) => format!("panic release at {}", at.to_rfc3339()),
                    None => "panic cancelled".into(),
                });
            }
            Ok(Response::Error { message }) => self.globals.flash = Some(message),
            Ok(_) => self.globals.flash = Some("no hard-mode session".into()),
            Err(e) => self.globals.flash = Some(e.to_string()),
        }
    }
}

pub async fn run() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = main_loop(&mut terminal).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    result
}

async fn main_loop<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    let mut app = App::new();
    let mut ticks: u64 = 0;

    loop {
        if ticks % 4 == 0 {
            app.refresh().await;
        }
        app.globals.frame = ticks;
        terminal.draw(|f| super::view::draw(f, &app))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key).await;
            }
        }

        ticks = ticks.wrapping_add(1);
        if app.should_quit {
            break;
        }
    }
    Ok(())
}
