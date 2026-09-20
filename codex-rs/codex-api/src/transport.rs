use crate::common::ResponseStream;
use crate::common::ResponsesApiRequest;
use crate::endpoint::ResponsesOptions;
use crate::error::ApiError;
use std::future::Future;
use std::pin::Pin;

/// Boxed future returned by a model response transport.
pub type ResponseTransportFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ResponseStream, ApiError>> + Send + 'a>>;

/// HTTP/SSE transport used to stream one model response.
///
/// The default implementation speaks the Responses API. Providers may supply a
/// different implementation without teaching the agent loop about another wire
/// format.
pub trait ResponseTransport: Send + Sync {
    fn stream_response(
        &self,
        request: ResponsesApiRequest,
        options: ResponsesOptions,
    ) -> ResponseTransportFuture<'_>;
}
