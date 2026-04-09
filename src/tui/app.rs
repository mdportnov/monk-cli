use std::{io, time::Duration};

use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    config::{Config, Hooks, Limits, Profile},
    ipc::{self, HardModeInfo, ModeSummary, Request, Response},
    session::Session,
    tui::widgets::{MultiSelectItem, MultiSelectList, TextInput},
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorField {
    Name,
    Max,
    Min,
    Cooldown,
    DailyCap,
    Sites,
    HookBefore,
    HookAfter,
    Apps,
    Groups,
}

impl EditorField {
    pub const ORDER: [EditorField; 10] = [
        EditorField::Name,
        EditorField::Max,
        EditorField::Min,
        EditorField::Cooldown,
        EditorField::DailyCap,
        EditorField::Sites,
        EditorField::HookBefore,
        EditorField::HookAfter,
        EditorField::Apps,
        EditorField::Groups,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EditorField::Name => "name",
            EditorField::Max => "max duration",
            EditorField::Min => "min duration",
            EditorField::Cooldown => "cooldown",
            EditorField::DailyCap => "daily cap",
            EditorField::Sites => "custom sites",
            EditorField::HookBefore => "hook before",
            EditorField::HookAfter => "hook after",
            EditorField::Apps => "blocked apps",
            EditorField::Groups => "site groups",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            EditorField::Name => "mode identifier (1-30 chars)",
            EditorField::Max => "ceiling — even you can't override (e.g. 2h, 90m)",
            EditorField::Min => "shorter doesn't count as a session",
            EditorField::Cooldown => "protects against compulsive restart",
            EditorField::DailyCap => "daily focus budget — prevents burnout",
            EditorField::Sites => "comma-separated hosts to block",
            EditorField::HookBefore => "shell command run before session",
            EditorField::HookAfter => "shell command run after session",
            EditorField::Apps => "space to toggle, ↑/↓ to navigate",
            EditorField::Groups => "preset site groups",
        }
    }
}

#[derive(Debug)]
pub struct EditorState {
    pub original_name: Option<String>,
    pub name: TextInput,
    pub max: TextInput,
    pub min: TextInput,
    pub cooldown: TextInput,
    pub daily_cap: TextInput,
    pub sites: TextInput,
    pub hook_before: TextInput,
    pub hook_after: TextInput,
    pub apps: MultiSelectList,
    pub groups: MultiSelectList,
    pub focus: EditorField,
    pub snapshot: Profile,
    pub error: Option<String>,
    pub confirm_cancel: bool,
}

impl EditorState {
    pub fn new_mode() -> Self {
        let (apps, groups) = load_picklists(&Profile::default());
        let mut s = Self {
            original_name: None,
            name: TextInput::new(""),
            max: TextInput::new(""),
            min: TextInput::new(""),
            cooldown: TextInput::new(""),
            daily_cap: TextInput::new(""),
            sites: TextInput::new(""),
            hook_before: TextInput::new(""),
            hook_after: TextInput::new(""),
            apps,
            groups,
            focus: EditorField::Name,
            snapshot: Profile::default(),
            error: None,
            confirm_cancel: false,
        };
        s.sync_focus();
        s
    }

    pub fn edit(name: String, profile: Profile) -> Self {
        let (apps, groups) = load_picklists(&profile);
        let limits = profile.limits.clone();
        let mut s = Self {
            original_name: Some(name.clone()),
            name: TextInput::new(name),
            max: TextInput::new(fmt_opt_humantime(limits.max_duration)),
            min: TextInput::new(fmt_opt_humantime(limits.min_duration)),
            cooldown: TextInput::new(fmt_opt_humantime(limits.cooldown)),
            daily_cap: TextInput::new(fmt_opt_humantime(limits.daily_cap)),
            sites: TextInput::new(profile.sites.join(", ")),
            hook_before: TextInput::new(profile.hooks.before.join(" && ")),
            hook_after: TextInput::new(profile.hooks.after.join(" && ")),
            apps,
            groups,
            focus: EditorField::Name,
            snapshot: profile,
            error: None,
            confirm_cancel: false,
        };
        s.sync_focus();
        s
    }

    pub fn input_mut(&mut self, f: EditorField) -> Option<&mut TextInput> {
        match f {
            EditorField::Name => Some(&mut self.name),
            EditorField::Max => Some(&mut self.max),
            EditorField::Min => Some(&mut self.min),
            EditorField::Cooldown => Some(&mut self.cooldown),
            EditorField::DailyCap => Some(&mut self.daily_cap),
            EditorField::Sites => Some(&mut self.sites),
            EditorField::HookBefore => Some(&mut self.hook_before),
            EditorField::HookAfter => Some(&mut self.hook_after),
            _ => None,
        }
    }

