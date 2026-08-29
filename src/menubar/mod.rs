//! macOS menu bar companion: a native NSStatusItem showing the current
//! session state with quick start/stop for every configured mode.
//!
//! Architecture: the tao event loop owns the tray and menu on the main
//! thread; a worker thread with its own single-thread tokio runtime polls
//! the daemon over IPC and ships `Snapshot`s back through the event-loop
//! proxy. Menu clicks arrive through the same proxy as user events.

mod agent;

pub use agent::{install as install_agent, uninstall as uninstall_agent};

use std::sync::mpsc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
use tray_icon::menu::{
    CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::ipc::{self, HardModeInfo, ModeSummary, Request, Response};
use crate::session::Session;
use crate::{Error, Result};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const FULL_REFRESH: Duration = Duration::from_secs(30);
/// Index of the first mode submenu inside the tray menu:
/// [header, info, separator, <modes...>].
const MODES_AT: usize = 3;

#[derive(Debug)]
enum UserEvent {
    State(Box<Snapshot>),
    MenuClick(String),
}

#[derive(Debug)]
struct Snapshot {
    daemon_ok: bool,
    session: Option<Session>,
    hard: Option<HardModeInfo>,
    /// `None` = not refreshed this tick (keep the current menu).
    modes: Option<Vec<ModeSummary>>,
    next_scheduled: Option<(Option<String>, Option<DateTime<Utc>>)>,
    default_duration: Option<Duration>,
}

enum Cmd {
    Start { profile: String, duration: Duration, hard: bool },
    Stop,
    StartDaemon,
    Refresh,
}

pub fn run() -> Result<()> {
    let _lock = single_instance_lock()?;

    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    event_loop.set_activation_policy(ActivationPolicy::Accessory);

    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
    let worker_proxy = event_loop.create_proxy();
    std::thread::spawn(move || worker(cmd_rx, worker_proxy));

    let click_proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = click_proxy.send_event(UserEvent::MenuClick(event.id().0.clone()));
    }));

    let mut ui = Ui::new()?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {
                if let Err(e) = ui.build_tray() {
                    tracing::error!(error = %e, "failed to create tray icon");
                    std::process::exit(1);
                }
            }
            Event::UserEvent(UserEvent::State(snap)) => ui.apply(*snap),
            Event::UserEvent(UserEvent::MenuClick(id)) => {
                let quit = ui.handle_click(&id, &cmd_tx);
                if quit {
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    });
}

struct Ui {
    tray: Option<TrayIcon>,
    menu: Menu,
    header: MenuItem,
    info: MenuItem,
    stop: MenuItem,
    start_daemon: MenuItem,
    login: CheckMenuItem,
    mode_menus: Vec<Submenu>,
    mode_key: String,
    default_duration: Duration,
    icon_idle: Icon,
    icon_active: Icon,
    active_icon_shown: bool,
}

impl Ui {
    fn new() -> Result<Self> {
        let menu = Menu::new();
        let header =
            MenuItem::with_id("header", crate::i18n::t!("menubar.connecting"), false, None);
        let info = MenuItem::with_id("info", "—", false, None);
        let stop = MenuItem::with_id("stop", crate::i18n::t!("menubar.stop"), false, None);
        let start_daemon =
            MenuItem::with_id("start_daemon", crate::i18n::t!("menubar.start_daemon"), false, None);
        let login = CheckMenuItem::with_id(
            "login",
            crate::i18n::t!("menubar.launch_at_login"),
            true,
            agent::installed(),
            None,
        );
        let quit = MenuItem::with_id("quit", crate::i18n::t!("menubar.quit"), true, None);

        let sep_top = PredefinedMenuItem::separator();
        let sep_modes = PredefinedMenuItem::separator();
        let sep_bottom = PredefinedMenuItem::separator();
        let items: [&dyn IsMenuItem; 9] = [
            &header,
            &info,
            &sep_top,
            // Mode submenus are inserted at MODES_AT (after sep_top) once
            // the first full snapshot arrives.
            &sep_modes,
            &stop,
            &start_daemon,
            &sep_bottom,
            &login,
            &quit,
        ];
        for item in items {
            menu.append(item).map_err(|e| Error::Other(e.to_string()))?;
        }

        Ok(Self {
            tray: None,
            menu,
            header,
            info,
            stop,
            start_daemon,
            login,
            mode_menus: Vec::new(),
            mode_key: String::new(),
            default_duration: Duration::from_secs(25 * 60),
            icon_idle: circle_icon(false),
            icon_active: circle_icon(true),
            active_icon_shown: false,
        })
    }

