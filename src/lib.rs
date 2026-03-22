use std::{
    fs,
    io::{self},
    path::PathBuf,
};

use teloxide::{
    Bot, RequestError,
    payloads::{EditMessageTextSetters, SendDocumentSetters, SendMessageSetters},
    prelude::Requester,
    types::{ChatId, InputFile, MessageId, ParseMode as TeloxideParseMode},
};

use crate::config::Config;

mod config;
mod secret_store;

#[derive(Debug)]
pub enum SendMessageError {
    MissingToken,
    MissingChatId,
    RuntimeInit(io::Error),
    Request(RequestError),
}

impl std::fmt::Display for SendMessageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendMessageError::MissingToken => {
                write!(f, "No token configured. Run `tg setup` first.")
            }
            SendMessageError::MissingChatId => {
                write!(f, "No chat ID configured. Run `tg setup` first.")
            }
            SendMessageError::RuntimeInit(err) => {
                write!(f, "Failed to initialize async runtime: {err}")
            }
            SendMessageError::Request(err) => write!(f, "Failed to send message: {err}"),
        }
    }
}

impl std::error::Error for SendMessageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SendMessageError::RuntimeInit(err) => Some(err),
            SendMessageError::Request(err) => Some(err),
            SendMessageError::MissingToken | SendMessageError::MissingChatId => None,
        }
    }
}

pub type TgResult<T> = Result<T, SendMessageError>;

pub struct SetupStatus {
    pub has_token: bool,
    pub chat_id: Option<i64>,
}

pub struct BotConfigStatus {
    pub path: PathBuf,
    pub config_file_present: bool,
    pub chat_id: Option<i64>,
    pub token: TokenStatus,
    pub secret_service: SecretServiceStatus,
}

pub enum TokenStatus {
    SecretService,
    PlaintextFallback,
    NotConfigured,
}

pub enum SecretServiceStatus {
    Available,
    Unavailable,
    Error(String),
}

pub struct TgSession {
    bot: Bot,
    chat_id: ChatId,
}

#[derive(Debug, Clone, Copy)]
pub enum ParseMode {
    Markdown,
    Html,
}

impl From<ParseMode> for TeloxideParseMode {
    fn from(mode: ParseMode) -> Self {
        match mode {
            ParseMode::Markdown => TeloxideParseMode::MarkdownV2,
            ParseMode::Html => TeloxideParseMode::Html,
        }
    }
}

impl TgSession {
    pub async fn from_config() -> TgResult<Self> {
        let config = Config::load();
        let token = config
            .resolved_token()
            .await
            .ok_or(SendMessageError::MissingToken)?;
        let chat_id = config.chat_id.ok_or(SendMessageError::MissingChatId)?;

        Ok(Self {
            bot: Bot::new(token),
            chat_id: ChatId(chat_id),
        })
    }

    fn sanitize_text(text: String) -> String {
        text.replace("\r\n", "\n")
            .replace('_', "\\_")
            .replace('*', "\\*")
            .replace('[', "\\[")
            .replace(']', "\\]")
            .replace('(', "\\(")
            .replace(')', "\\)")
            .replace('~', "\\~")
            .replace('`', "\\`")
            .replace('>', "\\>")
            .replace('#', "\\#")
            .replace('+', "\\+")
            .replace('-', "\\-")
            .replace('=', "\\=")
            .replace('|', "\\|")
            .replace('{', "\\{")
            .replace('}', "\\}")
            .replace('.', "\\.")
            .replace('!', "\\!")
    }

    pub async fn send_message(
        &self,
        text: String,
        parse_mode: ParseMode,
        silent: bool,
    ) -> TgResult<i32> {
        let mut req = self
            .bot
            .send_message(self.chat_id, Self::sanitize_text(text));
        req = req.parse_mode(parse_mode.into());

        if silent {
            req = req.disable_notification(true);
        }

        let message = req.await.map_err(SendMessageError::Request)?;
        Ok(message.id.0)
    }

    pub async fn send_document(&self, path: &std::path::Path, silent: bool) -> TgResult<()> {
        let input_file = InputFile::file(path);
        let mut req = self.bot.send_document(self.chat_id, input_file);

        if silent {
            req = req.disable_notification(true);
        }

        req.await.map_err(SendMessageError::Request)?;
        Ok(())
    }

    pub async fn edit_message(
        &self,
        message_id: i32,
        text: String,
        parse_mode: ParseMode,
    ) -> TgResult<()> {
        let mut req = self.bot.edit_message_text(
            self.chat_id,
            MessageId(message_id),
            Self::sanitize_text(text),
        );
        req = req.parse_mode(parse_mode.into());
        req.await.map_err(SendMessageError::Request)?;
        Ok(())
    }
}

