//! Events that are used by the Widget API.

use serde::{Deserialize, Serialize};

pub(crate) use self::{
    actions::{from_widget, to_widget, Empty, ErrorBody, IncomingMessageBody, Request, Response},
    openid::{Response as OpenIdResponse, State as OpenIdState},
};

mod actions;
mod openid;

#[derive(Debug, Deserialize)]
pub(crate) struct IncomingMessage {
    #[serde(flatten)]
    pub header: Header,

    #[serde(flatten)]
    pub body: IncomingMessageBody,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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

/// `data` and a `header` that is associated with it. This ensures that we never
/// handle a request without a header that is associated with it. Likewise, we
/// ensure that the responses come with the request's original header. The
/// fields are private by design so that the user can't modify any of the fields
/// outside of this module by accident. It also ensures that we can only
/// construct this data type from within this module.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct WithHeader<T> {
    //#[serde(flatten)]
    pub(crate) header: Header,
    pub(crate) data: T,
}

impl<T> WithHeader<T> {
    pub(crate) fn new(header: Header, data: T) -> Self {
        Self { header, data }
    }

    pub(crate) fn map<U>(self, data: U) -> WithHeader<U> {
        let Self { header, .. } = self;
        WithHeader { header, data }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ErrorMessage {
    #[serde(flatten)]
    pub original_request: Option<serde_json::Value>,
    pub response: ErrorBody,
}
