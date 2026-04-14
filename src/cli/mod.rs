mod commands;

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
    #[command(about = "Run the interactive onboarding wizard")]
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
    },
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
    Doctor,
    #[command(subcommand, about = "Manage configuration")]
    Config(ConfigCmd),
    #[command(about = "Open the interactive TUI")]
    Tui,
    #[command(subcommand, about = "Manage the background daemon")]
    Daemon(DaemonCmd),
    #[command(about = "Generate shell completions")]
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
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
    #[command(about = "Interactively edit a profile's app and site group selection")]
    Edit {
        #[arg(value_name = "PROFILE")]
        name: String,
        #[arg(long, value_name = "APP_ID")]
        add: Vec<String>,
        #[arg(long, value_name = "APP_ID")]
        remove: Vec<String>,
    },
    #[command(about = "Create an empty profile")]
    Create {
        #[arg(value_name = "PROFILE")]
        name: String,
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
enum DaemonCmd {
    Start,
    Stop,
    Status,
    Run,
    Install,
    Uninstall {
        #[arg(long)]
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

    let _daemon_guard = match &cli.command {
        Command::Daemon(DaemonCmd::Run) => crate::telemetry::init_daemon(),
        _ => {
            crate::telemetry::init();
            None
        }
    };

    let cfg_locale = crate::config::Config::load().ok().and_then(|c| c.general.locale.clone());
    crate::i18n::init(cfg_locale.as_deref(), cli.locale.as_deref());

    maybe_first_run_onboarding(&cli.command, cli.locale.as_deref())?;

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
        } => {
            let opts = crate::onboarding::Options {
                locale,
                presets: preset,
                duration,
                hard_mode: hard,
                autostart,
                yes,
                reset,
            };
            if non_interactive || opts.yes {
                crate::onboarding::run_non_interactive(opts)
            } else {
                crate::onboarding::run(opts)
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
        Command::Profile(ProfileCmd::Edit { name, add, remove }) => {
            commands::profile_edit(&name, add, remove).await
        }
        Command::Profile(ProfileCmd::Create { name }) => commands::profile_create(&name).await,
        Command::Profile(ProfileCmd::Delete { name }) => commands::profile_delete(&name).await,
        Command::Profile(ProfileCmd::Limits { name, max, min, cooldown, daily_cap, clear }) => {
            commands::profile_limits(&name, max, min, cooldown, daily_cap, clear).await
        }
        Command::Apps(AppsCmd::List { refresh }) => commands::apps_list(refresh),
        Command::Apps(AppsCmd::Scan) => commands::apps_scan(),
        Command::Stats => commands::stats(),
        Command::Doctor => commands::doctor().await,
        Command::Config(ConfigCmd::Path) => commands::config_path(),
        Command::Config(ConfigCmd::Export) => commands::config_export(),
        Command::Config(ConfigCmd::Import { file }) => commands::config_import(&file).await,
        Command::Tui => commands::tui().await,
        Command::Daemon(DaemonCmd::Run) => commands::daemon_run().await,
        Command::Daemon(DaemonCmd::Start) => commands::daemon_start().await,
        Command::Daemon(DaemonCmd::Stop) => commands::daemon_stop().await,
        Command::Daemon(DaemonCmd::Status) => commands::daemon_status().await,
        Command::Daemon(DaemonCmd::Install) => commands::daemon_install(),
        Command::Daemon(DaemonCmd::Uninstall { purge }) => commands::daemon_uninstall(purge).await,
        Command::Completions { shell } => {
            use clap::CommandFactory;
            clap_complete::generate(shell, &mut Cli::command(), "monk", &mut std::io::stdout());
            Ok(())
        }
    };
    result.map_err(miette::Report::from)
}

fn maybe_first_run_onboarding(cmd: &Command, locale: Option<&str>) -> crate::Result<()> {
    use std::io::IsTerminal;
    if matches!(
        cmd,
        Command::Init { .. }
            | Command::Completions { .. }
            | Command::Daemon(DaemonCmd::Run)
            | Command::Lang { .. }
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
    let opts =
        crate::onboarding::Options { locale: locale.map(|s| s.to_string()), ..Default::default() };
    crate::onboarding::run(opts)
}
