use std::ops::Deref;

pub use self::openid::{OpenIdResponse, OpenIdState};
use serde::{Deserialize, Serialize};

mod openid;
mod permissions;

pub mod from_widget;
pub mod to_widget;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct IncomingMessage {
    #[serde(flatten)]
    pub(crate) header: Header,
    #[serde(flatten)]
    pub(crate) kind: IncomingMessageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "api")]
#[serde(rename_all = "camelCase")]
pub(crate) enum IncomingMessageKind {
    FromWidget(from_widget::RequestType),
    ToWidget(to_widget::ResponseType),
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct OutgoingMessage {
    #[serde(flatten)]
    pub(crate) header: Header,
    #[serde(flatten)]
    pub(crate) kind: OutgoingMessageKind,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "api")]
#[serde(rename_all = "camelCase")]
pub(crate) enum OutgoingMessageKind {
    FromWidget(from_widget::ResponseType),
    ToWidget(to_widget::RequestType),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Header {
    pub request_id: String,
    pub widget_id: String,
}

impl Header {
    pub fn new(request_id: impl Into<String>, widget_id: impl Into<String>) -> Self {
        Self { request_id: request_id.into(), widget_id: widget_id.into() }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Request<T> {
    #[serde(rename = "data")]
    pub content: T,
}

impl<T> Request<T> {
    pub fn new(content: T) -> Self {
        Self { content }
    }

    pub fn map<R>(self, response: Result<R, String>) -> Response<T, R> {
        Response {
            request: self.content,
            response: match response {
                Ok(response) => ResponseBody::Success(response),
                Err(error) => ResponseBody::Failure(ErrorBody::new(error)),
            },
        }
    }
}

impl<T> Deref for Request<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.content
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Response<Req, Resp> {
    /// Responses contain the original request under the data field, just like
    /// the request itself.
    #[serde(rename = "data")]
    pub request: Req,
    pub response: ResponseBody<Resp>,
}

impl<Req, Resp: Clone> Response<Req, Resp> {
    pub fn result(&self) -> Result<Resp, String> {
        match &self.response {
            ResponseBody::Success(resp) => Ok(resp.clone()),
            ResponseBody::Failure(err) => Err(err.as_ref().to_owned()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ResponseBody<T> {
    Success(T),
    Failure(ErrorBody),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ErrorBody {
    pub error: ErrorContent,
}

impl ErrorBody {
    pub fn new(message: impl AsRef<str>) -> Self {
        Self { error: ErrorContent { message: message.as_ref().to_owned() } }
    }
}

impl AsRef<str> for ErrorBody {
    fn as_ref(&self) -> &str {
        &self.error.message
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ErrorContent {
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Empty {}
