use std::{io, time::Duration};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    ipc::{self, HardModeInfo, Request, Response},
    session::Session,
    Result,
};

#[derive(Debug, Default)]
pub struct App {
    pub active: Option<Session>,
    pub hard_mode: Option<HardModeInfo>,
    pub should_quit: bool,
}

impl App {
    pub async fn refresh(&mut self) {
        if let Ok(Response::Status { active, hard_mode, .. }) = ipc::send(&Request::Status).await {
            self.active = active.map(|b| *b);
            self.hard_mode = hard_mode.map(|b| *b);
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
    let mut app = App::default();

    loop {
        app.refresh().await;
        terminal.draw(|f| super::view::draw(f, &app))?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                        KeyCode::Char('x') if app.hard_mode.is_some() => {}
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
