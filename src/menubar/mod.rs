//! macOS menu bar companion: a native NSStatusItem showing the current
//! session state with quick start/stop for every configured mode.
//!
//! Architecture: the tao event loop owns the tray and menu on the main
//! thread; a worker thread with its own single-thread tokio runtime polls
//! the daemon over IPC and ships `Snapshot`s back through the event-loop
//! proxy. Menu clicks arrive through the same proxy as user events.

mod agent;
mod bundle;
mod icon;
mod notify;

pub use agent::{
    install as install_agent, spawn_now as launch_detached, uninstall_and_stop as uninstall_agent,
};
pub use bundle::is_bundled;

/// What `monk doctor` and `monk update` need to know about the menu bar app
/// without reaching into its internals.
#[derive(Debug, Clone)]
pub struct Status {
    /// The LaunchAgent that starts it at login is registered.
    pub login_item: bool,
    /// Path of the installed `monk.app`, if it exists.
    pub bundle: Option<std::path::PathBuf>,
    /// The installed bundle was built by a different version of monk.
    pub stale: bool,
    /// An instance is up right now.
    pub running: bool,
}

pub fn status() -> Status {
    let bundle = bundle::app_path().ok().filter(|p| p.exists());
    Status {
        login_item: agent::installed(),
        stale: bundle.is_some() && bundle::is_stale(),
        running: running_pid().is_some(),
        bundle,
    }
}

/// Rebuilds the bundle around the current binary and restarts the app, so an
/// upgraded CLI does not leave an old menu bar app running forever. No-op
/// when the menu bar app was never installed.
pub fn refresh_after_update(version: &str) -> Result<bool> {
    let status = status();
    if status.bundle.is_none() {
        return Ok(false);
    }
    require_user_session()?;
    stop_running();
    let bin = bundle::install_as(version)?;
    // Put it back the way the user had it: a login item stays a login item,
    // and an app they started by hand is only restarted if it was up.
    if status.login_item {
        agent::register(&bin)?;
    } else if status.running {
        agent::spawn_now()?;
    }
    Ok(true)
}

use std::sync::mpsc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
use tray_icon::menu::{
    CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{TrayIcon, TrayIconBuilder};

use self::icon::Mark;
use self::notify::{Kind, Notifier};
use crate::ipc::{self, HardModeInfo, ModeSummary, Request, Response};
use crate::session::Session;
use crate::{Error, Result};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const FULL_REFRESH: Duration = Duration::from_secs(30);
/// Index of the first mode submenu inside the tray menu:
/// [header, info, usage, separator, <modes...>].
const MODES_AT: usize = 4;
/// Offered from the "Add time" submenu, in minutes.
const EXTEND_CHOICES: [u64; 4] = [5, 15, 30, 60];
/// Start durations every mode submenu offers, on top of the configured
/// default duration.
const START_CHOICES: [u64; 4] = [15 * 60, 30 * 60, 60 * 60, 2 * 60 * 60];

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
    /// The daemon's general settings; `None` when not refreshed. Kept whole
    /// because `UpdateGeneral` replaces the section, so changing one field
    /// means echoing back the rest.
    general: Option<crate::config::General>,
    /// Sum of every mode's rolling 24h usage; `None` when not refreshed.
    used_24h: Option<Duration>,
}

enum Cmd {
    Start { profile: String, duration: Duration, hard: bool },
    Extend { by: Duration },
    SetDefault { profile: String },
    Stop,
    StartDaemon,
    Refresh,
}

