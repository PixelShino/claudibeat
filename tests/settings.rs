use claudibeat::notify::ToastKind;
use claudibeat::settings::Settings;
use claudibeat::sound::wav;
use std::path::Path;

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("settings.json")
}

#[test]
fn missing_file_means_sound_on() {
    assert!(Settings::load(&tmp("settings_missing")).sound);
}

#[test]
fn round_trip() {
    let path = tmp("settings_round_trip");
    Settings { sound: false }.save(&path).unwrap();
    assert!(!Settings::load(&path).sound);
}

#[test]
fn torn_file_falls_back_to_defaults() {
    let path = tmp("settings_torn");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{\"sound\":").unwrap();
    assert!(Settings::load(&path).sound);
}

#[test]
fn wav_has_header_and_length() {
    for (kind, ms) in [(ToastKind::Waiting, 180), (ToastKind::Done, 140)] {
        let w = wav(kind);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        let data_len = u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize;
        assert_eq!(data_len, 44_100 * ms / 1000 * 2, "{kind:?}: 16-bit mono");
        assert_eq!(w.len(), 44 + data_len);
        let peak = w[44..].chunks(2).map(|s| i16::from_le_bytes([s[0], s[1]]).unsigned_abs()).max().unwrap();
        assert!(peak > 1_000 && peak < 6_000, "soft, about -18 dBFS: {peak}");
    }
}
