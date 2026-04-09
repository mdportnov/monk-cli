#![allow(dead_code)]

use std::collections::BTreeSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph, StatefulWidget, Widget},
};

#[derive(Debug, Default, Clone)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize,
    pub focused: bool,
}

impl TextInput {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.chars().count();
        Self { value, cursor, focused: false }
    }

    pub fn set(&mut self, value: impl Into<String>) {
        self.value = value.into();
        self.cursor = self.value.chars().count();
    }

    pub fn handle(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert(c);
                true
            }
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                false
            }
            KeyCode::Right => {
                if self.cursor < self.value.chars().count() {
                    self.cursor += 1;
                }
                false
            }
            KeyCode::Home => {
                self.cursor = 0;
                false
            }
            KeyCode::End => {
                self.cursor = self.value.chars().count();
                false
            }
            _ => false,
        }
    }

    fn insert(&mut self, c: char) {
        let byte = self.char_to_byte(self.cursor);
        self.value.insert(byte, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let end = self.char_to_byte(self.cursor);
        let start = self.char_to_byte(self.cursor - 1);
        self.value.replace_range(start..end, "");
        self.cursor -= 1;
        true
    }

    fn delete(&mut self) -> bool {
        let n = self.value.chars().count();
        if self.cursor >= n {
            return false;
        }
        let start = self.char_to_byte(self.cursor);
        let end = self.char_to_byte(self.cursor + 1);
        self.value.replace_range(start..end, "");
        true
    }

    fn char_to_byte(&self, idx: usize) -> usize {
        self.value.char_indices().nth(idx).map(|(b, _)| b).unwrap_or(self.value.len())
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, style: Style) {
        let mut spans: Vec<Span> = Vec::new();
        let chars: Vec<char> = self.value.chars().collect();
        for (i, ch) in chars.iter().enumerate() {
            if self.focused && i == self.cursor {
                spans.push(Span::styled(ch.to_string(), style.add_modifier(Modifier::REVERSED)));
            } else {
                spans.push(Span::styled(ch.to_string(), style));
            }
        }
        if self.focused && self.cursor == chars.len() {
            spans.push(Span::styled(" ", style.add_modifier(Modifier::REVERSED)));
        } else if chars.is_empty() {
            spans.push(Span::styled(" ", style));
        }
        Paragraph::new(Line::from(spans)).render(area, buf);
    }
}

#[derive(Debug, Clone)]
pub struct MultiSelectItem {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Default)]
pub struct MultiSelectList {
    pub items: Vec<MultiSelectItem>,
    pub selected: BTreeSet<usize>,
    pub cursor: usize,
    pub filter: String,
}

