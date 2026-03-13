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
pub mod oauth;
#[cfg(feature = "ssr")]
pub mod s3;
#[cfg(feature = "ssr")]
pub mod state;
#[cfg(feature = "ssr")]
pub mod views;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    // SSR-only app – nothing to hydrate on the client.
}
