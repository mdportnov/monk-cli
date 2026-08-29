use std::{io::IsTerminal, time::Duration};

use indicatif::{ProgressBar, ProgressStyle};
use inquire::{MultiSelect, Select, Text};

use crate::{
    blocker,
    config::Config,
    daemon::{self, ServiceAction},
    i18n::{self, t},
    ipc::{self, Request, Response},
    onboarding::{
        curated,
        presets::{load_preset, preset_blurb, preset_label, PresetTier, PRESETS, PRESET_NAMES},
    },
    paths, platform, Error, Result,
};

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub locale: Option<String>,
    pub presets: Vec<String>,
    pub duration: Option<Duration>,
    pub hard_mode: Option<bool>,
    pub autostart: Option<bool>,
    pub yes: bool,
    pub reset: bool,
    pub quick: bool,
    pub no_daemon: bool,
    pub no_completions: bool,
    pub no_doctor: bool,
}

pub async fn run(opts: Options) -> Result<()> {
    if opts.reset {
        return reset();
    }

    if !std::io::stdin().is_terminal() || opts.yes {
        return run_non_interactive(opts).await;
    }

    let mut cfg = Config::load().unwrap_or_default();

    let locale = pick_locale(opts.locale.as_deref())?;
    i18n::set(&locale);
    cfg.general.locale = Some(locale);

    banner();

    let presets = pick_presets()?;
    let duration = pick_duration(cfg.general.default_duration)?;

    let (hard_mode, autostart, chosen_apps);
    if opts.quick {
        hard_mode = cfg.general.hard_mode;
        autostart = true;
        let cache = scan_with_spinner()?;
        chosen_apps = curated_only(&cache);
        if !chosen_apps.is_empty() {
            println!(
                "  pre-selected {} common distractor app(s); edit later with `monk profile edit`",
                chosen_apps.len()
            );
        }
    } else {
        hard_mode = pick_hard_mode(cfg.general.hard_mode)?;
        autostart = pick_autostart(cfg.general.autostart)?;
        let cache = scan_with_spinner()?;
        println!("  found {} applications", cache.apps.len());
        chosen_apps = pick_apps_for_presets(&cache)?;
    }

    let (created, kept) = apply(&mut cfg, &presets, duration, hard_mode, autostart)?;
    // Only freshly created profiles get the wizard's app selection —
    // customized existing profiles keep whatever the user configured.
    for preset in &created {
        if let Some(profile) = cfg.profiles.get_mut(preset) {
            profile.apps = chosen_apps.clone();
        }
    }
    print_apply_notices(&cfg, &created, &kept);

    check_hosts();

    // Persist before elevation: the elevated child re-reads config from disk
    // (or relies on its own defaults). If we save *after*, the root child can
    // race the parent and observe stale state.
    cfg.general.initialized = true;
    persist_config(&cfg).await?;

    if !opts.no_daemon {
        if autostart {
            run_service_install();
        } else {
            println!("  {}", t!("onboarding.autostart_skipped"));
        }
    }

    // Install shell completions if not skipped
    if !opts.no_completions {
        install_completions_step();
    }

    // Print health checks if not skipped
    if !opts.no_doctor {
        print_doctor_summary_async().await;
    } else {
        print_doctor_summary();
    }

    farewell()?;
    Ok(())
}