    fn build_tray(&mut self) -> Result<()> {
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(self.menu.clone()))
            .with_icon(self.icon_idle.clone())
            .with_icon_as_template(true)
            .with_tooltip("monk")
            .build()
            .map_err(|e| Error::Other(e.to_string()))?;
        self.tray = Some(tray);
        Ok(())
    }

    fn apply(&mut self, snap: Snapshot) {
        if let Some(d) = snap.default_duration {
            self.default_duration = d;
        }
        if let Some(modes) = &snap.modes {
            self.rebuild_modes(modes);
        }
        self.render(&snap);
    }

    fn render(&mut self, snap: &Snapshot) {
        let hard_active = snap.hard.is_some();
        let session_active = snap.session.is_some();

        // Header + tray title.
        let title;
        if !snap.daemon_ok {
            self.header.set_text(crate::i18n::t!("menubar.daemon_offline"));
            title = None;
        } else if let Some(s) = &snap.session {
            let remaining = fmt_duration(s.remaining());
            let p = s.profile.clone();
            let r = remaining.clone();
            let line = crate::i18n::t!("menubar.active", profile = p, remaining = r);
            if s.hard_mode {
                self.header.set_text(format!("🔒 {line}"));
                title = Some(format!("🔒 {remaining}"));
            } else {
                self.header.set_text(line);
                title = Some(remaining);
            }
        } else {
            self.header.set_text(crate::i18n::t!("menubar.idle"));
            title = None;
        }

        // Info line: panic countdown > hard-mode hint > next schedule.
        if let Some(h) = &snap.hard {
            if let Some(release) = h.panic_releases_at {
                let at = release.with_timezone(&Local).format("%H:%M").to_string();
                self.info.set_text(crate::i18n::t!("menubar.panic_pending", time = at));
            } else {
                self.info.set_text(crate::i18n::t!("menubar.hard_hint"));
            }
        } else if let Some((Some(profile), Some(at))) = &snap.next_scheduled {
            let at = at.with_timezone(&Local).format("%a %H:%M").to_string();
            let p = profile.clone();
            self.info.set_text(crate::i18n::t!("menubar.next_scheduled", profile = p, time = at));
        } else if !snap.daemon_ok {
            self.info.set_text(crate::i18n::t!("menubar.offline_hint"));
        } else {
            self.info.set_text(crate::i18n::t!("menubar.no_schedule"));
        }

        self.stop.set_enabled(snap.daemon_ok && session_active && !hard_active);
        self.start_daemon.set_enabled(!snap.daemon_ok);
        for m in &self.mode_menus {
            m.set_enabled(snap.daemon_ok && !session_active);
        }

        if let Some(tray) = &self.tray {
            tray.set_title(title.as_deref());
            if session_active != self.active_icon_shown {
                let icon =
                    if session_active { self.icon_active.clone() } else { self.icon_idle.clone() };
                let _ = tray.set_icon(Some(icon));
                tray.set_icon_as_template(true);
                self.active_icon_shown = session_active;
            }
        }
    }

    fn rebuild_modes(&mut self, modes: &[ModeSummary]) {
        let mut key = modes
            .iter()
            .map(|m| {
                format!(
                    "{}|{}|{}|{}|{}|{}",
                    m.name,
                    m.blocked_sites,
                    m.blocked_apps,
                    m.blocked_groups,
                    m.has_schedule,
                    m.is_default
                )
            })
            .collect::<Vec<_>>()
            .join(";");
        key.push_str(&format!("#{}", self.default_duration.as_secs()));
        if key == self.mode_key {
            return;
        }
        for old in self.mode_menus.drain(..) {
            let _ = self.menu.remove(&old);
        }
        for (i, mode) in modes.iter().enumerate() {
            let sub = self.mode_submenu(mode);
            if self.menu.insert(&sub, MODES_AT + i).is_ok() {
                self.mode_menus.push(sub);
            }
        }
        self.mode_key = key;
    }

    fn mode_submenu(&self, mode: &ModeSummary) -> Submenu {
        let mut label = mode.name.clone();
        if mode.is_default {
            label.push_str(" ★");
        }
        if mode.has_schedule {
            label.push_str(" ⏰");
        }
        let sub = Submenu::new(&label, true);
        let sites = mode.blocked_sites + mode.blocked_groups;
        let apps = mode.blocked_apps;
        let counts = crate::i18n::t!("menubar.mode_counts", sites = sites, apps = apps);
        let _ = sub.append(&MenuItem::new(counts, false, None));
        let _ = sub.append(&PredefinedMenuItem::separator());

        let def = self.default_duration;
        let start_id = |secs: u64, hard: bool| {
            format!("start:{}:{}:{}", mode.name, secs, if hard { 1 } else { 0 })
        };
        let _ = sub.append(&MenuItem::with_id(
            start_id(def.as_secs(), false),
            crate::i18n::t!("menubar.start_default", duration = fmt_duration(def)),
            true,
            None,
        ));
        for secs in [30 * 60u64, 3600, 2 * 3600] {
            if secs == def.as_secs() {
                continue;
            }
            let d = fmt_duration(Duration::from_secs(secs));
            let _ = sub.append(&MenuItem::with_id(
                start_id(secs, false),
                crate::i18n::t!("menubar.start_for", duration = d),
                true,
                None,
            ));
        }
        let _ = sub.append(&PredefinedMenuItem::separator());
        let _ = sub.append(&MenuItem::with_id(
            start_id(def.as_secs(), true),
            crate::i18n::t!("menubar.start_hard", duration = fmt_duration(def)),
            true,
            None,
        ));
        sub
    }

    /// Returns `true` when the app should quit.
    fn handle_click(&mut self, id: &str, cmd_tx: &mpsc::Sender<Cmd>) -> bool {
        match id {
            "quit" => return true,
            "stop" => {
                let _ = cmd_tx.send(Cmd::Stop);
            }
            "start_daemon" => {
                let _ = cmd_tx.send(Cmd::StartDaemon);
            }
            "login" => {
                let result = if agent::installed() { agent::uninstall() } else { agent::install() };
                if let Err(e) = result {
                    tracing::error!(error = %e, "toggling login item failed");
                }
                self.login.set_checked(agent::installed());
            }
            other => {
                if let Some(rest) = other.strip_prefix("start:") {
                    // Mode names may contain ':'; the two trailing segments
                    // are always duration and the hard flag.
                    let mut parts = rest.rsplitn(3, ':');
                    let hard = parts.next().map(|p| p == "1").unwrap_or(false);
                    let secs = parts.next().and_then(|p| p.parse::<u64>().ok());
                    let profile = parts.next().map(str::to_string);
                    if let (Some(secs), Some(profile)) = (secs, profile) {
                        let _ = cmd_tx.send(Cmd::Start {
                            profile,
                            duration: Duration::from_secs(secs),
                            hard,
                        });
                    }
                }
            }
        }
        let _ = cmd_tx.send(Cmd::Refresh);
        false
    }
}