    pub fn next_field(&mut self) {
        let idx = EditorField::ORDER.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = EditorField::ORDER[(idx + 1) % EditorField::ORDER.len()];
        self.sync_focus();
    }

    pub fn prev_field(&mut self) {
        let idx = EditorField::ORDER.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus =
            EditorField::ORDER[(idx + EditorField::ORDER.len() - 1) % EditorField::ORDER.len()];
        self.sync_focus();
    }

    fn sync_focus(&mut self) {
        let focus = self.focus;
        for f in EditorField::ORDER {
            let is = f == focus;
            if let Some(inp) = self.input_mut(f) {
                inp.focused = is;
            }
        }
    }

    pub fn build_profile(&self) -> std::result::Result<(String, Profile), String> {
        let name = self.name.value.trim().to_string();
        if name.is_empty() {
            return Err("name is required".into());
        }
        if name.chars().count() > 30 {
            return Err("name must be ≤ 30 chars".into());
        }
        let limits = Limits {
            max_duration: parse_opt_humantime(&self.max)?,
            min_duration: parse_opt_humantime(&self.min)?,
            cooldown: parse_opt_humantime(&self.cooldown)?,
            daily_cap: parse_opt_humantime(&self.daily_cap)?,
        };
        if let (Some(mn), Some(mx)) = (limits.min_duration, limits.max_duration) {
            if mn > mx {
                return Err("min must be ≤ max".into());
            }
        }
        let sites: Vec<String> = self
            .sites
            .value
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        let hooks = Hooks {
            before: split_cmds(&self.hook_before.value),
            after: split_cmds(&self.hook_after.value),
        };
        let profile = Profile {
            sites,
            site_groups: self.groups.selected_ids(),
            apps: self.apps.selected_ids(),
            allow: self.snapshot.allow.clone(),
            hooks,
            limits,
            color: self.snapshot.color.clone(),
        };
        Ok((name, profile))
    }

    pub fn is_dirty(&self) -> bool {
        match self.build_profile() {
            Ok((name, p)) => {
                if self.original_name.as_deref() != Some(name.as_str())
                    && self.original_name.is_some()
                {
                    return true;
                }
                if self.original_name.is_none() {
                    return true;
                }
                !profile_eq(&p, &self.snapshot)
            }
            Err(_) => true,
        }
    }
}

fn fmt_opt_humantime(d: Option<Duration>) -> String {
    match d {
        Some(v) => humantime::format_duration(v).to_string(),
        None => String::new(),
    }
}

fn parse_opt_humantime(input: &TextInput) -> std::result::Result<Option<Duration>, String> {
    let raw = input.value.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    humantime::parse_duration(raw)
        .map(Some)
        .map_err(|e| format!("invalid duration: {e}"))
}

