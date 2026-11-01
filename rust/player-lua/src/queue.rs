use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::backend::ScriptEvent;
use crate::host::HostError;

pub const QUEUE_CAPACITY: usize = 1024;

const DROPPABLE_EVENTS: [&str; 3] = ["property-change", "tick", "log-message"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Posted {
    Queued,
    Dropped,
}

struct QueueState {
    items: VecDeque<ScriptEvent>,
    dropped: u64,
    closed: bool,
}

struct Shared {
    state: Mutex<QueueState>,
    ready: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, QueueState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Clone)]
pub struct EventSender {
    shared: Arc<Shared>,
}

pub struct EventReceiver {
    shared: Arc<Shared>,
}

pub enum Received {
    Event(ScriptEvent),
    Timeout,
    Closed,
}

pub fn event_queue() -> (EventSender, EventReceiver) {
    let shared = Arc::new(Shared {
        state: Mutex::new(QueueState {
            items: VecDeque::new(),
            dropped: 0,
            closed: false,
        }),
        ready: Condvar::new(),
    });
    (
        EventSender {
            shared: shared.clone(),
        },
        EventReceiver { shared },
    )
}

impl EventSender {
    pub fn post(&self, event: ScriptEvent) -> Result<Posted, HostError> {
        let mut state = self.shared.lock();
        if state.closed {
            return Err(HostError::PostClosed);
        }
        if state.items.len() >= QUEUE_CAPACITY && DROPPABLE_EVENTS.contains(&event.name.as_str()) {
            state.dropped += 1;
            return Ok(Posted::Dropped);
        }
        state.items.push_back(event);
        self.shared.ready.notify_one();
        Ok(Posted::Queued)
    }

    pub fn close(&self) {
        let mut state = self.shared.lock();
        state.closed = true;
        self.shared.ready.notify_all();
    }
}

impl EventReceiver {
    pub fn recv_timeout(&self, timeout: Duration) -> Received {
        let deadline = Instant::now().checked_add(timeout);
        let mut state = self.shared.lock();
        loop {
            if let Some(event) = state.items.pop_front() {
                return Received::Event(event);
            }
            if state.closed {
                return Received::Closed;
            }
            let wait = match deadline {
                Some(deadline) => {
                    let now = Instant::now();
                    if now >= deadline {
                        return Received::Timeout;
                    }
                    deadline - now
                }
                None => Duration::from_secs(86_400),
            };
            state = self
                .shared
                .ready
                .wait_timeout(state, wait)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
    }

    pub fn take_dropped(&self) -> u64 {
        std::mem::take(&mut self.shared.lock().dropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick() -> ScriptEvent {
        ScriptEvent::new("tick")
    }

    #[test]
    fn full_queue_drops_volume_events_and_keeps_the_rest() {
        let (sender, receiver) = event_queue();
        for _ in 0..QUEUE_CAPACITY {
            assert_eq!(sender.post(tick()), Ok(Posted::Queued));
        }
        assert_eq!(sender.post(tick()), Ok(Posted::Dropped));
        assert_eq!(
            sender.post(ScriptEvent::new("client-message")),
            Ok(Posted::Queued)
        );
        assert_eq!(sender.post(ScriptEvent::shutdown()), Ok(Posted::Queued));
        assert_eq!(receiver.take_dropped(), 1);
        assert_eq!(receiver.take_dropped(), 0);
    }

    #[test]
    fn closed_queue_rejects_posts_and_drains_before_reporting_closed() {
        let (sender, receiver) = event_queue();
        sender.post(tick()).unwrap();
        sender.close();
        assert_eq!(sender.post(tick()), Err(HostError::PostClosed));
        assert!(matches!(
            receiver.recv_timeout(Duration::ZERO),
            Received::Event(_)
        ));
        assert!(matches!(
            receiver.recv_timeout(Duration::ZERO),
            Received::Closed
        ));
    }
}