impl MultiSelectList {
    pub fn new(items: Vec<MultiSelectItem>, preselected: &[String]) -> Self {
        let selected = items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| preselected.contains(&it.id).then_some(i))
            .collect();
        Self { items, selected, cursor: 0, filter: String::new() }
    }

    pub fn handle(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return false;
        }
        match key.code {
            KeyCode::Char(' ') => {
                let visible = self.visible_indices();
                if !visible.is_empty() {
                    self.toggle(self.cursor);
                    return true;
                }
                return false;
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.clamp_cursor();
                return true;
            }
            KeyCode::Backspace => {
                if self.filter.pop().is_some() {
                    self.clamp_cursor();
                    return true;
                }
                return false;
            }
            _ => {}
        }
        let visible = self.visible_indices();
        if visible.is_empty() {
            return false;
        }
        match key.code {
            KeyCode::Up => {
                if let Some(pos) = visible.iter().position(|&i| i == self.cursor) {
                    self.cursor = visible[pos.checked_sub(1).unwrap_or(visible.len() - 1)];
                } else {
                    self.cursor = visible[0];
                }
                false
            }
            KeyCode::Down => {
                if let Some(pos) = visible.iter().position(|&i| i == self.cursor) {
                    self.cursor = visible[(pos + 1) % visible.len()];
                } else {
                    self.cursor = visible[0];
                }
                false
            }
            _ => false,
        }
    }

    fn clamp_cursor(&mut self) {
        let visible = self.visible_indices();
        if visible.is_empty() {
            return;
        }
        if !visible.contains(&self.cursor) {
            self.cursor = visible[0];
        }
    }

    pub fn toggle(&mut self, idx: usize) {
        if !self.selected.insert(idx) {
            self.selected.remove(&idx);
        }
    }

    pub fn selected_ids(&self) -> Vec<String> {
        self.selected.iter().filter_map(|i| self.items.get(*i).map(|it| it.id.clone())).collect()
    }

    fn visible_indices(&self) -> Vec<usize> {
        if self.filter.is_empty() {
            return (0..self.items.len()).collect();
        }
        let needle = self.filter.to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.label.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, block: Block<'_>, focused: bool) {
        let items: Vec<ListItem> = self
            .visible_indices()
            .into_iter()
            .map(|i| {
                let it = &self.items[i];
                let mark = if self.selected.contains(&i) { "[x]" } else { "[ ]" };
                ListItem::new(Line::from(vec![
                    Span::raw(format!(" {mark} ")),
                    Span::raw(it.label.clone()),
                ]))
            })
            .collect();
        let highlight = if focused {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        };
        let list = List::new(items).block(block).highlight_style(highlight).highlight_symbol("▶ ");
        let mut state = ListState::default();
        let visible = self.visible_indices();
        let pos = visible.iter().position(|&i| i == self.cursor).unwrap_or(0);
        state.select(Some(pos));
        StatefulWidget::render(list, area, buf, &mut state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    fn k(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::empty())
    }

    #[test]
    fn text_input_insert_and_cursor() {
        let mut t = TextInput::new("");
        t.handle(k(KeyCode::Char('a')));
        t.handle(k(KeyCode::Char('b')));
        t.handle(k(KeyCode::Char('c')));
        assert_eq!(t.value, "abc");
        assert_eq!(t.cursor, 3);
        t.handle(k(KeyCode::Left));
        t.handle(k(KeyCode::Backspace));
        assert_eq!(t.value, "ac");
        assert_eq!(t.cursor, 1);
    }

    #[test]
    fn text_input_unicode() {
        let mut t = TextInput::new("");
        t.handle(k(KeyCode::Char('м')));
        t.handle(k(KeyCode::Char('и')));
        assert_eq!(t.value, "ми");
        t.handle(k(KeyCode::Left));
        t.handle(k(KeyCode::Backspace));
        assert_eq!(t.value, "и");
    }

    #[test]
    fn multi_select_filter_typing() {
        let items = vec![
            MultiSelectItem { id: "a".into(), label: "Instagram".into() },
            MultiSelectItem { id: "b".into(), label: "Facebook".into() },
            MultiSelectItem { id: "c".into(), label: "Telegram".into() },
        ];
        let mut m = MultiSelectList::new(items, &[]);
        m.handle(k(KeyCode::Char('t')));
        m.handle(k(KeyCode::Char('e')));
        assert_eq!(m.filter, "te");
        let vis: Vec<_> = m.visible_indices().iter().map(|i| m.items[*i].id.clone()).collect();
        assert_eq!(vis, vec!["c".to_string()]);
        m.handle(k(KeyCode::Backspace));
        assert_eq!(m.filter, "t");
    }

    #[test]
    fn multi_select_toggle() {
        let items = vec![
            MultiSelectItem { id: "a".into(), label: "Alpha".into() },
            MultiSelectItem { id: "b".into(), label: "Beta".into() },
        ];
        let mut m = MultiSelectList::new(items, &["a".to_string()]);
        assert_eq!(m.selected_ids(), vec!["a".to_string()]);
        m.handle(k(KeyCode::Char(' ')));
        assert!(m.selected_ids().is_empty());
        m.handle(k(KeyCode::Down));
        m.handle(k(KeyCode::Char(' ')));
        assert_eq!(m.selected_ids(), vec!["b".to_string()]);
    }
}
