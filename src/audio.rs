//! Tiny synthesizer: every sound effect is generated in code as a WAV
//! buffer at startup, so the program needs no audio files. The same samples
//! are mixed into recorded videos.

use macroquad::audio::{load_sound_from_bytes, play_sound, PlaySoundParams, Sound};

pub const SAMPLE_RATE: u32 = 22_050;
/// Full volume of every effect. The volume setting scales playback; video
/// sound tracks are always mixed at this level.
pub const VOLUME: f32 = 0.6;

/// The sound effects: the framework's synthesized ones, and the game's own
/// WAV files (`Game::SOUNDS`), played with `Sfx::Game(index)`. To add a
/// synthesized one, add a variant here, to `ALL` and a recipe in `recipe`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sfx {
    /// A menu choice.
    Click,
    /// The example game's "point" sound.
    Blip,
    /// A level was completed.
    Success,
    /// The game's sound number `n` in `Game::SOUNDS` of the active game.
    Game(usize),
}

const ALL: [Sfx; 3] = [Sfx::Click, Sfx::Blip, Sfx::Success];

impl Sfx {
    /// Index into `all_samples()`.
    pub fn index(self) -> usize {
        match self {
            Sfx::Game(n) => ALL.len() + n,
            builtin => ALL.iter().position(|&s| s == builtin).unwrap_or(0),
        }
    }
}

#[derive(Clone, Copy)]
enum Wave {
    Square,
    Triangle,
}

/// One tone with a linear frequency sweep and a short attack/decay envelope.
#[derive(Clone, Copy)]
struct Note {
    wave: Wave,
    f0: f32,
    f1: f32,
    secs: f32,
    vol: f32,
}

const fn note(wave: Wave, f0: f32, f1: f32, secs: f32, vol: f32) -> Note {
    Note {
        wave,
        f0,
        f1,
        secs,
        vol,
    }
}

fn recipe(sfx: Sfx) -> Vec<Note> {
    use Wave::*;
    match sfx {
        Sfx::Game(_) => Vec::new(), // files, not recipes
        Sfx::Click => vec![note(Triangle, 520.0, 600.0, 0.04, 0.4)],
        Sfx::Blip => vec![note(Square, 420.0, 640.0, 0.08, 0.3)],
        Sfx::Success => [523.0, 659.0, 784.0, 1047.0]
            .iter()
            .map(|&f| note(Square, f, f, 0.1, 0.25))
            .collect(),
    }
}

fn synth(notes: &[Note]) -> Vec<f32> {
    let mut out = Vec::new();
    for n in notes {
        let len = (n.secs * SAMPLE_RATE as f32) as usize;
        let attack = (0.004 * SAMPLE_RATE as f32) as usize;
        let mut phase = 0.0f32;
        for i in 0..len {
            let t = i as f32 / len as f32;
            let freq = n.f0 + (n.f1 - n.f0) * t;
            phase = (phase + freq / SAMPLE_RATE as f32).fract();
            let raw = match n.wave {
                Wave::Square => {
                    if phase < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Wave::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
            };
            let env = if i < attack {
                i as f32 / attack as f32
            } else {
                (1.0 - t).powf(1.5)
            };
            out.push(raw * env * n.vol);
        }
    }
    out
}

/// Encodes mono samples in [-1, 1] as a 16-bit PCM WAV file.
pub fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    b.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    b.extend_from_slice(&2u16.to_le_bytes()); // block align
    b.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}

/// Samples of every effect, indexed by `Sfx::index`.
/// The framework's synthesized sounds followed by the active game's files.
/// A game file that cannot be read is a programming error and panics, with
/// the reason (`cargo test` checks the files of the active game).
pub fn all_samples() -> Vec<Vec<f32>> {
    let game = <crate::ActiveGame as crate::game::Game>::SOUNDS;
    ALL.into_iter()
        .map(|sfx| synth(&recipe(sfx)))
        .chain(
            game.iter().enumerate().map(|(i, wav)| {
                decode_wav(wav).unwrap_or_else(|e| panic!("Game::SOUNDS[{i}]: {e}"))
            }),
        )
        .collect()
}

