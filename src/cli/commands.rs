use std::time::Duration;

use crate::{
    config::Config,
    ipc::{self, Request, Response},
    Error, Result,
};

async fn load_cfg_via_daemon() -> Result<Config> {
    match ipc::send(&Request::GetConfig).await {
        Ok(Response::Config(c)) => Ok(*c),
        Ok(Response::Error { message }) => Err(Error::Other(message)),
        Ok(_) => Err(Error::Ipc("unexpected response".into())),
        Err(e) => Err(e),
    }
}

async fn save_cfg_via_daemon(cfg: Config) -> Result<()> {
    let req = Request::SaveConfig { config: Box::new(cfg) };
    match ipc::send(&req).await {
        Ok(Response::Ok) => Ok(()),
        Ok(Response::Error { message }) => Err(Error::Other(message)),
        Ok(_) => Err(Error::Ipc("unexpected response".into())),
        Err(e) => Err(e),
    }
}

pub async fn start(
    profile: Option<String>,
    duration: Option<Duration>,
    hard: bool,
    reason: Option<String>,
) -> Result<()> {
    let cfg = Config::load()?;
    let profile = profile.unwrap_or_else(|| cfg.general.default_profile.clone());
    let duration = duration.unwrap_or(cfg.general.default_duration);
    let hard_mode = hard || cfg.general.hard_mode;
    if hard_mode {
        eprintln!(
            "{}",
            crate::i18n::t!(
                "hard.ceremony_warning",
                duration = humantime::format_duration(duration).to_string()
            )
        );
        let confirm_text = crate::i18n::t!("hard.confirm_start").to_string();
        let yes = crate::i18n::t!("common.yes").to_string();
        let no = crate::i18n::t!("common.no").to_string();
        let ans = inquire::Select::new(&confirm_text, vec![no.clone(), yes.clone()])
            .with_starting_cursor(0)
            .prompt()
            .map_err(|e| Error::Other(e.to_string()))?;
        if ans != yes {
            return Err(Error::Other(crate::i18n::t!("hard.cancelled").to_string()));
        }
    }
    let req = Request::Start { profile: profile.clone(), duration, hard_mode, reason };
    match ipc::send(&req).await? {
        Response::Session(s) => {
            println!("started `{}` for {}", s.profile, humantime::format_duration(s.duration));
            if hard_mode {
                println!("{}", crate::i18n::t!("hard.started_note"));
            }
            Ok(())
        }
        Response::Error { message } => Err(Error::Other(message)),
        _ => Err(Error::Ipc("unexpected response".into())),
    }
}

pub async fn panic_cmd(phrase: Option<String>, cancel: bool) -> Result<()> {
    let phrase = phrase.unwrap_or_default();
    match ipc::send(&Request::Panic { phrase, cancel }).await? {
        Response::PanicScheduled(info) => {
            if let Some(at) = info.panic_releases_at {
                println!("{}", crate::i18n::t!("panic.scheduled", at = at.to_rfc3339()));
            } else {
                println!("{}", crate::i18n::t!("panic.cancelled"));
            }
            Ok(())
        }
        Response::Ok => {
            println!("{}", crate::i18n::t!("panic.cancelled"));
            Ok(())
        }
        Response::Error { message } => Err(Error::Other(message)),
        _ => Err(Error::Ipc("unexpected response".into())),
    }
}

pub async fn stop() -> Result<()> {
    match ipc::send(&Request::Stop { id: None }).await? {
        Response::Session(s) => {
            println!("stopped `{}`", s.profile);
            Ok(())
        }
        Response::HardModeActive(info) => {
            println!(
                "{}",
                crate::i18n::t!(
                    "hard.stop_denied",
                    remaining = humantime::format_duration(info.remaining).to_string()
                )
            );
            Err(Error::HardModeActive)
        }
        Response::Error { message } => Err(Error::Other(message)),
        _ => Err(Error::Ipc("unexpected response".into())),
    }
}

