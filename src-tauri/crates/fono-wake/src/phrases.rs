#[cfg(feature = "sherpa-wake")]
use crate::error::{WakeWordError, WakeWordResult};

pub(crate) const SHERPA_SUPPORTED_PHRASES: [&str; 3] = ["hey fono", "okay fun", "рамзи"];

#[cfg(feature = "sherpa-wake")]
const HEY_FONO_KEYWORDS: &[&str] = &[
    "▁HE Y ▁F ON O",
    "▁HE Y ▁PH ON O",
    "▁HE Y ▁PH ONE ▁O",
    "▁HE Y ▁F UN O",
    "▁SHE ▁PH ON O",
    "▁SHE ▁F ON O",
    "▁PH ON O",
    "▁F ON O",
];

#[cfg(feature = "sherpa-wake")]
const OKAY_FUN_KEYWORDS: &[&str] = &["▁OKAY ▁F UN"];

#[cfg(feature = "sherpa-wake")]
const RAMZI_KEYWORDS: &[&str] = &["▁RA M Z I"];

/// Returns the bundled Sherpa-ONNX keyword graph for a user-facing phrase.
///
/// The spellings in each graph are intentional BPE alternatives, not separate
/// phrases. The user-facing Cyrillic phrase `рамзи` is represented by the
/// available English-model BPE spelling `RAMZI`.
#[cfg(feature = "sherpa-wake")]
pub(crate) fn sherpa_phrase_to_tokens(phrase: &str) -> WakeWordResult<String> {
    let normalized = phrase.trim().to_uppercase();

    let keywords = match normalized.as_str() {
        "" | "HEY FONO" => HEY_FONO_KEYWORDS,
        "OKAY FUN" => OKAY_FUN_KEYWORDS,
        "РАМЗИ" => RAMZI_KEYWORDS,
        _ => {
            return Err(WakeWordError::Backend(format!(
                "Sherpa-ONNX supports only these bundled wake phrases: {} (requested: {phrase})",
                SHERPA_SUPPORTED_PHRASES.join(", ")
            )));
        }
    };

    Ok(format!("{}\n", keywords.join("\n")))
}

#[cfg(all(test, feature = "sherpa-wake"))]
mod tests {
    use super::{sherpa_phrase_to_tokens, SHERPA_SUPPORTED_PHRASES};

    #[test]
    fn default_and_explicit_hey_fono_use_the_same_bpe_graph() {
        let expected = concat!(
            "▁HE Y ▁F ON O\n",
            "▁HE Y ▁PH ON O\n",
            "▁HE Y ▁PH ONE ▁O\n",
            "▁HE Y ▁F UN O\n",
            "▁SHE ▁PH ON O\n",
            "▁SHE ▁F ON O\n",
            "▁PH ON O\n",
            "▁F ON O\n"
        );

        assert_eq!(sherpa_phrase_to_tokens("").unwrap(), expected);
        assert_eq!(sherpa_phrase_to_tokens(" Hey Fono ").unwrap(), expected);
    }

    #[test]
    fn ramzi_uses_the_model_bpe_tokens() {
        assert_eq!(sherpa_phrase_to_tokens("рамзи").unwrap(), "▁RA M Z I\n");
        assert_eq!(sherpa_phrase_to_tokens(" РАМЗИ ").unwrap(), "▁RA M Z I\n");
    }

    #[test]
    fn supported_phrase_list_matches_keyword_graphs() {
        assert_eq!(SHERPA_SUPPORTED_PHRASES, ["hey fono", "okay fun", "рамзи"]);

        for phrase in SHERPA_SUPPORTED_PHRASES {
            assert!(sherpa_phrase_to_tokens(phrase).is_ok());
        }
    }

    #[test]
    fn unsupported_phrase_returns_actionable_error() {
        let error = sherpa_phrase_to_tokens("привет фоно").unwrap_err();

        assert!(error.to_string().contains("рамзи"));
        assert!(error.to_string().contains("привет фоно"));
    }
}
