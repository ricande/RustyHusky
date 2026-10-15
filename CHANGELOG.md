# Changelog

All notable changes to RustyHusky are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- README and repository description state the project's purpose: portrait
  games and gameplay videos for YouTube Shorts, TikTok and Instagram Reels.
  Videos are published by the user; there is no automatic upload.

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
