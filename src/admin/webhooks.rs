use crate::admin::auth::AccessToken;
use crate::admin::client::{
    AdminClient, AdminError, ApiResponse, Endpoint, MutationResponse, ResponseMeta,
};
use crate::admin::dto::{WebhookDto, WebhooksResponseDto};
use crate::domain::secret_result::SecretBuffer;
use crate::domain::webhook::{DestinationType, SubscriptionSet, WebhookEndpoint, WebhookError};

pub struct WebhookApiResult {
    pub endpoint: WebhookEndpoint,
    pub secret: Option<SecretBuffer>,
    pub meta: ResponseMeta,
}

impl std::fmt::Debug for WebhookApiResult {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WebhookApiResult")
            .field("endpoint", &self.endpoint)
            .field("secret", &self.secret.as_ref().map(|_| "<redacted>"))
            .field("meta", &self.meta)
            .finish()
    }
}

impl AdminClient {
    pub async fn list_webhooks(
        &self,
        token: &AccessToken,
        tailnet: &str,
    ) -> Result<ApiResponse<Vec<WebhookEndpoint>>, AdminError> {
        let response = self
            .json::<WebhooksResponseDto>(
                Endpoint::Webhooks,
                token,
                self.path(&["tailnet", tailnet, "webhooks"], &[])?,
                Some("application/json"),
            )
            .await?;
        let values = response
            .value
            .webhooks
            .ok_or_else(|| missing_collection(Endpoint::Webhooks))?;
        let endpoints = values
            .into_iter()
            .map(|value| endpoint_from_dto(value, response.meta.observed_at, tailnet, false))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ApiResponse {
            value: endpoints,
            meta: response.meta,
        })
    }

    pub async fn get_webhook(
        &self,
        token: &AccessToken,
        endpoint_id: &str,
    ) -> Result<WebhookApiResult, AdminError> {
        let response = self
            .json::<WebhookDto>(
                Endpoint::Webhook,
                token,
                self.path(&["webhooks", endpoint_id], &[])?,
                Some("application/json"),
            )
            .await?;
        endpoint_result(response, "webhook", false, Endpoint::Webhook)
    }

    pub async fn create_webhook(
        &self,
        token: &AccessToken,
        tailnet: &str,
        endpoint_url: &str,
        provider_type: &DestinationType,
        subscriptions: &SubscriptionSet,
    ) -> Result<WebhookApiResult, AdminError> {
        crate::domain::webhook::WebhookDraft {
            endpoint_url: endpoint_url.to_owned(),
            destination_type: provider_type.clone(),
            subscriptions: subscriptions.clone(),
        }
        .validate()
        .map_err(webhook_validation_error)?;
        let body = serde_json::json!({
            "endpointUrl": endpoint_url,
            "providerType": provider_type.wire_value(),
            "subscriptions": subscriptions.wire_subscriptions(),
        });
        let response = self
            .mutation_json(
                Endpoint::WebhookCreate,
                reqwest::Method::POST,
                token,
                self.path(&["tailnet", tailnet, "webhooks"], &[])?,
                Some(body),
            )
            .await?;
        endpoint_result(response, tailnet, true, Endpoint::WebhookCreate)
    }

    pub async fn edit_webhook_subscriptions(
        &self,
        token: &AccessToken,
        endpoint_id: &str,
        subscriptions: &SubscriptionSet,
    ) -> Result<WebhookApiResult, AdminError> {
        let body = serde_json::json!({
            "subscriptions": subscriptions.wire_subscriptions(),
        });
        let response = self
            .mutation_json(
                Endpoint::WebhookEdit,
                reqwest::Method::PATCH,
                token,
                self.path(&["webhooks", endpoint_id], &[])?,
                Some(body),
            )
            .await?;
        endpoint_result(response, "webhook", false, Endpoint::WebhookEdit)
    }

    pub async fn test_webhook(
        &self,
        token: &AccessToken,
        endpoint_id: &str,
    ) -> Result<MutationResponse<()>, AdminError> {
        self.mutation_unit(
            Endpoint::WebhookTest,
            reqwest::Method::POST,
            token,
            self.path(&["webhooks", endpoint_id, "test"], &[])?,
            None,
        )
        .await
    }

    pub async fn rotate_webhook_secret(
        &self,
        token: &AccessToken,
        endpoint_id: &str,
    ) -> Result<WebhookApiResult, AdminError> {
        let response = self
            .mutation_json(
                Endpoint::WebhookRotate,
                reqwest::Method::POST,
                token,
                self.path(&["webhooks", endpoint_id, "rotate"], &[])?,
                None,
            )
            .await?;
        endpoint_result(response, "webhook", true, Endpoint::WebhookRotate)
    }

    pub async fn delete_webhook(
        &self,
        token: &AccessToken,
        endpoint_id: &str,
    ) -> Result<MutationResponse<()>, AdminError> {
        self.mutation_unit(
            Endpoint::WebhookDelete,
            reqwest::Method::DELETE,
            token,
            self.path(&["webhooks", endpoint_id], &[])?,
            None,
        )
        .await
    }
}