pub fn run() -> Result<()> {
    require_user_session()?;
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
                if let Err(e) = ui.build_tray(&cmd_tx) {
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

/// What the previous snapshot said about the session, so a completion can be
/// told apart from a stop the user asked for.
#[derive(Debug, Clone)]
struct LastSession {
    id: uuid::Uuid,
    profile: String,
    duration: Duration,
    remaining: Duration,
}

struct Ui {
    tray: Option<TrayIcon>,
    menu: Menu,
    header: MenuItem,
    info: MenuItem,
    usage: MenuItem,
    add_time: Submenu,
    stop: MenuItem,
    start_daemon: MenuItem,
    login: CheckMenuItem,
    mode_menus: Vec<Submenu>,
    mode_key: String,
    default_duration: Duration,
    general: Option<crate::config::General>,
    mark: Mark,
    last_session: Option<LastSession>,
    /// False until the first snapshot has been seen; see [`Ui::announce`].
    primed: bool,
    /// Built once the app is running: the notification center wants a live
    /// NSApplication before it will hand out permission prompts.
    notifier: Option<Notifier>,
}

impl Ui {
    fn new() -> Result<Self> {
        let menu = Menu::new();
        let header =
            MenuItem::with_id("header", crate::i18n::t!("menubar.connecting"), false, None);
        let info = MenuItem::with_id("info", "—", false, None);
        let usage =
            MenuItem::with_id("usage", crate::i18n::t!("menubar.usage_unknown"), false, None);
        let add_time = Submenu::with_id("add_time", crate::i18n::t!("menubar.add_time"), false);
        for mins in EXTEND_CHOICES {
            let d = fmt_duration(Duration::from_secs(mins * 60));
            let item = MenuItem::with_id(
                format!("extend:{}", mins * 60),
                crate::i18n::t!("menubar.add_minutes", duration = d),
                true,
                None,
            );
            add_time.append(&item).map_err(|e| Error::Other(e.to_string()))?;
        }
        let stop = MenuItem::with_id("stop", crate::i18n::t!("menubar.stop"), false, None);
        let start_daemon =
            MenuItem::with_id("start_daemon", crate::i18n::t!("menubar.start_daemon"), false, None);
        let open_tui =
            MenuItem::with_id("open_tui", crate::i18n::t!("menubar.open_tui"), true, None);
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
        let items: [&dyn IsMenuItem; 12] = [
            &header,
            &info,
            &usage,
            &sep_top,
            // Mode submenus are inserted at MODES_AT (after sep_top) once
            // the first full snapshot arrives.
            &sep_modes,
            &add_time,
            &stop,
            &start_daemon,
            &sep_bottom,
            &open_tui,
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
            usage,
            add_time,
            stop,
            start_daemon,
            login,
            mode_menus: Vec::new(),
            mode_key: String::new(),
            default_duration: Duration::from_secs(25 * 60),
            general: None,
            mark: Mark::Idle,
            last_session: None,
            primed: false,
            notifier: None,
        })
    }

    fn build_tray(&mut self, cmd_tx: &mpsc::Sender<Cmd>) -> Result<()> {
        if let Some(mtm) = objc2::MainThreadMarker::new() {
            let tx = cmd_tx.clone();
            self.notifier = Some(Notifier::new(mtm, move |action| {
                let cmd = match action {
                    notify::Action::Extend(by) => Cmd::Extend { by },
                    notify::Action::Stop => Cmd::Stop,
                };
                let _ = tx.send(cmd);
            }));
        }

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(self.menu.clone()))
            .with_icon(icon::render(Mark::Idle))
            .with_icon_as_template(true)
            .with_tooltip("monk")
            .build()
            .map_err(|e| Error::Other(e.to_string()))?;
        self.tray = Some(tray);
        Ok(())
    }

    fn apply(&mut self, snap: Snapshot) {
        if let Some(general) = &snap.general {
            self.default_duration = general.default_duration;
            self.general = Some(general.clone());
        }
        if let Some(modes) = &snap.modes {
            self.rebuild_modes(modes);
        }
        self.announce(&snap);
        self.render(&snap);
    }

    /// The daemon has no notifier of its own, so the menu bar app is where a
    /// finished session gets announced. A session the user stopped by hand
    /// stays silent — they were already looking at the screen.
    fn announce(&mut self, snap: &Snapshot) {
        /// A session whose last countdown was inside this window ran out on
        /// its own; anything longer was a stop the user asked for, and they
        /// were already looking at the screen.
        const COMPLETION_SLACK: Duration = Duration::from_secs(5);
        /// The one warning before the end, so a session never just stops.
        const ENDGAME: Duration = Duration::from_secs(5 * 60);

        // A snapshot that never reached the daemon says nothing about the
        // session — remembering its emptiness would turn every blip into a
        // "session ended" followed by a "session started".
        if !snap.daemon_ok {
            return;
        }
        let prev = self.last_session.take();
        if let Some(s) = &snap.session {
            self.last_session = Some(LastSession {
                id: s.id,
                profile: s.profile.clone(),
                duration: s.duration,
                remaining: s.remaining(),
            });
        }
        // The first snapshot that reaches the daemon describes a world the
        // app was not watching; announcing it would fire on every login.
        if !std::mem::replace(&mut self.primed, true) {
            return;
        }

        match (prev, &snap.session) {
            // A start the user may not have made themselves — the scheduler
            // fires sessions too.
            (prev, Some(now)) if prev.as_ref().is_none_or(|p| p.id != now.id) => {
                let d = fmt_duration(now.duration);
                let p = now.profile.clone();
                let title = crate::i18n::t!("menubar.notify_start_title");
                let body = crate::i18n::t!("menubar.notify_start_body", profile = p, duration = d);
                self.post(&title, &body, session_kind(snap));
            }
            (Some(prev), Some(now)) => {
                let left = now.remaining();
                if prev.remaining > ENDGAME && left <= ENDGAME {
                    let d = fmt_remaining(left);
                    let p = now.profile.clone();
                    let title = crate::i18n::t!("menubar.notify_endgame_title");
                    let body =
                        crate::i18n::t!("menubar.notify_endgame_body", profile = p, duration = d);
                    self.post(&title, &body, session_kind(snap));
                }
            }
            (Some(prev), None) if prev.remaining <= COMPLETION_SLACK => {
                let d = fmt_duration(prev.duration);
                let p = prev.profile;
                let title = crate::i18n::t!("menubar.notify_done_title");
                let body = crate::i18n::t!("menubar.notify_done_body", profile = p, duration = d);
                self.post(&title, &body, Kind::Plain);
            }
            _ => {}
        }
    }

    /// Buttons only make sense while there is a session to act on, so the
    /// caller picks the kind; outside the app bundle both kinds degrade to a
    /// plain scripted notification.
    fn post(&self, title: &str, body: &str, kind: Kind) {
        match &self.notifier {
            Some(notifier) => notifier.post(title, body, kind),
            None => crate::platform::notify(title, body),
        }
    }

    fn render(&mut self, snap: &Snapshot) {
        let hard_active = snap.hard.is_some();
        let session_active = snap.session.is_some();
        let panic_pending = snap.hard.as_ref().is_some_and(|h| h.panic_releases_at.is_some());

        // Header + tray title. The title is always written, empty string
        // included: on macOS `set_title(None)` is a no-op, so skipping it
        // would leave the last countdown frozen next to the icon.
        let mut title = String::new();
        if !snap.daemon_ok {
            self.header.set_text(crate::i18n::t!("menubar.daemon_offline"));
        } else if let Some(s) = &snap.session {
            let remaining = fmt_remaining(s.remaining());
            let p = s.profile.clone();
            let r = remaining.clone();
            self.header.set_text(crate::i18n::t!("menubar.active", profile = p, remaining = r));
            title = remaining;
        } else {
            self.header.set_text(crate::i18n::t!("menubar.idle"));
        }

        // Info line: panic countdown > hard-mode hint > end of the running
        // session > next schedule.
        if let Some(h) = &snap.hard {
            if let Some(release) = h.panic_releases_at {
                let at = release.with_timezone(&Local).format("%H:%M").to_string();
                self.info.set_text(crate::i18n::t!("menubar.panic_pending", time = at));
            } else {
                self.info.set_text(crate::i18n::t!("menubar.hard_hint"));
            }
        } else if let Some(s) = &snap.session {
            let at = s.ends_at().with_timezone(&Local).format("%H:%M").to_string();
            self.info.set_text(crate::i18n::t!("menubar.until", time = at));
        } else if let Some((Some(profile), Some(at))) = &snap.next_scheduled {
            let at = at.with_timezone(&Local).format("%a %H:%M").to_string();
            let p = profile.clone();
            self.info.set_text(crate::i18n::t!("menubar.next_scheduled", profile = p, time = at));
        } else if !snap.daemon_ok {
            self.info.set_text(crate::i18n::t!("menubar.offline_hint"));
        } else {
            self.info.set_text(crate::i18n::t!("menubar.no_schedule"));
        }

        if let Some(used) = snap.used_24h {
            if used.is_zero() {
                self.usage.set_text(crate::i18n::t!("menubar.usage_none"));
            } else {
                let d = fmt_duration(used);
                self.usage.set_text(crate::i18n::t!("menubar.usage_24h", duration = d));
            }
        }

        self.add_time.set_enabled(snap.daemon_ok && session_active && !panic_pending);
        self.stop.set_enabled(snap.daemon_ok && session_active && !hard_active);
        self.start_daemon.set_enabled(!snap.daemon_ok);
        for m in &self.mode_menus {
            m.set_enabled(snap.daemon_ok && !session_active);
        }

        let mark = match (session_active, hard_active) {
            (true, true) => Mark::Hard,
            (true, false) => Mark::Active,
            _ => Mark::Idle,
        };
        if let Some(tray) = &mut self.tray {
            tray.set_title(Some(&title));
            if mark != self.mark {
                if let Err(e) = tray.set_icon_with_as_template(Some(icon::render(mark)), true) {
                    tracing::warn!(error = %e, "could not swap the status icon");
                } else {
                    self.mark = mark;
                }
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
        for secs in START_CHOICES {
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
        let _ = sub.append(&PredefinedMenuItem::separator());
        // Disabled on the mode that already holds the star, so the item
        // doubles as a read-out of which mode is the default.
        let _ = sub.append(&MenuItem::with_id(
            format!("default:{}", mode.name),
            crate::i18n::t!("menubar.make_default"),
            !mode.is_default,
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
            "open_tui" => open_tui(),
            "login" => {
                let result = if agent::installed() {
                    agent::uninstall()
                } else {
                    agent::install().map(|_| ())
                };
                if let Err(e) = result {
                    tracing::error!(error = %e, "toggling login item failed");
                }
                self.login.set_checked(agent::installed());
            }
            other => {
                if let Some(profile) = other.strip_prefix("default:") {
                    let _ = cmd_tx.send(Cmd::SetDefault { profile: profile.to_string() });
                } else if let Some(secs) =
                    other.strip_prefix("extend:").and_then(|s| s.parse::<u64>().ok())
                {
                    let _ = cmd_tx.send(Cmd::Extend { by: Duration::from_secs(secs) });
                } else if let Some(rest) = other.strip_prefix("start:") {
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

/// Which buttons a session notification should carry. Hard mode refuses
/// every stop, so offering one would be a button that only ever fails.
fn session_kind(snap: &Snapshot) -> Kind {
    if snap.hard.is_some() {
        Kind::HardSession
    } else {
        Kind::Session
    }
}

/// Hands the user the full TUI. There is no window to fall back on, so a
/// terminal is the only place the rest of monk lives.
fn open_tui() {
    let Ok(exe) = std::env::current_exe() else {
        tracing::warn!("cannot resolve the monk binary path");
        return;
    };
    let quoted = exe.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        "tell application \"Terminal\"\nactivate\ndo script \"\\\"{quoted}\\\" tui\"\nend tell"
    );
    let spawned = std::process::Command::new("osascript")
        .args(["-e", &script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "could not open Terminal");
    }
}

/// Moving the star means replacing the whole `general` section, so the
/// current one is read back first and only `default_profile` is touched.
fn set_default_request(rt: &tokio::runtime::Runtime, profile: String) -> Option<Request> {
    match rt.block_on(ipc::send(&Request::GetGeneral)) {
        Ok(Response::General(mut general)) => {
            general.default_profile = profile;
            Some(Request::UpdateGeneral { general })
        }
        Ok(Response::Error { message }) => {
            report_rejection(&message);
            None
        }
        Ok(_) => None,
        Err(e) => {
            report_rejection(&e.to_string());
            None
        }
    }
}

/// A rejected click has nowhere to show itself in a menu, so the daemon's
/// reason goes to Notification Center instead of only to the log.
fn report_rejection(message: &str) {
    tracing::warn!(%message, "menu bar action rejected");
    crate::platform::notify(&crate::i18n::t!("menubar.action_failed"), message);
}

fn worker(rx: mpsc::Receiver<Cmd>, proxy: EventLoopProxy<UserEvent>) {
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!(error = %e, "menubar worker: tokio runtime");
            return;
        }
    };
    // `Instant` is anchored at boot on macOS and subtracting past it panics;
    // a login item can start with seconds of uptime. `None` also states the
    // thing directly: no full refresh has happened yet.
    let mut last_full: Option<Instant> = None;
    loop {
        let mut force_full = false;
        let request = match rx.recv_timeout(POLL_INTERVAL) {
            Ok(Cmd::Start { profile, duration, hard }) => {
                Some(Request::Start { profile, duration, hard_mode: hard, reason: None })
            }
            Ok(Cmd::Extend { by }) => Some(Request::Extend { by }),
            Ok(Cmd::SetDefault { profile }) => set_default_request(&rt, profile),
            Ok(Cmd::Stop) => Some(Request::Stop { id: None }),
            Ok(Cmd::StartDaemon) => {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::process::Command::new(exe).args(["daemon", "start"]).status();
                }
                force_full = true;
                None
            }
            Ok(Cmd::Refresh) => None,
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        if let Some(req) = request {
            match rt.block_on(ipc::send(&req)) {
                Ok(Response::Error { message }) => {
                    report_rejection(&ipc::explain_rejection(message))
                }
                Ok(Response::HardModeActive(info)) => {
                    let left = fmt_duration(info.remaining);
                    report_rejection(&crate::i18n::t!("menubar.hard_denied", remaining = left));
                }
                Err(e) => report_rejection(&e.to_string()),
                Ok(_) => {}
            }
            force_full = true;
        }

        let full = force_full || last_full.is_none_or(|at| at.elapsed() >= FULL_REFRESH);
        let snap = rt.block_on(fetch(full));
        if full && snap.daemon_ok {
            last_full = Some(Instant::now());
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
        general: None,
        used_24h: None,
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
        snap.used_24h = Some(modes.iter().fold(Duration::ZERO, |acc, m| acc + m.stats.used_24h));
        snap.modes = Some(modes);
    }
    if let Ok(Response::NextScheduled { profile, at }) = ipc::send(&Request::NextScheduled).await {
        snap.next_scheduled = Some((profile, at));
    }
    if let Ok(Response::General(general)) = ipc::send(&Request::GetGeneral).await {
        snap.general = Some(general);
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

/// Formats the countdown of a session that is still running. Rounds up, so
/// the last minute reads `1m` rather than `0m` and the last second reads
/// `1s` rather than `0s` — a running session never claims to have no time
/// left, and a finished one shows nothing at all.
fn fmt_remaining(d: Duration) -> String {
    let secs = d.as_secs().max(u64::from(d.subsec_nanos() > 0));
    if secs >= 60 {
        fmt_duration(Duration::from_secs(secs.div_ceil(60) * 60))
    } else {
        format!("{}s", secs.max(1))
    }
}

fn lock_path() -> Result<std::path::PathBuf> {
    // Per-user dir on purpose: in system mode data_dir() is root-owned and
    // the menu bar app runs as the logged-in user.
    Ok(crate::paths::user_cache_dir()?.join("menubar.lock"))
}

fn single_instance_lock() -> Result<fs_err::File> {
    use fs2::FileExt;
    use std::io::{Seek, Write};

    let mut file =
        fs_err::OpenOptions::new().create(true).truncate(false).write(true).open(lock_path()?)?;
    file.file()
        .try_lock_exclusive()
        .map_err(|_| Error::Other(crate::i18n::t!("menubar.already_running").to_string()))?;
    // The pid is what lets `monk menubar install` retire the instance that
    // is holding this lock; without it an upgrade would silently leave the
    // old binary running, since the new copy just exits on the lock.
    file.set_len(0)?;
    file.seek(std::io::SeekFrom::Start(0))?;
    write!(file, "{}", std::process::id())?;
    file.flush()?;
    Ok(file)
}

/// Pid of the running menu bar app, if any. The lock file carries it, and a
/// dead pid is filtered out with signal 0 so a stale file reads as "nothing
/// is running".
fn running_pid() -> Option<nix::unistd::Pid> {
    use fs2::FileExt;

    let path = lock_path().ok()?;
    let raw: i32 = fs_err::read_to_string(&path).ok()?.trim().parse().ok()?;
    if raw <= 1 || raw == std::process::id() as i32 {
        return None;
    }
    // The number alone proves nothing: pids are recycled, and a crashed app
    // leaves its own behind. The lock is the evidence — if it can be taken,
    // no menu bar app is running and that pid now belongs to a stranger we
    // must not signal.
    let file = fs_err::OpenOptions::new().write(true).open(&path).ok()?;
    if file.file().try_lock_exclusive().is_ok() {
        let _ = FileExt::unlock(file.file());
        return None;
    }
    let pid = nix::unistd::Pid::from_raw(raw);
    nix::sys::signal::kill(pid, None).ok()?;
    Some(pid)
}

/// Stops a menu bar app that is already running, whoever started it, and
/// waits for it to let go of the lock. Returns once no instance holds it.
pub(crate) fn stop_running() {
    use fs2::FileExt;

    let Some(pid) = running_pid() else { return };
    let Ok(path) = lock_path() else { return };
    if nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM).is_err() {
        return;
    }
    // Give it a moment to drop the lock; the loop exits as soon as the lock
    // is free, so the common case costs one 50ms sleep at most.
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(50));
        let Ok(file) = fs_err::OpenOptions::new().write(true).open(&path) else { return };
        if file.file().try_lock_exclusive().is_ok() {
            let _ = FileExt::unlock(file.file());
            return;
        }
    }
    tracing::warn!("the running menu bar app did not exit; the new one may refuse to start");
}

/// The menu bar app belongs to a login session. Run as root it would write
/// its login item and its bundle into `/var/root`, register with launchd's
/// root GUI domain, and show nothing to anybody.
pub(crate) fn require_user_session() -> Result<()> {
    if nix::unistd::geteuid().is_root() {
        return Err(Error::Permission(crate::i18n::t!("menubar.needs_user").to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_session_never_shows_zero() {
        assert_eq!(fmt_remaining(Duration::from_millis(400)), "1s");
        assert_eq!(fmt_remaining(Duration::from_secs(1)), "1s");
        assert_eq!(fmt_remaining(Duration::from_secs(59)), "59s");
    }

    #[test]
    fn the_countdown_rounds_minutes_up() {
        assert_eq!(fmt_remaining(Duration::from_secs(60)), "1m");
        assert_eq!(fmt_remaining(Duration::from_secs(61)), "2m");
        assert_eq!(fmt_remaining(Duration::from_secs(25 * 60)), "25m");
        assert_eq!(fmt_remaining(Duration::from_secs(90 * 60)), "1h30m");
    }
}
