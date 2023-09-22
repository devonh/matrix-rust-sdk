use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[serde(tag = "state")]
pub enum Response {
    Allowed(State),
    Blocked,
    #[serde(rename = "request")]
    Pending,
}
