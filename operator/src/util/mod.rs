mod controller;
mod indexer;
mod waiter;
mod watcher;

pub use controller::*;
pub use indexer::*;
use kube::runtime::reflector::Lookup;
use serde::de::DeserializeOwned;
pub use waiter::*;
pub use watcher::*;

use std::fmt::Debug;
use std::hash::Hash;

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
}

impl<T> KubeObject for T where
    T: 'static + kube::Resource<DynamicType = ()> + Clone + Debug + DeserializeOwned + Send + Sync
{
}
