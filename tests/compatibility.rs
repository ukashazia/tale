#[test]
fn unsupported_client_output_is_a_scoped_compatibility_result() {
    let error = tale::local::client::ClientError::UnsupportedOutput {
        operation: "version".to_owned(),
        detail: "required field missing".to_owned(),
    };
    let state = error.state("9.9.9");
    assert!(matches!(
        state,
        tale::domain::source::LocalState::UnsupportedClient { .. }
    ));
}