pub async fn load_setup_status() -> SetupStatus {
    let config = Config::load();
    SetupStatus {
        has_token: config.resolved_token().await.is_some(),
        chat_id: config.chat_id,
    }
}

pub async fn bot_from_config_token() -> TgResult<Bot> {
    let config = Config::load();
    let token = config
        .resolved_token()
        .await
        .ok_or(SendMessageError::MissingToken)?;
    Ok(Bot::new(token))
}

pub async fn listen_config() -> TgResult<(Bot, ChatId)> {
    let bot = bot_from_config_token().await?;
    let chat_id = Config::load()
        .chat_id
        .ok_or(SendMessageError::MissingChatId)?;
    Ok((bot, ChatId(chat_id)))
}

pub async fn inspect_bot_config() -> BotConfigStatus {
    let path = config::config_path();
    let config = Config::load();

    let (secret_service, token) = match secret_store::load_token().await {
        Ok(Some(_)) => (SecretServiceStatus::Available, TokenStatus::SecretService),
        Ok(None) => (
            SecretServiceStatus::Available,
            if config.token.is_some() {
                TokenStatus::PlaintextFallback
            } else {
                TokenStatus::NotConfigured
            },
        ),
        Err(err) if secret_store::is_unavailable(&err) => (
            SecretServiceStatus::Unavailable,
            if config.token.is_some() {
                TokenStatus::PlaintextFallback
            } else {
                TokenStatus::NotConfigured
            },
        ),
        Err(err) => (
            SecretServiceStatus::Error(err.to_string()),
            if config.token.is_some() {
                TokenStatus::PlaintextFallback
            } else {
                TokenStatus::NotConfigured
            },
        ),
    };

    BotConfigStatus {
        config_file_present: path.exists(),
        path,
        chat_id: config.chat_id,
        token,
        secret_service,
    }
}

pub async fn save_bot_config(token: &str, chat_id: i64) {
    let mut config = Config::load();
    let _ = config.persist_token(token).await;
    config.chat_id = Some(chat_id);
    config.save();
}

pub fn save_chat_id(chat_id: i64) {
    let mut config = Config::load();
    config.chat_id = Some(chat_id);
    config.save();
}

pub async fn delete_bot_config() -> bool {
    let path = config::config_path();
    let mut removed_any = false;

    if path.exists() {
        fs::remove_file(&path).expect("failed to delete config");
        removed_any = true;
    }

    match secret_store::delete_token().await {
        Ok(()) => {
            removed_any = true;
        }
        Err(err) if secret_store::is_unavailable(&err) => {
            eprintln!(
                "Warning: Secret Service API unavailable; could not delete keyring token ({err})."
            );
        }
        Err(err) => {
            eprintln!("Warning: failed to delete keyring token ({err}).");
        }
    }

    removed_any
}

pub async fn send_tg_message(text: String, parse_mode: ParseMode, silent: bool) -> TgResult<()> {
    let session = TgSession::from_config().await?;
    session.send_message(text, parse_mode, silent).await?;
    Ok(())
}

pub fn send_tg_message_blocking(text: String, parse_mode: ParseMode, silent: bool) -> TgResult<()> {
    if tokio::runtime::Handle::try_current().is_ok() {
        let worker = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(SendMessageError::RuntimeInit)?;
            rt.block_on(send_tg_message(text, parse_mode, silent))
        });

        return match worker.join() {
            Ok(result) => result,
            Err(_) => Err(SendMessageError::RuntimeInit(io::Error::other(
                "failed to join Telegram sender thread",
            ))),
        };
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(SendMessageError::RuntimeInit)?;
    rt.block_on(send_tg_message(text, parse_mode, silent))
}

#[cfg(feature = "non-blocking")]
#[macro_export]
macro_rules! telegram {
    () => {{
        $crate::telegram!("")
    }};
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        tokio::spawn(async move {
            if let Err(err) = $crate::send_tg_message(
                msg,
                $crate::ParseMode::Markdown,
                false,
            ).await {
                eprintln!("{err}");
            }
        });
    }};
}

#[cfg(not(feature = "non-blocking"))]
#[macro_export]
macro_rules! telegram {
    () => {{
        $crate::telegram!("")
    }};
    ($($arg:tt)*) => {{
        if let Err(err) = $crate::send_tg_message_blocking(
            format!($($arg)*),
            $crate::ParseMode::Markdown,
            false,
        ) {
            eprintln!("{err}");
        }
    }};
}
