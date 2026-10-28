//! cargo run -p fono-wake --no-default-features --features sherpa-wake
//! --example wake_replay -- ru MODEL_DIR PHRASE corpus.json
//! Corpus JSON: [{"wav":"sample.wav","positive":true,"holdout":true,
//! "phrase_end_ms":1400},{"wav":"negative.wav","positive":false}]
#[cfg(feature = "sherpa-wake")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("expected BACKEND(ru|en|kws) MODEL_DIR PHRASE CORPUS_JSON".into());
    }
    let backend = match args[0].as_str() {
        "ru" => fono_wake::WakeWordBackend::SherpaStreamingRu,
        "en" => fono_wake::WakeWordBackend::SherpaStreamingEn,
        "kws" => fono_wake::WakeWordBackend::SherpaOnnx,
        _ => return Err("unknown backend".into()),
    };
    let config = fono_wake::WakeWordConfig {
        backend,
        model_dir: args[1].clone().into(),
        phrase: args[2].clone(),
        ..Default::default()
    };
    let corpus: Vec<fono_wake::replay::CorpusItem> =
        serde_json::from_slice(&std::fs::read(&args[3])?)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&fono_wake::replay::evaluate_corpus(&config, &corpus)?)?
    );
    Ok(())
}
#[cfg(not(feature = "sherpa-wake"))]
fn main() {
    eprintln!("wake_replay requires --features sherpa-wake");
    std::process::exit(2);
}
