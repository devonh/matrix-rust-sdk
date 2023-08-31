//! Strong data types for validation of the incoming requests (widget -> client)
//! and proper response generation for them.

use std::ops::Deref;

use crate::widget::messages::{
    from_widget::{self, Action, SupportedApiVersionsResponse},
    Action as ActionType, Empty, Header, Message, MessageKind, OpenIdRequest, OpenIdResponse,
    Request as RequestBody,
};

// Generates a bunch of request types and their responses. In particular:
// - A `Request` enum that contains all **valid** incoming request types.
// - Separate structures for each valid incoming request along with the data
//   that each of them contains.
// - A function that maps a valid incoming request to a proper response that
//   could then be used to construct an actual response message later on.
macro_rules! generate_requests {
    ($($request:ident($request_data:ty) -> $response_data:ty),* $(,)?) => {
        #[derive(Debug, Clone)]
        pub(crate) enum Request {
            $(
                $request($request),
            )*
        }

        impl Request {
            pub(crate) fn new(header: Header, action: Action) -> Option<Self> {
                match action {
                    $(
                        from_widget::Action::$request(MessageKind::Request(r)) => {
                            Some(Self::$request($request(WithHeader::new(header, r))))
                        }
                    )*
                    _ => None,
                }
            }

            pub(crate) fn fail(self, error: impl Into<String>) -> Response {
                match self {
                    $(
                        Self::$request(r) => r.map(Err(error.into())),
                    )*
                }
            }
        }

        $(
            #[derive(Debug, Clone)]
            pub(crate) struct $request(WithHeader<RequestBody<$request_data>>);

            impl $request {
                pub(crate) fn map(self, response_data: Result<$response_data, String>) -> Response {
                    Response {
                        data: from_widget::Action::$request(self.0.data.map(response_data)),
                        header: self.0.header,
                    }
                }
            }

            impl Deref for $request {
                type Target = $request_data;

                fn deref(&self) -> &Self::Target {
                    &self.0.data.content
                }
            }
        )*
    };
}

// <the name of the from_widget::Action variant>(<the data type inside the
// action>) -> <response type>
generate_requests! {
    GetSupportedApiVersion(Empty) -> SupportedApiVersionsResponse,
    ContentLoaded(Empty) -> Empty,
    GetOpenId(OpenIdRequest) -> OpenIdResponse,
    SendEvent(from_widget::SendEventRequest) -> from_widget::SendEventResponse,
    ReadEvent(from_widget::ReadEventRequest) -> from_widget::ReadEventResponse,
}

/// Represents a response that could be sent back to a widget.
pub(crate) type Response = WithHeader<Action>;

/// We can construct a `Message` once we get a valid `Response`.
impl From<Response> for Message {
    fn from(response: Response) -> Self {
        Self { header: response.header, action: ActionType::FromWidget(response.data) }
    }
}

/// `data` and a `header` that is associated with it. This ensures that we never
/// handle a request without a header that is associated with it. Likewise, we
/// ensure that the responses come with the request's original header. The
/// fields are private by design so that the user can't modify any of the fields
/// outside of this module by accident. It also ensures that we can only
/// construct this data type from within this module.
#[derive(Debug, Clone)]
pub(crate) struct WithHeader<T> {
    header: Header,
    data: T,
}

impl<T> WithHeader<T> {
    fn new(header: Header, data: T) -> Self {
        Self { header, data }
    }
}
