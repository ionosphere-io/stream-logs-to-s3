use std::{
    future::Future,
    io::{Error as IOError, IoSlice},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use async_compression::tokio::write::GzipEncoder;

use futures::{
    future::{pending, Pending},
    stream::{FuturesUnordered, Stream},
};

use tokio::{
    fs::File as TokioFile,
    io::AsyncWrite,
    time::{sleep, Sleep},
};

/// A union that allows us to either sleep or wait forever.
pub(crate) enum MaybeTimeout {
    Pending(Pin<Box<Pending<()>>>),
    Sleep(Pin<Box<Sleep>>),
}

impl MaybeTimeout {
    pub fn pending() -> Pin<Box<Self>> {
        Box::pin(Self::Pending(Box::pin(pending())))
    }

    pub fn sleep(duration: Duration) -> Pin<Box<Self>> {
        Box::pin(Self::Sleep(Box::pin(sleep(duration))))
    }
}

impl Future for MaybeTimeout {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        match *self.as_mut() {
            Self::Pending(ref mut p) => p.as_mut().poll(cx),
            Self::Sleep(ref mut s) => s.as_mut().poll(cx),
        }
    }
}

/// A task queue. This wraps a `FuturesUnordered` but modifies it so that it returns `Poll::Pending` when empty instead
/// of `Poll::Ok(None)`. This prevents a busy-wait loop when we have no tasks to do.
pub(crate) struct TaskQueue<Fut>
where
    Fut: Future,
{
    f: Pin<Box<FuturesUnordered<Fut>>>,
}

impl<Fut> TaskQueue<Fut>
where
    Fut: Future,
{
    pub fn new() -> Self {
        Self {
            f: Box::pin(FuturesUnordered::<Fut>::new()),
        }
    }

    pub fn push(&self, future: Fut) {
        self.f.push(future)
    }

    pub fn len(&self) -> usize {
        self.f.len()
    }
}

impl<Fut> Stream for TaskQueue<Fut>
where
    Fut: Future,
{
    type Item = <Fut as Future>::Output;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.f.is_empty() {
            Poll::Pending
        } else {
            self.as_mut().f.as_mut().poll_next(cx)
        }
    }
}

/// A union type for holding either a plain Tokio file or a Tokio file wrapped in a Gzip encoder.
pub(crate) enum MaybeCompressedFile {
    Gzip(GzipEncoder<TokioFile>),
    Uncompressed(TokioFile),
}

impl AsyncWrite for MaybeCompressedFile {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, IOError>> {
        match *self.as_mut() {
            Self::Gzip(ref mut g) => {
                tokio::pin!(g);
                g.poll_write(cx, buf)
            }
            Self::Uncompressed(ref mut u) => {
                tokio::pin!(u);
                u.poll_write(cx, buf)
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), IOError>> {
        match *self.as_mut() {
            Self::Gzip(ref mut g) => {
                tokio::pin!(g);
                g.poll_flush(cx)
            }
            Self::Uncompressed(ref mut u) => {
                tokio::pin!(u);
                u.poll_flush(cx)
            }
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), IOError>> {
        match *self.as_mut() {
            Self::Gzip(ref mut g) => {
                tokio::pin!(g);
                g.poll_shutdown(cx)
            }
            Self::Uncompressed(ref mut u) => {
                tokio::pin!(u);
                u.poll_shutdown(cx)
            }
        }
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<Result<usize, IOError>> {
        match *self.as_mut() {
            Self::Gzip(ref mut g) => {
                tokio::pin!(g);
                g.poll_write_vectored(cx, bufs)
            }
            Self::Uncompressed(ref mut u) => {
                tokio::pin!(u);
                u.poll_write_vectored(cx, bufs)
            }
        }
    }

    fn is_write_vectored(&self) -> bool {
        match self {
            Self::Gzip(ref g) => g.is_write_vectored(),
            Self::Uncompressed(ref u) => u.is_write_vectored(),
        }
    }
}
