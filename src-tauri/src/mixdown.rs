//! Mixdown export and bounce in place: the project's playback rendered
//! offline, faster than real time.
//!
//! [`build`] makes the graph playback runs, from the project alone, on a
//! runtime with no stream behind it:
//!
//! ```text
//! player or sequencer -> delay -> bus -> pedals... -> strip -> master -> output
//! ```
//!
//! Every node is the one the live graph uses, made the same way, with
//! the same latency compensation and the same mute and solo logic. What
//! live playback has besides (the input, the monitor gate, the recorder,
//! meters and the scope) carries nothing from the timeline to the
//! output, so it is left out. A bounce builds one track and stops before
//! its strip, so the track's gain, pan and mute still apply to the
//! bounced clip as they did to the chain.
//!
//! The graph is built under the engine's lock, then a [`Render`] runs it
//! on any thread through the engine's [`Offline`] driver, which waits for
//! the players' disk threads between blocks rather than ever playing
//! silence for them.

use std::any::Any;
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Result};
use cantina_dsp::{LoadedKit, Processor};
use cantina_engine::{
    plan, Delay, Endpoint, GraphConfig, Offline, Player, Runtime, Sequencer, Strip,
};
use cantina_project::{Pedal, Project, Track, TrackId};

use crate::engine::{engine_clips, score_for, tempo_map};

/// Frames per block: the app's default buffer, so block-rate behaviour
/// matches what most playback hears.
pub const BLOCK: usize = 128;
/// Channels the graph runs, as the live one does.
const CHANNELS: usize = 2;
/// The longest a tail is rendered for after the last clip ends.
const MAX_TAIL_SECONDS: f64 = 10.0;
/// A tail ends once this much in a row is below [`SILENCE`].
const QUIET_SECONDS: f64 = 0.1;
/// -100 dBFS.
const SILENCE: f32 = 1e-5;

/// What to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The master: every track through its strip, summed.
    Mix,
    /// One track, before its strip: what a bounce writes.
    Track(TrackId),
}

/// What [`build`] needs from the engine besides the project.
pub struct Parts<'a> {
    /// Where the project's audio files are.
    pub audio_dir: &'a Path,
    /// Builds a pedal's processor with its saved state and values set.
    pub pedal: &'a mut dyn FnMut(&Pedal) -> Result<Box<dyn Processor>>,
    /// The kit a drum track plays.
    pub kit: &'a mut dyn FnMut(&str) -> Arc<LoadedKit>,
}

/// A built graph, ready to render a range of the timeline.
pub struct Render {
    offline: Offline,
    /// Frames the output lags the timeline by: the compensated latency of
    /// the chains, cut from the front of every render.
    latency: usize,
    sample_rate: f64,
    /// The UI-thread halves of the players and sequencers, kept for as
    /// long as their nodes run.
    _handles: Vec<Box<dyn Any + Send>>,
}

/// A range of the timeline, in samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    /// The first sample.
    pub start: u64,
    /// Just past the last sample.
    pub end: u64,
}

/// Where the last thing on the timeline ends, in samples: an audio clip,
/// or a pattern clip at the project's tempo. Zero for an empty project.
pub fn song_end(project: &Project) -> u64 {
    project
        .tracks
        .iter()
        .filter_map(|t| track_range(project, t))
        .map(|r| r.end)
        .max()
        .unwrap_or(0)
}

/// From the start of a track's first clip to the end of its last, in
/// samples, or none for a track with nothing on it.
pub fn track_range(project: &Project, track: &Track) -> Option<Range> {
    let map = tempo_map(&project.tempo);
    let rate = project.sample_rate;
    let audio = track.clips.iter().map(|c| (c.start, c.end()));
    let patterns = track.pattern_clips.iter().map(|c| {
        (
            map.ticks_to_samples(c.start as f64, rate),
            map.ticks_to_samples(c.end() as f64, rate),
        )
    });
    // A drum track plays its patterns; an audio track its clips.
    let spans: Vec<(u64, u64)> = if track.kit.is_some() {
        patterns.collect()
    } else {
        audio.collect()
    };
    let start = spans.iter().map(|s| s.0).min()?;
    let end = spans.iter().map(|s| s.1).max()?;
    Some(Range { start, end })
}

