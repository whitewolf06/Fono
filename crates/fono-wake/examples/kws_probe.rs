//! File-only fixed-keyword candidate. No microphone, application activation or downloads.
use serde::{Deserialize, Serialize};
use sherpa_onnx::{KeywordSpotter, KeywordSpotterConfig, Wave};
use std::{io::Read, path::PathBuf, time::Instant};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    model_dir: PathBuf,
    keyword_tokens: String,
    wavs: Vec<PathBuf>,
}
#[derive(Serialize)]
struct Clip {
    audio_samples: usize,
    context_samples: usize,
    events_ms: Vec<f64>,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::io::stdin().take(65537).read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err("request exceeds 64 KiB".into());
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    if request.wavs.is_empty()
        || request.wavs.len() > 200
        || request.keyword_tokens.len() > 512
        || request
            .keyword_tokens
            .contains(['\n', '\r', '\0', '@', ':', '#'])
    {
        return Err("invalid fixed keyword request".into());
    }
    let path = |name: &str| Some(request.model_dir.join(name).to_string_lossy().into_owned());
    let mut config = KeywordSpotterConfig::default();
    config.model_config.transducer.encoder =
        path("encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
    config.model_config.transducer.decoder =
        path("decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
    config.model_config.transducer.joiner =
        path("joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
    config.model_config.tokens = path("tokens.txt");
    config.model_config.num_threads = 1;
    config.model_config.provider = Some("cpu".into());
    config.keywords_buf = Some(format!("{} @HEY_FONO\n", request.keyword_tokens));
    let loading = Instant::now();
    let kws = KeywordSpotter::create(&config).ok_or("keyword model load failed")?;
    let model_load_ms = loading.elapsed().as_secs_f64() * 1000.;
    let started = Instant::now();
    let mut clips = Vec::new();
    for path in &request.wavs {
        if std::fs::metadata(path)?.len() > 704_044 {
            return Err("recording exceeds bounded sample size".into());
        }
        let wave = Wave::read(&path.to_string_lossy()).ok_or("could not read recording")?;
        if wave.sample_rate() != 16000 || wave.samples().is_empty() {
            return Err("expected nonempty 16 kHz WAV".into());
        }
        let stream = kws.create_stream();
        // Explicit synthetic context lets the streaming model settle after a user pressed Stop.
        // It is never counted as recorded negative audio or phrase-end ground truth.
        let tail = vec![0f32; 10_240];
        let mut clock = 0;
        let mut events = Vec::new();
        let mut cooldown_until = 0;
        let mut decode = |clock: usize| {
            while kws.is_ready(&stream) {
                kws.decode(&stream);
                if kws
                    .get_result(&stream)
                    .is_some_and(|r| !r.keyword.is_empty())
                {
                    if clock >= cooldown_until {
                        events.push(clock as f64 / 16.);
                        cooldown_until = clock + 32_000;
                    }
                    kws.reset(&stream);
                }
            }
        };
        for chunk in wave.samples().chunks(320).chain(tail.chunks(320)) {
            clock += chunk.len();
            stream.accept_waveform(16000, chunk);
            decode(clock);
        }
        stream.input_finished();
        decode(clock);
        clips.push(Clip {
            audio_samples: wave.samples().len(),
            context_samples: tail.len(),
            events_ms: events,
        });
    }
    println!(
        "{}",
        serde_json::json!({"schema":"fono-fixed-kws-probe@1", "model_load_ms":model_load_ms, "wall_ms":started.elapsed().as_secs_f64()*1000., "keyword_score":1.0, "keyword_threshold":0.25, "clips":clips, "qualified":false})
    );
    Ok(())
}
