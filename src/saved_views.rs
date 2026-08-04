use std::path::{Path, PathBuf};

use crate::domain::saved_view::{SavedView, SavedViewError, SavedViewStore, ViewRegistry};

#[derive(Debug, Clone)]
pub struct SavedViewsState {
    pub path: PathBuf,
    pub store: SavedViewStore,
    pub registry: ViewRegistry,
}

impl SavedViewsState {
    pub fn load(state_dir: &Path) -> Result<Self, SavedViewError> {
        let registry = ViewRegistry::default();
        let path = state_dir.join("saved-views.toml");
        let store = SavedViewStore::load(&path, &registry)?;
        Ok(Self {
            path,
            store,
            registry,
        })
    }

    pub fn names(&self) -> Vec<String> {
        self.store
            .file()
            .views
            .iter()
            .map(|view| view.name.clone())
            .collect()
    }

    pub fn apply(&self, name: &str) -> Result<&SavedView, SavedViewError> {
        self.store.apply(name)
    }
}