pub async fn status() -> Result<()> {
    match ipc::send(&Request::Status).await {
        Ok(Response::Status { active, hard_mode, pid }) => {
            println!("daemon: running (pid {pid})");
            if let Some(s) = active {
                println!(
                    "active: {} ({} remaining)",
                    s.profile,
                    humantime::format_duration(s.remaining())
                );
            } else {
                println!("active: none");
            }
            if let Some(h) = hard_mode {
                println!("hard mode: on ({} remaining)", humantime::format_duration(h.remaining));
                if let Some(at) = h.panic_releases_at {
                    println!("panic releases at: {}", at.to_rfc3339());
                }
            }
            Ok(())
        }
        Ok(Response::Error { message }) => Err(Error::Other(message)),
        Ok(_) => Err(Error::Ipc("unexpected response".into())),
        Err(Error::DaemonNotRunning) => {
            println!("daemon: not running");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

pub async fn profiles() -> Result<()> {
    let cfg = load_cfg_via_daemon().await?;
    if cfg.profiles.is_empty() {
        println!("no profiles defined");
        return Ok(());
    }
    for (name, p) in &cfg.profiles {
        println!(
            "{name}: {} sites, {} groups, {} apps",
            p.sites.len(),
            p.site_groups.len(),
            p.apps.len()
        );
    }
    Ok(())
}

pub fn apps_list(refresh: bool) -> Result<()> {
    let cache = crate::apps::load_or_scan(refresh)?;
    println!("scanned {} — {} apps", cache.scanned_at.to_rfc3339(), cache.apps.len());
    for app in &cache.apps {
        println!("  {} [{}] -> {}", app.label, app.id, app.exec_path.display());
    }
    Ok(())
}

pub fn apps_scan() -> Result<()> {
    let cache = crate::apps::load_or_scan(true)?;
    println!("scanned {} apps", cache.apps.len());
    Ok(())
}

pub async fn profile_create(name: &str) -> Result<()> {
    let mut cfg = load_cfg_via_daemon().await?;
    if cfg.profiles.contains_key(name) {
        return Err(Error::Config(format!("profile `{name}` already exists")));
    }
    cfg.profiles.insert(name.to_string(), crate::config::Profile::default());
    save_cfg_via_daemon(cfg).await?;
    println!("created profile `{name}` — run `monk profile edit {name}` to populate");
    Ok(())
}

pub async fn profile_limits(
    name: &str,
    max: Option<String>,
    min: Option<String>,
    cooldown: Option<String>,
    daily_cap: Option<String>,
    clear: bool,
) -> Result<()> {
    let mut cfg = load_cfg_via_daemon().await?;
    let profile = cfg
        .profiles
        .get_mut(name)
        .ok_or_else(|| Error::Config(format!("profile `{name}` not found")))?;
    if clear {
        profile.limits = crate::config::Limits::default();
    }
    let parse = |s: String| humantime::parse_duration(&s).map_err(|e| Error::Config(e.to_string()));
    if let Some(v) = max {
        profile.limits.max_duration = Some(parse(v)?);
    }
    if let Some(v) = min {
        profile.limits.min_duration = Some(parse(v)?);
    }
    if let Some(v) = cooldown {
        profile.limits.cooldown = Some(parse(v)?);
    }
    if let Some(v) = daily_cap {
        profile.limits.daily_cap = Some(parse(v)?);
    }
    let snapshot = profile.limits.clone();
    save_cfg_via_daemon(cfg).await?;
    println!(
        "limits for `{name}`: max={} min={} cooldown={} daily_cap={}",
        fmt_opt(snapshot.max_duration),
        fmt_opt(snapshot.min_duration),
        fmt_opt(snapshot.cooldown),
        fmt_opt(snapshot.daily_cap),
    );
    Ok(())
}

fn fmt_opt(d: Option<Duration>) -> String {
    match d {
        Some(v) => humantime::format_duration(v).to_string(),
        None => "-".into(),
    }
}

pub async fn profile_delete(name: &str) -> Result<()> {
    let mut cfg = load_cfg_via_daemon().await?;
    if cfg.profiles.remove(name).is_none() {
        return Err(Error::Config(format!("profile `{name}` not found")));
    }
    if cfg.general.default_profile == name {
        cfg.general.default_profile = cfg.profiles.keys().next().cloned().unwrap_or_default();
    }
    save_cfg_via_daemon(cfg).await?;
    println!("deleted profile `{name}`");
    Ok(())
}

pub async fn profile_edit(name: &str, add: Vec<String>, remove: Vec<String>) -> Result<()> {
    use std::io::IsTerminal;

    let mut cfg = load_cfg_via_daemon().await?;
    if !cfg.profiles.contains_key(name) {
        return Err(Error::Config(format!("profile `{name}` not found")));
    }

    if !add.is_empty() || !remove.is_empty() {
        let profile = cfg.profiles.get_mut(name).expect("checked");
        for id in &remove {
            profile.apps.retain(|a| a != id);
        }
        for id in add {
            if !profile.apps.contains(&id) {
                profile.apps.push(id);
            }
        }
        save_cfg_via_daemon(cfg).await?;
        println!("profile `{name}` updated");
        return Ok(());
    }

    if !std::io::stdin().is_terminal() {
        return Err(Error::Other("profile edit requires a TTY (or use --add/--remove)".into()));
    }

    let cache = crate::apps::load_or_scan(false)?;
    let profile = cfg.profiles.get(name).expect("checked").clone();

    let selected_apps = pick_apps(&profile, &cache)?;
    let selected_groups = pick_site_groups(&profile)?;
    let custom_sites = pick_custom_sites(&profile)?;

    let profile = cfg.profiles.get_mut(name).expect("checked");
    profile.apps = selected_apps;
    profile.site_groups = selected_groups;
    profile.sites = custom_sites;
    save_cfg_via_daemon(cfg).await?;
    println!("profile `{name}` saved");
    Ok(())
}

fn pick_apps(
    profile: &crate::config::Profile,
    cache: &crate::apps::AppCache,
) -> Result<Vec<String>> {
    use inquire::MultiSelect;

    #[derive(Clone)]
    struct Row {
        id: String,
        display: String,
        stale: bool,
    }
    impl std::fmt::Display for Row {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.display)
        }
    }

    let mut rows: Vec<Row> = cache
        .apps
        .iter()
        .map(|a| Row { id: a.id.clone(), display: format!("{} [{}]", a.label, a.id), stale: false })
        .collect();
    for id in &profile.apps {
        if !cache.apps.iter().any(|a| &a.id == id) {
            rows.push(Row { id: id.clone(), display: format!("[removed] {id}"), stale: true });
        }
    }

    let default_indices: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| !r.stale && profile.apps.contains(&r.id))
        .map(|(i, _)| i)
        .collect();

    let prompt =
        "Select apps to block (space to toggle, enter to confirm). Stale entries marked [removed]";
    let chosen = MultiSelect::new(prompt, rows)
        .with_default(&default_indices)
        .with_page_size(15)
        .prompt()
        .map_err(|e| Error::Other(e.to_string()))?;
    Ok(chosen.into_iter().map(|r| r.id).collect())
}

