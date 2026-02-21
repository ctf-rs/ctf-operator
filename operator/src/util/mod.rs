mod controller_runner;
mod indexer;
mod reconciler;
mod waiter;
mod watcher;

pub use controller_runner::*;
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
}

impl<T> KubeObject for T where
    T: 'static + kube::Resource<DynamicType = ()> + Clone + Debug + DeserializeOwned + Send + Sync
{
}
