use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Buf, Bytes, BytesMut};
use http_body::{Body as _, Frame, SizeHint};
use http_body_util::BodyExt;
use http_body_util::combinators::UnsyncBoxBody;

use crate::{Error, Result};

/// The error type a boxed body reports.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// A streaming HTTP body. Streaming is the representation; buffering is a convenience that always takes an explicit limit.
///
/// There is deliberately no method that reads without a limit (the old axum adapter read with `usize::MAX`).
pub struct Body(UnsyncBoxBody<Bytes, BoxError>);

impl Body {
    /// A body with no bytes.
    pub fn empty() -> Self {
        Self::new(http_body_util::Empty::<Bytes>::new())
    }

    /// A body holding exactly these bytes.
    pub fn from_bytes(bytes: impl Into<Bytes>) -> Self {
        Self::new(http_body_util::Full::new(bytes.into()))
    }

    /// Wrap any `http_body::Body` over `Bytes`.
    pub fn new<B>(body: B) -> Self
    where
        B: http_body::Body<Data = Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        Self(body.map_err(Into::into).boxed_unsync())
    }

    /// Read the whole body, failing with [`Error::BodyTooLarge`] as soon as more than `limit` bytes are seen or announced.
    pub async fn bytes(mut self, limit: usize) -> Result<Bytes> {
        if self.size_hint().lower() > limit as u64 {
            return Err(Error::BodyTooLarge { limit });
        }
        let mut out = BytesMut::new();
        while let Some(frame) = self.frame().await {
            let frame = frame.map_err(Error::Body)?;
            if let Ok(mut data) = frame.into_data() {
                if data.remaining() > limit - out.len() {
                    return Err(Error::BodyTooLarge { limit });
                }
                out.extend_from_slice(&data.copy_to_bytes(data.remaining()));
            }
        }
        Ok(out.freeze())
    }

    /// Like [`Body::bytes`], then require valid UTF-8.
    pub async fn text(self, limit: usize) -> Result<String> {
        String::from_utf8(self.bytes(limit).await?.to_vec()).map_err(|_| Error::NotUtf8)
    }
}

impl Default for Body {
    fn default() -> Self {
        Self::empty()
    }
}

impl std::fmt::Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Body")
            .field("size_hint", &self.size_hint())
            .finish()
    }
}

impl http_body::Body for Body {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Bytes>, BoxError>>> {
        Pin::new(&mut self.0).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.0.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use http_body_util::StreamBody;

    use super::*;

    fn chunked(parts: &[&'static str]) -> Body {
        let frames: Vec<std::result::Result<Frame<Bytes>, Infallible>> = parts
            .iter()
            .map(|p| Ok(Frame::data(Bytes::from(*p))))
            .collect();
        Body::new(StreamBody::new(futures_util::stream::iter(frames)))
    }

    #[tokio::test]
    async fn reads_a_body_under_the_limit() {
        assert_eq!(Body::from_bytes("hello").bytes(5).await.unwrap(), "hello");
    }

    #[tokio::test]
    async fn empty_body_is_empty() {
        assert!(Body::empty().bytes(0).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn announced_size_over_the_limit_is_refused_before_reading() {
        let err = Body::from_bytes("hello").bytes(4).await.unwrap_err();
        assert!(matches!(err, Error::BodyTooLarge { limit: 4 }), "{err:?}");
    }

    #[tokio::test]
    async fn unannounced_size_over_the_limit_is_refused_while_streaming() {
        // A stream has no lower size hint, so only the running count can catch this.
        assert_eq!(chunked(&["abc", "de"]).size_hint().lower(), 0);
        let err = chunked(&["abc", "de"]).bytes(4).await.unwrap_err();
        assert!(matches!(err, Error::BodyTooLarge { limit: 4 }), "{err:?}");
    }

    #[tokio::test]
    async fn chunks_are_joined_in_order_and_exact_limit_is_allowed() {
        assert_eq!(chunked(&["abc", "de"]).bytes(5).await.unwrap(), "abcde");
    }

    #[tokio::test]
    async fn invalid_utf8_is_an_error_not_a_lossy_string() {
        let err = Body::from_bytes(vec![0xff, 0xfe])
            .text(8)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::NotUtf8), "{err:?}");
    }

    #[tokio::test]
    async fn a_failing_stream_surfaces_as_a_body_error() {
        let frames: Vec<std::result::Result<Frame<Bytes>, std::io::Error>> = vec![
            Ok(Frame::data(Bytes::from("a"))),
            Err(std::io::Error::other("reset")),
        ];
        let body = Body::new(StreamBody::new(futures_util::stream::iter(frames)));
        let err = body.bytes(10).await.unwrap_err();
        assert!(matches!(err, Error::Body(_)), "{err:?}");
    }
}