/// Builds the graph for `scope` from `project`.
pub fn build(project: &Project, scope: Scope, parts: Parts<'_>) -> Result<Render> {
    let Parts {
        audio_dir,
        pedal: make_pedal,
        kit: make_kit,
    } = parts;
    let sample_rate = project.sample_rate;
    let (runtime, mut controller) = Runtime::new(GraphConfig {
        sample_rate,
        max_block_frames: BLOCK,
        channels: CHANNELS,
    })?;
    controller
        .transport_mut()
        .set_tempo_map(tempo_map(&project.tempo));

    let tracks: Vec<&Track> = match scope {
        Scope::Mix => project.tracks.iter().collect(),
        Scope::Track(id) => match project.track(id) {
            Some(t) => vec![t],
            None => bail!("no track {id}"),
        },
    };

    let mut handles: Vec<Box<dyn Any + Send>> = Vec::new();
    let mut players = Vec::new();
    // Per track: the node its source feeds, the end of its chain, and
    // the chain's latency.
    let mut built = Vec::with_capacity(tracks.len());
    for track in &tracks {
        let graph = controller.graph_mut();
        let source = match &track.kit {
            Some(kit) => {
                let loaded = make_kit(kit);
                let (sequencer, mut handle) = Sequencer::new(Arc::clone(&loaded));
                if !handle.set_score(score_for(track, project, &loaded, None, None)) {
                    bail!("{}: the sequencer would not take its patterns", track.name);
                }
                handles.push(Box::new(handle));
                graph.add(Box::new(sequencer))
            }
            None => {
                let (player, mut handle) = Player::new(sample_rate, CHANNELS);
                handle.set_clips(engine_clips(track, audio_dir))?;
                players.push(Arc::clone(handle.stats()));
                handles.push(Box::new(handle));
                graph.add(Box::new(player))
            }
        };
        let bus = graph.add_processor(cantina_dsp::stock("gain").expect("gain is stock"));
        let mut chain = Vec::new();
        let mut latency = 0;
        for pedal in track.chain.iter().filter(|p| !p.bypassed) {
            let node = graph.add_processor(make_pedal(pedal)?);
            latency += graph.node(node).map_or(0, |n| n.latency);
            chain.push(node);
        }
        built.push((source, bus, chain, latency));
    }

    let latencies: Vec<usize> = built.iter().map(|b| b.3).collect();
    let (max, delays) = plan(&latencies);
    let any_solo = project.tracks.iter().any(|t| t.strip.solo);
    let master = match scope {
        Scope::Mix => {
            let m = &project.master;
            Some(
                controller
                    .graph_mut()
                    .add(Box::new(Strip::new(m.gain_db, 0.0, m.mute))),
            )
        }
        Scope::Track(_) => None,
    };
    for ((track, (source, bus, chain, _)), delay) in tracks.iter().zip(&built).zip(delays) {
        let graph = controller.graph_mut();
        let mut previous = *source;
        if master.is_some() {
            let node = graph.add(Box::new(Delay::new(max.max(1))));
            graph.connect(previous, node)?;
            controller.set_param(node, 0, delay as f32);
            previous = node;
        }
        let graph = controller.graph_mut();
        graph.connect(previous, *bus)?;
        previous = *bus;
        for node in chain {
            graph.connect(previous, *node)?;
            previous = *node;
        }
        match master {
            Some(master) => {
                let s = &track.strip;
                let muted = s.mute || (any_solo && !s.solo);
                let strip = graph.add(Box::new(Strip::new(s.gain_db, s.pan, muted)));
                graph.connect(previous, strip)?;
                graph.connect(strip, master)?;
            }
            None => graph.connect(previous, Endpoint::Output)?,
        }
    }
    if let Some(master) = master {
        controller.graph_mut().connect(master, Endpoint::Output)?;
    }
    controller.commit()?;

    let offline = Offline::new(runtime, controller, players, BLOCK);
    Ok(Render {
        offline,
        latency: match scope {
            Scope::Mix => max,
            Scope::Track(_) => latencies.first().copied().unwrap_or(0),
        },
        sample_rate,
        _handles: handles,
    })
}

impl Render {
    /// Runs at a live stream's pace instead of as fast as it can: what
    /// playback would produce. For tests.
    #[cfg(test)]
    pub fn set_paced(&mut self, paced: bool) {
        self.offline.set_paced(paced);
    }

