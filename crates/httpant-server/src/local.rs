// SPDX-License-Identifier: MIT OR Apache-2.0
//! Handlers that are not `Send` (ROADMAP row 9, design `docs/ACTIX-ADAPTER-NOTES.md`).
//!
//! The one service every adapter runs is `Send`, and every guarantee HTTPAnt gives (the body limits, the shutdown switch, the ACME prefix, the
//! trusted-proxy `Origin`, the HTTPS redirect) is a wrapper around it. A local route therefore does **not** get a service of its own: the router
//! holds, for each local route, an ordinary `Send` handler (the shim below) that hands the request to the handler instance that lives on the
//! worker thread, and gets the `Send` response back. The shim runs on the worker's own thread (actix polls the whole request there), so the
//! instance is found in a thread-local, and the `!Send` future is spawned on that thread's `LocalSet`. Routing, `405`/`Allow`, and all five
//! wrappers are the ones every other route uses: a local route cannot bypass what it is inside of.

use std::cell::RefCell;
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use httpant_core::http::{Method, StatusCode};
use httpant_core::{FromRequest, IntoResponse, Request, Response, Router};
use tokio::task::JoinHandle;

/// The future of a local handler: not `Send`.
pub type LocalFuture = Pin<Box<dyn Future<Output = Response>>>;
type LocalFn = Rc<dyn Fn(Request) -> LocalFuture>;
type Factory = Arc<dyn Fn() -> LocalFn + Send + Sync>;

#[doc(hidden)]
pub struct NoArgs;
#[doc(hidden)]
pub struct WithRequest;
#[doc(hidden)]
pub struct Extracted<T>(PhantomData<fn() -> T>);

/// What [`LocalHandlers::route`] runs: the counterpart of [`Handler`](httpant_core::Handler) without `Send`. An async function (or closure) that
/// takes nothing, the [`Request`], or from one to four extractors ([`Path`](httpant_core::Path), [`Query`](httpant_core::Query),
/// [`PathParams`](httpant_core::PathParams) or your own [`FromRequest`]), built from the request before the handler runs; one that fails is
/// answered instead and the handler does not run, exactly as for a handler that is `Send`. Its future need not be `Send`.
pub trait LocalHandler<Marker>: 'static {
    /// Run the handler.
    fn call(&self, req: Request) -> LocalFuture;
}

impl<F, Fut, R> LocalHandler<NoArgs> for F
where
    F: Fn() -> Fut + 'static,
    Fut: Future<Output = R> + 'static,
    R: IntoResponse + 'static,
{
    fn call(&self, _req: Request) -> LocalFuture {
        let fut = self();
        Box::pin(async move { fut.await.into_response() })
    }
}

impl<F, Fut, R> LocalHandler<WithRequest> for F
where
    F: Fn(Request) -> Fut + 'static,
    Fut: Future<Output = R> + 'static,
    R: IntoResponse + 'static,
{
    fn call(&self, req: Request) -> LocalFuture {
        let fut = self(req);
        Box::pin(async move { fut.await.into_response() })
    }
}

macro_rules! local_extractors {
    ($($t:ident),+) => {
        #[allow(non_snake_case)]
        impl<F, Fut, R, $($t),+> LocalHandler<Extracted<($($t,)+)>> for F
        where
            F: Fn($($t),+) -> Fut + 'static,
            Fut: Future<Output = R> + 'static,
            R: IntoResponse + 'static,
            $($t: FromRequest),+
        {
            fn call(&self, req: Request) -> LocalFuture {
                $(let $t = match $t::from_request(&req) {
                    Ok(v) => v,
                    Err(res) => return Box::pin(async move { *res }),
                };)+
                drop(req);
                let fut = self($($t),+);
                Box::pin(async move { fut.await.into_response() })
            }
        }
    };
}

local_extractors!(T1);
local_extractors!(T1, T2);
local_extractors!(T1, T2, T3);
local_extractors!(T1, T2, T3, T4);

thread_local! {
    /// The handler instances of this worker thread, by route index; set by [`LocalHandlers::install`] when the worker starts.
    static INSTALLED: RefCell<Vec<LocalFn>> = const { RefCell::new(Vec::new()) };
}

/// The local routes of one server: a factory per route, because a handler that is not `Send` cannot be shared between threads. The adapter
/// runs each factory once on every worker thread it starts, so every worker has handlers (and state) of its own.
#[derive(Clone, Default)]
pub struct LocalHandlers {
    factories: Vec<Factory>,
}

impl fmt::Debug for LocalHandlers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LocalHandlers({} routes)", self.factories.len())
    }
}

impl LocalHandlers {
    /// No local routes.
    pub fn new() -> Self {
        Self::default()
    }

    /// No local route has been added: an adapter has nothing to do for them, and any adapter may serve.
    pub fn is_empty(&self) -> bool {
        self.factories.is_empty()
    }

    /// How many local routes there are.
    pub fn len(&self) -> usize {
        self.factories.len()
    }

    /// Add a local route to `router` and remember its factory. `factory` runs once per worker thread and returns the handler, an async function
    /// (see [`LocalHandler`]: nothing, the [`Request`], or up to four extractors) whose future need not be `Send` and which may keep `Rc` and `RefCell` state. Serving these routes is refused with
    /// [`Error::LocalHandlersNotSupported`](httpant_core::Error) by an adapter whose
    /// [`Capabilities::local_handlers`](httpant_core::Capabilities) is not `Yes`.
    pub fn route<H, M>(
        &mut self,
        router: Router,
        method: Method,
        path: &str,
        factory: impl Fn() -> H + Send + Sync + 'static,
    ) -> Router
    where
        H: LocalHandler<M>,
    {
        let index = self.factories.len();
        self.factories.push(Arc::new(move || {
            let handler = factory();
            let local: LocalFn = Rc::new(move |req| handler.call(req));
            local
        }));
        router.route(method, path, move |req: Request| dispatch(index, req))
    }

    /// Build this thread's handler instances. An adapter calls it on each worker thread before that worker serves its first request.
    pub fn install(&self) {
        let built: Vec<LocalFn> = self.factories.iter().map(|f| f()).collect();
        INSTALLED.with(|cell| *cell.borrow_mut() = built);
    }
}

/// Ends the spawned handler when the request is abandoned (a body limit, the end of the shutdown grace, a client that went away): the
/// `Send` path drops a handler's future in those cases, and a local handler must stop the same way.
struct EndOnDrop(JoinHandle<()>);

impl Drop for EndOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn unavailable() -> Response {
    let mut res = Response::new(httpant_core::Body::empty());
    *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    res
}

/// The `Send` handler the router holds for local route `index`.
async fn dispatch(index: usize, req: Request) -> Response {
    // Looked up and moved into the task in one step, so no `Rc` is held across the await below.
    let (tx, rx) = tokio::sync::oneshot::channel();
    let spawned = INSTALLED.with(|cell| {
        let handler = cell.borrow().get(index).cloned()?;
        Some(tokio::task::spawn_local(async move {
            let _ = tx.send(handler(req).await);
        }))
    });
    let Some(task) = spawned else {
        // Not on a worker thread that installed the handlers: the adapter was never meant to run this route (`bind` refuses it).
        return unavailable();
    };
    let _end = EndOnDrop(task);
    // The handler panicked or was cancelled: the sender was dropped without an answer.
    rx.await.unwrap_or_else(|_| unavailable())
}
