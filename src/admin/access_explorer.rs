use sha2::{Digest, Sha256};

use crate::admin::auth::AccessToken;
use crate::admin::client::{AdminClient, AdminError};
use crate::domain::access_explorer::{AccessDecision, AccessQuestion, AccessResult};
use crate::domain::policy_workflow::{PolicyDocument, PolicySelectorType};

impl AdminClient {
    pub async fn ask_access(
        &self,
        token: &AccessToken,
        tailnet: &str,
        question: &AccessQuestion,
        policy: &PolicyDocument,
        requested_at: u64,
    ) -> Result<AccessResult, AdminError> {
        let selector_type = match question.supported_preview_type() {
            Some("user") => PolicySelectorType::User,
            Some("ipport") => PolicySelectorType::IpPort,
            Some(_) | None => {
                return Ok(AccessResult::indeterminate(
                    policy.hash(),
                    question.destination_selector.clone(),
                    requested_at,
                    question.policy_source.clone(),
                    "the documented preview API does not support all requested dimensions",
                ));
            }
        };
        let selector = question
            .preview_input()
            .ok_or_else(|| AdminError::ValidationFailed {
                operation: "access explorer".to_owned(),
                detail: "the question could not be translated to a documented preview input"
                    .to_owned(),
            })?;
        let response = self
            .preview_policy(
                token,
                tailnet,
                selector_type,
                selector.as_str(),
                policy.bytes(),
            )
            .await?;
        let Some(matches) = response.value.matches else {
            return Ok(AccessResult::indeterminate(
                policy_hash(policy.bytes()),
                selector,
                requested_at,
                question.policy_source.clone(),
                "the server returned no match envelope",
            ));
        };
        if matches.is_empty() {
            return Ok(AccessResult::indeterminate(
                policy_hash(policy.bytes()),
                selector,
                requested_at,
                question.policy_source.clone(),
                "an empty preview response is not treated as Denied",
            ));
        }
        let rule_locations = matches
            .iter()
            .filter_map(|value| value.line_number.and_then(|line| u32::try_from(line).ok()))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let matched_users = matches
            .iter()
            .flat_map(|value| value.users.iter().flatten().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let matched_ports = matches
            .iter()
            .flat_map(|value| value.ports.iter().flatten().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut limitations = vec![
            "decision is the server's documented policy-preview result".to_owned(),
            "packet reachability and runtime path are not evaluated".to_owned(),
        ];
        if question.protocol_or_port.is_some() {
            limitations.push(
                "the documented ipport preview input does not evaluate the source selector"
                    .to_owned(),
            );
        }
        Ok(AccessResult {
            decision: AccessDecision::Allowed,
            policy_hash: policy_hash(policy.bytes()),
            input: selector,
            requested_at,
            limitations,
            matched_users,
            matched_ports,
            rule_locations,
            source: question.policy_source.clone(),
        })
    }
}

fn policy_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
