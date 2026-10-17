//! Explicit public-WAV replay of the exact neural admission/trimming projection.
#[path = "support/pcm_wav.rs"]
mod pcm_wav;

#[cfg(feature = "speech-vad")]
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: trim-vad-wav INPUT16k.wav OUTPUT16k.wav SILERO.onnx".into());
    }
    let samples = pcm_wav::read(&args[1])?;
    let mut detector = fono_audio::StreamingSpeechVad::new(std::path::Path::new(&args[3]))
        .map_err(|error| error.to_string())?;
    let decision = detector
        .analyze_recording(&samples, || false)
        .ok_or("cancelled")?;
    let bounds = fono_audio::recorded_speech_bounds(samples.len(), &decision);
    // Silence is excluded before STT in production. Keep a valid silent WAV for
    // corpus accounting so a no-speech decision cannot disappear from WER.
    let output = if decision.has_speech {
        &samples[bounds.start_sample..]
    } else {
        &[]
    };
    pcm_wav::write(&args[2], output)?;
    println!("{{\"has_speech\":{},\"input_samples\":{},\"first_speech_sample\":{},\"last_speech_sample\":{},\"leading_trimmed_samples\":{},\"output_samples\":{},\"full_capture_retained\":true}}",
        decision.has_speech, samples.len(),
        decision.first_speech_sample.unwrap_or(0), decision.last_speech_sample.unwrap_or(0),
        bounds.start_sample, output.len());
    Ok(())
}

#[cfg(not(feature = "speech-vad"))]
fn run() -> Result<(), String> {
    // Keep the no-default-features example build useful and explicit.
    let _ = (
        pcm_wav::read as fn(&str) -> Result<Vec<i16>, String>,
        pcm_wav::write as fn(&str, &[i16]) -> Result<(), String>,
    );
    Err("trim-vad-wav requires speech-vad".into())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