    /// The rate it renders at: the project's.
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// Renders `range` of the timeline, lined up with it, as playback
    /// from the top of the song sounds there: the render always rolls
    /// from the start, so a delay or reverb from before the range rings
    /// into it and a pedal that moves on its own is where playback would
    /// have it, and what comes before the range is dropped. The chains'
    /// latency is rendered past the end and cut from the front. With
    /// `tail`, rendering goes on past the end until the output has been
    /// silent for a moment, or for at most ten seconds, so reverbs and
    /// delays ring out. `progress` hears the fraction done now and then
    /// and returns false to cancel.
    pub fn run(
        &mut self,
        range: Range,
        tail: bool,
        mut progress: impl FnMut(f32) -> bool,
    ) -> Result<Vec<Vec<f32>>> {
        // Output sample `i` is timeline sample `i - latency`.
        let skip = range.start as usize + self.latency;
        let wanted = range.end as usize + self.latency;
        let max_tail = if tail {
            (MAX_TAIL_SECONDS * self.sample_rate) as usize
        } else {
            0
        };
        let quiet_needed = (QUIET_SECONDS * self.sample_rate) as usize;
        let mut out: Vec<Vec<f32>> = (0..CHANNELS)
            .map(|_| Vec::with_capacity(wanted - skip))
            .collect();
        self.offline.start(0);
        let mut done = 0;
        let mut quiet = 0;
        let mut blocks = 0u32;
        loop {
            let in_range = done < wanted;
            if !in_range && (done >= wanted + max_tail || quiet >= quiet_needed) {
                break;
            }
            let n = if in_range {
                (wanted - done).min(BLOCK)
            } else {
                BLOCK
            };
            let block = self.offline.next(n)?;
            let loud = block.iter().any(|c| c.iter().any(|s| s.abs() > SILENCE));
            // The part of this block at or after `skip`.
            let keep = skip.saturating_sub(done).min(n);
            for (channel, samples) in out.iter_mut().zip(block) {
                channel.extend_from_slice(&samples[keep..]);
            }
            done += n;
            if !in_range || done >= wanted {
                quiet = if loud { 0 } else { quiet + n };
            }
            blocks += 1;
            if blocks % 64 == 0 && !progress((done as f32 / wanted.max(1) as f32).min(1.0)) {
                bail!("cancelled");
            }
        }
        progress(1.0);
        Ok(out)
    }
}

/// The render's channels at `to` Hz, from `from`.
pub fn convert_rate(channels: Vec<Vec<f32>>, from: f64, to: f64) -> Vec<Vec<f32>> {
    if from == to {
        return channels;
    }
    channels
        .iter()
        .map(|c| cantina_dsp::resample(c, from, to))
        .collect()
}

/// Peak and RMS of a render, in dBFS, for the notice after an export.
pub fn levels(channels: &[Vec<f32>]) -> (f32, f32) {
    let mut peak = 0.0f32;
    let mut sum = 0.0f64;
    let mut n = 0usize;
    for c in channels {
        for s in c {
            peak = peak.max(s.abs());
            sum += (*s as f64) * (*s as f64);
        }
        n += c.len();
    }
    let db = |x: f64| {
        if x > 0.0 {
            20.0 * x.log10()
        } else {
            -f64::INFINITY
        }
    };
    (
        db(peak as f64) as f32,
        db((sum / n.max(1) as f64).sqrt()) as f32,
    )
}

#[cfg(test)]
mod tests {
    //! The golden-file test for mixdown, and the claims the export and
    //! the bounce rest on: an offline render is what playback produces,
    //! and a bounced track plays back as its chain did.
    //!
    //! To accept a change to the mix on purpose, regenerate the
    //! reference and commit it:
    //!
    //! ```text
    //! UPDATE_GOLDEN=1 cargo test -p cantina mixdown
    //! ```

    use super::*;
    use cantina_engine::wav;
    use cantina_project::{
        Clip, ClipId, Command, Pattern, PatternClip, PatternClipId, PatternId, PatternRow, PedalId,
    };
    use std::path::PathBuf;
    use std::time::Instant;

