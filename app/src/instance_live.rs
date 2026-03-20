//! Client-side instance live updates via SSE and local timer.
//!
//! Compiled only for the `hydrate` feature (WASM).  Provides:
//!
//! - **Local countdown**: reads `data-expires-at` (epoch seconds) from the
//!   `#instance-timer` element and counts down every second purely in WASM.
//!   No server calls for timer ticks.
//!
//! - **SSE state push**: connects to `/challenge/{name}/instance-events`.
//!   The server only sends an event when the instance state *changes*
//!   (phase transition, deletion, solve, etc.).  On receiving an event the
//!   page is reloaded so the SSR view re-renders with the new state.

use serde::Deserialize;
use wasm_bindgen::prelude::*;
use web_sys::{EventSource, MessageEvent, window};

// ---------------------------------------------------------------------------
// Data types (mirrors server InstanceStateResponse)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize)]
struct InstanceState {
    has_instance: bool,
    ready: bool,
    #[allow(dead_code)]
    can_extend: bool,
    solved: bool,
    deleting: bool,
    #[allow(dead_code)]
    time_left_seconds: Option<u64>,
    expires_at_epoch: Option<u64>,
    instance_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_epoch() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

fn format_countdown(total: u64) -> String {
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

/// Compute the state key used for change detection.
fn state_key(st: &InstanceState) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        st.solved,
        st.has_instance,
        st.ready,
        st.deleting,
        st.expires_at_epoch.unwrap_or(0),
        st.instance_name.as_deref().unwrap_or(""),
    )
}

// ---------------------------------------------------------------------------
// Local timer
// ---------------------------------------------------------------------------

/// Start a 1-second `setInterval` that counts down from the `data-expires-at`
/// attribute on `#instance-timer`.  Shows/hides the `#extend-form` when ≤ 5 min
/// remain.  Reloads the page when time reaches zero.
fn start_local_timer() {
    let win = match window() {
        Some(w) => w,
        None => return,
    };
    let doc = match win.document() {
        Some(d) => d,
        None => return,
    };
    let timer_el = match doc.get_element_by_id("instance-timer") {
        Some(el) => el,
        None => return, // no timer on this page
    };

    let expires_at: u64 = timer_el
        .get_attribute("data-expires-at")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    if expires_at == 0 {
        return; // timer not active (no expiry set)
    }

    let cb = Closure::wrap(Box::new(move || {
        let now = now_epoch();
        let left = if expires_at > now {
            expires_at - now
        } else {
            0
        };

        let win = match window() {
            Some(w) => w,
            None => return,
        };

        if left == 0 {
            // Expired — reload to get the fresh SSR view.
            let _ = win.location().reload();
            return;
        }

        if let Some(doc) = win.document() {
            if let Some(el) = doc.get_element_by_id("instance-timer") {
                el.set_text_content(Some(&format_countdown(left)));
            }
            // Show extend button when ≤ 5 minutes remain
            if let Some(ext) = doc.get_element_by_id("extend-form") {
                let display = if left <= 300 { "inline-block" } else { "none" };
                let _ = ext.set_attribute("style", &format!("display:{display}"));
            }
        }
    }) as Box<dyn FnMut()>);

    let _ = win.set_interval_with_callback_and_timeout_and_arguments_0(
        cb.as_ref().unchecked_ref(),
        1000,
    );
    cb.forget();
}

// ---------------------------------------------------------------------------
// SSE connection
// ---------------------------------------------------------------------------

/// Connect to the SSE endpoint for this challenge.  On receiving a state
/// event, reload the page so the server re-renders the correct controls.
fn start_sse(challenge_name: &str) {
    let url = format!(
        "/challenge/{}/instance-events",
        urlencoding::encode(challenge_name),
    );

    let es = match EventSource::new(&url) {
        Ok(es) => es,
        Err(_) => return,
    };

    // Seed last_key from the SSR-rendered state so we can detect stale pages
    // (e.g. reflector hadn't updated when the redirect landed).
    let ssr_key = window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("instance-controls"))
        .and_then(|el| el.get_attribute("data-state-key"))
        .unwrap_or_default();

    let last_key = std::rc::Rc::new(std::cell::RefCell::new(ssr_key));

    let lk = last_key.clone();
    let onmessage = Closure::wrap(Box::new(move |event: MessageEvent| {
        let data = match event.data().as_string() {
            Some(d) => d,
            None => return,
        };
        let state: InstanceState = match serde_wasm_bindgen::from_value(
            js_sys::JSON::parse(&data).unwrap_or(JsValue::NULL),
        ) {
            Ok(s) => s,
            Err(_) => return,
        };

        let key = state_key(&state);
        let mut prev = lk.borrow_mut();
        if key != *prev {
            // State differs from what the page shows — reload.
            *prev = key;
            if let Some(win) = window() {
                let _ = win.location().reload();
            }
        }
    }) as Box<dyn FnMut(MessageEvent)>);

    es.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    // Keep the EventSource alive for the lifetime of the page.
    std::mem::forget(es);
}

// ---------------------------------------------------------------------------
// Public entry point — called from hydrate()
// ---------------------------------------------------------------------------

/// Initialize client-side instance updates for the current page.
///
/// Detects whether we're on a challenge page by looking for a
/// `data-challenge-name` attribute on the `#instance-controls` element.
pub fn init() {
    let challenge_name = window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("instance-controls"))
        .and_then(|el| el.get_attribute("data-challenge-name"));

    let Some(name) = challenge_name else {
        return; // not a challenge page — nothing to do
    };

    start_local_timer();
    start_sse(&name);
}
