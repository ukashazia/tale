use tale::admin::auth::SecretValue;
use tale::domain::secret_result::{SecretBuffer, SecretMetadata, SecretResult};

#[test]
fn secret_canaries_are_redacted_and_view_once_results_close() {
    let canary = "fictional-secret-canary";
    let credential = SecretValue::new(canary);
    assert!(!format!("{credential:?}").contains(canary));

    let metadata = SecretMetadata {
        result_id: 1,
        credential_id: Some("fictional-id".to_owned()),
        credential_type: "auth key".to_owned(),
        description: Some("one-time result".to_owned()),
        created_at: 1,
        expires_at: None,
        warning: "copy once".to_owned(),
    };
    let mut result = SecretResult::new(metadata, SecretBuffer::new(canary));
    assert!(!format!("{result:?}").contains(canary));
    result.close();
    assert!(result.is_closed());
}
