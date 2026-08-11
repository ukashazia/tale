#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum LocalSection {
    #[default]
    Client,
    Accounts,
}

impl LocalSection {
    pub const ALL: [Self; 2] = [Self::Client, Self::Accounts];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Client => "Client",
            Self::Accounts => "Accounts",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LocalAccount {
    pub id: String,
    pub tailnet_name: Option<String>,
    pub account_name: Option<String>,
    pub display_name: Option<String>,
    pub profile_name: Option<String>,
    pub active: bool,
}

impl LocalAccount {
    pub fn display_label(&self) -> &str {
        match self
            .display_name
            .as_deref()
            .or(self.profile_name.as_deref())
            .or(self.account_name.as_deref())
            .or(self.tailnet_name.as_deref())
        {
            Some(value) => value,
            None => &self.id,
        }
    }

    pub fn target_label(&self) -> String {
        format!("{} ({})", self.display_label(), self.id)
    }
}

pub fn deduplicate_accounts(accounts: &mut Vec<LocalAccount>) {
    accounts.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
            .then_with(|| left.display_label().cmp(right.display_label()))
            .then_with(|| left.id.cmp(&right.id))
    });
    accounts.dedup_by(|left, right| left.id == right.id);
}
