use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioCursor {
    pub epoch: u64,
    pub sample: u64,
}

#[derive(Debug, Clone)]
pub struct AudioPacket {
    pub samples: Arc<[i16]>,
    pub start_sample: u64,
    pub end_sample: u64,
    pub epoch: u64,
    pub sample_rate: u32,
}

pub(super) struct SampleRing {
    packets: VecDeque<AudioPacket>,
    retained: usize,
    limit: usize,
    pub cursor: AudioCursor,
}

impl SampleRing {
    pub fn new(limit: usize) -> Self {
        Self {
            packets: VecDeque::new(),
            retained: 0,
            limit,
            cursor: AudioCursor {
                epoch: 0,
                sample: 0,
            },
        }
    }
    pub fn reset(&mut self, epoch: u64, sample_rate: u32) {
        self.packets.clear();
        self.retained = 0;
        self.limit = sample_rate as usize * 15;
        self.cursor = AudioCursor { epoch, sample: 0 };
    }
    pub fn push(&mut self, samples: &[i16], sample_rate: u32) -> AudioPacket {
        let packet = AudioPacket {
            samples: Arc::from(samples),
            start_sample: self.cursor.sample,
            end_sample: self.cursor.sample + samples.len() as u64,
            epoch: self.cursor.epoch,
            sample_rate,
        };
        self.cursor.sample = packet.end_sample;
        self.retained += samples.len();
        self.packets.push_back(packet.clone());
        while self.retained > self.limit && self.packets.len() > 1 {
            self.retained -= self.packets.pop_front().unwrap().samples.len();
        }
        packet
    }
    pub fn since(&self, cursor: AudioCursor) -> Result<Vec<AudioPacket>, String> {
        if cursor.epoch != self.cursor.epoch {
            return Err("audio input changed after wake detection".into());
        }
        if cursor.sample > self.cursor.sample {
            return Err("audio cursor is ahead of capture".into());
        }
        if self
            .packets
            .front()
            .is_some_and(|p| cursor.sample < p.start_sample)
        {
            return Err("wake handoff exceeded the retained audio window".into());
        }
        Ok(self
            .packets
            .iter()
            .filter(|p| p.end_sample > cursor.sample)
            .map(|p| {
                if cursor.sample <= p.start_sample {
                    p.clone()
                } else {
                    let skip = (cursor.sample - p.start_sample) as usize;
                    AudioPacket {
                        samples: Arc::from(&p.samples[skip..]),
                        start_sample: cursor.sample,
                        ..p.clone()
                    }
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handoff_clips_the_first_packet_exactly() {
        let mut ring = SampleRing::new(10);
        ring.push(&[1, 2, 3], 16000);
        ring.push(&[4, 5], 16000);
        let replay = ring
            .since(AudioCursor {
                epoch: 0,
                sample: 2,
            })
            .unwrap();
        assert_eq!(&*replay[0].samples, &[3]);
        assert_eq!(replay[1].start_sample, 3);
    }
    #[test]
    fn replaced_device_rejects_stale_cursor() {
        let mut ring = SampleRing::new(10);
        ring.reset(4, 16000);
        assert!(ring
            .since(AudioCursor {
                epoch: 3,
                sample: 0
            })
            .is_err());
    }
}
