//! Client widget API implementation.

#![warn(unreachable_pub)]

use std::sync::Arc;

use serde_json::from_str as from_json;
use tracing::warn;

pub(crate) use self::matrix::Driver as MatrixDriver;
use self::{handler::MessageHandler, widget::WidgetProxy};
use super::{
    messages::{IncomingMessage, IncomingMessageBody, WithHeader},
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
            Ok(msg) => match msg.data {
                // This is an incoming request from a widget.
                IncomingMessageBody::FromWidget(action) => {
                    handler.handle(WithHeader::new(msg.header, action)).await;
                }
                // This is a response to our (outgoing) request.
                IncomingMessageBody::ToWidget(action) => {
                    widget.handle_widget_response(WithHeader::new(msg.header, action)).await
                }
            },
            // The message has an invalid format, report an error.
            Err(e) => {
                // TODO: The actual logic for the error handling for unknown message is a bit
                // more complicated. Implement it later based on a discussions with @toger5.
                widget.send_error(e.to_string()).await;
            }
        }
    }
}