fn split_cmds(raw: &str) -> Vec<String> {
    raw.split("&&")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn profile_eq(a: &Profile, b: &Profile) -> bool {
    a.sites == b.sites
        && a.site_groups == b.site_groups
        && a.apps == b.apps
        && a.allow == b.allow
        && a.hooks.before == b.hooks.before
        && a.hooks.after == b.hooks.after
        && a.limits.max_duration == b.limits.max_duration
        && a.limits.min_duration == b.limits.min_duration
        && a.limits.cooldown == b.limits.cooldown
        && a.limits.daily_cap == b.limits.daily_cap
        && a.color == b.color
}

fn load_picklists(profile: &Profile) -> (MultiSelectList, MultiSelectList) {
    let apps_items = crate::apps::load_or_scan(false)
        .map(|cache| {
            cache
                .apps
                .into_iter()
                .map(|a| MultiSelectItem { id: a.id.clone(), label: format!("{} [{}]", a.label, a.id) })
                .collect()
        })
        .unwrap_or_default();
    let groups_items = crate::sites::all_groups()
        .map(|gs| {
            gs.into_iter()
                .map(|g| MultiSelectItem {
                    id: g.qualified(),
                    label: format!("{} ({} hosts)", g.qualified(), g.hosts.len()),
                })
                .collect()
        })
        .unwrap_or_default();
    (
        MultiSelectList::new(apps_items, &profile.apps),
        MultiSelectList::new(groups_items, &profile.site_groups),
    )
}

#[derive(Debug)]
pub enum Screen {
    Home(HomeState),
    ModePicker(PickerState),
    ModeConfirm(Box<ConfirmState>),
    ModeEditor(Box<EditorState>),
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
    pub active_mode: Option<ModeSummary>,
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
                self.globals.active_mode = None;
            }
        }
        if let Some(session) = &self.globals.active {
            if self
                .globals
                .active_mode
                .as_ref()
                .map(|m| m.name != session.profile)
                .unwrap_or(true)
            {
                if let Ok(Response::Modes(modes)) = ipc::send(&Request::ListModes).await {
                    self.globals.active_mode =
                        modes.into_iter().find(|m| m.name == session.profile);
                }
            }
        } else {
            self.globals.active_mode = None;
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
            Screen::ModeEditor(_) => self.handle_editor_key(key).await,
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
            KeyCode::Char('n') => self.open_editor_new(),
            KeyCode::Char('e') => self.open_editor_edit(),
            KeyCode::Char('d') => self.delete_current_mode().await,
            _ => {}
        }
    }

    fn open_editor_new(&mut self) {
        self.screen = Screen::ModeEditor(Box::new(EditorState::new_mode()));
    }

    fn open_editor_edit(&mut self) {
        let Screen::ModePicker(picker) = &self.screen else { return };
        let Some(mode) = picker.current() else { return };
        let cfg = match Config::load() {
            Ok(c) => c,
            Err(e) => {
                if let Screen::ModePicker(p) = &mut self.screen {
                    p.error = Some(e.to_string());
                }
                return;
            }
        };
        let profile = cfg.profiles.get(&mode.name).cloned().unwrap_or_default();
        self.screen = Screen::ModeEditor(Box::new(EditorState::edit(mode.name.clone(), profile)));
    }

    async fn delete_current_mode(&mut self) {
        let name = {
            let Screen::ModePicker(picker) = &self.screen else { return };
            match picker.current() {
                Some(m) => m.name.clone(),
                None => return,
            }
        };
        match ipc::send(&Request::DeleteMode { name: name.clone() }).await {
            Ok(Response::Ok) => {
                self.globals.flash = Some(format!("deleted `{name}`"));
                self.refresh_picker().await;
            }
            Ok(Response::Error { message }) => {
                if let Screen::ModePicker(p) = &mut self.screen {
                    p.error = Some(message);
                }
            }
            Ok(_) => {}
            Err(e) => {
                if let Screen::ModePicker(p) = &mut self.screen {
                    p.error = Some(e.to_string());
                }
            }
        }
    }

    async fn handle_editor_key(&mut self, key: KeyEvent) {
        let Screen::ModeEditor(ed) = &mut self.screen else { return };
        if ed.confirm_cancel {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.open_picker().await;
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    ed.confirm_cancel = false;
                }
                _ => {}
            }
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('s')) {
            self.save_editor().await;
            return;
        }
        match key.code {
            KeyCode::Esc => {
                if ed.is_dirty() {
                    ed.confirm_cancel = true;
                } else {
                    self.open_picker().await;
                }
            }
            KeyCode::Tab => ed.next_field(),
            KeyCode::BackTab => ed.prev_field(),
            _ => {
                let focus = ed.focus;
                match focus {
                    EditorField::Apps => {
                        ed.apps.handle(key);
                    }
                    EditorField::Groups => {
                        ed.groups.handle(key);
                    }
                    _ => {
                        if let Some(input) = ed.input_mut(focus) {
                            input.handle(key);
                        }
                    }
                }
                ed.error = None;
            }
        }
    }

    async fn save_editor(&mut self) {
        let (name, profile) = {
            let Screen::ModeEditor(ed) = &mut self.screen else { return };
            match ed.build_profile() {
                Ok(v) => v,
                Err(e) => {
                    ed.error = Some(e);
                    return;
                }
            }
        };
        match ipc::send(&Request::SaveMode { name: name.clone(), profile }).await {
            Ok(Response::Ok) => {
                self.globals.flash = Some(format!("saved `{name}`"));
                self.open_picker().await;
            }
            Ok(Response::Error { message }) => {
                if let Screen::ModeEditor(ed) = &mut self.screen {
                    ed.error = Some(message);
                }
            }
            Ok(_) => {}
            Err(e) => {
                if let Screen::ModeEditor(ed) = &mut self.screen {
                    ed.error = Some(e.to_string());
                }
            }
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
