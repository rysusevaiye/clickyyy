//! Low-latency, polyphonic sample player.
//!
//! All samples are decoded at startup and resampled to the output device's rate, so
//! the audio callback only ever mixes pre-loaded `f32` slices. Keystrokes reach the
//! callback through a lock-free bounded channel and start on the very next buffer,
//! which is requested as small as the device allows (256 frames, ~5 ms at 48 kHz).

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{bounded, Receiver, Sender};

use crate::keys::Sound;

const VOICES: usize = 24;
const IO_BUFFER_FRAMES: u32 = 256;

/// Built-in sound profiles: the folder name under `assets/sounds/`.
pub const PROFILES: [&str; 3] = ["crisp", "creamy", "classic"];
pub const DEFAULT_PROFILE: &str = "crisp";

/// Embeds one profile's samples, five variants per sound in `Sound::ALL` order.
macro_rules! profile_sounds {
    ($profile:literal) => {
        profile_sounds!(@files $profile:
            "click_1", "click_2", "click_3", "click_4", "click_5",
            "thock_1", "thock_2", "thock_3", "thock_4", "thock_5",
            "clack_1", "clack_2", "clack_3", "clack_4", "clack_5",
            "snap_1", "snap_2", "snap_3", "snap_4", "snap_5",
            "topout_1", "topout_2", "topout_3", "topout_4", "topout_5")
    };
    (@files $profile:literal: $($name:literal),*) => {
        &[$(include_bytes!(concat!("../../assets/sounds/", $profile, "/", $name, ".wav")) as &[u8]),*]
    };
}

const BUILTIN: [&[&[u8]]; 3] = [profile_sounds!("crisp"), profile_sounds!("creamy"), profile_sounds!("classic")];
const VARIANTS_PER_SOUND: usize = 5;

/// Per category, the variants as mono samples at the device rate.
type Banks = Vec<Vec<Vec<f32>>>;

#[derive(Clone, Copy)]
struct Trigger {
    profile: usize,
    sound: usize,
    variant: usize,
    gain: f32,
}

struct Shared {
    /// Linear gain as f32 bits.
    gain: AtomicU32,
    muted: AtomicBool,
}

pub struct AudioHandle {
    tx: Sender<Trigger>,
    shared: Arc<Shared>,
    profile: AtomicUsize,
    variants: [[usize; 5]; 3],
    last_variant: [AtomicUsize; 5],
}

impl AudioHandle {
    /// Opens the default output device and starts the mixer. Custom samples in
    /// `custom_dir` (e.g. `thock_1.wav`) replace the built-in ones per category.
    pub fn start(custom_dir: &Path) -> AudioHandle {
        let (tx, rx) = bounded::<Trigger>(256);
        let shared = Arc::new(Shared { gain: AtomicU32::new(0.5f32.to_bits()), muted: AtomicBool::new(false) });

        // The cpal stream isn't Send on every platform, so it lives on its own thread.
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<[[usize; 5]; 3]>();
        let thread_shared = shared.clone();
        let custom_dir = custom_dir.to_path_buf();
        std::thread::Builder::new()
            .name("clicky-audio".into())
            .spawn(move || match open_stream(&custom_dir, rx, thread_shared) {
                Ok((stream, variants)) => {
                    let _ = ready_tx.send(variants);
                    let _stream = stream;
                    loop {
                        std::thread::park();
                    }
                }
                Err(err) => {
                    eprintln!("clicky: audio unavailable: {err}");
                    let _ = ready_tx.send([[0; 5]; 3]);
                }
            })
            .expect("spawn audio thread");

        let variants = ready_rx.recv().unwrap_or([[0; 5]; 3]);
        AudioHandle { tx, shared, profile: AtomicUsize::new(0), variants, last_variant: Default::default() }
    }

