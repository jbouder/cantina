//! WAV files on disk: what is in them and how to read a region.
//!
//! Everything here runs on ordinary threads. The audio thread only ever
//! sees samples that a disk thread has already pulled through a ring.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

/// What a WAV file holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WavInfo {
    /// The file.
    pub path: PathBuf,
    /// Frames per second.
    pub sample_rate: u32,
    /// Channels per frame.
    pub channels: u16,
    /// Bits per sample as stored.
    pub bits_per_sample: u16,
    /// True for IEEE float samples, false for integers.
    pub float: bool,
    /// Frames per channel.
    pub frames: u64,
}

/// Reads a file's header.
pub fn info(path: &Path) -> Result<WavInfo> {
    let reader = WavReader::open(path).with_context(|| format!("opening {}", path.display()))?;
    let spec = reader.spec();
    Ok(WavInfo {
        path: path.to_path_buf(),
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        bits_per_sample: spec.bits_per_sample,
        float: spec.sample_format == SampleFormat::Float,
        frames: reader.duration() as u64,
    })
}

/// Reads a whole file as channel-major `f32`.
pub fn read_all(path: &Path) -> Result<(WavInfo, Vec<Vec<f32>>)> {
    let mut reader = Reader::open(path)?;
    let info = reader.info().clone();
    let mut interleaved = Vec::with_capacity(info.frames as usize * info.channels as usize);
    let mut frame = 0;
    loop {
        let got = reader.read(frame, 1 << 16, &mut interleaved)?;
        if got == 0 {
            break;
        }
        frame += got as u64;
    }
    let channels = info.channels as usize;
    let mut planar = vec![Vec::with_capacity(info.frames as usize); channels];
    for (i, sample) in interleaved.into_iter().enumerate() {
        planar[i % channels].push(sample);
    }
    Ok((info, planar))
}

/// Writes channel-major `f32` as a 32-bit float file. Every channel must
/// be the same length.
pub fn write(path: &Path, sample_rate: f64, channels: &[Vec<f32>]) -> Result<()> {
    let frames = channels.first().map_or(0, |c| c.len());
    if channels.is_empty() || channels.iter().any(|c| c.len() != frames) {
        bail!("need at least one channel, all the same length");
    }
    let spec = WavSpec {
        channels: channels.len() as u16,
        sample_rate: sample_rate as u32,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer =
        WavWriter::create(path, spec).with_context(|| format!("creating {}", path.display()))?;
    for frame in 0..frames {
        for channel in channels {
            writer.write_sample(channel[frame])?;
        }
    }
    writer
        .finalize()
        .with_context(|| format!("finishing {}", path.display()))
}

/// How samples are stored in a file [`write_as`] makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormatOut {
    /// 16-bit integers, with TPDF dither.
    Int16,
    /// 24-bit integers, with TPDF dither.
    Int24,
    /// 32-bit IEEE float, written as is.
    Float32,
}

impl SampleFormatOut {
    /// The format for a bit depth: 16, 24 or 32 (float).
    pub fn from_bits(bits: u16) -> Result<Self> {
        match bits {
            16 => Ok(Self::Int16),
            24 => Ok(Self::Int24),
            32 => Ok(Self::Float32),
            _ => bail!("{bits}-bit files are not written; choose 16, 24 or 32"),
        }
    }

    /// Bits per sample.
    pub fn bits(self) -> u16 {
        match self {
            Self::Int16 => 16,
            Self::Int24 => 24,
            Self::Float32 => 32,
        }
    }
}

/// Writes channel-major `f32` in `format`. Integer formats are dithered
/// with triangular noise of one least significant bit peak, from a fixed
/// seed so the same input always writes the same file, and clamped to
/// full scale. Every channel must be the same length.
pub fn write_as(
    path: &Path,
    sample_rate: f64,
    channels: &[Vec<f32>],
    format: SampleFormatOut,
) -> Result<()> {
    let frames = channels.first().map_or(0, |c| c.len());
    if channels.is_empty() || channels.iter().any(|c| c.len() != frames) {
        bail!("need at least one channel, all the same length");
    }
    let bits = format.bits();
    let spec = WavSpec {
        channels: channels.len() as u16,
        sample_rate: sample_rate.round() as u32,
        bits_per_sample: bits,
        sample_format: match format {
            SampleFormatOut::Float32 => SampleFormat::Float,
            _ => SampleFormat::Int,
        },
    };
    let mut writer =
        WavWriter::create(path, spec).with_context(|| format!("creating {}", path.display()))?;
    let mut dither = Dither::new();
    let full = (1i64 << (bits - 1)) as f64;
    let (lo, hi) = (-full, full - 1.0);
    for frame in 0..frames {
        for channel in channels {
            let sample = channel[frame];
            match format {
                SampleFormatOut::Float32 => writer.write_sample(sample)?,
                _ => {
                    let scaled = sample as f64 * full + dither.next();
                    writer.write_sample(scaled.round().clamp(lo, hi) as i32)?;
                }
            }
        }
    }
    writer
        .finalize()
        .with_context(|| format!("finishing {}", path.display()))
}