fn scan_with_spinner() -> Result<crate::apps::AppCache> {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("  {spinner:.cyan} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    pb.set_message("scanning installed applications…");
    pb.enable_steady_tick(Duration::from_millis(80));
    let cache = crate::apps::load_or_scan(true);
    pb.finish_and_clear();
    cache
}

fn curated_only(cache: &crate::apps::AppCache) -> Vec<String> {
    cache
        .apps
        .iter()
        .filter(|a| curated::match_curated(a).is_some())
        .map(|a| a.id.clone())
        .collect()
}

/// Persist the wizard's config.
///
/// Once the system service is installed the config file is root-owned
/// (`/Library/Application Support/monk` on macOS), so a direct write from the
/// user's CLI fails with EACCES and the wizard would die on its last step
/// after asking every question. The daemon owns that file — route the save
/// through it, and only fall back to a direct write when it is unreachable.
async fn persist_config(cfg: &Config) -> Result<()> {
    if crate::paths::system_mode() && !nix_is_root() {
        match ipc::send(&Request::SaveConfig { config: Box::new(cfg.clone()) }).await {
            Ok(Response::Ok) => return Ok(()),
            Ok(Response::Error { message }) => {
                return Err(Error::Other(format!("daemon refused the config: {message}")))
            }
            Ok(other) => {
                tracing::warn!(?other, "unexpected SaveConfig response; trying a direct write");
            }
            Err(e) => {
                tracing::warn!(?e, "config save via daemon failed; trying a direct write");
            }
        }
    }
    cfg.save()
}

fn run_service_install() {
    let needs_elevation = cfg!(target_os = "macos") && !nix_is_root();
    let result = if needs_elevation {
        println!("  installing system service (you'll see a macOS admin prompt)…");
        platform::elevate_install_service()
    } else {
        daemon::service_run(ServiceAction::Install)
    };
    match result {
        Ok(msg) => {
            for line in platform::strip_service_markers(&msg).lines() {
                println!("  {line}");
            }
        }
        Err(e) => {
            eprintln!("  autostart setup failed: {e}");
            eprintln!("  → run `monk doctor --fix` later to retry");
        }
    }
}

#[cfg(unix)]
fn nix_is_root() -> bool {
    nix::unistd::geteuid().is_root()
}

#[cfg(not(unix))]
fn nix_is_root() -> bool {
    false
}

/// Run `doctor::run` from a sync onboarding flow without nesting tokio
/// runtimes. The wizard is invoked from inside `#[tokio::main]`, so we ship
/// the async work to a dedicated OS thread with its own current-thread
/// runtime. On thread-spawn or runtime-build failure we log and skip the
/// summary rather than crashing the wizard.
const DOCTOR_SUMMARY_TIMEOUT: Duration = Duration::from_secs(3);

fn print_doctor_summary() {
    let result = std::thread::Builder::new()
        .name("monk-doctor-summary".into())
        .spawn(|| {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::warn!(?e, "doctor summary: tokio runtime build failed");
                    return None;
                }
            };
            // Cap the whole report. A hung IPC check (slow daemon, dead
            // socket) must not block the wizard's last screen.
            rt.block_on(async {
                tokio::time::timeout(DOCTOR_SUMMARY_TIMEOUT, crate::doctor::run()).await.ok()
            })
        })
        .and_then(|h| h.join().map_err(|_| std::io::Error::other("doctor thread panicked")));
    let report = match result {
        Ok(Some(r)) => r,
        Ok(None) => {
            println!();
            println!("  health: skipped — check timed out after {:?}", DOCTOR_SUMMARY_TIMEOUT);
            println!("  run `monk doctor` manually to inspect the daemon state.");
            return;
        }
        Err(e) => {
            tracing::warn!(?e, "doctor summary: thread error");
            return;
        }
    };
    let (ok, warn, fail) = report.summary();
    println!();
    println!("  health: {ok} ok · {warn} warn · {fail} fail");
    for c in &report.checks {
        if matches!(c.status, crate::doctor::Status::Fail | crate::doctor::Status::Warn) {
            println!("    {} {} — {}", c.status.icon(), c.title, c.detail);
            if let Some(h) = &c.hint {
                println!("      hint: {h}");
            }
        }
    }
}

pub async fn run_non_interactive(opts: Options) -> Result<()> {
    let mut cfg = Config::load().unwrap_or_default();

    if let Some(l) = &opts.locale {
        let norm = i18n::normalize(l).to_string();
        i18n::set(&norm);
        cfg.general.locale = Some(norm);
    }

    let presets: Vec<String> =
        if opts.presets.is_empty() { vec!["deepwork".into()] } else { opts.presets.clone() };

    let duration = opts.duration.unwrap_or(cfg.general.default_duration);
    let hard_mode = opts.hard_mode.unwrap_or(cfg.general.hard_mode);
    let autostart = opts.autostart.unwrap_or(cfg.general.autostart);

    validate_default_duration(duration)?;
    let (created, kept) = apply(&mut cfg, &presets, duration, hard_mode, autostart)?;
    print_apply_notices(&cfg, &created, &kept);

    // Persist before elevation, exactly like the interactive path: the
    // elevated child re-reads config from disk.
    cfg.general.initialized = true;
    persist_config(&cfg).await?;

    if !opts.no_daemon {
        if autostart {
            run_service_install();
        } else {
            println!("  {}", t!("onboarding.autostart_skipped"));
        }
    }

    // Install shell completions if not skipped
    if !opts.no_completions {
        install_completions_step();
    }

    // Run health checks if not skipped
    if !opts.no_doctor {
        print_basic_doctor_summary().await;
    }

    println!("monk initialized at {}", paths::config_file()?.display());
    Ok(())
}

