# Upstream fixes

Fixes RustyHusky carries for its dependencies until they are released
upstream. Each one is a patch applied to an otherwise unchanged copy of the
published crate.

## miniquad 0.4.11: deleted textures stay in the binding cache

| | |
| --- | --- |
| Status | Fix proposed upstream, not merged or released yet |
| Issue | [not-fl3/macroquad#1006](https://github.com/not-fl3/macroquad/issues/1006) – "Text rendering corrupted - rendering as black rects" ([our comment](https://github.com/not-fl3/macroquad/issues/1006#issuecomment-6199729461)) |
| Reproduction | [ricande/miniquad-atlas-repro](https://github.com/ricande/miniquad-atlas-repro) (commits [c4e611f](https://github.com/ricande/miniquad-atlas-repro/commit/c4e611ff7fcbad2e1feef762cd47dba7a7d44b41), [de7a4f9](https://github.com/ricande/miniquad-atlas-repro/commit/de7a4f9)) |
| Pull request | [not-fl3/miniquad#691](https://github.com/not-fl3/miniquad/pull/691), commit [fbc5972](https://github.com/ricande/miniquad/commit/fbc59729b24dd6c3c4052a729431fa54ffe42b0d) |
| Patch | [`vendor/patches/miniquad-0.4.11-forget-deleted-texture.patch`](../vendor/patches/miniquad-0.4.11-forget-deleted-texture.patch) |
| Vendored copy | `vendor/miniquad-0.4.11/` = crates.io miniquad 0.4.11 (`.crate` SHA-256 `dcbaea97…1f59e65`, the checksum in `Cargo.lock` before the patch; git commit [4f13d4a](https://github.com/not-fl3/miniquad/commit/4f13d4a70caf6a8cbf0187a95461658e7812b566)) + the patch |
| Used through | `[patch.crates-io]` in `Cargo.toml` |

### Symptom

From some point in a run, all text is drawn as opaque black rectangles, in
the window and in recorded videos, whatever its colour, until the program
is restarted. In RustyHusky the UI tour in the "Prioritize recording" mode
hit it every time, from the second video frame on.

### Reproduction

The [reproduction repository](https://github.com/ricande/miniquad-atlas-repro)
holds a program of about 100 lines without any RustyHusky code: each frame
it draws text into a render target and reads it back, then draws text into
the window, with font sizes that keep filling macroquad's font atlas.
`./scripts/run.sh 10` counts failing runs; `./scripts/with-fix.sh 10` does
the same against miniquad with the patch below.

### Root cause

When macroquad's font atlas is full, `Atlas::texture` deletes its texture
and creates a bigger one. miniquad's `delete_texture` deletes the GL texture
but leaves it in its texture-binding cache (`GlCache`); the call that would
clear it, `//self.cache.clear_texture_bindings();`, has been commented out
since miniquad commit
[65f1da6](https://github.com/not-fl3/miniquad/commit/65f1da6), four days
after it was added as a fix in
[f08f001](https://github.com/not-fl3/miniquad/commit/f08f001). GL rebinds
the deleted texture's units to 0, but the cache still says the texture is
bound. When the new atlas texture gets the same GL name – as it did every
time on the test machine – `bind_texture` thinks it is already bound and
skips `glBindTexture`, the new atlas's pixels are uploaded elsewhere, and
the empty texture samples as opaque black.

This only happens if the atlas is the cached binding at that moment, which
it is right after a flush that ended with text, such as the switch back
from RustyHusky's video render target. miniquad clears the cache at every
frame's end (`commit_frame`), so growth before the first flush of a frame is
harmless. That is why the failure looks random.

### Patch

After `glDeleteTextures`, `delete_texture` calls a new
`GlCache::forget_texture`, which resets every cache entry that records the
deleted texture as bound, and the stored binding used by
`store_texture_binding`/`restore_texture_binding`, to 0. It makes no GL calls
– GL has already rebound those units to 0 – so other textures' bindings are
untouched, unlike restoring `clear_texture_bindings()`, which rebinds every
unit. 2 files, 20 lines added, 2 removed; identical to the pull request.

An earlier workaround in RustyHusky, ending every drawing batch with an
invisible rectangle (`view::end_batch`), was removed: in the reproduction it
still failed in 1 of 25 runs, for a reason that was not found.

### Verification

All on one machine: Linux 7.0, Mesa 26.0.8, Intel UHD Graphics 630
(OpenGL 4.6), macroquad 0.4.16, miniquad 0.4.11.

- Reproduction, unchanged miniquad: black text in 6 of 10 runs.
- Reproduction with the patch: 0 of 10 runs, and 0 of 12 runs with logging
  added to macroquad's `Atlas::texture`; 3 of those 12 deleted the atlas
  while it was bound, the condition that failed every time without the
  patch (5 of 5 such runs in an earlier logged batch).
- RustyHusky with the vendored, patched miniquad and without the workaround:
  the UI tour in Prioritize recording, which failed every time before, shows
  correct text in the window and in all video frames.

This shows that the patch fixes the failure reproduced here. Whether it
fixes every report in macroquad#1006 (other GPUs and systems) has not been
verified.

The vendored copy is checked by `scripts/check-vendored-miniquad.sh` (also
run in CI): it downloads miniquad 0.4.11 from crates.io, checks its SHA-256,
applies `vendor/patches/*.patch` and requires the result to equal
`vendor/miniquad-0.4.11` exactly.

Building prints one or two warnings from miniquad's own code, depending on
the platform (for example `set_or_replace_display` is never used). They are
unchanged upstream code and are not fixed here.

### Removing the copy

Once a miniquad release contains the fix (or an equivalent one):

1. Check that release with the reproduction: point its `Cargo.toml` at the
   new version (`miniquad = "=0.4.x"`), run `./scripts/run.sh 10` and expect
   no failures, and confirm that the release's `delete_texture` no longer
   leaves deleted textures in `GlCache`.
2. In RustyHusky, delete the `[patch.crates-io]` section from `Cargo.toml`,
   `vendor/`, `scripts/check-vendored-miniquad.sh` and its CI step.
3. `cargo update -p miniquad --precise 0.4.x` (and `-p macroquad` if a newer
   macroquad is needed for it), then `cargo build`.
4. Run `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`
   and the UI tour in Prioritize recording (see AGENTS.md); check the text in
   the window and in the saved video.
5. Remove this section (or mark it resolved with the release) and note the
   change in CHANGELOG.md and THIRD_PARTY_NOTICES.md.

If the fix is merged but not released, the same steps work with a git
dependency on the merge commit instead of a version, until a release
exists.

### Credits

The investigation, reproduction and patch were made with Claude (Anthropic)
working for the maintainer. The upstream issue comment, reproduction
repository and pull request are published by the maintainer, ricande.
