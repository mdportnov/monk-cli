mod commands;

pub fn cmd_factory() -> clap::Command {
    use clap::CommandFactory;
    Cli::command()
}

use std::time::Duration;

use clap::{Parser, Subcommand};
use miette::Result;

#[derive(Debug, Parser)]
#[command(
    name = "monk",
    version,
    about = "A cross-platform focus & distraction blocker",
    propagate_version = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[arg(long, global = true, value_parser = ["en", "ru"], env = "MONK_LOCALE")]
    pub locale: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(
        about = "Run the interactive onboarding wizard",
        alias = "setup",
        long_about = "Interactive onboarding. Use `monk setup` as a friendly alias. `--quick` skips optional prompts and pre-selects common distractor apps."
    )]
    Init {
        #[arg(long)]
        non_interactive: bool,
        #[arg(long, value_parser = ["en", "ru"])]
        locale: Option<String>,
        #[arg(long, value_delimiter = ',')]
        preset: Vec<String>,
        #[arg(long, value_parser = parse_duration)]
        duration: Option<Duration>,
        #[arg(long)]
        hard: Option<bool>,
        #[arg(long)]
        autostart: Option<bool>,
        #[arg(long, short)]
        yes: bool,
        #[arg(long)]
        reset: bool,
        #[arg(long, help = "Three-question quick path with sensible defaults")]
        quick: bool,
        #[arg(long, help = "Skip daemon service installation")]
        no_daemon: bool,
        #[arg(long, help = "Skip shell completions installation")]
        no_completions: bool,
        #[arg(long, help = "Skip health checks")]
        no_doctor: bool,
        #[arg(long, help = "Skip menu bar app setup (macOS)")]
        no_menubar: bool,
    },
    #[command(about = "Open the interactive TUI")]
    Tui,
    #[command(about = "Set the interface language")]
    Lang {
        #[arg(value_parser = ["en", "ru"])]
        locale: String,
    },
    #[command(about = "Start a focus session")]
    Start {
        #[arg(value_name = "PROFILE")]
        profile: Option<String>,
        #[arg(short, long, value_parser = parse_duration)]
        duration: Option<Duration>,
        #[arg(long)]
        hard: bool,
        #[arg(long)]
        reason: Option<String>,
    },
    #[command(about = "Stop the active session")]
    Stop,
    #[command(about = "Request an escape from hard mode")]
    Panic {
        #[arg(long)]
        phrase: Option<String>,
        #[arg(long)]
        cancel: bool,
    },
    #[command(about = "Show daemon and session status")]
    Status,
    #[command(about = "List profiles")]
    Profiles,
    #[command(subcommand, about = "Manage profiles")]
    Profile(ProfileCmd),
    #[command(subcommand, about = "Manage installed applications cache")]
    Apps(AppsCmd),
    #[command(about = "Show session statistics")]
    Stats,
    #[command(about = "Check environment, permissions, and daemon health")]
    Doctor {
        #[arg(long, help = "Output a machine-readable JSON report (exits 0 even on failures)")]
        json: bool,
        #[arg(
            long,
            help = "Try to auto-fix common issues (reinstall service, install completions, etc.)"
        )]
        fix: bool,
    },
    #[command(subcommand, about = "Manage configuration")]
    Config(ConfigCmd),
    #[command(subcommand, about = "Manage the background daemon", alias = "service")]
    Daemon(DaemonCmd),
    #[command(about = "Run the menu bar companion app (macOS only)")]
    Menubar {
        #[command(subcommand)]
        cmd: Option<MenubarCmd>,
    },
    #[command(about = "Generate shell completions")]
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    #[command(about = "Check GitHub releases for a new version and self-update")]
    Update {
        #[arg(long, help = "Only check and report; do not download or install")]
        check: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCmd {
    Path,
    Export,
    Import {
        #[arg(value_name = "FILE")]
        file: std::path::PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum ProfileCmd {
    #[command(about = "Show a profile's full configuration")]
    Show {
        #[arg(value_name = "PROFILE")]
        name: String,
        #[arg(long, help = "Output machine-readable JSON")]
        json: bool,
    },
    #[command(about = "Edit a profile (--add/--remove app id, or no flags for TTY editor)")]
    Edit {
        #[arg(value_name = "PROFILE")]
        name: String,
        #[arg(long, value_name = "APP_ID")]
        add: Vec<String>,
        #[arg(long, value_name = "APP_ID")]
        remove: Vec<String>,
    },
    #[command(about = "Create an empty profile or copy from a preset (`--preset deepwork`)")]
    Create {
        #[arg(value_name = "PROFILE")]
        name: String,
        #[arg(
            long,
            value_name = "PRESET",
            help = "Seed from a built-in preset (deepwork, study, detox, sleep, sober, lockdown, no-social, no-video, no-news, no-games, no-chat, no-shopping, no-adult, no-gambling, no-dating, no-ai)"
        )]
        preset: Option<String>,
    },
    #[command(about = "Duplicate an existing profile under a new name")]
    Duplicate {
        #[arg(value_name = "SOURCE")]
        source: String,
        #[arg(value_name = "TARGET")]
        target: Option<String>,
    },
    #[command(about = "Delete a profile")]
    Delete {
        #[arg(value_name = "PROFILE")]
        name: String,
    },
    #[command(about = "Set time limits for a profile (omit value to clear)")]
    Limits {
        #[arg(value_name = "PROFILE")]
        name: String,
        #[arg(long, value_parser = parse_duration_opt)]
        max: Option<String>,
        #[arg(long, value_parser = parse_duration_opt)]
        min: Option<String>,
        #[arg(long, value_parser = parse_duration_opt)]
        cooldown: Option<String>,
        #[arg(long = "daily-cap", value_parser = parse_duration_opt)]
        daily_cap: Option<String>,
        #[arg(long)]
        clear: bool,
    },
}

#[derive(Debug, Subcommand)]
enum AppsCmd {
    #[command(about = "List installed applications from cache")]
    List {
        #[arg(long)]
        refresh: bool,
    },
    #[command(about = "Force a rescan of installed applications")]
    Scan,
}

#[derive(Debug, Subcommand)]
enum MenubarCmd {
    #[command(about = "Install the menu bar app as a login item")]
    Install,
    #[command(about = "Remove the menu bar app login item")]
    Uninstall,
}

#[derive(Debug, Subcommand)]
enum DaemonCmd {
    #[command(about = "Start the background daemon (user mode)")]
    Start,
    #[command(about = "Stop the running daemon")]
    Stop,
    #[command(about = "Show daemon status (running / not running, pid)")]
    Status,
    #[command(
        about = "Run the daemon in the foreground (used by launchd / systemd; not normally invoked manually)"
    )]
    Run,
    #[command(
        about = "Install the daemon as a system service (asks for admin rights when needed)"
    )]
    Install {
        #[arg(
            long,
            help = "Force reinstall (uninstall then install). Useful after a macOS major upgrade."
        )]
        reinstall: bool,
    },
    #[command(
        about = "Remove the system service, asking for admin rights when needed (add --purge to also wipe data)"
    )]
    Uninstall {
        #[arg(long, help = "Also delete config, audit log, and cached data")]
        purge: bool,
    },
}

