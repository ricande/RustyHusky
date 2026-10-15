//! Recording a round as a 1080 × 1920 (9:16), 60 fps MP4 with sound.
//!
//! While playing, every video frame is rendered off screen (`capture.rs`)
//! and piped as raw RGBA to an `ffmpeg` process that encodes H.264 into a
//! temporary file. The sound track is mixed afterwards from the same sound
//! effects the player heard, with their recording-clock times, so picture
//! and sound stay in sync. Only when the player chooses "Save video" are
//! video and sound combined into the final file; otherwise the temporary
//! file is deleted.
//!
//! Nothing here touches the window, so it is tested headlessly (the
//! pipeline tests skip themselves when ffmpeg is missing).

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::audio::{wav_bytes, SAMPLE_RATE, VOLUME};
use crate::config::{self, APP_ID, APP_NAME};
use crate::t;

pub const WIDTH: u32 = 1080;
pub const HEIGHT: u32 = 1920;
pub const FPS: u32 = 60;
/// Longest wait for ffmpeg to finish a file before it is stopped, so a
/// stuck encoder can never lock the game.
const FFMPEG_TIMEOUT: Duration = Duration::from_secs(60);

/// Decides how many video frames are due as recording time passes, so the
/// video runs at exactly `FPS` whatever the monitor's refresh rate is.
#[derive(Debug, Default)]
pub struct FrameClock {
    time: f64,
    written: u64,
}

impl FrameClock {
    /// Advances the clock; returns how many frames to write now (the
    /// current picture, repeated if the display ran slower than `FPS`).
    pub fn advance(&mut self, dt: f64) -> u64 {
        self.time += dt.max(0.0);
        // Frame n shows time n / FPS; frame 0 is due immediately.
        let due = (self.time * FPS as f64 + 1e-9).floor() as u64 + 1;
        let n = due.saturating_sub(self.written);
        self.written = due.max(self.written);
        n
    }

    pub fn time(&self) -> f64 {
        self.time
    }

    pub fn frames(&self) -> u64 {
        self.written
    }
}

/// A sound effect started at `time` seconds into the recording.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundHit {
    pub time: f64,
    pub sfx: usize,
}

/// Mixes the effects into one mono track of `duration` seconds.
pub fn mix(hits: &[SoundHit], samples: &[Vec<f32>], duration: f64) -> Vec<f32> {
    let len = (duration * SAMPLE_RATE as f64).ceil() as usize;
    let mut out = vec![0.0f32; len];
    for hit in hits {
        let Some(sound) = samples.get(hit.sfx) else {
            continue;
        };
        let start = (hit.time * SAMPLE_RATE as f64).round() as usize;
        for (o, s) in out.iter_mut().skip(start).zip(sound) {
            *o += s * VOLUME;
        }
    }
    for o in &mut out {
        *o = o.clamp(-1.0, 1.0);
    }
    // The video can end in the middle of a sound; fade the last 20 ms so
    // the cut does not click.
    let fade = (SAMPLE_RATE as usize / 50).min(out.len());
    let n = out.len();
    for (i, o) in out[n - fade..].iter_mut().enumerate() {
        *o *= 1.0 - (i + 1) as f32 / fade as f32;
    }
    out
}

/// The ffmpeg program: `<ENV_PREFIX>_FFMPEG` if set, else `ffmpeg` in `PATH`.
fn ffmpeg_program() -> OsString {
    config::env("FFMPEG").unwrap_or_else(|| "ffmpeg".into())
}

fn ffmpeg() -> Command {
    let mut cmd = Command::new(ffmpeg_program());
    cmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
    cmd
}

