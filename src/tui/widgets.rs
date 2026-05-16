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
    /// Pre-computed lowercase label. Avoids re-allocating on every filter pass
    /// (visible_indices is called per render + per keystroke).
    label_lower: String,
}

impl MultiSelectItem {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        let label = label.into();
        let label_lower = label.to_lowercase();
        Self { id: id.into(), label, label_lower }
    }
}

#[derive(Debug, Default)]
pub struct MultiSelectList {
    pub items: Vec<MultiSelectItem>,
    pub selected: BTreeSet<usize>,
    pub cursor: usize,
    pub filter: String,
    /// Cached `filter.to_lowercase()`; kept in sync with `filter`.
    filter_lower: String,
    /// Memoized visible indices for the current `filter_lower`.
    visible_cache: Vec<usize>,
    /// True when `visible_cache` matches the current filter.
    visible_dirty: bool,
}

impl MultiSelectList {
    pub fn new(items: Vec<MultiSelectItem>, preselected: &[String]) -> Self {
        let selected = items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| preselected.contains(&it.id).then_some(i))
            .collect();
        let visible_cache = (0..items.len()).collect();
        Self {
            items,
            selected,
            cursor: 0,
            filter: String::new(),
            filter_lower: String::new(),
            visible_cache,
            visible_dirty: false,
        }
    }

    pub fn filter_clear(&mut self) {
        if !self.filter.is_empty() {
            self.filter.clear();
            self.filter_lower.clear();
            self.visible_dirty = true;
            self.clamp_cursor();
        }
    }

    pub fn handle(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return false;
        }
        match key.code {
            KeyCode::Char(' ') | KeyCode::Enter => {
                self.rebuild_visible_if_dirty();
                if !self.visible_cache.is_empty() {
                    self.toggle(self.cursor);
                    return true;
                }
                return false;
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.filter_lower.extend(c.to_lowercase());
                self.visible_dirty = true;
                self.clamp_cursor();
                return true;
            }
            KeyCode::Backspace => {
                if self.filter.pop().is_some() {
                    self.filter_lower = self.filter.to_lowercase();
                    self.visible_dirty = true;
                    self.clamp_cursor();
                    return true;
                }
                return false;
            }
            _ => {}
        }
        self.rebuild_visible_if_dirty();
        if self.visible_cache.is_empty() {
            return false;
        }
        match key.code {
            KeyCode::Up => {
                let v = &self.visible_cache;
                let pos = v.iter().position(|&i| i == self.cursor);
                self.cursor = match pos {
                    Some(p) => v[p.checked_sub(1).unwrap_or(v.len() - 1)],
                    None => v[0],
                };
                false
            }
            KeyCode::Down => {
                let v = &self.visible_cache;
                let pos = v.iter().position(|&i| i == self.cursor);
                self.cursor = match pos {
                    Some(p) => v[(p + 1) % v.len()],
                    None => v[0],
                };
                false
            }
            _ => false,
        }
    }

    fn clamp_cursor(&mut self) {
        self.rebuild_visible_if_dirty();
        if self.visible_cache.is_empty() {
            return;
        }
        if !self.visible_cache.contains(&self.cursor) {
            self.cursor = self.visible_cache[0];
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

    fn rebuild_visible_if_dirty(&mut self) {
        if !self.visible_dirty {
            return;
        }
        self.visible_cache.clear();
        if self.filter_lower.is_empty() {
            self.visible_cache.extend(0..self.items.len());
        } else {
            for (i, it) in self.items.iter().enumerate() {
                if it.label_lower.contains(&self.filter_lower) {
                    self.visible_cache.push(i);
                }
            }
        }
        self.visible_dirty = false;
    }

    pub fn visible_indices(&self) -> &[usize] {
        // Read path used by render. Render takes &self; we accept a tiny
        // inconsistency window: if dirty, fall back to a fresh scan into a
        // throwaway buffer through the mutable cache via interior mutability
        // would complicate this — instead callers (handle/clamp) keep cache
        // fresh, and render observes the last good cache. The widget is
        // single-threaded so a stale read is at worst one frame.
        &self.visible_cache
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, block: Block<'_>, focused: bool) {
        let v = &self.visible_cache;
        // Defensive: cache may be stale if `items` was mutated directly
        // (e.g. cleared) without going through the widget API.
        let items: Vec<ListItem> = v
            .iter()
            .filter_map(|&i| {
                let it = self.items.get(i)?;
                let mark = if self.selected.contains(&i) { "[x]" } else { "[ ]" };
                Some(ListItem::new(Line::from(vec![
                    Span::raw(format!(" {mark} ")),
                    Span::raw(it.label.clone()),
                ])))
            })
            .collect();
        let highlight = if focused {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        };
        let list = List::new(items).block(block).highlight_style(highlight).highlight_symbol("▶ ");
        let mut state = ListState::default();
        let pos = v.iter().position(|&i| i == self.cursor);
        state.select(pos);
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
            MultiSelectItem::new("a", "Instagram"),
            MultiSelectItem::new("b", "Facebook"),
            MultiSelectItem::new("c", "Telegram"),
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
        let items = vec![MultiSelectItem::new("a", "Alpha"), MultiSelectItem::new("b", "Beta")];
        let mut m = MultiSelectList::new(items, &["a".to_string()]);
        assert_eq!(m.selected_ids(), vec!["a".to_string()]);
        m.handle(k(KeyCode::Char(' ')));
        assert!(m.selected_ids().is_empty());
        m.handle(k(KeyCode::Down));
        m.handle(k(KeyCode::Char(' ')));
        assert_eq!(m.selected_ids(), vec!["b".to_string()]);
    }
}
