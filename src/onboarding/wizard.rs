use std::{io::IsTerminal, time::Duration};

use inquire::{MultiSelect, Select, Text};

use crate::{
    blocker,
    config::Config,
    daemon::{self, ServiceAction},
    i18n::{self, t},
    onboarding::presets::{load_preset, PRESET_NAMES},
    paths, Error, Result,
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
    let hard_mode = pick_hard_mode(cfg.general.hard_mode)?;
    let autostart = pick_autostart(cfg.general.autostart)?;

    apply(&mut cfg, &presets, duration, hard_mode, autostart)?;

    println!("scanning installed applications…");
    let cache = crate::apps::load_or_scan(true)?;
    println!("found {} applications", cache.apps.len());
    let chosen_apps = pick_apps_for_presets(&cache)?;
    for preset in &presets {
        if let Some(profile) = cfg.profiles.get_mut(preset) {
            profile.apps = chosen_apps.clone();
        }
    }

    check_hosts();

    if autostart {
        match daemon::service_run(ServiceAction::Install) {
            Ok(msg) => println!("{msg}"),
            Err(e) => eprintln!("autostart setup failed: {e}"),
        }
    }

    cfg.general.initialized = true;
    cfg.save()?;
    farewell()?;
    Ok(())
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
    let _ = daemon::service_run(ServiceAction::Uninstall);
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
    let labels = [
        (t!("onboarding.preset_deepwork").to_string(), "deepwork"),
        (t!("onboarding.preset_no_chat").to_string(), "no-chat"),
        (t!("onboarding.preset_no_news").to_string(), "no-news"),
        (t!("onboarding.preset_no_games").to_string(), "no-games"),
        (t!("onboarding.preset_custom").to_string(), "custom"),
    ];
    let display: Vec<String> = labels.iter().map(|(l, _)| l.clone()).collect();
    let chosen = MultiSelect::new(&t!("onboarding.pick_preset"), display.clone())
        .with_default(&[0])
        .prompt()
        .map_err(prompt_err)?;
    let mut out = Vec::new();
    for label in chosen {
        if let Some((_, id)) = labels.iter().find(|(l, _)| *l == label) {
            out.push((*id).to_string());
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
    let rows: Vec<Row> = cache
        .apps
        .iter()
        .map(|a| Row { id: a.id.clone(), display: format!("{} [{}]", a.label, a.id) })
        .collect();
    let chosen =
        MultiSelect::new("Select apps to block during focus sessions", rows.clone())
            .with_page_size(15)
            .prompt()
            .map_err(prompt_err)?;
    Ok(chosen.into_iter().map(|r| r.id).collect())
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