/// Checks that ffmpeg can be started and has the H.264 encoder. The error
/// is a player-readable reason.
pub fn check_ffmpeg() -> Result<(), String> {
    let out = Command::new(ffmpeg_program())
        .args(["-hide_banner", "-encoders"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(out) if out.status.success() => {
            if String::from_utf8_lossy(&out.stdout).contains("libx264") {
                Ok(())
            } else {
                Err(t!("video.no_x264").into())
            }
        }
        _ => Err(t!("video.no_ffmpeg").into()),
    }
}

/// Temporary recording files start with this, e.g. `rustyhusky-rec-`.
fn temp_prefix() -> String {
    format!("{APP_ID}-rec-")
}

fn temp_path(ext: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("{}{}-{n}.{ext}", temp_prefix(), std::process::id());
    std::env::temp_dir().join(name)
}

/// Removes leftovers from earlier runs that ended without cleaning up (for
/// example after a crash). Recent files are kept in case another copy of
/// the program is running.
pub fn remove_old_temp_files() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    let day = Duration::from_secs(24 * 3600);
    for entry in entries.flatten() {
        let name = entry.file_name();
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().unwrap_or_default() > day);
        if name.to_string_lossy().starts_with(&temp_prefix()) && old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Waits for `child` to exit, killing it after `FFMPEG_TIMEOUT`. Returns
/// the exit status and what it wrote to stderr.
fn wait_for(child: &mut Child) -> Result<(ExitStatus, String), String> {
    let deadline = Instant::now() + FFMPEG_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(t!("video.ffmpeg_timeout").into());
            }
            Err(e) => return Err(t!("video.ffmpeg_failed", error = e)),
        }
    };
    let mut err = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut err);
    }
    Ok((status, err))
}

/// Runs ffmpeg to completion and turns a failure into a readable message.
fn run_ffmpeg(args: &[&str]) -> Result<(), String> {
    let mut child = ffmpeg()
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| t!("video.ffmpeg_start", error = e))?;
    match wait_for(&mut child)? {
        (status, _) if status.success() => Ok(()),
        (_, err) => Err(t!("video.ffmpeg_failed", error = err.trim())),
    }
}

/// An ongoing recording.
pub struct Recorder {
    child: Child,
    frames_tx: Option<SyncSender<Vec<u8>>>,
    writer: Option<JoinHandle<std::io::Result<()>>>,
    path: PathBuf,
    clock: FrameClock,
    hits: Vec<SoundHit>,
}

