//! Glue between the frame loop and `video`: draws the running game off
//! screen at 1080 × 1920 and manages the current round's recording, from
//! the start of the round to Save or Discard on the results screen.
//!
//! Slow work – finishing the encoder and saving – runs on worker threads,
//! so the game never waits for ffmpeg; progress is reported in `App::video`.

use std::path::PathBuf;
use std::thread::JoinHandle;

use macroquad::prelude::*;

use crate::app::{App, VideoRequest, VideoStatus};
use crate::audio::Sfx;
use crate::game::Game;
use crate::t;
use crate::video::{self, Recorder, Recording, HEIGHT, WIDTH};
use crate::view::{View, BACKDROP};

/// A worker thread, tagged with the round it belongs to.
struct Job<T> {
    round: u64,
    handle: JoinHandle<T>,
}

type SaveResult = (Recording, Result<PathBuf, String>);

pub struct Capture {
    target: RenderTarget,
    camera: Camera2D,
    /// Pixels read back from the GPU are bottom-up on some platforms.
    flip: bool,
    /// Why recording can never work, if so.
    unavailable: Option<String>,
    round: u64,
    recorder: Option<Recorder>,
    /// The next captured frame is the recording's first (at time 0).
    first_frame: bool,
    finishing: Option<Job<Result<Recording, String>>>,
    /// The finished, unsaved recording of the current round.
    recording: Option<Recording>,
    saving: Option<Job<SaveResult>>,
    samples: Vec<Vec<f32>>,
}

impl Capture {
    pub fn new<G>(samples: Vec<Vec<f32>>, app: &mut App<G>) -> Self {
        video::remove_old_temp_files();
        let unavailable = video::check_ffmpeg().err();
        app.recording_unavailable = unavailable.clone();
        let target = render_target(WIDTH, HEIGHT);
        target.texture.set_filter(FilterMode::Linear);
        let mut camera =
            Camera2D::from_display_rect(Rect::new(0.0, 0.0, WIDTH as f32, HEIGHT as f32));
        camera.render_target = Some(target.clone());
        let flip = unavailable.is_none() && reads_bottom_up(&camera, &target);
        Capture {
            target,
            camera,
            flip,
            unavailable,
            round: app.round,
            recorder: None,
            first_frame: false,
            finishing: None,
            recording: None,
            saving: None,
            samples,
        }
    }

    pub fn is_recording(&self) -> bool {
        self.recorder.is_some()
    }

    /// Call once per frame after the game has been updated and before
    /// `App::end_round_if_finished`. `sounds` are this frame's effects.
    pub fn frame<G: Game>(&mut self, app: &mut App<G>, frame_time: f32, sounds: &[Sfx]) {
        self.handle_request(app);
        if app.round != self.round {
            self.new_round(app);
        }
        if self.recorder.is_some() {
            let stop = match app.running_game() {
                Some(game) => {
                    self.record(game, frame_time, sounds);
                    // The level's last picture is recorded; stop before
                    // the results are shown. A crashed or hung encoder is
                    // stopped at once; the game goes on without recording.
                    game.finished().is_some()
                        || self.recorder.as_ref().is_some_and(Recorder::failed)
                }
                // Paused frames are left out; otherwise the round was left.
                None => !app.paused(),
            };
            if stop {
                self.finish(app);
            }
        }
        self.poll_jobs(app);
    }

    fn record<G: Game>(&mut self, game: &G, frame_time: f32, sounds: &[Sfx]) {
        let Some(rec) = &mut self.recorder else {
            return;
        };
        let dt = if std::mem::take(&mut self.first_frame) {
            0.0
        } else {
            f64::from(frame_time)
        };
        let copies = rec.frames_due(dt);
        for sfx in sounds {
            rec.add_sound(sfx.index());
        }
        if copies > 0 {
            set_camera(&self.camera);
            clear_background(BACKDROP);
            crate::screens::draw_game(game, &View::fit(WIDTH as f32, HEIGHT as f32));
            set_default_camera();
            let image = self.target.texture.get_texture_data();
            rec.push_frame(image.bytes, copies);
        }
    }

    /// A new round: drop what is left of the previous one and start
    /// recording if the settings allow it.
    fn new_round<G>(&mut self, app: &mut App<G>) {
        self.round = app.round;
        if let Some(rec) = self.recorder.take() {
            rec.cancel();
        }
        if let Some(old) = self.recording.take() {
            old.discard();
        }
        app.video = if !app.settings.recording {
            VideoStatus::Off
        } else if let Some(why) = &self.unavailable {
            VideoStatus::Unavailable(why.clone())
        } else {
            match Recorder::start(self.flip) {
                Ok(rec) => {
                    self.recorder = Some(rec);
                    self.first_frame = true;
                    VideoStatus::Recording
                }
                Err(e) => VideoStatus::Unavailable(t!("video.start_failed", error = e)),
            }
        };
    }

