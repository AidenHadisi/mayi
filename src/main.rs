//! `mayi --agent <host>` runs the hook. `mayi --agent <host> install` writes it
//! into that host's config. `mayi stats` summarizes local decisions.
//! `mayi config` reads and writes `~/.config/mayi/config.toml`.

use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};

use mayi::config::{Config, Field};
use mayi::host::Agent;
use mayi::{hook, log};

#[derive(Parser)]
#[command(name = "mayi", version, about)]
struct Cli {
    #[arg(short, long, global = true, value_enum)]
    agent: Option<Agent>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Install the hook into the agent's config.
    Install,
    /// Summarize recorded hook decisions.
    Stats {
        /// Only events newer than this (`7d`, `12h`, or `30m`).
        #[arg(long)]
        since: Option<String>,
    },
    /// Read or write ~/.config/mayi/config.toml.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Print the config file path.
    Path,
    /// Print the effective config as JSON.
    Show {
        /// Include `api_key`.
        #[arg(long)]
        show_secrets: bool,
    },
    /// Print one effective value.
    Get {
        #[arg(value_enum)]
        key: Field,
    },
    /// Set one key.
    Set {
        #[arg(value_enum)]
        key: Field,
        value: String,
    },
    /// Remove one key so the compiled default returns.
    Unset {
        #[arg(value_enum)]
        key: Field,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        None => return hook::run(require_agent(cli.agent)),
        Some(Command::Install) => require_agent(cli.agent).install_hook(),
        Some(Command::Stats { since }) => {
            log::Summary::load(cli.agent, since.as_deref()).map(|s| s.print())
        }
        Some(Command::Config { command }) => match command {
            ConfigCommand::Path => Config::print_path(),
            ConfigCommand::Show { show_secrets } => Config::print(show_secrets),
            ConfigCommand::Get { key } => Config::print_get(key),
            ConfigCommand::Set { key, value } => Config::set(key, &value),
            ConfigCommand::Unset { key } => Config::unset(key),
        },
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `--agent` is global so `stats` can use it as a filter, but the hook and
/// `install` cannot run without one.
fn require_agent(agent: Option<Agent>) -> Agent {
    agent.unwrap_or_else(|| {
        Cli::command()
            .error(
                clap::error::ErrorKind::MissingRequiredArgument,
                "the following required arguments were not provided:\n  --agent <AGENT>",
            )
            .exit()
    })
}
