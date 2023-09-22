use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Header {
    pub request_id: String,
    pub widget_id: String,
}

impl Header {
    pub fn new(request_id: impl Into<String>, widget_id: impl Into<String>) -> Self {
        Self { request_id: request_id.into(), widget_id: widget_id.into() }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct WithHeader<T> {
    pub(crate) header: Header,
    pub(crate) data: T,
}

impl<T> WithHeader<T> {
    pub(crate) fn new(header: Header, data: T) -> Self {
        Self { header, data }
    }

    pub(crate) fn map<U>(self, data: U) -> WithHeader<U> {
        let Self { header, .. } = self;
        WithHeader { header, data }
    }
}