/// Materialize the chosen presets into the config. Profiles that already
/// exist are NEVER overwritten — a re-run of the wizard must not wipe the
/// user's customized sites/apps/schedule. Returns (created, kept) names.
fn apply(
    cfg: &mut Config,
    presets: &[String],
    duration: Duration,
    hard_mode: bool,
    autostart: bool,
) -> Result<(Vec<String>, Vec<String>)> {
    let mut created = Vec::new();
    let mut kept = Vec::new();
    for name in presets {
        if cfg.profiles.contains_key(name) {
            kept.push(name.clone());
            continue;
        }
        if name == "custom" {
            cfg.profiles.entry("custom".into()).or_default();
        } else {
            let profile = load_preset(name)?;
            cfg.profiles.insert(name.clone(), profile);
        }
        created.push(name.clone());
    }
    if !presets.is_empty() && presets[0] != "custom" {
        cfg.general.default_profile = presets[0].clone();
    }
    cfg.general.default_duration = duration;
    cfg.general.hard_mode = hard_mode;
    cfg.general.autostart = autostart;
    Ok((created, kept))
}

/// Print post-apply notices: which existing profiles were left untouched,
/// and which freshly created ones carry an auto-start schedule.
fn print_apply_notices(cfg: &Config, created: &[String], kept: &[String]) {
    for name in kept {
        println!("  {}", t!("onboarding.preset_kept", profile = name.as_str()));
    }
    for name in created {
        if let Some(sch) = cfg.profiles.get(name).and_then(|p| p.schedule.as_ref()) {
            if sch.enabled {
                let window = sch.human();
                println!(
                    "  {}",
                    t!("onboarding.preset_scheduled", profile = name.as_str(), window = window)
                );
            }
        }
    }
}

fn reset() -> Result<()> {
    // Elevate: on macOS the service is root-owned, and a failed uninstall
    // leaves both the LaunchDaemon and a root-owned config behind — which is
    // exactly what the user asked us to remove.
    match platform::elevate_uninstall_service(false) {
        Ok(msg) => {
            for line in platform::strip_service_markers(&msg).lines() {
                println!("  {line}");
            }
        }
        Err(e) => eprintln!("  service uninstall failed (continuing): {e}"),
    }
    let path = paths::config_file()?;
    if path.exists() {
        fs_err::remove_file(&path)?;
    }
    println!("monk configuration removed");
    Ok(())
}

fn pick_locale(cli: Option<&str>) -> Result<String> {
    if let Some(l) = cli {
        return Ok(i18n::normalize(l).to_string());
    }
    let options = vec!["English", "Русский"];
    let default_idx = usize::from(i18n::current() == "ru");
    let ans = Select::new(&t!("onboarding.pick_language"), options)
        .with_starting_cursor(default_idx)
        .prompt()
        .map_err(prompt_err)?;
    Ok(if ans == "Русский" { "ru".into() } else { "en".into() })
}

fn pick_presets() -> Result<Vec<String>> {
    let mut labels: Vec<(String, String)> = Vec::new();
    let mut last_tier: Option<PresetTier> = None;
    for meta in PRESETS {
        if Some(meta.tier) != last_tier {
            let header = i18n::lookup(meta.tier.label_key()).into_owned();
            labels.push((format!("── {} {} ──", meta.tier.glyph(), header), String::new()));
            last_tier = Some(meta.tier);
        }
        let label = preset_label(meta.id);
        let blurb = preset_blurb(meta.id);
        let display = if blurb.is_empty() {
            format!("  {} ({})", label, meta.id)
        } else {
            format!("  {} — {}", label, blurb)
        };
        labels.push((display, meta.id.to_string()));
    }
    labels.push((format!("  {}", t!("onboarding.preset_custom")), "custom".to_string()));

    let display: Vec<String> = labels.iter().map(|(l, _)| l.clone()).collect();
    let default_idx = labels.iter().position(|(_, id)| id == "deepwork").unwrap_or(0);
    let chosen = MultiSelect::new(&t!("onboarding.pick_preset"), display.clone())
        .with_default(&[default_idx])
        .prompt()
        .map_err(prompt_err)?;
    let mut out = Vec::new();
    for label in chosen {
        if let Some((_, id)) = labels.iter().find(|(l, _)| *l == label) {
            if !id.is_empty() {
                out.push(id.clone());
            }
        }
    }
    if out.is_empty() {
        println!("  {}", t!("onboarding.presets_fallback"));
        out.push("deepwork".into());
    }
    Ok(out)
}

