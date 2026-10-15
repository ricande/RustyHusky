# Third-party notices

RustyHusky as a whole is licensed under the GNU General Public License,
version 3 or (at your option) any later version (GPL-3.0-or-later). See
[LICENSE](LICENSE).

## RustOut

RustyHusky was created from [RustOut](https://github.com/ricande/RustOut),
which is licensed under the MIT License:

    Copyright (c) 2026 ricande

Code reused from RustOut – among others the view scaling and mouse
conversion, start window sizing, the translation catalog, the sound
synthesizer, the ffmpeg recording pipeline and script mode (see
[CHANGELOG.md](CHANGELOG.md)) – keeps that copyright notice and MIT
permission notice. The full original text is in
[LICENSES/MIT-RustOut.txt](LICENSES/MIT-RustOut.txt). The MIT License allows
the code to be included in a GPL-3.0-or-later work, provided the notice is
kept.

## Rust dependencies

RustyHusky depends on crates from crates.io, mainly
[Macroquad](https://github.com/not-fl3/macroquad), `serde` and `serde_json`,
and their own dependencies. They are not part of this repository; Cargo
downloads them when building. Each crate keeps its own license, as declared
in its `Cargo.toml` and license files – mostly `MIT OR Apache-2.0`, with some
crates under MIT, Apache-2.0, Zlib, BSD-3-Clause or Unicode-3.0 terms. None
of them is relicensed by RustyHusky. If you distribute a built binary,
include the license notices those crates require. To list them:

```bash
cargo metadata --format-version 1 | jq -r '.packages[] | "\(.name) \(.version): \(.license)"'
```

## FFmpeg

Video recording runs the `ffmpeg` program installed on the user's system as
a separate process. FFmpeg is not included in or distributed with RustyHusky
and keeps its own license (LGPL or GPL, depending on the build).

## Recorded videos

The GPL covers RustyHusky's code. Gameplay videos you record with it are not
automatically covered by the code license. Any assets that appear in a game
or video – graphics, music, sounds, fonts, logos – keep their own rights and
licenses, which you must respect when you publish a video.
