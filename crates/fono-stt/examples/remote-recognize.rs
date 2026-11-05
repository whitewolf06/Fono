//! Explicit, file-only smoke check. Never records audio or prints a credential
//! or transcript. See docs/remote-recognizer.md for the protected Kari tunnel.
use fono_core::OperationCancellation;
use fono_stt::{remote::RemoteEndpoint, SttEngine};
use fono_types::{ModelId, SttEngine as ModelEngine};
use std::{fs, time::Instant};
use zeroize::Zeroizing;

fn main() {
    if let Err(error) = run() {
        eprintln!("Remote recognition check failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(args.len() == 4
        || (args.len() == 5
            && matches!(
                args[4].as_str(),
                "--source-roundtrip" | "--health-only" | "--batch"
            )))
    {
        return Err("Usage: remote-recognize <server-root-url> <token-file> <mono-16k-pcm16.wav or -> <model-id> [--source-roundtrip|--health-only|--batch]".into());
    }
    let token =
        Zeroizing::new(fs::read_to_string(&args[1]).map_err(|_| "Cannot read the token file")?);
    let endpoint = RemoteEndpoint::new(&args[0], token.trim())?;
    let (family, variant) = args[3]
        .split_once(':')
        .ok_or("Expected a public model identifier")?;
    let model = ModelId {
        engine: match family {
            "whisper" => ModelEngine::Whisper,
            "giga_am" => ModelEngine::GigaAm,
            "parakeet" => ModelEngine::Parakeet,
            _ => return Err("Unknown model family".into()),
        },
        variant: variant.into(),
    };
    if args.get(4).is_some_and(|arg| arg == "--health-only") {
        let started = Instant::now();
        let health = fono_stt::remote::probe_health(
            endpoint,
            &model,
            std::sync::Arc::new(fono_stt::NoopActivity),
            &OperationCancellation::default(),
        )?;
        println!(
            "{}",
            serde_json::json!({ "health":health, "request_elapsed_ms":started.elapsed().as_millis(), "network_timeout_ms":fono_stt::remote::REMOTE_HEALTH_TIMEOUT.as_millis() })
        );
        return Ok(());
    }
    let wav = fs::read(&args[2]).map_err(|_| "Cannot read the WAV file")?;
    let samples = pcm_wav(&wav)?;
    if args.get(4).is_some_and(|arg| arg == "--batch") {
        let started = Instant::now();
        let cancellation = OperationCancellation::default();
        let client = fono_stt::remote::RemoteTranscriptionClient::connect(
            endpoint,
            model,
            std::sync::Arc::new(fono_stt::NoopActivity),
            &cancellation,
        )?;
        let result = client.transcribe_pcm_job(&samples, "en", &cancellation, &|_| {})?;
        if result.text.trim().is_empty() {
            return Err("The server returned an empty transcript".into());
        }
        println!(
            "{}",
            serde_json::json!({
                "recognized":true, "batch":true, "text_chars":result.text.chars().count(),
                "backend":result.backend, "model":result.model,
                "audio_seconds":result.audio_seconds, "transcribe_seconds":result.transcribe_seconds,
                "subtitle_segments":result.segments.len(),
                "request_elapsed_ms":started.elapsed().as_millis(), "local_stt_used":false
            })
        );
        return Ok(());
    }
    let engine = SttEngine::default();
    let cancellation = OperationCancellation::default();
    engine.connect_remote(endpoint.clone(), model.clone(), &cancellation)?;
    let started = Instant::now();
    let result = engine.transcribe_cancellable(&samples, "en", cancellation.clone())?;
    let request_elapsed_ms = started.elapsed().as_millis();
    if result.text.trim().is_empty() {
        return Err("The server returned an empty transcript".into());
    }
    let source_roundtrip = args.len() == 5;
    if source_roundtrip {
        engine.clear_remote_if_idle()?;
        if engine.is_remote() || engine.is_loaded() {
            return Err("Remote route was not cleared".into());
        }
        engine.ensure_remote_loaded(endpoint, &model, &cancellation)?;
        let returned = engine.transcribe_cancellable(&samples, "en", cancellation)?;
        if !engine.is_remote() || returned.text.trim().is_empty() || engine.gpu_residency().resident
        {
            return Err("Remote source return failed".into());
        }
    }
    println!(
        "{}",
        serde_json::json!({
            "recognized":true, "text_chars":result.text.chars().count(), "device":result.device,
            "audio_seconds":result.audio_secs, "decode_seconds":result.transcribe_secs,
            "remote":result.remote,
            "request_elapsed_ms":request_elapsed_ms, "local_gpu_resident":engine.gpu_residency().resident,
            "source_roundtrip":source_roundtrip
        })
    );
    Ok(())
}

fn pcm_wav(bytes: &[u8]) -> Result<Vec<i16>, &'static str> {
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err("Expected a RIFF/WAVE file");
    }
    let mut offset = 12;
    let mut format_valid = false;
    let mut data = None;
    while let Some(header) = bytes.get(offset..offset + 8) {
        let size = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        offset += 8;
        let body = bytes
            .get(offset..offset.checked_add(size).ok_or("Invalid WAV length")?)
            .ok_or("Truncated WAV")?;
        if &header[..4] == b"fmt " {
            format_valid = body.len() >= 16
                && body[..2] == [1, 0]
                && body[2..4] == [1, 0]
                && u32::from_le_bytes(body[4..8].try_into().unwrap()) == 16_000
                && body[14..16] == [16, 0];
        }
        if &header[..4] == b"data" {
            data = Some(body);
        }
        offset = offset
            .checked_add(size + (size & 1))
            .ok_or("Invalid WAV length")?;
    }
    let data = data.ok_or("WAV has no audio data")?;
    if !format_valid || data.is_empty() || data.len() % 2 != 0 || data.len() > 64 * 1024 * 1024 {
        return Err("Use signed PCM16 mono at 16 kHz, at most 64 MiB");
    }
    Ok(data
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
        .collect())
}
