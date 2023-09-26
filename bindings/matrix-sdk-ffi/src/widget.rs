use std::sync::Arc;

use matrix_sdk::{
    async_trait,
    widget::{MessageLikeEventFilter, StateEventFilter},
};

use crate::room::Room;

#[derive(uniffi::Record)]
pub struct Widget {
    /// Settings for the widget.
    pub settings: WidgetSettings,
    /// Communication channels with a widget.
    pub comm: Arc<WidgetComm>,
}

impl From<Widget> for matrix_sdk::widget::Widget {
    fn from(value: Widget) -> Self {
        let comm = &value.comm.0;
        Self {
            settings: value.settings.into(),
            comm: matrix_sdk::widget::Comm { from: comm.from.clone(), to: comm.to.clone() },
        }
    }
}

/// Information about a widget.
#[derive(uniffi::Record, Clone)]
pub struct WidgetSettings {
    /// Widget's unique identifier.
    pub id: String,
    /// Whether or not the widget should be initialized on load message
    /// (`ContentLoad` message), or upon creation/attaching of the widget to
    /// the SDK's state machine that drives the API.
    pub init_on_load: bool,
    /// This contains the url from the widget state event.
    /// In this url placeholders can be used to pass information from the client
    /// to the widget. Possible values are: `$widgetId`, `$parentUrl`,
    /// `$userId`, `$lang`, `$fontScale`, `$analyticsID`.
    ///
    /// # Examples
    ///
    /// e.g `http://widget.domain?username=$userId`
    /// will become: `http://widget.domain?username=@user_matrix_id:server.domain`.
    raw_url: String,
}

impl From<WidgetSettings> for matrix_sdk::widget::WidgetSettings {
    fn from(value: WidgetSettings) -> Self {
        let WidgetSettings { id, init_on_load, raw_url } = value;
        matrix_sdk::widget::WidgetSettings::new(id, init_on_load, raw_url)
    }
}

impl From<matrix_sdk::widget::WidgetSettings> for WidgetSettings {
    fn from(value: matrix_sdk::widget::WidgetSettings) -> Self {
        let matrix_sdk::widget::WidgetSettings { id, init_on_load, raw_url } = value;
        WidgetSettings { id, init_on_load, raw_url }
    }
}

/// Create the actual url that can be used to setup the WebView or IFrame
/// that contains the widget.
///
/// # Arguments
/// * `widget_settings` - The widget settings to generate the url for.
/// * `room` - A matrix room which is used to query the logged in username
/// * `props` - Propertis from the client that can be used by a widget to
///   adapt to the client. e.g. language, font-scale...
#[uniffi::export]
pub async fn generate_url(
    widget_settings: WidgetSettings,
    room: Arc<Room>,
    props: ClientProperties,
) -> Result<String, String> {
    matrix_sdk::widget::WidgetSettings::generate_url(
        &widget_settings.clone().into(),
        &room.inner,
        props.into(),
    )
    .await
    .map(|url| url.to_string())
    .map_err(|e| e.to_string())
}
/// `WidgetSettings` are usually created from a state event.
/// (currently unimplemented)
/// But in some cases the client wants to create custom `WidgetSettings`
/// for specific rooms based on other conditions.
/// This function returns a `WidgetSettings` object which can be used
/// to setup a widget using `run_client_widget_api`
/// and to generate the correct url for the widget.
///
/// # Arguments
/// * `base_path` the path to the app e.g. https://call.element.io.
/// * `id` the widget id.
/// * `embed` the embed param for the widget.
/// * `hide_header` for Element Call this defines if the branding header
///   should be hidden.
/// * `preload` if set, the lobby will be skipped and the widget will join
///   the call on the `io.element.join` action.
/// * `base_url` the url of the matrix homserver in use e.g. https://matrix-client.matrix.org.
/// * `analytics_id` can be used to pass a posthog id to element call.
#[uniffi::export]
pub fn new_virtual_element_call_widget(
    element_call_url: String,
    widget_id: String,
    parent_url: Option<String>,
    hide_header: Option<bool>,
    preload: Option<bool>,
    font_scale: Option<f64>,
    app_prompt: Option<bool>,
    skip_lobby: Option<bool>,
    confine_to_room: Option<bool>,
    fonts: Option<Vec<String>>,
    analytics_id: Option<String>,
) -> WidgetSettings {
    matrix_sdk::widget::WidgetSettings::new_virtual_element_call_widget(
        element_call_url,
        widget_id,
        parent_url,
        hide_header,
        preload,
        font_scale,
        app_prompt,
        skip_lobby,
        confine_to_room,
        fonts,
        analytics_id,
    )
    .into()
}
#[derive(uniffi::Record)]
pub struct ClientProperties {
    /// The language tag the client is set to e.g. en-us.
    pub language_tag: String,
    /// The client_id provides the widget with the option to behave differently
    /// for different clients. e.g org.example.ios.
    pub client_id: String,
    /// A string describing the theme (dark, light) or org.example.dark.
    pub theme: String,
}
impl From<ClientProperties> for matrix_sdk::widget::ClientProperties {
    fn from(value: ClientProperties) -> Self {
        let ClientProperties { language_tag, client_id, theme } = value;
        Self::new(language_tag, client_id, theme)
    }
}
/// Communication "pipes" with a widget.
#[derive(uniffi::Object)]
pub struct WidgetComm(matrix_sdk::widget::Comm);

