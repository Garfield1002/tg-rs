use keyring::Entry;

#[cfg(test)]
mod tests;

const SERVICE_NAME: &str = "tg-cli";
const TOKEN_USERNAME: &str = "telegram-bot-token";

#[derive(Debug)]
pub(crate) enum SecretStoreError {
    Unavailable(String),
    Backend(keyring::Error),
}

impl std::fmt::Display for SecretStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecretStoreError::Unavailable(message) => write!(f, "{message}"),
            SecretStoreError::Backend(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for SecretStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SecretStoreError::Unavailable(_) => None,
            SecretStoreError::Backend(err) => Some(err),
        }
    }
}

fn token_username_for(profile: Option<&str>) -> String {
    match profile {
        None => TOKEN_USERNAME.to_string(),
        Some(name) => format!("{TOKEN_USERNAME}-{name}"),
    }
}

pub(crate) async fn load_token_for(
    profile: Option<String>,
) -> Result<Option<String>, SecretStoreError> {
    let username = token_username_for(profile.as_deref());
    tokio::task::spawn_blocking(move || {
        let entry = Entry::new(SERVICE_NAME, &username)
            .map_err(map_keyring_error)
            .and_then(ensure_non_mock_backend)?;
        load_token_from_entry(&entry)
    })
    .await
    .unwrap_or_else(|_| {
        Err(SecretStoreError::Unavailable(
            "failed to access Secret Service from worker thread".to_string(),
        ))
    })
}

pub(crate) async fn save_token_for(
    profile: Option<String>,
    token: String,
) -> Result<(), SecretStoreError> {
    let username = token_username_for(profile.as_deref());
    tokio::task::spawn_blocking(move || {
        let entry = Entry::new(SERVICE_NAME, &username)
            .map_err(map_keyring_error)
            .and_then(ensure_non_mock_backend)?;
        save_token_to_entry(&entry, &token)
    })
    .await
    .unwrap_or_else(|_| {
        Err(SecretStoreError::Unavailable(
            "failed to access Secret Service from worker thread".to_string(),
        ))
    })
}

pub(crate) async fn delete_token_for(
    profile: Option<String>,
) -> Result<(), SecretStoreError> {
    let username = token_username_for(profile.as_deref());
    tokio::task::spawn_blocking(move || {
        let entry = Entry::new(SERVICE_NAME, &username)
            .map_err(map_keyring_error)
            .and_then(ensure_non_mock_backend)?;
        delete_token_from_entry(&entry)
    })
    .await
    .unwrap_or_else(|_| {
        Err(SecretStoreError::Unavailable(
            "failed to access Secret Service from worker thread".to_string(),
        ))
    })
}

fn load_token_from_entry(entry: &Entry) -> Result<Option<String>, SecretStoreError> {
    match entry.get_password() {
        Ok(token) => {
            if token.is_empty() {
                Ok(None)
            } else {
                Ok(Some(token))
            }
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(map_keyring_error(err)),
    }
}

fn save_token_to_entry(entry: &Entry, token: &str) -> Result<(), SecretStoreError> {
    entry.set_password(token).map_err(map_keyring_error)
}

fn delete_token_from_entry(entry: &Entry) -> Result<(), SecretStoreError> {
    map_delete_result(entry.delete_credential())
}

fn map_delete_result(result: Result<(), keyring::Error>) -> Result<(), SecretStoreError> {
    match result {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(map_keyring_error(err)),
    }
}

fn ensure_non_mock_backend(entry: Entry) -> Result<Entry, SecretStoreError> {
    match entry
        .get_credential()
        .downcast_ref::<keyring::mock::MockCredential>()
    {
        Some(_) => Err(SecretStoreError::Unavailable(
            "secure keyring backend unavailable (keyring is using a non-persistent mock backend)"
                .to_string(),
        )),
        None => Ok(entry),
    }
}

pub(crate) fn is_unavailable(err: &SecretStoreError) -> bool {
    matches!(err, SecretStoreError::Unavailable(_))
}

fn map_keyring_error(err: keyring::Error) -> SecretStoreError {
    if backend_unavailable(&err) {
        SecretStoreError::Unavailable(err.to_string())
    } else {
        SecretStoreError::Backend(err)
    }
}

fn backend_unavailable(err: &keyring::Error) -> bool {
    match err {
        keyring::Error::NoStorageAccess(_) => true,
        keyring::Error::PlatformFailure(platform_err) => {
            let message = platform_err.to_string().to_ascii_lowercase();
            message.contains("secret service")
                || message.contains("dbus")
                || message.contains("kwallet")
                || message.contains("wallet")
                || message.contains("no such interface")
                || message.contains("is not available")
                || message.contains("not available")
                || message.contains("service unknown")
                || message.contains("no session bus")
        }
        _ => false,
    }
}
