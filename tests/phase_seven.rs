use std::path::Path;

use tale::action::ActionId;
use tale::admin::audit::decode_audit;
use tale::admin::dto::{AuditResponse, KeyDto, PolicyPreviewDto, PolicyValidationDto};
use tale::admin::key_mutations::{
    AuthKeyCreateRequest, RemoteCredentialType, decode_created_auth_key,
};
use tale::admin::policy_mutations::{build_policy_diff, decode_preview, decode_validation};
use tale::domain::activity::AuditFilters;
use tale::domain::policy_workflow::{
    MAX_APPLY_AGE, PolicyDocument, PolicySelectorType, PolicyState, PolicyValidation,
    PolicyWorkflow, ServerPolicyTest, hash_bytes,
};
use tale::domain::secret_result::{SecretBuffer, SecretMetadata, SecretResult};
use tale::temporary::{TemporaryFileError, TemporaryPolicyFile};
use tale::terminal::EditorCommand;

fn fixture(path: &str) -> Vec<u8> {
    let value = match path {
        "valid" => include_bytes!("fixtures/admin/policy/valid.hujson").as_slice(),
        "invalid" => include_bytes!("fixtures/admin/policy/invalid.json").as_slice(),
        "tests" => include_bytes!("fixtures/admin/policy/failing-tests.json").as_slice(),
        "preview" => include_bytes!("fixtures/admin/policy/preview-limited.json").as_slice(),
        "remote-changed" => {
            include_bytes!("fixtures/admin/policy/remote-changed.hujson").as_slice()
        }
        "key" => include_bytes!("fixtures/admin/credentials/auth-key-success.json").as_slice(),
        "missing-key" => {
            include_bytes!("fixtures/admin/credentials/auth-key-no-secret.json").as_slice()
        }
        "audit" => include_bytes!("fixtures/admin/audit/filtered.json").as_slice(),
        _ => &[],
    };
    value.to_vec()
}

#[test]
fn editor_argv_is_direct_and_appends_one_path_argument() {
    let parsed = EditorCommand::parse("nvim --clean 'policy copy.hujson'");
    assert!(parsed.is_ok());
    if let Ok(command) = parsed {
        assert_eq!(command.executable(), Path::new("nvim"));
        let args = command.argv_with_path(Path::new("/tmp/tale policy/policy.hujson"));
        assert_eq!(args.len(), 3);
        assert_eq!(args[0], "--clean");
        assert_eq!(args[1], "policy copy.hujson");
        assert_eq!(args[2], "/tmp/tale policy/policy.hujson");
    }
    for value in [
        "sh -c 'editor $1'",
        "sh -c editor",
        "/bin/bash -c editor",
        "cmd /C editor",
        "powershell -Command editor",
        "editor;cat",
        "editor | cat",
        "editor $(cat policy)",
        "editor `cat policy`",
    ] {
        assert!(
            EditorCommand::parse(value).is_err(),
            "accepted shell syntax: {value}"
        );
    }
}

