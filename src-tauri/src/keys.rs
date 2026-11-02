use std::sync::Arc;
use std::time::{Duration, Instant};

/// The sound profiles, named to match the sample files (`click_1.wav`, `topout_2.wav`, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// Regular alphanumeric keys.
    Click,
    /// Spacebar.
    Thock,
    /// Enter / Return.
    Clack,
    /// Backspace, Delete and modifiers (Shift, Ctrl, Alt, Cmd/Win).
    Snap,
    /// Any key being released.
    Topout,
}

impl Sound {
    pub const ALL: [Sound; 5] = [Sound::Click, Sound::Thock, Sound::Clack, Sound::Snap, Sound::Topout];

    pub fn name(self) -> &'static str {
        match self {
            Sound::Click => "click",
            Sound::Thock => "thock",
            Sound::Clack => "clack",
            Sound::Snap => "snap",
            Sound::Topout => "topout",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_name(name: &str) -> Option<Sound> {
        Sound::ALL.into_iter().find(|s| s.name() == name)
    }
}

pub type OnSound = Arc<dyn Fn(Sound) + Send + Sync>;

/// Longer than any OS key-repeat interval (the initial repeat delay maxes out around 1 s).
const STALE: Duration = Duration::from_millis(1200);

/// Tracks which keys are physically held so that OS key-repeat never retriggers a
/// press sound, and so that a release only sounds for a key we saw go down.
pub struct PressTracker<K> {
    held: Vec<(K, Instant)>,
}

impl<K: PartialEq + Copy> PressTracker<K> {
    pub fn new() -> Self {
        Self { held: Vec::with_capacity(16) }
    }

    /// Records a key-down event and returns whether it is a real press.
    ///
    /// `repeat` is the platform's auto-repeat flag when it has one. A non-repeat
    /// key-down for a key we think is held means we missed its release (e.g. while a
    /// secure input field had focus), so it counts as a fresh press. Without the flag,
    /// a key-down that arrives long after the last event for that key counts as fresh.
    pub fn press(&mut self, key: K, repeat: Option<bool>) -> bool {
        let now = Instant::now();
        match self.held.iter_mut().find(|(k, _)| *k == key) {
            None => {
                self.held.push((key, now));
                true
            }
            Some(entry) => {
                let stale = now.duration_since(entry.1) > STALE;
                entry.1 = now;
                match repeat {
                    Some(is_repeat) => !is_repeat,
                    None => stale,
                }
            }
        }
    }

    /// Records a key-up event and returns whether the key was being tracked.
    pub fn release(&mut self, key: K) -> bool {
        match self.held.iter().position(|(k, _)| *k == key) {
            Some(i) => {
                self.held.swap_remove(i);
                true
            }
            None => false,
        }
    }

    pub fn clear(&mut self) {
        self.held.clear();
    }
}
