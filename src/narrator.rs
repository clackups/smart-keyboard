// src/narrator.rs
//
// Audio narration of keyboard-navigation selection changes.
//
// When `narrate = true` in the [output] section of config.toml, a short WAV
// file is played on every change of the navigation cursor.  Playback is
// delegated to `aplay` (ALSA utils).  All errors are silently ignored so that
// missing files or a missing `aplay` binary do not affect normal operation.
//
// Audio files are looked up in the directory given by the
// `SMART_KBD_AUDIO_PATH` environment variable, or in the `audio/` sub-
// directory of the current working directory when the variable is not set.
// Each file is named `<slug>.wav` where the slug is supplied by the caller.

use std::process::{Child, Command, Stdio};

pub struct Narrator {
    enabled:   bool,
    audio_dir: String,
    child:     Option<Child>,
}

impl Narrator {
    /// Create a narrator.  When `enabled` is `false` every call to [`play`]
    /// is a no-op.
    pub fn new(enabled: bool) -> Self {
        let audio_dir = std::env::var("SMART_KBD_AUDIO_PATH")
            .unwrap_or_else(|_| "audio".to_string());
        Narrator { enabled, audio_dir, child: None }
    }

    /// Play `<audio_dir>/<slug>.wav`.
    ///
    /// Any clip that is still playing is stopped first so navigation at high
    /// speed does not queue up a backlog of audio.
    pub fn play(&mut self, slug: &str) {
        if !self.enabled || slug.is_empty() {
            return;
        }
        // Kill and reap any previous clip so there is no zombie and the sound
        // device is freed before the new clip starts.
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;

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
}

impl Drop for Narrator {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
