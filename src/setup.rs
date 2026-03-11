//! Setup wizard for first-time users

use std::sync::{Arc, Mutex};

use teloxide::{
    Bot,
    prelude::Requester,
    types::{ChatId, Message},
};

use tg_cli::{bot_from_config_token, load_setup_status, save_bot_config, save_chat_id};

/// Setup wizard for first-time users
pub(crate) async fn run_setup() {
    let config = load_setup_status();

    if config.chat_id.is_some() {
        eprintln!("Already configured. Run `tg config reset` to reconfigure.");
        std::process::exit(1);
    }

    eprintln!("Step 1 of 3 — Bot token\n\n");
    let (bot, token) = if config.has_token {
        eprintln!("Bot token already configured. Run `tg config reset` to reconfigure.");
        (
            bot_from_config_token().expect("token reported configured but could not be loaded"),
            None,
        )
    } else {
        let token = prompt_token();
        (Bot::new(token.clone()), Some(token))
    };

    let code = pairing_code();

    eprintln!(
        "\nStep 2 of 3 — Link your account\n\
         \n\
         Open your bot in Telegram and send it /start.\n\
         Waiting..."
    );
    let chat_id = retrieve_chat_id(bot, &code).await;

    eprintln!("\nStep 3 of 3 — Confirm pairing\n\n");
    verify_code(&code).await;

    if let Some(token) = token {
        save_bot_config(&token, chat_id);
    } else {
        save_chat_id(chat_id);
    }

    eprintln!("\nAll done! Try it out:\n\n  tg \"Hello, World!\"");
    std::process::exit(0);
}

/// Retrieve a bot token
fn prompt_token() -> String {
    let suggested_username = random_bot_username();
    let token = prompt(&format!(
        "tg needs a Telegram bot to send messages on your behalf.\n\
             If you don't have one yet:\n\
               1. Open Telegram and start a chat with @BotFather (https://t.me/botfather)\n\
               2. Send /newbot — suggested username: {suggested_username}\n\
               3. Copy the token BotFather gives you\n\
             \n\
             Paste your bot token: ",
    ));
    if token.is_empty() {
        eprintln!("Token cannot be empty.");
        std::process::exit(1);
    }
    token
}

/// Retrieve a chat ID, this needs to be done in a seperate thread since we need to wait for the user to send a message to the bot in Telegram
async fn retrieve_chat_id(bot: Bot, code: &str) -> i64 {
    let (tx, rx) = tokio::sync::oneshot::channel::<ChatId>();
    let tx = Arc::new(Mutex::new(Some(tx)));

    let code_for_repl = code.to_string();
    tokio::spawn(async move {
        teloxide::repl(bot, move |bot: Bot, msg: Message| {
            let tx = Arc::clone(&tx);
            let code = code_for_repl.clone();
            async move {
                if msg.text() == Some("/start") {
                    let sender = tx.lock().unwrap().take();
                    if let Some(sender) = sender {
                        bot.send_message(
                            msg.chat.id,
                            format!("Your pairing code is: {code}\n\nEnter it in your terminal to complete setup."),
                        )
                        .await?;
                        let _ = sender.send(msg.chat.id);
                    }
                }
                Ok(())
            }
        })
        .await;
    });

    let chat_id = rx.await.expect("setup listener exited unexpectedly");
    chat_id.0
}

/// Ensure the user has access to the chat they're pairing with
async fn verify_code(code: &str) {
    let input = prompt(
        "Your bot just sent you a 3-digit code in Telegram.\n\
         Enter it here: ",
    );

    if input.trim() != code {
        eprintln!("Wrong code. Setup aborted — no config was saved.");
        std::process::exit(1);
    }
}

/// Prints a prompt and reads a line of input from the user
fn prompt(label: &str) -> String {
    eprint!("{}", label);
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .expect("failed to read input");
    input.trim().to_string()
}

/// Suggest a random bot username to help users who don't have a bot yet.
/// This bot isn't meant to be used by other users, so we don't want anything that looks like a real bot username.
/// We just want something random that won't collide with existing bots.
fn random_bot_username() -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut buf = [0u8; 24];
    let mut f = std::fs::File::open("/dev/urandom").expect("failed to open /dev/urandom");
    std::io::Read::read_exact(&mut f, &mut buf).expect("failed to read /dev/urandom");
    let name: String = buf
        .iter()
        .map(|&b| CHARS[b as usize % CHARS.len()] as char)
        .collect();
    format!("{name}_bot")
}

/// This is not a security measure !
/// This is just used to ensure that the user has access to the chat they're pairing with and to prevent them from accidentally pairing with the wrong chat
fn pairing_code() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("{:03}", nanos % 1000)
}
