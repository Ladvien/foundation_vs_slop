//! **The coagulation timeline.** One scalar age, four channels a renderer can read.
//!
//! # Why this is not a colour ramp
//!
//! Blood ages by a chemistry with a known sequence and by a physics with a known shape, and both are
//! visible. Bremmer et al., *"Forensic quest for age determination of bloodstains"*,
//! `doi:10.1016/j.forsciint.2011.07.027`, review the optical route: oxyhaemoglobin oxidises to
//! **methaemoglobin** and then denatures to **hemichrome**, and the colour walks with it — bright red
//! to red-brown to a dark brown-grey. That is three stops, not two, and the middle one is why fresh
//! blood and day-old blood do not look like the same paint at two brightnesses.
//!
//! The physics is a drying droplet. Laan et al., *"Morphology of drying blood pools"*,
//! `doi:10.1016/j.forsciint.2016.08.005`, find that pools of very different mass **collapse onto one
//! normalised drying curve**, which is what makes a single shared shape legitimate here rather than a
//! convenience. Smith, Nicloux & Brutin, `doi:10.1038/s41598-020-65465-4`, show the front is
//! **rim-first**: the edge is dry and matte while the centre is still wet and glossy. Above about
//! 50 % relative humidity the serum phase-separates and spreads *outside* the pool as a halo, and the
//! late crust cracks into a craquelure network.
//!
//! # The channel that matters most is not a colour
//!
//! **Wetness is specular, and it is the strongest cue.** Oum, Lieberman & Aylward, *"A feel for
//! disgust: tactile cues to pathogen presence"*, `doi:10.1080/02699931.2010.496997`, find that
//! moistness — not colour, not shape — is what drives the disgust response. So [`Appearance`] carries
//! roughness as a first-class channel and a renderer is expected to route it to a
//! metallic-roughness map rather than tinting an albedo darker.

use crate::settings::BloodSettings;
use crate::{m, vec};

/// Ticks a pool of [`DRY_REF_AREA_M2`] takes to dry, at 60 Hz.
///
/// **Compressed from the forensic timescale, deliberately and with the number stated.** Laan et al.
/// measure real pools drying over **tens of minutes**; a game cannot hold a scene for 25 minutes to
/// show a floor going matte, and a framework that shipped the physical constant would have every
/// consumer divide it by an arbitrary factor in private. 1800 ticks is 30 s at 60 Hz — the same
/// reference `bevy_wetmap`'s texel-scale `dry_ticks` uses, so blood on a wall and blood on a floor
/// dry at one rate.
///
/// The *shape* is the measured one. Only the clock is compressed.
pub const DRY_REF_TICKS: u32 = 1800;

/// The pool area [`DRY_REF_TICKS`] is quoted for, m² — 10 cm², a small slick.
pub const DRY_REF_AREA_M2: f32 = 1.0e-3;

/// How drying time scales with area.
///
/// Evaporation is limited by the wetted perimeter and the film thickness rather than by volume, so the
/// time goes as roughly the square root of the area: a pool four times as wide takes twice as long,
/// not four times. Laan's collapse onto one normalised curve is what licenses a single exponent.
pub const DRY_AREA_EXPONENT: f32 = 0.5;

/// Oxyhaemoglobin — fresh, bright arterial red. sRGB.
pub const SRGB_OXY: [f32; 3] = [0.60, 0.03, 0.03];
/// Methaemoglobin — the brown-red of the first hour. sRGB.
pub const SRGB_MET: [f32; 3] = [0.42, 0.09, 0.05];
/// Hemichrome — the dark brown-grey of a denatured crust. sRGB.
pub const SRGB_HEMI: [f32; 3] = [0.26, 0.11, 0.08];

/// Relative humidity above which serum phase-separates and spreads outside the pool.
///
/// Laan et al. put the transition near half saturation; below it the serum stays inside the wetted
/// edge and there is no halo at all — which is why this is a threshold and not a scale.
pub const HALO_HUMIDITY: f32 = 0.5;

/// Normalised age at which the crust begins to crack.
///
/// Late, because a crust that cracks while it is still glossy is a crust nobody has photographed.
pub const CRAQUELURE_ONSET: f32 = 0.72;

