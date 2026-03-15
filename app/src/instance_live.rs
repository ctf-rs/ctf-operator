//! Client-side instance polling and timer component.
//!
//! This module is only compiled for the `hydrate` feature and provides
//! reactive updates for instance status and countdown timer.

use serde::Deserialize;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response, console, window};

fn log(msg: &str) {
    console::log_1(&JsValue::from_str(msg));
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize, Default)]
pub struct InstanceState {
    pub has_instance: bool,
    pub ready: bool,
    pub can_extend: bool,
    pub solved: bool,
    pub time_left_seconds: Option<u64>,
    pub instance_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_countdown(seconds: i64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m {secs}s")
    } else if minutes > 0 {
        format!("{minutes}m {secs}s")
    } else {
        format!("{secs}s")
    }
}

fn state_key(state: &InstanceState) -> String {
    format!(
        "{}|{}|{}|{}",
        state.solved,
        state.has_instance,
        state.ready,
        state.instance_name.as_deref().unwrap_or("")
    )
}

async fn fetch_instance_state(endpoint: &str) -> Option<InstanceState> {
    let window = window()?;

    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::SameOrigin);

    let request = Request::new_with_str_and_init(endpoint, &opts).ok()?;
    request.headers().set("Cache-Control", "no-store").ok()?;

    let resp_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .ok()?;
    let resp: Response = resp_value.dyn_into().ok()?;

    if !resp.ok() {
        return None;
    }

    let json = JsFuture::from(resp.json().ok()?).await.ok()?;
    serde_wasm_bindgen::from_value(json).ok()
}

fn update_timer_display(secs: Option<i64>) {
    log(&format!(
        "[instance_live] update_timer_display called with {:?}",
        secs
    ));
    if let Some(window) = window() {
        if let Some(document) = window.document() {
            if let Some(el) = document.get_element_by_id("instance-timer") {
                log("[instance_live] found instance-timer element");
                if let Some(s) = secs {
                    if s > 0 {
                        let text = format_countdown(s);
                        log(&format!("[instance_live] setting timer text to: {}", text));
                        el.set_text_content(Some(&text));
                        let _ = el.set_attribute("style", "");
                    } else {
                        let _ = el.set_attribute("style", "display:none");
                    }
                } else {
                    let _ = el.set_attribute("style", "display:none");
                }
            } else {
                log("[instance_live] instance-timer element NOT found");
            }
        }
    }
}

