use super::Timestamp;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AdminUser {
    pub id: String,
    pub display_name: Option<String>,
    pub login_name: Option<String>,
    pub tailnet_id: Option<String>,
    pub created_at: Option<Timestamp>,
    pub relation_type: Option<String>,
    pub role: Option<String>,
    pub status: Option<String>,
    pub device_count: Option<u64>,
    pub last_seen: Option<Timestamp>,
    pub currently_connected: Option<bool>,
}

impl AdminUser {
    pub fn label(&self) -> &str {
        match (self.display_name.as_deref(), self.login_name.as_deref()) {
            (Some(display), _) => display,
            (None, Some(login)) => login,
            (None, None) => &self.id,
        }
    }
}
