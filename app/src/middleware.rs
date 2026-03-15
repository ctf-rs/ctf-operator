use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::CookieJar;
use http::header;
use tokio::sync::Mutex;
use tower::{Layer, Service};

use crate::{db, state::AppState};

// ---------------------------------------------------------------------------
// Tower Layer middleware (used with `#[middleware(crate::middleware::AuthLayer)]`)
// ---------------------------------------------------------------------------

/// Tower `Layer` that resolves the current user from the session cookie and
/// injects the `db::User` as an axum `Extension` on the request.
///
/// Requires `Extension<Arc<AppState>>` to be present on the axum `Router`.
#[derive(Debug)]
pub struct AuthLayer;

impl<S> Layer<S> for AuthLayer {
    type Service = AuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthService {
            inner: Arc::new(Mutex::new(inner)),
        }
    }
}

/// The inner service produced by [`AuthLayer`].
///
/// Uses `Arc<Mutex<S>>` so that `AuthService` is `Clone` even when the wrapped
/// service is not (e.g. `BoxedService` produced by `#[server]`).
#[derive(Clone, Debug)]
pub struct AuthService<S> {
    inner: Arc<Mutex<S>>,
}

impl<S> Service<Request<Body>> for AuthService<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Readiness is deferred to the locked call site.
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let inner = Arc::clone(&self.inner);

        Box::pin(async move {
            let state = req.extensions().get::<Arc<AppState>>().cloned();
            let Some(state) = state else {
                return Ok(redirect_to_login());
            };

            let token = extract_session_cookie(req.headers(), &state.config.session_cookie_name);
            let Some(token) = token else {
                return Ok(redirect_to_login());
            };

            match db::get_user_by_session(&state.db, &token).await {
                Ok(Some(user)) => {
                    req.extensions_mut().insert(user);
                    let mut svc = inner.lock().await;
                    svc.call(req).await
                }
                _ => Ok(redirect_to_login()),
            }
        })
    }
}

fn redirect_to_login() -> Response<Body> {
    Response::builder()
        .status(302)
        .header("Location", "/login")
        .body(Body::empty())
        .expect("building redirect response should not fail")
}

pub(crate) fn extract_session_cookie(
    headers: &http::HeaderMap,
    cookie_name: &str,
) -> Option<String> {
    headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|s| {
                let s = s.trim();
                let (name, value) = s.split_once('=')?;
                if name.trim() == cookie_name {
                    Some(value.trim().to_owned())
                } else {
                    None
                }
            })
        })
}

// ---------------------------------------------------------------------------
// Axum function middleware (used for page routes via `from_fn_with_state`)
// ---------------------------------------------------------------------------

/// Axum middleware that resolves the current user from the session cookie.
/// If no valid session exists, redirects to `/login`.
/// Otherwise, injects the `User` as an extension so handlers can access it
/// via `Extension<db::User>`.
pub async fn require_auth(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    mut request: Request,
    next: Next,
) -> Response {
    let user = jar
        .get(&state.config.session_cookie_name)
        .map(|cookie| cookie.value().to_owned());

    let Some(token) = user else {
        return Redirect::to("/login").into_response();
    };

    match db::get_user_by_session(&state.db, &token).await {
        Ok(Some(user)) => {
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Ok(None) | Err(_) => Redirect::to("/login").into_response(),
    }
}
