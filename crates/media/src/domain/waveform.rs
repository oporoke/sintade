//! Waveform peaks for the editor's timeline (ADR-0039). No I/O here: the audio is decoded by
//! FFmpeg to 8 kHz mono 16-bit PCM and the samples are folded into peaks here.

use std::ffi::OsString;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The most peaks a waveform has, whatever the recording's length: what keeps the JSON small
/// (≤ ~80 KB) and drawing it fast.
pub const MAX_PEAKS: u32 = 20_000;

/// The finest a peak gets: 20 per second.
pub const MIN_BUCKET_MS: u32 = 50;

/// What the audio is decoded to before folding. Peaks need no more.
pub const SAMPLE_RATE: u32 = 8_000;

/// The `version` field of the JSON, for readers to check.
pub const FORMAT_VERSION: u32 = 1;

/// How long each peak covers for a recording of `duration_ms`: 50 ms, or longer when that would
/// make more than [`MAX_PEAKS`].
pub fn bucket_ms(duration_ms: u32) -> u32 {
    duration_ms.div_ceil(MAX_PEAKS).max(MIN_BUCKET_MS)
}

/// How many peaks a recording of `duration_ms` has: one per bucket, the last one maybe short.
pub fn peak_count(duration_ms: u32, bucket_ms: u32) -> u32 {
    duration_ms.div_ceil(bucket_ms.max(1))
}

/// The peaks JSON stored as `img/waveform.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Waveform {
    pub version: u32,
    /// The length of the recording the peaks were taken from.
    pub duration_ms: u32,
    /// How long each peak covers; peak `i` is `[i * bucket_ms, (i + 1) * bucket_ms)`.
    pub bucket_ms: u32,
    /// `false` for a recording without sound: `peaks` is then empty.
    pub has_audio: bool,
    /// The loudest sample of each bucket, 0 (silence) to 255 (full scale).
    pub peaks: Vec<u8>,
}

impl Waveform {
    /// The waveform of a recording with no audio track.
    pub fn silent(duration_ms: u32) -> Self {
        Self {
            version: FORMAT_VERSION,
            duration_ms,
            bucket_ms: bucket_ms(duration_ms),
            has_audio: false,
            peaks: Vec::new(),
        }
    }
}

/// FFmpeg arguments that decode `mp4`'s audio to raw 8 kHz mono signed 16-bit little-endian PCM
/// in `raw`.
pub fn decode_args(mp4: &Path, raw: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-y", "-i"]
        .into_iter()
        .map(OsString::from)
        .collect();
    args.push(mp4.as_os_str().to_owned());
    let rate = SAMPLE_RATE.to_string();
    args.extend(
        [
            "-vn",
            "-sn",
            "-dn",
            "-ac",
            "1",
            "-ar",
            &rate,
            "-c:a",
            "pcm_s16le",
            "-f",
            "s16le",
            "-progress",
            "pipe:1",
            "-nostats",
        ]
        .into_iter()
        .map(OsString::from),
    );
    args.push(raw.as_os_str().to_owned());
    args
}

/// Folds 16-bit little-endian PCM, fed in pieces of any size, into one peak per bucket.
#[derive(Debug, Clone)]
pub struct PeakFolder {
    bucket_samples: u64,
    in_bucket: u64,
    loudest: u32,
    /// The first byte of a sample split across two pieces.
    carry: Option<u8>,
    peaks: Vec<u8>,
}

impl PeakFolder {
    pub fn new(bucket_ms: u32) -> Self {
        Self {
            bucket_samples: (u64::from(SAMPLE_RATE) * u64::from(bucket_ms) / 1000).max(1),
            in_bucket: 0,
            loudest: 0,
            carry: None,
            peaks: Vec::new(),
        }
    }

    pub fn push_bytes(&mut self, mut bytes: &[u8]) {
        if let Some(low) = self.carry.take() {
            let Some((&high, rest)) = bytes.split_first() else {
                self.carry = Some(low);
                return;
            };
            self.push_sample(i16::from_le_bytes([low, high]));
            bytes = rest;
        }
        let (samples, rest) = bytes.as_chunks::<2>();
        for sample in samples {
            self.push_sample(i16::from_le_bytes(*sample));
        }
        self.carry = rest.first().copied();
    }

