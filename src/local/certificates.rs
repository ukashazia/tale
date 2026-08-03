use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::domain::service::CertificateVerification;
use crate::domain::{Timestamp, certificate::CertificateRequest};
use crate::local::process::{LocalCommand, LocalOperation};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CertificateError {
    pub detail: String,
}

impl CertificateError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for CertificateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for CertificateError {}

pub fn certificate_command(
    path: &Path,
    timeout: Duration,
    request: &CertificateRequest,
) -> Result<LocalCommand, CertificateError> {
    if request.domain.is_empty() || request.domain.chars().any(char::is_control) {
        return Err(CertificateError::new("certificate domain is invalid"));
    }
    if request.certificate_path == request.key_path {
        return Err(CertificateError::new(
            "certificate and key paths must be different",
        ));
    }
    if request.certificate_path.as_os_str() == "-" || request.key_path.as_os_str() == "-" {
        return Err(CertificateError::new(
            "certificate and key paths cannot be '-'",
        ));
    }
    if request
        .min_validity
        .as_deref()
        .is_some_and(|value| value.is_empty() || value.chars().any(char::is_control))
    {
        return Err(CertificateError::new("minimum validity is invalid"));
    }
    let mut args = vec![
        OsString::from("cert"),
        OsString::from(format!(
            "--cert-file={}",
            request.certificate_path.display()
        )),
        OsString::from(format!("--key-file={}", request.key_path.display())),
    ];
    if let Some(min_validity) = request.min_validity.as_deref() {
        args.push(OsString::from(format!("--min-validity={min_validity}")));
    }
    args.push(OsString::from(request.domain.clone()));
    Ok(LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::Certificate,
        args,
    )
    .with_timeout(timeout))
}

pub fn verify_certificate_outputs(
    request: &CertificateRequest,
    _observed_at: Timestamp,
) -> Result<CertificateVerification, CertificateError> {
    let certificate = file_size(&request.certificate_path, "certificate")?;
    let key = file_size(&request.key_path, "key")?;
    Ok(CertificateVerification {
        domain: request.domain.clone(),
        certificate_path: request.certificate_path.clone(),
        key_path: request.key_path.clone(),
        certificate_size: certificate,
        key_size: key,
    })
}

fn file_size(path: &Path, label: &str) -> Result<u64, CertificateError> {
    let metadata = fs::metadata(path)
        .map_err(|_| CertificateError::new(format!("{label} output was not created")))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(CertificateError::new(format!(
            "{label} output is not a non-empty regular file"
        )));
    }
    Ok(metadata.len())
}
