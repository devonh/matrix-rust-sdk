//! Client widget API state machine.

use std::sync::Arc;

use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedSender},
    oneshot::Receiver,
};

use self::state::State;
pub(crate) use self::{
    capabilities::Capabilities,
    error::{Error, Result},
    incoming::{Request as IncomingRequest, Response as IncomingResponse},
    openid::{OpenIdDecision, OpenIdStatus},
    outgoing::{Request as OutgoingRequest, Response as OutgoingResponse},
};
use super::{MatrixDriver, WidgetProxy};
use crate::widget::{
    messages::{
        from_widget::{Action, SupportedApiVersionsResponse as SupportedApiVersions},
        Header, OpenIdResponse, OpenIdState,
    },
    PermissionsProvider,
};

mod capabilities;
mod error;
mod incoming;
mod openid;
mod outgoing;
mod state;

/// A component that processes incoming requests from a widget and generates
/// proper responses. This is essentially a state machine for the client-side
/// widget API.
#[allow(missing_debug_implementations)]
pub(crate) struct MessageHandler {
    /// The processing of the incoming requests is delegated to the worker
    /// (state machine runs in its own task or "thread" if you will), so that
    /// the `handle()` function does not block (originally it was non-async).
    /// This channel allows us sending incoming messages to that worker.
    state_tx: UnboundedSender<IncomingRequest>,
    /// A convenient proxy to the widget that allows us interacting with a
    /// widget via more convenient safely typed high level abstractions.
    widget: Arc<WidgetProxy>,
}

impl MessageHandler {
    /// Creates an instance of a message handler with a given matrix driver
    /// (used to handle all matrix related stuff) and a given widget proxy.
    pub(crate) fn new(client: MatrixDriver<impl PermissionsProvider>, widget: WidgetProxy) -> Self {
        let widget = Arc::new(widget);

        // Spawn a new task for the state machine. We'll use a channel to delegate
        // handling of messages and other tasks.
        let (state_tx, state_rx) = unbounded_channel();
        tokio::spawn(State::new(widget.clone(), client).listen(state_rx));

        Self { widget, state_tx }
    }

    /// Handles incoming messages from a widget.
    pub(crate) async fn handle(&self, header: Header, action: Action) -> Result<()> {
        // First let's try to convert the incoming message from a widget into a proper
        // "validated" message, i.e. `IncomingRequest`. If this conversion fails, then
        // it means that the widget sent the message that the widget is not supposed to
        // send. We ensure that we only process correct incoming requests.
        match IncomingRequest::new(header, action).ok_or(Error::custom("Invalid message"))? {
            // Normally, we send all incoming requests to the worker task (`State::listen`), but the
            // `SupportedApiVersions` request is a special case - not only the widget
            // can request them at any time, but it also may block the processing of
            // other messages until we reply with the supported API versions.
            // Luckily, this request is the only single request that does not require
            // any state (as its answer is essentially 'static'), so we can handle this message
            // right away by generating and sending an appropriate response.
            IncomingRequest::GetSupportedApiVersion(req) => self
                .widget
                .reply(req.map(Ok(SupportedApiVersions::new())))
                .await
                .map_err(|_| Error::WidgetDisconnected),
            // Otherwise, send the incoming request to a worker task. This way our
            // `self.handle()` should actually never block. So the caller can call it many times in
            // a row and it's the `State` (that runs in its own task) that will decide which of
            // them to process sequentially and which in parallel.
            request => self.state_tx.send(request).map_err(|_| Error::WidgetDisconnected),
        }
    }
}