fn pick_duration(current: Duration) -> Result<Duration> {
    let options = vec![
        t!("onboarding.duration_pomodoro").to_string(),
        t!("onboarding.duration_deep").to_string(),
        t!("onboarding.duration_long").to_string(),
        t!("onboarding.duration_custom").to_string(),
    ];
    let ans = Select::new(&t!("onboarding.pick_duration"), options)
        .with_starting_cursor(0)
        .prompt()
        .map_err(prompt_err)?;
    if ans == t!("onboarding.duration_pomodoro") {
        Ok(Duration::from_secs(25 * 60))
    } else if ans == t!("onboarding.duration_deep") {
        Ok(Duration::from_secs(50 * 60))
    } else if ans == t!("onboarding.duration_long") {
        Ok(Duration::from_secs(90 * 60))
    } else {
        // Re-prompt on typos instead of aborting the whole wizard; Esc/Ctrl+C
        // still cancels via prompt_err.
        loop {
            let raw = Text::new(&t!("onboarding.duration_custom_prompt"))
                .with_default(&humantime::format_duration(current).to_string())
                .prompt()
                .map_err(prompt_err)?;
            match humantime::parse_duration(raw.trim()) {
                Ok(d) => match validate_default_duration(d) {
                    Ok(()) => return Ok(d),
                    Err(e) => println!("  {e}"),
                },
                Err(e) => println!("  {e}"),
            }
        }
    }
}

/// Sane bounds for the default session length: 1 minute to 24 hours.
fn validate_default_duration(d: Duration) -> Result<()> {
    if d < Duration::from_secs(60) || d > Duration::from_secs(24 * 3600) {
        return Err(Error::Config(t!("onboarding.duration_out_of_range").to_string()));
    }
    Ok(())
}

fn pick_hard_mode(default: bool) -> Result<bool> {
    let yes = t!("common.yes").to_string();
    let no = t!("common.no").to_string();
    let q = format!("{}  ({})", t!("onboarding.hard_mode_q"), t!("onboarding.hard_mode_hint"));
    let opts = vec![no.clone(), yes.clone()];
    let cursor = usize::from(default);
    let ans = Select::new(&q, opts).with_starting_cursor(cursor).prompt().map_err(prompt_err)?;
    Ok(ans == yes)
}

fn pick_autostart(default: bool) -> Result<bool> {
    let yes = t!("common.yes").to_string();
    let no = t!("common.no").to_string();
    let opts = vec![no.clone(), yes.clone()];
    let cursor = usize::from(default);
    let ans = Select::new(&t!("onboarding.autostart_q"), opts)
        .with_starting_cursor(cursor)
        .prompt()
        .map_err(prompt_err)?;
    Ok(ans == yes)
}

fn pick_apps_for_presets(cache: &crate::apps::AppCache) -> Result<Vec<String>> {
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
    if cache.apps.is_empty() {
        return Ok(Vec::new());
    }
    let (curated_apps, rest) = curated::partition(&cache.apps);
    let mut rows: Vec<Row> = Vec::with_capacity(cache.apps.len());
    let mut default_idx: Vec<usize> = Vec::new();
    for a in curated_apps {
        let cat = curated::match_curated(a).unwrap_or("");
        default_idx.push(rows.len());
        rows.push(Row {
            id: a.id.clone(), display: format!("★ {} · {} [{}]", a.label, cat, a.id)
        });
    }
    for a in rest {
        rows.push(Row { id: a.id.clone(), display: format!("  {} [{}]", a.label, a.id) });
    }
    let prompt = if default_idx.is_empty() {
        "Select apps to block during focus sessions"
    } else {
        "Select apps to block (★ = common distractors, pre-selected)"
    };
    let chosen = MultiSelect::new(prompt, rows)
        .with_default(&default_idx)
        .with_page_size(15)
        .prompt()
        .map_err(prompt_err)?;
    let mut out: Vec<String> = chosen.into_iter().map(|r| r.id).collect();
    out.sort();
    out.dedup();
    Ok(out)
}

fn check_hosts() {
    println!("{}", t!("onboarding.checking_hosts"));
    let hosts = blocker::hosts_path();
    match fs_err::OpenOptions::new().append(true).open(&hosts) {
        Ok(_) => println!("  {}", t!("onboarding.hosts_ok")),
        Err(_) => println!("  {}", t!("onboarding.hosts_ro")),
    }
}

fn banner() {
    println!();
    println!("  {}", t!("onboarding.welcome_title"));
    println!();
    for line in t!("onboarding.welcome_body").split('\n') {
        println!("  {line}");
    }
    println!();
}

