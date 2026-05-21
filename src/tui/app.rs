use std::{io, time::Duration};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    config::{Config, Profile},
    ipc::{self, HardModeInfo, ModeSummary, Request, Response},
    session::Session,
    tui::screens,
    Result,
};

pub use screens::confirm::ConfirmState;
pub use screens::doctor::DoctorState;
pub use screens::editor::EditorState;
pub use screens::settings::SettingsState;

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
            MenuItem::Start => "pick a mode and start a focus session",
            MenuItem::Stop => "end the active session (soft mode only)",
            MenuItem::Panic => "request a delayed hard-mode escape",
            MenuItem::Profiles => "list / create / edit / delete modes (a = from preset)",
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
    /// When Some, the picker is asking for confirmation before deleting the
    /// named mode. y/Y commits, esc/n cancels.
    pub confirm_delete: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorField {
    Name,
    Color,
    Max,
    Min,
    Cooldown,
    DailyCap,
    PanicDelay,
    Schedule,
    Sites,
    HookBefore,
    HookAfter,
    Apps,
    Groups,
    Brands,
}

impl EditorField {
    pub const ORDER: [EditorField; 14] = [
        EditorField::Name,
        EditorField::Color,
        EditorField::Max,
        EditorField::Min,
        EditorField::Cooldown,
        EditorField::DailyCap,
        EditorField::PanicDelay,
        EditorField::Schedule,
        EditorField::Sites,
        EditorField::HookBefore,
        EditorField::HookAfter,
        EditorField::Apps,
        EditorField::Groups,
        EditorField::Brands,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EditorField::Name => "name",
            EditorField::Color => "color",
            EditorField::Max => "max duration",
            EditorField::Min => "min duration",
            EditorField::Cooldown => "cooldown",
            EditorField::DailyCap => "daily cap",
            EditorField::PanicDelay => "panic delay",
            EditorField::Schedule => "schedule",
            EditorField::Sites => "custom sites",
            EditorField::HookBefore => "hook before",
            EditorField::HookAfter => "hook after",
            EditorField::Apps => "blocked apps",
            EditorField::Groups => "site groups",
            EditorField::Brands => "brand presets",
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
            EditorField::PanicDelay => {
                "delay before hard-mode panic releases (empty = global default)"
            }
            EditorField::Schedule => "auto-start: `mon-fri 09:00-17:00` or `daily 22:00-23:00 UTC`",
            EditorField::Sites => "comma-separated hosts to block · + add custom",
            EditorField::HookBefore => "shell command run before session",
            EditorField::HookAfter => "shell command run after session",
            EditorField::Apps => {
                "space/enter toggle · + add custom · type to filter · ↑/↓ navigate"
            }
            EditorField::Groups => "space/enter toggle · type to filter · ↑/↓ navigate",
            EditorField::Brands => {
                "space/enter toggle · type to filter · auto-resolves domains + apps"
            }
        }
    }
}

pub const LOCALES: &[&str] = &["en", "ru"];

#[derive(Debug)]
pub enum Screen {
    Home(HomeState),
    ModePicker(PickerState),
    ModeConfirm(Box<ConfirmState>),
    ModeEditor(Box<EditorState>),
    Settings(Box<SettingsState>),
    Doctor(Box<DoctorState>),
    PresetPicker(PresetPickerState),
    Panic(Box<screens::panic::PanicState>),
}

#[derive(Debug)]
pub struct PresetPickerState {
    pub names: Vec<&'static str>,
    pub selected: usize,
    pub error: Option<String>,
}

impl Default for PresetPickerState {
    fn default() -> Self {
        Self { names: crate::onboarding::PRESET_NAMES.to_vec(), selected: 0, error: None }
    }
}

impl PresetPickerState {
    pub fn move_up(&mut self) {
        if self.names.is_empty() {
            return;
        }
        self.selected = if self.selected == 0 { self.names.len() - 1 } else { self.selected - 1 };
    }

    pub fn move_down(&mut self) {
        if self.names.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.names.len();
    }

