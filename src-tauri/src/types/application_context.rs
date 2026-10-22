//! Opt-in basename identity and observed send outcome; never field delivery.
use super::InjectionMode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationIdentity {
    pub id: String,
    pub name: String,
}
impl ApplicationIdentity {
    /// Accept a basename only. Paths and titles are not valid identity inputs.
    pub fn from_executable(name: &str) -> Option<Self> {
        if name.len() > 128
            || name.len() <= 4
            || name.trim() != name
            || !name.to_ascii_lowercase().ends_with(".exe")
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || " ._-".contains(c))
        {
            return None;
        }
        let id = format!("exe:{}", name.to_lowercase());
        let name = name[..name.len() - 4].to_owned();
        if name.trim().is_empty() {
            return None;
        }
        Some(Self { id, name })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsertionObservation {
    NotAttempted,
    Unknown,
    Sent,
    Partial,
    FocusChanged,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationContext {
    pub before: Option<ApplicationIdentity>,
    pub after: Option<ApplicationIdentity>,
    pub outcome: InsertionObservation,
    pub method: Option<InjectionMode>,
    pub attempted: bool,
}
impl Default for ApplicationContext {
    fn default() -> Self {
        Self {
            before: None,
            after: None,
            outcome: InsertionObservation::NotAttempted,
            method: None,
            attempted: false,
        }
    }
}
impl ApplicationContext {
    pub fn unknown_attempt(method: InjectionMode) -> Self {
        Self {
            outcome: InsertionObservation::Unknown,
            method: Some(method),
            attempted: true,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basename_identity_is_stable_but_never_accepts_paths_titles_or_urls() {
        assert_eq!(
            ApplicationIdentity::from_executable("Code.EXE"),
            ApplicationIdentity::from_executable("Code.exe")
        );
        assert_eq!(
            ApplicationIdentity::from_executable("Code.exe").unwrap().id,
            "exe:code.exe"
        );
        for invalid in [
            "C:\\private\\Code.exe",
            "../Code.exe",
            "https://site/Code.exe",
            "Code.exe\n",
            "Document - Code",
            ".exe",
        ] {
            assert!(ApplicationIdentity::from_executable(invalid).is_none());
        }
        assert!(ApplicationIdentity::from_executable("Редактор.exe").is_some());
    }
}
