//! Client-side state machine for handling incoming requests from a widget.

use std::sync::Arc;

use ruma::{api::client::filter::RoomEventFilter, assign, events::AnyTimelineEvent, serde::Raw};
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{info, warn};

use super::{
    outgoing::{CapabilitiesRequest, CapabilitiesUpdate, OpenIdCredentialsUpdate},
    Error, IncomingRequest as Request, IncomingResponse as Response, OpenIdResponse, OpenIdStatus,
    Result,
};
use crate::{
    room::MessagesOptions,
    widget::{
        client::{MatrixDriver, WidgetProxy},
        filter::{any_matches, MatrixEventFilterInput},
        messages::{
            from_widget::{
                ApiVersion, ReadEventRequest, ReadEventResponse, SendEventRequest,
                SendEventResponse, StateKeySelector, SupportedApiVersionsResponse,
            },
            to_widget::{CapabilitiesResponse, CapabilitiesUpdatedRequest},
            Empty,
        },
        EventFilter, Permissions, PermissionsProvider, StateEventFilter,
    },
    Room,
};

/// State of our client API state machine that handles incoming messages and
/// advances the state.
pub(super) struct State<T> {
    widget: Arc<WidgetProxy>,
    client: MatrixDriver<T>,
    permissions: Permissions,
    /// Receiver of incoming matrix events. `None` if we don't have permissions
    /// to subscribe to the new events, or it hasn't been initialized yet.
    listener: Option<UnboundedReceiver<Raw<AnyTimelineEvent>>>,
    initialized: bool,
}

impl<T: PermissionsProvider> State<T> {
    /// Creates a new [`Self`] with a given proxy and a matrix driver.
    pub(super) fn new(widget: Arc<WidgetProxy>, client: MatrixDriver<T>) -> Self {
        Self { widget, client, listener: None, permissions: Default::default(), initialized: false }
    }

    /// Start a task that will listen to the `rx` for new incoming requests from
    /// a widget and process them.
    pub(super) async fn listen(mut self, mut rx: UnboundedReceiver<Request>) {
        // Typically, widget's capabilities are initialized on a special `ContentLoad`
        // message. However, if this flag is set, we must initialize them right away.
        if !self.widget.init_on_load() {
            if let Err(err) = self.initialize().await {
                // We really don't have a mechanism to inform a widget about out of bound
                // errors. So the only thing we can do here is to log it.
                warn!(error = %err, "Failed to initialize widget");
                return;
            }
        }

        // Handle incoming requests from a widget.
        while let Some(request) = rx.recv().await {
            if let Err(err) = self.handle(request.clone()).await {
                if self.reply(request.fail(err.to_string())).await.is_err() {
                    info!("Dropped reply, widget is disconnected");
                    break;
                }
            }
        }
    }

    /// Handles a given incoming request from a widget.
    async fn handle(&mut self, request: Request) -> Result<()> {
        match request {
            Request::GetSupportedApiVersion(req) => {
                self.reply(req.map(Ok(SupportedApiVersionsResponse::new()))).await?;
            }

            Request::ContentLoaded(req) => {
                let (response, negotiate) = match (self.widget.init_on_load(), self.initialized) {
                    (true, false) => (Ok(Empty {}), true),
                    (true, true) => (Err("Already loaded".into()), false),
                    _ => (Ok(Empty {}), false),
                };

                self.reply(req.map(response)).await?;
                if negotiate {
                    self.initialize().await?;
                }
            }

            Request::GetOpenId(req) => {
                let (reply, handle) = match self.client.get_openid(req.id().to_owned()) {
                    OpenIdStatus::Resolved(decision) => (decision.into(), None),
                    OpenIdStatus::Pending(handle) => (OpenIdResponse::Pending, Some(handle)),
                };

                self.reply(req.map(Ok(reply))).await?;
                if let Some(handle) = handle {
                    let status = handle.await.map_err(|_| Error::WidgetDisconnected)?;
                    self.widget.send(OpenIdCredentialsUpdate::new(status.into())).await?;
                }
            }

            Request::ReadEvent(req) => {
                let fut = read(&self.client.room, (*req).clone(), &self.permissions.read);
                let response = req.map(Ok(fut.await?));
                self.reply(response).await?;
            }

            Request::SendEvent(req) => {
                let fut = send(&self.client.room, (*req).clone(), &self.permissions.send);
                let response = req.map(Ok(fut.await?));
                self.reply(response).await?;
            }
        }

        Ok(())
    }

