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
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::audio::{wav_bytes, SAMPLE_RATE, VOLUME};
use crate::config::{self, APP_ID, APP_NAME};
use crate::t;

pub const WIDTH: u32 = 1080;
pub const HEIGHT: u32 = 1920;
pub const FPS: u32 = 60;
/// Pictures waiting for the encoder (8 MB each, however often they are
/// repeated): about 0.4 s of video, at most about 200 MB, used only while
/// the encoder is behind. Bounded, so memory stays bounded.
const QUEUE_PICTURES: usize = 24;
/// Longest wait for ffmpeg to finish or save a file (on a worker thread).
const FINISH_TIMEOUT: Duration = Duration::from_secs(60);
/// Longest wait for the ffmpeg check at startup.
const CHECK_TIMEOUT: Duration = Duration::from_secs(5);
/// How long to wait for a pipe thread after its process has ended.
const PIPE_GRACE: Duration = Duration::from_secs(1);

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
    let mut cmd = Command::new(ffmpeg_program());
    cmd.args(["-hide_banner", "-encoders"]);
    check_encoders(cmd, CHECK_TIMEOUT)
}

/// Runs `cmd` (an `ffmpeg -encoders`) and looks for libx264 in its output.
fn check_encoders(mut cmd: Command, timeout: Duration) -> Result<(), String> {
    cmd.stdin(Stdio::null());
    let Ok(mut process) = Process::spawn(cmd) else {
        return Err(t!("video.no_ffmpeg").into());
    };
    match process.wait(timeout, &SHUTDOWN) {
        Ok(exit) if exit.status.success() && exit.stdout.contains("libx264") => Ok(()),
        Ok(exit) if exit.status.success() => Err(t!("video.no_x264").into()),
        Ok(_) => Err(t!("video.no_ffmpeg").into()),
        Err(_) => Err(t!("video.no_answer").into()),
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

/// Set when the program closes: every wait for ffmpeg stops at once and
/// kills the process (see `shut_down`).
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// Stops all running ffmpeg work. Called when the program closes, so no
/// worker thread can keep it waiting.
pub fn shut_down() {
    SHUTDOWN.store(true, Ordering::Relaxed);
}

/// A child process whose stdout and stderr are drained by reader threads,
/// so a full pipe can never make it block.
struct Process {
    child: Child,
    stdout: Option<JoinHandle<Vec<u8>>>,
    stderr: Option<JoinHandle<Vec<u8>>>,
}

/// How a `Process` ended.
struct Exit {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

fn drain(mut pipe: impl Read + Send + 'static) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        bytes
    })
}

/// Joins `handle` if it finishes within `grace`; otherwise leaves it, so a
/// thread stuck on a pipe can never block the caller.
fn join_within<T>(handle: JoinHandle<T>, grace: Duration) -> Option<std::thread::Result<T>> {
    let deadline = Instant::now() + grace;
    while !handle.is_finished() {
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Some(handle.join())
}

impl Process {
    /// Starts `cmd` with stdout and stderr captured (stdin as set on `cmd`).
    fn spawn(mut cmd: Command) -> Result<Process, String> {
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| t!("video.ffmpeg_start", error = e))?;
        let stdout = child.stdout.take().map(drain);
        let stderr = child.stderr.take().map(drain);
        Ok(Process {
            child,
            stdout,
            stderr,
        })
    }

    /// Waits for the process to exit. After `timeout`, or as soon as
    /// `cancel` is set, it is killed and an error returned.
    fn wait(&mut self, timeout: Duration, cancel: &AtomicBool) -> Result<Exit, String> {
        let deadline = Instant::now() + timeout;
        let status = loop {
            match self.child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline && !cancel.load(Ordering::Relaxed) => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(None) => {
                    self.kill();
                    return Err(t!("video.ffmpeg_timeout").into());
                }
                Err(e) => {
                    self.kill();
                    return Err(t!("video.ffmpeg_failed", error = e));
                }
            }
        };
        let text = |pipe: Option<JoinHandle<Vec<u8>>>| {
            pipe.and_then(|h| join_within(h, PIPE_GRACE))
                .and_then(Result::ok)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default()
        };
        let stdout = text(self.stdout.take());
        let stderr = text(self.stderr.take());
        Ok(Exit {
            status,
            stdout,
            stderr,
        })
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Runs ffmpeg to completion and turns a failure into a readable message.
fn run_ffmpeg(args: &[&str]) -> Result<(), String> {
    let mut cmd = ffmpeg();
    cmd.args(args).stdin(Stdio::null());
    let exit = Process::spawn(cmd)?.wait(FINISH_TIMEOUT, &SHUTDOWN)?;
    if exit.status.success() {
        Ok(())
    } else {
        Err(t!("video.ffmpeg_failed", error = exit.stderr.trim()))
    }
}

/// A picture and how many video frames show it.
type Picture = (Arc<Vec<u8>>, u64);

/// The result of offering a picture to the encoder without waiting.
#[derive(Debug)]
pub enum Offer {
    Accepted,
    /// The queue is full; the picture is handed back to offer again later.
    Full(Vec<u8>),
    /// The encoder has stopped; `finish` gives the reason.
    Failed,
}

/// Why a recording was stopped by the program rather than by ffmpeg.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stopped {
    /// Prioritize gameplay: the queue was full, the encoder could not keep up.
    Behind,
    /// Prioritize recording: the encoder took nothing for a long time.
    Hung,
}

/// An ongoing recording.
pub struct Recorder {
    process: Process,
    frames_tx: Option<SyncSender<Picture>>,
    writer: Option<JoinHandle<std::io::Result<()>>>,
    /// Video frames the writer thread has passed to ffmpeg so far: shows
    /// whether a slow encoder still makes progress.
    written: Arc<AtomicU64>,
    path: PathBuf,
    /// Size of one frame in bytes (`WIDTH` × `HEIGHT` × 4; smaller in tests).
    frame_bytes: usize,
    clock: FrameClock,
    /// Video frames accepted into the queue: the length of the video.
    frames: u64,
    hits: Vec<SoundHit>,
    stopped: Option<Stopped>,
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
            .arg(&path);
        let frame_bytes = (WIDTH * HEIGHT * 4) as usize;
        Recorder::spawn(cmd, path, frame_bytes, QUEUE_PICTURES)
    }

    /// Starts `cmd` as the encoder, feeding it frames on stdin through a
    /// queue of at most `queue` pictures.
    fn spawn(
        mut cmd: Command,
        path: PathBuf,
        frame_bytes: usize,
        queue: usize,
    ) -> Result<Recorder, String> {
        cmd.stdin(Stdio::piped());
        let mut process = Process::spawn(cmd)?;
        let mut stdin = process.child.stdin.take().expect("piped stdin");
        let written = Arc::new(AtomicU64::new(0));
        let progress = written.clone();
        // A small, bounded queue: encoding runs in parallel with the game.
        let (tx, rx) = sync_channel::<Picture>(queue);
        let writer = std::thread::spawn(move || {
            for (picture, copies) in rx {
                for _ in 0..copies {
                    stdin.write_all(&picture)?;
                    progress.fetch_add(1, Ordering::Relaxed);
                }
            }
            Ok(())
        });
        Ok(Recorder {
            process,
            frames_tx: Some(tx),
            writer: Some(writer),
            written,
            path,
            frame_bytes,
            clock: FrameClock::default(),
            frames: 0,
            hits: Vec::new(),
            stopped: None,
        })
    }

    /// Prioritize gameplay: advances the recording clock by real time and
    /// returns how many frames are due.
    pub fn frames_due(&mut self, dt: f64) -> u64 {
        self.clock.advance(dt)
    }

    /// Offers a picture for the next `copies` video frames, without waiting.
    pub fn offer(&mut self, rgba: Vec<u8>, copies: u64) -> Offer {
        debug_assert_eq!(rgba.len(), self.frame_bytes);
        let Some(tx) = &self.frames_tx else {
            return Offer::Failed;
        };
        match tx.try_send((Arc::new(rgba), copies)) {
            Ok(()) => {
                self.frames += copies;
                Offer::Accepted
            }
            Err(TrySendError::Full((picture, _))) => {
                Offer::Full(Arc::try_unwrap(picture).unwrap_or_else(|p| (*p).clone()))
            }
            Err(TrySendError::Disconnected(_)) => {
                self.frames_tx = None;
                Offer::Failed
            }
        }
    }

    /// Prioritize gameplay: queues the current picture for `copies` video
    /// frames (RGBA, `WIDTH` × `HEIGHT`) without ever waiting.
    ///
    /// If the bounded queue is full, the encoder has fallen behind (it hangs
    /// or cannot encode in real time). Frames are never dropped, since the
    /// video would then be shorter than its sound; instead the encoder is
    /// stopped and the recording fails (`failed`). So the video is either
    /// complete or absent.
    pub fn push_frame(&mut self, rgba: Vec<u8>, copies: u64) {
        if copies == 0 {
            return;
        }
        if let Offer::Full(_) = self.offer(rgba, copies) {
            self.stop(Stopped::Behind);
        }
    }

    /// Length of the video recorded so far, in seconds.
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / FPS as f64
    }

    /// Video frames ffmpeg has taken so far (grows while it makes progress).
    pub fn written(&self) -> u64 {
        self.written.load(Ordering::Relaxed)
    }

    /// Prioritize recording: the encoder took nothing for too long.
    pub fn give_up_hung(&mut self) {
        self.stop(Stopped::Hung);
    }

    /// Stops the encoder, which also ends the writer thread.
    fn stop(&mut self, why: Stopped) {
        self.stopped = Some(why);
        self.frames_tx = None;
        self.process.kill();
    }

    /// The encoder stopped (crashed, hung or fell behind) and no more
    /// frames can be recorded; `finish` gives the reason.
    pub fn failed(&self) -> bool {
        self.frames_tx.is_none()
    }

    /// Prioritize gameplay: notes a sound effect at the current recording
    /// time.
    pub fn add_sound(&mut self, sfx: usize) {
        self.add_sound_at(self.clock.time(), sfx);
    }

    /// Notes a sound effect at `time` seconds into the video.
    pub fn add_sound_at(&mut self, time: f64, sfx: usize) {
        self.hits.push(SoundHit { time, sfx });
    }

    fn close(&mut self) -> Result<(), String> {
        // Closing the queue ends the writer thread, which closes ffmpeg's
        // input so it finishes the file.
        self.frames_tx = None;
        let waited = self.process.wait(FINISH_TIMEOUT, &SHUTDOWN);
        let wrote = self.writer.take().and_then(|w| join_within(w, PIPE_GRACE));
        match self.stopped {
            Some(Stopped::Behind) => return Err(t!("video.encoder_behind").into()),
            Some(Stopped::Hung) => return Err(t!("video.encoder_hung").into()),
            None => {}
        }
        let exit = waited?;
        if !exit.status.success() {
            return Err(t!("video.ffmpeg_failed", error = exit.stderr.trim()));
        }
        match wrote {
            Some(Ok(Err(e))) => Err(t!("video.pipe", error = e)),
            Some(Err(_)) => Err(t!("video.crashed").into()),
            _ => Ok(()),
        }
    }

    /// Ends the recording and waits for the encoder to finish the file.
    /// The video is as long as the frames accepted into the queue. On
    /// failure the temporary file is removed.
    pub fn finish(mut self) -> Result<Recording, String> {
        let result = self.close();
        let frames = self.frames;
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

    /// Stops the encoder at once and throws the recording away, without
    /// waiting for ffmpeg to finish the file.
    pub fn cancel(mut self) {
        self.frames_tx = None;
        self.process.kill();
        if let Some(writer) = self.writer.take() {
            let _ = join_within(writer, PIPE_GRACE);
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(all(test, unix))]
impl Recorder {
    /// A recorder whose encoder is `sh -c script`, with frames of
    /// `frame_bytes` bytes and a queue of `queue` pictures.
    pub fn with_script(script: &str, frame_bytes: usize, queue: usize) -> Recorder {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script]);
        Recorder::spawn(cmd, temp_path("mp4"), frame_bytes, queue).unwrap()
    }

    #[cfg(target_os = "linux")]
    pub fn frames(&self) -> u64 {
        self.frames
    }

    #[cfg(target_os = "linux")]
    pub fn hits(&self) -> &[SoundHit] {
        &self.hits
    }

    pub fn pid(&self) -> u32 {
        self.process.child.id()
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
pub(crate) mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// Tests that encode video or measure waiting take turns, so they do
    /// not starve each other of CPU on small test machines.
    pub(crate) fn heavy() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether the tests that need a real ffmpeg can run. Without it they
    /// are skipped, unless `REQUIRE_FFMPEG` is set (as in CI): then a
    /// missing ffmpeg is a failure instead of a silent skip.
    pub(crate) fn ffmpeg_for_tests() -> bool {
        match check_ffmpeg() {
            Ok(()) => true,
            Err(e) if std::env::var_os("REQUIRE_FFMPEG").is_some() => {
                panic!("REQUIRE_FFMPEG is set, but: {e}")
            }
            Err(_) => {
                eprintln!("ffmpeg missing: skipping this test");
                false
            }
        }
    }

    /// Queues a picture, waiting for room: tests that produce frames faster
    /// than real time use this to test the file, not the machine's speed.
    pub(crate) fn push_waiting(rec: &mut Recorder, mut rgba: Vec<u8>, copies: u64) {
        let deadline = Instant::now() + Duration::from_secs(30);
        if copies == 0 {
            return;
        }
        loop {
            match rec.offer(rgba, copies) {
                Offer::Accepted | Offer::Failed => return,
                Offer::Full(back) => rgba = back,
            }
            assert!(
                Instant::now() < deadline,
                "the encoder took nothing for 30 s"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

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
        let _turn = heavy();
        if !ffmpeg_for_tests() {
            return;
        }
        let mut rec = Recorder::start(false).unwrap();
        let mut frame = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
        let mut total = rec.frames_due(0.0);
        push_waiting(&mut rec, frame.clone(), total);
        // A 45 Hz display: some pictures are repeated to keep 60 fps.
        for i in 0..44 {
            frame.iter_mut().for_each(|b| *b = (i * 5) as u8);
            let n = rec.frames_due(1.0 / 45.0);
            if i == 10 {
                rec.add_sound(crate::audio::Sfx::Blip.index());
            }
            push_waiting(&mut rec, frame.clone(), n);
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
        let _turn = heavy();
        if !ffmpeg_for_tests() {
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
        let _turn = heavy();
        if !ffmpeg_for_tests() {
            return;
        }
        let mut rec = Recorder::start(false).unwrap();
        let path = rec.path.clone();
        rec.process.child.kill().unwrap();
        for _ in 0..10 {
            let n = rec.frames_due(1.0 / 60.0);
            rec.push_frame(vec![0; (WIDTH * HEIGHT * 4) as usize], n);
        }
        assert!(rec.finish().is_err());
        assert!(!path.exists(), "no temporary file is left");
    }

    /// An encoder run by `sh -c script`, fed frames on stdin.
    #[cfg(unix)]
    fn fake_encoder(script: &str) -> Recorder {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script]);
        let frame_bytes = (WIDTH * HEIGHT * 4) as usize;
        Recorder::spawn(cmd, temp_path("mp4"), frame_bytes, QUEUE_PICTURES).unwrap()
    }

    /// A stand-in for an encoder that starts but never reads its input
    /// and never exits.
    #[cfg(unix)]
    fn hanging_encoder() -> Recorder {
        fake_encoder("exec sleep 30")
    }

    #[cfg(unix)]
    fn process_is_gone(pid: u32) -> bool {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| !s.success())
    }

    /// Feeds `rec` one picture per 1/60 s of real time, like the game, for
    /// up to `seconds`. Returns the longest time one `push_frame` took.
    #[cfg(unix)]
    fn play_in_real_time(rec: &mut Recorder, seconds: f64) -> Duration {
        let frame = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
        let mut longest = Duration::ZERO;
        let start = Instant::now();
        let mut n = 0;
        while start.elapsed().as_secs_f64() < seconds && !rec.failed() {
            let copies = rec.frames_due(if n == 0 { 0.0 } else { 1.0 / 60.0 });
            let t = Instant::now();
            rec.push_frame(frame.clone(), copies);
            longest = longest.max(t.elapsed());
            n += 1;
            let next = Duration::from_secs_f64(n as f64 / 60.0);
            if let Some(rest) = next.checked_sub(start.elapsed()) {
                std::thread::sleep(rest);
            }
        }
        longest
    }

    /// `push_frame` never waits for the encoder. Stopping one kills it and
    /// reaps the process, which a fully loaded machine can delay; a real
    /// regression would block for seconds or forever.
    #[cfg(unix)]
    pub(crate) const MAX_PUSH: Duration = Duration::from_millis(250);

    /// Regression test: an encoder that hangs without exiting must not
    /// freeze the game. No frame waits for it; once the queue is full the
    /// encoder is killed and the recording fails with a clear message.
    #[cfg(unix)]
    #[test]
    fn a_hanging_encoder_is_stopped_instead_of_freezing_the_game() {
        let _turn = heavy();
        let mut rec = hanging_encoder();
        let (pid, path) = (rec.process.child.id(), rec.path.clone());
        let longest = play_in_real_time(&mut rec, 3.0);
        assert!(longest < MAX_PUSH, "one frame waited {longest:?}");
        assert!(rec.failed(), "the hang is detected");
        assert!(process_is_gone(pid), "the encoder is killed");
        let started = Instant::now();
        let err = rec.finish().unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(err.contains("could not keep up"), "{err}");
        assert!(!path.exists(), "no temporary file is left");
    }

    /// An encoder that makes progress but slower than real time (about 15
    /// pictures per second) must not slow the game down either: the
    /// recording is stopped instead of dropping frames or growing the queue.
    #[cfg(unix)]
    #[test]
    fn a_slow_encoder_stops_the_recording_without_slowing_the_game() {
        let _turn = heavy();
        let mut rec = fake_encoder(
            "while dd of=/dev/null bs=1M count=8 iflag=fullblock 2>/dev/null; do sleep 0.06; done",
        );
        let pid = rec.process.child.id();
        let longest = play_in_real_time(&mut rec, 5.0);
        assert!(longest < MAX_PUSH, "one frame waited {longest:?}");
        assert!(rec.failed(), "falling behind is detected");
        assert!(process_is_gone(pid));
        assert!(rec.finish().unwrap_err().contains("could not keep up"));
    }

    /// An encoder that keeps up is never stopped, and repeated pictures are
    /// written as separate frames.
    #[cfg(unix)]
    #[test]
    fn an_encoder_that_keeps_up_records_every_frame() {
        let _turn = heavy();
        let out = temp_path("raw");
        let mut rec = fake_encoder(&format!("cat > '{}'", out.display()));
        let longest = play_in_real_time(&mut rec, 1.0);
        let kept_up = !rec.failed();
        rec.push_frame(vec![0; (WIDTH * HEIGHT * 4) as usize], 3);
        let frames = rec.frames;
        let finished = rec.finish().map(Recording::discard);
        let bytes = std::fs::metadata(&out).map(|m| m.len());
        // Clean up before checking, so a failure leaves no 300 MB file.
        let _ = std::fs::remove_file(&out);
        assert!(longest < MAX_PUSH, "one frame waited {longest:?}");
        assert!(kept_up && finished.is_ok(), "{finished:?}");
        assert_eq!(bytes.unwrap(), frames * u64::from(WIDTH * HEIGHT * 4));
    }

    #[cfg(unix)]
    #[test]
    fn cancelling_does_not_wait_for_a_hanging_encoder() {
        let mut rec = hanging_encoder();
        let (pid, path) = (rec.process.child.id(), rec.path.clone());
        rec.push_frame(vec![0; (WIDTH * HEIGHT * 4) as usize], 1);
        let started = Instant::now();
        rec.cancel();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(process_is_gone(pid));
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_startup_check_has_a_time_limit() {
        let sh = |script: &str| {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", script]);
            cmd
        };
        let limit = Duration::from_millis(300);
        let started = Instant::now();
        let err = check_encoders(sh("exec sleep 30"), limit).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(err, t!("video.no_answer"));
        assert_eq!(check_encoders(sh("echo ' V..... libx264'"), limit), Ok(()));
        let err = check_encoders(sh("echo ' V..... mpeg4'"), limit).unwrap_err();
        assert_eq!(err, t!("video.no_x264"));
        let missing = Command::new("/nonexistent/ffmpeg");
        assert_eq!(
            check_encoders(missing, limit).unwrap_err(),
            t!("video.no_ffmpeg")
        );
    }

    #[cfg(unix)]
    #[test]
    fn shutting_down_stops_a_wait_at_once() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "exec sleep 30"]);
        let mut process = Process::spawn(cmd).unwrap();
        let started = Instant::now();
        let stop = AtomicBool::new(true);
        assert!(process.wait(Duration::from_secs(60), &stop).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    /// Lots of output on stdout and stderr must not block the process.
    #[cfg(unix)]
    #[test]
    fn full_pipes_do_not_block() {
        let mut cmd = Command::new("sh");
        cmd.args([
            "-c",
            "head -c 1000000 /dev/zero; head -c 1000000 /dev/zero >&2",
        ]);
        let exit = Process::spawn(cmd)
            .unwrap()
            .wait(Duration::from_secs(10), &AtomicBool::new(false))
            .unwrap();
        assert!(exit.status.success());
        assert_eq!(
            (exit.stdout.len(), exit.stderr.len()),
            (1_000_000, 1_000_000)
        );
    }

    #[test]
    fn a_recording_without_frames_is_an_error_and_leaves_no_file() {
        if !ffmpeg_for_tests() {
            return;
        }
        let rec = Recorder::start(false).unwrap();
        let path = rec.path.clone();
        assert!(rec.finish().unwrap_err().contains("empty"));
        assert!(!path.exists());
    }
}
