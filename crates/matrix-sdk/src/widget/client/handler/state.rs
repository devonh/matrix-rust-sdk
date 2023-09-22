//! Client-side state machine for handling incoming requests from a widget.

use std::sync::Arc;

use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{info, warn};

use super::{
    outgoing::{CapabilitiesRequest, CapabilitiesUpdate, OpenIdCredentialsUpdate, SendEvent},
    Capabilities, Error, OpenIdResponse, OpenIdStatus, Result,
};
use crate::widget::{
    client::{MatrixDriver, WidgetProxy},
    messages::{
        from_widget::{
            ApiVersion, SupportedApiVersionsResponse, SupportedRequest, SupportedResponse,
        },
        to_widget::{CapabilitiesResponse, CapabilitiesUpdatedRequest},
        Empty, WithHeader,
    },
    PermissionsProvider,
};

/// State of our client API state machine that handles incoming messages and
/// advances the state.
pub(super) struct State<T> {
    capabilities: Option<Capabilities>,
    widget: Arc<WidgetProxy>,
    client: MatrixDriver<T>,
}

impl<T: PermissionsProvider> State<T> {
    /// Creates a new [`Self`] with a given proxy and a matrix driver.
    pub(super) fn new(widget: Arc<WidgetProxy>, client: MatrixDriver<T>) -> Self {
        Self { capabilities: None, widget, client }
    }

    /// Start a task that will listen to the `rx` for new incoming requests from
    /// a widget and process them.
    pub(super) async fn listen(mut self, mut rx: UnboundedReceiver<WithHeader<SupportedRequest>>) {
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
    async fn handle(&mut self, msg: WithHeader<SupportedRequest>) -> Result<()> {
        match msg.data.clone() {
            SupportedRequest::GetSupportedApiVersion(req) => {
                let resp = req.map(Ok(SupportedApiVersionsResponse::new()));
                self.reply(msg.map(SupportedResponse::GetSupportedApiVersion(resp))).await?;
            }

            SupportedRequest::ContentLoaded(req) => {
                let (response, negotiate) =
                    match (self.widget.init_on_load(), self.capabilities.as_ref()) {
                        (true, None) => (Ok(Empty {}), true),
                        (true, Some(..)) => (Err("Already loaded".into()), false),
                        _ => (Ok(Empty {}), false),
                    };

                let resp = req.map(response);
                self.reply(msg.map(SupportedResponse::ContentLoaded(resp))).await?;

                if negotiate {
                    self.initialize().await?;
                }
            }

            SupportedRequest::GetOpenId(req) => {
                let (reply, handle) = match self.client.get_openid(msg.header.request_id.clone()) {
                    OpenIdStatus::Resolved(decision) => (decision.into(), None),
                    OpenIdStatus::Pending(handle) => (OpenIdResponse::Pending, Some(handle)),
                };

                let resp = req.map(Ok(reply));
                self.reply(msg.map(SupportedResponse::GetOpenId(resp))).await?;

                if let Some(handle) = handle {
                    let status = handle.await.map_err(|_| Error::WidgetDisconnected)?;
                    self.widget.send(OpenIdCredentialsUpdate::new(status.into())).await?;
                }
            }

            SupportedRequest::ReadEvent(req) => {
                let fut = self
                    .caps()?
                    .reader
                    .as_ref()
                    .ok_or(Error::custom("No permissions to read events"))?
                    .read(req.content.clone());
                let resp = req.map(Ok(fut.await?));
                self.reply(msg.map(SupportedResponse::ReadEvent(resp))).await?;
            }

            SupportedRequest::SendEvent(req) => {
                let fut = self
                    .caps()?
                    .sender
                    .as_ref()
                    .ok_or(Error::custom("No permissions to send events"))?
                    .send(req.content.clone());
                let resp = req.map(Ok(fut.await?));
                self.reply(msg.map(SupportedResponse::SendEvent(resp))).await?;
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

        // Initialise the capabilities with the desired capabilities.
        let mut capabilities = self.client.initialize(desired.clone()).await;

        // Subscribe to the events if the widget was granted such capabilities.
        // `take()` is fine here since we never rely upon this value again.
        if let Some(mut listener) = capabilities.listener.take() {
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
        self.capabilities = Some(capabilities);
        self.widget
            .send(CapabilitiesUpdate::new(CapabilitiesUpdatedRequest {
                requested: desired,
                approved: self.capabilities.as_ref().unwrap().into(),
            }))
            .await?;

        Ok(())
    }

    async fn reply(&self, response: WithHeader<SupportedResponse>) -> Result<()> {
        self.widget.reply(response).await.map_err(|_| Error::WidgetDisconnected)
    }

    fn caps(&mut self) -> Result<&mut Capabilities> {
        self.capabilities.as_mut().ok_or(Error::custom("Capabilities have not been negotiated"))
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

impl WithHeader<SupportedRequest> {
    fn fail(self, error: impl Into<String>) -> WithHeader<SupportedResponse> {
        match self.data.clone() {
            SupportedRequest::ContentLoaded(req) => {
                self.map(SupportedResponse::ContentLoaded(req.map(Err(error.into()))))
            }
            SupportedRequest::GetOpenId(req) => {
                self.map(SupportedResponse::GetOpenId(req.map(Err(error.into()))))
            }
            SupportedRequest::GetSupportedApiVersion(req) => {
                self.map(SupportedResponse::GetSupportedApiVersion(req.map(Err(error.into()))))
            }
            SupportedRequest::ReadEvent(req) => {
                self.map(SupportedResponse::ReadEvent(req.map(Err(error.into()))))
            }
            SupportedRequest::SendEvent(req) => {
                self.map(SupportedResponse::SendEvent(req.map(Err(error.into()))))
            }
        }
    }
}
