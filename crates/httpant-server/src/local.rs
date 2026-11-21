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
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use httpant_core::http::{Method, StatusCode};
use httpant_core::{IntoResponse, Request, Response, Router};
use tokio::task::JoinHandle;

type LocalFuture = Pin<Box<dyn Future<Output = Response>>>;
type LocalFn = Rc<dyn Fn(Request) -> LocalFuture>;
type Factory = Arc<dyn Fn() -> LocalFn + Send + Sync>;

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
    /// of the [`Request`] whose future need not be `Send` and which may keep `Rc` and `RefCell` state. Serving these routes is refused with
    /// [`Error::LocalHandlersNotSupported`](httpant_core::Error) by an adapter whose
    /// [`Capabilities::local_handlers`](httpant_core::Capabilities) is not `Yes`.
    pub fn route<H, Fut, R>(
        &mut self,
        router: Router,
        method: Method,
        path: &str,
        factory: impl Fn() -> H + Send + Sync + 'static,
    ) -> Router
    where
        H: Fn(Request) -> Fut + 'static,
        Fut: Future<Output = R> + 'static,
        R: IntoResponse + 'static,
    {
        let index = self.factories.len();
        self.factories.push(Arc::new(move || {
            let handler = factory();
            let local: LocalFn = Rc::new(move |req| {
                let fut = handler(req);
                Box::pin(async move { fut.await.into_response() })
            });
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