fn pick_site_groups(profile: &crate::config::Profile) -> Result<Vec<String>> {
    use inquire::MultiSelect;

    #[derive(Clone)]
    struct Row {
        id: String,
        display: String,
    }
    impl std::fmt::Display for Row {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.display)
        }
    }

    let groups = crate::sites::all_groups()?;
    let rows: Vec<Row> = groups
        .iter()
        .map(|g| Row {
            id: g.qualified(),
            display: format!("{:<20} {} ({} hosts)", g.qualified(), g.label, g.hosts.len()),
        })
        .collect();
    let default_indices: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| profile.site_groups.contains(&r.id))
        .map(|(i, _)| i)
        .collect();
    let chosen = MultiSelect::new("Select site groups to block", rows)
        .with_default(&default_indices)
        .with_page_size(15)
        .prompt()
        .map_err(|e| Error::Other(e.to_string()))?;
    Ok(chosen.into_iter().map(|r| r.id).collect())
}

fn pick_custom_sites(profile: &crate::config::Profile) -> Result<Vec<String>> {
    use inquire::Text;
    let prompt = "Custom hosts to block (comma-separated, leave blank to keep current)";
    let current = profile.sites.join(",");
    let raw = Text::new(prompt)
        .with_default(&current)
        .prompt()
        .map_err(|e| Error::Other(e.to_string()))?;
    Ok(raw.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect())
}

