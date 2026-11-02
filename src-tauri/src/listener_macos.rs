//! Global keystrokes on macOS through a listen-only Quartz event tap, running on a
//! dedicated user-interactive thread. Requires Input Monitoring permission.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use crate::keys::{OnSound, PressTracker, Sound};

type TapCallback = extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(tap: u32, place: u32, options: u32, events: u64, callback: TapCallback, user_info: *mut c_void) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
    fn CGEventGetFlags(event: *mut c_void) -> u64;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopCommonModes: *const c_void;
    fn CFMachPortCreateRunLoopSource(allocator: *const c_void, port: *mut c_void, order: isize) -> *mut c_void;
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(run_loop: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopRun();
}

extern "C" {
    fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
}

const SESSION_EVENT_TAP: u32 = 1;
const HEAD_INSERT_EVENT_TAP: u32 = 0;
const LISTEN_ONLY: u32 = 1;
const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const FLAGS_CHANGED: u32 = 12;
const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
const FIELD_AUTOREPEAT: u32 = 8;
const FIELD_KEYCODE: u32 = 9;
const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
const CAPS_LOCK: i64 = 57;

struct TapState {
    tap: *mut c_void,
    tracker: PressTracker<i64>,
    on_sound: OnSound,
}

pub fn has_permission() -> bool {
    unsafe { CGPreflightListenEventAccess() }
}

/// Shows the system Input Monitoring prompt (only the first time it is asked).
pub fn request_permission() {
    unsafe {
        CGRequestListenEventAccess();
    }
}

/// Installs the tap. Returns false if macOS refuses it (no permission yet, or the
/// permission was granted while running and needs a relaunch to take effect).
pub fn start(on_sound: OnSound) -> bool {
    if !has_permission() {
        return false;
    }
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("clicky-keytap".into()).spawn(move || unsafe {
        pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0);
        let state = Box::into_raw(Box::new(TapState { tap: null_mut(), tracker: PressTracker::new(), on_sound }));
        let mask = (1u64 << KEY_DOWN) | (1u64 << KEY_UP) | (1u64 << FLAGS_CHANGED);
        let tap = CGEventTapCreate(SESSION_EVENT_TAP, HEAD_INSERT_EVENT_TAP, LISTEN_ONLY, mask, tap_callback, state.cast());
        if tap.is_null() {
            drop(Box::from_raw(state));
            let _ = ready_tx.send(false);
            return;
        }
        (*state).tap = tap;
        let source = CFMachPortCreateRunLoopSource(null(), tap, 0);
        CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
        let _ = ready_tx.send(true);
        CFRunLoopRun();
    });
    spawned.is_ok() && ready_rx.recv().unwrap_or(false)
}

extern "C" fn tap_callback(_proxy: *mut c_void, event_type: u32, event: *mut c_void, user_info: *mut c_void) -> *mut c_void {
    let state = unsafe { &mut *(user_info as *mut TapState) };
    match event_type {
        KEY_DOWN => {
            let code = unsafe { CGEventGetIntegerValueField(event, FIELD_KEYCODE) };
            let repeat = unsafe { CGEventGetIntegerValueField(event, FIELD_AUTOREPEAT) } != 0;
            if state.tracker.press(code, Some(repeat)) {
                (state.on_sound)(sound_for(code));
            }
        }
        KEY_UP => {
            let code = unsafe { CGEventGetIntegerValueField(event, FIELD_KEYCODE) };
            if state.tracker.release(code) {
                (state.on_sound)(Sound::Topout);
            }
        }
        FLAGS_CHANGED => {
            let code = unsafe { CGEventGetIntegerValueField(event, FIELD_KEYCODE) };
            if code == CAPS_LOCK {
                // Caps Lock only reports its toggle, never a separate press and release.
                (state.on_sound)(Sound::Snap);
            } else if let Some(down) = modifier_down(code, unsafe { CGEventGetFlags(event) }) {
                if down {
                    if state.tracker.press(code, None) {
                        (state.on_sound)(Sound::Snap);
                    }
                } else if state.tracker.release(code) {
                    (state.on_sound)(Sound::Topout);
                }
            }
        }
        TAP_DISABLED_BY_TIMEOUT | TAP_DISABLED_BY_USER_INPUT => {
            // Releases may have happened while we weren't listening.
            state.tracker.clear();
            unsafe { CGEventTapEnable(state.tap, true) };
        }
        _ => {}
    }
    event
}

/// macOS virtual key codes (Carbon `kVK_*`).
fn sound_for(code: i64) -> Sound {
    match code {
        49 => Sound::Thock,           // Space
        36 | 76 => Sound::Clack,      // Return, keypad Enter
        51 | 117 => Sound::Snap,      // Delete (backspace), Forward Delete
        54..=63 => Sound::Snap,       // Command, Shift, Caps Lock, Option, Control, Fn
        _ => Sound::Click,
    }
}

/// `flagsChanged` fires on both press and release. The device-specific (left vs
/// right) flag bits tell whether this particular modifier is now down.
fn modifier_down(code: i64, flags: u64) -> Option<bool> {
    let mask: u64 = match code {
        59 => 0x0000_0001, // left Control
        56 => 0x0000_0002, // left Shift
        60 => 0x0000_0004, // right Shift
        55 => 0x0000_0008, // left Command
        54 => 0x0000_0010, // right Command
        58 => 0x0000_0020, // left Option
        61 => 0x0000_0040, // right Option
        62 => 0x0000_2000, // right Control
        63 => 0x0080_0000, // Fn
        _ => return None,
    };
    Some(flags & mask != 0)
}
