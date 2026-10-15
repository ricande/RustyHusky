//! Pacing in the "Prioritize recording" mode: the game's time follows the
//! video frames produced, not the clock on the wall.
//!
//! Every video frame is made the same way: the game gets the input gathered
//! since the previous frame, is updated exactly `STEPS_PER_FRAME` physics
//! steps (2 × 1/120 s = 1/60 s), is drawn once, and the picture is offered
//! to the encoder exactly once as the next frame of the 60 fps video. If the
//! encoder's queue is full, the picture waits here and the game does not
//! advance until it has been accepted; the window keeps running (drawing,
//! pause, abort, closing) in the meantime. Nothing catches up afterwards,
//! so a slow encoder makes the game slower, never the video.
//!
//! Window-free: drawing is a closure, so the whole sequence is tested with
//! fake encoders.

use std::time::{Duration, Instant};

use crate::app::App;
use crate::audio::Sfx;
use crate::game::{Game, FIXED_DT};
use crate::video::{Offer, Recorder, FPS};

/// Physics steps per video frame: 2 × 1/120 s = 1/60 s.
pub const STEPS_PER_FRAME: u32 = 2;
const FRAME_DT: f64 = 1.0 / FPS as f64;
/// Most frames produced per displayed frame, so a 30 Hz display still
/// gives real-time speed. Unused time is not saved up beyond this, so
/// nothing is made up for after the encoder was slow.
const MAX_PER_DISPLAY: u32 = 2;
/// A waiting picture whose encoder writes nothing for this long means the
/// encoder has stopped responding (a slow encoder keeps writing).
pub const STALL_TIMEOUT: Duration = Duration::from_secs(5);

const _: () = assert!(
    (STEPS_PER_FRAME as f64 * FIXED_DT as f64 - 1.0 / FPS as f64).abs() < 1e-9,
    "two physics steps must make one video frame"
);

/// What the caller should do after a displayed frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// Keep going.
    Continue,
    /// The round is over (finished with its last frame accepted, or left):
    /// finish the recording.
    Finish,
    /// The encoder failed or hung: finish (it reports why) and go on with
    /// normal timing.
    Failed,
}

pub struct Lockstep {
    /// Real time available for producing frames, at most `MAX_PER_DISPLAY`
    /// frames' worth.
    credit: f64,
    /// A produced picture the encoder has not accepted yet.
    pending: Option<Vec<u8>>,
    /// Video frames produced so far, i.e. the index of the next one.
    produced: u64,
    /// While a picture waits: when the encoder last made progress, and its
    /// written-frame count then.
    progress: Option<(Instant, u64)>,
    pub stall_timeout: Duration,
}

impl Default for Lockstep {
    fn default() -> Self {
        Lockstep {
            credit: 0.0,
            pending: None,
            produced: 0,
            progress: None,
            stall_timeout: STALL_TIMEOUT,
        }
    }
}

impl Lockstep {
    /// A picture is waiting for the encoder. While the last picture of a
    /// level waits, the results must not be shown yet.
    pub fn holding(&self) -> bool {
        self.pending.is_some()
    }

    /// How long the waiting picture's encoder has made no progress.
    pub fn stalled_for(&self) -> Option<Duration> {
        self.pending.as_ref()?;
        self.progress.map(|(since, _)| since.elapsed())
    }

    /// Video frames produced so far.
    #[cfg(all(test, unix))]
    pub fn produced(&self) -> u64 {
        self.produced
    }

