use crate::util::KubeObject;
use k8s_openapi::NamespaceResourceScope;
use kube::api::{Patch, PatchParams};
use kube::runtime::controller::Action;
use kube::{Api, Client, Resource, ResourceExt};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

pub trait Reconciler: Send + Sync + 'static {
    const NAME: &'static str;
    const ERROR_RECONCILE_REQUEUE_TIME: u64 = 60; // 1 minute

    type K: KubeObject;
    type Ctx: Send + Sync + 'static;
    type Err: std::error::Error + Send + Sync + 'static;

    fn reconcile(
        resource: Arc<Self::K>,
        ctx: Arc<Self::Ctx>,
    ) -> impl Future<Output = Result<Action, Self::Err>> + Send + 'static;

    fn error_policy(resource: Arc<Self::K>, error: &Self::Err, ctx: Arc<Self::Ctx>) -> Action {
        tracing::error!("Reconcile failed: {}", error);
        let _ = (resource, error, ctx);
        Action::requeue(Duration::from_secs(Self::ERROR_RECONCILE_REQUEUE_TIME))
    }

    fn patch_status_namespaced<S>(
        resource: &Self::K,
        client: Client,
        status: S,
    ) -> impl std::future::Future<Output = Result<(), kube::Error>>
    where
        S: Serialize,
        Self::K: Resource<Scope = NamespaceResourceScope> + DeserializeOwned,
        // ) -> impl std::future::Future<Output = Result<(), kube::Error>> + Send + std::marker::Sync
        // where
        //     Self::K: KubeObject + Resource<Scope = NamespaceResourceScope>,
        //     S: Serialize + Sync + Send,
    {
        async move {
            let ns = resource.namespace_unchecked();
            let name = resource.name_unchecked();
            let api: Api<Self::K> = Api::namespaced(client, &ns);

            let patch = Patch::Apply(json!({
                "apiVersion": format!("{}/{}", Self::K::group(&()), Self::K::version(&())),
                "kind": Self::K::kind(&()),
                "status": status
            }));

            let pp = PatchParams::apply(Self::NAME).force();
            api.patch_status(&name, &pp, &patch).await?;
            Ok(())
        }
    }
}
