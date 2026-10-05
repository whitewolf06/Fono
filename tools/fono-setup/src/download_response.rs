use crate::{cache::PartialState, manifest::MAX_INSTALLER_BYTES, Error, Result};
use reqwest::{
    header::{HeaderMap, CONTENT_LENGTH, CONTENT_RANGE, ETAG, LAST_MODIFIED},
    StatusCode,
};

pub(crate) enum ResponsePlan {
    Body {
        offset: u64,
        total: Option<u64>,
        expected_bytes: Option<u64>,
        validator: Option<String>,
    },
    Complete,
    Restart,
}

fn integer(headers: &HeaderMap, name: reqwest::header::HeaderName) -> Result<Option<u64>> {
    let Some(value) = headers.get(name) else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| Error::Network)?
        .parse()
        .map_err(|_| Error::Network)?;
    Ok(Some(value))
}

fn validator(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get(ETAG).and_then(|value| value.to_str().ok()) {
        if value.len() <= 1024 && value.starts_with('"') && value.ends_with('"') {
            return Some(value.to_owned());
        }
    }
    headers
        .get(LAST_MODIFIED)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .map(str::to_owned)
}

pub(crate) fn inspect(
    status: StatusCode,
    headers: &HeaderMap,
    offset: u64,
    state: &PartialState,
) -> Result<ResponsePlan> {
    let length = integer(headers, CONTENT_LENGTH)?;
    if length.is_some_and(|length| length > MAX_INSTALLER_BYTES) {
        return Err(Error::Size);
    }
    let current_validator = validator(headers);
    if status == StatusCode::OK {
        if length == Some(0) {
            return Err(Error::Size);
        }
        return Ok(ResponsePlan::Body {
            offset: 0,
            total: length,
            expected_bytes: length,
            validator: current_validator,
        });
    }
    let range = headers
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if status == StatusCode::RANGE_NOT_SATISFIABLE {
        let total = range
            .strip_prefix("bytes */")
            .and_then(|value| value.parse::<u64>().ok());
        return Ok(
            if total == Some(offset)
                && total == state.total
                && offset > 0
                && state.validator.is_some()
            {
                ResponsePlan::Complete
            } else {
                ResponsePlan::Restart
            },
        );
    }
    if status != StatusCode::PARTIAL_CONTENT {
        return Err(Error::Network);
    }
    let Some((bounds, total)) = range
        .strip_prefix("bytes ")
        .and_then(|value| value.split_once('/'))
    else {
        return Ok(ResponsePlan::Restart);
    };
    let Some((start, end)) = bounds.split_once('-') else {
        return Ok(ResponsePlan::Restart);
    };
    let parsed = (
        start.parse::<u64>(),
        end.parse::<u64>(),
        total.parse::<u64>(),
    );
    let (Ok(start), Ok(end), Ok(total)) = parsed else {
        return Ok(ResponsePlan::Restart);
    };
    if total == 0 || total > MAX_INSTALLER_BYTES {
        return Err(Error::Size);
    }
    if start != offset
        || end < start
        || end >= total
        || length.is_some_and(|length| length != end - start + 1)
        || (offset > 0 && state.total.is_some_and(|previous| previous != total))
        || (offset > 0
            && current_validator
                .as_ref()
                .is_some_and(|value| Some(value) != state.validator.as_ref()))
    {
        return Ok(ResponsePlan::Restart);
    }
    Ok(ResponsePlan::Body {
        offset,
        total: Some(total),
        expected_bytes: Some(end - start + 1),
        validator: current_validator.or_else(|| state.validator.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn headers(range: &str, length: u64) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_RANGE, range.parse().unwrap());
        headers.insert(CONTENT_LENGTH, length.to_string().parse().unwrap());
        headers
    }

    #[test]
    fn ignores_range_with_200_and_restarts_at_zero() {
        assert!(matches!(
            inspect(
                StatusCode::OK,
                &headers("", 10),
                5,
                &PartialState::default()
            )
            .unwrap(),
            ResponsePlan::Body { offset: 0, .. }
        ));
    }

    #[test]
    fn rejects_wrong_content_range_and_length() {
        for (range, length) in [
            ("bytes 4-9/10", 6),
            ("bytes 5-9/10", 4),
            ("bytes 5-10/10", 6),
            ("bytes 5-9/*", 5),
        ] {
            assert!(matches!(
                inspect(
                    StatusCode::PARTIAL_CONTENT,
                    &headers(range, length),
                    5,
                    &PartialState::default()
                )
                .unwrap(),
                ResponsePlan::Restart
            ));
        }
    }

    #[test]
    fn handles_complete_and_invalid_416() {
        let state = PartialState {
            validator: Some("\"v1\"".into()),
            total: Some(10),
        };
        assert!(matches!(
            inspect(
                StatusCode::RANGE_NOT_SATISFIABLE,
                &headers("bytes */10", 0),
                10,
                &state
            )
            .unwrap(),
            ResponsePlan::Complete
        ));
        assert!(matches!(
            inspect(
                StatusCode::RANGE_NOT_SATISFIABLE,
                &headers("bytes */11", 0),
                10,
                &state
            )
            .unwrap(),
            ResponsePlan::Restart
        ));
    }

    #[test]
    fn rejects_resume_when_validator_or_total_changes() {
        let state = PartialState {
            validator: Some("\"v1\"".into()),
            total: Some(10),
        };
        let mut headers = headers("bytes 5-9/10", 5);
        headers.insert(ETAG, "\"v2\"".parse().unwrap());
        assert!(matches!(
            inspect(StatusCode::PARTIAL_CONTENT, &headers, 5, &state).unwrap(),
            ResponsePlan::Restart
        ));
        assert!(matches!(
            inspect(
                StatusCode::PARTIAL_CONTENT,
                &self::headers("bytes 5-9/11", 5),
                5,
                &state
            )
            .unwrap(),
            ResponsePlan::Restart
        ));
    }
}
