use tokio::sync::mpsc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub enum HubEvent {
    Push(String),
    Close(u16),
}

#[derive(Clone)]
pub struct TvHub {
    inner: std::sync::Arc<Mutex<Inner>>,
}

struct Inner {
    next_id: u64,
    current: Option<Slot>,
}

struct Slot {
    id: u64,
    outgoing: mpsc::Sender<HubEvent>,
}

impl TvHub {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(Mutex::new(Inner {
                next_id: 1,
                current: None,
            })),
        }
    }

    pub async fn online(&self) -> bool {
        self.inner.lock().await.current.is_some()
    }

    pub async fn attach(
        &self,
        outgoing: mpsc::Sender<HubEvent>,
    ) -> (u64, Option<mpsc::Sender<HubEvent>>) {
        let mut inner = self.inner.lock().await;
        let id = inner.next_id;
        inner.next_id += 1;
        let previous = inner.current.take().map(|slot| slot.outgoing);
        inner.current = Some(Slot { id, outgoing });
        (id, previous)
    }

    pub async fn release(&self, connection_id: u64) {
        let mut inner = self.inner.lock().await;
        if inner.current.as_ref().is_some_and(|slot| slot.id == connection_id) {
            inner.current = None;
        }
    }

    pub async fn push(&self, payload: &str) -> bool {
        let outgoing = {
            let inner = self.inner.lock().await;
            inner.current.as_ref().map(|slot| slot.outgoing.clone())
        };
        let Some(outgoing) = outgoing else {
            return false;
        };
        match outgoing.send(HubEvent::Push(payload.to_string())).await {
            Ok(()) => true,
            Err(_) => {
                let mut inner = self.inner.lock().await;
                if inner
                    .current
                    .as_ref()
                    .is_some_and(|slot| slot.outgoing.same_channel(&outgoing))
                {
                    inner.current = None;
                }
                false
            }
        }
    }
}

impl Default for TvHub {
    fn default() -> Self {
        Self::new()
    }
}
