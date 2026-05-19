use std::{io::IsTerminal, time::Duration};

use indicatif::{ProgressBar, ProgressStyle};
use inquire::{MultiSelect, Select, Text};

use crate::{
    blocker,
    config::Config,
    daemon::{self, ServiceAction},
    i18n::{self, t},
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
}

pub fn run(opts: Options) -> Result<()> {
    if opts.reset {
        return reset();
    }

    if !std::io::stdin().is_terminal() || opts.yes {
        return run_non_interactive(opts);
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

    apply(&mut cfg, &presets, duration, hard_mode, autostart)?;
    for preset in &presets {
        if let Some(profile) = cfg.profiles.get_mut(preset) {
            profile.apps = chosen_apps.clone();
        }
    }

    check_hosts();

    // Persist before elevation: the elevated child re-reads config from disk
    // (or relies on its own defaults). If we save *after*, the root child can
    // race the parent and observe stale state.
    cfg.general.initialized = true;
    cfg.save()?;

    if autostart {
        run_service_install();
    }

    print_doctor_summary();

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
            for line in msg.lines() {
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
            Some(rt.block_on(crate::doctor::run()))
        })
        .and_then(|h| h.join().map_err(|_| std::io::Error::other("doctor thread panicked")));
    let Ok(Some(report)) = result else {
        if let Err(e) = result {
            tracing::warn!(?e, "doctor summary: thread error");
        }
        return;
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

pub fn run_non_interactive(opts: Options) -> Result<()> {
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

    apply(&mut cfg, &presets, duration, hard_mode, autostart)?;

    if autostart {
        let _ = daemon::service_run(ServiceAction::Install);
    }

    cfg.general.initialized = true;
    cfg.save()?;
    println!("monk initialized at {}", paths::config_file()?.display());
    Ok(())
}

fn apply(
    cfg: &mut Config,
    presets: &[String],
    duration: Duration,
    hard_mode: bool,
    autostart: bool,
) -> Result<()> {
    for name in presets {
        if name == "custom" {
            cfg.profiles.entry("custom".into()).or_default();
            continue;
        }
        let profile = load_preset(name)?;
        cfg.profiles.insert(name.clone(), profile);
    }
    if !presets.is_empty() && presets[0] != "custom" {
        cfg.general.default_profile = presets[0].clone();
    }
    cfg.general.default_duration = duration;
    cfg.general.hard_mode = hard_mode;
    cfg.general.autostart = autostart;
    Ok(())
}

fn reset() -> Result<()> {
    let _ = daemon::service_run(ServiceAction::Uninstall { purge: false });
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
        let raw = Text::new(&t!("onboarding.duration_custom_prompt"))
            .with_default(&humantime::format_duration(current).to_string())
            .prompt()
            .map_err(prompt_err)?;
        humantime::parse_duration(&raw).map_err(|e| Error::Other(e.to_string()))
    }
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

fn prompt_err(e: inquire::InquireError) -> Error {
    match e {
        inquire::InquireError::OperationCanceled | inquire::InquireError::OperationInterrupted => {
            Error::Other("cancelled".into())
        }
        other => Error::Other(other.to_string()),
    }
}