/// **Everything a renderer needs to draw blood of a given age.** Four channels and a colour.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Appearance {
    /// Base colour, sRGB, walking oxyHb → metHb → hemichrome.
    pub srgb: [f32; 3],
    /// Perceptual roughness, [`BloodSettings::wet_roughness`] → [`BloodSettings::dry_roughness`].
    /// **The strongest cue** — see the module docs.
    pub roughness: f32,
    /// How far the rim-first drying front has advanced, `[0, 1]`. `1.0` means the edge is fully dry;
    /// the centre lags, which is what a renderer uses to keep a wet middle in a matte ring.
    pub rim: f32,
    /// Serum halo intensity outside the wetted edge, `[0, 1]`. **Exactly zero** below
    /// [`HALO_HUMIDITY`].
    pub halo: f32,
    /// Crack-network intensity, `[0, 1]`. Zero until [`CRAQUELURE_ONSET`].
    pub craquelure: f32,
}

/// Ticks this pool takes to dry, from its area.
///
/// `hz` scales the reference, which was quoted at 60 Hz — a caller on 30 Hz gets half the ticks for
/// the same wall-clock drying, which is the only way a tick count can be rate-independent.
pub fn dry_ticks(area_m2: f32, hz: u32) -> u32 {
    let area = if area_m2.is_finite() && area_m2 > 0.0 { area_m2 } else { DRY_REF_AREA_M2 };
    let scale = m::powf(area / DRY_REF_AREA_M2, DRY_AREA_EXPONENT);
    let rate = if hz == 0 { 60.0 } else { hz as f32 };
    let ticks = DRY_REF_TICKS as f32 * scale * (rate / 60.0);
    if !ticks.is_finite() {
        return DRY_REF_TICKS;
    }
    (m::round(ticks) as u32).max(1)
}

/// **The appearance of blood at `age_ticks`.** One scalar age in, four channels and a colour out.
///
/// Every channel is monotone in age, and that is a contract rather than an accident: blood does not
/// re-wet, un-oxidise or un-crack, so a channel that could fall would be indistinguishable from a
/// bug at exactly the moment a player is looking at it.
pub fn appearance(age_ticks: u32, hz: u32, area_m2: f32, s: &BloodSettings) -> Appearance {
    let span = dry_ticks(area_m2, hz);
    // Normalised age on the shared curve Laan's mass series collapse onto.
    let t = (age_ticks as f32 / span as f32).clamp(0.0, 1.0);

    // Colour: two segments through three stops. The first is fast — oxidation to methaemoglobin is
    // most of the visible colour change and it happens early (Bremmer 2012).
    let srgb = if t < 0.35 {
        vec::lerp(SRGB_OXY, SRGB_MET, t / 0.35)
    } else {
        vec::lerp(SRGB_MET, SRGB_HEMI, (t - 0.35) / 0.65)
    };

    // Gloss collapses with the rim front rather than linearly with age: the surface stops being wet
    // when the film breaks, not gradually over the whole timeline.
    let rim = smoothstep(t / 0.6);
    let roughness = s.wet_roughness + (s.dry_roughness - s.wet_roughness) * rim;

    // The serum halo is a threshold, not a scale: below `HALO_HUMIDITY` the serum never leaves the
    // wetted edge, so this is exactly zero rather than very small.
    let halo = if s.humidity >= HALO_HUMIDITY {
        let over = ((s.humidity - HALO_HUMIDITY) / (1.0 - HALO_HUMIDITY)).clamp(0.0, 1.0);
        over * smoothstep(t / 0.5)
    } else {
        0.0
    };

    let craquelure = if t <= CRAQUELURE_ONSET {
        0.0
    } else {
        ((t - CRAQUELURE_ONSET) / (1.0 - CRAQUELURE_ONSET)).clamp(0.0, 1.0)
    };

    Appearance { srgb, roughness, rim, halo, craquelure }
}