fn worker(rx: mpsc::Receiver<Cmd>, proxy: EventLoopProxy<UserEvent>) {
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!(error = %e, "menubar worker: tokio runtime");
            return;
        }
    };
    let mut last_full = Instant::now() - FULL_REFRESH;
    loop {
        let mut force_full = false;
        match rx.recv_timeout(POLL_INTERVAL) {
            Ok(Cmd::Start { profile, duration, hard }) => {
                let req = Request::Start { profile, duration, hard_mode: hard, reason: None };
                match rt.block_on(ipc::send(&req)) {
                    Ok(Response::Error { message }) => {
                        tracing::warn!(%message, "start rejected");
                    }
                    Err(e) => tracing::warn!(error = %e, "start failed"),
                    Ok(_) => {}
                }
                force_full = true;
            }
            Ok(Cmd::Stop) => {
                match rt.block_on(ipc::send(&Request::Stop { id: None })) {
                    Ok(Response::Error { message }) => {
                        tracing::warn!(%message, "stop rejected");
                    }
                    Err(e) => tracing::warn!(error = %e, "stop failed"),
                    Ok(_) => {}
                }
                force_full = true;
            }
            Ok(Cmd::StartDaemon) => {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::process::Command::new(exe).args(["daemon", "start"]).status();
                }
                force_full = true;
            }
            Ok(Cmd::Refresh) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }

        let full = force_full || last_full.elapsed() >= FULL_REFRESH;
        let snap = rt.block_on(fetch(full));
        if full && snap.daemon_ok {
            last_full = Instant::now();
        }
        if proxy.send_event(UserEvent::State(Box::new(snap))).is_err() {
            return;
        }
    }
}

