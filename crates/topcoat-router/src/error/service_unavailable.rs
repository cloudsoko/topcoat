use http::{HeaderValue, StatusCode};
use topcoat_core::{context::Cx, error::Result};

use crate::{IntoResponse, Response};

/// Builds a service-unavailable (HTTP 503) response with a `Retry-After` hint.
///
/// Use this when the server is **temporarily at capacity** — load shedding,
/// admission control, a dependency that is saturated. It is deliberately
/// distinct from [`internal_server_error`](crate::error::internal_server_error):
/// a 500 tells a client (and a load balancer, and an on-call engineer) that
/// something is *broken*, and answering "broken" when the truth is "busy" is
/// both a worse user experience and a misleading signal to anything
/// automated.
///
/// `Retry-After` is the whole point of the type — a bare 503 tells a caller to
/// go away without saying when to come back, so every well-behaved client
/// invents its own backoff and they all synchronise.
///
/// # Examples
///
/// ```rust
/// use topcoat::Result;
/// use topcoat::context::Cx;
/// use topcoat::router::error::service_unavailable;
/// # fn try_admit() -> Option<()> { Some(()) }
/// # struct View;
/// # fn render() -> View { View }
///
/// async fn page(_cx: &Cx) -> Result<View> {
///     let Some(_permit) = try_admit() else {
///         return Err(service_unavailable(2).into());
///     };
///     Ok(render())
/// }
/// ```
#[must_use]
pub fn service_unavailable(retry_after_secs: u64) -> ServiceUnavailableError {
    ServiceUnavailableError::new(retry_after_secs)
}

/// A service-unavailable response carried as the `Err` variant of a handler
/// `Result`.
///
/// Construct one with [`service_unavailable`].
#[derive(Debug)]
pub struct ServiceUnavailableError {
    retry_after_secs: u64,
}

impl ServiceUnavailableError {
    fn new(retry_after_secs: u64) -> Self {
        Self { retry_after_secs }
    }

    /// The `Retry-After` value this response will carry, in seconds.
    #[must_use]
    pub fn retry_after_secs(&self) -> u64 {
        self.retry_after_secs
    }
}

impl std::fmt::Display for ServiceUnavailableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "service unavailable (retry after {}s)", self.retry_after_secs)
    }
}

impl std::error::Error for ServiceUnavailableError {}

impl IntoResponse for ServiceUnavailableError {
    fn into_response(self, cx: &Cx) -> Result<Response> {
        let mut response =
            (StatusCode::SERVICE_UNAVAILABLE, "service unavailable").into_response(cx)?;
        if let Ok(value) = HeaderValue::from_str(&self.retry_after_secs.to_string()) {
            response.headers_mut().insert(http::header::RETRY_AFTER, value);
        }
        Ok(response)
    }
}
