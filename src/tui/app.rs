use std::{io, time::Duration};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    config::Config,
    ipc::{self, HardModeInfo, Request, Response},
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
            MenuItem::Profiles => "Profiles",
            MenuItem::Doctor => "Doctor",
            MenuItem::Quit => "Quit",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            MenuItem::Start => "begin a focus session with the default profile",
            MenuItem::Stop => "end the active session (soft mode only)",
            MenuItem::Panic => "request a delayed hard-mode escape",
            MenuItem::Profiles => "list configured profiles",
            MenuItem::Doctor => "check environment and daemon health",
            MenuItem::Quit => "leave the TUI",
        }
    }
}

#[derive(Debug, Default)]
pub struct App {
    pub active: Option<Session>,
    pub hard_mode: Option<HardModeInfo>,
    pub daemon_running: bool,
    pub selected: usize,
    pub frame: u64,
    pub flash: Option<String>,
    pub profile_names: Vec<String>,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        let profile_names = Config::load()
            .ok()
            .map(|c| c.profiles.keys().cloned().collect())
            .unwrap_or_default();
        Self { profile_names, ..Self::default() }
    }

    pub fn selected_item(&self) -> MenuItem {
        MenuItem::ALL[self.selected]
    }

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

    pub async fn refresh(&mut self) {
        match ipc::send(&Request::Status).await {
            Ok(Response::Status { active, hard_mode, .. }) => {
                self.daemon_running = true;
                self.active = active.map(|b| *b);
                self.hard_mode = hard_mode.map(|b| *b);
            }
            _ => {
                self.daemon_running = false;
                self.active = None;
                self.hard_mode = None;
            }
        }
    }

    pub async fn activate(&mut self) {
        match self.selected_item() {
            MenuItem::Start => self.do_start().await,
            MenuItem::Stop => self.do_stop().await,
            MenuItem::Panic => self.do_panic().await,
            MenuItem::Profiles => {
                let msg = if self.profile_names.is_empty() {
                    "no profiles — run `monk init`".to_string()
                } else {
                    format!("profiles: {}", self.profile_names.join(", "))
                };
                self.flash = Some(msg);
            }
            MenuItem::Doctor => {
                self.flash = Some("run `monk doctor` in a shell for the full report".into());
            }
            MenuItem::Quit => self.should_quit = true,
        }
    }

    async fn do_start(&mut self) {
        let cfg = match Config::load() {
            Ok(c) => c,
            Err(e) => {
                self.flash = Some(format!("config error: {e}"));
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
                self.flash = Some(format!("started `{}`", s.profile));
            }
            Ok(Response::Error { message }) => self.flash = Some(message),
            Ok(_) => self.flash = Some("unexpected response".into()),
            Err(e) => self.flash = Some(e.to_string()),
        }
    }

    async fn do_stop(&mut self) {
        match ipc::send(&Request::Stop { id: None }).await {
            Ok(Response::Session(s)) => self.flash = Some(format!("stopped `{}`", s.profile)),
            Ok(Response::HardModeActive(_)) => {
                self.flash = Some("hard mode active — stop denied".into())
            }
            Ok(Response::Error { message }) => self.flash = Some(message),
            Ok(_) => self.flash = Some("nothing to stop".into()),
            Err(e) => self.flash = Some(e.to_string()),
        }
    }

    async fn do_panic(&mut self) {
        match ipc::send(&Request::Panic { phrase: String::new(), cancel: false }).await {
            Ok(Response::PanicScheduled(info)) => {
                self.flash = Some(match info.panic_releases_at {
                    Some(at) => format!("panic release at {}", at.to_rfc3339()),
                    None => "panic cancelled".into(),
                });
            }
            Ok(Response::Error { message }) => self.flash = Some(message),
            Ok(_) => self.flash = Some("no hard-mode session".into()),
            Err(e) => self.flash = Some(e.to_string()),
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
        app.frame = ticks;
        terminal.draw(|f| super::view::draw(f, &app))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                        KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                        KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                        KeyCode::Enter | KeyCode::Char(' ') => app.activate().await,
                        KeyCode::Char('s') => {
                            app.selected = 0;
                            app.activate().await;
                        }
                        KeyCode::Char('x') => {
                            app.selected = 1;
                            app.activate().await;
                        }
                        KeyCode::Char('p') => {
                            app.selected = 2;
                            app.activate().await;
                        }
                        _ => {}
                    }
                }
            }
        }

        ticks = ticks.wrapping_add(1);
        if app.should_quit {
            break;
        }
    }
    Ok(())
}
