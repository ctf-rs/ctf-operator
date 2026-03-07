// use kube::{
//     Client,
//     runtime::events::{Recorder, Reporter},
// };
// use std::sync::Arc;

// use crate::{Metrics, OPERATOR_NAME};

// #[derive(Clone)]
// pub struct State {
//     // Reporter
//     reporter: Arc<Reporter>,

//     /// Metrics
//     metrics: Arc<Metrics>,
// }

// impl State {
//     pub fn new() -> Self {
//         let reporter = Reporter {
//             controller: OPERATOR_NAME.into(),
//             instance: std::env::var("CONTROLLER_POD_NAME").ok(),
//         };

//         Self {
//             reporter: Arc::new(reporter),
//             metrics: Arc::new(Metrics::default()),
//         }
//     }

//     pub async fn to_context(&self, client: Client) -> Arc<Context> {
//         Arc::new(Context {
//             client: client.clone(),
//             recorder: Recorder::new(client, self.reporter.as_ref().to_owned()),
//             metrics: self.metrics.clone(),
//         })
//     }
// }