    /// Runs one displayed frame of `frame_time` seconds: delivers a waiting
    /// picture, then produces as many new frames as real time allows (none
    /// while one waits or the game is paused). `render` draws the game into
    /// a picture; `play` plays the game's sounds as they happen.
    pub fn frame<G: Game>(
        &mut self,
        app: &mut App<G>,
        rec: &mut Recorder,
        frame_time: f64,
        mut render: impl FnMut(&G) -> Vec<u8>,
        mut play: impl FnMut(Sfx),
    ) -> Next {
        if !app.in_round() {
            // Aborted: the waiting picture is dropped, not waited for.
            self.pending = None;
            return Next::Finish;
        }
        if let Some(picture) = self.pending.take() {
            match rec.offer(picture, 1) {
                Offer::Accepted => self.progress = None,
                Offer::Failed => return Next::Failed,
                Offer::Full(back) => {
                    self.pending = Some(back);
                    return self.check_progress(rec);
                }
            }
        }
        let Some(game) = app.running_game() else {
            // Paused: no simulation, no frames, and no time saved up.
            self.credit = 0.0;
            return Next::Continue;
        };
        if game.finished().is_some() || rec.at_limit() {
            return Next::Finish;
        }
        self.credit = (self.credit + frame_time).min(f64::from(MAX_PER_DISPLAY) * FRAME_DT);
        let mut made = 0;
        while self.credit >= FRAME_DT - 1e-9 && made < MAX_PER_DISPLAY {
            self.credit -= FRAME_DT;
            made += 1;
            // Frame 0 shows the state before any step.
            if self.produced > 0 {
                app.advance(STEPS_PER_FRAME);
            }
            let time = self.produced as f64 * FRAME_DT;
            for sfx in app.take_game_sounds() {
                play(sfx);
                rec.add_sound_at(time, sfx.index());
            }
            let game = app
                .running_game()
                .expect("advancing keeps the round running");
            let picture = render(game);
            let finished = game.finished().is_some();
            self.produced += 1;
            match rec.offer(picture, 1) {
                Offer::Accepted => {}
                Offer::Failed => return Next::Failed,
                Offer::Full(back) => {
                    self.pending = Some(back);
                    self.progress = Some((Instant::now(), rec.written()));
                    return Next::Continue;
                }
            }
            if finished || rec.at_limit() {
                return Next::Finish;
            }
        }
        Next::Continue
    }

    /// A picture waits: fine while the encoder still writes frames, a
    /// failure once it has written nothing for `stall_timeout`.
    fn check_progress(&mut self, rec: &mut Recorder) -> Next {
        let written = rec.written();
        match self.progress {
            Some((since, before)) if before == written => {
                if since.elapsed() >= self.stall_timeout {
                    self.pending = None;
                    rec.give_up_hung();
                    return Next::Failed;
                }
            }
            _ => self.progress = Some((Instant::now(), written)),
        }
        Next::Continue
    }
}