    const SR: f64 = 48_000.0;
    /// Largest difference a render may show against its reference: float
    /// summation order can move the last bits, a real change is far more.
    const TOLERANCE: f32 = 1e-5;

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    fn tempdir(name: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "cantina-mixdown-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A plucked note: a decaying pair of partials, 0.6 s, mono.
    fn pluck() -> Vec<f32> {
        (0..(SR * 0.6) as usize)
            .map(|i| {
                let t = i as f64 / SR;
                let env = (-t * 6.0).exp();
                (env * (0.4 * (std::f64::consts::TAU * 196.0 * t).sin()
                    + 0.2 * (std::f64::consts::TAU * 392.0 * t).sin())) as f32
            })
            .collect()
    }

    /// A guitar clip through a drive (which oversamples, so it has
    /// latency to compensate) and a reverb (which rings past the end),
    /// panned left; a drum track with a half-bar beat panned right.
    fn fixture(dir: &Path) -> Project {
        let audio = cantina_project::audio_dir(dir);
        std::fs::create_dir_all(&audio).unwrap();
        wav::write(&audio.join("pluck.wav"), SR, &[pluck()]).unwrap();
        let mut project = Project::new("Mixdown", SR);
        let mut guitar = Track::new(TrackId(1), "Guitar");
        guitar.clips.push(Clip {
            id: ClipId(2),
            name: "pluck".into(),
            file: "pluck.wav".into(),
            start: 4_800,
            offset: 0,
            length: (SR * 0.6) as u64,
            tab: None,
        });
        guitar.chain.push(Pedal {
            id: PedalId(3),
            spec: "overdrive".into(),
            values: Vec::new(),
            bypassed: false,
        });
        guitar.chain.push(Pedal {
            id: PedalId(4),
            spec: "reverb".into(),
            values: Vec::new(),
            bypassed: false,
        });
        guitar.strip.gain_db = -3.0;
        guitar.strip.pan = -0.4;
        project.tracks.push(guitar);

        let mut beat = Pattern::new(PatternId(5), "Beat");
        beat.length = 8;
        let mut kick = vec![0.0; 8];
        kick[0] = 1.0;
        let mut snare = vec![0.0; 8];
        snare[4] = 0.8;
        beat.rows = vec![
            PatternRow {
                pad: "kick".into(),
                steps: kick,
                ..Default::default()
            },
            PatternRow {
                pad: "snare".into(),
                steps: snare,
                ..Default::default()
            },
        ];
        let ticks = beat.ticks();
        project.patterns.push(beat);
        let mut drums = Track::new(TrackId(6), "Drums");
        drums.kit = Some("rock".into());
        drums.pattern_clips.push(PatternClip {
            id: PatternClipId(7),
            pattern: PatternId(5),
            start: 0,
            offset: 0,
            length: ticks,
            fill: None,
        });
        drums.strip.pan = 0.3;
        project.tracks.push(drums);
        project.master.gain_db = -1.0;
        project.validate().unwrap();
        project
    }

    fn render_of(project: &Project, dir: &Path, scope: Scope) -> Render {
        let audio = cantina_project::audio_dir(dir);
        let mut pedal = |p: &Pedal| -> Result<Box<dyn Processor>> {
            let mut processor = cantina_engine::processors::create(&p.spec, CHANNELS)?;
            for (i, v) in p.values.iter().enumerate() {
                processor.set_param(i, *v);
            }
            Ok(processor)
        };
        let kit_dir = repo().join("kits");
        let mut kit = |id: &str| Arc::new(cantina_engine::kit::load(&kit_dir.join(id)).unwrap().1);
        build(
            project,
            scope,
            Parts {
                audio_dir: &audio,
                pedal: &mut pedal,
                kit: &mut kit,
            },
        )
        .unwrap()
    }

    fn whole(project: &Project) -> Range {
        Range {
            start: 0,
            end: song_end(project),
        }
    }

    fn worst(a: &[Vec<f32>], b: &[Vec<f32>]) -> (f32, usize) {
        assert_eq!(a.len(), b.len(), "channel counts differ");
        let mut worst = (0.0f32, 0);
        for (x, y) in a.iter().zip(b) {
            assert_eq!(x.len(), y.len(), "lengths differ");
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                if (p - q).abs() > worst.0 {
                    worst = ((p - q).abs(), i);
                }
            }
        }
        worst
    }

    fn rms(channels: &[Vec<f32>]) -> f32 {
        levels(channels).1
    }

