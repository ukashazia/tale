use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CertificateRequest {
    pub domain: String,
    pub certificate_path: PathBuf,
    pub key_path: PathBuf,
    pub min_validity: Option<String>,
    pub overwrites_existing: bool,
}

impl CertificateRequest {
    pub fn validate(&self, eligible_domains: &[String]) -> Result<(), String> {
        if !eligible_domains.iter().any(|domain| domain == &self.domain) {
            return Err(
                "certificate domain is not an eligible local certificate domain".to_owned(),
            );
        }
        validate_output_path(&self.certificate_path, "certificate")?;
        validate_output_path(&self.key_path, "key")?;
        if self.certificate_path == self.key_path {
            return Err("certificate and key paths must be different".to_owned());
        }
        if let Some(value) = self.min_validity.as_deref()
            && (value.is_empty() || value.chars().any(char::is_control))
        {
            return Err("minimum validity must be a non-empty duration".to_owned());
        }
        Ok(())
    }
}

pub fn validate_output_path(path: &Path, label: &str) -> Result<(), String> {
    if path.as_os_str().is_empty() || path.as_os_str() == "-" {
        return Err(format!(
            "{label} output path must be explicit and cannot be '-'"
        ));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let metadata = std::fs::metadata(parent)
        .map_err(|_| format!("{label} output parent directory does not exist"))?;
    if !metadata.is_dir() {
        return Err(format!("{label} output parent is not a directory"));
    }
    if path.exists() {
        let existing = std::fs::metadata(path)
            .map_err(|_| format!("{label} output file cannot be inspected"))?;
        if !existing.is_file() {
            return Err(format!("{label} output path is not a regular file"));
        }
    }
    let probe = parent.join(".tale-certificate-write-check");
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(probe);
            Ok(())
        }
        Err(_) => Err(format!("{label} output parent directory is not writable")),
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BugReportRequest {
    pub note: Option<String>,
    pub diagnose: bool,
}

impl BugReportRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.note.as_deref().is_some_and(|note| {
            note.chars()
                .any(|character| character.is_control() && character != '\n' && character != '\t')
        }) {
            return Err("bug-report note may contain only text, newline, and tab".to_owned());
        }
        Ok(())
    }
}
