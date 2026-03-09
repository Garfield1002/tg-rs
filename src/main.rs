use std::{
    fs,
    io::{IsTerminal, Read},
    time::{Duration, Instant},
};

use clap::{Args, Parser, Subcommand};
use teloxide::{
    Bot,
    payloads::GetUpdatesSetters,
    prelude::Requester,
    types::{ChatId, UpdateKind},
};
use tg_cli::{ParseMode, TgSession, send_tg_message};
use std::path::PathBuf;

use crate::config::{Config, config_path};

use crate::setup::run_setup;

mod config;
mod secret_store;
mod setup;

#[derive(Parser)]
#[command(
    name = "tg",
    version,
    about = "Send Telegram messages from the command line"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Interpret escape sequences (\n, \t, \\)
    #[arg(short = 'e')]
    escape: bool,

    /// No trailing newline (strips trailing newline from input)
    #[arg(short = 'n')]
    no_newline: bool,

    /// Telegram parse mode: markdown or html
    #[arg(short = 'm', long, default_value = "markdown")]
    parse_mode: String,

    /// Suppress non-error output
    #[arg(short = 'q', long)]
    quiet: bool,

    /// Send silently (no device notification)
    #[arg(short = 's', long)]
    silent: bool,

    /// Interactive mode: stream stdin updates and edit a single Telegram message (max 1 update/s)
    #[arg(short = 'i', long)]
    interactive: bool,

    /// Seconds between interactive message edits
    #[arg(
        short = 'f',
        long,
        default_value_t = 1,
        value_name = "SECONDS",
        requires = "interactive",
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    interactive_frequency: u64,

    /// Message text (reads stdin if empty)
    words: Vec<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Run interactive setup to configure bot token and chat ID
    Setup,
    /// Listen for incoming Telegram messages and print them to stdout (stop with /eof)
    Listen,
    /// Send files as Telegram document attachments
    Attach(AttachArgs),
    /// Manage configuration
    Config(ConfigArgs),
}

#[derive(Args)]
struct AttachArgs {
    /// Files to attach
    #[arg(required = true)]
    files: Vec<PathBuf>,

    /// Send silently (no device notification)
    #[arg(short = 's', long)]
    silent: bool,

    /// Suppress non-error output
    #[arg(short = 'q', long)]
    quiet: bool,
}

#[derive(Args)]
struct ConfigArgs {
    #[command(subcommand)]
    action: ConfigAction,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Print current config path and contents
    Show,
    /// Delete config file (allows re-running setup)
    Reset,
}

fn interpret_escapes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('\\') => result.push('\\'),
                Some('0') => result.push('\0'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let cli = Cli::parse();

    match cli.command {
        Some(Command::Setup) => run_setup().await,
        Some(Command::Listen) => {
            if let Err(err) = run_listen().await {
                eprintln!("{}", err);
                std::process::exit(1);
            }
        }
        Some(Command::Attach(args)) => {
            if let Err(err) = run_attach(args).await {
                eprintln!("{}", err);
                std::process::exit(1);
            }
        }
        Some(Command::Config(args)) => run_config(args.action),
        None => run_messgae(cli).await,
    }
}

async fn run_messgae(cli: Cli) {
    if cli.interactive {
        if let Err(err) = run_interactive(cli).await {
            eprintln!("{}", err);
            std::process::exit(1);
        }
        return;
    }

    let mut text = if cli.words.is_empty() {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .expect("failed to read stdin");
        buf
    } else {
        cli.words.join(" ")
    };

    if cli.escape {
        text = interpret_escapes(&text);
    }

    if cli.no_newline {
        text = text.trim_end_matches(['\n', '\r']).to_string();
    }

    let parse_mode = match cli.parse_mode.as_str() {
        "markdown" | "md" => ParseMode::Markdown,
        "html" => ParseMode::Html,
        other => {
            eprintln!("Unknown parse mode '{}'. Use 'markdown' or 'html'.", other);
            std::process::exit(1);
        }
    };

    if let Err(err) = send_tg_message(text, parse_mode, cli.silent).await {
        eprintln!("{}", err);
        std::process::exit(1);
    }
}

