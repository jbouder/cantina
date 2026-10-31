//! WAV files on disk: what is in them and how to read a region.
//!
//! Everything here runs on ordinary threads. The audio thread only ever
//! sees samples that a disk thread has already pulled through a ring.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use hound::{SampleFormat, WavReader};

/// What a WAV file holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WavInfo {
    pub path: PathBuf,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
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

    pub(crate) fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cantina-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
