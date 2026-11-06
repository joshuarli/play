pub type EntryId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    entries: Vec<EntryId>,
    current: Option<EntryId>,
    current_was_replaced: bool,
    completed: bool,
    started: bool,
    next_id: EntryId,
}

impl Default for Playlist {
    fn default() -> Self {
        Self::new()
    }
}

impl Playlist {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            current: None,
            current_was_replaced: false,
            completed: false,
            started: false,
            next_id: 0,
        }
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn first(&self) -> Option<EntryId> {
        self.entries.first().copied()
    }

    pub fn last(&self) -> Option<EntryId> {
        self.entries.last().copied()
    }

    pub fn entry_at(&self, index: usize) -> Option<EntryId> {
        self.entries.get(index).copied()
    }

    pub fn index_of(&self, id: EntryId) -> Option<usize> {
        self.entries.iter().position(|entry| *entry == id)
    }

    pub fn insert_new_at(&mut self, index: usize) -> (EntryId, usize) {
        self.next_id += 1;
        let id = self.next_id;
        let index = index.min(self.entries.len());
        self.entries.insert(index, id);
        (id, index)
    }

    pub fn remove(&mut self, id: EntryId) -> Option<usize> {
        let index = self.index_of(id)?;
        if self.current == Some(id) {
            self.current = self.entries.get(index + 1).copied();
            self.current_was_replaced = true;
        }
        self.entries.remove(index);
        Some(index)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current = None;
        self.current_was_replaced = false;
        self.completed = false;
        self.started = false;
    }

    pub fn clear_except_current(&mut self) {
        if let Some(current) = self.current {
            self.entries.retain(|entry| *entry == current);
        } else {
            self.entries.clear();
        }
        self.completed = false;
        self.started = false;
    }

    pub fn move_entry(&mut self, id: EntryId, at: Option<EntryId>) -> bool {
        if at == Some(id) {
            return false;
        }
        let Some(old_index) = self.index_of(id) else {
            return false;
        };
        let at_index = match at {
            Some(at) => match self.index_of(at) {
                Some(index) => index,
                None => return false,
            },
            None => self.entries.len(),
        };
        self.entries.remove(old_index);
        let index = if old_index < at_index {
            at_index - 1
        } else {
            at_index
        };
        self.entries.insert(index.min(self.entries.len()), id);
        true
    }

    pub fn current(&self) -> Option<EntryId> {
        self.current
    }

    pub fn set_current(&mut self, current: Option<EntryId>) {
        self.current = current;
    }

    pub fn current_was_replaced(&self) -> bool {
        self.current_was_replaced
    }

    pub fn set_current_was_replaced(&mut self, value: bool) {
        self.current_was_replaced = value;
    }

    pub fn completed(&self) -> bool {
        self.completed
    }

    pub fn set_completed(&mut self, value: bool) {
        self.completed = value;
    }

    pub fn started(&self) -> bool {
        self.started
    }

    pub fn set_started(&mut self, value: bool) {
        self.started = value;
    }

    pub fn next(&self, direction: i32) -> Option<EntryId> {
        if direction != -1 && direction != 1 {
            return None;
        }
        match self.current {
            None if self.completed && direction < 0 => self.last(),
            None if !self.started && direction > 0 => self.first(),
            None => None,
            Some(current) if direction < 0 => self.entry_rel(current, -1),
            Some(current) if self.current_was_replaced => Some(current),
            Some(current) => self.entry_rel(current, 1),
        }
    }

    pub fn entry_rel(&self, id: EntryId, direction: i32) -> Option<EntryId> {
        if direction != -1 && direction != 1 {
            return None;
        }
        let index = self.index_of(id)?;
        let target = if direction < 0 {
            index.checked_sub(1)?
        } else {
            index.checked_add(1)?
        };
        self.entry_at(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_inserts_and_indexes() {
        let mut playlist = Playlist::new();
        let (first, index) = playlist.insert_new_at(0);
        assert_eq!(index, 0);
        let (second, _) = playlist.insert_new_at(1);
        let (middle, index) = playlist.insert_new_at(1);
        assert_eq!(index, 1);
        assert_eq!(playlist.count(), 3);
        assert_eq!(playlist.entry_at(0), Some(first));
        assert_eq!(playlist.entry_at(1), Some(middle));
        assert_eq!(playlist.entry_at(2), Some(second));
        assert_eq!(playlist.index_of(second), Some(2));
        assert_eq!(playlist.first(), Some(first));
        assert_eq!(playlist.last(), Some(second));
    }

    #[test]
    fn playlist_removes_and_replaces_current() {
        let mut playlist = Playlist::new();
        let (first, _) = playlist.insert_new_at(0);
        let (second, _) = playlist.insert_new_at(1);
        let (third, _) = playlist.insert_new_at(2);
        playlist.set_current(Some(second));
        assert_eq!(playlist.remove(second), Some(1));
        assert_eq!(playlist.current(), Some(third));
        assert!(playlist.current_was_replaced());
        assert_eq!(playlist.count(), 2);
        playlist.set_current_was_replaced(false);
        playlist.set_current(Some(third));
        playlist.remove(first);
        assert_eq!(playlist.current(), Some(third));
        assert!(!playlist.current_was_replaced());
        playlist.remove(third);
        assert_eq!(playlist.current(), None);
        assert!(playlist.current_was_replaced());
    }

    #[test]
    fn playlist_next_and_previous_follow_c_behavior() {
        let mut playlist = Playlist::new();
        let (first, _) = playlist.insert_new_at(0);
        let (second, _) = playlist.insert_new_at(1);
        assert_eq!(playlist.next(1), Some(first));
        assert_eq!(playlist.next(-1), None);
        playlist.set_started(true);
        assert_eq!(playlist.next(1), None);
        playlist.set_current(Some(first));
        assert_eq!(playlist.next(1), Some(second));
        assert_eq!(playlist.next(-1), None);
        playlist.set_current(Some(second));
        assert_eq!(playlist.next(-1), Some(first));
        assert_eq!(playlist.next(1), None);
        playlist.set_current(Some(first));
        playlist.set_current_was_replaced(true);
        assert_eq!(playlist.next(1), Some(first));
        playlist.set_current(None);
        playlist.set_completed(true);
        assert_eq!(playlist.next(-1), Some(second));
        assert_eq!(playlist.next(1), None);
    }

    #[test]
    fn playlist_moves_and_clears() {
        let mut playlist = Playlist::new();
        let (first, _) = playlist.insert_new_at(0);
        let (second, _) = playlist.insert_new_at(1);
        let (third, _) = playlist.insert_new_at(2);
        assert!(playlist.move_entry(third, Some(first)));
        assert_eq!(playlist.entry_at(0), Some(third));
        assert_eq!(playlist.entry_at(1), Some(first));
        assert_eq!(playlist.entry_at(2), Some(second));
        assert!(!playlist.move_entry(third, Some(third)));
        playlist.set_current(Some(first));
        playlist.clear_except_current();
        assert_eq!(playlist.count(), 1);
        assert_eq!(playlist.first(), Some(first));
        playlist.clear();
        assert!(playlist.is_empty());
        assert_eq!(playlist.current(), None);
        assert!(!playlist.completed());
        assert!(!playlist.started());
    }

    #[test]
    fn playlist_ids_increase_across_clear() {
        let mut playlist = Playlist::new();
        let (first, _) = playlist.insert_new_at(0);
        playlist.clear();
        let (second, _) = playlist.insert_new_at(0);
        assert!(second > first);
    }
}
