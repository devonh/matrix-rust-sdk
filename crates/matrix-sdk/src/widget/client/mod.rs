//! Internal client widget API implementation.

use std::{
    borrow::Cow,
    error::Error,
    ops::Deref,
    sync::{Arc, Mutex},
};

use ruma::{
    api::client::account::request_openid_token::v3::Response as RumaOpenIdResponse,
    events::{AnyTimelineEvent, TimelineEventType},
    serde::Raw,
    OwnedEventId, OwnedRoomId,
};
use serde::{Deserialize, Serialize};
use serde_json::{
    from_str as from_json, from_value as from_json_value, to_string as to_json, Value as JsonValue,
};
use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    oneshot,
};
use tracing::error;

use self::messages::{
    from_widget::{RequestType, ResponseType, SupportedApiVersionsResponse},
    Empty, IncomingMessage, IncomingMessageKind, OpenIdResponse, OpenIdState, OutgoingMessage,
    OutgoingMessageKind,
};

use super::{
    filter::{MatrixEventContent, MatrixEventFilterInput},
    Permissions,
};

mod messages;

/// State machine that handles the client widget API interractions.
pub struct ClientApi {
    actions_tx: UnboundedSender<Action>,
    init_on_content_load: bool,
    permissions: Arc<Mutex<Option<Permissions>>>,
    request_proxy: Arc<RequestProxy>,
    room_id: OwnedRoomId,
}

impl ClientApi {
    /// Creates a new instance of a client widget API state machine.
    /// Returns the client api handler as well as the channel to receive
    /// actions (commands) from the client.
    pub fn new(
        init_on_content_load: bool,
        room_id: OwnedRoomId,
    ) -> (Self, UnboundedReceiver<Action>) {
        let permissions = Arc::new(Mutex::new(None));
        let request_proxy = Arc::new(RequestProxy);

        // Unless we're asked to wait for the content load message,
        // we must start the negotiation of permissions right away.
        if !init_on_content_load {
            let (perm, proxy) = (permissions.clone(), request_proxy.clone());
            tokio::spawn(async move {
                // TODO: Handle an error.
                if let Ok(negotiated) = negotiate_permissions(proxy).await {
                    perm.lock().unwrap().replace(negotiated);
                }
            });
        }

        let (actions_tx, actions_rx) = unbounded_channel();
        (Self { actions_tx, init_on_content_load, permissions, request_proxy, room_id }, actions_rx)
    }

