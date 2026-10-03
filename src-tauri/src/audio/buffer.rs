//! Bounded PCM storage with an absolute sample clock. Live sessions discard
//! acknowledged audio only; overflow is visible instead of silently dropping it.

#[derive(Default)]
pub struct RecordingBuffer {
    samples: Vec<i16>,
    start: u64,
}

impl RecordingBuffer {
    pub fn new(pre_roll: &[i16], capacity: usize) -> Self {
        let mut samples = Vec::with_capacity(capacity.max(pre_roll.len()));
        samples.extend_from_slice(pre_roll);
        Self { samples, start: 0 }
    }

    pub fn append(&mut self, chunk: &[i16], maximum: usize) -> bool {
        let accepted = maximum.saturating_sub(self.samples.len()).min(chunk.len());
        self.samples.extend_from_slice(&chunk[..accepted]);
        accepted != chunk.len()
    }

    pub fn end(&self) -> u64 {
        self.start + self.samples.len() as u64
    }
    pub fn start(&self) -> u64 {
        self.start
    }

    pub fn window(&self, from: u64, maximum: usize) -> (u64, Vec<i16>) {
        let start = from.max(self.start).min(self.end());
        let offset = (start - self.start) as usize;
        let end = (offset + maximum).min(self.samples.len());
        (start, self.samples[offset..end].to_vec())
    }

    pub fn discard_before(&mut self, position: u64) {
        let count = position
            .saturating_sub(self.start)
            .min(self.samples.len() as u64) as usize;
        self.samples.drain(..count);
        self.start += count as u64;
    }

    pub fn take(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discard_keeps_the_absolute_clock_and_overlap() {
        let mut buffer = RecordingBuffer::new(&[1, 2, 3], 16);
        assert!(!buffer.append(&[4, 5, 6], 16));
        buffer.discard_before(3);
        assert_eq!(buffer.end(), 6);
        assert_eq!(buffer.window(2, 2), (3, vec![4, 5]));
        assert_eq!(buffer.window(5, 16), (5, vec![6]));
    }
    #[test]
    fn overflow_is_explicit_and_does_not_evict_unread_audio() {
        let mut buffer = RecordingBuffer::new(&[], 4);
        assert!(buffer.append(&[1, 2, 3, 4, 5], 4));
        assert_eq!(buffer.window(0, 10).1, vec![1, 2, 3, 4]);
    }
}