impl Recorder {
    /// Starts ffmpeg, which reads raw RGBA frames of `WIDTH` × `HEIGHT`
    /// from a pipe and writes a temporary H.264 file without sound.
    /// `flip` mirrors the frames vertically (OpenGL reads them bottom-up).
    pub fn start(flip: bool) -> Result<Recorder, String> {
        let path = temp_path("mp4");
        let size = format!("{WIDTH}x{HEIGHT}");
        let fps = FPS.to_string();
        let mut cmd = ffmpeg();
        cmd.args(["-f", "rawvideo", "-pix_fmt", "rgba", "-s", &size])
            .args(["-framerate", &fps, "-i", "-"]);
        if flip {
            cmd.args(["-vf", "vflip"]);
        }
        cmd.args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "20"])
            .args(["-pix_fmt", "yuv420p", "-r", &fps, "-an"])
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| t!("video.ffmpeg_start", error = e))?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        // A small queue: encoding runs in parallel with the game, and if
        // ffmpeg falls behind the game waits rather than dropping frames.
        let (tx, rx) = sync_channel::<Vec<u8>>(4);
        let writer = std::thread::spawn(move || {
            for frame in rx {
                stdin.write_all(&frame)?;
            }
            Ok(())
        });
        Ok(Recorder {
            child,
            frames_tx: Some(tx),
            writer: Some(writer),
            path,
            clock: FrameClock::default(),
            hits: Vec::new(),
        })
    }

    /// Advances the recording clock; returns how many frames are due.
    pub fn frames_due(&mut self, dt: f64) -> u64 {
        self.clock.advance(dt)
    }

    /// Queues the current picture `copies` times (RGBA, `WIDTH` × `HEIGHT`).
    pub fn push_frame(&mut self, rgba: Vec<u8>, copies: u64) {
        debug_assert_eq!(rgba.len(), (WIDTH * HEIGHT * 4) as usize);
        let Some(tx) = &self.frames_tx else { return };
        let mut frame = Some(rgba);
        for i in 0..copies {
            let data = if i + 1 == copies {
                frame.take().unwrap_or_default()
            } else {
                frame.clone().unwrap_or_default()
            };
            if tx.send(data).is_err() {
                // ffmpeg has stopped; `finish` reports why.
                self.frames_tx = None;
                return;
            }
        }
    }

    /// Notes a sound effect at the current recording time.
    pub fn add_sound(&mut self, sfx: usize) {
        self.hits.push(SoundHit {
            time: self.clock.time(),
            sfx,
        });
    }

    fn close(&mut self) -> Result<(), String> {
        // Closing the queue ends the writer thread, which closes ffmpeg's
        // input so it finishes the file.
        self.frames_tx = None;
        let waited = wait_for(&mut self.child);
        let wrote = self.writer.take().map(|w| w.join());
        let (status, err) = waited?;
        if !status.success() {
            return Err(t!("video.ffmpeg_failed", error = err.trim()));
        }
        match wrote {
            Some(Ok(Err(e))) => Err(t!("video.pipe", error = e)),
            Some(Err(_)) => Err(t!("video.crashed").into()),
            _ => Ok(()),
        }
    }

    /// Ends the recording and waits for the encoder to finish the file.
    /// On failure the temporary file is removed.
    pub fn finish(mut self) -> Result<Recording, String> {
        let result = self.close();
        let frames = self.clock.frames();
        let path = std::mem::take(&mut self.path);
        let result = match result {
            Ok(()) if frames > 0 => {
                return Ok(Recording {
                    video: path,
                    hits: std::mem::take(&mut self.hits),
                    duration: frames as f64 / FPS as f64,
                })
            }
            _ if frames == 0 => Err(t!("video.empty").into()),
            Ok(()) => unreachable!("frames > 0 is handled above"),
            Err(e) => Err(e),
        };
        let _ = std::fs::remove_file(&path);
        result
    }

    /// Stops and throws the recording away.
    pub fn cancel(self) {
        if let Ok(rec) = self.finish() {
            rec.discard();
        }
    }
}

/// A finished, not yet saved recording (a temporary video without sound).
#[derive(Debug)]
pub struct Recording {
    video: PathBuf,
    hits: Vec<SoundHit>,
    duration: f64,
}

impl Recording {
    #[cfg(test)]
    pub fn duration(&self) -> f64 {
        self.duration
    }

    #[cfg(test)]
    pub fn temp_file(&self) -> &Path {
        &self.video
    }

    /// Writes the final MP4 (video + mixed sound) to `out`. The recording
    /// stays available, so a failed save can be retried.
    pub fn save(&self, samples: &[Vec<f32>], out: &Path) -> Result<(), String> {
        let wav = temp_path("wav");
        let result = (|| {
            if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)
                    .map_err(|e| t!("video.mkdir", path = dir.display(), error = e))?;
            }
            let track = mix(&self.hits, samples, self.duration);
            std::fs::write(&wav, wav_bytes(&track))
                .map_err(|e| t!("video.write_sound", error = e))?;
            // Write next to the target and rename, so a failure never
            // leaves a half-written file under the final name.
            let partial = out.with_extension("partial.mp4");
            let (video, sound, part) = (
                self.video.to_string_lossy(),
                wav.to_string_lossy(),
                partial.to_string_lossy(),
            );
            #[rustfmt::skip]
            let encoded = run_ffmpeg(&[
                "-i", &video,
                "-i", &sound,
                "-map", "0:v:0",
                "-map", "1:a:0",
                "-c:v", "copy",
                "-c:a", "aac",
                "-b:a", "160k",
                "-ar", "48000",
                // Stereo: mono tracks are dropped or played on one side by
                // some players and sites.
                "-ac", "2",
                "-movflags", "+faststart",
                "-f", "mp4",
                &part,
            ]);
            let renamed = encoded.and_then(|()| {
                std::fs::rename(&partial, out)
                    .map_err(|e| t!("video.write_file", path = out.display(), error = e))
            });
            if renamed.is_err() {
                let _ = std::fs::remove_file(&partial);
            }
            renamed
        })();
        let _ = std::fs::remove_file(&wav);
        result
    }

    /// Deletes the temporary video.
    pub fn discard(self) {
        let _ = std::fs::remove_file(&self.video);
    }
}

