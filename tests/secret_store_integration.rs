use std::any::Any;
use std::io;
use std::sync::Mutex;

use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
use keyring::{Error, mock, set_default_credential_builder};

#[path = "../src/secret_store.rs"]
mod secret_store;

static BUILDER_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn mock_store_is_supported_for_secret_store_calls() {
    let _guard = BUILDER_LOCK.lock().expect("builder lock poisoned");
    set_default_credential_builder(mock::default_credential_builder());

    assert_eq!(
        secret_store::load_token().expect("load should succeed"),
        None
    );
    secret_store::save_token("abc123").expect("save should succeed with mock store");

    // keyring's mock builder is EntryOnly: each call gets a fresh credential.
    assert_eq!(
        secret_store::load_token().expect("load should succeed"),
        None
    );

    secret_store::delete_token().expect("delete should be treated as success");
}

#[test]
fn unavailable_store_errors_are_mapped_to_unavailable() {
    let _guard = BUILDER_LOCK.lock().expect("builder lock poisoned");
    set_default_credential_builder(Box::new(UnavailableBuilder));

    assert!(matches!(
        secret_store::load_token(),
        Err(secret_store::SecretStoreError::Unavailable(_))
    ));
    assert!(matches!(
        secret_store::save_token("abc123"),
        Err(secret_store::SecretStoreError::Unavailable(_))
    ));
    assert!(matches!(
        secret_store::delete_token(),
        Err(secret_store::SecretStoreError::Unavailable(_))
    ));
}

#[test]
fn generic_store_errors_are_mapped_to_backend() {
    let _guard = BUILDER_LOCK.lock().expect("builder lock poisoned");
    set_default_credential_builder(Box::new(BackendErrorBuilder));

    assert!(matches!(
        secret_store::load_token(),
        Err(secret_store::SecretStoreError::Backend(_))
    ));
    assert!(matches!(
        secret_store::save_token("abc123"),
        Err(secret_store::SecretStoreError::Backend(_))
    ));
    assert!(matches!(
        secret_store::delete_token(),
        Err(secret_store::SecretStoreError::Backend(_))
    ));
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
