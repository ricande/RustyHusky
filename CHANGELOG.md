# Changelog

All notable changes to RustyHusky are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- Prioritize recording is now the default recording mode, and old
  `"recording": true` settings migrate to it. A saved `recording_mode` is
  kept as it is.

### Added

- `Game::SOUNDS`: a game's own WAV files (8- or 16-bit PCM), played with
  `Sfx::Game(n)` and mixed into the video. The example game plays
  `assets/chime.wav` on every fifth point.
- `View::image` draws an embedded image file (`include_bytes!`) in logical
  coordinates, so it is scaled the same in the window and in the video. The
  example game draws `assets/paw.png`.
- `scripts/new-game.sh "My Game"` names a new game made from the template
  and resets its changelog and rules; `docs/NEW_GAME.md` is a step-by-step
  guide.

## [0.2.0] - 2026-10-15

### Added

- The repository is a GitHub template; the README explains how to start a
  game from it ("Use this template") and what the GPL means for it.
- CI runs clippy and the tests, including real FFmpeg recordings, on
  Windows and macOS as well as Linux. With `REQUIRE_FFMPEG` set (as in CI),
  a test that needs FFmpeg fails instead of being skipped.
- **Recording mode** in the settings, replacing recording on/off:
  - *Prioritize gameplay* (default): as before, the game runs in real time;
    the main thread now never waits for the encoder at all. If it cannot
    keep up, the recording stops with a message.
  - *Prioritize recording*: each video frame is made from exactly two
    physics steps, drawn once and queued once; the game does not advance
    until the encoder has accepted the frame. A slow computer plays slower,
    but the video is always smooth at normal speed. A slow encoder (still
    writing frames) is waited for, with a note in the window after 1 s; one
    that writes nothing for 5 s is stopped and the round goes on in real
    time. Sounds are placed by game time.
  - *Off*.
  The mode applies from the next round, and the settings screen explains
  each one. Old `settings.json` files are migrated (`"recording": true` →
  Prioritize gameplay, `false` → Off).
- "Recording stopped" in the window when a recording fails during a round.
- Regression tests (Unix) for an encoder that hangs without exiting, one
  that is slower than real time, one that keeps up, a hanging startup
  check, cancelling, shutdown and full pipes.

- `src/config.rs`: the project's name (`APP_NAME`), id (`APP_ID`) and
  environment variable prefix (`ENV_PREFIX`) in one place. Window title,
  menu title, data and video folders, file names and environment variables
  follow them.
- `Game::TEXTS`: a game brings its own translations; `i18n.rs` holds only
  the framework's. The menu subtitle (`menu.subtitle`) belongs to the game;
  the example's is "Build. Play. Record. Share.".
- Script commands `hold <key>` and `release <key>` for held keys.
- GitHub Actions CI: fmt, clippy and tests (with FFmpeg, so the video tests
  run), and a build with the minimum Rust version, now declared as
  `rust-version` in `Cargo.toml`.
- A screenshot in the README.

### Changed

- Game input is gathered until the game's next simulation step, so a press
  is delivered exactly once even in frames without a step.
- A round's first frame does not simulate, so the first video frame shows
  the state at time 0 in both modes.
- Menu sounds are kept apart from game sounds and are never recorded (the
  Play click used to land at the start of the video).
- Long button labels shrink to fit their button.
- The minimum Rust version is 1.87, not 1.82 as documented before: the
  dependency `fontdue` (via Macroquad) needs 1.87. Found by the new CI job.
- Keys are defined in one table, `ui::KEYS` (key, key code, script name),
  instead of four places.

- **License:** RustyHusky is now licensed under GPL-3.0-or-later instead of
  MIT. `LICENSE` holds the full GPLv3 text and `Cargo.toml` says
  `GPL-3.0-or-later`. RustOut's original MIT license and copyright notice are
  kept in `LICENSES/MIT-RustOut.txt` for the reused code, and
  `THIRD_PARTY_NOTICES.md` explains this, the dependencies' own licenses,
  FFmpeg, and that recorded videos are not automatically covered by the code
  license.
- README and repository description state the project's purpose: portrait
  games and gameplay videos for YouTube Shorts, TikTok and Instagram Reels.
  Videos are published by the user; there is no automatic upload.

### Fixed

- Text could turn into black rectangles for the rest of the run when the
  font atlas grew right after a render-target pass (macroquad#1006). The
  cause is in miniquad 0.4.11: deleting a texture left it in the
  texture-binding cache. RustyHusky now builds against miniquad 0.4.11 with
  that fixed (`vendor/`, `[patch.crates-io]`); see docs/UPSTREAM_FIXES.md.
  An earlier workaround (an invisible rectangle drawn last, `view::end_batch`)
  is removed: it made the failure rarer but did not prevent it.
- **A hung or slow encoder could freeze or slow the game.** If FFmpeg kept
  running but stopped reading frames, the game waited forever for room in
  the frame queue; if it encoded slower than real time, the game slowed down
  to its pace. Now no frame waits for the encoder; if the bounded queue
  (about 0.4 s, repeated pictures stored once) is full, FFmpeg is
  stopped, the round continues at full speed and the results screen shows
  "could not keep up". No frames are dropped, so a saved video is always
  complete and in sync.
- The FFmpeg check at startup has a 5 s time limit (before, a hanging
  `ffmpeg -encoders` kept the window from opening).
- Discarding a recording, starting a new round and closing the program no
  longer wait for FFmpeg (up to 60 s before); they stop it at once.
- FFmpeg's output is drained on reader threads, so a full pipe cannot block
  it.
- The documentation claimed that a hanging encoder could never lock the
  game; that was only true when finishing and saving.

## [0.1.0] - 2026-10-15

The first version of the template, built from the general parts of
[RustOut](https://github.com/ricande/RustOut) 0.1.0.

### Added

- `game::Game` trait separating a replaceable game module from the framework,
  with a placeholder example game ("Your game here", Space for points,
  Enter or a button to finish the level).
- Flow: main menu (Play, Settings, Quit) → round → results → next round or
  main menu; pause with Resume and Abort round; aborted rounds end on the
  results screen.
- Settings screen for language (English/Swedish), volume (0–100 %) and
  recording (on by default), saved in `settings.json`.
- Automatic recording of each round as a 1080 × 1920, 60 fps H.264/AAC MP4,
  kept as a temporary file until **Save video** or **Discard video** on the
  results screen; paused time and all menus are left out; status shown while
  the video is finished and saved; ffmpeg waits are limited to 60 s.
- `RUSTYHUSKY_DATA_DIR` and `RUSTYHUSKY_FFMPEG` environment variables.
- Scripts `scripts/ui_tour.txt` and `scripts/window_check.txt`, and the
  `wclick` script command for clicks in window pixels.

### Kept from RustOut

- The letterboxed 540 × 960 view and mouse conversion (`View`), start window
  sizing, the translation catalog and `t!`, the sound synthesizer, the
  ffmpeg recording pipeline with sound mixing, and script mode.

### Removed from RustOut

- All Breakout code: paddle, balls, blocks, bonuses, levels, the level
  editor and level files, and the save-video dialog (videos are now saved
  with an automatic, unique file name).

[Unreleased]: https://github.com/ricande/RustyHusky/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/ricande/RustyHusky/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ricande/RustyHusky/releases/tag/v0.1.0
