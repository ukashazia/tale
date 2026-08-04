use std::fmt;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::admin::auth::AccessToken;
use crate::admin::client::{AdminClient, AdminError, ApiResponse, Endpoint, MutationResponse};
use crate::admin::dto::{LogstreamConfigurationDto, LogstreamStatusDto, SettingsDto};
use crate::domain::log_stream::{
    LogStreamConfiguration, LogStreamDestination, LogStreamStatus, LogType,
};
use crate::domain::secret_result::SecretBuffer;

pub struct LogStreamReplacement {
    pub log_type: LogType,
    pub destination_type: String,
    pub url: String,
    pub user: Option<String>,
    pub upload_period_minutes: Option<u64>,
    pub compression_format: Option<String>,
    pub token: Option<SecretBuffer>,
    pub s3_bucket: Option<String>,
    pub s3_region: Option<String>,
    pub s3_key_prefix: Option<String>,
    pub s3_authentication_type: Option<String>,
    pub s3_access_key_id: Option<String>,
    pub s3_role_arn: Option<String>,
    pub gcs_bucket: Option<String>,
    pub gcs_key_prefix: Option<String>,
    pub gcs_scopes: Vec<String>,
    pub gcs_credentials: Option<SecretBuffer>,
}

pub fn is_supported_destination(value: &str) -> bool {
    matches!(
        value,
        "splunk" | "elastic" | "panther" | "cribl" | "datadog" | "axiom" | "s3" | "gcs"
    )
}

