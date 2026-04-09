use std::{io, time::Duration};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    config::Config,
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
pub enum Screen {
    Home(HomeState),
    ModePicker(PickerState),
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
            _ => {}
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
