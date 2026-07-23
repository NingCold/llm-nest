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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_bus_publish_subscribe() {
        let bus: EventBus<i32> = EventBus::new();
        let mut rx = bus.subscribe();
        bus.publish(42);
        let received = rx.try_recv().unwrap();
        assert_eq!(received, 42);
    }

    #[test]
    fn event_bus_multiple_subscribers() {
        let bus: EventBus<&str> = EventBus::new();
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();
        bus.publish("hello");
        assert_eq!(rx1.try_recv().unwrap(), "hello");
        assert_eq!(rx2.try_recv().unwrap(), "hello");
    }

    #[test]
    fn event_bus_no_message_is_lag() {
        let bus: EventBus<i32> = EventBus::new();
        let mut rx = bus.subscribe();
        let result = rx.try_recv();
        assert!(result.is_err());
    }
}