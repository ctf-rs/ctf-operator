mod condition;
mod controller;
mod indexer;
mod reconciler;
mod waiter;
mod watcher;

pub use condition::*;
pub use controller::*;
pub use indexer::*;
pub use reconciler::*;
pub use waiter::*;
pub use watcher::*;

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