fn parse_duration(raw: &str) -> std::result::Result<Duration, String> {
    humantime::parse_duration(raw).map_err(|e| e.to_string())
}

fn parse_duration_opt(raw: &str) -> std::result::Result<String, String> {
    humantime::parse_duration(raw).map_err(|e| e.to_string())?;
    Ok(raw.to_string())
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    let is_daemon_run = matches!(cli.command, Command::Daemon(DaemonCmd::Run));

    let _daemon_guard = if is_daemon_run {
        crate::telemetry::init_daemon()
    } else {
        crate::telemetry::init();
        None
    };

    // Skip locale + onboarding probes in the daemon path: Config::load creates
    // the default config.toml if missing, which would defeat migration. The
    // daemon handles locale and migration itself in server::run.
    if !is_daemon_run {
        let cfg_locale = crate::config::Config::load().ok().and_then(|c| c.general.locale.clone());
        crate::i18n::init(cfg_locale.as_deref(), cli.locale.as_deref());
        maybe_first_run_onboarding(&cli.command, cli.locale.as_deref()).await?;
    } else {
        crate::i18n::init(None, cli.locale.as_deref());
    }

    let result: crate::Result<()> = match cli.command {
        Command::Init {
            non_interactive,
            locale,
            preset,
            duration,
            hard,
            autostart,
            yes,
            reset,
            quick,
            no_daemon,
            no_completions,
            no_doctor,
            no_menubar,
        } => {
            let opts = crate::onboarding::Options {
                locale,
                presets: preset,
                duration,
                hard_mode: hard,
                autostart,
                yes,
                reset,
                quick,
                no_daemon,
                no_completions,
                no_doctor,
                no_menubar,
            };
            if non_interactive || opts.yes {
                crate::onboarding::run_non_interactive(opts).await
            } else {
                crate::onboarding::run(opts).await
            }
        }
        Command::Lang { locale } => commands::set_lang(&locale).await,
        Command::Start { profile, duration, hard, reason } => {
            commands::start(profile, duration, hard, reason).await
        }
        Command::Stop => commands::stop().await,
        Command::Panic { phrase, cancel } => commands::panic_cmd(phrase, cancel).await,
        Command::Status => commands::status().await,
        Command::Profiles => commands::profiles().await,
        Command::Profile(ProfileCmd::Show { name, json }) => {
            commands::profile_show(&name, json).await
        }
        Command::Profile(ProfileCmd::Edit { name, add, remove }) => {
            commands::profile_edit(&name, add, remove).await
        }
        Command::Profile(ProfileCmd::Create { name, preset }) => {
            commands::profile_create(&name, preset.as_deref()).await
        }
        Command::Profile(ProfileCmd::Duplicate { source, target }) => {
            commands::profile_duplicate(&source, target.as_deref()).await
        }
        Command::Profile(ProfileCmd::Delete { name }) => commands::profile_delete(&name).await,
        Command::Profile(ProfileCmd::Limits { name, max, min, cooldown, daily_cap, clear }) => {
            commands::profile_limits(&name, max, min, cooldown, daily_cap, clear).await
        }
        Command::Apps(AppsCmd::List { refresh }) => commands::apps_list(refresh),
        Command::Apps(AppsCmd::Scan) => commands::apps_scan(),
        Command::Stats => commands::stats().await,
        Command::Doctor { json, fix } => commands::doctor(json, fix).await,
        Command::Config(ConfigCmd::Path) => commands::config_path(),
        Command::Config(ConfigCmd::Export) => commands::config_export(),
        Command::Config(ConfigCmd::Import { file }) => commands::config_import(&file).await,
        Command::Tui => commands::tui().await,
        Command::Daemon(DaemonCmd::Run) => commands::daemon_run().await,
        Command::Daemon(DaemonCmd::Start) => commands::daemon_start().await,
        Command::Daemon(DaemonCmd::Stop) => commands::daemon_stop().await,
        Command::Daemon(DaemonCmd::Status) => commands::daemon_status().await,
        Command::Daemon(DaemonCmd::Install { reinstall }) => commands::daemon_install(reinstall),
        Command::Daemon(DaemonCmd::Uninstall { purge }) => commands::daemon_uninstall(purge).await,
        Command::Menubar { cmd } => match cmd {
            None => commands::menubar_run(),
            Some(MenubarCmd::Install) => commands::menubar_install(),
            Some(MenubarCmd::Uninstall) => commands::menubar_uninstall(),
        },
        Command::Completions { shell } => {
            use clap::CommandFactory;
            clap_complete::generate(shell, &mut Cli::command(), "monk", &mut std::io::stdout());
            Ok(())
        }
        Command::Update { check } => commands::update(check).await,
    };
    result.map_err(miette::Report::from)
}

async fn maybe_first_run_onboarding(cmd: &Command, _locale: Option<&str>) -> crate::Result<()> {
    use std::io::IsTerminal;
    if matches!(
        cmd,
        Command::Init { .. }
            | Command::Completions { .. }
            | Command::Daemon(_)
            | Command::Menubar { .. }
            | Command::Lang { .. }
            | Command::Doctor { .. }
            | Command::Update { .. }
    ) {
        return Ok(());
    }
    let already = crate::config::Config::load().map(|c| c.general.initialized).unwrap_or(false);
    if already {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        eprintln!("{}", crate::i18n::t!("onboarding.first_run_nudge"));
        return Ok(());
    }
    // Interactive terminal: offer to run the wizard right now instead of
    // only hinting at it. Declining (or Esc) falls back to the nudge.
    let offer = crate::i18n::t!("onboarding.first_run_offer").to_string();
    match inquire::Confirm::new(&offer).with_default(true).prompt() {
        Ok(true) => crate::onboarding::run(crate::onboarding::Options::default()).await,
        _ => {
            eprintln!("{}", crate::i18n::t!("onboarding.first_run_nudge"));
            Ok(())
        }
    }
}
