use std::ops::Not;

use ruma::{
    api::client::account::request_openid_token::v3::Request as MatrixOpenIdRequest,
    events::{AnySyncTimelineEvent, AnyTimelineEvent},
    serde::Raw,
};
use tokio::sync::{mpsc, oneshot};
use tracing::info;

use super::handler::{OpenIdDecision, OpenIdStatus};
use crate::{
    event_handler::EventHandlerDropGuard,
    room::Room,
    widget::{
        filter::{any_matches, EventFilter, MatrixEventFilterInput},
        messages::OpenIdState,
        Permissions, PermissionsProvider,
    },
};

#[derive(Debug)]
pub(crate) struct Driver<T> {
    /// The room this driver is attached to.
    ///
    /// Expected to be a room the user is a member of (not a room in invited or
    /// left state).
    pub(super) room: Room,
    permissions_provider: T,
    event_handler_handle: Option<EventHandlerDropGuard>,
}

impl<T> Driver<T> {
    pub(crate) fn new(room: Room, permissions_provider: T) -> Self {
        Self { room, permissions_provider, event_handler_handle: None }
    }

    pub(crate) async fn initialize(
        &mut self,
        permissions: Permissions,
    ) -> (Permissions, Option<mpsc::UnboundedReceiver<Raw<AnyTimelineEvent>>>)
    where
        T: PermissionsProvider,
    {
        let permissions = self.permissions_provider.acquire_permissions(permissions).await;
        let listener = permissions
            .read
            .is_empty()
            .not()
            .then(|| self.setup_matrix_event_handler(&permissions.read));
        (permissions, listener)
    }

    pub(crate) fn get_openid(&self, request_id: String) -> OpenIdStatus {
        let user_id = self.room.own_user_id().to_owned();
        let client = self.room.client.clone();
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let _ = tx.send(
                client
                    .send(MatrixOpenIdRequest::new(user_id), None)
                    .await
                    .map(|res| {
                        OpenIdDecision::Allowed(OpenIdState {
                            id: request_id,
                            token: res.access_token,
                            expires_in_seconds: res.expires_in.as_secs() as usize,
                            server: res.matrix_server_name.to_string(),
                            kind: res.token_type.to_string(),
                        })
                    })
                    .unwrap_or(OpenIdDecision::Blocked),
            );
        });

        // TODO: getting an OpenId token generally has multiple phases as per the JS
        // implementation of the `ClientWidgetAPI`, e.g. the `MatrixDriver`
        // would normally have some state, so that if a token is requested
        // multiple times, it may return/resolve the token right away.
        // Currently, we assume that we always request a new token. Fix it later.
        OpenIdStatus::Pending(rx)
    }

    fn setup_matrix_event_handler(
        &mut self,
        filters: &[EventFilter],
    ) -> mpsc::UnboundedReceiver<Raw<AnyTimelineEvent>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let room_id = self.room.room_id().to_owned();
        let filters = filters.to_owned();
        let callback = move |raw_ev: Raw<AnySyncTimelineEvent>| {
            let (filter, tx) = (filters.clone(), tx.clone());
            if let Ok(ev) = raw_ev.deserialize_as::<MatrixEventFilterInput>() {
                any_matches(&filter, &ev).then(|| {
                    info!("received event for room: {room_id}");
                    // deserialize should be possible if Raw<AnySyncTimelineEvent> is possible
                    let mut ev_value = raw_ev.deserialize_as::<serde_json::Value>().unwrap();
                    let ev_obj = ev_value.as_object_mut().unwrap();
                    ev_obj.insert("room_id".to_owned(), room_id.to_string().into());
                    let ev_with_room_id =
                        serde_json::from_value::<Raw<AnyTimelineEvent>>(ev_value).unwrap();
                    info!("final Event: {}", ev_with_room_id.json());

                    tx.send(ev_with_room_id)
                });
            }
            async {}
        };

        let handle = self.room.add_event_handler(callback);
        let drop_guard = self.room.client().event_handler_drop_guard(handle);
        self.event_handler_handle = Some(drop_guard);
        rx
    }
}