    /// Performs capability negotiation with a widget. This initialization
    /// is typically performed at the beginning (either once a `ContentLoad` is
    /// received or once the widget is connected, depending on widget settings).
    async fn initialize(&mut self) -> Result<()> {
        // Request the desired capabilities from a widget.
        let CapabilitiesResponse { capabilities: desired } =
            self.widget.send(CapabilitiesRequest::new(Empty {})).await?;

        // Initialize the capabilities with the desired capabilities.
        (self.permissions, self.listener) = self.client.initialize(desired.clone()).await;

        // Subscribe to the events if the widget was granted such capabilities.
        // `take()` is fine here since we never rely upon this value again.
        /* if let Some(mut listener) = capabilities.listener.take() {
            let widget = self.widget.clone();
            tokio::spawn(async move {
                while let Some(event) = listener.recv().await {
                    if let Err(err) = widget
                        .send(SendEvent::new(
                            serde_json::to_value(event).expect("Could not convert to value"),
                        ))
                        .await
                    {
                        warn!("Failed to send an event to a widget: {err}");
                    }
                }
            });
        }

        // Update the capabilities with the approved ones and send the response back.
        self.capabilities = Some(capabilities); */
        self.widget
            .send(CapabilitiesUpdate::new(CapabilitiesUpdatedRequest {
                requested: desired,
                approved: self.permissions.clone(),
            }))
            .await?;

        Ok(())
    }

    async fn reply(&self, response: Response) -> Result<()> {
        self.widget.reply(response).await.map_err(|_| Error::WidgetDisconnected)
    }
}

impl SupportedApiVersionsResponse {
    pub(crate) fn new() -> Self {
        Self {
            versions: vec![
                ApiVersion::V0_0_1,
                ApiVersion::V0_0_2,
                ApiVersion::MSC2762,
                ApiVersion::MSC2871,
                ApiVersion::MSC3819,
            ],
        }
    }
}

// TODO: This function currently makes a request if there are any filters, even
// if we already know that type being requested won't match the filter. Instead,
// we should check the filter before making the request, and when seeing the
// response filter based on what was requested, rather than what the widget is
// allowed to receive (probably..).
//
// Also, if the limit for state events is 1, i.e. only the latest event can be
// retrieved, we should use the endpoint that is specialized for that. However,
// we might still need the messages API if the request is uses
// `StateKeySelector::Any`?
pub(crate) async fn read(
    room: &Room,
    req: ReadEventRequest,
    filters: &[EventFilter],
) -> Result<ReadEventResponse> {
    if filters.is_empty() {
        return Err(Error::custom("No permissions to read any events"));
    }

    let limit = req.limit.unwrap_or(match req.state_key {
        Some(..) => 1, // Default state events limit.
        None => 50,    // Default message-like events limit.
    });

    let options = assign!(MessagesOptions::backward(), {
        limit: limit.into(),
        filter: assign!(RoomEventFilter::default(), {
            types: Some(vec![req.event_type.to_string()])
        })
    });

    // There may be an additional state key filter depending on the `req.state_key`.
    let state_key_filter = move |ev: &MatrixEventFilterInput| -> bool {
        if let Some(StateKeySelector::Key(state_key)) = req.state_key.clone() {
            EventFilter::State(StateEventFilter::WithTypeAndStateKey(
                req.event_type.to_string().into(),
                state_key,
            ))
            .matches(ev)
        } else {
            true
        }
    };

    // Fetch messages from the server.
    let messages = room.messages(options).await.map_err(Error::other)?;

    // Filter the timeline events.
    let events = messages
        .chunk
        .into_iter()
        .map(|ev| ev.event.cast())
        // TODO: Log events that failed to decrypt?
        .filter(|raw| match raw.deserialize_as() {
            Ok(filter_in) => any_matches(filters, &filter_in) && state_key_filter(&filter_in),
            Err(e) => {
                warn!("Failed to deserialize timeline event: {e}");
                false
            }
        })
        .collect();

    Ok(ReadEventResponse { events })
}

pub(crate) async fn send(
    room: &Room,
    req: SendEventRequest,
    filters: &[EventFilter],
) -> Result<SendEventResponse> {
    if filters.is_empty() {
        return Err(Error::custom("No permissions to send events"));
    }

    let filter_in = MatrixEventFilterInput::from_send_event_request(req.clone());

    // Run the request through the filter.
    if !any_matches(filters, &filter_in) {
        return Err(Error::custom("Message not allowed by filter"));
    }

    // Send the request based on whether the state key is set or not.
    //
    // TODO: not sure about the `*_raw` methods here, same goes for
    // the `MatrixEvent`. I feel like stronger types would suit better here,
    // but that's how it was originally implemented by @toger5, clarify it
    // later once @jplatte reviews it.
    let event_id = match req.state_key {
        Some(state_key) => {
            room.send_state_event_raw(req.content, &req.event_type.to_string(), &state_key)
                .await
                .map_err(Error::other)?
                .event_id
        }
        None => {
            room.send_raw(req.content, &req.event_type.to_string(), None)
                .await
                .map_err(Error::other)?
                .event_id
        }
    };

    Ok(SendEventResponse { room_id: room.room_id().to_owned(), event_id: event_id.to_owned() })
}
