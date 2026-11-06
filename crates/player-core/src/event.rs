use std::collections::VecDeque;

const CAPACITY: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusEvent {
    pub sequence: u64,
    pub name: String,
}

#[derive(Debug)]
pub struct EventBus {
    entries: VecDeque<BusEvent>,
    next_sequence: u64,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            next_sequence: 1,
        }
    }

    pub fn push(&mut self, name: &str) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        if self.entries.len() >= CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(BusEvent {
            sequence,
            name: name.to_owned(),
        });
        sequence
    }

    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    pub fn since(&self, sequence: u64) -> Vec<BusEvent> {
        self.entries
            .iter()
            .filter(|entry| entry.sequence >= sequence)
            .cloned()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_records_and_replays_in_order() {
        let mut bus = EventBus::new();
        assert!(bus.is_empty());
        let first = bus.push("pause");
        let second = bus.push("volume");
        assert_eq!(second, first + 1);
        let replay = bus.since(first);
        assert_eq!(
            replay,
            vec![
                BusEvent {
                    sequence: first,
                    name: "pause".to_owned(),
                },
                BusEvent {
                    sequence: second,
                    name: "volume".to_owned(),
                },
            ]
        );
        assert!(bus.since(second + 1).is_empty());
        assert_eq!(bus.next_sequence(), second + 1);
    }

    #[test]
    fn bus_bounds_memory() {
        let mut bus = EventBus::new();
        for index in 0..(CAPACITY + 10) {
            bus.push(if index % 2 == 0 { "pause" } else { "volume" });
        }
        assert_eq!(bus.len(), CAPACITY);
        let replay = bus.since(1);
        assert_eq!(replay.len(), CAPACITY);
        assert_eq!(replay.first().unwrap().sequence, 11);
        assert_eq!(bus.next_sequence() as usize, CAPACITY + 11);
    }
}
