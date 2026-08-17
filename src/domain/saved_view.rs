use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::private_file::write_private_atomic;

pub const SAVED_VIEW_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum SavedViewError {
    #[error("saved-view file could not be read: {0}")]
    Read(String),
    #[error("saved-view file could not be written: {0}")]
    Write(String),
    #[error("saved-view TOML is invalid: {0}")]
    Decode(String),
    #[error("saved-view schema version {0} is unsupported")]
    UnsupportedVersion(u32),
    #[error("saved view name already exists: {0}")]
    DuplicateName(String),
    #[error("saved view does not exist: {0}")]
    MissingName(String),
    #[error("saved view name is invalid")]
    InvalidName,
    #[error("saved view route is not canonical: {0}")]
    InvalidRoute(String),
    #[error("saved view filter field is not registered for route {route}: {field}")]
    InvalidFilterField { route: String, field: String },
    #[error("saved view filter operator is not registered for route {route}: {operator}")]
    InvalidFilterOperator { route: String, operator: String },
    #[error("saved view column is not registered for route {route}: {column}")]
    InvalidColumn { route: String, column: String },
    #[error("saved view column is repeated for route {route}: {column}")]
    DuplicateColumn { route: String, column: String },
    #[error("saved view sort field is not registered for route {route}: {field}")]
    InvalidSortField { route: String, field: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SavedViewsFile {
    pub version: u32,
    pub views: Vec<SavedView>,
}

impl Default for SavedViewsFile {
    fn default() -> Self {
        Self {
            version: SAVED_VIEW_SCHEMA_VERSION,
            views: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SavedView {
    pub name: String,
    pub route: String,
    pub wide_columns: bool,
    pub columns: Vec<String>,
    pub filters: Vec<FilterClause>,
    pub sort: Vec<SortTerm>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FilterClause {
    pub field: String,
    pub operator: FilterOperator,
    pub value: FilterValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FilterOperator {
    Equals,
    NotEquals,
    Contains,
    StartsWith,
    GreaterThan,
    LessThan,
}

impl FilterOperator {
    pub const fn wire_value(&self) -> &'static str {
        match self {
            Self::Equals => "equals",
            Self::NotEquals => "not_equals",
            Self::Contains => "contains",
            Self::StartsWith => "starts_with",
            Self::GreaterThan => "greater_than",
            Self::LessThan => "less_than",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(untagged)]
pub enum FilterValue {
    Boolean(bool),
    Number(i64),
    Text(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SortTerm {
    pub field: String,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ViewRegistry {
    routes: BTreeMap<String, RegisteredRoute>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct RegisteredRoute {
    fields: BTreeSet<String>,
    columns: BTreeSet<String>,
    operators: BTreeMap<String, BTreeSet<FilterOperator>>,
}

impl Ord for FilterOperator {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.wire_value().cmp(other.wire_value())
    }
}

impl PartialOrd for FilterOperator {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl ViewRegistry {
    pub fn new() -> Self {
        Self {
            routes: BTreeMap::new(),
        }
    }

    pub fn register_route(
        &mut self,
        route: impl Into<String>,
        fields: impl IntoIterator<Item = impl Into<String>>,
        columns: impl IntoIterator<Item = impl Into<String>>,
        operators: impl IntoIterator<Item = (impl Into<String>, Vec<FilterOperator>)>,
    ) {
        let operators = operators
            .into_iter()
            .map(|(field, values)| (field.into(), values.into_iter().collect()))
            .collect();
        self.routes.insert(
            route.into(),
            RegisteredRoute {
                fields: fields.into_iter().map(Into::into).collect(),
                columns: columns.into_iter().map(Into::into).collect(),
                operators,
            },
        );
    }

    pub fn standard() -> Self {
        let mut registry = Self::new();
        registry.register_route(
            "devices",
            [
                "id",
                "name",
                "owner",
                "os",
                "path",
                "tag",
                "last_seen",
                "online",
                "approval",
                "key_expiry",
                "version",
                "sharing",
                "posture",
                "route_role",
                "state",
                "source",
                "rx",
                "tx",
            ],
            [
                "id",
                "name",
                "owner",
                "version",
                "last_seen",
                "os",
                "path",
                "tags",
                "online",
                "state",
                "source",
            ],
            device_operators(),
        );
        registry.register_route(
            "users",
            [
                "id",
                "name",
                "role",
                "status",
                "last_seen",
                "state",
                "source",
            ],
            [
                "id",
                "name",
                "role",
                "status",
                "last_seen",
                "state",
                "source",
            ],
            standard_operators(),
        );
        registry.register_route(
            "routes",
            ["id", "cidr", "advertiser", "approval", "state", "source"],
            ["id", "cidr", "advertiser", "approval", "state", "source"],
            standard_operators(),
        );
        registry.register_route(
            "dns",
            ["name", "value", "state", "source"],
            ["name", "value", "state", "source"],
            standard_operators(),
        );
        registry.register_route(
            "access",
            ["id", "state", "severity", "source"],
            ["id", "state", "severity", "source"],
            standard_operators(),
        );
        registry.register_route(
            "credentials",
            [
                "id",
                "type",
                "status",
                "created_at",
                "expires_at",
                "state",
                "source",
            ],
            [
                "id",
                "type",
                "status",
                "created_at",
                "expires_at",
                "state",
                "source",
            ],
            standard_operators(),
        );
        registry.register_route(
            "activity",
            ["id", "time", "action", "actor", "target", "state", "source"],
            ["id", "time", "action", "actor", "target", "state", "source"],
            standard_operators(),
        );
        for route in ["overview", "local", "services"] {
            registry.register_route(
                route,
                ["id", "name", "state", "source", "severity", "protocol"],
                ["id", "name", "state", "source", "severity", "protocol"],
                standard_operators(),
            );
        }
        registry
    }

    fn route(&self, route: &str) -> Result<&RegisteredRoute, SavedViewError> {
        self.routes
            .get(route)
            .ok_or_else(|| SavedViewError::InvalidRoute(route.to_owned()))
    }
}

fn standard_operators() -> Vec<(String, Vec<FilterOperator>)> {
    vec![
        (
            "id".to_owned(),
            vec![FilterOperator::Equals, FilterOperator::Contains],
        ),
        (
            "name".to_owned(),
            vec![FilterOperator::Equals, FilterOperator::Contains],
        ),
        (
            "state".to_owned(),
            vec![FilterOperator::Equals, FilterOperator::NotEquals],
        ),
        (
            "source".to_owned(),
            vec![FilterOperator::Equals, FilterOperator::Contains],
        ),
        (
            "severity".to_owned(),
            vec![FilterOperator::Equals, FilterOperator::NotEquals],
        ),
        ("protocol".to_owned(), vec![FilterOperator::Equals]),
    ]
}

fn device_operators() -> Vec<(String, Vec<FilterOperator>)> {
    let mut operators = standard_operators();
    for field in [
        "owner",
        "os",
        "path",
        "tag",
        "last_seen",
        "online",
        "approval",
        "key_expiry",
        "version",
        "sharing",
        "posture",
        "route_role",
    ] {
        let mut values = vec![
            FilterOperator::Equals,
            FilterOperator::NotEquals,
            FilterOperator::Contains,
        ];
        if matches!(field, "last_seen" | "key_expiry") {
            values.extend([FilterOperator::GreaterThan, FilterOperator::LessThan]);
        }
        operators.push((field.to_owned(), values));
    }
    operators
}

impl Default for ViewRegistry {
    fn default() -> Self {
        Self::standard()
    }
}

impl SavedView {
    pub fn validate(&self, registry: &ViewRegistry) -> Result<(), SavedViewError> {
        validate_name(self.name.as_str())?;
        let route = registry.route(self.route.as_str())?;
        let mut columns = BTreeSet::new();
        for column in &self.columns {
            if !route.columns.contains(column) {
                return Err(SavedViewError::InvalidColumn {
                    route: self.route.clone(),
                    column: column.clone(),
                });
            }
            if !columns.insert(column.as_str()) {
                return Err(SavedViewError::DuplicateColumn {
                    route: self.route.clone(),
                    column: column.clone(),
                });
            }
        }
        for filter in &self.filters {
            if !route.fields.contains(&filter.field) {
                return Err(SavedViewError::InvalidFilterField {
                    route: self.route.clone(),
                    field: filter.field.clone(),
                });
            }
            if !route
                .operators
                .get(&filter.field)
                .is_some_and(|operators| operators.contains(&filter.operator))
            {
                return Err(SavedViewError::InvalidFilterOperator {
                    route: self.route.clone(),
                    operator: filter.operator.wire_value().to_owned(),
                });
            }
        }
        for sort in &self.sort {
            if !route.fields.contains(&sort.field) {
                return Err(SavedViewError::InvalidSortField {
                    route: self.route.clone(),
                    field: sort.field.clone(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SavedViewStore {
    path: PathBuf,
    file: SavedViewsFile,
}

impl SavedViewStore {
    pub fn load(path: impl Into<PathBuf>, registry: &ViewRegistry) -> Result<Self, SavedViewError> {
        let path = path.into();
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(SavedViewError::Read(
                    "saved-view path is not a regular file".to_owned(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(SavedViewError::Read(error.to_string())),
        }
        let file = match fs::read_to_string(&path) {
            Ok(contents) => {
                let file = toml::from_str::<SavedViewsFile>(&contents)
                    .map_err(|error| SavedViewError::Decode(error.to_string()))?;
                if file.version != SAVED_VIEW_SCHEMA_VERSION {
                    return Err(SavedViewError::UnsupportedVersion(file.version));
                }
                for view in &file.views {
                    view.validate(registry)?;
                }
                ensure_unique_names(&file.views)?;
                file
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => SavedViewsFile::default(),
            Err(error) => return Err(SavedViewError::Read(error.to_string())),
        };
        Ok(Self { path, file })
    }

    pub fn file(&self) -> &SavedViewsFile {
        &self.file
    }

    pub fn create(
        &mut self,
        view: SavedView,
        registry: &ViewRegistry,
    ) -> Result<(), SavedViewError> {
        view.validate(registry)?;
        if self.file.views.iter().any(|item| item.name == view.name) {
            return Err(SavedViewError::DuplicateName(view.name));
        }
        self.file.views.push(view);
        self.persist()
    }

    pub fn replace(
        &mut self,
        name: &str,
        replacement: SavedView,
        registry: &ViewRegistry,
    ) -> Result<(), SavedViewError> {
        replacement.validate(registry)?;
        if replacement.name != name
            && self
                .file
                .views
                .iter()
                .any(|item| item.name == replacement.name)
        {
            return Err(SavedViewError::DuplicateName(replacement.name));
        }
        let Some(index) = self.file.views.iter().position(|item| item.name == name) else {
            return Err(SavedViewError::MissingName(name.to_owned()));
        };
        self.file.views[index] = replacement;
        self.persist()
    }

    pub fn rename(&mut self, name: &str, replacement: String) -> Result<(), SavedViewError> {
        validate_name(replacement.as_str())?;
        if self.file.views.iter().any(|item| item.name == replacement) {
            return Err(SavedViewError::DuplicateName(replacement));
        }
        let Some(view) = self.file.views.iter_mut().find(|item| item.name == name) else {
            return Err(SavedViewError::MissingName(name.to_owned()));
        };
        view.name = replacement;
        self.persist()
    }

    pub fn delete(&mut self, name: &str) -> Result<(), SavedViewError> {
        let before = self.file.views.len();
        self.file.views.retain(|view| view.name != name);
        if before == self.file.views.len() {
            return Err(SavedViewError::MissingName(name.to_owned()));
        }
        self.persist()
    }

    pub fn apply(&self, name: &str) -> Result<&SavedView, SavedViewError> {
        self.file
            .views
            .iter()
            .find(|view| view.name == name)
            .ok_or_else(|| SavedViewError::MissingName(name.to_owned()))
    }

    fn persist(&self) -> Result<(), SavedViewError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| SavedViewError::Write("saved-view path has no parent".to_owned()))?;
        fs::create_dir_all(parent).map_err(|error| SavedViewError::Write(error.to_string()))?;
        let serialized = toml::to_string(&self.file)
            .map_err(|error| SavedViewError::Write(error.to_string()))?;
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let temporary = parent.join(format!(
            ".saved-views.{}.{}.tmp",
            std::process::id(),
            suffix
        ));
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(SavedViewError::Write(
                    "saved-view target is not a regular file".to_owned(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(SavedViewError::Write(error.to_string())),
        }
        write_private_atomic(&temporary, &self.path, serialized.as_bytes())
            .map_err(|error| SavedViewError::Write(error.to_string()))?;
        Ok(())
    }
}

fn validate_name(value: &str) -> Result<(), SavedViewError> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        Err(SavedViewError::InvalidName)
    } else {
        Ok(())
    }
}

fn ensure_unique_names(views: &[SavedView]) -> Result<(), SavedViewError> {
    let mut names = BTreeSet::new();
    for view in views {
        if !names.insert(view.name.as_str()) {
            return Err(SavedViewError::DuplicateName(view.name.clone()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(name: &str) -> SavedView {
        SavedView {
            name: name.to_owned(),
            route: "devices".to_owned(),
            wide_columns: false,
            columns: vec!["id".to_owned()],
            filters: vec![FilterClause {
                field: "id".to_owned(),
                operator: FilterOperator::Contains,
                value: FilterValue::Text("device".to_owned()),
            }],
            sort: vec![SortTerm {
                field: "id".to_owned(),
                direction: SortDirection::Ascending,
            }],
        }
    }

    #[test]
    fn saved_views_are_strict_and_atomic() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let directory =
            directory.map_or_else(|_| PathBuf::from("."), |value| value.path().to_path_buf());
        let path = directory.join("saved-views.toml");
        let store_result = SavedViewStore::load(&path, &ViewRegistry::default());
        assert!(store_result.is_ok());
        let Ok(mut store) = store_result else {
            return;
        };
        assert!(
            store
                .create(view("fleet"), &ViewRegistry::default())
                .is_ok()
        );
        assert!(store.apply("fleet").is_ok());
    }
}