// The tests drive fake encoders through `sh`, so they need a Unix system;
// the slow-encoder ones also need GNU `dd`, so Linux.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::game::RoundResult;
    use crate::settings::Settings;
    use crate::ui::{Input, Key};
    #[cfg(target_os = "linux")]
    use crate::video::SoundHit;
    use crate::view::View;
    #[cfg(target_os = "linux")]
    use std::path::PathBuf;

    /// Small pictures keep the test files small, but are larger than a pipe
    /// buffer (64 KiB), so a slow encoder really holds the writer back.
    const BYTES: usize = 256 * 1024;
    /// The test game finishes after this many physics steps: frame 40.
    const END_STEPS: u64 = 80;

    /// Counts physics steps and presses, and beeps every 30 steps.
    struct Counter {
        steps: u64,
        presses: u32,
        sounds: Vec<Sfx>,
    }

    impl Game for Counter {
        const TEXTS: &'static [crate::i18n::Text] = &[];

        fn new(_level: u32) -> Self {
            Counter {
                steps: 0,
                presses: 0,
                sounds: Vec::new(),
            }
        }
        fn handle(&mut self, input: &Input) {
            self.presses += input.keys.iter().filter(|&&k| k == Key::Space).count() as u32;
        }
        fn update(&mut self, dt: f32) {
            assert_eq!(dt, FIXED_DT);
            self.steps += 1;
            if self.steps.is_multiple_of(30) {
                self.sounds.push(Sfx::Blip);
            }
        }
        fn draw(&self, _view: &View) {}
        fn take_sounds(&mut self) -> Vec<Sfx> {
            std::mem::take(&mut self.sounds)
        }
        fn finished(&self) -> Option<RoundResult> {
            (self.steps >= END_STEPS).then_some(RoundResult {
                score: self.presses,
            })
        }
    }

    /// The picture of a game state: its step and press counts.
    fn render(game: &Counter) -> Vec<u8> {
        let mut picture = vec![0u8; BYTES];
        picture[..8].copy_from_slice(&game.steps.to_le_bytes());
        picture[8..12].copy_from_slice(&game.presses.to_le_bytes());
        picture
    }

    fn key(k: Key) -> Input {
        Input {
            keys: vec![k],
            held: vec![k],
            ..Default::default()
        }
    }

    #[cfg(target_os = "linux")]
    fn out_path(name: &str) -> PathBuf {
        let dir = crate::tests::temp_dir(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("frames.raw")
    }

    #[cfg(target_os = "linux")]
    /// `cat` keeps up with anything.
    fn fast(out: &std::path::Path) -> String {
        format!("cat > '{}'", out.display())
    }

    #[cfg(target_os = "linux")]
    /// Reads one picture at a time with a pause: about 25 per second, and
    /// it never stops making progress.
    fn slow(out: &std::path::Path) -> String {
        let chunk = out.with_extension("chunk");
        format!(
            "while :; do dd bs={BYTES} count=1 iflag=fullblock of='{c}' 2>/dev/null; \
             [ -s '{c}' ] || exit 0; cat '{c}'; sleep 0.03; done > '{o}'",
            c = chunk.display(),
            o = out.display()
        )
    }

    #[cfg(target_os = "linux")]
    /// What a recorded round looked like.
    struct Run {
        /// (steps, presses) of every video frame, in file order.
        frames: Vec<(u64, u32)>,
        hits: Vec<SoundHit>,
        /// Display frames the round took.
        display_frames: u32,
        /// The display frames that found a picture waiting.
        waits: u32,
    }

    #[cfg(target_os = "linux")]
    /// Plays a round of `Counter` in "Prioritize recording" timing at a
    /// 60 Hz display. Space is pressed when frames 5 and 12 are next, and
    /// the game is paused for 10 display frames when frame 20 is next.
    fn record_round(script: &str, queue: usize, out: &std::path::Path) -> Run {
        let mut app: App<Counter> = App::new(Settings::default(), None);
        app.handle(&key(Key::Enter)); // Play
        assert!(app.take_round_started());
        let mut rec = Recorder::with_script(script, BYTES, queue);
        let mut lockstep = Lockstep::default();
        let (mut display_frames, mut waits, mut paused_for) = (0, 0, 0);
        let mut done = std::collections::BTreeSet::new();
        loop {
            display_frames += 1;
            assert!(display_frames < 5000, "the round never ended");
            let next_frame = lockstep.produced();
            let mut input = Input::default();
            if matches!(next_frame, 5 | 12) && done.insert(next_frame) {
                input = key(Key::Space);
            }
            if next_frame == 20 && done.insert(next_frame) {
                input = key(Key::Escape); // pause
            }
            if app.paused() {
                paused_for += 1;
                if paused_for == 10 {
                    input = key(Key::Escape); // resume
                }
            }
            app.handle(&input);
            waits += u32::from(lockstep.holding());
            match lockstep.frame(&mut app, &mut rec, 1.0 / 60.0, render, |_| {}) {
                Next::Continue => std::thread::sleep(Duration::from_millis(2)),
                Next::Finish => break,
                Next::Failed => panic!("the encoder failed"),
            }
        }
        assert!(!lockstep.holding(), "the last picture was accepted");
        let hits = rec.hits().to_vec();
        assert_eq!(rec.frames(), lockstep.produced());
        rec.finish().unwrap().discard();
        let bytes = std::fs::read(out).unwrap();
        assert_eq!(bytes.len() % BYTES, 0);
        let frames = bytes
            .chunks(BYTES)
            .map(|p| {
                let steps = u64::from_le_bytes(p[..8].try_into().unwrap());
                let presses = u32::from_le_bytes(p[8..12].try_into().unwrap());
                (steps, presses)
            })
            .collect();
        std::fs::remove_dir_all(out.parent().unwrap()).unwrap();
        Run {
            frames,
            hits,
            display_frames,
            waits,
        }
    }

    #[cfg(target_os = "linux")]
    /// The timeline every run must have: frame k shows the state after
    /// exactly 2k physics steps, from frame 0 (no step) to frame 40 (the
    /// level finished), each frame once; presses delivered once each.
    fn check_timeline(run: &Run) {
        let expected: Vec<u64> = (0..=END_STEPS / 2).map(|k| 2 * k).collect();
        let steps: Vec<u64> = run.frames.iter().map(|f| f.0).collect();
        assert_eq!(steps, expected, "two steps per frame, no frame twice");
        let presses: Vec<u32> = run.frames.iter().map(|f| f.1).collect();
        assert_eq!(presses[4], 0);
        assert_eq!(presses[5], 1, "a press reaches the next frame's step");
        assert_eq!(presses[11], 1);
        assert_eq!(presses[12], 2);
        assert_eq!(*presses.last().unwrap(), 2, "each press counted once");
        // Beeps at steps 30 and 60 belong to frames 15 and 30.
        let times: Vec<f64> = run.hits.iter().map(|h| h.time).collect();
        assert_eq!(times, vec![15.0 / 60.0, 30.0 / 60.0]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn fast_and_slow_encoders_give_the_same_video() {
        let _turn = crate::video::tests::heavy();
        let out = out_path("lockstep-fast");
        let fast_run = record_round(&fast(&out), 24, &out);
        check_timeline(&fast_run);
        let out = out_path("lockstep-slow");
        let slow_run = record_round(&slow(&out), 1, &out);
        check_timeline(&slow_run);
        assert_eq!(fast_run.frames, slow_run.frames, "the same simulation");
        assert_eq!(fast_run.hits, slow_run.hits, "the same sound timeline");
        // 41 frames and 10 paused display frames at 60 Hz; the slow encoder
        // (about 25 pictures per second) makes the round take longer, by
        // waiting, not by recording anything twice.
        assert_eq!(fast_run.display_frames, 41 + 10);
        assert!(slow_run.waits > 20, "it really waited: {}", slow_run.waits);
        assert!(slow_run.display_frames > fast_run.display_frames + 20);
    }

    /// A press while a picture waits is kept and reaches the game once.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_press_while_a_picture_waits_counts_once() {
        let _turn = crate::video::tests::heavy();
        let out = out_path("lockstep-press");
        let mut app: App<Counter> = App::new(Settings::default(), None);
        app.handle(&key(Key::Enter));
        let mut rec = Recorder::with_script(&slow(&out), BYTES, 1);
        let mut lockstep = Lockstep::default();
        let mut pressed_before = None;
        for _ in 0..2000 {
            let mut input = Input::default();
            if lockstep.holding() && pressed_before.is_none() && lockstep.produced() > 3 {
                input = key(Key::Space);
                pressed_before = Some(lockstep.produced());
            }
            app.handle(&input);
            if lockstep.frame(&mut app, &mut rec, 1.0 / 60.0, render, |_| {}) == Next::Finish {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        rec.finish().unwrap().discard();
        let bytes = std::fs::read(&out).unwrap();
        std::fs::remove_dir_all(out.parent().unwrap()).unwrap();
        let first = pressed_before.expect("a picture waited") as usize;
        let presses: Vec<u32> = bytes
            .chunks(BYTES)
            .map(|p| u32::from_le_bytes(p[8..12].try_into().unwrap()))
            .collect();
        assert_eq!(presses[first - 1], 0, "not before");
        assert_eq!(presses[first], 1, "in the next produced frame");
        assert_eq!(*presses.last().unwrap(), 1, "once");
    }

    /// An encoder that lives but never reads: the waiting picture never
    /// blocks a displayed frame, and after `stall_timeout` without progress
    /// the encoder is stopped with a clear message.
    #[cfg(unix)]
    #[test]
    fn a_hung_encoder_is_detected_without_blocking() {
        let _turn = crate::video::tests::heavy();
        let mut app: App<Counter> = App::new(Settings::default(), None);
        app.handle(&key(Key::Enter));
        let mut rec = Recorder::with_script("exec sleep 30", BYTES, 1);
        let pid = rec.pid();
        let mut lockstep = Lockstep {
            stall_timeout: Duration::from_millis(300),
            ..Lockstep::default()
        };
        let started = Instant::now();
        let mut longest = Duration::ZERO;
        let next = loop {
            assert!(started.elapsed() < Duration::from_secs(5), "never detected");
            let t = Instant::now();
            let next = lockstep.frame(&mut app, &mut rec, 1.0 / 60.0, render, |_| {});
            longest = longest.max(t.elapsed());
            if next != Next::Continue {
                break next;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(next, Next::Failed);
        assert!(
            longest < crate::video::tests::MAX_PUSH,
            "a frame took {longest:?}"
        );
        assert!(lockstep.produced() < 10, "the game did not run ahead");
        let err = rec.finish().unwrap_err();
        assert!(err.contains("stopped responding"), "{err}");
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(!alive, "the encoder was killed");
    }

    /// Aborting while a picture waits ends the round at once, and the
    /// recording can be thrown away without waiting for the encoder.
    #[cfg(unix)]
    #[test]
    fn aborting_while_a_picture_waits_is_immediate() {
        let _turn = crate::video::tests::heavy();
        let mut app: App<Counter> = App::new(Settings::default(), None);
        app.handle(&key(Key::Enter));
        let mut rec = Recorder::with_script("exec sleep 30", BYTES, 1);
        let mut lockstep = Lockstep::default();
        for _ in 0..20 {
            lockstep.frame(&mut app, &mut rec, 1.0 / 60.0, render, |_| {});
        }
        assert!(lockstep.holding());
        let produced = lockstep.produced();
        app.handle(&key(Key::Escape)); // pause
        app.handle(&key(Key::Down)); // to "Abort round"
        app.handle(&key(Key::Enter));
        assert!(!app.in_round());
        let started = Instant::now();
        let next = lockstep.frame(&mut app, &mut rec, 1.0 / 60.0, render, |_| {});
        assert_eq!(next, Next::Finish);
        assert!(!lockstep.holding(), "the waiting picture is dropped");
        assert_eq!(lockstep.produced(), produced);
        rec.cancel();
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    /// At the length limit no more frames are produced, the recording is to
    /// be finished, and the game is left to go on in real time.
    #[test]
    fn production_stops_at_the_length_limit() {
        let mut app: App<Counter> = App::new(Settings::default(), None);
        app.handle(&key(Key::Enter));
        let mut rec = Recorder::with_script("cat > /dev/null", BYTES, 24);
        rec.set_max_frames(Some(10));
        let mut lockstep = Lockstep::default();
        let mut next = Next::Continue;
        for _ in 0..50 {
            next = lockstep.frame(&mut app, &mut rec, 1.0 / 60.0, render, |_| {});
            if next != Next::Continue {
                break;
            }
        }
        assert_eq!(next, Next::Finish);
        assert_eq!(lockstep.produced(), 10);
        assert!(rec.at_limit());
        assert!(app.in_round() && app.running_game().unwrap().finished().is_none());
        rec.cancel();
    }

    /// While paused nothing is produced and no time is saved up: the frame
    /// after resuming is the next one, not a burst.
    #[test]
    fn pausing_stops_production_without_saving_up_time() {
        let mut app: App<Counter> = App::new(Settings::default(), None);
        app.handle(&key(Key::Enter));
        let mut rec = Recorder::with_script("cat > /dev/null", BYTES, 24);
        let mut lockstep = Lockstep::default();
        lockstep.frame(&mut app, &mut rec, 1.0 / 60.0, render, |_| {});
        assert_eq!(lockstep.produced(), 1);
        app.handle(&key(Key::P));
        for _ in 0..30 {
            lockstep.frame(&mut app, &mut rec, 1.0 / 60.0, render, |_| {});
        }
        assert_eq!(lockstep.produced(), 1, "nothing while paused");
        assert_eq!(app.play_time, 0.0);
        app.handle(&key(Key::P));
        lockstep.frame(&mut app, &mut rec, 0.25, render, |_| {});
        assert_eq!(
            lockstep.produced(),
            3,
            "at most two frames per display frame"
        );
        assert!(
            (app.play_time - 2.0 / 60.0).abs() < 1e-6,
            "two steps per frame"
        );
        rec.cancel();
    }
}
