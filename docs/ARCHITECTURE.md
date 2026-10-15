# Architecture

RustyHusky is a single binary crate built on [Macroquad](https://macroquad.rs).
This document explains how the code is organised, how a frame flows through
it and how recording works. For the current game's behaviour, see
[GAME_RULES.md](GAME_RULES.md).

## Design principles

1. **Framework and game are separate.** The framework owns menus, settings,
   translations, scaling, the round's life cycle and recording. The game is
   one module behind the `game::Game` trait and knows nothing about them.
2. **Logic does not touch the window.** The flow (`app.rs`), settings,
   translations, input and video bookkeeping are plain Rust working on
   abstract input, tested with `cargo test` without a window, GPU or sound card.
3. **One coordinate system.** Everything uses the logical 540 × 960 field.
   Only `view::View` knows about real pixels.
4. **Fixed time step.** Game logic advances in steps of `FIXED_DT` = 1/120 s,
   so behaviour does not depend on the frame rate.
5. **No files at run time.** The framework draws with shapes and text and
   synthesizes its sounds; a game's images and WAV files in `assets/` are
   embedded with `include_bytes!` (`View::image`, `Game::SOUNDS`).

## Modules

| Module        | Window-free | Responsibility |
| ------------- | :---------: | -------------- |
| `main.rs`     |   | Window setup, the frame loop, Macroquad input → `ui::Input`, `ActiveGame`. |
| `config.rs`   | ✓ | The project's name, id and environment variable prefix. |
| `game.rs`     | ✓ | The `Game` trait (with the game's `TEXTS` and `SOUNDS`), `RoundResult`, `FIXED_DT`. |
| `example.rs`  | ✓¹ | The placeholder game ("Your game here", an image, its own sound, hover). Replace it. |
| `app.rs`      | ✓ | Screens (`Menu`, `Settings`, `Playing`, `Results`), transitions, button layouts, `VideoStatus`. |
| `screens.rs`  |   | Draws the screens, the pause menu and the REC marker with the recording's length (window only). |
| `view.rs`     | ✓² | `View` (logical ↔ pixels), colours, drawing helpers including embedded images (`image`), buttons. |
| `ui.rs`       | ✓ | `Input`, `Key` and the `KEYS` table (key code, script name), `Button`, button layout and keyboard/mouse choice. |
| `settings.rs` | ✓ | `settings.json`: language, volume, recording mode (with migration of the old on/off); the data folder. |
| `i18n.rs`     | ✓ | The framework's translation catalog, lookup in it and the game's `TEXTS`, the `t!` macro. |
| `audio.rs`    |   | The synthesizer and the WAV decoder for the game's sounds (window-free), and playback with the volume setting. |
| `video.rs`    | ✓³ | FFmpeg process, frame clock, sound mixing, saving, file names. |
| `capture.rs`  |   | Renders video frames off screen and drives a round's recording in either recording mode. |
| `pacing.rs`   | ✓ | `Lockstep`: the "Prioritize recording" timing, two physics steps per produced video frame. |
| `screen.rs`   | ✓ | Picks the start window size from the screen's work area. |
| `script.rs`   | ✓ | Script mode: text scripts as input, for repeatable UI checks. |

¹ except `draw`. ² `View`'s mapping is window-free; its drawing helpers call
Macroquad. ³ needs `ffmpeg` at run time; its tests skip when it is missing.

## The frame loop

`main.rs` runs this once per displayed frame:

1. **View.** `View::fit(screen_width, screen_height)`: the largest 9:16 area
   that fits the window, centred (letterboxing).
2. **Input.** Pressed and held keys and the mouse. The mouse position is
   mapped to logical coordinates; in the letterbox bars it is `None`, so
   clicks there do nothing.
3. **Handle.** `App::handle` runs the current screen: menu choices, settings,
   pause (Esc/P). While the round runs, the input is *gathered* in
   `App::game_input` (`Input::merge`) rather than handed to the game: the
   game gets it at its next simulation step, so a press is never lost or
   repeated, however many frames pass before that step.
4. **Recording start.** `Capture::begin_frame` takes Save/Discard requests
   and, when a round has started, starts its recording in the mode set in
   the settings. It returns which timing this frame uses:
   - **Real time** (Prioritize gameplay, Off, or no recording): the frame
     time (capped at 0.25 s) is added to an accumulator, and
     `App::advance(steps)` gives the game its input once and runs `steps`
     fixed steps (none in a new round's first frame, which is video time 0).
     The game's sounds are played and `Capture::gameplay_frame` records the
     frame.
   - **Video frames** (Prioritize recording): `Capture::lockstep_frame`
     advances the game itself, see below.
5. **Sounds.** The menus' sounds (`App::take_ui_sounds`) are played; they
   are never recorded.
6. **End of level.** `App::end_round_if_finished` switches to the results
   once `Game::finished` returns a result – *after* the last frame was
   recorded, and in Prioritize recording only once that frame has been
   accepted by the encoder (`Capture::holding`).
7. **Draw.** `screens::draw` draws the current screen in the window.

Closing the window or choosing Quit ends the loop normally, so `Capture` is
dropped and cleans up.

## Coordinates

- The field is 540 × 960 logical pixels, origin top left, y downwards.
- `View` holds a scale and an offset. `View::to_field` converts a window
  position to logical coordinates and `View::pointer` also rejects positions
  in the bars. Drawing helpers (`rect`, `circle`, `line`, `text` …) take
  logical coordinates.
- The window view is `View::fit(window)`; the video uses
  `View::fit(1080, 1920)`, i.e. scale 2 with no bars. Drawing code is shared,
  so the video shows exactly the game view.
- Buttons are defined once (`app::menu_buttons`, `settings_buttons`,
  `pause_buttons`, `results_buttons`, `example::FINISH_BUTTON`) and used by
  both input and drawing.

## Time step

`FIXED_DT` is 1/120 s. Games keep their own clock in `update` (the
example's moving dot does), never `get_time()`, so the recording shows what
was played. Script mode uses a fixed frame time of 1/60 s, so runs are
repeatable. Where the steps come from depends on the recording mode:

- **Real time** (Prioritize gameplay, Off): a frame runs
  `floor(accumulated / FIXED_DT)` steps, keeping the remainder.
- **Video frames** (Prioritize recording, `pacing::Lockstep`): every video
  frame k is produced by `App::advance(2)` (except frame 0, which shows the
  state before any step), drawn once and offered to the encoder once. Game
  time is therefore exactly k/60 s at frame k. Production is limited by real
  time (at most two frames per displayed frame, no time saved up beyond
  that), so a fast machine plays at normal speed; when the encoder's queue
  is full the picture waits in `Lockstep` and the game does not advance
  until it is accepted. Nothing catches up afterwards.

In both, the game's input waits in `App::game_input` until the next step,
and a paused game advances neither game time, video time nor sound.

## Screens and the round life cycle

```
Menu ──Play──► Playing{level 1} ──finished──► Results{level, Some(result)}
 ▲ │              │  ▲                          │        │
 │ Settings       Esc/P  Resume                 │ Next round → Playing{level+1}
 │ │              ▼  │                          │
 │ ▼           Playing{paused} ──Abort──► Results{level, None}
 └─Back/Esc───────────────────── Main menu ◄────┘
```

`App::round` grows each time a round starts; `Capture` uses it to start a new
recording. `App::play_time` counts unpaused game time for the results.

## Recording flow

`App` holds `video: VideoStatus` (what the results screen shows) and
`video_request` (the player's Save/Discard). `Capture` reads and updates
them, so `app.rs` stays window-free.

1. **Start.** On a new `App::round`, `Capture` drops anything left from the
   previous round. Unless the recording mode is Off, and if FFmpeg was found at
   startup (`video::check_ffmpeg`: the program answers within 5 s and lists
   libx264), it
   starts a `video::Recorder`, plus a `Lockstep` in Prioritize recording;
   otherwise the status is `Off` or `Unavailable(reason)`. The mode is read
   here, so a change applies from the next round.
2. **Frames.** For each frame where `App::running_game` is `Some` (playing,
   not paused), the game is drawn with `screens::draw_game` into a
   1080 × 1920 render target, read back as RGBA and queued (a bounded queue
   of `QUEUE_PICTURES` = 24 pictures, each with the number of video frames
   that show it, so repeats cost no memory) to a writer thread that pipes
   every frame to
   `ffmpeg -f rawvideo -pix_fmt rgba -s 1080x1920 -framerate 60 -i -` →
   `libx264 -preset veryfast -crf 20 -pix_fmt yuv420p -r 60 -an` into a
   temporary `rustyhusky-rec-<pid>-<n>.mp4` in the system temp folder.
   `capture.rs` detects at startup whether the GPU returns rows bottom-up and
   adds `-vf vflip` if so.
3. **Timing.** In Prioritize gameplay, `video::FrameClock` turns frame times
   into an exact 60 fps frame count: the first frame is at time 0; a picture
   is repeated when the display is slower than 60 Hz and frames are skipped
   when it is faster; each sound effect is stored with its clock time. In
   Prioritize recording, every produced picture is exactly one frame and
   each sound gets the time of the frame in which it happened (k/60 s).
   In both, paused frames are not captured and advance nothing, and the
   video's length is the number of frames queued / 60 s
   (`Recorder::frames`), so sound and picture end together.
   `Recorder` takes at most `config::MAX_VIDEO_SECONDS` × 60 frames
   (10 800 for 3:00); repeated pictures are cut to fit. At the limit the
   recording is finished like at the end of a level, but the round goes on
   in real time and `App::video_cut_at_limit` lets the window and the
   results say so.
4. **Stop.** On the frame the game reports `finished`, that frame is recorded
   and the recorder is closed; for an aborted round the recorder is closed
   on the frame the round is left. Closing runs on a worker thread
   (`VideoStatus::Finishing`): ffmpeg's input is closed and it finishes the
   file. The results screen never reaches the video because it is never
   drawn into the render target.
5. **Ready.** The finished temporary file becomes `Ready`; the results screen
   shows Save video / Discard video. Next round / Main menu are disabled
   while the status is busy (recording, finishing, saving).
6. **Save.** A worker thread mixes the sound track (`video::mix`), writes it
   as a WAV file and runs ffmpeg to copy the video stream and add the sound
   as stereo AAC, into `<name>.partial.mp4` next to the target, then renames
   it. A failed save keeps the recording so it can be retried. After success
   the temporary file is deleted (`Saved(path)`).
7. **Discard.** Discard video, leaving the results with a ready video,
   starting a new round and closing the program all delete the temporary
   file. Worker results for an older round are cleaned up silently.

### Failures and time limits

FFmpeg may be missing, crash, or hang without exiting. Every case ends in a
translated `VideoStatus` message and the game keeps running:

- **Prioritize gameplay: encoder hung or too slow.** The main thread must
  never wait, the timeline must stay exact and memory must stay bounded; all
  three hold only while the encoder keeps up. So `Recorder::push_frame`
  offers the picture once (`try_send`) to the queue (about 0.4 s of video).
  If it is full, the encoder has fallen behind: it is killed, which also
  ends the writer thread, and the recording fails with "could not keep up".
  Frames are never dropped, because the video would then be shorter than its
  sound track.
- **Prioritize recording: slow versus hung.** A full queue is normal here;
  the picture waits in `Lockstep` and is offered again every displayed
  frame, never blocking. The writer thread counts the frames it has passed
  to ffmpeg (`Recorder::written`): while that count grows the encoder is
  merely slow and the game keeps waiting (the window says so after 1 s);
  if it does not grow for `pacing::STALL_TIMEOUT` (5 s) the encoder is
  considered hung, killed, and the round goes on in real time. An abort
  drops the waiting picture at once.
- **Crashed encoder.** The queue disconnects; the next frame notices it.
- In every case `Capture` closes the recorder at once; the window shows
  "Recording stopped" instead of the REC marker and the results screen shows
  the reason.
- **Other waits** go through `video::Process`, which drains stdout and stderr
  on reader threads (a full pipe can never block ffmpeg) and kills the
  process after a time limit: 5 s for the startup check, 60 s for finishing
  and saving (on worker threads).
- **Closing the program.** `Capture::drop` calls `video::shut_down`, which
  makes every running wait kill its ffmpeg at once; discarding a recorder
  (`Recorder::cancel`) also kills instead of waiting. Threads stuck on a pipe
  are given a 1 s grace and then left behind rather than joined.
- Tests (Unix; the slow ones need GNU `dd`, so Linux) use fake encoders:
  `sh -c "exec sleep 30"` never reads, a `dd`/`sleep` loop reads slowly but
  steadily, and `cat` keeps up. In `video.rs` they check Prioritize gameplay
  (no frame blocks; slow and hung encoders are stopped and killed; nothing
  is left behind). In `pacing.rs` a test game writes its step and press
  counts into every picture, and the tests check that the fast and the slow
  encoder give byte-identical videos with exactly two steps per frame, the
  same sound times, presses counted once (also one made while a picture
  waits), no frame produced or repeated during a pause, a hung encoder
  detected without blocking, and an immediate abort.

### Text and the patched miniquad

RustyHusky builds against a vendored miniquad 0.4.11 with one fix
(`vendor/`, `[patch.crates-io]` in `Cargo.toml`). Without it, text could turn
into black rectangles for the rest of the run when macroquad's font atlas
grew right after a render-target pass (macroquad#1006); the recording tour
hit it. Details, verification and how to drop the copy again are in
[UPSTREAM_FIXES.md](UPSTREAM_FIXES.md).

## Translations

- Texts are `(key, English, Swedish)`. The framework's are in `CATALOG` in
  `i18n.rs`; the game's are in `Game::TEXTS` of `ActiveGame`, so a new game
  brings its own texts without editing `i18n.rs`. Lookup searches both; a
  key must be unique across them. The game must define `menu.subtitle`.
  The menu title is `config::APP_NAME`, a name that is not translated.
- `t!("key")` returns the text in the current language; `t!("key", name = v)`
  fills `{name}` placeholders.
- The language is thread-local. `main.rs` sets it from the settings (or
  `RUSTYHUSKY_LANG`); worker threads in `capture.rs` copy it when they start.
- Tests check that every `t!` key used in the source exists, that keys are
  unique and that both languages have the same placeholders.
- To add a language: add a `Lang` variant (with `code`, `next`), a column to
  `CATALOG`, to every game's `TEXTS` and the match in `tr_in`.

## Persistence

| What             | Where                                             | Module        |
| ---------------- | ------------------------------------------------- | ------------- |
| Settings         | `<data folder>/settings.json`                     | `settings.rs` |
| Saved videos     | `~/Videos/RustyHusky` (see README)                | `video.rs`    |
| Recording temp   | system temp folder, `rustyhusky-rec-*`            | `video.rs`    |

Settings are written via a temporary file and a rename. Unknown or missing
fields get defaults.

## Testing

- `cargo test` runs the flow (menu → round → results → next round / menu,
  pause, abort, Save/Discard requests, busy states), settings and their
  migration, translations, the view mapping, buttons, scripts, sounds and WAV
  decoding, the "Prioritize recording" pacing with fake encoders, and the
  real video pipeline (1080 × 1920, 60 fps, sound, failure cases) when
  ffmpeg is installed. With `REQUIRE_FFMPEG` set (as in CI on Linux, Windows
  and macOS) a missing ffmpeg fails those tests instead of skipping them.
- Drawing and `capture.rs` are checked by running the real binary in script
  mode (see [AGENTS.md](../AGENTS.md)).
