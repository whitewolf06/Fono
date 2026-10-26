use super::{check_cancel, manifest, PackageError, Result};
use reqwest::{
    blocking::{Client, Response},
    header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_RANGE, LOCATION, RANGE},
    redirect::Policy,
    Url,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

const PREFIX: &str = "https://github.com/whitewolf06/fono/releases/download/";
const CHUNK: u64 = 8 * 1024 * 1024;
const BUDGET: Duration = Duration::from_secs(30 * 60);

pub(super) fn asset(version: &str, name: &str) -> Result<Url> {
    // Production passes only the compile-time version and a validated signed
    // archive name. Do not accept a URL or key from a renderer/manifest.
    let url = Url::parse(&format!("{PREFIX}v{version}/{name}"))
        .map_err(|_| PackageError::Configuration)?;
    if version.is_empty()
        || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        || name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
    {
        return Err(PackageError::Configuration);
    }
    Ok(url)
}

pub(super) fn valid_redirect(url: &Url) -> bool {
    if url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    match url.host_str() {
        Some("github.com") => {
            url.query().is_none() && url.as_str().starts_with(PREFIX) && !url.path().contains('%')
        }
        Some("release-assets.githubusercontent.com") => true,
        _ => false,
    }
}

pub(super) fn client() -> Result<Client> {
    Client::builder()
        .https_only(true)
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        // Downloads use bounded range requests. A stalled request, including
        // response reads, cannot retain a cancellation for more than 20 sec.
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("FonoCuda/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| PackageError::Network)
}

fn request(
    client: &Client,
    origin: &Url,
    range: Option<(u64, u64)>,
    cancel: &AtomicBool,
) -> Result<Response> {
    if !valid_redirect(origin) || origin.host_str() != Some("github.com") {
        return Err(PackageError::Network);
    }
    let mut url = origin.clone();
    for _ in 0..=5 {
        check_cancel(cancel)?;
        let mut request = client.get(url.clone()).header(ACCEPT_ENCODING, "identity");
        if let Some((start, end)) = range {
            request = request.header(RANGE, format!("bytes={start}-{end}"));
        }
        let response = request.send();
        check_cancel(cancel)?;
        let response = response.map_err(|_| PackageError::Network)?;
        if !response.status().is_redirection() {
            return Ok(response);
        }
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(PackageError::Network)?;
        url = url.join(location).map_err(|_| PackageError::Network)?;
        if !valid_redirect(&url) {
            return Err(PackageError::Network);
        }
    }
    Err(PackageError::Network)
}

pub(super) fn small(
    client: &Client,
    origin: &Url,
    limit: u64,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    let mut response = request(client, origin, None, cancel)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(PackageError::Unpublished);
    }
    if response.status() != reqwest::StatusCode::OK
        || response.content_length().is_some_and(|size| size > limit)
    {
        return Err(PackageError::Network);
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PackageError::Network)?;
    check_cancel(cancel)?;
    if bytes.is_empty() || bytes.len() as u64 > limit {
        return Err(PackageError::Network);
    }
    Ok(bytes)
}

pub(super) fn archive(
    client: &Client,
    manifest: &manifest::Manifest,
    destination: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(u64, u64),
) -> Result<()> {
    let origin = asset(&manifest.app_version, &manifest.archive.name)?;
    super::storage::reject_links(destination)?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|_| PackageError::Storage)?;
    transfer(client, &origin, file, &manifest.archive, cancel, progress)
}

fn transfer(
    client: &Client,
    origin: &Url,
    mut file: File,
    expected: &manifest::Archive,
    cancel: &AtomicBool,
    progress: impl Fn(u64, u64),
) -> Result<()> {
    let started = Instant::now();
    let mut count = 0u64;
    let mut digest = Sha256::new();
    progress(0, expected.size);
    while count < expected.size {
        check_cancel(cancel)?;
        if started.elapsed() >= BUDGET {
            return Err(PackageError::Network);
        }
        let end = (count + CHUNK - 1).min(expected.size - 1);
        let mut response = request(client, origin, Some((count, end)), cancel)?;
        let content_range = format!("bytes {count}-{end}/{}", expected.size);
        if response.status() != reqwest::StatusCode::PARTIAL_CONTENT
            || response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|value| value.to_str().ok())
                != Some(content_range.as_str())
            || response
                .content_length()
                .is_some_and(|size| size != end - count + 1)
            || response
                .headers()
                .get(CONTENT_ENCODING)
                .is_some_and(|encoding| encoding != "identity")
        {
            return Err(PackageError::Network);
        }
        let chunk_size = end - count + 1;
        copy_chunk(
            &mut response,
            &mut file,
            &mut digest,
            count,
            chunk_size,
            expected.size,
            cancel,
            &|| started.elapsed(),
            &progress,
        )?;
        count += chunk_size;
    }
    check_cancel(cancel)?;
    if count != expected.size || format!("{:x}", digest.finalize()) != expected.sha256 {
        return Err(PackageError::Hash);
    }
    file.sync_all().map_err(|_| PackageError::Storage)
}