/// The folder videos are saved in: the user's video folder + `APP_NAME`
/// (overridable with `<ENV_PREFIX>_VIDEO_DIR`).
pub fn default_dir() -> PathBuf {
    let env = |key: &str| std::env::var_os(key).filter(|v| !v.is_empty());
    if let Some(dir) = config::env("VIDEO_DIR") {
        return PathBuf::from(dir);
    }
    let home = env("HOME")
        .or_else(|| env("USERPROFILE"))
        .map(PathBuf::from);
    let videos = if cfg!(windows) {
        home.map(|h| h.join("Videos"))
    } else if cfg!(target_os = "macos") {
        home.map(|h| h.join("Movies"))
    } else {
        env("XDG_VIDEOS_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                let home = home.as_ref()?;
                let dirs = std::fs::read_to_string(home.join(".config/user-dirs.dirs")).ok()?;
                xdg_videos_dir(&dirs, home)
            })
            .or_else(|| home.map(|h| h.join("Videos")))
    };
    videos.unwrap_or_default().join(APP_NAME)
}

/// Reads `XDG_VIDEOS_DIR="$HOME/Videos"` from `user-dirs.dirs`.
fn xdg_videos_dir(dirs: &str, home: &Path) -> Option<PathBuf> {
    let line = dirs
        .lines()
        .find(|l| l.trim_start().starts_with("XDG_VIDEOS_DIR="))?;
    let value = line.split_once('=')?.1.trim().trim_matches('"');
    let path = match value.strip_prefix("$HOME") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(value),
    };
    // A video folder equal to home means "not set".
    (path != home).then_some(path)
}

/// File name from `APP_ID` and the time (UTC), e.g.
/// `rustyhusky-2026-10-15-153000`.
pub fn default_name(now: SystemTime) -> String {
    let secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let s = secs % 86_400;
    format!(
        "{APP_ID}-{y:04}-{m:02}-{d:02}-{:02}{:02}{:02}",
        s / 3600,
        s / 60 % 60,
        s % 60
    )
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian
/// calendar (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// `<dir>/<name>.mp4`, or `<name>-2.mp4`, `-3` … if that file exists, so a
/// save never overwrites an earlier video.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let mut path = dir.join(format!("{name}.mp4"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{name}-{n}.mp4"));
        n += 1;
    }
    path
}

