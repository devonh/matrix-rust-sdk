use ruma::api::client::account::request_openid_token::v3::Response as RumaOpenIdResponse;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OpenIdState {
    #[serde(rename = "original_request_id")]
    pub id: String,
    #[serde(rename = "access_token")]
    pub token: String,
    #[serde(rename = "expires_in")]
    pub expires_in_seconds: usize,
    #[serde(rename = "matrix_server_name")]
    pub server: String,
    #[serde(rename = "token_type")]
    pub kind: String,
}

impl OpenIdState {
    pub fn new(id: impl Into<String>, ruma: RumaOpenIdResponse) -> Self {
        Self {
            id: id.into(),
            token: ruma.access_token,
            expires_in_seconds: ruma.expires_in.as_secs() as usize,
            server: ruma.matrix_server_name.into(),
            kind: ruma.token_type.to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[serde(tag = "state")]
pub enum OpenIdResponse {
    Allowed(OpenIdState),
    Blocked,
    #[serde(rename = "request")]
    Pending,
}
