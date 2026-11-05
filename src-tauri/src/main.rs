#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod keys;
mod settings;

#[cfg(target_os = "macos")]
mod listener_macos;
#[cfg(not(target_os = "macos"))]
mod listener_rdev;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent, Wry};

use audio::AudioHandle;
use keys::{OnSound, Sound};
use settings::Settings;

// A monochrome template image on macOS (tinted by the menu bar); the color logo elsewhere.
#[cfg(target_os = "macos")]
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray.png");
#[cfg(not(target_os = "macos"))]
const TRAY_ICON: &[u8] = include_bytes!("../icons/64x64.png");

const PERMISSION_GRANTED: u8 = 0;
const PERMISSION_WAITING: u8 = 1;
const PERMISSION_RELAUNCH: u8 = 2;

struct AppState {
    audio: Arc<AudioHandle>,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    sounds_dir: PathBuf,
    permission: AtomicU8,
    mute_item: Mutex<Option<CheckMenuItem<Wry>>>,
    profile_items: Mutex<Vec<(&'static str, CheckMenuItem<Wry>)>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UiState {
    volume: f32,
    muted: bool,
    close_to_tray: bool,
    start_minimized: bool,
    profile: String,
    output_device: Option<String>,
    permission: &'static str,
    platform: &'static str,
    version: String,
    sounds_dir: String,
}

fn ui_state(app: &AppHandle) -> UiState {
    let state = app.state::<AppState>();
    let s = state.settings.lock().unwrap().clone();
    UiState {
        volume: s.volume,
        muted: s.muted,
        close_to_tray: s.close_to_tray,
        start_minimized: s.start_minimized,
        profile: s.profile,
        output_device: s.output_device,
        permission: match state.permission.load(Ordering::Relaxed) {
            PERMISSION_GRANTED => "granted",
            PERMISSION_RELAUNCH => "relaunch",
            _ => "waiting",
        },
        platform: std::env::consts::OS,
        version: app.package_info().version.to_string(),
        sounds_dir: state.sounds_dir.display().to_string(),
    }
}

fn notify(app: &AppHandle) {
    let _ = app.emit("state-changed", ui_state(app));
}

fn update_settings(app: &AppHandle, change: impl FnOnce(&mut Settings)) {
    let state = app.state::<AppState>();
    let mut settings = state.settings.lock().unwrap();
    change(&mut settings);
    settings.save(&state.settings_path);
}

fn apply_muted(app: &AppHandle, muted: bool) {
    let state = app.state::<AppState>();
    state.audio.set_muted(muted);
    update_settings(app, |s| s.muted = muted);
    if let Some(item) = state.mute_item.lock().unwrap().as_ref() {
        let _ = item.set_checked(muted);
    }
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(if muted { "Clicky (muted)" } else { "Clicky by Heyrishi" }));
    }
    notify(app);
}

fn apply_profile(app: &AppHandle, profile: &str) {
    let state = app.state::<AppState>();
    if !state.audio.set_profile(profile) {
        return;
    }
    update_settings(app, |s| s.profile = profile.to_string());
    for (name, item) in state.profile_items.lock().unwrap().iter() {
        let _ = item.set_checked(*name == profile);
    }
    notify(app);
}

fn profile_label(profile: &str) -> &'static str {
    match profile {
        "creamy" => "Creamy",
        "classic" => "Classic",
        _ => "Crisp",
    }
}

fn show_window(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Hides the window (and the Dock icon on macOS); Clicky keeps running in the tray.
fn hide_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}

// MARK: - Commands

#[tauri::command]
fn get_state(app: AppHandle) -> UiState {
    ui_state(&app)
}

#[tauri::command]
fn set_volume(app: AppHandle, state: State<AppState>, volume: f32) {
    state.audio.set_volume(volume);
    update_settings(&app, |s| s.volume = volume.clamp(0.0, 1.0));
}

#[tauri::command]
fn set_muted(app: AppHandle, muted: bool) {
    apply_muted(&app, muted);
}

#[tauri::command]
fn set_profile(app: AppHandle, profile: String) {
    apply_profile(&app, &profile);
}

// Off the main thread: listing devices can take a moment on some systems.
#[tauri::command(async)]
fn list_output_devices(app: AppHandle) -> Vec<String> {
    let state = app.state::<AppState>();
    state.audio.output_devices()
}

#[tauri::command]
fn set_output_device(app: AppHandle, state: State<AppState>, device: Option<String>) {
    state.audio.set_output_device(device.clone());
    update_settings(&app, |s| s.output_device = device);
    notify(&app);
}

#[tauri::command]
fn set_close_to_tray(app: AppHandle, value: bool) {
    update_settings(&app, |s| s.close_to_tray = value);
    notify(&app);
}

#[tauri::command]
fn set_start_minimized(app: AppHandle, value: bool) {
    update_settings(&app, |s| s.start_minimized = value);
    notify(&app);
}

#[tauri::command]
fn preview(state: State<AppState>, sound: String) {
    if let Some(sound) = Sound::from_name(&sound) {
        state.audio.play(sound);
    }
}

