use std::collections::VecDeque;

pub(crate) const FRAME_BYTES: usize = 128 * 1024 * 1024;
pub(crate) const LAYER_BYTES: usize = 64 * 1024 * 1024;

pub(crate) struct Cache<K, V> {
    entries: VecDeque<(K, V, usize)>,
    bytes: usize,
    limit: usize,
}

impl<K: PartialEq, V: Clone> Cache<K, V> {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            limit,
        }
    }

    pub(crate) fn get(&mut self, key: &K) -> Option<V> {
        let index = self.entries.iter().position(|(prior, ..)| prior == key)?;
        let entry = self.entries.remove(index)?;
        let value = entry.1.clone();
        self.entries.push_back(entry);
        Some(value)
    }

    pub(crate) fn insert(&mut self, key: K, value: V, bytes: usize) {
        if bytes > self.limit {
            return;
        }
        if let Some(index) = self.entries.iter().position(|(prior, ..)| *prior == key)
            && let Some((_, _, bytes)) = self.entries.remove(index)
        {
            self.bytes -= bytes;
        }
        while self.bytes > self.limit - bytes || self.entries.len() >= 512 {
            let Some((_, _, bytes)) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= bytes;
        }
        self.bytes += bytes;
        self.entries.push_back((key, value, bytes));
    }

    pub(crate) fn invalidate(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Quality {
    #[default]
    Full,
    Half,
    Quarter,
}

impl Quality {
    pub(crate) const ALL: [Self; 3] = [Self::Full, Self::Half, Self::Quarter];
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::Half => "Half",
            Self::Quarter => "Quarter",
        }
    }
    pub(crate) fn view(
        self,
        (origin, scale, (width, height)): crate::render::View,
    ) -> crate::render::View {
        let divisor = match self {
            Self::Full => 1,
            Self::Half => 2,
            Self::Quarter => 4,
        };
        (
            origin,
            scale / divisor as f32,
            (width.div_ceil(divisor), height.div_ceil(divisor)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_evicts_old_frames_and_an_edit_invalidates_every_frame() {
        let mut cache = Cache::new(20);
        cache.insert(0, "zero", 10);
        cache.insert(1, "one", 10);
        assert_eq!(cache.get(&0), Some("zero"));
        cache.insert(2, "two", 10);
        assert_eq!(cache.get(&1), None);
        assert_eq!(cache.get(&0), Some("zero"));
        cache.invalidate();
        assert_eq!(cache.get(&0), None);
        assert_eq!(cache.bytes(), 0);
    }
}
