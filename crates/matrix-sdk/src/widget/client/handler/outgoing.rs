//! Outgoing requests (client -> widget).

use crate::widget::messages::{
    to_widget::{CapabilitiesResponse, CapabilitiesUpdatedRequest, RequestType, ResponseType},
    Empty, OpenIdResponse, Request as RequestContent,
};

pub(crate) type Response<T> = Result<T, String>;

pub(crate) trait Request: Sized + Send + Sync + 'static {
    type Response;

    fn into_request_type(self) -> RequestType;
    fn from_response_type(response: ResponseType) -> Option<Response<Self::Response>>;
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

                fn into_request_type(self) -> RequestType {
                    RequestType::$request(RequestContent::new(self.0))
                }

                fn from_response_type(response: ResponseType) -> Option<Response<Self::Response>> {
                    match response {
                        ResponseType::$request(r) => Some(r.response()),
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