#[tauri::command]
fn hide_to_tray(app: AppHandle) {
    hide_window(&app);
}

#[tauri::command]
fn open_permission_settings() {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
        .spawn();
}

#[tauri::command]
fn open_sounds_folder(state: State<AppState>) {
    let dir = &state.sounds_dir;
    let _ = std::fs::create_dir_all(dir);
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(target_os = "windows")]
    let opener = "explorer";
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let opener = "xdg-open";
    let _ = std::process::Command::new(opener).arg(dir).spawn();
}

#[tauri::command]
fn relaunch(app: AppHandle) {
    app.restart();
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

// MARK: - Setup

fn build_tray(app: &AppHandle, muted: bool, profile: &str) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Clicky", true, None::<&str>)?;
    let mute = CheckMenuItem::with_id(app, "mute", "Mute sounds", true, muted, None::<&str>)?;
    let mut profile_items = Vec::new();
    for name in audio::PROFILES {
        let item = CheckMenuItem::with_id(app, format!("profile:{name}"), profile_label(name), true, name == profile, None::<&str>)?;
        profile_items.push((name, item));
    }
    let profile_refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> =
        profile_items.iter().map(|(_, item)| item as &dyn tauri::menu::IsMenuItem<Wry>).collect();
    let profiles = Submenu::with_items(app, "Sound profile", true, &profile_refs)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Clicky", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &PredefinedMenuItem::separator(app)?,
            &mute,
            &profiles,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let state = app.state::<AppState>();
    *state.mute_item.lock().unwrap() = Some(mute);
    *state.profile_items.lock().unwrap() = profile_items;

    TrayIconBuilder::with_id("main")
        .icon(tauri::image::Image::from_bytes(TRAY_ICON)?)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip(if muted { "Clicky (muted)" } else { "Clicky by Heyrishi" })
        .menu(&menu)
        // macOS convention is a menu on click; elsewhere a left click opens the window.
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_window(app),
            "mute" => {
                let muted = !app.state::<AppState>().settings.lock().unwrap().muted;
                apply_muted(app, muted);
            }
            "quit" => app.exit(0),
            id => {
                if let Some(profile) = id.strip_prefix("profile:") {
                    apply_profile(app, profile);
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn start_key_listener(app: &AppHandle) {
    let state = app.state::<AppState>();
    let audio = state.audio.clone();
    let on_sound: OnSound = Arc::new(move |sound| audio.play(sound));

    #[cfg(not(target_os = "macos"))]
    {
        listener_rdev::start(on_sound);
        state.permission.store(PERMISSION_GRANTED, Ordering::Relaxed);
    }

    #[cfg(target_os = "macos")]
    {
        if listener_macos::start(on_sound.clone()) {
            state.permission.store(PERMISSION_GRANTED, Ordering::Relaxed);
            return;
        }
        state.permission.store(PERMISSION_WAITING, Ordering::Relaxed);
        listener_macos::request_permission();
        let app = app.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let state = app.state::<AppState>();
            if listener_macos::start(on_sound.clone()) {
                state.permission.store(PERMISSION_GRANTED, Ordering::Relaxed);
                notify(&app);
                break;
            } else if listener_macos::has_permission()
                && state.permission.swap(PERMISSION_RELAUNCH, Ordering::Relaxed) != PERMISSION_RELAUNCH
            {
                notify(&app);
            }
        });
    }
}

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_window(app)))
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let settings_path = config_dir.join("settings.json");
            let sounds_dir = config_dir.join("sounds");
            let mut settings = Settings::load(&settings_path);

            let audio = Arc::new(AudioHandle::start(&sounds_dir, settings.output_device.clone()));
            audio.set_volume(settings.volume);
            audio.set_muted(settings.muted);
            if !audio.set_profile(&settings.profile) {
                settings.profile = audio::DEFAULT_PROFILE.to_string();
                audio.set_profile(&settings.profile);
            }

            let muted = settings.muted;
            let profile = settings.profile.clone();
            let start_hidden = settings.start_minimized && settings.close_to_tray;
            app.manage(AppState {
                audio,
                settings: Mutex::new(settings),
                settings_path,
                sounds_dir,
                permission: AtomicU8::new(PERMISSION_WAITING),
                mute_item: Mutex::new(None),
                profile_items: Mutex::new(Vec::new()),
            });

            let handle = app.handle();
            build_tray(handle, muted, &profile)?;
            start_key_listener(handle);
            if start_hidden {
                hide_window(handle);
            } else {
                show_window(handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                if app.state::<AppState>().settings.lock().unwrap().close_to_tray {
                    api.prevent_close();
                    hide_window(app);
                } else {
                    app.exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            set_volume,
            set_muted,
            set_profile,
            list_output_devices,
            set_output_device,
            set_close_to_tray,
            set_start_minimized,
            preview,
            hide_to_tray,
            open_permission_settings,
            open_sounds_folder,
            relaunch,
            quit
        ])
        .build(tauri::generate_context!())
        .expect("error while building Clicky");

    app.run(|_app, _event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = _event {
            show_window(_app);
        }
    });
}
