//! Strong data types for validation of the incoming requests (widget -> client)
//! and proper response generation for them.

use std::ops::Deref;

use crate::widget::messages::{
    from_widget::{self, SupportedApiVersionsResponse, SupportedRequest},
    Empty, ErrorBody, ErrorMessage, Header, OpenIdResponse, Request as RequestBody, WithHeader,
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
            pub(crate) fn new(header: Header, req: SupportedRequest) -> Result<Self, ErrorResponse> {
                match req {
                    $(
                        SupportedRequest::$request(r) => {
                            Ok(Self::$request($request(WithHeader::new(header, r))))
                        }
                    )*
                }
            }

            pub(crate) fn fail(
                self,
                error: impl Into<String>,
            ) -> WithHeader<from_widget::SupportedResponse> {
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
                pub(crate) fn map(
                    self,
                    response_data: Result<$response_data, String>,
                ) -> WithHeader<from_widget::SupportedResponse> {
                    WithHeader {
                        data: from_widget::SupportedResponse::$request(self.0.data.map(response_data)),
                        header: self.0.header,
                    }
                }

                #[allow(dead_code)]
                pub(crate) fn id(&self) -> &str {
                    &self.0.header.request_id
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
    GetOpenId(Empty) -> OpenIdResponse,
    SendEvent(from_widget::SendEventRequest) -> from_widget::SendEventResponse,
    ReadEvent(from_widget::ReadEventRequest) -> from_widget::ReadEventResponse,
}

/// Represents a response that could be sent back to a widget.
pub(crate) type Response = WithHeader<from_widget::SupportedResponse>;

/// Represents an error message that we send to the widget in case of an invalid
/// message.
pub(crate) type ErrorResponse = WithHeader<ErrorBody>;

// Or an `ErrorMessage` if we get an invalid response.
impl From<ErrorResponse> for ErrorMessage {
    fn from(response: ErrorResponse) -> Self {
        Self {
            original_request: serde_json::to_value(&response).ok(),
            response: response.data.clone(),
        }
    }
}
