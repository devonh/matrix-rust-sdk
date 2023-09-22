//! A high-level (safer) API to interract with a widget.

use std::{collections::HashMap, result::Result as StdResult, sync::Mutex, time::Duration};

use async_channel::Sender;
use serde::Serialize;
use serde_json::to_string as to_json;
use tokio::{sync::oneshot, time::timeout};
use uuid::Uuid;

use super::handler::{Error, OutgoingRequest, Result};
use crate::widget::{
    messages::{from_widget::ResponseType, to_widget, Header, OutgoingMessage, WithHeader},
    WidgetSettings,
};

/// Handles interaction with a widget. Essentially, it's a proxy to the widget
/// that adds some additional type-safety and a convenient "higher level" access
/// to the widget (convenient `async` function that receives a request and
/// returns once the response is received, validated and matched, etc).
pub(crate) struct WidgetProxy {
    /// Widget settings.
    info: WidgetSettings,
    /// Raw communication channel to send `String`s (JSON) to the widget.
    sink: Sender<String>,
    /// Map that stores pending responses for the **outgoing requests**
    /// (requests that **we** send to the widget).
    pending: Mutex<HashMap<String, oneshot::Sender<to_widget::ResponseType>>>,
}

#[derive(Debug, Serialize)]
struct ResponseMessage {
    #[serde(flatten)]
    pub(crate) original_request: Option<serde_json::Value>,
    pub(crate) response: serde_json::Value,
}
impl WidgetProxy {
    pub(super) fn new(info: WidgetSettings, sink: Sender<String>) -> Self {
        let pending = Mutex::new(HashMap::new());
        Self { info, sink, pending }
    }

    /// Sends a request to the widget, returns the response from the widget once
    /// received.
    pub(crate) async fn send<T: OutgoingRequest>(&self, msg: T) -> Result<T::Response> {
        let id = Uuid::new_v4().to_string();
        let message = {
            let msg = OutgoingMessage {
                header: Header::new(&id, &self.info.id),
                data: msg.into_request_type(),
            };
            to_json(&msg).expect("Bug: can't serialise a message")
        };
        self.sink.send(message).await.map_err(|_| Error::WidgetDisconnected)?;

        // Insert a one-shot channel into the map of pending responses.
        let (tx, rx) = oneshot::channel();
        self.pending.lock().expect("Pending mutex poisoned").insert(id.clone(), tx);

        // Wait for the response from the widget. If the widget does not reply within
        // ten seconds (as per the spec), we return an error and don't wait for
        // the reply.
        let reply = timeout(Duration::from_secs(10), rx)
            .await
            .map_err(|_| {
                self.pending.lock().expect("Pending mutex poisoned").remove(&id);
                // Unfortunately we don't have a way to inform the widget that we're not waiting
                // for the reply anymore.
                Error::custom("Timeout reached while waiting for reply")
            })?
            .map_err(|_| Error::WidgetDisconnected)?;

        T::from_response_type(reply)
            .ok_or(Error::custom("Widget sent invalid response"))?
            .map_err(Error::WidgetErrorReply)
    }

    /// Sends a reply to one of the incoming requests from the widget. The reply
    /// itself can only be constructed from a valid incoming request, so we
    /// ensure that only valid replies could be constructed. Error is returned
    /// if the reply cannot be sent due to the widget being disconnected.
    pub(crate) async fn reply(&self, response: WithHeader<ResponseType>) -> StdResult<(), ()> {
        let msg: OutgoingMessage<_> = response.into();
        let json = to_json(&msg).expect("Bug: can't serialize a message");
        self.sink.send(json).await.map_err(|_| ())
    }

    /// Sends an out-of-band error to the widget. Only for internal use.
    pub(super) async fn send_error(&self, error: impl AsRef<str>) {
        // TODO: Create a structure for the out-of-band errors (refer to the
        // discussion with @toger5 about the tricky error handling in the widget
        // api spec).
        let msg = to_json(error.as_ref()).expect("Bug: can't serialise a message");
        let _ = self.sink.send(msg).await;
    }

    /// Handles a response from the widget to one of the outgoing requests that
    /// we initiated.
    pub(super) async fn handle_widget_response(
        &self,
        message: WithHeader<to_widget::ResponseType>,
    ) {
        let id = message.header.request_id;

        // Check if we have a pending oneshot response channel.
        if let Some(tx) = self.pending.lock().expect("Pending mutex poisoned").remove(&id) {
            // It's ok if send fails here, it just means that the widget has disconnected.
            let _ = tx.send(message.data);
        } else {
            self.send_error("Unexpected response from a widget").await;
        }
    }

    /// Tells whether or not we should negotiate supported capabilities on
    /// `ContentLoad` or not (if `false`, they are negotiated right away).
    pub(crate) fn init_on_load(&self) -> bool {
        self.info.init_on_load
    }
}
