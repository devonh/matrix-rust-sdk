//! Outgoing requests (client -> widget).

use crate::widget::messages::{
    to_widget::{
        CapabilitiesResponse, CapabilitiesUpdatedRequest, SupportedRequest, SupportedResponse,
    },
    Empty, OpenIdResponse, Request as GenericRequest,
};

pub(crate) type Response<T> = Result<T, String>;

// TODO: This trait could be improved and restricted even more by making sure
// that `into_action()` only allows types that e.g. implement `T: AsRequest` and
// `extract_response` only accept `T: AsResponse`. Though for this both such
// traits must be introduced and implemented.
pub(crate) trait Request: Sized + Send + Sync + 'static {
    type Response;

    fn into_request(self) -> SupportedRequest;
    fn extract_response(reply: SupportedResponse) -> Option<Response<Self::Response>>;
}

macro_rules! generate_requests {
    ($($request:ident($request_data:ty) -> $response_data:ty),* $(,)?) => {
        $(
            #[derive(Debug, Clone)]
            pub(crate) struct $request($request_data);

            impl $request {
                pub(crate) fn new(data: $request_data) -> Self {
                    Self(data)
                }
            }

            impl Request for $request {
                type Response = $response_data;

                fn into_request(self) -> SupportedRequest {
                    SupportedRequest::$request(GenericRequest::new(self.0))
                }

                fn extract_response(reply: SupportedResponse) -> Option<Response<Self::Response>> {
                    match reply {
                        SupportedResponse::$request(r) => Some(r.response()),
                        _ => None,
                    }
                }
            }
        )*
    };
}

generate_requests! {
    CapabilitiesRequest(Empty) -> CapabilitiesResponse,
    CapabilitiesUpdate(CapabilitiesUpdatedRequest) -> Empty,
    OpenIdCredentialsUpdate(OpenIdResponse) -> Empty,
    SendEvent(serde_json::Value) -> Empty,
}