    /// Processes an incoming event (an incoming raw message from a widget,
    /// or a data produced as a result of a previously sent `Action`).
    /// Produceses a list of actions that the client must perform.
    pub fn process(&mut self, event: Event) {
        // Most requests require some async actions, so we run them in a separate
        // task to not block the `process()` function. So we clone the local state,
        // so that we can move it into the task.
        let (permissions, proxy, actions) =
            (self.permissions.clone(), self.request_proxy.clone(), self.actions_tx.clone());

        // Process the event.
        match event {
            Event::MessageFromWidget(raw) => match from_json::<IncomingMessage>(&raw) {
                Ok(msg) => match msg.kind {
                    IncomingMessageKind::FromWidget(req) => match req {
                        RequestType::GetSupportedApiVersion(body) => {
                            let msg = OutgoingMessage {
                                header: msg.header.clone(),
                                kind: OutgoingMessageKind::FromWidget(
                                    ResponseType::GetSupportedApiVersion(
                                        body.map(Ok(SupportedApiVersionsResponse::new())),
                                    ),
                                ),
                            };
                            let raw = to_json(&msg).expect("Failed to serialize a message");
                            let _ = actions.send(Action::SendToWidget(raw));
                        }
                        RequestType::ContentLoaded(body) => {
                            let (response, negotiate) = match (
                                self.init_on_content_load,
                                permissions.lock().unwrap().as_ref(),
                            ) {
                                (true, None) => (Ok(Empty {}), true),
                                (true, Some(..)) => (Err("Already loaded".into()), false),
                                _ => (Ok(Empty {}), false),
                            };

                            let msg = OutgoingMessage {
                                header: msg.header.clone(),
                                kind: OutgoingMessageKind::FromWidget(ResponseType::ContentLoaded(
                                    body.map(response),
                                )),
                            };
                            let raw = to_json(&msg).expect("Failed to serialize a message");
                            let _ = actions.send(Action::SendToWidget(raw));

                            if negotiate {
                                tokio::spawn(async move {
                                    // TODO: Handle an error.
                                    if let Ok(negotiated) = negotiate_permissions(proxy).await {
                                        permissions.lock().unwrap().replace(negotiated);
                                    }
                                });
                            }
                        }
                        RequestType::GetOpenId(body) => {
                            tokio::spawn(async move {
                                let msg = OutgoingMessage {
                                    header: msg.header.clone(),
                                    kind: OutgoingMessageKind::FromWidget(ResponseType::GetOpenId(
                                        body.map(Ok(OpenIdResponse::Pending)),
                                    )),
                                };
                                let raw = to_json(&msg).expect("Failed to serialize a message");
                                let _ = actions.send(Action::SendToWidget(raw));

                                let openid = proxy.send(RequestOpenId).await;
                                let response = match openid {
                                    Ok(openid) => OpenIdResponse::Allowed(OpenIdState::new(
                                        msg.header.request_id,
                                        openid,
                                    )),
                                    Err(_) => OpenIdResponse::Blocked,
                                };

                                if let Err(err) = proxy.send(UpdateOpenId(response)).await {
                                    error!("Widget rejected OpenId update: {err}");
                                }
                            });
                        }
                        RequestType::ReadEvent(body) => {
                            tokio::spawn(async move {
                                let filters = permissions
                                    .lock()
                                    .unwrap()
                                    .as_ref()
                                    .map(|perm| perm.read.clone())
                                    .unwrap_or_default();

                                let input = MatrixEventFilterInput {
                                    event_type: body.event_type.clone(),
                                    state_key: body.state_key.clone(),
                                    content: MatrixEventContent::default(),
                                };

                                if !filters.iter().any(|filter| filter.matches(&input)) {
                                    let msg =
                                        OutgoingMessage {
                                            header: msg.header.clone(),
                                            kind: OutgoingMessageKind::FromWidget(
                                                ResponseType::ReadEvent(body.map(Err(
                                                    "No permissions to read events".into(),
                                                ))),
                                            ),
                                        };
                                    let raw = to_json(&msg).expect("Failed to serialize a message");
                                    let _ = actions.send(Action::SendToWidget(raw));
                                    return;
                                }

                                let cmd = ReadEventCommand {
                                    event_type: body.event_type.clone(),
                                    limit: body.limit.unwrap_or(50),
                                };

                                let response = match proxy.send(ReadMatrixEvent(cmd)).await {
                                    Ok(events) => body.map(Ok(events
                                        .into_iter()
                                        .filter(|raw| {
                                            raw.deserialize_as()
                                                .ok()
                                                .map(|de_helper| {
                                                    filters.iter().any(|f| f.matches(&de_helper))
                                                })
                                                .unwrap_or(false)
                                        })
                                        .collect::<Vec<_>>()
                                        .into())),
                                    Err(err) => body.map(Err(err.into())),
                                };

                                let msg = OutgoingMessage {
                                    header: msg.header.clone(),
                                    kind: OutgoingMessageKind::FromWidget(ResponseType::ReadEvent(
                                        response,
                                    )),
                                };
                                let raw = to_json(&msg).expect("Failed to serialize a message");
                                let _ = actions.send(Action::SendToWidget(raw));
                            });
                        }
                        RequestType::SendEvent(body) => {
                            let room_id = self.room_id.clone();
                            tokio::spawn(async move {
                                let filter = permissions
                                    .lock()
                                    .unwrap()
                                    .as_ref()
                                    .map(|p| p.send.clone())
                                    .unwrap_or_default();

                                let input = MatrixEventFilterInput {
                                    event_type: body.event_type.clone(),
                                    state_key: body.state_key.clone(),
                                    content: from_json_value((*body).content.clone())
                                        .unwrap_or_default(),
                                };

                                if !filter.iter().any(|filter| filter.matches(&input)) {
                                    let msg =
                                        OutgoingMessage {
                                            header: msg.header.clone(),
                                            kind: OutgoingMessageKind::FromWidget(
                                                ResponseType::SendEvent(body.map(Err(
                                                    "No permissions to send events".into(),
                                                ))),
                                            ),
                                        };
                                    let raw = to_json(&msg).expect("Failed to serialize a message");
                                    let _ = actions.send(Action::SendToWidget(raw));
                                    return;
                                }

                                let response =
                                    match proxy.send(SendMatrixEvent((*body).clone())).await {
                                        Ok(event_id) => {
                                            body.map(Ok(messages::from_widget::SendEventResponse {
                                                room_id: room_id.to_string(),
                                                event_id: event_id.to_string(),
                                            }))
                                        }
                                        Err(err) => body.map(Err(err.into())),
                                    };
                                let msg = OutgoingMessage {
                                    header: msg.header.clone(),
                                    kind: OutgoingMessageKind::FromWidget(ResponseType::SendEvent(
                                        response,
                                    )),
                                };
                                let raw = to_json(&msg).expect("Failed to serialize a message");
                                let _ = actions.send(Action::SendToWidget(raw));
                            });
                        }
                    },
                    IncomingMessageKind::ToWidget(resp) => {}
                },
                Err(_) => {
                    // TODO: Implement that sophisticated error handling that requires us sending different responses to the widget depending on the body of the original message. These are only used by the widget to print something to the console though, so not a deal breaker.
                }
            },
            Event::MatrixEventReceived(event) => {
                tokio::spawn(async move {
                    // TODO: Do a more elaborate check on `perm.read`.
                    let allowed_to_receive_events = permissions
                        .lock()
                        .unwrap()
                        .as_ref()
                        .map(|perm| !perm.read.is_empty())
                        .unwrap_or(false);

                    if allowed_to_receive_events {
                        if let Err(err) = proxy.send(ReadEventNotification(event)).await {
                            error!("Failed to send an event to the client: {err}");
                        }
                    }
                });
            }
            Event::PermissionsAcquired(result) => {}
            Event::OpenIdReceived(result) => {}
            Event::MatrixEventRead(result) => {}
            Event::MatrixEventSent(result) => {}
        }
    }
}

