//! Two short, soft chimes made in memory: no asset files, no system "ding".
//! Waiting rises 660 → 880 Hz (a question), done is a single 880 Hz note.

use crate::notify::ToastKind;
use std::sync::OnceLock;

const RATE: u32 = 44_100;
/// About -18 dBFS: present but never startling.
const AMPLITUDE: f32 = 0.126 * i16::MAX as f32;

fn tone(out: &mut Vec<i16>, hz: f32, ms: u32) {
    let n = RATE * ms / 1000;
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let attack = (t / 0.005).min(1.0);
        let decay = (-t * 18.0).exp();
        let s = (std::f32::consts::TAU * hz * t).sin() * attack * decay * AMPLITUDE;
        out.push(s as i16);
    }
}

/// A complete 16-bit mono PCM WAV file for `kind`.
pub fn wav(kind: ToastKind) -> Vec<u8> {
    let mut samples = Vec::new();
    if kind.is_waiting() {
        tone(&mut samples, 660.0, 90);
        tone(&mut samples, 880.0, 90);
    } else {
        tone(&mut samples, 880.0, 140);
    }
    let data_len = (samples.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + data_len as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&RATE.to_le_bytes());
    w.extend_from_slice(&(RATE * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        w.extend_from_slice(&s.to_le_bytes());
    }
    w
}

/// Play the chime for `kind` without blocking.
pub fn play(kind: ToastKind) {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    static WAITING: OnceLock<Vec<u8>> = OnceLock::new();
    static DONE: OnceLock<Vec<u8>> = OnceLock::new();
    let bytes = if kind.is_waiting() { WAITING.get_or_init(|| wav(ToastKind::Waiting)) } else { DONE.get_or_init(|| wav(ToastKind::Done)) };
    // SAFETY: the buffer is 'static, so it outlives the asynchronous playback.
    unsafe {
        let _ = PlaySoundW(windows::core::PCWSTR(bytes.as_ptr().cast()), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
    }
}
