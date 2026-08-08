use std::fs;

use tale::secrets::{
    AccessTokenRecord, CredentialRecord, CredentialStore, FileCredentialStore, OAuthClientRecord,
    SecretValue, SecretsError,
};

fn access_token(value: &str) -> CredentialRecord {
    CredentialRecord::AccessToken(AccessTokenRecord {
        version: 1,
        access_token: SecretValue::new(value),
    })
}

fn oauth_client() -> CredentialRecord {
    CredentialRecord::OAuthClient(OAuthClientRecord {
        version: 1,
        client_id: SecretValue::new("fictional-client-id"),
        client_secret: SecretValue::new("fictional-client-secret"),
        requested_scopes: vec!["devices:core:read".to_owned(), "users:read".to_owned()],
    })
}

/// The keyring backend kept secrets out of the filesystem entirely. A file backend only
/// holds that line if the mode does, so the mode is part of the contract.
#[cfg(unix)]
#[test]
fn the_store_is_created_readable_only_by_its_owner() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir();
    assert!(directory.is_ok());
    let Ok(directory) = directory else { return };
    let path = directory.path().join("nested").join("credentials.toml");
    let store = FileCredentialStore::new(&path);

    assert!(store.set("ops", &access_token("fictional-token")).is_ok());

    let file_mode = fs::metadata(&path).map(|data| data.permissions().mode() & 0o777);
    assert_eq!(file_mode.ok(), Some(0o600), "credential file must be 0600");
    let parent_mode = fs::metadata(path.parent().unwrap_or(directory.path()))
        .map(|data| data.permissions().mode() & 0o777);
    assert_eq!(parent_mode.ok(), Some(0o700), "parent must be 0700");
}

#[cfg(unix)]
#[test]
fn a_widened_store_is_refused_rather_than_read() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir();
    assert!(directory.is_ok());
    let Ok(directory) = directory else { return };
    let path = directory.path().join("credentials.toml");
    let store = FileCredentialStore::new(&path);
    assert!(store.set("ops", &access_token("fictional-token")).is_ok());

    assert!(fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).is_ok());
    let read = store.get("ops");
    assert!(
        matches!(read, Err(SecretsError::UnsafePermissions(_))),
        "a group- or world-readable store must be refused"
    );
}

#[test]
fn records_round_trip_through_the_file_with_their_kind_and_scopes() -> Result<(), String> {
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let store = FileCredentialStore::new(directory.path().join("credentials.toml"));

    store
        .set("token-profile", &access_token("fictional-token"))
        .map_err(|error| error.to_string())?;
    store
        .set("oauth-profile", &oauth_client())
        .map_err(|error| error.to_string())?;

    let token = store
        .get("token-profile")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "token record missing".to_owned())?;
    assert_eq!(token.kind().label(), "access_token");
    assert!(token.requested_scopes().is_empty());

    let oauth = store
        .get("oauth-profile")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "oauth record missing".to_owned())?;
    assert_eq!(oauth.kind().label(), "oauth_client");
    assert_eq!(
        oauth.requested_scopes(),
        vec!["devices:core:read".to_owned(), "users:read".to_owned()]
    );
    Ok(())
}

/// Writing one profile must not disturb another, and must not rewrite the file in place
/// where a crash could leave it truncated.
#[test]
fn writing_one_record_preserves_the_others_and_leaves_no_temporary() -> Result<(), String> {
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let path = directory.path().join("credentials.toml");
    let store = FileCredentialStore::new(&path);

    store
        .set("first", &access_token("first-token"))
        .map_err(|error| error.to_string())?;
    store
        .set("second", &oauth_client())
        .map_err(|error| error.to_string())?;
    store
        .set("first", &access_token("replacement-token"))
        .map_err(|error| error.to_string())?;

    assert!(
        store
            .get("second")
            .map_err(|error| error.to_string())?
            .is_some(),
        "an unrelated record must survive a neighbouring write"
    );

    let leftovers = fs::read_dir(directory.path())
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
        .count();
    assert_eq!(leftovers, 0, "atomic write left a temporary file behind");
    Ok(())
}

#[test]
fn deleting_reports_whether_anything_was_removed() -> Result<(), String> {
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let store = FileCredentialStore::new(directory.path().join("credentials.toml"));

    assert!(!store.delete("absent").map_err(|e| e.to_string())?);
    store
        .set("present", &access_token("fictional-token"))
        .map_err(|error| error.to_string())?;
    assert!(store.delete("present").map_err(|e| e.to_string())?);
    assert!(
        store
            .get("present")
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}

#[test]
fn a_missing_store_reads_as_empty_rather_than_failing() -> Result<(), String> {
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let store = FileCredentialStore::new(directory.path().join("does-not-exist.toml"));
    assert!(store.get("anything").map_err(|e| e.to_string())?.is_none());
    Ok(())
}

/// The file is the user's to inspect, so it has to be legible TOML rather than an
/// encoded blob — but it must never be mistaken for the shareable configuration.
#[test]
fn the_file_is_readable_toml_keyed_by_reference() -> Result<(), String> {
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let path = directory.path().join("credentials.toml");
    let store = FileCredentialStore::new(&path);
    store
        .set("ops", &access_token("fictional-token"))
        .map_err(|error| error.to_string())?;

    let contents = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    assert!(contents.contains("[credentials.ops]"), "{contents}");
    assert!(contents.contains("kind = \"access_token\""), "{contents}");
    Ok(())
}

#[test]
fn a_malformed_store_is_reported_rather_than_silently_ignored() -> Result<(), String> {
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let path = directory.path().join("credentials.toml");
    fs::write(&path, "this is not toml {{{").map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    let store = FileCredentialStore::new(&path);
    assert!(matches!(store.get("ops"), Err(SecretsError::Malformed)));
    Ok(())
}