#[allow(clippy::too_many_arguments)]
fn copy_chunk(
    reader: &mut impl Read,
    writer: &mut impl Write,
    digest: &mut Sha256,
    offset: u64,
    expected_chunk: u64,
    expected_total: u64,
    cancel: &AtomicBool,
    elapsed: &impl Fn() -> Duration,
    progress: &impl Fn(u64, u64),
) -> Result<()> {
    let mut buffer = [0; 64 * 1024];
    let mut chunk = 0u64;
    loop {
        check_cancel(cancel)?;
        if elapsed() >= BUDGET {
            return Err(PackageError::Network);
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|_| PackageError::Network)?;
        check_cancel(cancel)?;
        if read == 0 {
            break;
        }
        chunk += read as u64;
        if chunk > expected_chunk || offset + chunk > expected_total {
            return Err(PackageError::Hash);
        }
        digest.update(&buffer[..read]);
        writer
            .write_all(&buffer[..read])
            .map_err(|_| PackageError::Storage)?;
        progress(offset + chunk, expected_total);
    }
    if chunk != expected_chunk {
        return Err(PackageError::Network);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Cursor, sync::atomic::Ordering};

    #[test]
    fn interrupted_and_oversized_downloads_are_rejected_without_publishing_extra_bytes() {
        for (data, expected, error) in [
            (b"short".as_slice(), 20, PackageError::Network),
            (b"too long".as_slice(), 2, PackageError::Hash),
        ] {
            let mut output = Vec::new();
            let result = copy_chunk(
                &mut Cursor::new(data),
                &mut output,
                &mut Sha256::new(),
                0,
                expected,
                expected,
                &AtomicBool::new(false),
                &|| Duration::ZERO,
                &|_, _| {},
            );
            assert_eq!(result, Err(error));
            assert!(output.len() <= expected as usize);
        }
    }

    #[test]
    fn cancellation_during_a_blocking_read_does_not_publish_the_late_chunk() {
        struct Cancelling<'a>(&'a AtomicBool);
        impl Read for Cancelling<'_> {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                self.0.store(true, Ordering::Release);
                bytes[0] = b'x';
                Ok(1)
            }
        }
        let cancel = AtomicBool::new(false);
        let mut output = Vec::new();
        assert_eq!(
            copy_chunk(
                &mut Cancelling(&cancel),
                &mut output,
                &mut Sha256::new(),
                0,
                1,
                1,
                &cancel,
                &|| Duration::ZERO,
                &|_, _| {}
            ),
            Err(PackageError::Cancelled)
        );
        assert!(output.is_empty());
    }

    #[test]
    fn overall_budget_and_successful_stream_hash_are_checked() {
        let cancel = AtomicBool::new(false);
        let mut output = Vec::new();
        let mut digest = Sha256::new();
        assert_eq!(
            copy_chunk(
                &mut Cursor::new(b"bytes"),
                &mut output,
                &mut digest,
                0,
                5,
                5,
                &cancel,
                &|| BUDGET,
                &|_, _| {}
            ),
            Err(PackageError::Network)
        );
        assert!(output.is_empty());
        copy_chunk(
            &mut Cursor::new(b"bytes"),
            &mut output,
            &mut digest,
            0,
            5,
            5,
            &cancel,
            &|| Duration::ZERO,
            &|done, total| assert!(done <= total),
        )
        .unwrap();
        assert_eq!(output, b"bytes");
        assert_eq!(format!("{:x}", digest.finalize()), manifest::hash(b"bytes"));
    }
}