    /// Stops recording; the encoder finishes the file on a worker thread.
    fn finish<G>(&mut self, app: &mut App<G>) {
        let Some(rec) = self.recorder.take() else {
            return;
        };
        app.video = VideoStatus::Finishing;
        self.finishing = Some(Job {
            round: self.round,
            handle: spawn_with_lang(move || rec.finish()),
        });
    }

    fn handle_request<G>(&mut self, app: &mut App<G>) {
        match app.video_request.take() {
            Some(VideoRequest::Save) => match self.recording.take() {
                Some(rec) => {
                    let samples = self.samples.clone();
                    let name = video::default_name(std::time::SystemTime::now());
                    let out = video::unique_path(&video::default_dir(), &name);
                    app.video = VideoStatus::Saving;
                    self.saving = Some(Job {
                        round: self.round,
                        handle: spawn_with_lang(move || {
                            let result = rec.save(&samples, &out).map(|()| out);
                            (rec, result)
                        }),
                    });
                }
                None => app.video = VideoStatus::Failed(t!("video.nothing").into()),
            },
            Some(VideoRequest::Discard) => {
                if let Some(rec) = self.recording.take() {
                    rec.discard();
                }
            }
            None => {}
        }
    }

    /// Picks up finished worker threads. Results for an older round than
    /// the current one are cleaned up without touching `App::video`.
    fn poll_jobs<G>(&mut self, app: &mut App<G>) {
        if let Some(job) = take_finished(&mut self.finishing) {
            let current = job.round == app.round;
            match (job.handle.join(), current) {
                (Ok(Ok(rec)), true) => {
                    self.recording = Some(rec);
                    app.video = VideoStatus::Ready { error: None };
                }
                (Ok(Ok(rec)), false) => rec.discard(),
                (Ok(Err(e)), true) => {
                    app.video = VideoStatus::Failed(t!("video.record_failed", error = e));
                }
                (Err(_), true) => app.video = VideoStatus::Failed(t!("video.crashed").into()),
                _ => {}
            }
        }
        if let Some(job) = take_finished(&mut self.saving) {
            let current = job.round == app.round;
            match (job.handle.join(), current) {
                (Ok((rec, Ok(path))), true) => {
                    rec.discard();
                    app.video = VideoStatus::Saved(path);
                }
                (Ok((rec, Err(e))), true) => {
                    // Keep the recording so the player can try again.
                    self.recording = Some(rec);
                    let error = Some(t!("video.save_failed", error = e));
                    app.video = VideoStatus::Ready { error };
                }
                (Ok((rec, _)), false) => rec.discard(),
                (Err(_), true) => app.video = VideoStatus::Failed(t!("video.crashed").into()),
                _ => {}
            }
        }
    }
}

impl Drop for Capture {
    /// Stops ffmpeg and removes temporary files when the program closes,
    /// without waiting for ffmpeg: a running save is stopped too.
    fn drop(&mut self) {
        video::shut_down();
        if let Some(rec) = self.recorder.take() {
            rec.cancel();
        }
        if let Some(rec) = self.recording.take() {
            rec.discard();
        }
        if let Some(Ok(Ok(rec))) = self.finishing.take().map(|j| j.handle.join()) {
            rec.discard();
        }
        if let Some(Ok((rec, _))) = self.saving.take().map(|j| j.handle.join()) {
            rec.discard();
        }
    }
}

fn take_finished<T>(job: &mut Option<Job<T>>) -> Option<Job<T>> {
    job.as_ref()
        .is_some_and(|j| j.handle.is_finished())
        .then(|| job.take())
        .flatten()
}

/// Starts a worker thread whose messages use the player's language.
fn spawn_with_lang<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> JoinHandle<T> {
    let lang = crate::i18n::lang();
    std::thread::spawn(move || {
        crate::i18n::set_lang(lang);
        f()
    })
}

/// Draws red in the top half and checks which end of the read-back image
/// it lands in.
fn reads_bottom_up(camera: &Camera2D, target: &RenderTarget) -> bool {
    set_camera(camera);
    clear_background(BLACK);
    draw_rectangle(0.0, 0.0, WIDTH as f32, HEIGHT as f32 / 2.0, RED);
    set_default_camera();
    let image = target.texture.get_texture_data();
    let first_row_red = image.bytes.first().is_some_and(|&r| r > 128);
    !first_row_red
}
