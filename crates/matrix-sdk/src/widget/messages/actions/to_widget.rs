use serde::{Deserialize, Serialize};

use crate::widget::{
    messages::{Empty, OpenIdResponse, Request, Response},
    Permissions as Capabilities,
};

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "action")]
pub enum SupportedRequest {
    #[serde(rename = "capabilities")]
    CapabilitiesRequest(Request<Empty>),
    #[serde(rename = "notify_capabilities")]
    CapabilitiesUpdate(Request<CapabilitiesUpdatedRequest>),
    #[serde(rename = "openid_credentials")]
    OpenIdCredentialsUpdate(Request<OpenIdResponse>),
    #[serde(rename = "send_event")]
    SendEvent(Request<serde_json::Value>),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "action")]
pub enum SupportedResponse {
    #[serde(rename = "capabilities")]
    CapabilitiesRequest(Response<Empty, CapabilitiesResponse>),
    #[serde(rename = "notify_capabilities")]
    CapabilitiesUpdate(Response<CapabilitiesUpdatedRequest, Empty>),
    #[serde(rename = "openid_credentials")]
    OpenIdCredentialsUpdate(Response<OpenIdResponse, Empty>),
    #[serde(rename = "send_event")]
    SendEvent(Response<serde_json::Value, Empty>),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CapabilitiesResponse {
    pub capabilities: Capabilities,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CapabilitiesUpdatedRequest {
    pub requested: Capabilities,
    pub approved: Capabilities,
}
