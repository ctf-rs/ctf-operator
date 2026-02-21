use std::sync::Arc;

use async_stream::stream;
use futures::{Stream, StreamExt};
use kube::runtime::watcher;
use parking_lot::RwLock;

use crate::util::KubeObject;

pub trait Index<K> {
    fn apply(&mut self, obj: K);
    fn delete(&mut self, obj: &K);
    fn rehydrate(&mut self, objs: Vec<K>);
}

pub fn indexer<K: KubeObject, I: Index<K>, W>(
    index: Arc<RwLock<I>>,
    stream: W,
) -> impl Stream<Item = W::Item>
where
    W: Stream<Item = watcher::Result<watcher::Event<K>>>,
{
    let mut stream = Box::pin(stream);
    stream! {
        let mut buffer = Vec::new();
        while let Some(event) = stream.next().await {
            match event {
                Ok(ev) => {
                    match &ev {
                        watcher::Event::Apply(obj) => {
                            index.write().apply(obj.clone());
                        }
                        watcher::Event::Delete(obj) => {
                            index.write().delete(&obj);
                        }
                        watcher::Event::Init => {
                            buffer = Vec::new();
                        }
                        watcher::Event::InitApply(obj) => {
                            buffer.push(obj.clone());
                        }
                        watcher::Event::InitDone => {
                            index.write().rehydrate(buffer);
                            buffer = Vec::new();
                        }
                    };

                    yield Ok(ev);
                },
                Err(ev) => yield Err(ev)
            }
        }
    }
}

pub trait WatchStreamIndexExt: Stream {
    fn index<K: KubeObject, I: Index<K>>(
        self,
        index: Arc<RwLock<I>>,
    ) -> impl Stream<Item = Self::Item>
    where
        Self: Stream<Item = watcher::Result<watcher::Event<K>>> + Sized,
    {
        indexer(index, self)
    }
}

impl<St: ?Sized> WatchStreamIndexExt for St where St: Stream {}
