#[cfg(feature = "ssr")]
pub mod config;
#[cfg(feature = "ssr")]
pub mod db;
#[cfg(feature = "ssr")]
pub mod handlers;
#[cfg(feature = "ssr")]
pub mod helpers;
#[cfg(feature = "ssr")]
pub mod k8s;
#[cfg(feature = "ssr")]
pub mod metrics;
#[cfg(feature = "ssr")]
pub mod middleware;
#[cfg(feature = "ssr")]
pub mod oauth;
#[cfg(feature = "ssr")]
pub mod s3;
#[cfg(feature = "ssr")]
pub mod scoreboard;
#[cfg(feature = "ssr")]
pub mod state;
#[cfg(feature = "ssr")]
pub mod views;

#[cfg(feature = "hydrate")]
pub mod instance_live;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    // The instance_live module exposes init_instance_live() directly via wasm_bindgen.
    // It will be called from the page's inline script.
}
