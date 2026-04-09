use std::{io, time::Duration};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
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
    Settings,
    Doctor,
    Quit,
}

impl MenuItem {
    pub const ALL: [MenuItem; 7] = [
        MenuItem::Start,
        MenuItem::Stop,
        MenuItem::Panic,
        MenuItem::Profiles,
        MenuItem::Settings,
        MenuItem::Doctor,
        MenuItem::Quit,
    ];

    pub fn label(self) -> String {
        let key = match self {
            MenuItem::Start => "tui.menu.start",
            MenuItem::Stop => "tui.menu.stop",
            MenuItem::Panic => "tui.menu.panic",
            MenuItem::Profiles => "tui.menu.modes",
            MenuItem::Settings => "tui.menu.settings",
            MenuItem::Doctor => "tui.menu.doctor",
            MenuItem::Quit => "tui.menu.quit",
        };
        crate::i18n::t!(key).to_string()
    }

    pub fn hint(self) -> &'static str {
        match self {
            MenuItem::Start => "begin a focus session with the default mode",
            MenuItem::Stop => "end the active session (soft mode only)",
            MenuItem::Panic => "request a delayed hard-mode escape",
            MenuItem::Profiles => "list configured modes",
            MenuItem::Settings => "general settings and data reset",
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
        self.requested = self.requested.checked_sub(Self::STEP).unwrap_or(Self::MIN_BOUND);
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
    Color,
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
    pub const ORDER: [EditorField; 11] = [
        EditorField::Name,
        EditorField::Color,
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
            EditorField::Color => "color",
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
            EditorField::Color => "← → cycle accent color",
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

pub const COLOR_PALETTE: &[(&str, &str)] = &[
    ("none", "—"),
    ("blue", "blue"),
    ("cyan", "cyan"),
    ("green", "green"),
    ("amber", "amber"),
    ("violet", "violet"),
    ("red", "red"),
];

pub fn palette_index(color: &Option<String>) -> usize {
    let key = color.as_deref().unwrap_or("none");
    COLOR_PALETTE.iter().position(|(k, _)| *k == key).unwrap_or(0)
}

pub fn palette_value(idx: usize) -> Option<String> {
    let (k, _) = COLOR_PALETTE[idx % COLOR_PALETTE.len()];
    if k == "none" {
        None
    } else {
        Some(k.to_string())
    }
}

#[derive(Debug)]
pub struct EditorState {
    pub original_name: Option<String>,
    pub name: TextInput,
    pub color_idx: usize,
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
            color_idx: 0,
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
        let color_idx = palette_index(&profile.color);
        let mut s = Self {
            original_name: Some(name.clone()),
            name: TextInput::new(name),
            color_idx,
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
            color: palette_value(self.color_idx),
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
    humantime::parse_duration(raw).map(Some).map_err(|e| format!("invalid duration: {e}"))
}

fn split_cmds(raw: &str) -> Vec<String> {
    raw.split("&&").map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
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
                .map(|a| MultiSelectItem {
                    id: a.id.clone(),
                    label: format!("{} [{}]", a.label, a.id),
                })
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsField {
    DefaultProfile,
    DefaultDuration,
    HardMode,
    Autostart,
    Locale,
    PanicDelay,
    TamperPenalty,
    Reset,
}

impl SettingsField {
    pub const ORDER: [SettingsField; 8] = [
        SettingsField::DefaultProfile,
        SettingsField::DefaultDuration,
        SettingsField::HardMode,
        SettingsField::Autostart,
        SettingsField::Locale,
        SettingsField::PanicDelay,
        SettingsField::TamperPenalty,
        SettingsField::Reset,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsField::DefaultProfile => "default profile",
            SettingsField::DefaultDuration => "default duration",
            SettingsField::HardMode => "hard mode by default",
            SettingsField::Autostart => "autostart at login",
            SettingsField::Locale => "locale",
            SettingsField::PanicDelay => "panic delay",
            SettingsField::TamperPenalty => "tamper penalty",
            SettingsField::Reset => "reset all data",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            SettingsField::DefaultProfile => "mode used when you press Start",
            SettingsField::DefaultDuration => "e.g. 25m, 50m, 1h30m",
            SettingsField::HardMode => "space to toggle",
            SettingsField::Autostart => "space to toggle",
            SettingsField::Locale => "← → en / ru",
            SettingsField::PanicDelay => "delay before panic releases hard-mode",
            SettingsField::TamperPenalty => "time added to session on tamper",
            SettingsField::Reset => "enter to wipe config and audit log",
        }
    }
}

#[derive(Debug)]
pub struct SettingsState {
    pub original: crate::config::General,
    pub default_profile: TextInput,
    pub default_duration: TextInput,
    pub hard_mode: bool,
    pub autostart: bool,
    pub locale_idx: usize,
    pub panic_delay: TextInput,
    pub tamper_penalty: TextInput,
    pub focus: SettingsField,
    pub error: Option<String>,
    pub confirm_reset: bool,
}

pub const LOCALES: &[&str] = &["en", "ru"];

impl SettingsState {
    pub fn from_general(g: crate::config::General) -> Self {
        let locale_idx =
            g.locale.as_deref().and_then(|l| LOCALES.iter().position(|x| *x == l)).unwrap_or(0);
        let mut s = Self {
            default_profile: TextInput::new(g.default_profile.clone()),
            default_duration: TextInput::new(
                humantime::format_duration(g.default_duration).to_string(),
            ),
            hard_mode: g.hard_mode,
            autostart: g.autostart,
            locale_idx,
            panic_delay: TextInput::new(humantime::format_duration(g.panic_delay).to_string()),
            tamper_penalty: TextInput::new(
                humantime::format_duration(g.tamper_penalty).to_string(),
            ),
            focus: SettingsField::DefaultProfile,
            error: None,
            confirm_reset: false,
            original: g,
        };
        s.sync_focus();
        s
    }

    pub fn input_mut(&mut self, f: SettingsField) -> Option<&mut TextInput> {
        match f {
            SettingsField::DefaultProfile => Some(&mut self.default_profile),
            SettingsField::DefaultDuration => Some(&mut self.default_duration),
            SettingsField::PanicDelay => Some(&mut self.panic_delay),
            SettingsField::TamperPenalty => Some(&mut self.tamper_penalty),
            _ => None,
        }
    }

    pub fn next_field(&mut self) {
        let idx = SettingsField::ORDER.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = SettingsField::ORDER[(idx + 1) % SettingsField::ORDER.len()];
        self.sync_focus();
    }

    pub fn prev_field(&mut self) {
        let idx = SettingsField::ORDER.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = SettingsField::ORDER
            [(idx + SettingsField::ORDER.len() - 1) % SettingsField::ORDER.len()];
        self.sync_focus();
    }

    fn sync_focus(&mut self) {
        let focus = self.focus;
        for f in SettingsField::ORDER {
            let is = f == focus;
            if let Some(inp) = self.input_mut(f) {
                inp.focused = is;
            }
        }
    }

    pub fn build(&self) -> std::result::Result<crate::config::General, String> {
        let mut g = self.original.clone();
        g.default_profile = self.default_profile.value.trim().to_string();
        if g.default_profile.is_empty() {
            return Err("default profile is required".into());
        }
        g.default_duration = humantime::parse_duration(self.default_duration.value.trim())
            .map_err(|e| format!("default duration: {e}"))?;
        g.panic_delay = humantime::parse_duration(self.panic_delay.value.trim())
            .map_err(|e| format!("panic delay: {e}"))?;
        g.tamper_penalty = humantime::parse_duration(self.tamper_penalty.value.trim())
            .map_err(|e| format!("tamper penalty: {e}"))?;
        g.hard_mode = self.hard_mode;
        g.autostart = self.autostart;
        g.locale = Some(LOCALES[self.locale_idx].to_string());
        Ok(g)
    }
}

#[derive(Debug)]
pub enum Screen {
    Home(HomeState),
    ModePicker(PickerState),
    ModeConfirm(Box<ConfirmState>),
    ModeEditor(Box<EditorState>),
    Settings(Box<SettingsState>),
}

impl Default for Screen {
    fn default() -> Self {
        Screen::Home(HomeState::default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashLevel {
    Info,
    Success,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct Flash {
    pub message: String,
    pub level: FlashLevel,
    pub expires_at: u64,
}

#[derive(Debug, Default)]
pub struct Globals {
    pub active: Option<Session>,
    pub hard_mode: Option<HardModeInfo>,
    pub daemon_running: bool,
    pub flash: Option<Flash>,
    pub frame: u64,
    pub active_mode: Option<ModeSummary>,
    pub help_open: bool,
    pub cached_modes: Vec<ModeSummary>,
}

impl Globals {
    pub fn set_flash(&mut self, message: impl Into<String>, level: FlashLevel) {
        let ttl_frames = match level {
            FlashLevel::Error => 40,
            FlashLevel::Warn => 30,
            _ => 20,
        };
        self.flash = Some(Flash {
            message: message.into(),
            level,
            expires_at: self.frame.saturating_add(ttl_frames),
        });
    }

    pub fn tick_flash(&mut self) {
        if let Some(f) = &self.flash {
            if self.frame >= f.expires_at {
                self.flash = None;
            }
        }
    }
}

#[derive(Default)]
pub struct App {
    pub screen: Screen,
    pub globals: Globals,
    pub should_quit: bool,
    pub effect: Option<tachyonfx::Effect>,
    pub last_effect_tick: Option<std::time::Instant>,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("screen", &self.screen)
            .field("globals", &self.globals)
            .field("should_quit", &self.should_quit)
            .finish()
    }
}

impl App {
    pub fn new() -> Self {
        let _ = Config::load();
        Self::default()
    }

    pub fn trigger_enter_effect(&mut self) {
        use tachyonfx::{fx, Interpolation, Motion};
        self.effect = Some(fx::sweep_in(
            Motion::LeftToRight,
            12,
            0,
            ratatui::style::Color::Rgb(30, 40, 60),
            tachyonfx::EffectTimer::from_ms(260, Interpolation::QuadOut),
        ));
        self.last_effect_tick = Some(std::time::Instant::now());
    }

    pub fn trigger_clamp_effect(&mut self) {
        use tachyonfx::{fx, Interpolation};
        self.effect =
            Some(fx::hsl_shift(Some([0.0, -40.0, 0.0]), None, (220, Interpolation::SineInOut)));
        self.last_effect_tick = Some(std::time::Instant::now());
    }

    fn set_screen(&mut self, screen: Screen) {
        self.screen = screen;
        self.trigger_enter_effect();
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
        if self.globals.daemon_running {
            if let Ok(Response::Modes { modes }) = ipc::send(&Request::ListModes).await {
                if let Some(session) = &self.globals.active {
                    self.globals.active_mode =
                        modes.iter().find(|m| m.name == session.profile).cloned();
                } else {
                    self.globals.active_mode = None;
                }
                self.globals.cached_modes = modes;
            }
        } else {
            self.globals.active_mode = None;
            self.globals.cached_modes.clear();
        }
    }

    pub async fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        if self.globals.help_open {
            if matches!(key.code, KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q')) {
                self.globals.help_open = false;
            }
            return;
        }
        if matches!(key.code, KeyCode::Char('?'))
            && !matches!(self.screen, Screen::ModeEditor(_) | Screen::Settings(_))
        {
            self.globals.help_open = true;
            return;
        }
        match &self.screen {
            Screen::Home(_) => self.handle_home_key(key).await,
            Screen::ModePicker(_) => self.handle_picker_key(key).await,
            Screen::ModeConfirm(_) => self.handle_confirm_key(key).await,
            Screen::ModeEditor(_) => self.handle_editor_key(key).await,
            Screen::Settings(_) => self.handle_settings_key(key).await,
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
            KeyCode::Char(c @ '1'..='9') => {
                let idx = (c as u8 - b'1') as usize;
                self.quick_start(idx).await;
            }
            _ => {}
        }
    }

    async fn handle_picker_key(&mut self, key: KeyEvent) {
        let Screen::ModePicker(picker) = &mut self.screen else { return };
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.set_screen(Screen::Home(HomeState::default()));
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
        self.set_screen(Screen::ModeEditor(Box::new(EditorState::new_mode())));
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
        self.set_screen(Screen::ModeEditor(Box::new(EditorState::edit(
            mode.name.clone(),
            profile,
        ))));
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
                self.globals.set_flash(
                    crate::i18n::t!("tui.flash.deleted", profile = name).to_string(),
                    FlashLevel::Success,
                );
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
                    EditorField::Color => {
                        let n = COLOR_PALETTE.len();
                        match key.code {
                            KeyCode::Left | KeyCode::Char('h') => {
                                ed.color_idx = (ed.color_idx + n - 1) % n;
                            }
                            KeyCode::Right | KeyCode::Char('l') => {
                                ed.color_idx = (ed.color_idx + 1) % n;
                            }
                            _ => {}
                        }
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
                self.globals.set_flash(
                    crate::i18n::t!("tui.flash.saved", profile = name).to_string(),
                    FlashLevel::Success,
                );
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
        self.set_screen(Screen::ModeConfirm(Box::new(ConfirmState::from_mode(
            mode,
            default_dur,
            hard,
        ))));
    }

    async fn handle_confirm_key(&mut self, key: KeyEvent) {
        let mut clamped = false;
        {
            let Screen::ModeConfirm(confirm) = &mut self.screen else { return };
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.open_picker().await;
                    return;
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    confirm.dec();
                    clamped = confirm.clamped;
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    confirm.inc();
                    clamped = confirm.clamped;
                }
                KeyCode::Char('H') => confirm.hard = !confirm.hard,
                KeyCode::Enter | KeyCode::Char(' ') => {
                    self.start_from_confirm().await;
                    return;
                }
                _ => {}
            }
        }
        if clamped {
            self.trigger_clamp_effect();
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
                self.globals.set_flash(
                    crate::i18n::t!("tui.flash.started", profile = s.profile).to_string(),
                    FlashLevel::Success,
                );
                self.set_screen(Screen::Home(HomeState::default()));
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

    async fn open_settings(&mut self) {
        match ipc::send(&Request::GetGeneral).await {
            Ok(Response::General(g)) => {
                self.set_screen(Screen::Settings(Box::new(SettingsState::from_general(g))));
            }
            Ok(Response::Error { message }) => {
                self.globals.set_flash(message, FlashLevel::Error);
            }
            Ok(_) => {
                self.globals.set_flash(
                    crate::i18n::t!("tui.flash.unexpected").to_string(),
                    FlashLevel::Error,
                );
            }
            Err(e) => self.globals.set_flash(e.to_string(), FlashLevel::Error),
        }
    }

    async fn handle_settings_key(&mut self, key: KeyEvent) {
        let Screen::Settings(st) = &mut self.screen else { return };
        if st.confirm_reset {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    st.confirm_reset = false;
                    self.reset_all().await;
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    st.confirm_reset = false;
                }
                _ => {}
            }
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('s')) {
            self.save_settings().await;
            return;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.set_screen(Screen::Home(HomeState::default()));
            }
            KeyCode::Tab | KeyCode::Down => st.next_field(),
            KeyCode::BackTab | KeyCode::Up => st.prev_field(),
            _ => {
                let focus = st.focus;
                match focus {
                    SettingsField::HardMode => {
                        if matches!(key.code, KeyCode::Char(' ') | KeyCode::Enter) {
                            st.hard_mode = !st.hard_mode;
                        }
                    }
                    SettingsField::Autostart => {
                        if matches!(key.code, KeyCode::Char(' ') | KeyCode::Enter) {
                            st.autostart = !st.autostart;
                        }
                    }
                    SettingsField::Locale => {
                        let n = LOCALES.len();
                        match key.code {
                            KeyCode::Left | KeyCode::Char('h') => {
                                st.locale_idx = (st.locale_idx + n - 1) % n;
                            }
                            KeyCode::Right | KeyCode::Char('l') => {
                                st.locale_idx = (st.locale_idx + 1) % n;
                            }
                            _ => {}
                        }
                    }
                    SettingsField::Reset => {
                        if matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
                            st.confirm_reset = true;
                        }
                    }
                    _ => {
                        if let Some(input) = st.input_mut(focus) {
                            input.handle(key);
                        }
                    }
                }
                st.error = None;
            }
        }
    }

    async fn save_settings(&mut self) {
        let general = {
            let Screen::Settings(st) = &mut self.screen else { return };
            match st.build() {
                Ok(g) => g,
                Err(e) => {
                    st.error = Some(e);
                    return;
                }
            }
        };
        let new_locale = general.locale.clone();
        match ipc::send(&Request::UpdateGeneral { general }).await {
            Ok(Response::Ok) => {
                if let Some(l) = new_locale {
                    crate::i18n::set(&l);
                }
                self.globals.set_flash(
                    crate::i18n::t!("tui.flash.settings_saved").to_string(),
                    FlashLevel::Success,
                );
                self.set_screen(Screen::Home(HomeState::default()));
            }
            Ok(Response::Error { message }) => {
                if let Screen::Settings(st) = &mut self.screen {
                    st.error = Some(message);
                }
            }
            Ok(_) => {}
            Err(e) => {
                if let Screen::Settings(st) = &mut self.screen {
                    st.error = Some(e.to_string());
                }
            }
        }
    }

    async fn reset_all(&mut self) {
        match ipc::send(&Request::ResetAll).await {
            Ok(Response::Ok) => {
                self.globals.set_flash(
                    crate::i18n::t!("tui.flash.reset_done").to_string(),
                    FlashLevel::Warn,
                );
                self.set_screen(Screen::Home(HomeState::default()));
            }
            Ok(Response::Error { message }) => {
                self.globals.set_flash(message, FlashLevel::Error);
            }
            Ok(_) => {}
            Err(e) => self.globals.set_flash(e.to_string(), FlashLevel::Error),
        }
    }

    async fn open_picker(&mut self) {
        self.set_screen(Screen::ModePicker(PickerState { loading: true, ..Default::default() }));
        self.refresh_picker().await;
    }

    async fn refresh_picker(&mut self) {
        let result = ipc::send(&Request::ListModes).await;
        let Screen::ModePicker(picker) = &mut self.screen else { return };
        picker.loading = false;
        match result {
            Ok(Response::Modes { modes }) => {
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
            MenuItem::Settings => self.open_settings().await,
            MenuItem::Doctor => {
                self.globals.set_flash(
                    crate::i18n::t!("tui.flash.doctor_hint").to_string(),
                    FlashLevel::Info,
                );
            }
            MenuItem::Quit => self.should_quit = true,
        }
    }

    async fn quick_start(&mut self, idx: usize) {
        let Some(mode) = self.globals.cached_modes.get(idx).cloned() else {
            self.globals.set_flash(
                crate::i18n::t!("tui.flash.no_slot", slot = (idx + 1)).to_string(),
                FlashLevel::Error,
            );
            return;
        };
        let cfg = match Config::load() {
            Ok(c) => c,
            Err(e) => {
                self.globals.set_flash(
                    crate::i18n::t!("tui.flash.config_error", message = e).to_string(),
                    FlashLevel::Error,
                );
                return;
            }
        };
        let mut duration = cfg.general.default_duration;
        if let Some(max) = mode.limits.max_duration {
            if duration > max {
                duration = max;
            }
        }
        if let Some(min) = mode.limits.min_duration {
            if duration < min {
                duration = min;
            }
        }
        let req = Request::Start {
            profile: mode.name.clone(),
            duration,
            hard_mode: cfg.general.hard_mode,
            reason: None,
        };
        match ipc::send(&req).await {
            Ok(Response::Session(s)) => {
                self.globals.set_flash(
                    crate::i18n::t!("tui.flash.started", profile = s.profile).to_string(),
                    FlashLevel::Success,
                );
            }
            Ok(Response::Error { message }) => self.globals.set_flash(message, FlashLevel::Info),
            Ok(_) => self
                .globals
                .set_flash(crate::i18n::t!("tui.flash.unexpected").to_string(), FlashLevel::Error),
            Err(e) => self.globals.set_flash(e.to_string(), FlashLevel::Info),
        }
    }

    async fn do_start(&mut self) {
        let cfg = match Config::load() {
            Ok(c) => c,
            Err(e) => {
                self.globals.set_flash(
                    crate::i18n::t!("tui.flash.config_error", message = e).to_string(),
                    FlashLevel::Error,
                );
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
                self.globals.set_flash(
                    crate::i18n::t!("tui.flash.started", profile = s.profile).to_string(),
                    FlashLevel::Success,
                );
            }
            Ok(Response::Error { message }) => self.globals.set_flash(message, FlashLevel::Info),
            Ok(_) => self
                .globals
                .set_flash(crate::i18n::t!("tui.flash.unexpected").to_string(), FlashLevel::Error),
            Err(e) => self.globals.set_flash(e.to_string(), FlashLevel::Info),
        }
    }

    async fn do_stop(&mut self) {
        match ipc::send(&Request::Stop { id: None }).await {
            Ok(Response::Session(s)) => self.globals.set_flash(
                crate::i18n::t!("tui.flash.stopped", profile = s.profile).to_string(),
                FlashLevel::Success,
            ),
            Ok(Response::HardModeActive(_)) => self.globals.set_flash(
                crate::i18n::t!("tui.flash.hard_stop_denied").to_string(),
                FlashLevel::Error,
            ),
            Ok(Response::Error { message }) => self.globals.set_flash(message, FlashLevel::Info),
            Ok(_) => self.globals.set_flash(
                crate::i18n::t!("tui.flash.nothing_to_stop").to_string(),
                FlashLevel::Warn,
            ),
            Err(e) => self.globals.set_flash(e.to_string(), FlashLevel::Info),
        }
    }

    async fn do_panic(&mut self) {
        match ipc::send(&Request::Panic { phrase: String::new(), cancel: false }).await {
            Ok(Response::PanicScheduled(info)) => {
                let msg = match info.panic_releases_at {
                    Some(at) => crate::i18n::t!("tui.flash.panic_release_at", at = at.to_rfc3339())
                        .to_string(),
                    None => crate::i18n::t!("tui.flash.panic_cancelled").to_string(),
                };
                self.globals.set_flash(msg, FlashLevel::Success);
            }
            Ok(Response::Error { message }) => self.globals.set_flash(message, FlashLevel::Info),
            Ok(_) => self.globals.set_flash(
                crate::i18n::t!("tui.flash.no_hard_session").to_string(),
                FlashLevel::Warn,
            ),
            Err(e) => self.globals.set_flash(e.to_string(), FlashLevel::Info),
        }
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}
#[cfg(not(unix))]
fn is_root() -> bool {
    false
}

fn hosts_writable() -> bool {
    #[cfg(unix)]
    {
        std::fs::OpenOptions::new().append(true).open("/etc/hosts").is_ok()
    }
    #[cfg(not(unix))]
    {
        true
    }
}

async fn ensure_daemon() {
    let expected = env!("CARGO_PKG_VERSION");
    match ipc::send(&Request::Ping).await {
        Ok(Response::Pong { version }) if version == expected => return,
        Ok(Response::Pong { .. }) => {
            let _ = ipc::send(&Request::Shutdown).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        _ => {}
    }
    let Ok(exe) = std::env::current_exe() else { return };
    use std::process::{Command, Stdio};
    let need_sudo = cfg!(unix) && !is_root() && !hosts_writable();
    if need_sudo {
        eprintln!("monk: elevating daemon via sudo to manage /etc/hosts…");
        let user = std::env::var("USER").unwrap_or_default();
        let home = std::env::var("HOME").unwrap_or_default();
        let log = crate::paths::log_file()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "/tmp/monkd.log".into());
        let shell = format!("nohup {exe:?} daemon run >>{log:?} 2>&1 &", exe = exe, log = log);
        let _ = Command::new("sudo")
            .args([
                "-E",
                "env",
                &format!("SUDO_USER={user}"),
                &format!("HOME={home}"),
                "sh",
                "-c",
                &shell,
            ])
            .status();
    } else {
        let _ = Command::new(&exe)
            .args(["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if matches!(ipc::send(&Request::Ping).await, Ok(Response::Pong { version }) if version == expected)
        {
            return;
        }
    }
}

pub async fn run() -> Result<()> {
    ensure_daemon().await;
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = main_loop(&mut terminal).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

async fn main_loop<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    let mut app = App::new();
    let start = std::time::Instant::now();
    let mut last_refresh = std::time::Instant::now()
        .checked_sub(Duration::from_secs(10))
        .unwrap_or_else(std::time::Instant::now);

    loop {
        let now = std::time::Instant::now();
        if now.duration_since(last_refresh) >= Duration::from_millis(800) {
            app.refresh().await;
            last_refresh = now;
        }
        app.globals.frame = now.duration_since(start).as_millis() as u64 / 200;
        app.globals.tick_flash();
        let dt = app.last_effect_tick.map(|t| now.duration_since(t)).unwrap_or(Duration::ZERO);
        app.last_effect_tick = Some(now);
        terminal.draw(|f| super::view::draw_with_effects(f, &mut app, dt))?;

        if event::poll(Duration::from_millis(120))? {
            while event::poll(Duration::from_millis(0))? {
                if let Event::Key(key) = event::read()? {
                    app.handle_key(key).await;
                } else {
                    let _ = event::read()?;
                }
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
