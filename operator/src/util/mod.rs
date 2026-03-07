mod controller;
mod indexer;
mod reconciler;
mod waiter;
mod watcher;

pub use controller::*;
pub use indexer::*;
pub use reconciler::*;
pub use waiter::*;
pub use watcher::*;

use once_cell::sync::Lazy;
use regex::Regex;
use serde::de::DeserializeOwned;
use std::fmt::Debug;

pub trait KubeObject:
    'static + kube::Resource<DynamicType = ()> + Clone + Debug + DeserializeOwned + Send + Sync
{
    fn gvk() -> String {
        let group = Self::group(&());
        let version = Self::version(&());
        let kind = Self::kind(&());

        if group.is_empty() {
            format!("{}, {}", version, kind)
        } else {
            format!("{}/{}, {}", group, version, kind)
        }
    }

    fn namespace_unchecked(&self) -> String {
        self.meta()
            .namespace
            .clone()
            .expect(".metadata.namespace missing")
    }
}

impl<T> KubeObject for T where
    T: 'static + kube::Resource<DynamicType = ()> + Clone + Debug + DeserializeOwned + Send + Sync
{
}

static DNS1123_LABEL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?$").unwrap());

pub fn is_dns1123_label(name: &str) -> bool {
    DNS1123_LABEL.is_match(name)
}