/// Permissions that a widget can request from a client.
#[derive(uniffi::Record)]
pub struct WidgetPermissions {
    /// Types of the messages that a widget wants to be able to fetch.
    pub read: Vec<WidgetEventFilter>,
    /// Types of the messages that a widget wants to be able to send.
    pub send: Vec<WidgetEventFilter>,
    /// If a widget requests this capability, the client is not allowed
    /// to open the widget in a seperated browser.
    pub requires_client: bool,
}

impl From<WidgetPermissions> for matrix_sdk::widget::Permissions {
    fn from(value: WidgetPermissions) -> Self {
        Self {
            read: value.read.into_iter().map(Into::into).collect(),
            send: value.send.into_iter().map(Into::into).collect(),
            requires_client: value.requires_client,
        }
    }
}

impl From<matrix_sdk::widget::Permissions> for WidgetPermissions {
    fn from(value: matrix_sdk::widget::Permissions) -> Self {
        Self {
            read: value.read.into_iter().map(Into::into).collect(),
            send: value.send.into_iter().map(Into::into).collect(),
            requires_client: value.requires_client,
        }
    }
}

/// Different kinds of filters that could be applied to the timeline events.
#[derive(uniffi::Enum)]
pub enum WidgetEventFilter {
    /// Matches message-like events with the given `type`.
    MessageLikeWithType { event_type: String },
    /// Matches `m.room.message` events with the given `msgtype`.
    RoomMessageWithMsgtype { msgtype: String },
    /// Matches state events with the given `type`, regardless of `state_key`.
    StateWithType { event_type: String },
    /// Matches state events with the given `type` and `state_key`.
    StateWithTypeAndStateKey { event_type: String, state_key: String },
}

impl From<WidgetEventFilter> for matrix_sdk::widget::EventFilter {
    fn from(value: WidgetEventFilter) -> Self {
        match value {
            WidgetEventFilter::MessageLikeWithType { event_type } => {
                Self::MessageLike(MessageLikeEventFilter::WithType(event_type.into()))
            }
            WidgetEventFilter::RoomMessageWithMsgtype { msgtype } => {
                Self::MessageLike(MessageLikeEventFilter::RoomMessageWithMsgtype(msgtype))
            }
            WidgetEventFilter::StateWithType { event_type } => {
                Self::State(StateEventFilter::WithType(event_type.into()))
            }
            WidgetEventFilter::StateWithTypeAndStateKey { event_type, state_key } => {
                Self::State(StateEventFilter::WithTypeAndStateKey(event_type.into(), state_key))
            }
        }
    }
}

impl From<matrix_sdk::widget::EventFilter> for WidgetEventFilter {
    fn from(value: matrix_sdk::widget::EventFilter) -> Self {
        use matrix_sdk::widget::EventFilter as F;

        match value {
            F::MessageLike(MessageLikeEventFilter::WithType(event_type)) => {
                Self::MessageLikeWithType { event_type: event_type.to_string() }
            }
            F::MessageLike(MessageLikeEventFilter::RoomMessageWithMsgtype(msgtype)) => {
                Self::RoomMessageWithMsgtype { msgtype }
            }
            F::State(StateEventFilter::WithType(event_type)) => {
                Self::StateWithType { event_type: event_type.to_string() }
            }
            F::State(StateEventFilter::WithTypeAndStateKey(event_type, state_key)) => {
                Self::StateWithTypeAndStateKey { event_type: event_type.to_string(), state_key }
            }
        }
    }
}

#[uniffi::export(callback_interface)]
pub trait WidgetPermissionsProvider: Send + Sync {
    fn acquire_permissions(&self, permissions: WidgetPermissions) -> WidgetPermissions;
}

struct PermissionsProviderWrap(Arc<dyn WidgetPermissionsProvider>);

#[async_trait]
impl matrix_sdk::widget::PermissionsProvider for PermissionsProviderWrap {
    async fn acquire_permissions(
        &self,
        permissions: matrix_sdk::widget::Permissions,
    ) -> matrix_sdk::widget::Permissions {
        let this = self.0.clone();
        // This could require a prompt to the user. Ideally the callback
        // interface would just be async, but that's not supported yet so use
        // one of tokio's blocking task threads instead.
        tokio::task::spawn_blocking(move || this.acquire_permissions(permissions.into()).into())
            .await
            // propagate panics from the blocking task
            .unwrap()
    }
}

#[uniffi::export]
pub async fn run_client_widget_api(
    room: Arc<Room>,
    widget: Widget,
    permissions_provider: Box<dyn WidgetPermissionsProvider>,
) {
    let permissions_provider = PermissionsProviderWrap(permissions_provider.into());
    matrix_sdk::widget::run_client_widget_api(
        widget.into(),
        permissions_provider,
        room.inner.clone(),
    )
    .await
}
