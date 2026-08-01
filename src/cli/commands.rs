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
            let dur_h = humantime::format_duration(s.duration).to_string();
            println!("started `{}` for {dur_h}", s.profile);
            if hard_mode {
                println!("{}", crate::i18n::t!("hard.started_note"));
            }
            if is_freshly_started(&s) {
                crate::platform::notify(
                    "monk: focus started",
                    &format!("`{}` for {dur_h}", s.profile),
                );
            }
            Ok(())
        }
        Response::Error { message } => Err(Error::Other(message)),
        _ => Err(Error::Ipc("unexpected response".into())),
    }
}

/// True when a session was started within the last few seconds. Guards
/// against duplicate notifications if a future daemon refactor decides to
/// echo back an already-active session instead of erroring on a re-`start`.
///
/// Asymmetric on purpose: a negative delta means the daemon clock is ahead
/// of the CLI clock (NTP/suspend/container skew). That's not a fresh start
/// from the CLI's frame of reference — suppress the notification rather
/// than firing it spuriously.
fn is_freshly_started(s: &crate::session::Session) -> bool {
    let age = chrono::Utc::now().signed_duration_since(s.started_at);
    let secs = age.num_seconds();
    (0..5).contains(&secs)
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
            crate::platform::notify("monk: session ended", &format!("`{}`", s.profile));
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
                println!("panic phrase: {}", h.panic_phrase);
                println!("    invoke: monk panic \"{}\"", h.panic_phrase);
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

pub async fn profile_create(name: &str, preset: Option<&str>) -> Result<()> {
    let mut cfg = load_cfg_via_daemon().await?;
    if cfg.profiles.contains_key(name) {
        return Err(Error::Config(format!("profile `{name}` already exists")));
    }
    let profile = if let Some(pname) = preset {
        crate::onboarding::lookup_preset(pname).ok_or_else(|| {
            Error::Config(format!(
                "unknown preset `{pname}` — try one of: {}",
                crate::onboarding::PRESET_NAMES.join(", ")
            ))
        })?
    } else {
        crate::config::Profile::default()
    };
    cfg.profiles.insert(name.to_string(), profile);
    save_cfg_via_daemon(cfg).await?;
    let hint = match preset {
        Some(p) => format!("seeded from preset `{p}`"),
        None => format!("run `monk profile edit {name}` to populate"),
    };
    println!("created profile `{name}` — {hint}");
    Ok(())
}

pub async fn profile_duplicate(source: &str, target: Option<&str>) -> Result<()> {
    let mut cfg = load_cfg_via_daemon().await?;
    let src = cfg
        .profiles
        .get(source)
        .ok_or_else(|| Error::Config(format!("profile `{source}` not found")))?
        .clone();
    let base = target.unwrap_or(source).to_string();
    let taken: std::collections::BTreeSet<String> = cfg.profiles.keys().cloned().collect();
    let new_name = if target.is_none() || taken.contains(&base) {
        unique_dup_name(&base, &taken)
    } else {
        base
    };
    cfg.profiles.insert(new_name.clone(), src);
    save_cfg_via_daemon(cfg).await?;
    println!("duplicated `{source}` → `{new_name}`");
    Ok(())
}

fn unique_dup_name(base: &str, taken: &std::collections::BTreeSet<String>) -> String {
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

pub async fn profile_show(name: &str, json: bool) -> Result<()> {
    let cfg = load_cfg_via_daemon().await?;
    let profile = cfg
        .profiles
        .get(name)
        .ok_or_else(|| Error::Config(format!("profile `{name}` not found")))?;
    if json {
        let payload = serde_json::json!({
            "name": name,
            "profile": profile,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }
    println!("profile: {name}");
    if let Some(c) = &profile.color {
        println!("  color: {c}");
    }
    let l = &profile.limits;
    println!("  limits:");
    println!("    max:       {}", fmt_opt(l.max_duration));
    println!("    min:       {}", fmt_opt(l.min_duration));
    println!("    cooldown:  {}", fmt_opt(l.cooldown));
    println!("    daily cap: {}", fmt_opt(l.daily_cap));
    println!("    panic delay: {}", fmt_opt(l.panic_delay));
    if let Some(sch) = &profile.schedule {
        println!("  schedule: {:?}", sch);
    }
    println!("  apps ({}):", profile.apps.len());
    for a in &profile.apps {
        println!("    · {a}");
    }
    println!("  site groups ({}):", profile.site_groups.len());
    for g in &profile.site_groups {
        let size = crate::sites::all_groups()
            .ok()
            .and_then(|all| all.iter().find(|x| &x.qualified() == g).map(|x| x.hosts.len()))
            .unwrap_or(0);
        println!("    · {g}  ({size} hosts)");
    }
    println!("  custom sites ({}):", profile.sites.len());
    for s in &profile.sites {
        println!("    · {s}");
    }
    println!("  brands ({}):", profile.brands.len());
    for b in &profile.brands {
        println!("    · {b}");
    }
    if !profile.hooks.before.is_empty() || !profile.hooks.after.is_empty() {
        println!("  hooks:");
        for h in &profile.hooks.before {
            println!("    before: {h}");
        }
        for h in &profile.hooks.after {
            println!("    after:  {h}");
        }
    }
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

pub async fn stats() -> Result<()> {
    // For now we render per-mode summary: today's used + budget left, and
    // each mode's blocked-apps/sites/groups counts. Full history with
    // sessions list would need a new daemon RPC; this is the 80% answer.
    let resp = ipc::send(&Request::ListModes).await?;
    let Response::Modes { modes } = resp else {
        return Err(Error::Ipc("unexpected response from daemon".into()));
    };
    if modes.is_empty() {
        println!("no modes configured");
        return Ok(());
    }
    println!("{:<24} {:>8} {:>10} {:>10}", "mode", "today", "budget", "blocked");
    for m in &modes {
        let used = humantime::format_duration(m.stats.used_24h).to_string();
        let budget = m
            .stats
            .daily_cap_remaining
            .map(|d| humantime::format_duration(d).to_string())
            .unwrap_or_else(|| "—".into());
        let blocked = format!("{}a/{}g/{}s", m.blocked_apps, m.blocked_groups, m.blocked_sites);
        println!("{:<24} {:>8} {:>10} {:>10}", m.name, used, budget, blocked);
    }
    Ok(())
}

pub async fn tui() -> Result<()> {
    crate::tui::run().await
}

pub async fn daemon_run() -> Result<()> {
    crate::daemon::run().await
}

pub async fn daemon_start() -> Result<()> {
    crate::daemon::spawn_detached()?;
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

pub fn daemon_install(reinstall: bool) -> Result<()> {
    if reinstall {
        match crate::daemon::service_run(crate::daemon::ServiceAction::Uninstall { purge: false }) {
            Ok(msg) => println!("{msg}"),
            Err(e) => eprintln!("uninstall step (continuing): {e}"),
        }
    }
    let msg = crate::daemon::service_run(crate::daemon::ServiceAction::Install)?;
    println!("{msg}");
    Ok(())
}

pub async fn daemon_uninstall(purge: bool) -> Result<()> {
    let msg = crate::daemon::service_run(crate::daemon::ServiceAction::Uninstall { purge })?;
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

pub async fn doctor(json: bool, fix: bool) -> Result<()> {
    if fix {
        return crate::doctor::run_fix().await;
    }
    let report = crate::doctor::run().await;
    if json {
        // CI/script mode: emit the full report as one JSON object on stdout.
        // Exit code still reflects has_failures so `if monk doctor --json | ...`
        // can both pipe and short-circuit on red.
        let payload = serde_json::json!({
            "checks": report.checks,
            "duration_ms": report.duration.as_millis(),
            "summary": {
                "ok": report.summary().0,
                "warn": report.summary().1,
                "fail": report.summary().2,
            },
            "has_failures": report.has_failures(),
        });
        println!("{}", serde_json::to_string(&payload)?);
        if report.has_failures() {
            std::process::exit(1);
        }
        return Ok(());
    }
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

pub async fn update(check_only: bool) -> Result<()> {
    println!("{}", crate::i18n::t!("update.checking"));
    let status = tokio::task::spawn_blocking(|| crate::update::check(true))
        .await
        .map_err(|e| crate::Error::Other(e.to_string()))??;
    let cur = crate::update::CURRENT_VERSION;
    let latest = status.latest.clone();
    if !status.newer {
        println!("{}", crate::i18n::t!("update.up_to_date", current = cur, latest = latest));
        return Ok(());
    }
    println!("{}", crate::i18n::t!("update.available", current = cur, latest = latest));
    if check_only {
        println!("{}", crate::i18n::t!("update.run_hint"));
        return Ok(());
    }
    println!("{}", crate::i18n::t!("update.downloading", latest = latest));
    let outcome = tokio::task::spawn_blocking(crate::update::perform_update)
        .await
        .map_err(|e| crate::Error::Other(e.to_string()))??;
    let path = outcome.exe.display().to_string();
    println!("{}", crate::i18n::t!("update.updated", version = outcome.version, path = path));
    println!("{}", crate::i18n::t!("update.daemon_hint"));
    Ok(())
}
