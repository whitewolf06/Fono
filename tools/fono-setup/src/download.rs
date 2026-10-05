use crate::{
    cache::{Cache, PartialState},
    download_response::{inspect, ResponsePlan},
    manifest::{self, Release, MAX_INSTALLER_BYTES, MAX_MANIFEST_BYTES},
    Config, Error, Phase, Progress, Result,
};
use reqwest::{
    blocking::{Client, Response},
    header::{ACCEPT_ENCODING, IF_RANGE, LOCATION, RANGE},
    redirect::Policy,
    Url,
};
use std::{
    io::{Read, Write},
    sync::atomic::AtomicBool,
    thread,
    time::{Duration, Instant},
};

// reqwest's blocking client applies this timeout to each connect/read/write
// operation. Active transfers can continue; a stalled read bounds cancellation.
const IO_TIMEOUT: Duration = Duration::from_secs(20);
const DOWNLOAD_BUDGET: Duration = Duration::from_secs(30 * 60);
const MAX_FAILURES: u32 = 3;
const RANGE_CHUNK_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) fn range_header(offset: u64) -> String {
    if offset == MAX_INSTALLER_BYTES {
        // A complete maximum-sized partial can legitimately receive 416.
        // Do not create a range whose end is before its start.
        return format!("bytes={offset}-");
    }
    let end = (offset + RANGE_CHUNK_BYTES - 1).min(MAX_INSTALLER_BYTES - 1);
    format!("bytes={offset}-{end}")
}

