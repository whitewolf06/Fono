//! Safe file-to-PCM adapter for the local transcription service.
//!
//! The source file is decoded as a stream; only the PCM required by the
//! current Whisper contract is retained. Upload transport and job persistence
//! belong to later application layers.

use std::fs::File;
use std::path::Path;

use audiopus::{
    coder::Decoder as OpusDecoder, Channels as OpusChannels, SampleRate as OpusSampleRate,
};
use serde::Serialize;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_OPUS};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

use crate::application::transcription_contract::TranscriptionRequest;

pub const WHISPER_SAMPLE_RATE: u32 = 16_000;
const OPUS_SAMPLE_RATE: u32 = 48_000;
const MAX_OPUS_PACKET_SAMPLES: usize = 5_760;

#[derive(Debug, Clone, Copy)]
pub struct AudioIngestPolicy {
    pub max_file_bytes: u64,
    pub max_duration_seconds: u32,
}

impl Default for AudioIngestPolicy {
    fn default() -> Self {
        Self {
            max_file_bytes: 100 * 1024 * 1024,
            max_duration_seconds: 3 * 60 * 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NormalizedAudio {
    pub pcm_samples: Vec<i16>,
    pub duration_seconds: f32,
    pub source_sample_rate: u32,
    pub source_channels: u16,
}

impl NormalizedAudio {
    /// Transfers normalized PCM to the Tauri-independent transcription use case.
    pub fn into_transcription_request(
        self,
        language: String,
        model: String,
    ) -> TranscriptionRequest {
        TranscriptionRequest {
            pcm_samples: self.pcm_samples,
            language,
            model,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum AudioIngestError {
    UnsupportedAudio(String),
    FileTooLarge(String),
    DurationLimit(String),
    CorruptedAudio(String),
    Io(String),
}

pub fn decode_file(
    path: &Path,
    policy: AudioIngestPolicy,
) -> Result<NormalizedAudio, AudioIngestError> {
    let extension = allowed_extension(path)?;
    let metadata =
        std::fs::metadata(path).map_err(|error| AudioIngestError::Io(error.to_string()))?;
    if !metadata.is_file() {
        return Err(AudioIngestError::UnsupportedAudio(
            "input must be a regular audio file".into(),
        ));
    }
    if metadata.len() > policy.max_file_bytes {
        return Err(AudioIngestError::FileTooLarge(format!(
            "audio file exceeds the {} byte limit",
            policy.max_file_bytes
        )));
    }

    let file = File::open(path).map_err(|error| AudioIngestError::Io(error.to_string()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    hint.with_extension(&extension);
    let probed = get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(map_probe_error)?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| AudioIngestError::UnsupportedAudio("audio track is missing".into()))?;
    let track_id = track.id;
    let codec = track.codec_params.codec;
    let source_sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| AudioIngestError::UnsupportedAudio("audio sample rate is missing".into()))?;
    if codec == CODEC_TYPE_OPUS {
        let source_channels = track
            .codec_params
            .channels
            .map(|channels| channels.count())
            .ok_or_else(|| {
                AudioIngestError::UnsupportedAudio("audio channel layout is missing".into())
            })?;
        return decode_ogg_opus(
            format,
            track_id,
            source_sample_rate,
            source_channels,
            policy,
        );
    }
    let mut decoder = get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(map_decode_error)?;
    let max_samples = policy.max_duration_seconds as usize * WHISPER_SAMPLE_RATE as usize;
    let mut pcm_samples = Vec::new();
    let mut source_channels = None;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(SymphoniaError::ResetRequired) => {
                return Err(AudioIngestError::CorruptedAudio(
                    "audio decoder reset is required".into(),
                ))
            }
            Err(error) => return Err(map_decode_error(error)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(error)) => {
                return Err(AudioIngestError::CorruptedAudio(error.to_string()))
            }
            Err(error) => return Err(map_decode_error(error)),
        };
        let channels = decoded.spec().channels.count();
        if channels == 0 {
            return Err(AudioIngestError::CorruptedAudio(
                "audio has no channels".into(),
            ));
        }
        source_channels.get_or_insert(channels as u16);
        if source_channels != Some(channels as u16) {
            return Err(AudioIngestError::CorruptedAudio(
                "audio channel count changed mid-stream".into(),
            ));
        }
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        buffer.copy_interleaved_ref(decoded);
        let mono = downmix_to_mono(buffer.samples(), channels);
        append_resampled(&mut pcm_samples, &mono, source_sample_rate, max_samples)?;
    }

    if pcm_samples.is_empty() {
        return Err(AudioIngestError::CorruptedAudio(
            "audio contains no decodable samples".into(),
        ));
    }
    Ok(NormalizedAudio {
        duration_seconds: pcm_samples.len() as f32 / WHISPER_SAMPLE_RATE as f32,
        pcm_samples,
        source_sample_rate,
        source_channels: source_channels.unwrap_or_default(),
    })
}

fn decode_ogg_opus(
    mut format: Box<dyn FormatReader>,
    track_id: u32,
    source_sample_rate: u32,
    source_channels: usize,
    policy: AudioIngestPolicy,
) -> Result<NormalizedAudio, AudioIngestError> {
    if source_sample_rate != OPUS_SAMPLE_RATE {
        return Err(AudioIngestError::UnsupportedAudio(format!(
            "Opus must use the {OPUS_SAMPLE_RATE} Hz decode rate"
        )));
    }
    let channels = opus_channels(source_channels)?;
    let mut decoder = OpusDecoder::new(OpusSampleRate::Hz48000, channels).map_err(|error| {
        AudioIngestError::CorruptedAudio(format!("Opus decoder init failed: {error}"))
    })?;
    let max_samples = policy.max_duration_seconds as usize * WHISPER_SAMPLE_RATE as usize;
    let mut pcm_samples = Vec::new();
    let mut decoded = vec![0_i16; MAX_OPUS_PACKET_SAMPLES * source_channels];

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(SymphoniaError::ResetRequired) => {
                return Err(AudioIngestError::CorruptedAudio(
                    "audio decoder reset is required".into(),
                ))
            }
            Err(error) => return Err(map_decode_error(error)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded_frames = decoder
            .decode(Some(packet.buf()), &mut decoded, false)
            .map_err(|error| {
                AudioIngestError::CorruptedAudio(format!("Opus decode failed: {error}"))
            })?;
        let decoded_len = decoded_frames
            .checked_mul(source_channels)
            .ok_or_else(|| AudioIngestError::CorruptedAudio("Opus packet is too large".into()))?;
        let decoded_packet = trim_opus_packet(
            &decoded[..decoded_len],
            source_channels,
            packet.trim_start() as usize,
            packet.trim_end() as usize,
        )?;
        let mono = downmix_i16_to_mono(decoded_packet, source_channels);
        append_resampled(&mut pcm_samples, &mono, source_sample_rate, max_samples)?;
    }

    if pcm_samples.is_empty() {
        return Err(AudioIngestError::CorruptedAudio(
            "audio contains no decodable samples".into(),
        ));
    }
    Ok(NormalizedAudio {
        duration_seconds: pcm_samples.len() as f32 / WHISPER_SAMPLE_RATE as f32,
        pcm_samples,
        source_sample_rate,
        source_channels: source_channels as u16,
    })
}

fn opus_channels(channels: usize) -> Result<OpusChannels, AudioIngestError> {
    match channels {
        1 => Ok(OpusChannels::Mono),
        2 => Ok(OpusChannels::Stereo),
        _ => Err(AudioIngestError::UnsupportedAudio(
            "OGG/Opus supports mono or stereo audio only".into(),
        )),
    }
}

fn trim_opus_packet(
    packet: &[i16],
    channels: usize,
    trim_start: usize,
    trim_end: usize,
) -> Result<&[i16], AudioIngestError> {
    let frames = packet.len() / channels;
    let start = trim_start.min(frames);
    let end = frames.saturating_sub(trim_end);
    if start > end {
        return Err(AudioIngestError::CorruptedAudio(
            "OGG/Opus packet trim metadata is invalid".into(),
        ));
    }
    Ok(&packet[start * channels..end * channels])
}

fn allowed_extension(path: &Path) -> Result<String, AudioIngestError> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| {
            AudioIngestError::UnsupportedAudio("audio file extension is required".into())
        })?;
    match extension.as_str() {
        "wav" | "mp3" | "flac" | "ogg" => Ok(extension),
        _ => Err(AudioIngestError::UnsupportedAudio(
            "supported formats are WAV, MP3, FLAC, OGG/Vorbis and OGG/Opus".into(),
        )),
    }
}

fn downmix_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn downmix_i16_to_mono(interleaved: &[i16], channels: usize) -> Vec<f32> {
    interleaved
        .chunks_exact(channels)
        .map(|frame| {
            frame.iter().map(|sample| *sample as f32).sum::<f32>()
                / (channels as f32 * i16::MAX as f32)
        })
        .collect()
}

fn append_resampled(
    output: &mut Vec<i16>,
    mono: &[f32],
    source_sample_rate: u32,
    max_samples: usize,
) -> Result<(), AudioIngestError> {
    let expected =
        ((mono.len() as u64 * WHISPER_SAMPLE_RATE as u64) / source_sample_rate as u64) as usize + 1;
    if output.len().saturating_add(expected) > max_samples {
        return Err(AudioIngestError::DurationLimit(
            "audio exceeds the configured duration limit".into(),
        ));
    }
    if source_sample_rate == WHISPER_SAMPLE_RATE {
        output.extend(mono.iter().copied().map(float_to_i16));
        return Ok(());
    }
    let ratio = source_sample_rate as f64 / WHISPER_SAMPLE_RATE as f64;
    let target_frames = ((mono.len() as f64) / ratio).ceil() as usize;
    for index in 0..target_frames {
        let position = index as f64 * ratio;
        let lower = position.floor() as usize;
        let upper = (lower + 1).min(mono.len().saturating_sub(1));
        let fraction = (position - lower as f64) as f32;
        let sample = mono[lower] + (mono[upper] - mono[lower]) * fraction;
        output.push(float_to_i16(sample));
    }
    Ok(())
}

fn float_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

fn map_probe_error(error: SymphoniaError) -> AudioIngestError {
    match error {
        SymphoniaError::Unsupported(message) => AudioIngestError::UnsupportedAudio(message.into()),
        other => AudioIngestError::CorruptedAudio(other.to_string()),
    }
}

fn map_decode_error(error: SymphoniaError) -> AudioIngestError {
    match error {
        SymphoniaError::Unsupported(message) => AudioIngestError::UnsupportedAudio(message.into()),
        SymphoniaError::DecodeError(message) => AudioIngestError::CorruptedAudio(message.into()),
        other => AudioIngestError::CorruptedAudio(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_extensions_outside_the_v1_contract() {
        assert!(matches!(
            allowed_extension(Path::new("voice.m4a")),
            Err(AudioIngestError::UnsupportedAudio(_))
        ));
    }

    #[test]
    fn downmixes_and_resamples_pcm_to_whisper_format() {
        let stereo = [0.5, -0.5, 1.0, 1.0];
        let mono = downmix_to_mono(&stereo, 2);
        let mut output = Vec::new();
        append_resampled(&mut output, &mono, 8_000, 16).unwrap();

        assert_eq!(output.len(), 4);
        assert_eq!(output[0], 0);
        assert!(output.iter().any(|sample| *sample > 0));
    }

    #[test]
    fn opus_packet_trimming_preserves_complete_interleaved_frames() {
        let samples = [10_i16, 11, 20, 21, 30, 31];

        assert_eq!(trim_opus_packet(&samples, 2, 1, 1).unwrap(), &[20_i16, 21]);
        assert!(matches!(
            opus_channels(3),
            Err(AudioIngestError::UnsupportedAudio(_))
        ));
    }

    #[test]
    fn decodes_the_existing_mp3_fixture_without_loading_the_source_file_whole() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/whisper.cpp/samples/jfk.mp3");
        let audio = decode_file(&path, AudioIngestPolicy::default()).unwrap();

        assert_eq!(audio.source_sample_rate, 16_000);
        assert!(!audio.pcm_samples.is_empty());
        assert!(audio.duration_seconds > 0.0);
    }

    #[test]
    fn decodes_the_existing_wav_fixture() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("vendor/whisper.cpp/bindings/go/samples/jfk.wav");
        let audio = decode_file(&path, AudioIngestPolicy::default()).unwrap();

        assert_eq!(audio.source_sample_rate, WHISPER_SAMPLE_RATE);
        assert_eq!(audio.source_channels, 1);
        assert!(!audio.pcm_samples.is_empty());
    }

    #[test]
    fn file_size_limit_is_checked_before_decoder_creation() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/whisper.cpp/samples/jfk.mp3");
        let policy = AudioIngestPolicy {
            max_file_bytes: 1,
            ..AudioIngestPolicy::default()
        };

        assert!(matches!(
            decode_file(&path, policy),
            Err(AudioIngestError::FileTooLarge(_))
        ));
    }

    #[test]
    fn normalized_audio_becomes_a_transcription_request_without_path_data() {
        let audio = NormalizedAudio {
            pcm_samples: vec![1, -1],
            duration_seconds: 0.001,
            source_sample_rate: WHISPER_SAMPLE_RATE,
            source_channels: 1,
        };

        let request = audio.into_transcription_request("ru".into(), "large-v3-turbo".into());

        assert_eq!(request.pcm_samples, vec![1, -1]);
        assert_eq!(request.language, "ru");
        assert_eq!(request.model, "large-v3-turbo");
    }
}