    #[test]
    fn ranges_cover_audio_and_patterns() {
        let dir = tempdir("ranges");
        let project = fixture(&dir);
        assert_eq!(
            track_range(&project, &project.tracks[0]),
            Some(Range {
                start: 4_800,
                end: 4_800 + 28_800
            })
        );
        // Eight sixteenths at 120 bpm is a second.
        assert_eq!(
            track_range(&project, &project.tracks[1]),
            Some(Range {
                start: 0,
                end: 48_000
            })
        );
        assert_eq!(song_end(&project), 48_000);
        assert_eq!(song_end(&Project::new("Empty", SR)), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_offline_render_is_what_playback_produces() {
        let dir = tempdir("paced");
        let project = fixture(&dir);
        let range = whole(&project);

        let began = Instant::now();
        let offline = render_of(&project, &dir, Scope::Mix)
            .run(range, false, |_| true)
            .unwrap();
        let fast = began.elapsed();

        let mut paced = render_of(&project, &dir, Scope::Mix);
        paced.set_paced(true);
        let began = Instant::now();
        let playback = paced.run(range, false, |_| true).unwrap();
        let real_time = began.elapsed();
        // Playback kept up with its disk, so it is the reference.
        for stats in paced.offline.players() {
            assert_eq!(
                stats
                    .underrun_frames
                    .load(std::sync::atomic::Ordering::Relaxed),
                0
            );
        }

        assert_eq!(offline[0].len(), (range.end - range.start) as usize);
        assert!(rms(&offline) > -40.0, "the mix is silent");
        let (diff, at) = worst(&offline, &playback);
        assert!(
            diff <= TOLERANCE,
            "offline differs from playback by {diff} at {at}"
        );
        assert!(
            fast < real_time,
            "offline took {fast:?}, playback {real_time:?}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_export_matches_its_golden_file() {
        let dir = tempdir("golden");
        let project = fixture(&dir);
        // The reverb rings past the last clip, and the tail stops once it
        // has died away rather than running the full ten seconds.
        let tailed = render_of(&project, &dir, Scope::Mix)
            .run(whole(&project), true, |_| true)
            .unwrap();
        let frames = tailed[0].len();
        assert!(frames > 48_000 + 4_800, "no tail: {frames}");
        assert!(frames < 48_000 + (SR * MAX_TAIL_SECONDS) as usize);
        let quiet = (QUIET_SECONDS * SR) as usize;
        assert!(tailed
            .iter()
            .all(|c| c[frames - quiet..].iter().all(|s| s.abs() <= SILENCE)));

        // The range itself is pinned, as the export writes it.
        let mix = render_of(&project, &dir, Scope::Mix)
            .run(whole(&project), false, |_| true)
            .unwrap();
        assert_eq!(&tailed[0][..48_000], &mix[0][..]);
        let exported = dir.join("mix.wav");
        let at_44 = convert_rate(mix, SR, 44_100.0);
        wav::write_as(&exported, 44_100.0, &at_44, wav::SampleFormatOut::Int24).unwrap();
        let (info, back) = wav::read_all(&exported).unwrap();
        assert_eq!(
            (info.sample_rate, info.bits_per_sample, info.channels),
            (44_100, 24, 2)
        );

        let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/mixdown.wav");
        if std::env::var_os("UPDATE_GOLDEN").is_some_and(|v| !v.is_empty() && v != "0") {
            std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
            std::fs::copy(&exported, &golden).unwrap();
        }
        let (_, reference) = wav::read_all(&golden)
            .expect("no golden mixdown; run UPDATE_GOLDEN=1 cargo test -p cantina mixdown");
        let (diff, at) = worst(&back, &reference);
        assert!(diff <= TOLERANCE, "the mix moved by {diff} at frame {at}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_bounced_track_plays_back_as_its_chain_did() {
        let dir = tempdir("bounce");
        let project = fixture(&dir);
        let before = render_of(&project, &dir, Scope::Mix)
            .run(whole(&project), false, |_| true)
            .unwrap();

        // Bounce both tracks, the drums becoming an audio track.
        let mut bounced = project.clone();
        for (n, id) in [TrackId(1), TrackId(6)].into_iter().enumerate() {
            let track = bounced.track(id).unwrap().clone();
            let range = track_range(&bounced, &track).unwrap();
            let audio = render_of(&bounced, &dir, Scope::Track(id))
                .run(range, true, |_| true)
                .unwrap();
            let file = format!("bounce-{n}.wav");
            wav::write(&cantina_project::audio_dir(&dir).join(&file), SR, &audio).unwrap();
            let clip = Clip {
                id: ClipId(bounced.fresh_id()),
                name: file.clone(),
                file,
                start: range.start,
                offset: 0,
                length: audio[0].len() as u64,
                tab: None,
            };
            Command::BounceTrack {
                track: Track {
                    kit: None,
                    chain: Vec::new(),
                    clips: vec![clip],
                    pattern_clips: Vec::new(),
                    ..track
                },
            }
            .apply(&mut bounced)
            .unwrap();
        }
        bounced.validate().unwrap();
        assert!(bounced
            .tracks
            .iter()
            .all(|t| t.chain.is_empty() && t.kit.is_none()));

        let after = render_of(&bounced, &dir, Scope::Mix)
            .run(whole(&project), false, |_| true)
            .unwrap();
        let (diff, at) = worst(&before, &after);
        assert!(
            diff <= TOLERANCE,
            "the bounce sounds different by {diff} at {at}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
