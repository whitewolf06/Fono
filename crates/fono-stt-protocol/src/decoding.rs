//! Optional protocol-3 decoding policy. Missing fields retain the historic fast mode.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecodingStrategy {
    #[default]
    Fast,
    Accurate,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecodingOptions {
    #[serde(default)]
    pub strategy: DecodingStrategy,
    #[serde(default)]
    pub threads: Option<u8>,
}

impl DecodingOptions {
    pub fn validate(self) -> Result<(), String> {
        if self
            .threads
            .is_some_and(|threads| !(1..=8).contains(&threads))
        {
            return Err("Whisper threads must be between 1 and 8".into());
        }
        Ok(())
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn effective_threads(self) -> i32 {
        i32::from(self.threads.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map_or(4, |count| count.get())
                .clamp(1, 8) as u8
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_and_explicit_fast_modes_preserve_the_historic_default() {
        let legacy: DecodingOptions = serde_json::from_str("{}").unwrap();
        assert!(legacy.is_default());
        assert!((1..=8).contains(&legacy.effective_threads()));
        let accurate: DecodingOptions =
            serde_json::from_str(r#"{"strategy":"accurate","threads":3}"#).unwrap();
        assert_eq!(accurate.strategy, DecodingStrategy::Accurate);
        assert_eq!(accurate.effective_threads(), 3);
        assert!(!accurate.is_default());
    }

    #[test]
    fn thread_policy_rejects_unbounded_or_zero_requests() {
        for threads in [0, 9, 255] {
            assert!(DecodingOptions {
                threads: Some(threads),
                ..Default::default()
            }
            .validate()
            .is_err());
        }
        assert!(serde_json::from_str::<DecodingOptions>(r#"{"strategy":"guess"}"#).is_err());
    }
}