fn update_status_display(state: &InstanceState) {
    if let Some(window) = window() {
        if let Some(document) = window.document() {
            if let Some(status_el) = document.get_element_by_id("instance-status") {
                status_el.class_list().remove_1("ready").ok();

                let text = if state.solved {
                    "Solved"
                } else if !state.has_instance {
                    "No instance"
                } else if !state.ready {
                    if state.time_left_seconds == Some(0) {
                        "Expired"
                    } else {
                        "Starting..."
                    }
                } else {
                    status_el.class_list().add_1("ready").ok();
                    "Ready"
                };
                status_el.set_text_content(Some(text));
            }

            // Show/hide extend button
            if let Some(extend_el) = document.get_element_by_id("extend-form") {
                if state.can_extend {
                    let _ = extend_el.set_attribute("style", "display:block");
                } else {
                    let _ = extend_el.set_attribute("style", "display:none");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Shared state for timers
// ---------------------------------------------------------------------------

struct LiveState {
    local_seconds: Option<i64>,
    last_state_key: Option<String>,
    endpoint: String,
}

// ---------------------------------------------------------------------------
// Main entry point - called from page script
// ---------------------------------------------------------------------------

/// Initialize the instance live updater for a given challenge.
/// This is called on page load when there's an instance panel.
#[wasm_bindgen]
pub fn init_instance_live(challenge_name: String) {
    log(&format!(
        "[instance_live] init_instance_live called for: {}",
        challenge_name
    ));

    let endpoint = format!(
        "/challenge/{}/instance-state",
        urlencoding::encode(&challenge_name)
    );

    let state = Rc::new(RefCell::new(LiveState {
        local_seconds: None,
        last_state_key: None,
        endpoint,
    }));

    // Countdown tick every second
    let tick_state = Rc::clone(&state);
    let tick_closure = Closure::wrap(Box::new(move || {
        let mut s = tick_state.borrow_mut();
        if let Some(ref mut secs) = s.local_seconds {
            *secs = (*secs - 1).max(0);
            log(&format!("[instance_live] tick: {}s", secs));
            update_timer_display(Some(*secs));

            // Show extend button when less than 5 minutes remaining
            if *secs <= 300 && *secs > 0 {
                if let Some(window) = window() {
                    if let Some(document) = window.document() {
                        if let Some(el) = document.get_element_by_id("extend-form") {
                            let _ = el.set_attribute("style", "display:block");
                        }
                    }
                }
            }
        }
    }) as Box<dyn FnMut()>);

    if let Some(window) = window() {
        log("[instance_live] setting up tick interval");
        let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
            tick_closure.as_ref().unchecked_ref(),
            1000,
        );
    }
    tick_closure.forget();

    // Poll server every 5 seconds
    let poll_state = Rc::clone(&state);
    let poll_closure = Closure::wrap(Box::new(move || {
        log("[instance_live] poll interval fired");
        let poll_state_inner = Rc::clone(&poll_state);
        wasm_bindgen_futures::spawn_local(async move {
            let endpoint = poll_state_inner.borrow().endpoint.clone();
            log(&format!("[instance_live] fetching: {}", endpoint));
            if let Some(new_state) = fetch_instance_state(&endpoint).await {
                log(&format!(
                    "[instance_live] got state: has_instance={}, ready={}, time_left={:?}",
                    new_state.has_instance, new_state.ready, new_state.time_left_seconds
                ));
                update_status_display(&new_state);

                // Check for state change - reload page if significant change
                let key = state_key(&new_state);
                {
                    let s = poll_state_inner.borrow();
                    if let Some(ref prev_key) = s.last_state_key {
                        if key != *prev_key {
                            if let Some(window) = window() {
                                let _ = window.location().reload();
                                return;
                            }
                        }
                    }
                }

                // Update state
                let mut s = poll_state_inner.borrow_mut();
                s.last_state_key = Some(key);

                if new_state.solved || !new_state.has_instance || !new_state.ready {
                    s.local_seconds = None;
                    update_timer_display(None);
                } else if let Some(secs) = new_state.time_left_seconds {
                    log(&format!(
                        "[instance_live] setting local_seconds to {}",
                        secs
                    ));
                    s.local_seconds = Some(secs as i64);
                    update_timer_display(Some(secs as i64));
                } else {
                    s.local_seconds = None;
                    update_timer_display(None);
                }
            } else {
                log("[instance_live] fetch returned None");
            }
        });
    }) as Box<dyn FnMut()>);

    if let Some(window) = window() {
        log("[instance_live] setting up poll interval");
        let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
            poll_closure.as_ref().unchecked_ref(),
            5000,
        );
    }
    poll_closure.forget();

    // Initial poll
    log("[instance_live] starting initial poll");
    let init_state = Rc::clone(&state);
    wasm_bindgen_futures::spawn_local(async move {
        let endpoint = init_state.borrow().endpoint.clone();
        log(&format!("[instance_live] initial fetch: {}", endpoint));
        if let Some(new_state) = fetch_instance_state(&endpoint).await {
            log(&format!(
                "[instance_live] initial state: has_instance={}, ready={}, time_left={:?}",
                new_state.has_instance, new_state.ready, new_state.time_left_seconds
            ));
            update_status_display(&new_state);

            let mut s = init_state.borrow_mut();
            s.last_state_key = Some(state_key(&new_state));
            if new_state.ready {
                if let Some(secs) = new_state.time_left_seconds {
                    log(&format!(
                        "[instance_live] initial: setting local_seconds to {}",
                        secs
                    ));
                    s.local_seconds = Some(secs as i64);
                    update_timer_display(Some(secs as i64));
                }
            }
        } else {
            log("[instance_live] initial fetch returned None");
        }
    });
}
