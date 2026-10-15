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
`docs/GAME_RULES.md`. Then rewrite the README for your game.

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
| `draw(&View)` | Draw through `view` (`rect`, `circle`, `line`, `text`, `text_centered` …) in logical 540 × 960 coordinates. Must not change the game. |
| `take_sounds()` | Return the sounds started since the last call. |
| `finished()` | `Some(RoundResult { score })` when the level is over. |

Then delete `src/example.rs`.

## 3. Rules that keep the video right

- Keep all game time in `update`; never read the clock (`get_time`). The
  video is made from your game's own time.
- Draw only through `View` and only in logical coordinates. Then the window
  and the 1080 × 1920 video show the same thing at any window size.
- Show every text with `t!("key")` and put the key in `TEXTS`; `cargo test`
  fails on a missing key.
- New keys: a variant in `ui::Key` and a row in `ui::KEYS`. New sounds: a
  variant in `audio::Sfx`, `ALL` and a recipe in `recipe`.
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

## Not in the template yet

Drawing images, sound files and music, saved high scores. Images can be
drawn with Macroquad directly, but they will not be scaled into the video
correctly until `View` gets a helper for them.
