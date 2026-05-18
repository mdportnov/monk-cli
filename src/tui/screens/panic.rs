use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::tui::{
    app::{App, HomeState, Screen},
    theme::*,
    widgets::TextInput,
};

#[derive(Debug)]
pub struct PanicState {
    pub expected: String,
    pub input: TextInput,
    pub error: Option<String>,
}

impl PanicState {
    pub fn new(expected: String) -> Self {
        let mut input = TextInput::new(String::new());
        input.focused = true;
        Self { expected, input, error: None }
    }
}

pub async fn handle_panic_key(app: &mut App, key: KeyEvent) {
    let Screen::Panic(st) = &mut app.screen else { return };
    match key.code {
        KeyCode::Esc => {
            app.set_screen(Screen::Home(HomeState::default()));
        }
        KeyCode::Enter => {
            let typed = st.input.value.trim().to_string();
            if typed != st.expected {
                st.error = Some(crate::i18n::t!("tui.panic_modal.mismatch").to_string());
                return;
            }
            app.submit_panic(typed).await;
        }
        _ => {
            st.input.handle(key);
            st.error = None;
        }
    }
}

pub fn draw_panic(f: &mut Frame, _app: &App, st: &PanicState) {
    let area = f.area();
    let width = 60.min(area.width.saturating_sub(4));
    let height = 13u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let rect = Rect { x, y, width, height };

    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ALERT))
        .title(Span::styled(" panic ", Style::default().fg(ALERT).add_modifier(Modifier::BOLD)));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        crate::i18n::t!("tui.panic_modal.prompt").to_string(),
        Style::default().fg(DIM),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        st.expected.clone(),
        Style::default().fg(GLOW).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    if let Some(err) = &st.error {
        lines.push(Line::from(Span::styled(
            err.clone(),
            Style::default().fg(ALERT),
        )));
    } else {
        lines.push(Line::from(""));
    }
    let para = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    let layout = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(5),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
        ])
        .split(inner);

    f.render_widget(para, layout[0]);
    st.input.render(layout[1], f.buffer_mut(), Style::default().fg(TEXT));

    let hint = Paragraph::new(Span::styled(
        "enter confirm · esc cancel",
        Style::default().fg(DIM),
    ))
    .alignment(Alignment::Center);
    f.render_widget(hint, layout[3]);
}
