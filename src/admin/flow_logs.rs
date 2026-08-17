use crate::admin::auth::AccessToken;
use crate::admin::client::{AdminClient, AdminError, ApiResponse, Endpoint};
use crate::admin::dto::{FlowConnectionDto, FlowNodeDto, NetworkFlowLogDto};
use crate::domain::flow::{
    FlowConnection, FlowError, FlowMessage, FlowNode, FlowWindow, MAX_FLOW_MESSAGES,
};

impl AdminClient {
    pub async fn get_network_flow_logs(
        &self,
        token: &AccessToken,
        tailnet: &str,
        window: &FlowWindow,
    ) -> Result<ApiResponse<Vec<FlowMessage>>, AdminError> {
        let (start, end) = window.query_values().map_err(flow_validation_error)?;
        let url = self.path(
            &["tailnet", tailnet, "logging", "network"],
            &[("start", start.as_str()), ("end", end.as_str())],
        )?;
        let response = self
            .json_with_limit::<crate::admin::dto::NetworkFlowResponseDto>(
                Endpoint::NetworkFlowLogs,
                token,
                url,
                Some("application/json"),
                crate::domain::flow::MAX_FLOW_BODY_BYTES,
            )
            .await
            .map_err(flow_bound_error)?;
        let logs = response
            .value
            .logs
            .ok_or_else(|| missing_flow_collection(Endpoint::NetworkFlowLogs))?;
        if logs.len() > MAX_FLOW_MESSAGES {
            return Err(AdminError::BodyTooLarge {
                operation: "get network flow logs: decoded message limit; choose a narrower window"
                    .to_owned(),
            });
        }
        let messages = logs
            .into_iter()
            .map(flow_message)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ApiResponse {
            value: messages,
            meta: response.meta,
        })
    }
}

fn flow_bound_error(error: AdminError) -> AdminError {
    match error {
        AdminError::BodyTooLarge { .. } => AdminError::BodyTooLarge {
            operation:
                "get network flow logs: response cap reached; choose a narrower window; incomplete aggregates are disabled"
                    .to_owned(),
        },
        other => other,
    }
}

fn flow_validation_error(error: FlowError) -> AdminError {
    AdminError::ValidationFailed {
        operation: Endpoint::NetworkFlowLogs.operation().to_owned(),
        detail: error.to_string(),
    }
}

fn missing_flow_collection(endpoint: Endpoint) -> AdminError {
    AdminError::DecodeFailed {
        operation: endpoint.operation().to_owned(),
        detail: "response did not contain the required logs collection".to_owned(),
    }
}

fn flow_message(value: NetworkFlowLogDto) -> Result<FlowMessage, AdminError> {
    let node_id = required(value.node_id, "nodeId")?;
    let logged = required(value.logged, "logged")?;
    let start = required(value.start, "start")?;
    let end = required(value.end, "end")?;
    Ok(FlowMessage {
        node_id,
        reporting_node_name: None,
        logged,
        start,
        end,
        source_node: value.source_node.map(flow_node).transpose()?,
        destination_nodes: value
            .destination_nodes
            .unwrap_or_default()
            .into_iter()
            .map(flow_node)
            .collect::<Result<Vec<_>, _>>()?,
        virtual_traffic: value
            .virtual_traffic
            .unwrap_or_default()
            .into_iter()
            .map(flow_connection)
            .collect::<Result<Vec<_>, _>>()?,
        subnet_traffic: value
            .subnet_traffic
            .unwrap_or_default()
            .into_iter()
            .map(flow_connection)
            .collect::<Result<Vec<_>, _>>()?,
        exit_traffic: value
            .exit_traffic
            .unwrap_or_default()
            .into_iter()
            .map(flow_connection)
            .collect::<Result<Vec<_>, _>>()?,
        physical_traffic: value
            .physical_traffic
            .unwrap_or_default()
            .into_iter()
            .map(flow_connection)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn flow_node(value: FlowNodeDto) -> Result<FlowNode, AdminError> {
    Ok(FlowNode {
        node_id: required(value.node_id, "nodeId")?,
        name: value.name,
        addresses: value.addresses.unwrap_or_default(),
        os: value.os,
        user: value.user,
        tags: value.tags.unwrap_or_default(),
    })
}

fn flow_connection(value: FlowConnectionDto) -> Result<FlowConnection, AdminError> {
    let src = required(value.src, "src")?;
    let dst = required(value.dst, "dst")?;
    let (src, src_port) = split_endpoint(&src);
    let (dst, dst_port) = split_endpoint(&dst);
    Ok(FlowConnection {
        proto: required(value.proto, "proto")?,
        src,
        dst,
        src_port,
        dst_port,
        tx_packets: value.tx_packets.unwrap_or(0),
        tx_bytes: value.tx_bytes.unwrap_or(0),
        rx_packets: value.rx_packets.unwrap_or(0),
        rx_bytes: value.rx_bytes.unwrap_or(0),
    })
}

fn required(value: Option<String>, field: &'static str) -> Result<String, AdminError> {
    value.ok_or_else(|| AdminError::DecodeFailed {
        operation: Endpoint::NetworkFlowLogs.operation().to_owned(),
        detail: format!("flow response omitted required field {field}"),
    })
}

fn split_endpoint(value: &str) -> (String, Option<u16>) {
    if let Some(rest) = value.strip_prefix('[')
        && let Some((address, port)) = rest.split_once("]:")
    {
        return (address.to_owned(), port.parse::<u16>().ok());
    }
    if let Some((address, port)) = value.rsplit_once(':')
        && !address.contains(':')
        && let Ok(port) = port.parse::<u16>()
    {
        return (address.to_owned(), Some(port));
    }
    (value.to_owned(), None)
}