    fn push_sample(&mut self, sample: i16) {
        self.loudest = self.loudest.max(i32::from(sample).unsigned_abs());
        self.in_bucket += 1;
        if self.in_bucket == self.bucket_samples {
            self.close_bucket();
        }
    }

    fn close_bucket(&mut self) {
        // 32768 (i16::MIN) is full scale: 255.
        self.peaks
            .push(u8::try_from(self.loudest * 255 / 32_768).unwrap_or(u8::MAX));
        self.in_bucket = 0;
        self.loudest = 0;
    }

    /// The peaks, exactly [`peak_count`] of them for `duration_ms`: a short last bucket is kept,
    /// audio that ends early is padded with silence and audio past the end is dropped, so the
    /// peaks line up with the recording's length.
    pub fn finish(mut self, duration_ms: u32, bucket_ms: u32) -> Vec<u8> {
        if self.in_bucket > 0 {
            self.close_bucket();
        }
        let wanted = peak_count(duration_ms, bucket_ms) as usize;
        self.peaks.resize(wanted, 0);
        self.peaks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    #[test]
    fn buckets_are_50_ms_until_the_peaks_would_be_too_many() {
        assert_eq!(bucket_ms(10_000), 50);
        assert_eq!(peak_count(10_000, 50), 200);
        assert_eq!(bucket_ms(1_000_000), 50);
        assert_eq!(bucket_ms(3_600_000), 180);
        for minutes in [1, 17, 60, 61, 240, 600] {
            let duration = minutes * 60_000;
            assert!(
                peak_count(duration, bucket_ms(duration)) <= MAX_PEAKS,
                "{minutes} min"
            );
        }
        assert_eq!(peak_count(0, bucket_ms(0)), 0);
    }

    #[test]
    fn each_bucket_keeps_its_loudest_sample() {
        // 50 ms at 8 kHz is 400 samples a bucket.
        let mut folder = PeakFolder::new(50);
        let mut samples = vec![100_i16; 400];
        samples[7] = 16_384;
        samples.extend(vec![-32_768_i16; 400]);
        samples.extend(vec![0_i16; 400]);
        folder.push_bytes(&pcm(&samples));
        assert_eq!(folder.finish(150, 50), vec![127, 255, 0]);
    }

    #[test]
    fn pieces_of_any_size_fold_the_same() {
        let samples: Vec<i16> = (0..1_000)
            .map(|i| ((i * 37) % 20_000 - 10_000) as i16)
            .collect();
        let bytes = pcm(&samples);
        let mut whole = PeakFolder::new(50);
        whole.push_bytes(&bytes);
        let mut split = PeakFolder::new(50);
        for piece in bytes.chunks(3) {
            split.push_bytes(piece);
        }
        split.push_bytes(&[]);
        assert_eq!(split.finish(125, 50), whole.finish(125, 50));
    }

    #[test]
    fn the_peaks_match_the_recording_length() {
        // A short last bucket counts; audio that stops early is padded; audio past the end goes.
        let mut short = PeakFolder::new(50);
        short.push_bytes(&pcm(&[1_000; 500]));
        assert_eq!(short.finish(120, 50), vec![7, 7, 0]);

        let mut long = PeakFolder::new(50);
        long.push_bytes(&pcm(&[1_000; 4_000]));
        assert_eq!(long.finish(100, 50).len(), 2);
    }

    #[test]
    fn the_json_has_the_documented_fields() {
        let json = serde_json::to_value(Waveform::silent(10_000)).expect("json");
        assert_eq!(
            json,
            serde_json::json!({
                "version": 1, "duration_ms": 10000, "bucket_ms": 50,
                "has_audio": false, "peaks": []
            })
        );
    }

    #[test]
    fn decoding_writes_mono_8_khz_pcm() {
        let args: Vec<String> = decode_args(Path::new("in.mp4"), Path::new("out.raw"))
            .into_iter()
            .map(|arg| arg.into_string().expect("utf-8"))
            .collect();
        let joined = args.join(" ");
        assert!(joined.contains("-i in.mp4 -vn"), "{joined}");
        assert!(
            joined.contains("-ac 1 -ar 8000 -c:a pcm_s16le -f s16le"),
            "{joined}"
        );
        assert_eq!(args.last().map(String::as_str), Some("out.raw"));
    }
}