/// Triangular dither: the sum of two uniform values in ±0.5 LSB, from a
/// small fixed-seed generator.
struct Dither(u64);

impl Dither {
    fn new() -> Self {
        Self(0x9E37_79B9_7F4A_7C15)
    }

    fn uniform(&mut self) -> f64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let x = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (x >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    }

    fn next(&mut self) -> f64 {
        self.uniform() + self.uniform()
    }
}

/// A file open for reading regions as `f32`, whatever it stores.
pub struct Reader {
    wav: WavReader<BufReader<File>>,
    info: WavInfo,
    /// Frame the next read starts at unless it seeks.
    next_frame: u64,
    /// Multiplier that brings integer samples to ±1.
    scale: f32,
}

impl Reader {
    /// Opens a WAV file and reads its header.
    pub fn open(path: &Path) -> Result<Reader> {
        let wav = WavReader::open(path).with_context(|| format!("opening {}", path.display()))?;
        let spec = wav.spec();
        if spec.channels == 0 {
            bail!("{} has no channels", path.display());
        }
        let float = spec.sample_format == SampleFormat::Float;
        if float && spec.bits_per_sample != 32 {
            bail!(
                "{} is {}-bit float, only 32-bit float is supported",
                path.display(),
                spec.bits_per_sample
            );
        }
        let info = WavInfo {
            path: path.to_path_buf(),
            sample_rate: spec.sample_rate,
            channels: spec.channels,
            bits_per_sample: spec.bits_per_sample,
            float,
            frames: wav.duration() as u64,
        };
        let scale = if float {
            1.0
        } else {
            1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32
        };
        Ok(Reader {
            wav,
            info,
            next_frame: 0,
            scale,
        })
    }

    /// What the file holds.
    pub fn info(&self) -> &WavInfo {
        &self.info
    }

