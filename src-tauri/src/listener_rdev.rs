//! Global keystrokes on Windows (low-level keyboard hook) and Linux (X11 XRecord)
//! through `rdev`, on a dedicated thread.

use std::cell::RefCell;

use rdev::{EventType, Key};

use crate::keys::{OnSound, PressTracker, Sound};

pub fn start(on_sound: OnSound) {
    let spawned = std::thread::Builder::new().name("clicky-keyhook".into()).spawn(move || {
        let tracker = RefCell::new(PressTracker::<Key>::new());
        let result = rdev::listen(move |event| match event.event_type {
            EventType::KeyPress(key) => {
                // Neither platform flags auto-repeat here; held keys just re-send KeyPress.
                if tracker.borrow_mut().press(key, None) {
                    on_sound(sound_for(key));
                }
            }
            EventType::KeyRelease(key) => {
                if tracker.borrow_mut().release(key) {
                    on_sound(Sound::Topout);
                }
            }
            _ => {}
        });
        if let Err(err) = result {
            eprintln!("clicky: couldn't listen to the keyboard: {err:?}");
        }
    });
    if let Err(err) = spawned {
        eprintln!("clicky: couldn't start keyboard thread: {err}");
    }
}

fn sound_for(key: Key) -> Sound {
    match key {
        Key::Space => Sound::Thock,
        Key::Return | Key::KpReturn => Sound::Clack,
        Key::Backspace
        | Key::Delete
        | Key::ShiftLeft
        | Key::ShiftRight
        | Key::ControlLeft
        | Key::ControlRight
        | Key::Alt
        | Key::AltGr
        | Key::MetaLeft
        | Key::MetaRight
        | Key::CapsLock
        | Key::Function => Sound::Snap,
        _ => Sound::Click,
    }
}