/// Incoming event that the client API must process.
pub enum Event {
    /// An incoming raw message from the widget.
    MessageFromWidget(String),
    /// Matrix event received. This one is delivered as a result of client
    /// subscribing to the events (`Action::Subscribe` command).
    MatrixEventReceived(Raw<AnyTimelineEvent>),
    /// Client acquired permissions from the user.
    /// A response to an `Action::AcquirePermissions` command.
    PermissionsAcquired(CommandResult<Permissions>),
    /// Client got OpenId token for a given request ID.
    /// A response to an `Action::GetOpenId` command.
    OpenIdReceived(CommandResult<RumaOpenIdResponse>),
    /// Client read some matrix event(s).
    /// A response to an `Action::ReadMatrixEvent` commands.
    MatrixEventRead(CommandResult<Vec<Raw<AnyTimelineEvent>>>),
    /// Client sent some matrix event. The response contains the event ID.
    /// A response to an `Action::SendMatrixEvent` command.
    MatrixEventSent(CommandResult<OwnedEventId>),
}

/// Action (a command) that client (driver) must perform.
#[allow(dead_code)] // TODO: Remove once all actions are implemented.
pub enum Action {
    /// Send a raw message to the widget.
    SendToWidget(String),
    /// Acquire permissions from the user given the set of desired permissions.
    /// Must eventually be answered with `Event::PermissionsAcquired`.
    AcquirePermissions(Command<Permissions>),
    /// Get OpenId token for a given request ID.
    GetOpenId(Command<()>),
    /// Read matrix event(s) that corresponds to the given description.
    ReadMatrixEvent(Command<ReadEventCommand>),
    // Send matrix event that corresponds to the given description.
    SendMatrixEvent(Command<SendEventCommand>),
    /// Subscribe to the events in the *current* room, i.e. a room which this
    /// widget is instantiated with. The client is aware of the room.
    Subscribe,
    /// Unsuscribe from the events in the *current* room. Symmetrical to
    /// `Subscribe`.
    Unsubscribe,
}