    /// Reads up to `frames` frames starting at `frame`, interleaved with
    /// the file's channel count, appending to `into`. Returns how many
    /// frames were read, fewer only at the end of the file.
    pub fn read(&mut self, frame: u64, frames: usize, into: &mut Vec<f32>) -> Result<usize> {
        let frame = frame.min(self.info.frames);
        let frames = frames.min((self.info.frames - frame) as usize);
        if frames == 0 {
            return Ok(0);
        }
        if frame != self.next_frame {
            let target = u32::try_from(frame).context("file is too long to seek in")?;
            self.wav.seek(target).with_context(|| {
                format!("seeking {} to frame {frame}", self.info.path.display())
            })?;
            self.next_frame = frame;
        }
        let wanted = frames * self.info.channels as usize;
        let before = into.len();
        if self.info.float {
            for sample in self.wav.samples::<f32>().take(wanted) {
                into.push(sample?);
            }
        } else {
            for sample in self.wav.samples::<i32>().take(wanted) {
                into.push(sample? as f32 * self.scale);
            }
        }
        let got = (into.len() - before) / self.info.channels as usize;
        self.next_frame += got as u64;
        Ok(got)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use hound::{WavSpec, WavWriter};

    /// Writes a file where frame `i` of channel `c` holds `(i + c) / 65536`,
    /// so any region is recognisable.
    pub(crate) fn ramp_file(
        dir: &Path,
        name: &str,
        channels: u16,
        frames: u32,
        bits: u16,
    ) -> PathBuf {
        let path = dir.join(name);
        let float = bits == 32;
        let spec = WavSpec {
            channels,
            sample_rate: 48_000,
            bits_per_sample: bits,
            sample_format: if float {
                SampleFormat::Float
            } else {
                SampleFormat::Int
            },
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        for i in 0..frames {
            for c in 0..channels {
                let value = ramp(i as u64, c);
                if float {
                    writer.write_sample(value).unwrap();
                } else {
                    let full = (1i64 << (bits - 1)) as f32;
                    writer.write_sample((value * full) as i32).unwrap();
                }
            }
        }
        writer.finalize().unwrap();
        path
    }

    pub(crate) fn ramp(frame: u64, channel: u16) -> f32 {
        (frame + channel as u64) as f32 / 65_536.0
    }

    #[test]
    fn reads_regions_in_every_format() {
        let dir = tempdir();
        for (bits, channels) in [(32u16, 1u16), (16, 2), (24, 1), (32, 2)] {
            let path = ramp_file(
                &dir,
                &format!("ramp{bits}x{channels}.wav"),
                channels,
                1000,
                bits,
            );
            let info = info(&path).unwrap();
            assert_eq!(info.frames, 1000);
            assert_eq!(info.channels, channels);
            assert_eq!(info.float, bits == 32);
            let mut reader = Reader::open(&path).unwrap();
            let mut out = Vec::new();
            assert_eq!(reader.read(10, 5, &mut out).unwrap(), 5);
            assert_eq!(out.len(), 5 * channels as usize);
            for (i, sample) in out.iter().enumerate() {
                let (frame, c) = (10 + i / channels as usize, (i % channels as usize) as u16);
                assert!(
                    (sample - ramp(frame as u64, c)).abs() < 1e-4,
                    "{bits}-bit frame {frame}"
                );
            }
            // Sequential reads do not seek; a jump does; the end truncates.
            out.clear();
            assert_eq!(reader.read(15, 5, &mut out).unwrap(), 5);
            assert!((out[0] - ramp(15, 0)).abs() < 1e-4);
            out.clear();
            assert_eq!(reader.read(998, 10, &mut out).unwrap(), 2);
            assert_eq!(reader.read(1000, 10, &mut out).unwrap(), 0);
            out.clear();
            assert_eq!(reader.read(3, 1, &mut out).unwrap(), 1);
            assert!((out[0] - ramp(3, 0)).abs() < 1e-4);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn whole_files_round_trip() {
        let dir = tempdir();
        let path = dir.join("planar.wav");
        let channels = vec![
            (0..1000).map(|i| ramp(i, 0)).collect::<Vec<f32>>(),
            (0..1000).map(|i| ramp(i, 1)).collect::<Vec<f32>>(),
        ];
        write(&path, 48_000.0, &channels).unwrap();
        let (info, back) = read_all(&path).unwrap();
        assert_eq!(info.channels, 2);
        assert_eq!(info.frames, 1000);
        assert_eq!(back, channels);
        assert!(write(&path, 48_000.0, &[]).is_err());
        assert!(write(&path, 48_000.0, &[vec![0.0; 2], vec![0.0; 3]]).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn integer_files_are_dithered_clamped_and_reproducible() {
        let dir = tempdir();
        let tone: Vec<f32> = (0..4800)
            .map(|i| 0.5 * (i as f32 * 0.05).sin())
            .chain([1.5, -1.5])
            .collect();
        for format in [SampleFormatOut::Int16, SampleFormatOut::Int24] {
            let a = dir.join(format!("a{}.wav", format.bits()));
            let b = dir.join(format!("b{}.wav", format.bits()));
            write_as(&a, 44_100.0, std::slice::from_ref(&tone), format).unwrap();
            write_as(&b, 44_100.0, std::slice::from_ref(&tone), format).unwrap();
            assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
            let (info, back) = read_all(&a).unwrap();
            assert_eq!(info.sample_rate, 44_100);
            assert_eq!(info.bits_per_sample, format.bits());
            assert!(!info.float);
            let lsb = 1.0 / (1u64 << (format.bits() - 1)) as f32;
            for (x, y) in tone[..4800].iter().zip(&back[0]) {
                // Rounding plus up to one LSB of dither either way.
                assert!((x - y).abs() <= 1.5 * lsb + 1e-7, "{x} vs {y}");
            }
            assert!(back[0][4800] <= 1.0 && back[0][4800] > 1.0 - 2.0 * lsb);
            assert_eq!(back[0][4801], -1.0);
        }
        let f = dir.join("f.wav");
        write_as(
            &f,
            48_000.0,
            std::slice::from_ref(&tone),
            SampleFormatOut::Float32,
        )
        .unwrap();
        assert_eq!(read_all(&f).unwrap().1[0], tone);
        assert!(SampleFormatOut::from_bits(8).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A fresh directory per call. Tests run in parallel and the clock
    /// is coarser than two calls, so a counter keeps them apart.
    pub(crate) fn tempdir() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "cantina-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
