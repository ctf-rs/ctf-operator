use std::{sync::Arc, time::Duration};

use anyhow::bail;
use cfg_if::cfg_if;
use clap::Parser;
use k8s_openapi::chrono::{DateTime, Utc};
use kube::runtime::watcher;
use tokio::sync::mpsc;
use tracing::{info_span, Instrument};

use crate::cli::Args;

mod cli;
mod controller;
mod crd;
mod ctx;
mod runtime;

const DETECT_TIMEOUT: Duration = Duration::from_secs(10);
const RECONCILIATION_PERIOD: Duration = Duration::from_secs(10);

// The maximum number of status patches to buffer. As a conservative estimate,
// we assume that sending a patch will take at least 1ms, so we set the buffer
// size to be the same as the reconciliation period in milliseconds.
const UPDATE_QUEUE_SIZE: usize = RECONCILIATION_PERIOD.as_millis() as usize;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let mut rt = kubert::Runtime::builder()
        .with_log(args.log_level, args.log_format)
        .with_client(args.client)
        .build()
        .await?;

    cfg_if! {
        if #[cfg(debug_assertions)] {
            let hostname = "test".to_string();
            let claim = kubert::lease::Claim {
                holder: "test".to_string(),
                expiry: DateTime::<Utc>::MAX_UTC,
            };
            let (_claims_tx, claims) = tokio::sync::watch::channel(Arc::new(claim));

        } else {
            let hostname =
                std::env::var("HOSTNAME").expect("Failed to fetch `HOSTNAME` environment variable");

            let claims = crate::runtime::lease::init(
                &rt,
                &args.control_plane_namespace,
                &args.ctf_deployment_name,
                &hostname,
            )
            .await?;
        }
    }

    let (updates_tx, updates_rx) = mpsc::channel(UPDATE_QUEUE_SIZE);

    let jeopardy_index =
        controller::jeopardy::Index::shared(hostname.clone(), claims.clone(), updates_tx);

    let ctfs = rt.watch_all::<crd::v1alpha1::Ctf>(watcher::Config::default());
    let ctf_indexer = tokio::spawn(
        kubert::index::namespaced(jeopardy_index.clone(), ctfs).instrument(info_span!("ctfs")),
    );

    let challs = rt.watch_all::<crd::v1alpha1::Ctf>(watcher::Config::default());
    let chall_indexer = tokio::spawn(
        kubert::index::namespaced(jeopardy_index.clone(), challs).instrument(info_span!("challs")),
    );

    let instances = rt.watch_all::<crd::v1alpha1::Ctf>(watcher::Config::default());
    let instance_indexer = tokio::spawn(
        kubert::index::namespaced(jeopardy_index.clone(), instances)
            .instrument(info_span!("instances")),
    );

    let client = rt.client();

    let ctf_controller = controller::jeopardy::Controller::new(
        claims,
        client,
        hostname,
        updates_rx,
        Duration::from_millis(args.patch_timeout_ms),
    );

    let ctf_controller_task = tokio::spawn(
        ctf_controller
            .run()
            .instrument(info_span!("jeopardy_controller")),
    );

    tokio::select! {
        res = ctf_indexer => match res {
            Err(error) => bail!("Ctf indexer failed: {}", error),
            Ok(()) => {
                tracing::debug!("Ctf indexer completed");
            }
        },
        res = chall_indexer => match res {
            Err(error) => bail!("Challenge indexer failed: {}", error),
            Ok(()) => {
                tracing::debug!("Challenge indexer completed");
            }
        },
        res = instance_indexer => match res {
            Err(error) => bail!("Instance indexer failed: {}", error),
            Ok(()) => {
                tracing::debug!("Instance indexer completed");
            }
        },

        res = rt.run() => {
            if res.is_err() {
                bail!("aborted");
            }
        }

        res = ctf_controller_task  => match res {
            Err(error) => bail!("ctf controller failed: {}", error),
            Ok(()) => {
                tracing::debug!("ctf controller completed");
            }
        },
    }

    Ok(())
}