/// Command to read matrix event(s).
pub struct ReadEventCommand {
    /// Read event(s) of a given type.
    pub event_type: TimelineEventType,
    /// Limits for the Matrix request.
    pub limit: u32,
}

/// Command to send matrix event.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SendEventCommand {
    #[serde(rename = "type")]
    /// type of an event.
    pub event_type: TimelineEventType,
    /// State key of an event (if it's a state event).
    pub state_key: Option<String>,
    /// Raw content of an event.
    pub content: JsonValue,
}

/// Command that is sent from the client widget API state machine to the
/// client (driver) that must be performed. Once the command is executed,
/// the client will typically generate an `Event` with the result of it.
pub struct Command<T> {
    /// Certain commands are typically answered with certain event once the
    /// command is performed. The api state machine will "tag" each command
    /// with some "cookie" (in this case just an ID), so that once the
    /// result of the execution of this command is received, it could be
    /// matched.
    id: String,
    // Data associated with this command.
    data: T,
}

impl<T> Command<T> {
    /// Consumes the command and produces a command result with given data.
    pub fn result<U, E: Error>(self, result: Result<U, E>) -> CommandResult<U> {
        CommandResult { id: self.id, result: result.map_err(|e| e.to_string().into()) }
    }

    pub fn ok<U>(self, value: U) -> CommandResult<U> {
        CommandResult { id: self.id, result: Ok(value) }
    }
}

impl<T> Deref for Command<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

/// The result of the execution of a command. Note that this type can only be
/// constructed within this module, i.e. it can only be constructed as a result
/// of a command that has been sent from this module, which means that the
/// client (driver) won't be able to send "invalid" commands, because they could
/// only be generated from a `Command` instance.
#[allow(dead_code)] // TODO: Remove once results are used.
pub struct CommandResult<T> {
    /// ID of the command that was executed. See `Command::id` for more details.
    id: String,
    /// Result of the execution of the command.
    result: Result<T, Cow<'static, str>>,
}

struct RequestProxy;

impl RequestProxy {
    async fn send<T: Request>(&self, _request: T) -> Result<T::Response, Cow<'static, str>> {
        todo!()
    }
}

async fn negotiate_permissions(proxy: Arc<RequestProxy>) -> Result<Permissions, Cow<'static, str>> {
    let desired_permissions = proxy.send(RequestPermissions).await?;
    let granted_permissions = proxy.send(AcquirePermissions(desired_permissions)).await?;
    let _ = proxy.send(UpdatePermissions(granted_permissions.clone())).await?;
    Ok(granted_permissions)
}

trait Request {
    type Response;
}

struct RequestPermissions;
impl Request for RequestPermissions {
    type Response = Permissions;
}

struct AcquirePermissions(Permissions);
impl Request for AcquirePermissions {
    type Response = Permissions;
}

struct UpdatePermissions(Permissions);
impl Request for UpdatePermissions {
    type Response = ();
}

struct ReadEventNotification(Raw<AnyTimelineEvent>);
impl Request for ReadEventNotification {
    type Response = ();
}

struct RequestOpenId;
impl Request for RequestOpenId {
    type Response = RumaOpenIdResponse;
}

struct UpdateOpenId(OpenIdResponse);
impl Request for UpdateOpenId {
    type Response = ();
}

struct ReadMatrixEvent(ReadEventCommand);
impl Request for ReadMatrixEvent {
    type Response = Vec<Raw<AnyTimelineEvent>>;
}

struct SendMatrixEvent(SendEventCommand);
impl Request for SendMatrixEvent {
    type Response = OwnedEventId;
}
