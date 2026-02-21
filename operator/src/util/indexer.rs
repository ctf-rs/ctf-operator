use async_stream::stream;
use futures::{Stream, StreamExt};
use kube::runtime::watcher;

use crate::util::KubeObject;

pub trait Index<K> {
    fn apply(&self, obj: K);
    fn delete(&self, obj: &K);
    fn rehydrate(&self, objs: Vec<K>);
}

pub fn indexer<K: KubeObject, I: Index<K>, W>(index: &I, stream: W) -> impl Stream<Item = W::Item>
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
                            index.apply(obj.clone());
                        }
                        watcher::Event::Delete(obj) => {
                            index.delete(&obj);
                        }
                        watcher::Event::Init => {
                            buffer = Vec::new();
                        }
                        watcher::Event::InitApply(obj) => {
                            buffer.push(obj.clone());
                        }
                        watcher::Event::InitDone => {
                            index.rehydrate(buffer);
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

/// Extension trait for streams returned by [`watcher`](watcher()) or [`reflector`](crate::reflector::reflector)
pub trait WatchStreamIndexExt: Stream {
    fn index<K: KubeObject, I: Index<K>>(self, index: &I) -> impl Stream<Item = Self::Item>
    where
        Self: Stream<Item = watcher::Result<watcher::Event<K>>> + Sized,
    {
        indexer(index, self)
    }
}

impl<St: ?Sized> WatchStreamIndexExt for St where St: Stream {}
