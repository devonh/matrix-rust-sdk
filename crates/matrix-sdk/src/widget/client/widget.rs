//! A high-level (safer) API to interract with a widget.

use std::{collections::HashMap, sync::Mutex, time::Duration};

use async_trait::async_trait;
use async_channel::Sender;
use serde_json::to_string as to_json;
use tokio::{sync::oneshot, time::timeout};
use uuid::Uuid;

use super::handler::{Error, OutgoingRequest, Result, ReplyToWidget, Widget};
use crate::widget::{
    messages::{
        to_widget::Action as ToWidgetAction, Action, ErrorBody, ErrorMessage, Header, Message,
    },
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
    pending: Mutex<HashMap<String, oneshot::Sender<ToWidgetAction>>>,
}

impl WidgetProxy {
    pub(super) fn new(info: WidgetSettings, sink: Sender<String>) -> Self {
        let pending = Mutex::new(HashMap::new());
        Self { info, sink, pending }
    }

    /// Sends an out-of-band error to the widget. Only for internal use.
    pub(super) async fn send_error(&self, request_id: Option<String>, error: impl AsRef<str>) {
        let error = ErrorMessage {
            widget_id: self.info.id.clone(),
            request_id,
            response: ErrorBody::new(error),
        };
        let _ = self.sink.send(to_json(&error).expect("Bug: can't serialise a message")).await;
    }

    /// Handles a response from the widget to one of the outgoing requests that
    /// we initiated.
    pub(super) async fn handle_widget_response(&self, header: Header, action: ToWidgetAction) {
        let id = header.request_id;

        // Check if we have a pending oneshot response channel.
        if let Some(tx) = self.pending.lock().expect("Pending mutex poisoned").remove(&id) {
            // It's ok if send fails here, it just means that the widget has disconnected.
            let _ = tx.send(action);
        } else {
            self.send_error(Some(id), "Unexpected response from a widget").await;
        }
    }
}

#[async_trait]
impl Widget for WidgetProxy {
    /// Sends a request to the widget, returns the response from the widget once received.
    async fn send<T: OutgoingRequest>(&self, msg: T) -> Result<T::Response> {
        let id = Uuid::new_v4().to_string();
        let message = {
            let header = Header::new(&id, &self.info.id);
            let action = Action::ToWidget(msg.into_action());
            to_json(&Message::new(header, action)).expect("Bug: can't serialise a message")
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

        T::extract_response(reply)
            .ok_or(Error::custom("Widget sent invalid response"))?
            .map_err(Error::WidgetErrorReply)
    }

    /// Sends a reply to one of the incoming requests from the widget. The reply
    /// itself can only be constructed from a valid incoming request, so we
    /// ensure that only valid replies could be constructed. Error is returned
    /// if the reply cannot be sent due to the widget being disconnected.
    async fn reply(&self, response: impl Into<ReplyToWidget> + Send) -> Result<(), ()> {
        let message = response.into().into_json().expect("Bug: can't serialise a message");
        self.sink.send(message).await.map_err(|_| ())
    }

    /// Tells whether or not we should negotiate supported capabilities on
    /// `ContentLoad` or not (if `false`, they are negotiated right away).
    fn init_on_load(&self) -> bool {
        self.info.init_on_load
    }
}