pub(crate) fn client() -> Result<Client> {
    Client::builder()
        .https_only(true)
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(IO_TIMEOUT)
        .user_agent(concat!("FonoSetup/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| Error::Configuration)
}

fn request(
    client: &Client,
    config: &Config,
    origin: &Url,
    offset: u64,
    validator: Option<&str>,
    cancel: &AtomicBool,
) -> Result<Response> {
    let mut url = origin.clone();
    for _ in 0..=5 {
        crate::check_cancel(cancel)?;
        let mut request = client.get(url.clone()).header(ACCEPT_ENCODING, "identity");
        if offset > 0 {
            let validator = validator.ok_or(Error::Network)?;
            request = request
                .header(RANGE, range_header(offset))
                .header(IF_RANGE, validator);
        }
        let response = request.send();
        crate::check_cancel(cancel)?;
        let response = response.map_err(|_| Error::Network)?;
        if !response.status().is_redirection() {
            return Ok(response);
        }
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(Error::UnsafeUrl)?;
        url = url.join(location).map_err(|_| Error::UnsafeUrl)?;
        manifest::validate_redirect(&url, &config.repository)?;
    }
    Err(Error::Network)
}

fn wait_retry(cancel: &AtomicBool, attempt: u32) -> Result<()> {
    for _ in 0..attempt * 10 {
        crate::check_cancel(cancel)?;
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

pub(crate) fn resolve(client: &Client, config: &Config, cancel: &AtomicBool) -> Result<Release> {
    let origin = Url::parse(&config.endpoint).map_err(|_| Error::Configuration)?;
    for attempt in 1..=MAX_FAILURES {
        let result = (|| {
            let response = request(client, config, &origin, 0, None, cancel)?;
            if !response.status().is_success() {
                return Err(Error::Network);
            }
            if response
                .content_length()
                .is_some_and(|length| length > MAX_MANIFEST_BYTES)
            {
                return Err(Error::Manifest);
            }
            let mut bytes = Vec::new();
            response
                .take(MAX_MANIFEST_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| Error::Network)?;
            crate::check_cancel(cancel)?;
            manifest::parse(&bytes, config)
        })();
        match result {
            Err(Error::Network) if attempt < MAX_FAILURES => wait_retry(cancel, attempt)?,
            result => return result,
        }
    }
    Err(Error::Network)
}

pub(crate) fn installer(
    client: &Client,
    config: &Config,
    release: &Release,
    cache: &Cache,
    cancel: &AtomicBool,
    progress: &impl Fn(Progress),
) -> Result<()> {
    installer_with_budget(
        client,
        config,
        release,
        cache,
        cancel,
        progress,
        DOWNLOAD_BUDGET,
    )
}

pub(crate) fn installer_with_budget(
    client: &Client,
    config: &Config,
    release: &Release,
    cache: &Cache,
    cancel: &AtomicBool,
    progress: &impl Fn(Progress),
    budget: Duration,
) -> Result<()> {
    let started = Instant::now();
    let mut failures = 0;
    loop {
        crate::check_cancel(cancel)?;
        if started.elapsed() >= budget {
            return Err(Error::Network);
        }
        let mut state = cache.load_state();
        crate::cache::reject_link(&cache.partial)?;
        let mut offset = cache
            .partial
            .metadata()
            .map(|value| value.len())
            .unwrap_or(0);
        if offset > MAX_INSTALLER_BYTES {
            cache.reset_partial()?;
            return Err(Error::Size);
        }
        if offset > 0
            && (state.validator.is_none()
                || state
                    .total
                    .is_some_and(|total| offset > total || total > MAX_INSTALLER_BYTES))
        {
            cache.reset_partial()?;
            state = PartialState::default();
            offset = 0;
        }
        progress(Progress {
            phase: Phase::Downloading,
            downloaded_bytes: offset,
            total_bytes: state.total,
            version: Some(release.version.clone()),
        });
        let response = request(
            client,
            config,
            &release.url,
            offset,
            state.validator.as_deref(),
            cancel,
        );
        let mut response = match response {
            Err(Error::Network) => {
                failures += 1;
                if failures >= MAX_FAILURES {
                    return Err(Error::Network);
                }
                wait_retry(cancel, failures)?;
                continue;
            }
            result => result?,
        };
        let plan = match inspect(response.status(), response.headers(), offset, &state) {
            Err(Error::Network) => {
                failures += 1;
                if failures >= MAX_FAILURES {
                    return Err(Error::Network);
                }
                wait_retry(cancel, failures)?;
                continue;
            }
            result => result?,
        };
        let (offset, total, expected_bytes, validator) = match plan {
            ResponsePlan::Complete => return Ok(()),
            ResponsePlan::Restart => {
                cache.reset_partial()?;
                failures += 1;
                if failures >= MAX_FAILURES {
                    return Err(Error::Network);
                }
                continue;
            }
            ResponsePlan::Body {
                offset,
                total,
                expected_bytes,
                validator,
            } => (offset, total, expected_bytes, validator),
        };
        let mut file = crate::cache::open_regular(&cache.partial, offset > 0)?;
        if offset == 0 {
            file.set_len(0)?;
        }
        let resumable = validator.is_some();
        cache.save_state(&PartialState { validator, total })?;
        let mut transferred = 0;
        let mut buffer = [0u8; 64 * 1024];
        let read_result = loop {
            crate::check_cancel(cancel)?;
            if started.elapsed() >= budget {
                break Err(Error::Network);
            }
            match response.read(&mut buffer) {
                Ok(0) => break Ok(()),
                Ok(read) => {
                    transferred += read as u64;
                    if offset + transferred > MAX_INSTALLER_BYTES
                        || expected_bytes.is_some_and(|expected| transferred > expected)
                    {
                        cache.reset_partial()?;
                        return Err(Error::Size);
                    }
                    file.write_all(&buffer[..read])?;
                    progress(Progress {
                        phase: Phase::Downloading,
                        downloaded_bytes: offset + transferred,
                        total_bytes: total,
                        version: Some(release.version.clone()),
                    });
                }
                Err(_) => break Err(Error::Network),
            }
        };
        file.flush()?;
        file.sync_all()?;
        drop(file);
        crate::check_cancel(cancel)?;
        let downloaded = offset + transferred;
        if read_result.is_ok() && expected_bytes.is_none_or(|expected| transferred == expected) {
            if total.is_none_or(|total| downloaded == total) {
                return Ok(());
            }
            if transferred > 0 {
                failures = 0;
                continue;
            }
        }
        // A stalled or interrupted transfer resumes from the original GitHub
        // URL, obtaining a fresh asset token. An active transfer stays open;
        // its overall duration is bounded independently by DOWNLOAD_BUDGET.
        if transferred > 0 && resumable {
            failures = 0;
        }
        failures += 1;
        if failures >= MAX_FAILURES {
            return Err(Error::Network);
        }
        wait_retry(cancel, failures)?;
    }
}