fn endpoint_result(
    response: ApiResponse<WebhookDto>,
    source_id: &str,
    allow_secret: bool,
    secret_endpoint: Endpoint,
) -> Result<WebhookApiResult, AdminError> {
    let ApiResponse { mut value, meta } = response;
    let secret = value.secret.take();
    if secret.is_some() && !allow_secret {
        return Err(AdminError::DecodeFailed {
            operation: Endpoint::Webhook.operation().to_owned(),
            detail: "webhook inventory response contained write-only secret material".to_owned(),
        });
    }
    if allow_secret && secret.is_none() {
        return Err(AdminError::DecodeFailed {
            operation: secret_endpoint.operation().to_owned(),
            detail: "webhook mutation did not return the documented one-time secret".to_owned(),
        });
    }
    let endpoint = endpoint_from_dto(value, meta.observed_at, source_id, allow_secret)?;
    let secret = secret.map(|value| SecretBuffer::new(value.as_bytes()));
    Ok(WebhookApiResult {
        endpoint,
        secret,
        meta,
    })
}

fn endpoint_from_dto(
    value: WebhookDto,
    observed_at: u64,
    source_id: &str,
    allow_secret: bool,
) -> Result<WebhookEndpoint, AdminError> {
    if value.secret.is_some() && !allow_secret {
        return Err(AdminError::DecodeFailed {
            operation: Endpoint::Webhooks.operation().to_owned(),
            detail: "webhook inventory response contained write-only secret material".to_owned(),
        });
    }
    let endpoint_id = value
        .endpoint_id
        .ok_or_else(|| missing_field("endpointId"))?;
    let endpoint_url = value
        .endpoint_url
        .ok_or_else(|| missing_field("endpointUrl"))?;
    let subscriptions =
        SubscriptionSet::from_wire(Vec::new(), value.subscriptions.unwrap_or_default())
            .map_err(webhook_validation_error)?;
    Ok(WebhookEndpoint {
        stable_id: endpoint_id,
        endpoint_url,
        destination_type: DestinationType::from_wire(
            value.provider_type.as_deref().map_or("none", |value| value),
        ),
        subscriptions,
        creator_login_name: value.creator_login_name,
        created_at: value.created,
        last_modified_at: value.last_modified,
        status: value.status.unwrap_or_else(|| "observed".to_owned()),
        last_result: value.last_result,
        observed_at,
        source_id: source_id.to_owned(),
    })
}

fn missing_collection(endpoint: Endpoint) -> AdminError {
    AdminError::DecodeFailed {
        operation: endpoint.operation().to_owned(),
        detail: "response did not contain the required webhook collection".to_owned(),
    }
}

fn missing_field(field: &'static str) -> AdminError {
    AdminError::DecodeFailed {
        operation: Endpoint::Webhook.operation().to_owned(),
        detail: format!("webhook response omitted required field {field}"),
    }
}

fn webhook_validation_error(error: WebhookError) -> AdminError {
    AdminError::ValidationFailed {
        operation: "webhook".to_owned(),
        detail: error.to_string(),
    }
}
