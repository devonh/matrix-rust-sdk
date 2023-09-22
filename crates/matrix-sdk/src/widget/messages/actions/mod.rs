use serde::Deserialize;

pub mod from_widget;
mod message;
pub mod to_widget;

pub use self::message::{Empty, ErrorBody, Kind as MessageKind, Request, Response, ResponseBody};

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "api")]
#[serde(rename_all = "camelCase")]
pub(crate) enum IncomingMessageBody {
    FromWidget(from_widget::SupportedRequest),
    ToWidget(to_widget::SupportedResponse),
}
