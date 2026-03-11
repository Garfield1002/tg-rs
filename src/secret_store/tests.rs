use std::io;

use std::any::Any;
use std::sync::{Arc, Mutex};

use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
use keyring::{Error, mock, set_default_credential_builder};

use super::{
    SecretStoreError, backend_unavailable, delete_token, is_unavailable, load_token,
    map_delete_result, save_token,
};

#[test]
fn marks_no_storage_access_as_unavailable() {
    let err = keyring::Error::NoStorageAccess(Box::new(io::Error::other("permission denied")));
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
    let err =
        keyring::Error::PlatformFailure(Box::new(io::Error::other("unexpected backend timeout")));
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

static BUILDER_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn mock_store_is_treated_as_unavailable() {
    let _guard = BUILDER_LOCK.lock().expect("builder lock poisoned");
    set_default_credential_builder(mock::default_credential_builder());

    assert!(matches!(
        load_token(),
        Err(SecretStoreError::Unavailable(_))
    ));
    assert!(matches!(
        save_token("abc123"),
        Err(SecretStoreError::Unavailable(_))
    ));
    assert!(matches!(
        delete_token(),
        Err(SecretStoreError::Unavailable(_))
    ));
}

#[test]
fn unavailable_store_errors_are_mapped_to_unavailable() {
    let _guard = BUILDER_LOCK.lock().expect("builder lock poisoned");
    set_default_credential_builder(Box::new(UnavailableBuilder));

    assert!(matches!(
        load_token(),
        Err(SecretStoreError::Unavailable(_))
    ));
    assert!(matches!(
        save_token("abc123"),
        Err(SecretStoreError::Unavailable(_))
    ));
    assert!(matches!(
        delete_token(),
        Err(SecretStoreError::Unavailable(_))
    ));
}

#[test]
fn generic_store_errors_are_mapped_to_backend() {
    let _guard = BUILDER_LOCK.lock().expect("builder lock poisoned");
    set_default_credential_builder(Box::new(BackendErrorBuilder));

    assert!(matches!(load_token(), Err(SecretStoreError::Backend(_))));
    assert!(matches!(
        save_token("abc123"),
        Err(SecretStoreError::Backend(_))
    ));
    assert!(matches!(delete_token(), Err(SecretStoreError::Backend(_))));
}

#[test]
fn token_round_trip_succeeds_with_persistent_backend() {
    let _guard = BUILDER_LOCK.lock().expect("builder lock poisoned");
    let state = Arc::new(Mutex::new(None));
    set_default_credential_builder(Box::new(HappyPathBuilder {
        state: Arc::clone(&state),
    }));

    assert_eq!(load_token().expect("initial load should succeed"), None);

    save_token("abc123").expect("save should succeed");
    assert_eq!(
        load_token().expect("load after save should succeed"),
        Some("abc123".to_string())
    );

    delete_token().expect("delete should succeed");
    assert_eq!(load_token().expect("load after delete should succeed"), None);
}

#[derive(Debug)]
struct UnavailableBuilder;

impl CredentialBuilderApi for UnavailableBuilder {
    fn build(
        &self,
        _target: Option<&str>,
        _service: &str,
        _user: &str,
    ) -> keyring::Result<Box<Credential>> {
        Ok(Box::new(UnavailableCredential))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct UnavailableCredential;

impl CredentialApi for UnavailableCredential {
    fn set_secret(&self, _password: &[u8]) -> keyring::Result<()> {
        Err(Error::NoStorageAccess(Box::new(io::Error::other(
            "dbus unavailable",
        ))))
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        Err(Error::NoStorageAccess(Box::new(io::Error::other(
            "dbus unavailable",
        ))))
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        Err(Error::NoStorageAccess(Box::new(io::Error::other(
            "dbus unavailable",
        ))))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct BackendErrorBuilder;

impl CredentialBuilderApi for BackendErrorBuilder {
    fn build(
        &self,
        _target: Option<&str>,
        _service: &str,
        _user: &str,
    ) -> keyring::Result<Box<Credential>> {
        Ok(Box::new(BackendErrorCredential))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct BackendErrorCredential;

impl CredentialApi for BackendErrorCredential {
    fn set_secret(&self, _password: &[u8]) -> keyring::Result<()> {
        Err(Error::Invalid(
            "service".to_string(),
            "bad request".to_string(),
        ))
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        Err(Error::Invalid(
            "service".to_string(),
            "bad request".to_string(),
        ))
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        Err(Error::Invalid(
            "service".to_string(),
            "bad request".to_string(),
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct HappyPathBuilder {
    state: Arc<Mutex<Option<Vec<u8>>>>,
}

impl CredentialBuilderApi for HappyPathBuilder {
    fn build(
        &self,
        _target: Option<&str>,
        _service: &str,
        _user: &str,
    ) -> keyring::Result<Box<Credential>> {
        Ok(Box::new(HappyPathCredential {
            state: Arc::clone(&self.state),
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct HappyPathCredential {
    state: Arc<Mutex<Option<Vec<u8>>>>,
}

impl CredentialApi for HappyPathCredential {
    fn set_secret(&self, password: &[u8]) -> keyring::Result<()> {
        let mut state = self.state.lock().expect("happy path state poisoned");
        *state = Some(password.to_vec());
        Ok(())
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        let state = self.state.lock().expect("happy path state poisoned");
        match &*state {
            Some(secret) => Ok(secret.clone()),
            None => Err(Error::NoEntry),
        }
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        let mut state = self.state.lock().expect("happy path state poisoned");
        match state.take() {
            Some(_) => Ok(()),
            None => Err(Error::NoEntry),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
