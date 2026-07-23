use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct EventBus<E> {
    tx: broadcast::Sender<E>,
}

impl<E: Clone> EventBus<E> {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    pub fn publish(&self, event: E) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<E> {
        self.tx.subscribe()
    }
}

impl<E: Clone> Default for EventBus<E> {
    fn default() -> Self {
        Self::new()
    }
}