async fn fetch(full: bool) -> Snapshot {
    let mut snap = Snapshot {
        daemon_ok: false,
        session: None,
        hard: None,
        modes: None,
        next_scheduled: None,
        default_duration: None,
    };
    match ipc::send(&Request::Status).await {
        Ok(Response::Status { active, hard_mode, .. }) => {
            snap.daemon_ok = true;
            snap.session = active.map(|s| *s);
            snap.hard = hard_mode.map(|h| *h);
        }
        Ok(_) | Err(_) => return snap,
    }
    if !full {
        return snap;
    }
    if let Ok(Response::Modes { modes }) = ipc::send(&Request::ListModes).await {
        snap.modes = Some(modes);
    }
    if let Ok(Response::NextScheduled { profile, at }) = ipc::send(&Request::NextScheduled).await {
        snap.next_scheduled = Some((profile, at));
    }
    if let Ok(Response::General(general)) = ipc::send(&Request::GetGeneral).await {
        snap.default_duration = Some(general.default_duration);
    }
    snap
}

fn fmt_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        if m > 0 {
            format!("{h}h{m:02}m")
        } else {
            format!("{h}h")
        }
    } else if m > 0 {
        format!("{m}m")
    } else {
        format!("{s}s")
    }
}

/// Draws a 32×32 template icon: a ring when idle, a filled disc when a
/// session is active. Black + alpha only, so macOS re-tints it per theme.
fn circle_icon(filled: bool) -> Icon {
    const SIZE: u32 = 32;
    let c = (SIZE as f32 - 1.0) / 2.0;
    let outer = 11.0f32;
    let inner = 7.5f32;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let alpha = if filled {
                smooth_edge(outer - d)
            } else {
                smooth_edge(outer - d) * smooth_edge(d - inner)
            };
            rgba.extend_from_slice(&[0, 0, 0, (alpha * 255.0) as u8]);
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE).expect("static icon dimensions are valid")
}

fn smooth_edge(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

fn single_instance_lock() -> Result<fs_err::File> {
    use fs2::FileExt;
    // Per-user dir on purpose: in system mode data_dir() is root-owned and
    // the menu bar app runs as the logged-in user.
    let dir = crate::paths::user_cache_dir()?;
    let file = fs_err::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("menubar.lock"))?;
    file.file()
        .try_lock_exclusive()
        .map_err(|_| Error::Other(crate::i18n::t!("menubar.already_running").to_string()))?;
    Ok(file)
}
