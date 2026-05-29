//! Client-side scoreboard: localises timestamps, runs a live countdown,
//! and periodically reloads the page to pick up new solves.
//!
//! Compiled only for the `hydrate` feature (WASM).

use wasm_bindgen::prelude::*;
use web_sys::window;

/// Scoreboard data refresh interval in milliseconds (30 seconds).
const REFRESH_INTERVAL_MS: i32 = 30_000;

fn now_epoch() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

/// Format a unix timestamp in the browser's local timezone via `Intl.DateTimeFormat`.
fn format_local_ts(epoch: u64) -> Option<String> {
    let date = js_sys::Date::new(&JsValue::from_f64(epoch as f64 * 1000.0));
    let opts = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&opts, &"month".into(), &"short".into());
    let _ = js_sys::Reflect::set(&opts, &"day".into(), &"numeric".into());
    let _ = js_sys::Reflect::set(&opts, &"hour".into(), &"2-digit".into());
    let _ = js_sys::Reflect::set(&opts, &"minute".into(), &"2-digit".into());
    let _ = js_sys::Reflect::set(&opts, &"hour12".into(), &JsValue::FALSE);
    let _ = js_sys::Reflect::set(&opts, &"timeZoneName".into(), &"short".into());
    Some(date.to_locale_string("default", &opts.into()).into())
}

fn format_countdown(total: u64) -> String {
    let d = total / 86400;
    let h = (total % 86400) / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

// ---------------------------------------------------------------------------
// Timestamp localisation
// ---------------------------------------------------------------------------

fn localise_timestamps(start_ts: u64, end_ts: u64, freeze_ts: u64) {
    let doc = match window().and_then(|w| w.document()) {
        Some(d) => d,
        None => return,
    };
    for (sel, ts) in [
        (".ts-start", start_ts),
        (".ts-end", end_ts),
        (".freeze-time-label", freeze_ts),
    ] {
        if ts > 0 {
            if let Some(el) = doc.query_selector(sel).ok().flatten() {
                if let Some(s) = format_local_ts(ts) {
                    el.set_text_content(Some(&s));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Live countdown (1 s tick)
// ---------------------------------------------------------------------------

fn start_countdown(start_ts: u64, end_ts: u64, freeze_ts: u64) {
    if end_ts == 0 {
        return;
    }

    let tick = Closure::wrap(Box::new(move || {
        let now = now_epoch();
        let remaining = end_ts.saturating_sub(now);
        let frozen = freeze_ts > 0 && now >= freeze_ts;
        let ended = now >= end_ts;

        let status = if ended {
            "CTF has ended".to_string()
        } else if frozen {
            format!(
                "Scoreboard frozen \u{2014} {} remaining",
                format_countdown(remaining)
            )
        } else {
            format!("{} remaining", format_countdown(remaining))
        };

        let doc = match window().and_then(|w| w.document()) {
            Some(d) => d,
            None => return,
        };

        // Update status text
        if let Some(el) = doc.query_selector(".timer-status").ok().flatten() {
            el.set_text_content(Some(&status));
        }

        // Update progress bar fill width + colour
        let total = end_ts.saturating_sub(start_ts);
        if total > 0 {
            let elapsed = now.saturating_sub(start_ts).min(total);
            let pct = (elapsed as f64 / total as f64 * 100.0).min(100.0);
            if let Some(el) = doc.query_selector(".timer-bar-fill").ok().flatten() {
                let fp = if freeze_ts > 0 {
                    Some(
                        freeze_ts.saturating_sub(start_ts).min(total) as f64
                            / total as f64
                            * 100.0,
                    )
                } else {
                    None
                };
                let style = match fp {
                    Some(fp) if pct > fp => format!(
                        "width:{pct:.2}%;background:linear-gradient(90deg,#22c55e 0%,#4ade80 {fp:.2}%,#f59e0b {fp:.2}%,#ef4444 100%);"
                    ),
                    _ => format!(
                        "width:{pct:.2}%;background:linear-gradient(90deg,#22c55e,#4ade80);"
                    ),
                };
                let _ = el.set_attribute("style", &style);
            }
        }
    }) as Box<dyn FnMut()>);

    if let Some(win) = window() {
        let _ = win.set_interval_with_callback_and_timeout_and_arguments_0(
            tick.as_ref().unchecked_ref(),
            1000,
        );
    }
    tick.forget();
}

// ---------------------------------------------------------------------------
// Periodic scoreboard data refresh
// ---------------------------------------------------------------------------

/// Reload the page every `REFRESH_INTERVAL_MS` to pick up new solves.
/// The live countdown keeps ticking between reloads so the user always
/// sees an accurate timer.
fn start_auto_refresh() {
    let cb = Closure::wrap(Box::new(move || {
        if let Some(win) = window() {
            let _ = win.location().reload();
        }
    }) as Box<dyn FnMut()>);

    if let Some(win) = window() {
        let _ = win.set_interval_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            REFRESH_INTERVAL_MS,
        );
    }
    cb.forget();
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn init() {
    let doc = match window().and_then(|w| w.document()) {
        Some(d) => d,
        None => return,
    };
    let container = match doc.query_selector(".timer-bar-container").ok().flatten() {
        Some(el) => el,
        None => {
            // No timer bar but still refresh scoreboard data if on the page
            if doc
                .query_selector(".scoreboard-container")
                .ok()
                .flatten()
                .is_some()
            {
                start_auto_refresh();
            }
            return;
        }
    };

    let start_ts: u64 = container
        .get_attribute("data-start-ts")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let end_ts: u64 = container
        .get_attribute("data-end-ts")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let freeze_ts: u64 = container
        .get_attribute("data-freeze-ts")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    localise_timestamps(start_ts, end_ts, freeze_ts);
    start_countdown(start_ts, end_ts, freeze_ts);
    start_auto_refresh();
}
