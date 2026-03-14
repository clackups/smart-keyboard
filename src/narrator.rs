// src/narrator.rs
//
// Audio feedback for keyboard-navigation selection changes.
//
// The mode is configured via `audio` in the [output] section of config.toml:
//   "none"       - silent (default)
//   "narrate"    - play a WAV clip naming each focused button; clips are loaded
//                  from the directory given by SMART_KBD_AUDIO_PATH (env var)
//                  or from the `audio/` sub-directory of the current working
//                  directory.  Playback is delegated to `aplay` (ALSA utils).
//   "tone"       - play a short synthesised musical tone whose pitch is
//                  determined by the key category (see `play`).  A PCM WAV is
//                  generated in memory and fed to `aplay` via its stdin pipe.
//   "tone_hint"  - like "tone" but all letter and punctuation keys are silent
//                  except for F and J (the home-row bump keys).  The caller
//                  passes 0.0 for silent keys; this module plays a tone for
//                  any tone_hz > 0, exactly as in "tone" mode.
//
// All errors are silently ignored so that a missing `aplay` binary or missing
// audio files do not affect normal keyboard operation.

use std::io::Write;
use std::process::{Child, Command, Stdio};

use crate::config::AudioMode;

pub struct Narrator {
    mode:      AudioMode,
    audio_dir: String,
    child:     Option<Child>,
}

impl Narrator {
    /// Create a narrator.  When `mode` is [`AudioMode::None`] every call to
    /// [`play`] is a no-op.
    pub fn new(mode: AudioMode) -> Self {
        let audio_dir = std::env::var("SMART_KBD_AUDIO_PATH")
            .unwrap_or_else(|_| "audio".to_string());
        Narrator { mode, audio_dir, child: None }
    }

    /// Like [`play`], but in `Narrate` mode tries `slug` first and falls back
    /// to `fallback_slug` if `<audio_dir>/<slug>.wav` does not exist on disk.
    ///
    /// This is used to implement shift-aware narration: the caller passes the
    /// shifted slug as `slug` and the unshifted slug as `fallback_slug`.  For
    /// layouts that do not have a shifted audio clip the fallback (unshifted)
    /// clip is used automatically.  In `Tone`/`ToneHint`/`None` modes the
    /// fallback is ignored and the call is identical to [`play`].
    pub fn play_with_fallback(&mut self, slug: &str, fallback_slug: &str, tone_hz: f32) {
        if let AudioMode::Narrate = self.mode {
            let path = format!("{}/{}.wav", self.audio_dir, slug);
            let effective = if !slug.is_empty() && std::path::Path::new(&path).exists() {
                slug
            } else {
                fallback_slug
            };
            self.play(effective, tone_hz);
        } else {
            self.play(slug, tone_hz);
        }
    }

    /// Provide audio feedback for the currently focused element.
    ///
    /// * In `Narrate` mode, `slug` is used to locate `<audio_dir>/<slug>.wav`
    ///   which is played via `aplay`.  An empty `slug` is a no-op.
    /// * In `Tone` and `ToneHint` modes, `tone_hz` is the frequency (Hz) of
    ///   the sine-wave tone to synthesise and play.  A value <= 0 is a no-op.
    /// * In `None` mode the call is always a no-op.
    ///
    /// Any clip/tone that is still playing is stopped first so that rapid
    /// navigation does not queue up a backlog of audio.
    pub fn play(&mut self, slug: &str, tone_hz: f32) {
        match self.mode {
            AudioMode::None => {}
            AudioMode::Narrate => {
                if slug.is_empty() { return; }
                self.kill_current();
                let path = format!("{}/{}.wav", self.audio_dir, slug);
                if let Ok(child) = Command::new("aplay")
                    .arg("-q")
                    .arg(&path)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    self.child = Some(child);
                }
            }
            AudioMode::Tone | AudioMode::ToneHint => {
                if tone_hz <= 0.0 { return; }
                self.kill_current();
                // 100 ms gives ~5-6 sine-wave cycles even at the lowest tone
                // frequency (F1 ~= 55 Hz), which is enough for clear pitch
                // recognition, while the exponential decay makes it feel shorter.
                const TONE_DURATION_MS: u32 = 100;
                let wav = generate_tone_wav(tone_hz, TONE_DURATION_MS);
                if let Ok(mut child) = Command::new("aplay")
                    .arg("-q")
                    .arg("-")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(&wav);
                    }
                    self.child = Some(child);
                }
            }
        }
    }

    fn kill_current(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
    }
}

impl Drop for Narrator {
    fn drop(&mut self) {
        self.kill_current();
    }
}

// =============================================================================
// Tone synthesis
// =============================================================================

/// Generate a mono 16-bit PCM WAV file containing a sine wave at `freq_hz`
/// for `duration_ms` milliseconds.
///
/// The amplitude follows an exponential decay envelope so the tone sounds
/// like a soft chime rather than an abrupt click when it ends.
/// Sample rate: 22 050 Hz.  Format: PCM 16-bit LE, 1 channel.
fn generate_tone_wav(freq_hz: f32, duration_ms: u32) -> Vec<u8> {
    const SAMPLE_RATE: u32 = 22_050;
    const AMPLITUDE:   f32 = 20_000.0; // well below i16::MAX = 32_767

    let num_samples = (SAMPLE_RATE * duration_ms / 1000) as usize;
    let data_bytes  = num_samples * 2; // 16-bit = 2 bytes per sample

    let mut wav = Vec::with_capacity(44 + data_bytes);

    // RIFF / WAVE header
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&((36 + data_bytes) as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");

    // fmt  chunk (16 bytes, PCM = format type 1)
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());               // PCM
    wav.extend_from_slice(&1u16.to_le_bytes());               // mono
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());  // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes());               // block align
    wav.extend_from_slice(&16u16.to_le_bytes());              // bits/sample

    // data chunk
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_bytes as u32).to_le_bytes());

    // PCM samples: sine wave with exponential decay
    let duration_s = duration_ms as f32 / 1000.0;
    for i in 0..num_samples {
        let t        = i as f32 / SAMPLE_RATE as f32;
        let envelope = (-6.0 * t / duration_s).exp();
        let sample   = (AMPLITUDE * envelope
                        * (2.0 * std::f32::consts::PI * freq_hz * t).sin()) as i16;
        wav.extend_from_slice(&sample.to_le_bytes());
    }

    wav
}