pub fn stats() -> Result<()> {
    println!("stats: coming soon");
    Ok(())
}

pub async fn tui() -> Result<()> {
    crate::tui::run().await
}

pub async fn daemon_run() -> Result<()> {
    crate::daemon::run().await
}

pub async fn daemon_start() -> Result<()> {
    let exe = std::env::current_exe()?;
    #[cfg(unix)]
    {
        use std::process::{Command, Stdio};
        Command::new(exe)
            .args(["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
    }
    #[cfg(windows)]
    {
        use std::process::{Command, Stdio};
        Command::new(exe)
            .args(["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
    }
    println!("monkd starting");
    Ok(())
}

pub async fn daemon_stop() -> Result<()> {
    match ipc::send(&Request::Shutdown).await {
        Ok(_) => {
            println!("monkd stopped");
            Ok(())
        }
        Err(Error::DaemonNotRunning) => {
            println!("monkd not running");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

pub async fn daemon_status() -> Result<()> {
    status().await
}

pub fn daemon_install() -> Result<()> {
    let msg = crate::daemon::service_run(crate::daemon::ServiceAction::Install)?;
    println!("{msg}");
    Ok(())
}

pub fn daemon_uninstall() -> Result<()> {
    let msg = crate::daemon::service_run(crate::daemon::ServiceAction::Uninstall)?;
    println!("{msg}");
    Ok(())
}

pub async fn set_lang(locale: &str) -> Result<()> {
    let mut cfg = load_cfg_via_daemon().await?;
    cfg.general.locale = Some(crate::i18n::normalize(locale).to_string());
    save_cfg_via_daemon(cfg).await?;
    crate::i18n::set(locale);
    println!("language: {}", crate::i18n::current());
    Ok(())
}

pub fn config_path() -> Result<()> {
    println!("{}", crate::paths::config_file()?.display());
    Ok(())
}

pub fn config_export() -> Result<()> {
    let raw = fs_err::read_to_string(crate::paths::config_file()?)?;
    print!("{raw}");
    Ok(())
}

pub async fn config_import(file: &std::path::Path) -> Result<()> {
    let raw = fs_err::read_to_string(file)?;
    let cfg: Config = toml::from_str(&raw)?;
    save_cfg_via_daemon(cfg).await?;
    println!("imported {}", file.display());
    Ok(())
}

pub async fn doctor() -> Result<()> {
    let report = crate::doctor::run().await;
    for c in &report.checks {
        println!("{} [{}] {} — {}", c.status.icon(), c.status.label(), c.title, c.detail);
        for extra in &c.extras {
            println!("      {extra}");
        }
        if let Some(hint) = &c.hint {
            println!("      hint: {hint}");
        }
    }
    let (ok, warn, fail) = report.summary();
    println!();
    println!("summary: {ok} ok · {warn} warn · {fail} fail (took {:.0?})", report.duration);
    if report.has_failures() {
        std::process::exit(1);
    }
    Ok(())
}