#[test]
fn phase_seven_action_ids_match_the_specification() {
    assert_eq!(ActionId::AdminPolicyEdit.as_str(), "admin.policy.edit");
    assert_eq!(
        ActionId::AdminPolicyEditorReopen.as_str(),
        "admin.policy.editor.reopen"
    );
    assert_eq!(
        ActionId::AdminPolicyCandidateDiscard.as_str(),
        "admin.policy.candidate.discard"
    );
    assert_eq!(
        ActionId::AdminPolicyRemoteRefresh.as_str(),
        "admin.policy.remote.refresh"
    );
    assert_eq!(
        ActionId::AdminPolicyWorkflowClose.as_str(),
        "admin.policy.workflow.close"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn editor_handoff_reads_after_zero_and_nonzero_direct_child_exit() {
    let file = TemporaryPolicyFile::create(b"// retained\n{}\n");
    assert!(file.is_ok());
    if let Ok(mut file) = file {
        let path = file.path().to_path_buf();
        let success = EditorCommand::parse("/usr/bin/true");
        assert!(success.is_ok());
        if let Ok(success) = success {
            let result = success.run(&path).await;
            assert!(result.is_ok());
            if let Ok(result) = result {
                assert!(result.success);
            }
        }
        let failure = EditorCommand::parse("/usr/bin/false");
        assert!(failure.is_ok());
        if let Ok(failure) = failure {
            let result = failure.run(&path).await;
            assert!(result.is_ok());
            if let Ok(result) = result {
                assert!(!result.success);
            }
        }
        let missing = EditorCommand::parse("/fictional/missing-editor");
        assert!(missing.is_ok());
        if let Ok(missing) = missing {
            assert!(matches!(
                missing.run(&path).await,
                Err(tale::terminal::EditorError::Spawn)
            ));
        }
        assert_eq!(
            file.read_candidate().ok().as_deref(),
            Some(b"// retained\n{}\n".as_slice())
        );
        assert!(file.close().is_ok());
        assert!(!path.exists());
    }
}

#[test]
fn temporary_policy_storage_preserves_exact_bytes_and_is_bounded() {
    let bytes = fixture("valid");
    let file = TemporaryPolicyFile::create(&bytes);
    assert!(file.is_ok());
    if let Ok(mut file) = file {
        assert_eq!(
            file.read_candidate().ok().as_deref(),
            Some(bytes.as_slice())
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(file.path());
            assert!(metadata.is_ok());
            if let Ok(metadata) = metadata {
                assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
            }
        }
        let path = file.path().to_path_buf();
        assert!(file.close().is_ok());
        assert!(!path.exists());
    }
    let oversized = vec![b'x'; tale::domain::policy_workflow::MAX_POLICY_BYTES + 1];
    assert!(matches!(
        TemporaryPolicyFile::create(&oversized),
        Err(TemporaryFileError::TooLarge)
    ));
    let directory = tempfile::tempdir();
    assert!(directory.is_ok());
    if let Ok(directory) = directory {
        assert!(matches!(
            TemporaryPolicyFile::read_candidate_path(directory.path()),
            Err(TemporaryFileError::NotRegular)
        ));
    }
}

#[test]
fn policy_bytes_hash_diff_and_server_results_remain_authoritative() {
    let base_bytes = fixture("valid");
    let mut candidate_bytes = base_bytes.clone();
    candidate_bytes.extend_from_slice(b"\r\n");
    let base = PolicyDocument::from_bytes(base_bytes.clone(), 100);
    let candidate = PolicyDocument::from_bytes(candidate_bytes.clone(), 101);
    assert!(base.is_ok() && candidate.is_ok());
    if let (Ok(base), Ok(candidate)) = (base, candidate) {
        assert_eq!(base.bytes(), base_bytes.as_slice());
        assert_eq!(candidate.bytes(), candidate_bytes.as_slice());
        assert_eq!(candidate.hash(), hash_bytes(&candidate_bytes));
        let diff = build_policy_diff(&base, &candidate);
        assert!(diff.is_ok());
        if let Ok(diff) = diff {
            assert_eq!(diff.base_observed_at, 100);
            assert_eq!(diff.candidate_observed_at, 101);
            assert!(diff.text.contains("remote/base"));
        }

        let invalid_value = serde_json::from_slice::<PolicyValidationDto>(&fixture("invalid"));
        assert!(invalid_value.is_ok());
        let tests_value = serde_json::from_slice::<PolicyValidationDto>(&fixture("tests"));
        assert!(tests_value.is_ok());
        if let (Ok(invalid_value), Ok(tests_value)) = (invalid_value, tests_value) {
            let invalid = decode_validation(invalid_value, &candidate, 110);
            assert!(!invalid.valid);
            assert_eq!(invalid.candidate_hash, candidate.hash());
            assert_eq!(invalid.validated_at, 110);
            assert_eq!(
                invalid.diagnostics.first().and_then(|value| value.line),
                Some(7)
            );
            let failed_tests = decode_validation(tests_value, &candidate, 111);
            assert!(!failed_tests.valid);
            assert_eq!(
                failed_tests
                    .server_tests
                    .first()
                    .map(|value| value.name.as_str()),
                Some("fictional declared SSH test")
            );

            let valid = PolicyValidation {
                candidate_hash: candidate.hash().to_owned(),
                validated_at: 120,
                valid: true,
                message: None,
                bounded_safe_detail: None,
                diagnostics: Vec::new(),
                server_tests: vec![ServerPolicyTest {
                    name: "fictional test".to_owned(),
                    passed: true,
                    message: None,
                }],
                observed_at: 120,
            };
            let preview_value = serde_json::from_slice::<PolicyPreviewDto>(&fixture("preview"));
            assert!(preview_value.is_ok());
            if let Ok(preview_value) = preview_value {
                let preview = decode_preview(
                    preview_value,
                    &candidate,
                    PolicySelectorType::User,
                    "user-fictional-001",
                    121,
                );
                let mut workflow = PolicyWorkflow::opening(
                    1,
                    "fictional-profile".to_owned(),
                    "example.test".to_owned(),
                    100,
                );
                workflow.set_base(base.clone());
                workflow.set_candidate(candidate.clone(), "/tmp/fictional-policy.hujson".into());
                assert!(workflow.set_validation(valid));
                assert!(workflow.set_preview(preview));
                assert_eq!(workflow.state(), PolicyState::ReadyToApply);
                assert!(workflow.apply_guard(122).is_ok());
                assert!(matches!(
                    workflow.apply_guard(120 + MAX_APPLY_AGE.as_secs() + 1),
                    Err(tale::domain::policy_workflow::PolicyApplyGuardError::StaleCandidate)
                        | Err(
                            tale::domain::policy_workflow::PolicyApplyGuardError::StaleValidation
                        )
                ));
                let remote = PolicyDocument::from_bytes(fixture("remote-changed"), 123);
                assert!(remote.is_ok());
                if let Ok(remote) = remote {
                    workflow.set_latest_remote(remote);
                    assert_eq!(workflow.state(), PolicyState::RemoteConflict);
                }
            }
        }
    }
}

#[test]
fn auth_key_request_preserves_all_fields_and_secret_traits_are_safe() {
    let request = AuthKeyCreateRequest {
        description: Some("fictional operator".to_owned()),
        expiry_seconds: 7 * 24 * 60 * 60,
        reusable: true,
        ephemeral: false,
        preauthorized: true,
        tags: vec!["tag:fictional-a".to_owned(), "tag:fictional-b".to_owned()],
    };
    let body = request.json_body();
    assert!(body.is_ok());
    if let Ok(body) = body {
        assert_eq!(body["keyType"], "auth");
        assert_eq!(body["expirySeconds"], 7 * 24 * 60 * 60);
        assert_eq!(body["capabilities"]["devices"]["create"]["reusable"], true);
        assert_eq!(
            body["capabilities"]["devices"]["create"]["ephemeral"],
            false
        );
        assert_eq!(
            body["capabilities"]["devices"]["create"]["preauthorized"],
            true
        );
        assert_eq!(
            body["capabilities"]["devices"]["create"]["tags"][0],
            "tag:fictional-a"
        );
        assert_eq!(
            body["capabilities"]["devices"]["create"]["tags"][1],
            "tag:fictional-b"
        );
    }

    let key = serde_json::from_slice::<KeyDto>(&fixture("key"));
    assert!(key.is_ok());
    if let Ok(key) = key {
        let debug = format!("{key:?}");
        assert!(!debug.contains("fictional-secret-canary"));
        let created = decode_created_auth_key(key, 1);
        assert!(created.is_ok());
        if let Ok(created) = created {
            let debug = format!("{created:?}");
            assert!(!debug.contains("fictional-secret-canary"));
            let result = SecretResult::new(
                SecretMetadata {
                    result_id: 1,
                    credential_id: Some(created.metadata.id.clone()),
                    credential_type: "auth".to_owned(),
                    description: None,
                    created_at: 1,
                    expires_at: None,
                    warning: "fictional warning".to_owned(),
                },
                SecretBuffer::new("fictional-secret-canary"),
            );
            let debug = format!("{result:?}");
            assert!(!debug.contains("fictional-secret-canary"));
            assert!(
                !format!(
                    "{:?}",
                    tale::event::CredentialEvent::AuthKeyCreated {
                        result_id: 1,
                        metadata: created.metadata,
                        secret: created.secret,
                        observed_at: 1,
                    }
                )
                .contains("fictional-secret-canary")
            );
        }
    }
    let missing = serde_json::from_slice::<KeyDto>(&fixture("missing-key"));
    assert!(
        missing.is_ok(),
        "missing-secret fixture failed to decode: {missing:?}"
    );
    if let Ok(missing) = missing {
        let error = decode_created_auth_key(missing, 1);
        assert!(error.is_err());
        if let Err(error) = error {
            assert!(!error.to_string().contains("fictional-secret-canary"));
        }
    }
}

#[test]
fn credential_types_have_explicit_revoke_capabilities() {
    assert!(RemoteCredentialType::AuthKey.supported_for_revoke());
    assert!(RemoteCredentialType::ApiAccessToken.supported_for_revoke());
    assert!(RemoteCredentialType::ClientCredential.supported_for_revoke());
    assert!(!RemoteCredentialType::Federated.supported_for_revoke());
    assert!(!RemoteCredentialType::Unknown.supported_for_revoke());
    assert_eq!(
        RemoteCredentialType::AuthKey.write_scope(),
        Some("auth_keys:write")
    );
    assert_eq!(
        RemoteCredentialType::ApiAccessToken.read_scope(),
        Some("api_access_tokens:read")
    );
}

#[test]
fn audit_values_omit_unknown_secret_fields_and_filters_are_inclusive() {
    let response = serde_json::from_slice::<AuditResponse>(&fixture("audit"));
    assert!(response.is_ok());
    if let Ok(response) = response {
        let snapshot = decode_audit(response.logs, 1);
        assert!(snapshot.is_ok());
        if let Ok(snapshot) = snapshot {
            let first = snapshot.events.first();
            assert!(first.is_some());
            if let Some(first) = first {
                let old = first
                    .old
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                let new = first
                    .new
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                assert!(old.contains("fictional-old"));
                assert!(new.contains("fictional-new"));
                assert!(!old.contains("fictional-private-canary"));
                assert!(!new.contains("fictional-secret-canary"));
            }
            let filters = AuditFilters {
                start: Some(1_785_801_600),
                end: Some(1_785_801_600),
                actor_id: Some("user-fictional-001".to_owned()),
                actor_display: None,
                action: Some("policy.update".to_owned()),
                target_type: Some("policy".to_owned()),
                target_id: Some("policy-fictional-001".to_owned()),
                text: Some("policy source".to_owned()),
            };
            assert_eq!(snapshot.filtered_events(&filters).len(), 1);
        }
    }
}
