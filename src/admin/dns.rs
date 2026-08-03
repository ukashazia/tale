use serde_json::Map;
use serde_json::Value;

use crate::admin::dto::{
    DnsPreferencesDto, DtoError, NameserversResponse, SearchPathsDto, split_dns_values,
};
use crate::domain::Timestamp;
use crate::domain::dns::{AdminDnsPreferences, AdminNameservers, AdminSearchPaths, AdminSplitDns};

pub fn decode_nameservers(
    response: NameserversResponse,
    observed_at: Timestamp,
) -> Result<AdminNameservers, DtoError> {
    Ok(AdminNameservers {
        values: response
            .dns
            .ok_or(DtoError::MissingCollection { field: "dns" })?,
        observed_at,
    })
}

pub fn decode_preferences(
    response: DnsPreferencesDto,
    observed_at: Timestamp,
) -> AdminDnsPreferences {
    AdminDnsPreferences {
        magic_dns: response.magic_dns,
        observed_at,
    }
}

pub fn decode_search_paths(
    response: SearchPathsDto,
    observed_at: Timestamp,
) -> Result<AdminSearchPaths, DtoError> {
    Ok(AdminSearchPaths {
        values: response.search_paths.ok_or(DtoError::MissingCollection {
            field: "searchPaths",
        })?,
        observed_at,
    })
}

pub fn decode_split_dns(
    response: Map<String, Value>,
    observed_at: Timestamp,
) -> Result<AdminSplitDns, DtoError> {
    Ok(AdminSplitDns {
        entries: split_dns_values(response)?,
        observed_at,
    })
}