async fn run_listen() -> Result<(), tg_cli::SendMessageError> {
    let config = Config::load();
    let token = config
        .resolved_token()
        .ok_or(tg_cli::SendMessageError::MissingToken)?;
    let chat_id = config
        .chat_id
        .ok_or(tg_cli::SendMessageError::MissingChatId)?;

    let bot = Bot::new(token);
    let target_chat = ChatId(chat_id);

    // Start from the next update so listen mode only captures new incoming messages.
    let mut offset: i32 = match bot.get_updates().timeout(0).await {
        Ok(existing) => existing
            .iter()
            .map(|u| i32::try_from(u.id.0).unwrap_or(i32::MAX).saturating_add(1))
            .max()
            .unwrap_or(0),
        Err(err) => return Err(tg_cli::SendMessageError::Request(err)),
    };

    loop {
        let updates = bot
            .get_updates()
            .offset(offset)
            .timeout(30)
            .await
            .map_err(tg_cli::SendMessageError::Request)?;

        for update in updates {
            offset = i32::try_from(update.id.0)
                .unwrap_or(i32::MAX)
                .saturating_add(1);

            let UpdateKind::Message(message) = update.kind else {
                continue;
            };

            if message.chat.id != target_chat {
                continue;
            }

            let Some(text) = message.text() else {
                continue;
            };

            if text.trim() == "/eof" {
                return Ok(());
            }

            println!("{}", text);
        }
    }
}

fn parse_mode_from_cli(raw: &str) -> ParseMode {
    match raw {
        "markdown" | "md" => ParseMode::Markdown,
        "html" => ParseMode::Html,
        other => {
            eprintln!("Unknown parse mode '{}'. Use 'markdown' or 'html'.", other);
            std::process::exit(1);
        }
    }
}

async fn push_update(
    session: &TgSession,
    message_id: &mut Option<i32>,
    text: String,
    parse_mode: ParseMode,
    silent: bool,
) -> Result<(), tg_cli::SendMessageError> {
    if text.is_empty() {
        return Ok(());
    }

    if let Some(id) = *message_id {
        session.edit_message(id, text, parse_mode).await
    } else {
        let id = session.send_message(text, parse_mode, silent).await?;
        *message_id = Some(id);
        Ok(())
    }
}

async fn run_interactive(cli: Cli) -> Result<(), tg_cli::SendMessageError> {
    let parse_mode = parse_mode_from_cli(&cli.parse_mode);
    let session = TgSession::from_config()?;
    let mut message_id: Option<i32> = None;
    let update_interval = Duration::from_secs(cli.interactive_frequency);

    if !cli.words.is_empty() {
        let initial = cli.words.join(" ");
        push_update(&session, &mut message_id, initial, parse_mode, cli.silent).await?;
    }

    if std::io::stdin().is_terminal() {
        return Ok(());
    }

    let mut stdin = std::io::stdin().lock();
    let mut byte = [0u8; 1];
    let mut frame = Vec::<u8>::new();
    let mut pending: Option<String> = None;
    let now = Instant::now();
    let mut last_sent = now.checked_sub(update_interval).unwrap_or(now);

    loop {
        let n = stdin.read(&mut byte).expect("failed to read stdin");
        if n == 0 {
            break;
        }

        match byte[0] {
            b'\n' | b'\r' => {
                if !frame.is_empty() {
                    let text = String::from_utf8_lossy(&frame).to_string();
                    pending = Some(text);
                    frame.clear();
                }
            }
            b => frame.push(b),
        }

        if pending.is_some() && last_sent.elapsed() >= update_interval {
            let text = pending.take().unwrap_or_default();
            push_update(&session, &mut message_id, text, parse_mode, cli.silent).await?;
            last_sent = Instant::now();
        }
    }

    if !frame.is_empty() {
        pending = Some(String::from_utf8_lossy(&frame).to_string());
    }

    if let Some(text) = pending {
        push_update(&session, &mut message_id, text, parse_mode, cli.silent).await?;
    }

    Ok(())
}

async fn run_attach(args: AttachArgs) -> Result<(), tg_cli::SendMessageError> {
    let session = TgSession::from_config()?;

    for path in &args.files {
        if !path.exists() {
            eprintln!("File not found: {}", path.display());
            std::process::exit(1);
        }

        session.send_document(path, args.silent).await?;

        if !args.quiet {
            eprintln!("Sent: {}", path.display());
        }
    }

    Ok(())
}

fn run_config(action: ConfigAction) {
    match action {
        ConfigAction::Show => {
            let path = config_path();
            println!("Config path: {}", path.display());
            if path.exists() {
                let contents = fs::read_to_string(&path).unwrap_or_default();
                print!("{}", contents);
            } else {
                println!("(no config file found)");
            }
        }
        ConfigAction::Reset => {
            let path = config_path();
            if path.exists() {
                fs::remove_file(&path).expect("failed to delete config");
                println!("Config deleted. Run `tg setup` to reconfigure.");
            } else {
                println!("No config file found.");
            }
        }
    }
}
