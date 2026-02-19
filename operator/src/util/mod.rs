mod indexer;
mod waiter;
mod watcher;

pub use indexer::*;
use kube::Resource;
pub use waiter::*;
pub use watcher::*;

fn gvk_string<K: Resource>() -> String
where
    K::DynamicType: Default,
{
    let dt = K::DynamicType::default();

    let group = K::group(&dt);
    let version = K::version(&dt);
    let kind = K::kind(&dt);

    if group.is_empty() {
        format!("{}, {}", version, kind)
    } else {
        format!("{}/{}, {}", group, version, kind)
    }
}
