//! Client widget API implementation.

#![warn(unreachable_pub)]

use std::sync::Arc;

use serde_json::{from_str as from_json, json};
use tracing::warn;

pub(crate) use self::matrix::Driver as MatrixDriver;
use self::{handler::MessageHandler, widget::WidgetProxy};
use super::{
    messages::{IncomingMessage, IncomingMessageBody},
    PermissionsProvider, Widget,
};

mod handler;
mod matrix;
mod widget;

/// Runs the client widget API handler for a given widget with a provided
/// `client`. Returns once the widget is disconnected.
pub(super) async fn run<T: PermissionsProvider>(
    client: MatrixDriver<T>,
    Widget { settings, comm }: Widget,
) {
    // A small proxy object to interract with a widget via high-level API.
    let widget = Arc::new(WidgetProxy::new(settings, comm.to));

    // Create a message handler (handles incoming requests from the widget).
    let handler = MessageHandler::new(client, widget.clone());

    // Receive a plain JSON message from a widget and parse it.
    while let Ok(raw) = comm.from.recv().await {
        match from_json::<IncomingMessage>(&raw) {
            // The message is valid, process it.
            Ok(msg) => match msg.body {
                // This is an incoming request from a widget.
                IncomingMessageBody::FromWidget(action) => handler.handle(msg.header, action).await,
                // This is a response to our (outgoing) request.
                IncomingMessageBody::ToWidget(action) => {
                    widget.handle_widget_response(msg.header, action).await
                }
            },
            // The message has an invalid format, report an error.
            Err(e) => {
                if let Ok(message) = from_json::<serde_json::Value>(&raw) {
                    match message["response"] {
                        serde_json::Value::Null => {
                            widget.send_error(Some(message), e.to_string()).await;
                        }
                        serde_json::Value::Number(_)
                        | serde_json::Value::String(_)
                        | serde_json::Value::Object(_) => {
                            warn!("ERROR parsing response");
                            // This cannot be send to the widget as a response, because it already
                            // contains a response field
                            widget
                                .send_error(Some(json!({"widget_id": widget.id()})), e.to_string())
                                .await;
                        }
                        _ => {}
                    }
                } else {
                    widget
                        .send_error(
                            None,
                            "The request json could not be parsed as json. Its malformatted.",
                        )
                        .await;
                }
            }
        }
    }
}
