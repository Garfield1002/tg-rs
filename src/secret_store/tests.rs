use std::io;

use keyring::{Entry, Error, mock, mock::MockCredential};

use super::{
    SecretStoreError, backend_unavailable, delete_token_from_entry, ensure_non_mock_backend,
    is_unavailable, load_token_from_entry, map_delete_result, save_token_to_entry,
};

macro_rules! io_error_variant {
    ($variant:ident, $message:expr) => {
        Error::$variant(Box::new(io::Error::other($message)))
    };
}

#[test]
fn marks_no_storage_access_as_unavailable() {
    assert!(backend_unavailable(&io_error_variant!(
        NoStorageAccess,
        "permission denied"
    )));
}

#[test]
fn marks_platform_failures_with_secret_service_signals_as_unavailable() {
    assert!(backend_unavailable(&io_error_variant!(
        PlatformFailure,
        "Secret Service is not available on this session bus"
    )));
}

#[test]
fn does_not_mark_unrelated_platform_failures_as_unavailable() {
    assert!(!backend_unavailable(&io_error_variant!(
        PlatformFailure,
        "unexpected backend timeout"
    )));
}

#[test]
fn does_not_mark_no_entry_as_unavailable() {
    assert!(!backend_unavailable(&Error::NoEntry));
}

#[test]
fn unavailable_helper_matches_only_unavailable_variant() {
    assert!(is_unavailable(&SecretStoreError::Unavailable(
        "dbus unavailable".to_string()
    )));

    assert!(!is_unavailable(&SecretStoreError::Backend(
        keyring::Error::NoEntry
    )));
}

#[test]
fn delete_mapping_treats_no_entry_as_success() {
    assert!(map_delete_result(Err(Error::NoEntry)).is_ok());
}

#[test]
fn delete_mapping_marks_storage_access_errors_as_unavailable() {
    assert!(matches!(
        map_delete_result(Err(io_error_variant!(NoStorageAccess, "permission denied"))),
        Err(SecretStoreError::Unavailable(_))
    ));
}

#[test]
fn delete_mapping_preserves_non_unavailable_backend_errors() {
    assert!(matches!(
        map_delete_result(Err(Error::Invalid(
            "service".to_string(),
            "bad entry".to_string(),
        ))),
        Err(SecretStoreError::Backend(_))
    ));
}

#[test]
fn mock_store_is_treated_as_unavailable() {
    assert!(matches!(
        ensure_non_mock_backend(mock_entry()),
        Err(SecretStoreError::Unavailable(_))
    ));
}

#[test]
fn unavailable_store_errors_are_mapped_to_unavailable() {
    let err = || io_error_variant!(NoStorageAccess, "dbus unavailable");

    assert!(matches!(
        run_with_mock_error(err(), load_token_from_entry),
        Err(SecretStoreError::Unavailable(_))
    ));

    assert!(matches!(
        run_with_mock_error(err(), |entry| save_token_to_entry(entry, "abc123")),
        Err(SecretStoreError::Unavailable(_))
    ));

    assert!(matches!(
        run_with_mock_error(err(), delete_token_from_entry),
        Err(SecretStoreError::Unavailable(_))
    ));
}

#[test]
fn generic_store_errors_are_mapped_to_backend() {
    let err = || Error::Invalid("service".to_string(), "bad request".to_string());

    assert!(matches!(
        run_with_mock_error(err(), load_token_from_entry),
        Err(SecretStoreError::Backend(_))
    ));

    assert!(matches!(
        run_with_mock_error(err(), |entry| save_token_to_entry(entry, "abc123")),
        Err(SecretStoreError::Backend(_))
    ));

    assert!(matches!(
        run_with_mock_error(err(), delete_token_from_entry),
        Err(SecretStoreError::Backend(_))
    ));
}

#[test]
fn token_round_trip_succeeds_with_persistent_backend() {
    let entry = mock_entry();

    assert_eq!(
        load_token_from_entry(&entry).expect("initial load should succeed"),
        None
    );

    save_token_to_entry(&entry, "abc123").expect("save should succeed");

    assert_eq!(
        load_token_from_entry(&entry).expect("load after save should succeed"),
        Some("abc123".to_string())
    );

    delete_token_from_entry(&entry).expect("delete should succeed");

    assert_eq!(
        load_token_from_entry(&entry).expect("load after delete should succeed"),
        None
    );
}

fn mock_entry() -> Entry {
    let builder = mock::default_credential_builder();
    let credential = builder
        .build(None, "service", "user")
        .expect("failed to build mock credential");
    Entry::new_with_credential(credential)
}

fn set_mock_error(entry: &Entry, err: Error) {
    let mock: &MockCredential = entry
        .get_credential()
        .downcast_ref()
        .expect("downcast to MockCredential failed");
    mock.set_error(err);
}

fn run_with_mock_error<T>(
    err: Error,
    operation: impl FnOnce(&Entry) -> Result<T, SecretStoreError>,
) -> Result<T, SecretStoreError> {
    let entry = mock_entry();
    set_mock_error(&entry, err);
    operation(&entry)
}