/// Reads a PCM WAV file (8- or 16-bit, mono or stereo, any sample rate)
/// as mono samples in [-1, 1] at `SAMPLE_RATE`.
pub fn decode_wav(bytes: &[u8]) -> Result<Vec<f32>, String> {
    let u16_at = |i: usize| {
        bytes
            .get(i..i + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let u32_at = |i: usize| {
        bytes
            .get(i..i + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err("not a WAV file".into());
    }
    let (mut format, mut data) = (None, None);
    let mut at = 12;
    while let (Some(id), Some(len)) = (bytes.get(at..at + 4), u32_at(at + 4)) {
        let body = at + 8;
        let end = body.saturating_add(len as usize).min(bytes.len());
        match id {
            b"fmt " => format = Some(body),
            b"data" => data = Some(&bytes[body..end]),
            _ => {}
        }
        at = end + (len as usize & 1);
    }
    let f = format.ok_or("no fmt chunk")?;
    let data = data.ok_or("no data chunk")?;
    let (kind, channels, rate, bits) = (u16_at(f), u16_at(f + 2), u32_at(f + 4), u16_at(f + 14));
    let (Some(1), Some(channels @ 1..=2), Some(rate @ 1..), Some(bits @ (8 | 16))) =
        (kind, channels, rate, bits)
    else {
        return Err("only 8- or 16-bit PCM, mono or stereo, is supported".into());
    };
    let width = usize::from(bits / 8) * usize::from(channels);
    let mono: Vec<f32> = data
        .chunks_exact(width)
        .map(|frame| {
            let sample = |c: usize| match bits {
                8 => (f32::from(frame[c]) - 128.0) / 128.0,
                _ => f32::from(i16::from_le_bytes([frame[2 * c], frame[2 * c + 1]])) / 32768.0,
            };
            (0..usize::from(channels)).map(sample).sum::<f32>() / f32::from(channels)
        })
        .collect();
    // Linear resampling to the framework's rate.
    let step = rate as f64 / SAMPLE_RATE as f64;
    let len = (mono.len() as f64 / step).floor() as usize;
    Ok((0..len)
        .map(|i| {
            let pos = i as f64 * step;
            let (j, t) = (pos as usize, (pos.fract()) as f32);
            let a = mono[j];
            let b = mono.get(j + 1).copied().unwrap_or(a);
            a + (b - a) * t
        })
        .collect())
}

pub struct Sounds {
    sounds: Vec<Option<Sound>>,
    /// The raw samples of every effect, for mixing video sound tracks.
    samples: Vec<Vec<f32>>,
}

impl Sounds {
    /// Generates and loads all effects. A sound that fails to load is
    /// skipped, so the program still runs without working audio.
    pub async fn load() -> Self {
        let samples = all_samples();
        let mut sounds = Vec::new();
        for s in &samples {
            sounds.push(load_sound_from_bytes(&wav_bytes(s)).await.ok());
        }
        Sounds { sounds, samples }
    }

    pub fn samples(&self) -> &[Vec<f32>] {
        &self.samples
    }

    /// Plays an effect at `volume` percent (0–100) of full volume.
    pub fn play(&self, sfx: Sfx, volume: u8) {
        if volume == 0 {
            return;
        }
        if let Some(Some(sound)) = self.sounds.get(sfx.index()) {
            let params = PlaySoundParams {
                looped: false,
                volume: VOLUME * f32::from(volume.min(100)) / 100.0,
            };
            play_sound(sound, params);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_files_are_decoded_to_mono_at_our_rate() {
        let samples: Vec<f32> = (0..100).map(|i| i as f32 / 200.0).collect();
        let back = decode_wav(&wav_bytes(&samples)).unwrap();
        assert_eq!(back.len(), 100);
        assert!(back.iter().zip(&samples).all(|(a, b)| (a - b).abs() < 1e-3));
        // Stereo, 8 bit, twice our rate: channels averaged, length halved.
        let mut wav = wav_bytes(&[0.0; 4]);
        wav[22] = 2; // channels
        wav[24..28].copy_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
        wav[34] = 8; // bits
        wav.truncate(44);
        wav.extend([128u8, 255, 128, 255, 0, 0, 0, 0]);
        let n = (wav.len() - 44) as u32;
        wav[40..44].copy_from_slice(&n.to_le_bytes());
        let back = decode_wav(&wav).unwrap();
        assert_eq!(back.len(), 2);
        assert!((back[0] - 127.0 / 256.0).abs() < 0.01, "{back:?}");
        assert!(decode_wav(b"not a wav").is_err());
        let mut float = wav_bytes(&[0.0; 4]);
        float[20] = 3; // IEEE float
        assert!(decode_wav(&float).unwrap_err().contains("PCM"));
    }

    #[test]
    fn the_active_games_sounds_follow_the_builtin_ones() {
        let all = all_samples();
        let game = <crate::ActiveGame as crate::game::Game>::SOUNDS;
        assert_eq!(all.len(), ALL.len() + game.len());
        assert_eq!(Sfx::Game(0).index(), ALL.len());
        for i in 0..game.len() {
            assert!(!all[Sfx::Game(i).index()].is_empty(), "sound {i} is empty");
        }
    }

    #[test]
    fn wav_header_is_valid() {
        let samples = synth(&recipe(Sfx::Blip));
        let wav = wav_bytes(&samples);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 44 + samples.len() * 2);
        let riff_len = u32::from_le_bytes(wav[4..8].try_into().unwrap());
        assert_eq!(riff_len as usize, wav.len() - 8);
    }

    #[test]
    fn all_sounds_are_short_and_in_range() {
        for (i, sfx) in ALL.into_iter().enumerate() {
            assert_eq!(sfx.index(), i, "ALL must match enum order");
            let s = synth(&recipe(sfx));
            assert!(!s.is_empty());
            assert!(s.len() < SAMPLE_RATE as usize, "effects stay under 1 s");
            assert!(s.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        }
    }
}
