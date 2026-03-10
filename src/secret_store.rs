use keyring::Entry;

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

pub(crate) fn load_token() -> Result<Option<String>, SecretStoreError> {
    run_outside_tokio(load_token_impl)
}

#[allow(dead_code)]
pub(crate) fn save_token(token: &str) -> Result<(), SecretStoreError> {
    let token = token.to_string();
    run_outside_tokio(move || save_token_impl(&token))
}

#[allow(dead_code)]
pub(crate) fn delete_token() -> Result<(), SecretStoreError> {
    run_outside_tokio(delete_token_impl)
}

fn load_token_impl() -> Result<Option<String>, SecretStoreError> {
    let entry = token_entry()?;
    ensure_non_mock_backend(&entry)?;

    match entry.get_password() {
        Ok(token) => {
            if token.is_empty() {
                Ok(None)
            } else {
                Ok(Some(token))
            }
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) if backend_unavailable(&err) => Err(SecretStoreError::Unavailable(err.to_string())),
        Err(err) => Err(SecretStoreError::Backend(err)),
    }
}

fn save_token_impl(token: &str) -> Result<(), SecretStoreError> {
    let entry = token_entry()?;
    ensure_non_mock_backend(&entry)?;

    match entry.set_password(token) {
        Ok(()) => Ok(()),
        Err(err) if backend_unavailable(&err) => Err(SecretStoreError::Unavailable(err.to_string())),
        Err(err) => Err(SecretStoreError::Backend(err)),
    }
}

fn delete_token_impl() -> Result<(), SecretStoreError> {
    let entry = token_entry()?;
    ensure_non_mock_backend(&entry)?;

    match map_delete_result(entry.delete_credential()) {
        Ok(()) => Ok(()),
        Err(err) => Err(err),
    }
}

fn map_delete_result(result: Result<(), keyring::Error>) -> Result<(), SecretStoreError> {
    match result {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) if backend_unavailable(&err) => Err(SecretStoreError::Unavailable(err.to_string())),
        Err(err) => Err(SecretStoreError::Backend(err)),
    }
}

fn run_outside_tokio<F, T>(operation: F) -> Result<T, SecretStoreError>
where
    F: FnOnce() -> Result<T, SecretStoreError> + Send + 'static,
    T: Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        return match std::thread::spawn(operation).join() {
            Ok(result) => result,
            Err(_) => Err(SecretStoreError::Unavailable(
                "failed to access Secret Service from worker thread".to_string(),
            )),
        };
    }

    operation()
}

fn ensure_non_mock_backend(entry: &Entry) -> Result<(), SecretStoreError> {
    if entry
        .get_credential()
        .downcast_ref::<keyring::mock::MockCredential>()
        .is_some()
    {
        return Err(SecretStoreError::Unavailable(
            "secure keyring backend unavailable (keyring is using a non-persistent mock backend)"
                .to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn is_unavailable(err: &SecretStoreError) -> bool {
    matches!(err, SecretStoreError::Unavailable(_))
}

fn token_entry() -> Result<Entry, SecretStoreError> {
    Entry::new(SERVICE_NAME, TOKEN_USERNAME).map_err(|err| {
        if backend_unavailable(&err) {
            SecretStoreError::Unavailable(err.to_string())
        } else {
            SecretStoreError::Backend(err)
        }
    })
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

#[cfg(test)]
mod tests {
    use std::io;

    use super::{SecretStoreError, backend_unavailable, is_unavailable, map_delete_result};

    #[test]
    fn marks_no_storage_access_as_unavailable() {
        let err = keyring::Error::NoStorageAccess(Box::new(io::Error::other(
            "permission denied",
        )));
        assert!(backend_unavailable(&err));
    }

    #[test]
    fn marks_platform_failures_with_secret_service_signals_as_unavailable() {
        let err = keyring::Error::PlatformFailure(Box::new(io::Error::other(
            "Secret Service is not available on this session bus",
        )));
        assert!(backend_unavailable(&err));
    }

    #[test]
    fn does_not_mark_unrelated_platform_failures_as_unavailable() {
        let err = keyring::Error::PlatformFailure(Box::new(io::Error::other(
            "unexpected backend timeout",
        )));
        assert!(!backend_unavailable(&err));
    }

    #[test]
    fn does_not_mark_no_entry_as_unavailable() {
        assert!(!backend_unavailable(&keyring::Error::NoEntry));
    }

    #[test]
    fn unavailable_helper_matches_only_unavailable_variant() {
        let unavailable = SecretStoreError::Unavailable("dbus unavailable".to_string());
        let backend = SecretStoreError::Backend(keyring::Error::NoEntry);

        assert!(is_unavailable(&unavailable));
        assert!(!is_unavailable(&backend));
    }

    #[test]
    fn delete_mapping_treats_no_entry_as_success() {
        assert!(map_delete_result(Err(keyring::Error::NoEntry)).is_ok());
    }

    #[test]
    fn delete_mapping_marks_storage_access_errors_as_unavailable() {
        let result = map_delete_result(Err(keyring::Error::NoStorageAccess(Box::new(
            io::Error::other("permission denied"),
        ))));
        assert!(matches!(result, Err(SecretStoreError::Unavailable(_))));
    }

    #[test]
    fn delete_mapping_preserves_non_unavailable_backend_errors() {
        let result = map_delete_result(Err(keyring::Error::Invalid(
            "service".to_string(),
            "bad entry".to_string(),
        )));
        assert!(matches!(result, Err(SecretStoreError::Backend(_))));
    }
}
