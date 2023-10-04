//! Facilities to handle incoming requests from the widget.

use std::sync::{Arc, Mutex};

use ruma::OwnedRoomId;

use super::{
    messages::{
        from_widget::{
            ReadEventBody, ReadEventResponseBody, SendEventBody, SendEventResponseBody,
            SupportedApiVersionsResponse,
        },
        Empty, OpenIdResponse, OpenIdState,
    },
    negotiate_permissions, ReadEventCommand, ReadMatrixEvent, RequestOpenId, RequestProxy,
    SendMatrixEvent, UpdateOpenId,
};
use crate::widget::{
    filter::{MatrixEventContent, MatrixEventFilterInput},
    Permissions,
};

/// Everything that we need in order to process an incoming request.
struct HandlerContext {
    /// Negotiated permissions (capabilities as the widget api calls it).
    // TODO: I don't like the `Arc<Mutex<>>` here. It's a bit ugly. But unfortunately
    //       one of the incoming requests is meant to negotiate capabilities. Furthermore,
    //       widget API actually has a re-negotiation command. Maybe a better way would be
    //       letting the handler **return** a new state to the caller!
    permissions: Arc<Mutex<Option<Permissions>>>,
    /// Should the initialisation of the widget be delayed until the reception
    /// of the `ContentLoad` message.
    init_on_content_load: bool,
    /// ID of the room in which the widget is running.
    room_id: OwnedRoomId,
    /// High level API to send outgoing requests to the widget or the client.
    /// Handling of certain incoming requests may require sending outgoing
    /// requests to the widget or commands to the client.
    proxy: Arc<RequestProxy>,
}

#[async_trait::async_trait]
trait RequestHandler: Send + Sync + 'static {
    type Request: Sized;
    type Response: Sized;

    async fn handle(
        self,
        id: String,
        request: Self::Request,
        ctx: HandlerContext,
    ) -> Result<Self::Response, String>;
}

struct GetSupportedApiVersionsRequest;

#[async_trait::async_trait]
impl RequestHandler for GetSupportedApiVersionsRequest {
    type Request = Empty;
    type Response = SupportedApiVersionsResponse;

    async fn handle(
        self,
        _: String,
        _: Self::Request,
        _: HandlerContext,
    ) -> Result<Self::Response, String> {
        Ok(SupportedApiVersionsResponse::new())
    }
}

struct ContentLoadedRequest;

#[async_trait::async_trait]
impl RequestHandler for ContentLoadedRequest {
    type Request = Empty;
    type Response = Empty;

    async fn handle(
        self,
        _: String,
        _: Self::Request,
        ctx: HandlerContext,
    ) -> Result<Self::Response, String> {
        let (response, negotiate) = match (ctx.init_on_content_load, ctx.permissions()) {
            (true, None) => (Ok(Empty {}), true),
            (true, Some(..)) => (Err("Already loaded".into()), false),
            _ => (Ok(Empty {}), false),
        };

        if negotiate {
            // TODO: Actually we need to spawn this *after* sending a reply.
            tokio::spawn(async move {
                if let Ok(negotiated) = negotiate_permissions(ctx.proxy).await {
                    ctx.permissions.lock().unwrap().replace(negotiated);
                }
            });
        }

        response
    }
}

struct GetOpenIdRequest;

#[async_trait::async_trait]
impl RequestHandler for GetOpenIdRequest {
    type Request = Empty;
    type Response = OpenIdResponse;

    async fn handle(
        self,
        id: String,
        _: Self::Request,
        ctx: HandlerContext,
    ) -> Result<Self::Response, String> {
        // TODO: Actually we need to spawn this *after* sending a reply.
        tokio::spawn(async move {
            let response = match ctx.proxy.send(RequestOpenId).await {
                Ok(openid) => OpenIdResponse::Allowed(OpenIdState::new(id, openid)),
                Err(_) => OpenIdResponse::Blocked,
            };

            if let Err(err) = ctx.proxy.send(UpdateOpenId(response)).await {
                tracing::error!("Widget rejected OpenID response: {}", err);
            }
        });

        Ok(OpenIdResponse::Pending)
    }
}

struct ReadEventRequest;

#[async_trait::async_trait]
impl RequestHandler for ReadEventRequest {
    type Request = ReadEventBody;
    type Response = ReadEventResponseBody;

    async fn handle(
        self,
        _: String,
        req: Self::Request,
        ctx: HandlerContext,
    ) -> Result<Self::Response, String> {
        let filters = ctx.permissions().map(|p| p.read).unwrap_or_default();

        let input = req.clone().into();
        if !filters.iter().any(|f| f.matches(&input)) {
            return Err("Not allowed".into());
        }

        let cmd = ReadEventCommand { event_type: req.event_type, limit: req.limit.unwrap_or(50) };
        let events = ctx.proxy.send(ReadMatrixEvent(cmd)).await.map_err(|err| err.to_string())?;

        Ok(events
            .into_iter()
            .filter(|raw| {
                raw.deserialize_as()
                    .ok()
                    .map(|de_helper| filters.iter().any(|f| f.matches(&de_helper)))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>()
            .into())
    }
}

struct SendEventRequest;

#[async_trait::async_trait]
impl RequestHandler for SendEventRequest {
    type Request = SendEventBody;
    type Response = SendEventResponseBody;

    async fn handle(
        self,
        _: String,
        req: Self::Request,
        ctx: HandlerContext,
    ) -> Result<Self::Response, String> {
        let filters = ctx.permissions().map(|p| p.send).unwrap_or_default();

        let input = req.clone().into();
        if !filters.iter().any(|f| f.matches(&input)) {
            return Err("Not allowed".into());
        }

        let id = ctx.proxy.send(SendMatrixEvent(req)).await.map_err(|err| err.to_string())?;
        Ok(SendEventResponseBody { event_id: id.to_string(), room_id: ctx.room_id.to_string() })
    }
}

impl HandlerContext {
    fn permissions(&self) -> Option<Permissions> {
        self.permissions.lock().unwrap().clone()
    }
}

impl From<ReadEventBody> for MatrixEventFilterInput {
    fn from(body: ReadEventBody) -> Self {
        Self {
            event_type: body.event_type,
            state_key: body.state_key,
            content: MatrixEventContent::default(),
        }
    }
}

impl From<SendEventBody> for MatrixEventFilterInput {
    fn from(body: SendEventBody) -> Self {
        Self {
            event_type: body.event_type,
            state_key: body.state_key,
            content: serde_json::from_value(body.content).unwrap_or_default(),
        }
    }
}
