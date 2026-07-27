use http::header::{CONTENT_TYPE, HeaderValue};
use topcoat_core::{context::Cx, error::Result};

use crate::{Body, IntoResponse, Response};

/// WebAssembly response wrapper.
///
/// Wrap any value convertible into a [`Body`] (such as a `&'static [u8]` from
/// `include_bytes!`) to reply with `Content-Type: application/wasm`. Use it
/// from a [`route`](../topcoat_router_macro/attr.route.html) that serves a
/// module by hand: the streaming compilation entry points
/// (`WebAssembly.compileStreaming` and `instantiateStreaming`) reject any
/// response that does not carry this exact media type.
#[derive(Debug, Clone, Copy, Default)]
#[must_use]
pub struct Wasm<T>(pub T);

impl<T> From<T> for Wasm<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> IntoResponse for Wasm<T>
where
    T: Into<Body>,
{
    fn into_response(self, cx: &Cx) -> Result<Response> {
        (
            [(CONTENT_TYPE, HeaderValue::from_static("application/wasm"))],
            self.0.into(),
        )
            .into_response(cx)
    }
}

#[cfg(test)]
mod tests {
    use topcoat_core::context::Cx;

    use super::*;
    use crate::to_bytes;

    #[tokio::test]
    async fn into_response_sets_wasm_content_type() {
        let response = Wasm(b"\0asm\x01\0\0\0".as_slice())
            .into_response(&Cx::default())
            .expect("response builds");

        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .map(http::HeaderValue::as_bytes),
            Some(b"application/wasm".as_slice())
        );

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("reading the response body");
        assert_eq!(&body[..], b"\0asm\x01\0\0\0");
    }
}
