use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Request<T> {
    #[serde(rename = "data")]
    pub content: T,
}

impl<T> Request<T> {
    pub fn new(content: T) -> Self {
        Self { content }
    }

    pub fn map<R>(self, response: Result<R, String>) -> Response<T, R> {
        Response {
            request: self.content,
            response: match response {
                Ok(response) => ResponseBody::Success(response),
                Err(error) => ResponseBody::Failure(ErrorBody::new(error)),
            },
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Response<Req, Resp> {
    /// Responses contain the original request under the data field, just like
    /// the request itself.
    #[serde(rename = "data")]
    pub request: Req,
    pub response: ResponseBody<Resp>,
}

impl<Req, Resp: Clone> Response<Req, Resp> {
    pub fn response(&self) -> Result<Resp, String> {
        match &self.response {
            ResponseBody::Success(resp) => Ok(resp.clone()),
            ResponseBody::Failure(err) => Err(err.as_ref().to_owned()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum ResponseBody<T> {
    Success(T),
    Failure(ErrorBody),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ErrorBody {
    pub error: ErrorContent,
}

impl ErrorBody {
    pub fn new(message: impl AsRef<str>) -> Self {
        Self { error: ErrorContent { message: message.as_ref().to_owned() } }
    }
}

impl AsRef<str> for ErrorBody {
    fn as_ref(&self) -> &str {
        &self.error.message
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ErrorContent {
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Empty {}