    pub fn current(&self) -> Option<&'static str> {
        self.names.get(self.selected).copied()
    }
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
    pub active_profile_detail: Option<Profile>,
    pub help_open: bool,
    pub cached_modes: Vec<ModeSummary>,
    pub next_scheduled: Option<(String, chrono::DateTime<chrono::Utc>)>,
}

/// Live snapshot of daemon-side state, updated by the refresh worker.
#[derive(Default, Clone, Debug)]
pub struct Snapshot {
    pub daemon_running: bool,
    pub active: Option<Session>,
    pub hard_mode: Option<HardModeInfo>,
    pub active_mode: Option<ModeSummary>,
    pub active_profile_detail: Option<Profile>,
    pub cached_modes: Vec<ModeSummary>,
    pub next_scheduled: Option<(String, chrono::DateTime<chrono::Utc>)>,
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
    pub snapshot: std::sync::Arc<parking_lot::RwLock<Snapshot>>,
    pub refresh_kick: std::sync::Arc<tokio::sync::Notify>,
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

    pub fn set_screen(&mut self, screen: Screen) {
        self.screen = screen;
        self.trigger_enter_effect();
    }

    /// Pull the latest snapshot from the refresh worker into Globals so the
    /// view layer can keep reading from `app.globals` unchanged. Cheap: one
    /// rwlock-read + clones.
    pub fn pull_snapshot(&mut self) {
        let s = self.snapshot.read().clone();
        self.globals.daemon_running = s.daemon_running;
        self.globals.active = s.active;
        self.globals.hard_mode = s.hard_mode;
        self.globals.active_mode = s.active_mode;
        self.globals.active_profile_detail = s.active_profile_detail;
        // Mirror the cached mode list into the picker if we're currently on
        // it. Lets us avoid awaiting an IPC call on open_picker, which would
        // otherwise stall the main render loop.
        if let Screen::ModePicker(picker) = &mut self.screen {
            picker.modes = s.cached_modes.clone();
            if picker.selected >= picker.modes.len() {
                picker.selected = picker.modes.len().saturating_sub(1);
            }
            if !picker.modes.is_empty() {
                picker.loading = false;
            }
        }
        self.globals.cached_modes = s.cached_modes;
        self.globals.next_scheduled = s.next_scheduled;
    }

    /// Ask the background worker to refresh ASAP — called after actions that
    /// change daemon state (start, stop, save, delete, panic). Returns
    /// immediately; UI sees fresh state on the next snapshot poll.
    pub fn kick_refresh(&self) {
        self.refresh_kick.notify_one();
    }

