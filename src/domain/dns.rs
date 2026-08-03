use super::Timestamp;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AdminNameservers {
    pub values: Vec<String>,
    pub observed_at: Timestamp,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AdminDnsPreferences {
    pub magic_dns: Option<bool>,
    pub observed_at: Timestamp,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AdminSearchPaths {
    pub values: Vec<String>,
    pub observed_at: Timestamp,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AdminSplitDns {
    pub entries: Vec<(String, Option<Vec<String>>)>,
    pub observed_at: Timestamp,
}

impl AdminSplitDns {
    pub fn iter(&self) -> impl Iterator<Item = (&str, Option<&[String]>)> {
        self.entries
            .iter()
            .map(|(domain, resolvers)| (domain.as_str(), resolvers.as_deref()))
    }
}
