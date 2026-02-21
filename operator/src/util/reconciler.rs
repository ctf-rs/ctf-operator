use crate::util::KubeObject;
use kube::runtime::controller::Action;
use std::sync::Arc;
use std::time::Duration;

pub trait Reconciler<K: KubeObject, Ctx, Err> {
    const NAME: &'static str;
    const ERROR_RECONCILE_REQUEUE_TIME: u64 = 60; // 1 minute

    fn reconcile(
        resource: Arc<K>,
        ctx: Arc<Ctx>,
    ) -> impl Future<Output = Result<Action, Err>> + Send + 'static;

    fn error_policy(resource: Arc<K>, error: &Err, ctx: Arc<Ctx>) -> Action {
        Action::requeue(Duration::from_secs(Self::ERROR_RECONCILE_REQUEUE_TIME))
    }
}
