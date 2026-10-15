# Starting a new game

From an empty repository to your own game on the RustyHusky template.

## 1. Create and name it

```bash
gh repo create my-game --template ricande/RustyHusky --public --clone
cd my-game
./scripts/new-game.sh "My Game"
```

`new-game.sh` sets the name in `src/config.rs` (window and menu title,
folders, file names, environment variable prefix) and `Cargo.toml`, resets
the version to 0.1.0 and starts a new `CHANGELOG.md` and
`docs/GAME_RULES.md`. Then rewrite the README for your game; keep its
requirements (Rust version, Linux packages, FFmpeg) and its license section.

## 2. Write the game module

Copy `src/example.rs` to `src/my_game.rs`, add `mod my_game;` and set
`type ActiveGame = my_game::MyGame;` in `src/main.rs`. Implement
`game::Game` (see `src/game.rs`):

| Method | What to do there |
| --- | --- |
| `TEXTS` | Every text you show, as (key, English, Swedish). Must contain `menu.subtitle`. |
| `new(level)` | Set up a round. `level` is 1, 2, 3 … with "Next round". |
| `handle(&Input)` | Read input: `input.key(k)` for presses, `input.held(k)` for movement, `input.pointer`/`input.clicked(rect)` for the mouse. Called once per simulation step with all input since the last one. |
| `update(dt)` | Move things. `dt` is always 1/120 s. |
| `draw(&View)` | Draw through `view` (`rect`, `circle`, `line`, `text`, `text_centered`, `image` …) in logical 540 × 960 coordinates. Must not change the game. |
| `SOUNDS` | Your own WAV files (optional), see *Sounds* below. |
| `take_sounds()` | Return the sounds started since the last call. |
| `finished()` | `Some(RoundResult { score })` when the level is over. |

Then delete `src/example.rs` (and `assets/paw.png`, `assets/chime.wav`
unless you use them). The framework's tests have their own test game, and
helpers your game does not use are fine.

## 3. Rules that keep the video right

- Videos stop at 3:00 (`MAX_VIDEO_SECONDS` in `src/config.rs`), the YouTube
  Shorts limit, while the game goes on. If a whole level should fit in a
  Short, make it a little shorter than that: the first video frame is at
  time 0 and the level's last frame comes one step after its end time.

- Keep all game time in `update`; never read the clock (`get_time`). The
  video is made from your game's own time.
- Draw only through `View` and only in logical coordinates. Then the window
  and the 1080 × 1920 video show the same thing at any window size.
- Show every text with `t!("key")` and put the key in `TEXTS`; `cargo test`
  fails on a missing key.
- New keys: a variant in `ui::Key` and a row in `ui::KEYS`.
- Esc and P pause the game; don't use them for anything else.

## 4. Check it

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo run --release
```

Play a round, save the video and check it with
`ffprobe -v error -show_entries stream=width,height,r_frame_rate -of csv=p=0 <file>`
(expect `1080,1920,60/1`). AGENTS.md describes script mode for repeatable
checks with screenshots.

## Hover and the mouse

`draw` only gets the view. To show hover, keep the pointer from `handle`
(`self.pointer = input.pointer;`) and test it in `draw`, as the example game
does for its button.

## Randomness

Keep a small random generator in your game, seeded per level, rather than
the global `macroquad::rand`: the framework's tests run in parallel, and a
shared global generator makes rounds differ between runs. For example:

```rust
/// xorshift, one per game; the same seed gives the same level.
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self { Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1) }
    /// A number in [0, 1).
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}
```

Create it in `new` with `Rng::new(level as u64)` and draw numbers in
`handle`/`update`, never in `draw` (it runs more than once per frame while
recording). [Husky Bounce](https://github.com/ricande/husky-bounce) does
this.

## Images

Put image files in `assets/` and draw them with
`view.image(include_bytes!("../assets/hero.png"), x, y, w, h, WHITE)`. The
file is built into the program, loaded on first use and scaled like
everything else, so it looks the same in the window and in the video. The
example game draws `assets/paw.png` this way. Record where each asset comes
from and its license in `THIRD_PARTY_NOTICES.md`.

## Sounds

Put WAV files (8- or 16-bit PCM, mono or stereo, any sample rate) in
`assets/` and list them in your game:

```rust
const SOUNDS: &'static [&'static [u8]] = &[include_bytes!("../assets/jump.wav")];
```

Return `Sfx::Game(0)` from `take_sounds` to play the first one. It is played
at the volume setting and mixed into the video at the time it happened. The
example game plays `assets/chime.wav` on every fifth point. The framework's
own synthesized sounds (`Sfx::Blip` …) can be used too.

## Not in the template yet

Background music (long sounds work, but there is no looping or music
track), saved high scores.