/// Shows a path with the home folder as `~`.
pub fn display_path(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        if let Ok(rest) = path.strip_prefix(&home) {
            return if rest.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~/{}", rest.display())
            };
        }
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_clock_gives_exactly_60_frames_per_second() {
        for display_hz in [30.0, 60.0, 100.0, 144.0, 47.3] {
            let mut clock = FrameClock::default();
            let mut written = clock.advance(0.0);
            assert_eq!(written, 1, "first frame at t = 0");
            let frames = (display_hz * 10.0) as usize;
            for _ in 0..frames {
                written += clock.advance(1.0 / display_hz);
            }
            // Ten seconds: frames for t = 0, 1/60, ..., 10.
            assert_eq!(written, 10 * FPS as u64 + 1, "{display_hz} Hz");
        }
        // A long hitch repeats the picture instead of skipping time.
        let mut clock = FrameClock::default();
        clock.advance(0.0);
        assert_eq!(clock.advance(0.1), 6);
    }

    #[test]
    fn sounds_are_mixed_at_their_time() {
        let samples = vec![vec![0.5; 100], vec![0.25; 10]];
        let hits = [
            SoundHit { time: 0.0, sfx: 0 },
            SoundHit { time: 1.0, sfx: 1 },
            SoundHit { time: 1.0, sfx: 1 },
            SoundHit { time: 9.0, sfx: 0 }, // after the end: ignored
            SoundHit { time: 0.5, sfx: 7 }, // unknown effect: ignored
        ];
        let track = mix(&hits, &samples, 2.0);
        assert_eq!(track.len(), 2 * SAMPLE_RATE as usize);
        assert!((track[0] - 0.5 * VOLUME).abs() < 1e-6);
        assert_eq!(track[100], 0.0);
        let at = SAMPLE_RATE as usize;
        assert!(
            (track[at] - 0.5 * VOLUME).abs() < 1e-6,
            "overlapping hits add up"
        );
        assert_eq!(track[at + 10], 0.0);
        let loud = mix(&[SoundHit { time: 0.0, sfx: 0 }; 5], &samples, 0.1);
        assert_eq!(loud[0], 1.0, "clamped");
        // A sound cut off by the end fades to silence.
        let cut = mix(&[SoundHit { time: 0.0, sfx: 0 }], &[vec![0.5; 5000]], 0.1);
        assert_eq!(*cut.last().unwrap(), 0.0);
        assert!(cut[cut.len() - 500] > 0.0);
    }

    #[test]
    fn names_and_paths() {
        let t = UNIX_EPOCH + Duration::from_secs(1_791_991_800); // 2026-10-14 15:30:00 UTC
        assert_eq!(default_name(t), format!("{APP_ID}-2026-10-14-153000"));
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));

        let dir = crate::tests::temp_dir("unique");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unique_path(&dir, "a"), dir.join("a.mp4"));
        std::fs::write(dir.join("a.mp4"), "x").unwrap();
        std::fs::write(dir.join("a-2.mp4"), "x").unwrap();
        assert_eq!(unique_path(&dir, "a"), dir.join("a-3.mp4"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn xdg_video_dir_is_read() {
        let home = Path::new("/home/u");
        let dirs =
            "# comment\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\nXDG_VIDEOS_DIR=\"$HOME/Videor\"\n";
        assert_eq!(
            xdg_videos_dir(dirs, home),
            Some(PathBuf::from("/home/u/Videor"))
        );
        assert_eq!(xdg_videos_dir("XDG_VIDEOS_DIR=\"$HOME/\"", home), None);
        assert_eq!(xdg_videos_dir("", home), None);
        if config::env("VIDEO_DIR").is_none() {
            assert!(default_dir().ends_with(APP_NAME));
        }
    }

    fn probe(file: &Path) -> String {
        let out = Command::new("ffprobe")
            .args(["-v", "error", "-show_entries"])
            .args(["stream=codec_type,codec_name,width,height,r_frame_rate,avg_frame_rate,channels:format=duration"])
            .args(["-of", "default=noprint_wrappers=1"])
            .arg(file)
            .output()
            .expect("ffprobe is installed with ffmpeg");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// The real pipeline: encode frames with ffmpeg, mix sound, save, and
    /// check the file with ffprobe. Skipped where ffmpeg is not installed.
    #[test]
    fn recording_is_saved_as_1080x1920_60fps_mp4_with_sound() {
        if check_ffmpeg().is_err() {
            eprintln!("ffmpeg missing: skipping the video test");
            return;
        }
        let mut rec = Recorder::start(false).unwrap();
        let mut frame = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
        let mut total = rec.frames_due(0.0);
        rec.push_frame(frame.clone(), total);
        // A 45 Hz display: some pictures are repeated to keep 60 fps.
        for i in 0..44 {
            frame.iter_mut().for_each(|b| *b = (i * 5) as u8);
            let n = rec.frames_due(1.0 / 45.0);
            if i == 10 {
                rec.add_sound(crate::audio::Sfx::Blip.index());
            }
            rec.push_frame(frame.clone(), n);
            total += n;
        }
        let recording = rec.finish().unwrap();
        assert!((recording.duration() - total as f64 / 60.0).abs() < 1e-9);
        let temp = recording.temp_file().to_owned();
        assert!(temp.exists());

        let dir = crate::tests::temp_dir("video");
        let out = dir.join("sub/clip.mp4");
        let samples = crate::audio::all_samples();
        recording.save(&samples, &out).unwrap();
        // Saving twice works (retry).
        recording.save(&samples, &out).unwrap();

        let info = probe(&out);
        for expected in [
            "codec_name=h264",
            "width=1080",
            "height=1920",
            "r_frame_rate=60/1",
            "avg_frame_rate=60/1",
            "codec_type=audio",
            "codec_name=aac",
            "channels=2",
        ] {
            assert!(info.contains(expected), "{expected} missing in:\n{info}");
        }
        let duration: f64 = info
            .lines()
            .find_map(|l| l.strip_prefix("duration="))
            .and_then(|d| d.parse().ok())
            .unwrap();
        assert!((duration - recording.duration()).abs() < 0.1, "{duration}");
        assert!(!out.with_extension("partial.mp4").exists());

        // The sound track really contains the sound (not silence).
        let stats = Command::new("ffmpeg")
            .args(["-hide_banner", "-i"])
            .arg(&out)
            .args(["-map", "0:a", "-af", "volumedetect", "-f", "null", "-"])
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&stats.stderr);
        let max_db: f64 = log
            .lines()
            .find_map(|l| l.split("max_volume:").nth(1))
            .and_then(|v| v.trim().trim_end_matches("dB").trim().parse().ok())
            .unwrap_or_else(|| panic!("no volume in:\n{log}"));
        assert!(
            max_db > -30.0,
            "the sound track is (nearly) silent: {max_db} dB"
        );
        recording.discard();
        assert!(!temp.exists(), "discard removes the temporary file");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_save_is_reported_and_can_be_retried() {
        if check_ffmpeg().is_err() {
            return;
        }
        let mut rec = Recorder::start(false).unwrap();
        let n = rec.frames_due(0.2);
        rec.push_frame(vec![0; (WIDTH * HEIGHT * 4) as usize], n);
        let recording = rec.finish().unwrap();
        // The "folder" is a file, so the folder cannot be created.
        let dir = crate::tests::temp_dir("video-blocked");
        std::fs::create_dir_all(&dir).unwrap();
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, "x").unwrap();
        let err = recording
            .save(&crate::audio::all_samples(), &blocker.join("a.mp4"))
            .unwrap_err();
        assert!(err.contains("folder"), "{err}");
        let ok = dir.join("retry.mp4");
        recording.save(&crate::audio::all_samples(), &ok).unwrap();
        assert!(ok.exists());
        recording.discard();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_encoder_that_dies_gives_an_error_not_a_hang() {
        if check_ffmpeg().is_err() {
            return;
        }
        let mut rec = Recorder::start(false).unwrap();
        let path = rec.path.clone();
        rec.child.kill().unwrap();
        for _ in 0..10 {
            let n = rec.frames_due(1.0 / 60.0);
            rec.push_frame(vec![0; (WIDTH * HEIGHT * 4) as usize], n);
        }
        assert!(rec.finish().is_err());
        assert!(!path.exists(), "no temporary file is left");
    }

    #[test]
    fn a_recording_without_frames_is_an_error_and_leaves_no_file() {
        if check_ffmpeg().is_err() {
            return;
        }
        let rec = Recorder::start(false).unwrap();
        let path = rec.path.clone();
        assert!(rec.finish().unwrap_err().contains("empty"));
        assert!(!path.exists());
    }
}
