use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::CookieJar;
use http::header;
use tokio::sync::Mutex;
use tower::{Layer, Service};

use crate::{db, helpers, state::AppState, views};

// ---------------------------------------------------------------------------
// HTTP metrics layer  (OTel semantic conventions)
// ---------------------------------------------------------------------------

/// HTTP server metrics following OpenTelemetry semantic conventions.
///
/// Records:
/// - `http.server.request.duration` histogram (seconds) with attributes:
///   `http.request.method`, `http.route`, `http.response.status_code`
/// - Uses `axum::extract::MatchedPath` for low-cardinality route labels.
pub async fn http_metrics(request: Request, next: Next) -> Response {
    let method = request.method().as_str().to_owned();
    let route = request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map(|m| m.as_str().to_owned())
        .unwrap_or_else(|| "unknown".into());

    let start = std::time::Instant::now();
    let response = next.run(request).await;
    let status = response.status().as_u16().to_string();
    let elapsed = start.elapsed().as_secs_f64();

    let m = crate::metrics::get();
    let attrs = m.attrs(&[
        opentelemetry::KeyValue::new("http.request.method", method),
        opentelemetry::KeyValue::new("http.route", route),
        opentelemetry::KeyValue::new("http.response.status_code", status),
    ]);
    m.http_server_request_duration.record(elapsed, &attrs);

    response
}

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
                Ok(Some(mut user)) => {
                    if !state.config.mode.is_team() {
                        user.team_id = None;
                        user.team_name = None;
                    }
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
        Ok(Some(mut user)) => {
            // In user mode, ignore team fields from Keycloak
            if !state.config.mode.is_team() {
                user.team_id = None;
                user.team_name = None;
            }
            // In team mode, block users without a team
            if state.config.mode.is_team() && user.team_id.is_none() {
                let ctf_title = helpers::get_ctf_full_name(&state);
                let user_label = user
                    .name
                    .clone()
                    .or(user.email.clone())
                    .unwrap_or_else(|| user.sub.clone());
                let html = views::render_to_html(move || {
                    leptos::prelude::view! {
                        <views::NoTeamPage ctf_title=ctf_title user_label=user_label/>
                    }
                });
                return Html(html).into_response();
            }
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Ok(None) | Err(_) => Redirect::to("/login").into_response(),
    }
}

/// Axum middleware that loads the user from the session cookie when
/// present but does **not** redirect to `/login` when anonymous.
/// The handler receives `Option<Extension<db::User>>`.
pub async fn optional_auth(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    mut request: Request,
    next: Next,
) -> Response {
    if let Some(cookie) = jar.get(&state.config.session_cookie_name) {
        let token = cookie.value().to_owned();
        if let Ok(Some(mut user)) = db::get_user_by_session(&state.db, &token).await {
            if !state.config.mode.is_team() {
                user.team_id = None;
                user.team_name = None;
            }
            request.extensions_mut().insert(user);
        }
    }
    next.run(request).await
}

/// Axum middleware that returns 500 Internal Server Error when the
/// configured CTF resource does not exist in the cluster.
pub async fn require_ctf_exists(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    if helpers::ctf_exists(&state) {
        return next.run(request).await;
    }
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        format!(
            "CTF '{}' not found in namespace '{}'",
            state.config.ctf_name, state.config.ctf_namespace
        ),
    )
        .into_response()
}

/// Axum middleware that blocks access to protected data endpoints when the
/// CTF has not opened yet.  Page handlers (index, scoreboard) render their
/// own "not open" views, but this middleware protects the remaining API /
/// data routes (instance-state, file downloads, server-functions, etc.)
/// so they never leak data before the competition begins.
pub async fn require_ctf_open(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let schedule = helpers::get_ctf_schedule(&state);
    if helpers::is_ctf_open(&schedule) {
        return next.run(request).await;
    }
    (axum::http::StatusCode::FORBIDDEN, "CTF has not started yet").into_response()
}