fn farewell() -> Result<()> {
    println!();
    println!("  {}", t!("onboarding.done_title"));
    println!();
    println!("  {} {}", t!("onboarding.done_config_at"), paths::config_file()?.display());
    println!();
    println!("  {}", t!("onboarding.done_next_header"));
    println!("{}", t!("onboarding.done_next_start"));
    println!("{}", t!("onboarding.done_next_tui"));
    println!("{}", t!("onboarding.done_next_doctor"));
    println!("{}", t!("onboarding.done_next_help"));
    println!();
    let _ = PRESET_NAMES;
    Ok(())
}

fn install_completions_step() {
    if let Some(shell) = detect_shell() {
        println!("  {}", t!("setup.completions_installing", shell = shell));
        match crate::doctor::ActionKind::InstallCompletions.run() {
            Ok(msg) => {
                for line in msg.lines() {
                    if line.starts_with("wrote completions →") {
                        let path = line.strip_prefix("wrote completions → ").unwrap_or(line);
                        println!("  {}", t!("setup.completions_installed", path = path));
                    } else {
                        println!("    {line}");
                    }
                }
            }
            Err(e) => {
                eprintln!("  {}", t!("setup.completions_failed", error = e));
                eprintln!("  → run `monk completions {shell}` manually later");
            }
        }
    }
}

async fn print_doctor_summary_async() {
    println!("  {}", t!("setup.doctor_checking"));
    let result = std::thread::Builder::new()
        .name("monk-doctor-summary".into())
        .spawn(|| {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::warn!(?e, "doctor summary: tokio runtime build failed");
                    return None;
                }
            };
            rt.block_on(async {
                tokio::time::timeout(DOCTOR_SUMMARY_TIMEOUT, crate::doctor::run()).await.ok()
            })
        })
        .and_then(|h| h.join().map_err(|_| std::io::Error::other("doctor thread panicked")));
    let report = match result {
        Ok(Some(r)) => r,
        Ok(None) => {
            println!("  health: skipped — check timed out after {:?}", DOCTOR_SUMMARY_TIMEOUT);
            return;
        }
        Err(e) => {
            tracing::warn!(?e, "doctor summary: thread error");
            return;
        }
    };
    let (ok, warn, fail) = report.summary();
    println!("  {}", t!("setup.doctor_summary", ok = ok, warn = warn, fail = fail));

    // Show only warnings and failures
    for c in &report.checks {
        if matches!(c.status, crate::doctor::Status::Warn | crate::doctor::Status::Fail) {
            println!(
                "  {}",
                t!(
                    "setup.doctor_issue",
                    icon = c.status.icon(),
                    title = &c.title,
                    detail = &c.detail
                )
            );
            if let Some(hint) = &c.hint {
                println!("    {}", t!("setup.doctor_hint", hint = hint));
            }
        }
    }
}

async fn print_basic_doctor_summary() {
    println!("  {}", t!("setup.doctor_checking"));
    let report = crate::doctor::run().await;
    let (ok, warn, fail) = report.summary();
    println!("  {}", t!("setup.doctor_summary", ok = ok, warn = warn, fail = fail));
}

fn detect_shell() -> Option<String> {
    std::env::var("SHELL").ok().and_then(|s| {
        std::path::Path::new(&s).file_name().map(|n| n.to_string_lossy().to_lowercase())
    })
}

fn prompt_err(e: inquire::InquireError) -> Error {
    match e {
        inquire::InquireError::OperationCanceled | inquire::InquireError::OperationInterrupted => {
            Error::Other("cancelled".into())
        }
        other => Error::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_never_clobbers_existing_profiles() {
        let mut cfg = Config::default();
        let customized =
            crate::config::Profile { sites: vec!["mysite.com".into()], ..Default::default() };
        cfg.profiles.insert("deepwork".into(), customized);

        let presets = ["deepwork".to_string(), "study".to_string()];
        let (created, kept) =
            apply(&mut cfg, &presets, Duration::from_secs(25 * 60), false, false).unwrap();

        assert_eq!(kept, ["deepwork"]);
        assert_eq!(created, ["study"]);
        assert_eq!(
            cfg.profiles["deepwork"].sites,
            ["mysite.com"],
            "re-running the wizard must keep user customizations"
        );
        assert!(cfg.profiles.contains_key("study"));
    }

    #[test]
    fn default_duration_bounds() {
        assert!(validate_default_duration(Duration::from_secs(59)).is_err());
        assert!(validate_default_duration(Duration::from_secs(60)).is_ok());
        assert!(validate_default_duration(Duration::from_secs(24 * 3600)).is_ok());
        assert!(validate_default_duration(Duration::from_secs(24 * 3600 + 1)).is_err());
    }
}
