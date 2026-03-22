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

use crate::config::{ConfigFile, config_path};

mod config;
mod secret_store;

#[derive(Debug)]
pub enum SendMessageError {
    MissingToken(Option<String>),
    MissingChatId(Option<String>),
    RuntimeInit(io::Error),
    Request(RequestError),
}

impl std::fmt::Display for SendMessageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendMessageError::MissingToken(None) => {
                write!(f, "No token configured. Run `tg setup` first.")
            }
            SendMessageError::MissingToken(Some(profile)) => {
                write!(
                    f,
                    "No token configured for profile '{profile}'. Run `tg --profile {profile} setup` first."
                )
            }
            SendMessageError::MissingChatId(None) => {
                write!(f, "No chat ID configured. Run `tg setup` first.")
            }
            SendMessageError::MissingChatId(Some(profile)) => {
                write!(
                    f,
                    "No chat ID configured for profile '{profile}'. Run `tg --profile {profile} setup` first."
                )
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
            SendMessageError::MissingToken(_) | SendMessageError::MissingChatId(_) => None,
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
    pub async fn from_config(profile: Option<&str>) -> TgResult<Self> {
        let file = ConfigFile::load();
        let profile_data = file.get_profile(profile);
        let token = profile_data
            .resolved_token_for(profile)
            .await
            .ok_or_else(|| SendMessageError::MissingToken(profile.map(|s| s.to_string())))?;
        let chat_id = profile_data
            .chat_id
            .ok_or_else(|| SendMessageError::MissingChatId(profile.map(|s| s.to_string())))?;

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

pub async fn load_setup_status(profile: Option<&str>) -> SetupStatus {
    let file = ConfigFile::load();
    let profile_data = file.get_profile(profile);
    SetupStatus {
        has_token: profile_data.resolved_token_for(profile).await.is_some(),
        chat_id: profile_data.chat_id,
    }
}

pub async fn bot_from_config_token(profile: Option<&str>) -> TgResult<Bot> {
    let file = ConfigFile::load();
    let profile_data = file.get_profile(profile);
    let token = profile_data
        .resolved_token_for(profile)
        .await
        .ok_or_else(|| SendMessageError::MissingToken(profile.map(|s| s.to_string())))?;
    Ok(Bot::new(token))
}

pub async fn listen_config(profile: Option<&str>) -> TgResult<(Bot, ChatId)> {
    let bot = bot_from_config_token(profile).await?;
    let file = ConfigFile::load();
    let chat_id = file
        .get_profile(profile)
        .chat_id
        .ok_or_else(|| SendMessageError::MissingChatId(profile.map(|s| s.to_string())))?;
    Ok((bot, ChatId(chat_id)))
}

pub async fn inspect_bot_config(profile: Option<&str>) -> BotConfigStatus {
    let path = config_path();
    let file = ConfigFile::load();
    let profile_data = file.get_profile(profile);

    let (secret_service, token) =
        match secret_store::load_token_for(profile.map(|s| s.to_string())).await {
            Ok(Some(_)) => (SecretServiceStatus::Available, TokenStatus::SecretService),
            Ok(None) => (
                SecretServiceStatus::Available,
                if profile_data.token.is_some() {
                    TokenStatus::PlaintextFallback
                } else {
                    TokenStatus::NotConfigured
                },
            ),
            Err(err) if secret_store::is_unavailable(&err) => (
                SecretServiceStatus::Unavailable,
                if profile_data.token.is_some() {
                    TokenStatus::PlaintextFallback
                } else {
                    TokenStatus::NotConfigured
                },
            ),
            Err(err) => (
                SecretServiceStatus::Error(err.to_string()),
                if profile_data.token.is_some() {
                    TokenStatus::PlaintextFallback
                } else {
                    TokenStatus::NotConfigured
                },
            ),
        };

    BotConfigStatus {
        config_file_present: path.exists(),
        path,
        chat_id: profile_data.chat_id,
        token,
        secret_service,
    }
}

pub async fn save_bot_config(token: &str, chat_id: i64, profile: Option<&str>) {
    let mut file = ConfigFile::load();
    let mut profile_data = file.get_profile(profile);
    let _ = profile_data.persist_token_for(token, profile).await;
    profile_data.chat_id = Some(chat_id);
    file.set_profile(profile, profile_data);
    file.save();
}

pub fn save_chat_id(chat_id: i64, profile: Option<&str>) {
    let mut file = ConfigFile::load();
    let mut profile_data = file.get_profile(profile);
    profile_data.chat_id = Some(chat_id);
    file.set_profile(profile, profile_data);
    file.save();
}

pub async fn delete_bot_config(profile: Option<&str>) -> bool {
    let path = config_path();
    let mut file = ConfigFile::load();
    let had_data = file.get_profile(profile).chat_id.is_some();
    file.delete_profile(profile);

    if file.is_empty() {
        if path.exists() {
            fs::remove_file(&path).expect("failed to delete config");
        }
    } else {
        file.save();
    }

    let mut removed_any = had_data;

    match secret_store::delete_token_for(profile.map(|s| s.to_string())).await {
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

pub fn list_profile_names() -> Vec<String> {
    let file = ConfigFile::load();
    let mut names: Vec<String> = file.profiles.keys().cloned().collect();
    names.sort();
    names
}

pub async fn send_tg_message(
    text: String,
    parse_mode: ParseMode,
    silent: bool,
    profile: Option<&str>,
) -> TgResult<()> {
    let session = TgSession::from_config(profile).await?;
    session.send_message(text, parse_mode, silent).await?;
    Ok(())
}

pub fn send_tg_message_blocking(
    text: String,
    parse_mode: ParseMode,
    silent: bool,
    profile: Option<&str>,
) -> TgResult<()> {
    let profile_owned = profile.map(|s| s.to_string());
    if tokio::runtime::Handle::try_current().is_ok() {
        let worker = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(SendMessageError::RuntimeInit)?;
            rt.block_on(send_tg_message(
                text,
                parse_mode,
                silent,
                profile_owned.as_deref(),
            ))
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
    rt.block_on(send_tg_message(
        text,
        parse_mode,
        silent,
        profile_owned.as_deref(),
    ))
}

#[cfg(feature = "non-blocking")]
#[macro_export]
macro_rules! telegram {
    () => {{
        $crate::telegram!("")
    }};
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        let profile = std::env::var("TG_PROFILE").ok();
        tokio::spawn(async move {
            if let Err(err) = $crate::send_tg_message(
                msg,
                $crate::ParseMode::Markdown,
                false,
                profile.as_deref(),
            )
            .await
            {
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
        let profile = std::env::var("TG_PROFILE").ok();
        if let Err(err) = $crate::send_tg_message_blocking(
            format!($($arg)*),
            $crate::ParseMode::Markdown,
            false,
            profile.as_deref(),
        ) {
            eprintln!("{err}");
        }
    }};
}
