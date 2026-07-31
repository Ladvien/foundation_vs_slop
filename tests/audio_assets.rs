//! **Asset contract: every footstep clip is a single footfall, not a walking loop.**
//!
//! GPU-free and `App`-free — runs in the `cargo test` hard gate, like `valkyrie_asset.rs`.
//!
//! # Why this exists
//!
//! `audio::footsteps` fires one clip per footfall on a shared timer (~0.34–0.5 s apart). That is a
//! silent contract with the bytes on disk: the clip must BE one footfall. It has now been broken
//! twice by the same mistake — an untrimmed multi-step recording shipped as a one-shot:
//!
//! * 2026-07-05: the four `carpet_*.ogg` were one identical 8.07 s recording of ~10 footsteps,
//!   fired every 0.12–0.5 s — "the footsteps sound like an army"
//!   (`slop/dev_journal/2026-07-05-dimensional-crab-swarm.md`).
//! * 2026-07-30: `concrete_1..4.ogg` and `mud_step_1..8.ogg` landed as 7–8 s recordings of ~15
//!   footfalls each. Up to ~24 overlapping walking loops alive at once — the same army, and the
//!   response (lowering rate + gain) buried the whole voice instead. Fixed 2026-07-31.
//!
//! Nothing fails when this regresses — it just sounds wrong — so the contract is asserted here,
//! against the bytes.

use std::path::Path;

/// A one-shot footfall with its decay tail. The shipped sets run 0.49–0.67 s; an untrimmed source
/// recording is 7–8 s. 1.5 s splits those cleanly with headroom for a boomier future clip.
const MAX_FOOTFALL_SECS: f64 = 1.5;
const MIN_FOOTFALL_SECS: f64 = 0.1;

/// Decode an Ogg/Vorbis file's duration from its bytes: total samples are the last Ogg page's
/// granule position; the rate is in the Vorbis identification header. No audio crate needed.
fn ogg_duration_secs(bytes: &[u8]) -> Result<f64, String> {
    // Vorbis ID header packet: 0x01 "vorbis" | version u32 | channels u8 | sample_rate u32 LE.
    let id = b"\x01vorbis";
    let id_at = bytes
        .windows(id.len())
        .position(|w| w == id)
        .ok_or("no Vorbis identification header")?;
    let rate_at = id_at + 12;
    let rate_bytes: [u8; 4] = bytes
        .get(rate_at..rate_at + 4)
        .and_then(|s| s.try_into().ok())
        .ok_or("truncated identification header")?;
    let sample_rate = u32::from_le_bytes(rate_bytes);
    if sample_rate == 0 {
        return Err("sample rate is zero".into());
    }
    // Last Ogg page ("OggS" capture pattern): granule position is bytes 6..14 of the page header.
    let last_page = bytes
        .windows(4)
        .rposition(|w| w == b"OggS")
        .ok_or("no Ogg page found")?;
    let granule_bytes: [u8; 8] = bytes
        .get(last_page + 6..last_page + 14)
        .and_then(|s| s.try_into().ok())
        .ok_or("truncated final Ogg page")?;
    let total_samples = u64::from_le_bytes(granule_bytes);
    Ok(total_samples as f64 / sample_rate as f64)
}

#[test]
fn every_footstep_clip_is_a_single_footfall() {
    let dir = Path::new("assets/audio/foot");
    let mut checked = 0usize;
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .expect("assets/audio/foot must exist")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "ogg"))
        .collect();
    entries.sort();
    for path in entries {
        let bytes = std::fs::read(&path).expect("readable clip");
        let secs = match ogg_duration_secs(&bytes) {
            Ok(s) => s,
            Err(e) => panic!("{}: {e}", path.display()),
        };
        assert!(
            (MIN_FOOTFALL_SECS..=MAX_FOOTFALL_SECS).contains(&secs),
            "{} is {secs:.2}s — a footstep one-shot must be a single footfall \
             ({MIN_FOOTFALL_SECS}–{MAX_FOOTFALL_SECS}s). An untrimmed multi-step recording here \
             reads as a marching army in-game (see this file's header; it has shipped twice).",
            path.display()
        );
        checked += 1;
    }
    // The manifest in `audio::load_audio` wires 4 carpet + 4 concrete + 8 mud clips. If files
    // vanish the load would 404 at runtime, not here — so pin the census too.
    assert_eq!(checked, 16, "expected the 16 wired footstep clips in assets/audio/foot");
}
