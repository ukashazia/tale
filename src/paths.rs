use std::env;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Platform {
    Unix,
    Windows,
}

#[derive(Debug, Clone)]
pub struct PathEnvironment {
    pub platform: Platform,
    pub current_dir: PathBuf,
    pub xdg_config_home: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub xdg_state_home: Option<PathBuf>,
    pub xdg_cache_home: Option<PathBuf>,
    pub appdata: Option<PathBuf>,
    pub localappdata: Option<PathBuf>,
}

impl PathEnvironment {
    pub fn from_process() -> Result<Self, PathError> {
        let current_dir = env::current_dir().map_err(PathError::CurrentDirectory)?;
        let value = |name: &str| env::var_os(name).map(PathBuf::from);
        let platform = if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Unix
        };

        Ok(Self {
            platform,
            current_dir,
            xdg_config_home: value("XDG_CONFIG_HOME"),
            home: value("HOME"),
            xdg_state_home: value("XDG_STATE_HOME"),
            xdg_cache_home: value("XDG_CACHE_HOME"),
            appdata: value("APPDATA"),
            localappdata: value("LOCALAPPDATA"),
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Paths {
    pub config_file: PathBuf,
    /// Secret material, kept beside the configuration but in its own owner-only file so
    /// the configuration itself stays shareable.
    pub credentials_file: PathBuf,
    pub state_dir: PathBuf,
    pub cache_dir: PathBuf,
}

#[derive(Debug, Error)]
pub enum PathError {
    #[error("could not determine the current directory")]
    CurrentDirectory(#[source] std::io::Error),
    #[error("required environment path {0} is not set")]
    MissingEnvironment(&'static str),
}

pub fn resolve_paths(environment: &PathEnvironment) -> Result<Paths, PathError> {
    match environment.platform {
        Platform::Unix => {
            let config_root = environment
                .xdg_config_home
                .clone()
                .or_else(|| environment.home.as_ref().map(|home| home.join(".config")))
                .ok_or(PathError::MissingEnvironment("HOME"))?;
            let state_root = environment
                .xdg_state_home
                .clone()
                .or_else(|| {
                    environment
                        .home
                        .as_ref()
                        .map(|home| home.join(".local/state"))
                })
                .ok_or(PathError::MissingEnvironment("HOME"))?;
            let cache_root = environment
                .xdg_cache_home
                .clone()
                .or_else(|| environment.home.as_ref().map(|home| home.join(".cache")))
                .ok_or(PathError::MissingEnvironment("HOME"))?;

            Ok(Paths {
                config_file: lexical_absolute(
                    &config_root.join("tale/config.toml"),
                    &environment.current_dir,
                ),
                credentials_file: lexical_absolute(
                    &config_root.join("tale/credentials.toml"),
                    &environment.current_dir,
                ),
                state_dir: lexical_absolute(&state_root.join("tale"), &environment.current_dir),
                cache_dir: lexical_absolute(&cache_root.join("tale"), &environment.current_dir),
            })
        }
        Platform::Windows => {
            let appdata = environment
                .appdata
                .clone()
                .ok_or(PathError::MissingEnvironment("APPDATA"))?;
            let localappdata = environment
                .localappdata
                .clone()
                .ok_or(PathError::MissingEnvironment("LOCALAPPDATA"))?;

            Ok(Paths {
                config_file: lexical_absolute(
                    &appdata.join("tale/config.toml"),
                    &environment.current_dir,
                ),
                credentials_file: lexical_absolute(
                    &appdata.join("tale/credentials.toml"),
                    &environment.current_dir,
                ),
                state_dir: lexical_absolute(&localappdata.join("tale"), &environment.current_dir),
                cache_dir: lexical_absolute(
                    &localappdata.join("tale/cache"),
                    &environment.current_dir,
                ),
            })
        }
    }
}

/// Relocating the configuration moves the credential file with it, so a `--config` in a
/// throwaway directory does not silently read or write secrets in the real one.
pub fn with_config_file(mut paths: Paths, config_file: &Path, current_dir: &Path) -> Paths {
    paths.config_file = lexical_absolute(config_file, current_dir);
    paths.credentials_file = paths.config_file.parent().map_or_else(
        || PathBuf::from("credentials.toml"),
        |parent| parent.join("credentials.toml"),
    );
    paths
}

pub fn lexical_absolute(path: &Path, current_dir: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        current_dir.join(path)
    };

    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Expands the current user's home shorthand without otherwise interpreting
/// the path. Relative paths stay relative, and `~user` is deliberately not an
/// account lookup.
pub fn expand_home(path: &Path, home: Option<&Path>) -> Result<PathBuf, PathError> {
    let Ok(relative) = path.strip_prefix("~") else {
        return Ok(path.to_path_buf());
    };
    let home = home.ok_or(PathError::MissingEnvironment("HOME"))?;
    Ok(home.join(relative))
}

pub fn expand_process_home(path: &Path) -> Result<PathBuf, PathError> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    expand_home(path, home.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_expansion_is_narrow_and_preserves_other_paths() {
        let home = Path::new("/home/alice");
        assert_eq!(
            expand_home(Path::new("~/documents/report.pdf"), Some(home)).ok(),
            Some(PathBuf::from("/home/alice/documents/report.pdf"))
        );
        assert_eq!(
            expand_home(Path::new("~"), Some(home)).ok(),
            Some(PathBuf::from("/home/alice"))
        );
        assert_eq!(
            expand_home(Path::new("relative/report.pdf"), Some(home)).ok(),
            Some(PathBuf::from("relative/report.pdf"))
        );
        assert_eq!(
            expand_home(Path::new("~someone/report.pdf"), Some(home)).ok(),
            Some(PathBuf::from("~someone/report.pdf"))
        );
    }

    #[test]
    fn home_is_required_only_when_shorthand_is_used() {
        assert!(matches!(
            expand_home(Path::new("~/report.pdf"), None),
            Err(PathError::MissingEnvironment("HOME"))
        ));
        assert_eq!(
            expand_home(Path::new("report.pdf"), None).ok(),
            Some(PathBuf::from("report.pdf"))
        );
    }
}
