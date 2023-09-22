//! Events that are used by the Widget API.

use serde::{Deserialize, Serialize};

pub(crate) use self::{
    header::{Header, WithHeader},
    message::{Empty, Request},
    openid::{Response as OpenIdResponse, State as OpenIdState},
};

mod header;
mod message;
mod openid;

pub mod from_widget;
pub mod to_widget;

// Note, that this structure is identical to `WithHeader<IncomingMessageBody>`,
// but `WithHeader` does not implement `Deserialize`.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct IncomingMessage {
    #[serde(flatten)]
    pub(crate) header: Header,
    #[serde(flatten)]
    pub(crate) data: IncomingMessageBody,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "api")]
#[serde(rename_all = "camelCase")]
pub(crate) enum IncomingMessageBody {
    FromWidget(from_widget::RequestType),
    ToWidget(to_widget::ResponseType),
}

// Note, that this structure is identical to `WithHeader<T>`,
// but `WithHeader` does not implement `Serialize`.
#[derive(Clone, Debug, Serialize)]
pub struct OutgoingMessage<T> {
    #[serde(flatten)]
    pub(crate) header: Header,
    #[serde(flatten)]
    pub(crate) data: T,
}

impl<T: Serialize> From<WithHeader<T>> for OutgoingMessage<T> {
    fn from(WithHeader { header, data }: WithHeader<T>) -> Self {
        Self { header, data }
    }
}