    /// Switches the built-in sound profile; returns false for an unknown name.
    pub fn set_profile(&self, name: &str) -> bool {
        match PROFILES.iter().position(|p| *p == name) {
            Some(index) => {
                self.profile.store(index, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// Perceptual 0...1 volume.
    pub fn set_volume(&self, volume: f32) {
        let v = volume.clamp(0.0, 1.0);
        self.shared.gain.store((v * v).to_bits(), Ordering::Relaxed);
    }

    pub fn set_muted(&self, muted: bool) {
        self.shared.muted.store(muted, Ordering::Relaxed);
    }

    pub fn play(&self, sound: Sound) {
        if self.shared.muted.load(Ordering::Relaxed) {
            return;
        }
        let profile = self.profile.load(Ordering::Relaxed);
        let count = self.variants[profile][sound.index()];
        if count == 0 {
            return;
        }
        // Random variant, never the same one twice in a row.
        let last = &self.last_variant[sound.index()];
        let mut variant = fastrand::usize(..count);
        if count > 1 && variant == last.load(Ordering::Relaxed) {
            variant = (variant + 1) % count;
        }
        last.store(variant, Ordering::Relaxed);
        let gain = 0.85 + fastrand::f32() * 0.15;
        let _ = self.tx.try_send(Trigger { profile, sound: sound.index(), variant, gain });
    }
}

type StreamAndVariants = (cpal::Stream, [[usize; 5]; 3]);

fn open_stream(custom_dir: &Path, rx: Receiver<Trigger>, shared: Arc<Shared>) -> Result<StreamAndVariants, String> {
    let device = cpal::default_host().default_output_device().ok_or("no output device")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let sample_rate = supported.sample_rate().0;
    let format = supported.sample_format();

    let profiles: Vec<Banks> = (0..PROFILES.len()).map(|p| load_banks(p, custom_dir, sample_rate)).collect();
    let mut variants = [[0; 5]; 3];
    for (p, banks) in profiles.iter().enumerate() {
        for (s, bank) in banks.iter().enumerate() {
            variants[p][s] = bank.len();
        }
    }
    let banks = Arc::new(profiles);

    let mut config: cpal::StreamConfig = supported.config();
    let mut buffer_sizes = vec![cpal::BufferSize::Default];
    if let cpal::SupportedBufferSize::Range { min, max } = supported.buffer_size() {
        buffer_sizes.insert(0, cpal::BufferSize::Fixed(IO_BUFFER_FRAMES.clamp(*min, *max)));
    }

    let mut last_err = String::new();
    for buffer_size in buffer_sizes {
        config.buffer_size = buffer_size;
        let mixer = Mixer::new(banks.clone(), rx.clone(), shared.clone());
        let result = match format {
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, mixer),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, mixer),
            cpal::SampleFormat::I32 => build::<i32>(&device, &config, mixer),
            _ => build::<f32>(&device, &config, mixer),
        };
        match result.and_then(|s| s.play().map(|_| s).map_err(|e| e.to_string())) {
            Ok(stream) => return Ok((stream, variants)),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, mut mixer: Mixer) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| mixer.fill(data, channels),
            |err| eprintln!("clicky: audio stream error: {err}"),
            None,
        )
        .map_err(|e| e.to_string())
}

#[derive(Clone, Copy, Default)]
struct Voice {
    profile: usize,
    sound: usize,
    variant: usize,
    pos: usize,
    gain: f32,
    active: bool,
}

struct Mixer {
    banks: Arc<Vec<Banks>>,
    rx: Receiver<Trigger>,
    shared: Arc<Shared>,
    voices: [Voice; VOICES],
    next: usize,
}

impl Mixer {
    fn new(banks: Arc<Vec<Banks>>, rx: Receiver<Trigger>, shared: Arc<Shared>) -> Self {
        Self { banks, rx, shared, voices: [Voice::default(); VOICES], next: 0 }
    }

    /// Runs on the real-time audio thread: no allocation, no locks.
    fn fill<T: cpal::Sample + cpal::FromSample<f32>>(&mut self, data: &mut [T], channels: usize) {
        while let Ok(t) = self.rx.try_recv() {
            self.voices[self.next] =
                Voice { profile: t.profile, sound: t.sound, variant: t.variant, pos: 0, gain: t.gain, active: true };
            self.next = (self.next + 1) % VOICES;
        }
        let master = f32::from_bits(self.shared.gain.load(Ordering::Relaxed));
        for frame in data.chunks_mut(channels) {
            let mut mix = 0.0f32;
            for voice in self.voices.iter_mut().filter(|v| v.active) {
                let samples = &self.banks[voice.profile][voice.sound][voice.variant];
                match samples.get(voice.pos) {
                    Some(s) => {
                        mix += s * voice.gain;
                        voice.pos += 1;
                    }
                    None => voice.active = false,
                }
            }
            let value = T::from_sample((mix * master).clamp(-1.0, 1.0));
            for out in frame.iter_mut() {
                *out = value;
            }
        }
    }
}

/// Custom samples, when present, replace that sound in every profile.
fn load_banks(profile: usize, custom_dir: &Path, sample_rate: u32) -> Banks {
    Sound::ALL
        .iter()
        .map(|sound| {
            let custom = load_custom(custom_dir, sound.name(), sample_rate);
            if !custom.is_empty() {
                return custom;
            }
            let start = sound.index() * VARIANTS_PER_SOUND;
            BUILTIN[profile][start..start + VARIANTS_PER_SOUND]
                .iter()
                .filter_map(|bytes| decode(std::io::Cursor::new(*bytes), sample_rate))
                .collect()
        })
        .collect()
}

fn load_custom(dir: &Path, prefix: &str, sample_rate: u32) -> Vec<Vec<f32>> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with(prefix) && name.to_ascii_lowercase().ends_with(".wav")
        })
        .collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|p| std::fs::File::open(p).ok())
        .filter_map(|f| decode(std::io::BufReader::new(f), sample_rate))
        .collect()
}

/// Decodes a WAV to mono f32 at `sample_rate`, with leading silence trimmed
/// (silence before the transient would read as latency).
fn decode<R: std::io::Read>(reader: R, sample_rate: u32) -> Option<Vec<f32>> {
    let mut wav = hound::WavReader::new(reader).ok()?;
    let spec = wav.spec();
    let channels = spec.channels.max(1) as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => wav.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            wav.samples::<i32>().filter_map(Result::ok).map(|s| s as f32 * scale).collect()
        }
    };
    let mono: Vec<f32> = interleaved.chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32).collect();
    let start = mono.iter().position(|s| s.abs() > 0.003)?.saturating_sub(16);
    Some(resample(&mono[start..], spec.sample_rate, sample_rate))
}

fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let step = from as f64 / to as f64;
    let len = (input.len() as f64 / step) as usize;
    (0..len)
        .map(|i| {
            let pos = i as f64 * step;
            let j = pos as usize;
            let frac = (pos - j as f64) as f32;
            let a = input[j];
            let b = *input.get(j + 1).unwrap_or(&a);
            a + (b - a) * frac
        })
        .collect()
}
