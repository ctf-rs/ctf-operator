use std::sync::Arc;

#[derive(Clone)]
pub struct Ctx {
    client: Arc<kube::Client>,
}

impl Ctx {
    pub fn new(rt: &kubert::Runtime) -> Self {
        let client = Arc::new(rt.client());
        Self { client }
    }

    pub fn client(&self) -> kube::Client {
        (*self.client).clone()
    }
}
