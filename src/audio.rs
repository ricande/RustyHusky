//! Tiny synthesizer: every sound effect is generated in code as a WAV
//! buffer at startup, so the program needs no audio files. The same samples
//! are mixed into recorded videos.

use macroquad::audio::{load_sound_from_bytes, play_sound, PlaySoundParams, Sound};

pub const SAMPLE_RATE: u32 = 22_050;
/// Full volume of every effect. The volume setting scales playback; video
/// sound tracks are always mixed at this level.
pub const VOLUME: f32 = 0.6;

/// The sound effects. To add one, add a variant here, to `ALL` and a
/// recipe in `recipe`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sfx {
    /// A menu choice.
    Click,
    /// The example game's "point" sound.
    Blip,
    /// A level was completed.
    Success,
}

const ALL: [Sfx; 3] = [Sfx::Click, Sfx::Blip, Sfx::Success];

impl Sfx {
    /// Index into `all_samples()`.
    pub fn index(self) -> usize {
        self as usize
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
pub fn all_samples() -> Vec<Vec<f32>> {
    ALL.into_iter().map(|sfx| synth(&recipe(sfx))).collect()
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
