//! Client widget API implementation.

#![warn(unreachable_pub)]

use std::{
    collections::HashMap,
    result::Result as StdResult,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_channel::Sender;
use serde_json::{from_str as from_json, to_string as to_json};
use tokio::{sync::oneshot, time::timeout};
use tracing::warn;
use uuid::Uuid;

use self::handler::{IncomingErrorResponse, IncomingResponse, MessageHandler, OutgoingRequest};
pub(crate) use self::{
    handler::{Error, Result},
    matrix::Driver as MatrixDriver,
};
use super::{
    messages::{
        to_widget::Action as ToWidgetAction, Action, ErrorBody, ErrorMessage, Header, Message,
    },
    PermissionsProvider, Widget, WidgetSettings as WidgetInfo,
};

mod handler;
mod matrix;

/// Runs the client widget API handler for a given widget with a provided
/// `client`. Returns once the widget is disconnected or some terminal error
/// occurs.
pub(super) async fn run<T: PermissionsProvider>(
    client: MatrixDriver<T>,
    widget: Widget,
) -> Result<()> {
    // A map of outgoing requests that we are waiting a response for, i.e. a
    // requests that we sent to the widget and that we're expecting an answer
    // from.
    let state: PendingResponses = Arc::new(Mutex::new(HashMap::new()));

    // Create a tiny helper to report errors to the widget that occur inside this
    // function.
    let reporter = ErrorReporter::new(widget.comm.to.clone(), widget.settings.id.clone());

    // Create a message handler (handles incoming requests from the widget).
    let handler = {
        let widget = WidgetProxy::new(widget.settings, widget.comm.to.clone(), state.clone());
        MessageHandler::new(client, widget)
    };

    // Receives plain JSON string messages from a widget, parses it and passes
    // it to the handler.
    while let Ok(raw) = widget.comm.from.recv().await {
        match from_json::<Message>(&raw) {
            Ok(msg) => match msg.action {
                // This is an incoming request from a widget.
                Action::FromWidget(action) => {
                    handler.handle(msg.header, action).await;
                }
                // This is a response from the widget to a request from the
                // client / driver (i.e. response to the outgoing request from
                // our perspective).
                Action::ToWidget(action) => {
                    let pending = state
                        .lock()
                        .expect("Pending mutex poisoned")
                        .remove(&msg.header.request_id);

                    // If we found a pending response handle, then we send it to the response.
                    // Otherwise, it means that the widget sent a response to the request we never
                    // sent.
                    if let Some(tx) = pending {
                        // It's ok if send fails here, it just means that the
                        // widget has disconnected.
                        let _ = tx.send(action);
                    } else {
                        let id = msg.header.request_id;
                        reporter.report(Some(id), "Unexpected response from a widget").await;
                    }
                }
            },
            Err(e) => {
                reporter.report(None, e.to_string()).await;
            }
        }
    }

    Ok(())
}

/// Map that stores pending responses for the outgoing requests (requests that
/// **we** send to the widget).
type PendingResponses = Arc<Mutex<HashMap<String, oneshot::Sender<ToWidgetAction>>>>;

/// Handles interaction with a widget. Essentially, it's a proxy to the widget
/// that adds some additional type-safety and a convenient "higher level" access
/// to the widget (convenient `async` function that receives a request and
/// returns once the response is received, validated and matched, etc).
pub(crate) struct WidgetProxy {
    /// Widget settings.
    info: WidgetInfo,
    /// Raw communication channel to send `String`s (JSON) to the widget.
    sink: Sender<String>,
    /// Map that stores pending responses for the **outgoing requests**
    /// (requests that **we** send to the widget).
    pending: PendingResponses,
}

impl WidgetProxy {
    fn new(info: WidgetInfo, sink: Sender<String>, pending: PendingResponses) -> Self {
        Self { info, sink, pending }
    }

    /// Sends a request to the widget, returns the response from the widget once
    /// received.
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
    async fn reply(&self, response: impl Into<ReplyToWidget>) -> StdResult<(), ()> {
        let message = response.into().into_json().expect("Bug: can't serialise a message");
        self.sink.send(message).await.map_err(|_| ())
    }

    /// Tells whether or not we should negotiate supported capabilities on
    /// `ContentLoad` or not (if `false`, they are negotiated right away).
    fn init_on_load(&self) -> bool {
        self.info.init_on_load
    }
}

#[derive(Debug)]
enum ReplyToWidget {
    Reply(IncomingResponse),
    Error(IncomingErrorResponse),
}

impl ReplyToWidget {
    fn into_json(self) -> Result<String, ()> {
        match self {
            Self::Reply(r) => to_json::<Message>(&(r.into())).map_err(|_| ()),
            Self::Error(e) => to_json::<ErrorMessage>(&(e.into())).map_err(|_| ()),
        }
    }
}

impl From<IncomingResponse> for ReplyToWidget {
    fn from(response: IncomingResponse) -> Self {
        Self::Reply(response)
    }
}

impl From<IncomingErrorResponse> for ReplyToWidget {
    fn from(msg: IncomingErrorResponse) -> Self {
        Self::Error(msg)
    }
}

// Tiny helper structure for sending out-of-band errors.
struct ErrorReporter {
    /// Raw communication channel to send `String`s (JSON) to the widget.
    sink: Sender<String>,
    /// Widget ID.
    widget_id: String,
}

impl ErrorReporter {
    fn new(sink: Sender<String>, widget_id: String) -> Self {
        Self { sink, widget_id }
    }

    /// Sends an out-of-band error to the widget. Only for internal use.
    async fn report(&self, request_id: Option<String>, error: impl AsRef<str>) {
        let error = ErrorMessage {
            widget_id: self.widget_id.clone(),
            request_id,
            response: ErrorBody::new(error),
        };
        let _ = self.sink.send(to_json(&error).expect("Bug: can't serialise a message")).await;
    }
}
