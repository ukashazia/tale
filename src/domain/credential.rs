use super::Timestamp;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CredentialMetadata {
    pub id: String,
    pub key_type: String,
    pub created_at: Option<Timestamp>,
    pub updated_at: Option<Timestamp>,
    pub expires_at: Option<Timestamp>,
    pub revoked_at: Option<Timestamp>,
    pub last_used_at: Option<Timestamp>,
    pub scopes: Vec<String>,
    pub tags: Vec<String>,
    pub description: Option<String>,
    pub invalid: Option<bool>,
    pub user_id: Option<String>,
    pub capability_summary: Vec<String>,
    pub known_dependents: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CredentialSnapshot {
    pub records: Vec<CredentialMetadata>,
    pub partial: bool,
    pub partial_reason: Option<String>,
    pub observed_at: Timestamp,
}