/// The usual cubic ease, clamped. Written out because `no_std` has no `smoothstep` and because a
/// linear ramp makes a drying front look like a wipe transition.
fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every channel is monotone**, which is the contract: blood does not re-wet or un-oxidise.
    #[test]
    fn every_channel_only_moves_one_way() {
        let s = BloodSettings { humidity: 0.8, ..Default::default() };
        let span = dry_ticks(DRY_REF_AREA_M2, 60);
        let mut last = appearance(0, 60, DRY_REF_AREA_M2, &s);
        for age in 1..=span + 60 {
            let now = appearance(age, 60, DRY_REF_AREA_M2, &s);
            assert!(now.roughness >= last.roughness - 1.0e-6, "gloss returned at age {age}");
            assert!(now.rim >= last.rim - 1.0e-6, "the drying front receded at age {age}");
            assert!(now.halo >= last.halo - 1.0e-6, "the serum halo shrank at age {age}");
            assert!(now.craquelure >= last.craquelure - 1.0e-6, "a crack healed at age {age}");
            // Red only ever falls, which is the oxidation walk in one number.
            assert!(now.srgb[0] <= last.srgb[0] + 1.0e-6, "blood got redder at age {age}");
            last = now;
        }
    }

    /// The colour walks through **three** stops, and the middle one is visited — a two-stop ramp
    /// would pass a monotonicity test and still be the wrong chemistry.
    #[test]
    fn the_colour_walks_through_methaemoglobin() {
        let s = BloodSettings::default();
        let span = dry_ticks(DRY_REF_AREA_M2, 60);
        let fresh = appearance(0, 60, DRY_REF_AREA_M2, &s).srgb;
        let mid = appearance((span as f32 * 0.35) as u32, 60, DRY_REF_AREA_M2, &s).srgb;
        let old = appearance(span, 60, DRY_REF_AREA_M2, &s).srgb;
        for c in 0..3 {
            assert!(
                (fresh[c] - SRGB_OXY[c]).abs() < 1.0e-5,
                "fresh blood must be exactly oxyhaemoglobin"
            );
            assert!(
                (mid[c] - SRGB_MET[c]).abs() < 2.0e-2,
                "at a third of the timeline the colour must be methaemoglobin, got {mid:?}"
            );
            assert!(
                (old[c] - SRGB_HEMI[c]).abs() < 1.0e-5,
                "fully dried blood must be exactly hemichrome"
            );
        }
        // Green rises while red falls — a brown is not a dark red, and that is the whole point.
        assert!(old[1] > fresh[1], "hemichrome must be browner, not merely darker");
    }

    /// Gloss is the channel that carries wetness, and it must actually span the authored range.
    #[test]
    fn gloss_collapses_from_wet_to_dry() {
        let s = BloodSettings::default();
        let span = dry_ticks(DRY_REF_AREA_M2, 60);
        let wet = appearance(0, 60, DRY_REF_AREA_M2, &s).roughness;
        let dry = appearance(span, 60, DRY_REF_AREA_M2, &s).roughness;
        assert!((wet - s.wet_roughness).abs() < 1.0e-5, "fresh blood must be exactly wet");
        assert!((dry - s.dry_roughness).abs() < 1.0e-5, "dried blood must be exactly dry");
    }

    /// The halo is a **threshold**: dry air produces exactly none of it, at every age.
    #[test]
    fn a_serum_halo_needs_humid_air() {
        let dry_air = BloodSettings { humidity: 0.4, ..Default::default() };
        let humid = BloodSettings { humidity: 0.9, ..Default::default() };
        let span = dry_ticks(DRY_REF_AREA_M2, 60);
        for age in [0, span / 4, span / 2, span] {
            assert_eq!(
                appearance(age, 60, DRY_REF_AREA_M2, &dry_air).halo,
                0.0,
                "below the humidity threshold there is no halo at all, at age {age}"
            );
        }
        assert!(
            appearance(span / 2, 60, DRY_REF_AREA_M2, &humid).halo > 0.0,
            "humid air must grow a serum ring"
        );
    }

    /// Cracks are late, and a fresh pool has none. A crust that cracked while glossy would be the
    /// bug this refuses.
    #[test]
    fn cracks_are_late() {
        let s = BloodSettings::default();
        let span = dry_ticks(DRY_REF_AREA_M2, 60);
        assert_eq!(appearance(0, 60, DRY_REF_AREA_M2, &s).craquelure, 0.0);
        assert_eq!(appearance(span / 2, 60, DRY_REF_AREA_M2, &s).craquelure, 0.0);
        assert!(appearance(span, 60, DRY_REF_AREA_M2, &s).craquelure > 0.5, "a dry crust cracks");
    }

    /// A wider pool takes longer, sublinearly, and the tick count is rate-scaled so the wall-clock
    /// drying is the same at 30 and 60 Hz.
    #[test]
    fn drying_time_scales_with_area_and_with_the_tick_rate() {
        let small = dry_ticks(DRY_REF_AREA_M2, 60);
        let wide = dry_ticks(DRY_REF_AREA_M2 * 4.0, 60);
        assert_eq!(small, DRY_REF_TICKS, "the reference area must give the reference ticks");
        assert!(
            wide > small && wide < small * 4,
            "four times the area must take longer but not four times longer: {small} then {wide}"
        );
        assert_eq!(
            dry_ticks(DRY_REF_AREA_M2, 30),
            DRY_REF_TICKS / 2,
            "half the tick rate is half the ticks for the same wall clock"
        );
        for bad in [0.0f32, -1.0, f32::NAN] {
            assert!(dry_ticks(bad, 60) >= 1, "a nonsense area must not produce a zero span");
        }
    }
}
