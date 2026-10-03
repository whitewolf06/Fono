use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::Url;

pub(super) struct Channel {
    pub endpoint: Url,
    pub public_key: String,
}

impl Channel {
    pub fn from_build() -> Option<Self> {
        if !cfg!(windows) {
            return None;
        }
        Self::parse(
            option_env!("FONO_UPDATER_ENDPOINT"),
            option_env!("FONO_UPDATER_PUBLIC_KEY"),
        )
    }

    fn parse(endpoint: Option<&str>, public_key: Option<&str>) -> Option<Self> {
        let endpoint = endpoint?.trim();
        let public_key = public_key?.trim();
        if endpoint.len() > 2048 || public_key.len() > 4096 {
            return None;
        }
        let url = Url::parse(endpoint).ok()?;
        if !is_public_https(&url) || url.query().is_some() || url.fragment().is_some() {
            return None;
        }
        let decoded = STANDARD.decode(public_key).ok()?;
        let decoded = std::str::from_utf8(&decoded).ok()?;
        let line = decoded
            .lines()
            .find(|line| !line.trim().is_empty() && !line.starts_with("untrusted comment:"))?;
        let key = STANDARD.decode(line).ok()?;
        if key.len() != 42 || !matches!(&key[..2], b"Ed" | b"ED") {
            return None;
        }
        Some(Self {
            endpoint: url,
            public_key: public_key.into(),
        })
    }
}

pub(super) fn is_public_https(url: &Url) -> bool {
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    public_dns(url)
}

fn public_dns(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    host.parse::<std::net::IpAddr>().is_err()
        && host.contains('.')
        && !host.starts_with('[')
        && !host.ends_with('.')
        && !host.ends_with(".local")
        && !host.ends_with(".localhost")
        && !host.ends_with(".internal")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn public_fixture() -> String {
        // A structural public-key fixture, not a generated signing key.
        let mut bytes = [0u8; 42];
        bytes[..2].copy_from_slice(b"Ed");
        STANDARD.encode(format!(
            "untrusted comment: test public key\n{}",
            STANDARD.encode(bytes)
        ))
    }

    #[test]
    fn absent_or_invalid_configuration_cannot_request_updates() {
        assert!(Channel::parse(None, None).is_none());
        assert!(Channel::parse(Some("https://example.com/latest.json"), Some("broken")).is_none());
        assert!(Channel::parse(
            Some("https://example.com/latest.json"),
            Some(&public_fixture())
        )
        .is_some());
    }

    #[test]
    fn public_channel_rejects_insecure_local_and_authenticated_urls() {
        for endpoint in [
            "http://example.com/latest.json",
            "https://127.0.0.1/latest.json",
            "https://[::1]/latest.json",
            "https://user:secret@example.com/latest.json",
            "https://localhost/latest.json",
            "https://fono.local/latest.json",
            "https://example.com/latest.json?token=secret",
        ] {
            assert!(Channel::parse(Some(endpoint), Some(&public_fixture())).is_none());
        }
    }
}