impl fmt::Debug for LogStreamReplacement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LogStreamReplacement")
            .field("log_type", &self.log_type)
            .field("destination_type", &self.destination_type)
            .field("url", &self.url)
            .field("user", &self.user)
            .field("upload_period_minutes", &self.upload_period_minutes)
            .field("compression_format", &self.compression_format)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .field("s3_bucket", &self.s3_bucket)
            .field("s3_region", &self.s3_region)
            .field("s3_key_prefix", &self.s3_key_prefix)
            .field("s3_authentication_type", &self.s3_authentication_type)
            .field("s3_access_key_id", &self.s3_access_key_id)
            .field("s3_role_arn", &self.s3_role_arn)
            .field("gcs_bucket", &self.gcs_bucket)
            .field("gcs_key_prefix", &self.gcs_key_prefix)
            .field("gcs_scopes", &self.gcs_scopes)
            .field(
                "gcs_credentials",
                &self.gcs_credentials.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

impl LogStreamReplacement {
    pub fn validate(&self) -> Result<(), AdminError> {
        if !is_supported_destination(self.destination_type.as_str()) {
            return Err(validation(
                "destination kind is not fully described by the adopted log-stream contract",
            ));
        }
        if self.destination_type.trim().is_empty() {
            return Err(validation("destination type is required"));
        }
        if [
            self.destination_type.as_str(),
            self.url.as_str(),
            self.user.as_deref().unwrap_or_default(),
            self.compression_format.as_deref().unwrap_or_default(),
            self.s3_bucket.as_deref().unwrap_or_default(),
            self.s3_region.as_deref().unwrap_or_default(),
            self.s3_key_prefix.as_deref().unwrap_or_default(),
            self.s3_authentication_type.as_deref().unwrap_or_default(),
            self.s3_access_key_id.as_deref().unwrap_or_default(),
            self.s3_role_arn.as_deref().unwrap_or_default(),
            self.gcs_bucket.as_deref().unwrap_or_default(),
            self.gcs_key_prefix.as_deref().unwrap_or_default(),
        ]
        .iter()
        .any(|value| value.chars().any(char::is_control))
            || self
                .gcs_scopes
                .iter()
                .any(|value| value.chars().any(char::is_control))
        {
            return Err(validation("destination fields contain a control character"));
        }
        if self
            .upload_period_minutes
            .is_some_and(|value| value > 1_440)
        {
            return Err(validation(
                "upload period exceeds the documented 1440-minute maximum",
            ));
        }
        match self.destination_type.as_str() {
            "s3" => {
                if self.s3_bucket.as_deref().is_none_or(str::is_empty)
                    || self.s3_region.as_deref().is_none_or(str::is_empty)
                    || self
                        .s3_authentication_type
                        .as_deref()
                        .is_none_or(str::is_empty)
                {
                    return Err(validation(
                        "s3 replacement requires bucket, region, and authentication type",
                    ));
                }
                match self.s3_authentication_type.as_deref() {
                    Some("accesskey") if self.s3_access_key_id.is_none() => {
                        return Err(validation(
                            "s3 accesskey authentication requires an access key",
                        ));
                    }
                    Some("accesskey") if self.token.is_none() => {
                        return Err(validation(
                            "s3 accesskey authentication requires an explicit secret replacement",
                        ));
                    }
                    Some("rolearn") if self.s3_role_arn.is_none() => {
                        return Err(validation("s3 rolearn authentication requires a role ARN"));
                    }
                    Some("accesskey" | "rolearn") => {}
                    Some(_) | None => {
                        return Err(validation(
                            "s3 authentication type must be accesskey or rolearn",
                        ));
                    }
                }
            }
            "gcs" => {
                if self.gcs_bucket.as_deref().is_none_or(str::is_empty) {
                    return Err(validation("gcs replacement requires a bucket"));
                }
                if self.gcs_credentials.is_none() {
                    return Err(validation(
                        "gcs replacement requires an explicit credentials replacement",
                    ));
                }
            }
            _ => {
                if self.url.trim().is_empty() {
                    return Err(validation("destination URL is required"));
                }
            }
        }
        Ok(())
    }

    fn json_body(&self) -> Result<serde_json::Value, AdminError> {
        self.validate()?;
        let mut body = serde_json::Map::new();
        body.insert(
            "destinationType".to_owned(),
            serde_json::Value::String(self.destination_type.clone()),
        );
        if !matches!(self.destination_type.as_str(), "s3" | "gcs") {
            body.insert(
                "url".to_owned(),
                serde_json::Value::String(self.url.clone()),
            );
        }
        insert_optional(
            &mut body,
            "user",
            self.user.clone().map(serde_json::Value::String),
        );
        insert_optional(
            &mut body,
            "uploadPeriodMinutes",
            self.upload_period_minutes.map(serde_json::Value::from),
        );
        insert_optional(
            &mut body,
            "compressionFormat",
            self.compression_format
                .clone()
                .map(serde_json::Value::String),
        );
        if let Some(token) = &self.token {
            let value = token
                .as_str()
                .ok_or_else(|| validation("replacement token is not valid UTF-8"))?;
            let field = if self.destination_type == "s3" {
                "s3SecretAccessKey"
            } else {
                "token"
            };
            body.insert(
                field.to_owned(),
                serde_json::Value::String(value.to_owned()),
            );
        }
        insert_optional(
            &mut body,
            "s3Bucket",
            self.s3_bucket.clone().map(serde_json::Value::String),
        );
        insert_optional(
            &mut body,
            "s3Region",
            self.s3_region.clone().map(serde_json::Value::String),
        );
        insert_optional(
            &mut body,
            "s3KeyPrefix",
            self.s3_key_prefix.clone().map(serde_json::Value::String),
        );
        insert_optional(
            &mut body,
            "s3AuthenticationType",
            self.s3_authentication_type
                .clone()
                .map(serde_json::Value::String),
        );
        insert_optional(
            &mut body,
            "s3AccessKeyId",
            self.s3_access_key_id.clone().map(serde_json::Value::String),
        );
        insert_optional(
            &mut body,
            "s3RoleArn",
            self.s3_role_arn.clone().map(serde_json::Value::String),
        );
        insert_optional(
            &mut body,
            "gcsBucket",
            self.gcs_bucket.clone().map(serde_json::Value::String),
        );
        insert_optional(
            &mut body,
            "gcsKeyPrefix",
            self.gcs_key_prefix.clone().map(serde_json::Value::String),
        );
        if !self.gcs_scopes.is_empty() {
            body.insert(
                "gcsScopes".to_owned(),
                serde_json::Value::Array(
                    self.gcs_scopes
                        .iter()
                        .cloned()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
        }
        insert_optional(
            &mut body,
            "gcsCredentials",
            match self.gcs_credentials.as_ref() {
                Some(secret) => Some(serde_json::Value::String(
                    secret
                        .as_str()
                        .ok_or_else(|| validation("GCS credentials are not valid UTF-8"))?
                        .to_owned(),
                )),
                None => None,
            },
        );
        Ok(serde_json::Value::Object(body))
    }
}

fn insert_optional(
    body: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<serde_json::Value>,
) {
    if let Some(value) = value {
        body.insert(key.to_owned(), value);
    }
}

fn validation(detail: &str) -> AdminError {
    AdminError::ValidationFailed {
        operation: "log-stream configuration".to_owned(),
        detail: detail.to_owned(),
    }
}

impl AdminClient {
    pub async fn get_log_stream_configuration(
        &self,
        token: &AccessToken,
        tailnet: &str,
        log_type: LogType,
    ) -> Result<ApiResponse<LogStreamConfiguration>, AdminError> {
        let response = self
            .json::<LogstreamConfigurationDto>(
                Endpoint::LogStreamConfiguration,
                token,
                self.path(
                    &[
                        "tailnet",
                        tailnet,
                        "logging",
                        log_type.wire_value(),
                        "stream",
                    ],
                    &[],
                )?,
                Some("application/json"),
            )
            .await?;
        let value = configuration_from_dto(response.value, response.meta.observed_at, tailnet)?;
        Ok(ApiResponse {
            value,
            meta: response.meta,
        })
    }

    pub async fn replace_log_stream_configuration(
        &self,
        token: &AccessToken,
        tailnet: &str,
        replacement: &LogStreamReplacement,
    ) -> Result<MutationResponse<()>, AdminError> {
        let body = replacement.json_body()?;
        self.mutation_empty(
            Endpoint::LogStreamConfigurationReplace,
            reqwest::Method::PUT,
            token,
            self.path(
                &[
                    "tailnet",
                    tailnet,
                    "logging",
                    replacement.log_type.wire_value(),
                    "stream",
                ],
                &[],
            )?,
            Some(body),
        )
        .await
    }

    pub async fn delete_log_stream_configuration(
        &self,
        token: &AccessToken,
        tailnet: &str,
        log_type: LogType,
    ) -> Result<MutationResponse<()>, AdminError> {
        self.mutation_empty(
            Endpoint::LogStreamConfigurationDelete,
            reqwest::Method::DELETE,
            token,
            self.path(
                &[
                    "tailnet",
                    tailnet,
                    "logging",
                    log_type.wire_value(),
                    "stream",
                ],
                &[],
            )?,
            None,
        )
        .await
    }

    pub async fn get_log_stream_status(
        &self,
        token: &AccessToken,
        tailnet: &str,
        log_type: LogType,
    ) -> Result<ApiResponse<LogStreamStatus>, AdminError> {
        let response = self
            .json::<LogstreamStatusDto>(
                Endpoint::LogStreamStatus,
                token,
                self.path(
                    &[
                        "tailnet",
                        tailnet,
                        "logging",
                        log_type.wire_value(),
                        "status",
                    ],
                    &[],
                )?,
                Some("application/json"),
            )
            .await?;
        let value = status_from_dto(response.value, response.meta.observed_at, tailnet, log_type)?;
        Ok(ApiResponse {
            value,
            meta: response.meta,
        })
    }

    pub async fn get_network_log_setting(
        &self,
        token: &AccessToken,
        tailnet: &str,
    ) -> Result<ApiResponse<SettingsDto>, AdminError> {
        self.json(
            Endpoint::NetworkLogSettings,
            token,
            self.path(&["tailnet", tailnet, "settings"], &[])?,
            Some("application/json"),
        )
        .await
    }

    pub async fn set_network_log_setting(
        &self,
        token: &AccessToken,
        tailnet: &str,
        enabled: bool,
    ) -> Result<MutationResponse<SettingsDto>, AdminError> {
        self.mutation_json(
            Endpoint::NetworkLogSettingsUpdate,
            reqwest::Method::PATCH,
            token,
            self.path(&["tailnet", tailnet, "settings"], &[])?,
            Some(serde_json::json!({"networkFlowLoggingOn": enabled})),
        )
        .await
    }
}

fn configuration_from_dto(
    value: LogstreamConfigurationDto,
    observed_at: u64,
    source_id: &str,
) -> Result<LogStreamConfiguration, AdminError> {
    if value.gcs_credentials.is_some() {
        return Err(AdminError::DecodeFailed {
            operation: Endpoint::LogStreamConfiguration.operation().to_owned(),
            detail: "log-stream configuration returned write-only secret material".to_owned(),
        });
    }
    let log_type = parse_log_type(value.log_type.as_deref())?;
    let kind = value
        .destination_type
        .ok_or_else(|| validation("destination type was not returned"))?;
    let identity = value
        .url
        .or(value.s3_bucket)
        .or(value.gcs_bucket)
        .unwrap_or_else(|| "<destination identity not returned>".to_owned());
    Ok(LogStreamConfiguration {
        log_type,
        enabled: true,
        destination: LogStreamDestination { kind, identity },
        secret_action: crate::domain::log_stream::SecretAction::KeepExisting,
        observed_at,
        source_id: source_id.to_owned(),
    })
}

fn status_from_dto(
    value: LogstreamStatusDto,
    _observed_at: u64,
    source_id: &str,
    log_type: LogType,
) -> Result<LogStreamStatus, AdminError> {
    let last_observation = value
        .last_activity
        .as_deref()
        .map(parse_status_timestamp)
        .transpose()?;
    let healthy = value.last_error.as_deref().is_none_or(str::is_empty);
    let status = value.last_error.map_or_else(
        || "publishing observed".to_owned(),
        |value| {
            if value.is_empty() {
                "publishing observed".to_owned()
            } else {
                value
            }
        },
    );
    Ok(LogStreamStatus {
        log_type,
        configured: true,
        healthy: Some(healthy),
        status,
        last_observation,
        source_id: source_id.to_owned(),
    })
}

fn parse_status_timestamp(value: &str) -> Result<u64, AdminError> {
    let timestamp =
        OffsetDateTime::parse(value, &Rfc3339).map_err(|_| AdminError::DecodeFailed {
            operation: Endpoint::LogStreamStatus.operation().to_owned(),
            detail: "log-stream status returned an invalid lastActivity timestamp".to_owned(),
        })?;
    u64::try_from(timestamp.unix_timestamp()).map_err(|_| AdminError::DecodeFailed {
        operation: Endpoint::LogStreamStatus.operation().to_owned(),
        detail: "log-stream status returned a pre-epoch lastActivity timestamp".to_owned(),
    })
}

fn parse_log_type(value: Option<&str>) -> Result<LogType, AdminError> {
    match value {
        Some("configuration") => Ok(LogType::Configuration),
        Some("network") => Ok(LogType::Network),
        Some(other) => Err(AdminError::Unsupported {
            operation: Endpoint::LogStreamConfiguration.operation().to_owned(),
            detail: format!("unsupported documented log type {other}"),
        }),
        None => Err(validation("log type was not returned")),
    }
}