    pub async fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        if self.globals.help_open {
            if matches!(
                key.code,
                KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') | KeyCode::F(1)
            ) {
                self.globals.help_open = false;
            }
            return;
        }
        // F1 works on every screen — text-input screens (editor/settings/
        // panic) eat '?' as a literal character, so F1 is the always-safe
        // fallback to surface the help overlay. '?' still works elsewhere.
        if matches!(key.code, KeyCode::F(1)) {
            self.globals.help_open = true;
            return;
        }
        if matches!(key.code, KeyCode::Char('?'))
            && !matches!(
                self.screen,
                Screen::ModeEditor(_) | Screen::Settings(_) | Screen::Doctor(_) | Screen::Panic(_)
            )
        {
            self.globals.help_open = true;
            return;
        }
        match &self.screen {
            Screen::Home(_) => screens::home::handle_home_key(self, key).await,
            Screen::ModePicker(_) => screens::picker::handle_picker_key(self, key).await,
            Screen::ModeConfirm(_) => screens::confirm::handle_confirm_key(self, key).await,
            Screen::ModeEditor(_) => screens::editor::handle_editor_key(self, key).await,
            Screen::Settings(_) => screens::settings::handle_settings_key(self, key).await,
            Screen::Doctor(_) => screens::doctor::handle_doctor_key(self, key).await,
            Screen::PresetPicker(_) => screens::picker::handle_preset_picker_key(self, key).await,
            Screen::Panic(_) => screens::panic::handle_panic_key(self, key).await,
        }
    }

    pub async fn instantiate_preset(&mut self) {
        let preset_name = {
            let Screen::PresetPicker(state) = &self.screen else { return };
            match state.current() {
                Some(n) => n.to_string(),
                None => return,
            }
        };
        let profile = match crate::onboarding::load_preset(&preset_name) {
            Ok(p) => p,
            Err(e) => {
                if let Screen::PresetPicker(s) = &mut self.screen {
                    s.error = Some(e.to_string());
                }
                return;
            }
        };
        let existing: std::collections::BTreeSet<String> =
            self.globals.cached_modes.iter().map(|m| m.name.clone()).collect();
        let unique = unique_mode_name(&preset_name, &existing);
        self.set_screen(Screen::ModeEditor(Box::new(EditorState::edit(unique, profile))));
    }

    pub fn open_editor_edit(&mut self) {
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

    pub fn open_editor_from_confirm(&mut self) {
        let Screen::ModeConfirm(confirm) = &self.screen else { return };
        let name = confirm.mode.name.clone();
        let profile = confirm
            .detail
            .as_ref()
            .map(|d| d.profile.clone())
            .or_else(|| Config::load().ok().and_then(|c| c.profiles.get(&name).cloned()))
            .unwrap_or_default();
        self.set_screen(Screen::ModeEditor(Box::new(EditorState::edit(name, profile))));
    }

    pub async fn delete_current_mode(&mut self) {
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
                self.kick_refresh();
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

    pub async fn duplicate_current_mode(&mut self) {
        let name = {
            let Screen::ModePicker(picker) = &self.screen else { return };
            match picker.current() {
                Some(m) => m.name.clone(),
                None => return,
            }
        };

        // Fetch the profile via IPC
        let profile = match ipc::send(&Request::ModeDetail { name: name.clone(), days: 1 }).await {
            Ok(Response::ModeDetailData(detail)) => detail.profile,
            Ok(Response::Error { message }) => {
                if let Screen::ModePicker(p) = &mut self.screen {
                    p.error = Some(message);
                }
                return;
            }
            Ok(_) => {
                if let Screen::ModePicker(p) = &mut self.screen {
                    p.error = Some("unexpected response".into());
                }
                return;
            }
            Err(e) => {
                if let Screen::ModePicker(p) = &mut self.screen {
                    p.error = Some(e.to_string());
                }
                return;
            }
        };

        // Generate unique name
        let existing: std::collections::BTreeSet<String> =
            self.globals.cached_modes.iter().map(|m| m.name.clone()).collect();
        let base = format!("{}-2", name);
        let unique_name = unique_mode_name(&base, &existing);

        // Open editor with the duplicated profile
        self.set_screen(Screen::ModeEditor(Box::new(EditorState::edit(unique_name, profile))));
    }

    pub async fn save_editor(&mut self) {
        let (name, profile) = {
            let Screen::ModeEditor(ed) = &mut self.screen else { return };
            match ed.build_profile() {
                Ok(v) => v,
                Err((field, msg)) => {
                    ed.error = Some(msg);
                    ed.error_field = field;
                    if let Some(f) = field {
                        ed.focus = f;
                    }
                    return;
                }
            }
        };
        match ipc::send(&Request::SaveMode { name: name.clone(), profile: Box::new(profile) }).await
        {
            Ok(Response::Ok) => {
                self.globals.set_flash(
                    crate::i18n::t!("tui.flash.saved", profile = name).to_string(),
                    FlashLevel::Success,
                );
                self.open_picker();
            }
            Ok(Response::Error { message }) => {
                if let Screen::ModeEditor(ed) = &mut self.screen {
                    ed.error = Some(message);
                    ed.error_field = None;
                }
            }
            Ok(_) => {}
            Err(e) => {
                if let Screen::ModeEditor(ed) = &mut self.screen {
                    ed.error = Some(e.to_string());
                    ed.error_field = None;
                }
            }
        }
    }

    pub async fn save_editor_and_open_confirm(&mut self) {
        let (name, profile) = {
            let Screen::ModeEditor(ed) = &mut self.screen else { return };
            match ed.build_profile() {
                Ok(v) => v,
                Err((field, msg)) => {
                    ed.error = Some(msg);
                    ed.error_field = field;
                    if let Some(f) = field {
                        ed.focus = f;
                    }
                    return;
                }
            }
        };
        match ipc::send(&Request::SaveMode { name: name.clone(), profile: Box::new(profile) }).await
        {
            Ok(Response::Ok) => {
                self.globals.set_flash(
                    crate::i18n::t!("tui.flash.saved", profile = name).to_string(),
                    FlashLevel::Success,
                );

                // Fetch mode details and open confirm screen
                match ipc::send(&Request::ModeDetail { name: name.clone(), days: 14 }).await {
                    Ok(Response::ModeDetailData(detail)) => {
                        let cfg = Config::load().ok();
                        let default_dur = cfg
                            .as_ref()
                            .map(|c| c.general.default_duration)
                            .unwrap_or(Duration::from_secs(25 * 60));
                        let hard = cfg.as_ref().map(|c| c.general.hard_mode).unwrap_or(false);

                        let mode = ModeSummary {
                            name: name.clone(),
                            color: detail.profile.color.clone(),
                            blocked_apps: detail.profile.apps.len(),
                            blocked_sites: detail.profile.sites.len(),
                            blocked_groups: detail.profile.site_groups.len(),
                            limits: detail.profile.limits.clone(),
                            stats: crate::audit::stats::ModeStats::default(),
                            is_default: false,
                            has_schedule: detail.profile.schedule.is_some(),
                        };

                        let mut state = ConfirmState::from_mode(mode, default_dur, hard);
                        state.detail = Some(*detail);
                        self.set_screen(Screen::ModeConfirm(Box::new(state)));
                    }
                    Ok(_) | Err(_) => {
                        // Fallback to picker with flash on fetch failure
                        self.globals.set_flash(
                            "saved mode, but couldn't open start screen".to_string(),
                            FlashLevel::Warn,
                        );
                        self.open_picker();
                    }
                }
            }
            Ok(Response::Error { message }) => {
                if let Screen::ModeEditor(ed) = &mut self.screen {
                    ed.error = Some(message);
                    ed.error_field = None;
                }
            }
            Ok(_) => {}
            Err(e) => {
                if let Screen::ModeEditor(ed) = &mut self.screen {
                    ed.error = Some(e.to_string());
                    ed.error_field = None;
                }
            }
        }
    }

    pub async fn open_confirm_from_picker(&mut self) {
        let Screen::ModePicker(picker) = &self.screen else { return };
        let Some(mode) = picker.current().cloned() else { return };
        let cfg = Config::load().ok();
        let default_dur = cfg
            .as_ref()
            .map(|c| c.general.default_duration)
            .unwrap_or(Duration::from_secs(25 * 60));
        let hard = cfg.as_ref().map(|c| c.general.hard_mode).unwrap_or(false);
        let mut state = ConfirmState::from_mode(mode.clone(), default_dur, hard);
        if let Ok(Response::ModeDetailData(detail)) =
            ipc::send(&Request::ModeDetail { name: mode.name.clone(), days: 14 }).await
        {
            state.detail = Some(*detail);
        }
        self.set_screen(Screen::ModeConfirm(Box::new(state)));
    }

    pub async fn start_from_confirm(&mut self) {
        // Pre-flight: if a session is already running for this exact profile,
        // explain instead of asking the daemon to reject it.
        let already_running = matches!(
            (&self.screen, &self.globals.active),
            (Screen::ModeConfirm(c), Some(s)) if c.mode.name == s.profile
        );
        if already_running {
            if let Screen::ModeConfirm(c) = &mut self.screen {
                c.error = Some(
                    "this mode is already running — esc to go back, stop or panic from home".into(),
                );
            }
            return;
        }
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
        self.kick_refresh();
    }

    pub async fn open_doctor(&mut self) {
        if !matches!(self.screen, Screen::Doctor(_)) {
            self.set_screen(Screen::Doctor(Box::new(DoctorState {
                loading: true,
                ..Default::default()
            })));
        } else if let Screen::Doctor(st) = &mut self.screen {
            st.loading = true;
        }
        let report = crate::doctor::run().await;
        if let Screen::Doctor(st) = &mut self.screen {
            st.loading = false;
            st.set_report(report);
        }
    }

    pub async fn open_settings(&mut self) {
        match ipc::send(&Request::GetGeneral).await {
            Ok(Response::General(g)) => {
                let names: Vec<String> =
                    self.globals.cached_modes.iter().map(|m| m.name.clone()).collect();
                self.set_screen(Screen::Settings(Box::new(SettingsState::from_general(g, names))));
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

    pub async fn save_settings(&mut self) {
        let general = {
            let Screen::Settings(st) = &mut self.screen else { return };
            match st.build_general() {
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

    pub async fn reset_all(&mut self) {
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

    /// Switch to the picker screen without blocking the main event loop on
    /// IPC. Seeds the list from the cached snapshot (so the user sees their
    /// modes immediately) and kicks the background refresh worker for fresh
    /// data; the snapshot pull on the next frame will overwrite picker.modes
    /// via `pull_snapshot`.
    pub fn open_picker(&mut self) {
        let cached = self.globals.cached_modes.clone();
        let loading = cached.is_empty();
        self.set_screen(Screen::ModePicker(PickerState {
            modes: cached,
            loading,
            ..Default::default()
        }));
        self.kick_refresh();
    }

    pub async fn refresh_picker(&mut self) {
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

    pub async fn quick_start(&mut self, idx: usize) {
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
        self.kick_refresh();
    }

    pub async fn do_start(&mut self) {
        self.open_picker();
    }

    pub async fn do_stop(&mut self) {
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
        self.kick_refresh();
    }

    /// Open the panic-confirm modal. The phrase comes from HardModeInfo
    /// (already in our snapshot) so the user sees exactly what to type.
    /// If there's no hard mode active, surface that as a flash and stay on
    /// the current screen — panic only makes sense for hard sessions.
    pub async fn do_panic(&mut self) {
        let Some(hard) = self.globals.hard_mode.clone() else {
            self.globals.set_flash(
                crate::i18n::t!("tui.flash.no_hard_session").to_string(),
                FlashLevel::Warn,
            );
            return;
        };
        let st = screens::panic::PanicState::new(hard.panic_phrase.clone());
        self.set_screen(Screen::Panic(Box::new(st)));
    }

    /// Send the user-typed phrase to the daemon. Called from the panic
    /// screen after local validation passes.
    pub async fn submit_panic(&mut self, phrase: String) {
        match ipc::send(&Request::Panic { phrase, cancel: false }).await {
            Ok(Response::PanicScheduled(info)) => {
                let msg = match info.panic_releases_at {
                    Some(at) => {
                        let remaining =
                            (at - chrono::Utc::now()).to_std().unwrap_or(std::time::Duration::ZERO);
                        let dur = humantime::format_duration(remaining).to_string();
                        crate::i18n::t!("tui.flash.panic_release_in", duration = dur).to_string()
                    }
                    None => crate::i18n::t!("tui.flash.panic_cancelled").to_string(),
                };
                self.globals.set_flash(msg, FlashLevel::Success);
                self.set_screen(Screen::Home(HomeState::default()));
            }
            Ok(Response::Error { message }) => {
                if let Screen::Panic(st) = &mut self.screen {
                    st.error = Some(message);
                } else {
                    self.globals.set_flash(message, FlashLevel::Info);
                }
            }
            Ok(_) => self.globals.set_flash(
                crate::i18n::t!("tui.flash.no_hard_session").to_string(),
                FlashLevel::Warn,
            ),
            Err(e) => self.globals.set_flash(e.to_string(), FlashLevel::Info),
        }
        self.kick_refresh();
    }
}

/// Return `base` if it isn't taken, else `base-2`, `base-3`, ... whichever
/// is free first.
pub fn unique_mode_name(base: &str, taken: &std::collections::BTreeSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_string();
    }
    for n in 2..=u32::MAX {
        let candidate = format!("{base}-{n}");
        if !taken.contains(&candidate) {
            return candidate;
        }
    }
    base.to_string()
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

    if crate::paths::system_mode() {
        eprintln!(
            "monk: daemon unreachable. \
             reload it with `sudo launchctl bootstrap system /Library/LaunchDaemons/dev.monk.monkd.plist`"
        );
        return;
    }

    let Ok(exe) = std::env::current_exe() else { return };
    use std::process::{Command, Stdio};
    let _ = Command::new(&exe)
        .args(["daemon", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if matches!(ipc::send(&Request::Ping).await, Ok(Response::Pong { version }) if version == expected)
        {
            return;
        }
    }
    if !hosts_writable() && !is_root() {
        eprintln!(
            "monk: daemon running as user — /etc/hosts not writable, blocking will be unavailable. \
             install the system daemon: `sudo monk service install`"
        );
    }
}

pub async fn run() -> Result<()> {
    ensure_daemon().await;

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        prev_hook(info);
    }));

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

    let shutdown = std::sync::Arc::new(tokio::sync::Notify::new());
    let worker =
        spawn_refresh_worker(app.snapshot.clone(), app.refresh_kick.clone(), shutdown.clone());
    // Kick once so we have data before the first frame.
    app.refresh_kick.notify_one();

    loop {
        let now = std::time::Instant::now();
        app.pull_snapshot();
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

    shutdown.notify_waiters();
    let _ = worker.await;
    Ok(())
}

fn spawn_refresh_worker(
    snapshot: std::sync::Arc<parking_lot::RwLock<Snapshot>>,
    kick: std::sync::Arc<tokio::sync::Notify>,
    shutdown: std::sync::Arc<tokio::sync::Notify>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let next = refresh_once().await;
            *snapshot.write() = next;

            tokio::select! {
                _ = shutdown.notified() => break,
                _ = kick.notified() => {} // immediate refresh
                _ = tokio::time::sleep(Duration::from_millis(800)) => {} // periodic
            }
        }
    })
}

async fn refresh_once() -> Snapshot {
    let mut snap = Snapshot::default();
    match ipc::send(&Request::Status).await {
        Ok(Response::Status { active, hard_mode, .. }) => {
            snap.daemon_running = true;
            snap.active = active.map(|b| *b);
            snap.hard_mode = hard_mode.map(|b| *b);
        }
        _ => return snap,
    }
    if let Ok(Response::Modes { modes }) = ipc::send(&Request::ListModes).await {
        if let Some(session) = &snap.active {
            snap.active_mode = modes.iter().find(|m| m.name == session.profile).cloned();
        }
        snap.cached_modes = modes;
    }
    if let Some(session) = &snap.active {
        if let Ok(Response::Config(cfg)) = ipc::send(&Request::GetConfig).await {
            snap.active_profile_detail = cfg.profiles.get(&session.profile).cloned();
        }
    }
    if snap.active.is_none() {
        if let Ok(Response::NextScheduled { profile: Some(p), at: Some(t) }) =
            ipc::send(&Request::NextScheduled).await
        {
            snap.next_scheduled = Some((p, t));
        }
    }
    snap
}

#[cfg(test)]
mod schedule_spec_tests {
    use crate::tui::screens::editor::{format_schedule_spec, parse_schedule_spec};

    #[test]
    fn round_trip() {
        for spec in [
            "mon,tue,wed,thu,fri 09:00-17:00",
            "sat,sun 10:00-12:00 UTC",
            "fri 22:00-02:00",
            "off mon,tue 09:00-10:00",
        ] {
            let parsed = parse_schedule_spec(spec).unwrap().unwrap();
            let again = format_schedule_spec(Some(&parsed));
            let reparsed = parse_schedule_spec(&again).unwrap().unwrap();
            assert_eq!(parsed, reparsed, "round-trip drift on `{spec}` -> `{again}`");
        }
    }

    #[test]
    fn empty_is_none() {
        assert!(parse_schedule_spec("").unwrap().is_none());
        assert!(parse_schedule_spec("   ").unwrap().is_none());
    }

    #[test]
    fn shorthands_expand() {
        let s = parse_schedule_spec("weekdays 09:00-17:00").unwrap().unwrap();
        assert_eq!(s.days.len(), 5);
        let s = parse_schedule_spec("daily 06:00-07:00").unwrap().unwrap();
        assert_eq!(s.days.len(), 7);
    }
}
