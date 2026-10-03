use cpal::SampleFormat;
pub(super) fn sample_format_priority(format: SampleFormat) -> u8 {
    match format {
        SampleFormat::F32 => 0,
        SampleFormat::I16 => 1,
        SampleFormat::I32 => 2,
        SampleFormat::F64 => 3,
        SampleFormat::I64 => 4,
        SampleFormat::U16 => 5,
        SampleFormat::U32 => 6,
        SampleFormat::U64 => 7,
        SampleFormat::I8 => 8,
        SampleFormat::U8 => 9,
        _ => 10,
    }
}

pub(super) fn convert_to_i16(data: &cpal::Data, format: SampleFormat, output: &mut Vec<i16>) {
    output.clear();
    let bytes = data.bytes();
    match format {
        SampleFormat::I8 => samples_to_i16::<i8, _>(bytes, output, |s| (s as i16) << 8),
        SampleFormat::I16 => samples_to_i16::<i16, _>(bytes, output, |s| s),
        SampleFormat::I32 => samples_to_i16::<i32, _>(bytes, output, |s| (s >> 16) as i16),
        SampleFormat::I64 => samples_to_i16::<i64, _>(bytes, output, |s| (s >> 48) as i16),
        SampleFormat::U8 => samples_to_i16::<u8, _>(bytes, output, |s| (s as i16 - 128) << 8),
        SampleFormat::U16 => samples_to_i16::<u16, _>(bytes, output, |s| (s as i32 - 32768) as i16),
        SampleFormat::U32 => {
            samples_to_i16::<u32, _>(bytes, output, |s| ((s as i64 - 2_147_483_648) >> 16) as i16)
        }
        SampleFormat::U64 => samples_to_i16::<u64, _>(bytes, output, |s| {
            ((s as i128 - 9_223_372_036_854_775_808i128) >> 48) as i16
        }),
        SampleFormat::F32 => samples_to_i16::<f32, _>(bytes, output, |s| float_to_i16(s as f64)),
        SampleFormat::F64 => samples_to_i16::<f64, _>(bytes, output, float_to_i16),
        _ => {
            tracing::warn!("fono-wake: unsupported sample format {format:?}");
        }
    }
}

fn samples_to_i16<T: Copy, F>(bytes: &[u8], output: &mut Vec<i16>, mut convert: F)
where
    F: FnMut(T) -> i16,
{
    if bytes.is_empty() {
        return;
    }
    let sample_size = std::mem::size_of::<T>();
    if bytes.len() % sample_size != 0 {
        tracing::warn!("fono-wake: misaligned audio bytes");
    }
    // SAFETY: `align_to` never reinterprets the unaligned prefix/suffix. The
    // aligned middle is read only as `T: Copy`; the caller selects `T` from
    // CPAL's declared sample format, so no references outlive `bytes`.
    let (_, samples, _) = unsafe { bytes.align_to::<T>() };
    output.extend(samples.iter().copied().map(&mut convert));
}

fn float_to_i16(v: f64) -> i16 {
    (v.clamp(-1.0, 1.0) * i16::MAX as f64) as i16
}

pub(super) fn mix_to_mono(data: &[i16], channels: usize, output: &mut Vec<i16>) {
    output.clear();
    output.extend(data.chunks(channels).map(|chunk| {
        let sum: i64 = chunk.iter().map(|&s| s as i64).sum();
        (sum / chunk.len() as i64) as i16
    }));
}
