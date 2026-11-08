//! The sampler's rules: what a chop may be ([`Sampler::sanitized`]), how cut points become
//! chops ([`chops_at`]), which onsets a fetch cuts at ([`strongest_onsets`]), the region a fresh
//! link suggests ([`suggested_region`]), what each chop tool cuts a region into
//! ([`chop_region`]) and what a re-cut keeps ([`rechop`]), the two sampler commands, and what
//! loading a file and flattening a sequence do with a sampler track.
//!
//! `ui/src/sampler.test.ts` pins the TypeScript mirror of the same helpers to the same numbers.

use dissonant_core::*;
use uuid::Uuid;

/// An eight-second 44.1 kHz stereo source, as a fetched link would arrive.
fn source() -> AudioSource {
    AudioSource {
        id: Uuid::new_v4(),
        name: "break".into(),
        frames: 8 * 44_100,
        sample_rate: 44_100,
        channels: 2,
        origin_url: Some("https://www.youtube.com/watch?v=abc".into()),
    }
}

fn onset(seconds: f64, strength: f64) -> Onset {
    Onset { seconds, strength }
}

fn bounds(chops: &[Chop]) -> Vec<(f64, f64)> {
    chops.iter().map(|c| (c.start_seconds, c.end_seconds)).collect()
}

/// A document holding one sampler track over [`source`], and that track's id.
fn sampler_doc() -> (Document, Uuid, AudioSource) {
    let src = source();
    let mut d = Document::new(ProjectModel::starter());
    d.apply(
        Command::AddSamplerTrack {
            name: "flip".into(),
            source: src.clone(),
            sampler: Sampler::new(src.id, chops_at(&[1.0, 2.0], 8.0)),
        },
        false,
    )
    .unwrap();
    let id = d.model().tracks.last().unwrap().id;
    (d, id, src)
}

// ---------------------------------------------------------------- sanitized

#[test]
fn sanitizing_clamps_every_chop_field_into_its_range() {
    let mut chop = Chop::new(-1.0, 99.0);
    chop.gain = 9.0;
    chop.attack_seconds = 7.0;
    chop.release_seconds = -1.0;
    chop.choke = 200;
    let s = Sampler::new(Uuid::new_v4(), vec![chop]).sanitized(8.0).unwrap();
    let c = &s.chops[0];
    assert_eq!((c.start_seconds, c.end_seconds), (0.0, 8.0));
    assert_eq!(c.gain, MAX_CHOP_GAIN);
    assert_eq!(c.attack_seconds, MAX_CHOP_ATTACK_SECONDS);
    assert_eq!(c.release_seconds, 0.0);
    assert_eq!(c.choke, MAX_CHOKE_GROUP);
}

#[test]
fn sanitizing_gives_a_chop_at_least_the_minimum_length_and_drops_one_that_cannot_fit() {
    let s = Sampler::new(
        Uuid::new_v4(),
        vec![
            Chop::new(2.0, 1.0),
            Chop::new(8.0 - MIN_CHOP_SECONDS / 2.0, 8.0),
            Chop::new(9.0, 10.0),
            Chop::new(f64::NAN, 1.0),
        ],
    )
    .sanitized(8.0)
    .unwrap();
    assert_eq!(bounds(&s.chops), vec![(2.0, 2.0 + MIN_CHOP_SECONDS)]);
}

#[test]
fn sanitizing_keeps_chop_order_because_order_is_the_key_mapping() {
    let s = Sampler::new(
        Uuid::new_v4(),
        vec![Chop::new(4.0, 5.0), Chop::new(0.0, 1.0), Chop::new(2.0, 3.0)],
    );
    assert_eq!(
        bounds(&s.sanitized(8.0).unwrap().chops),
        vec![(4.0, 5.0), (0.0, 1.0), (2.0, 3.0)]
    );
}

#[test]
fn sanitizing_refuses_more_than_max_chops() {
    let chops = (0..=MAX_CHOPS).map(|i| Chop::new(i as f64 * 0.1, i as f64 * 0.1 + 0.05)).collect();
    assert_eq!(Sampler::new(Uuid::new_v4(), chops).sanitized(60.0), None);
}

#[test]
fn sanitizing_lowers_the_base_pitch_until_the_last_chop_has_a_key() {
    let chops: Vec<Chop> = chops_at(&(1..10).map(|i| i as f64).collect::<Vec<_>>(), 10.0);
    assert_eq!(chops.len(), 10);
    let mut s = Sampler::new(Uuid::new_v4(), chops);
    s.base_pitch = 127;
    let s = s.sanitized(10.0).unwrap();
    assert_eq!(s.base_pitch, 118);
    assert_eq!(s.chop_for(127).map(|c| c.start_seconds), Some(9.0));
    assert_eq!(s.chop_for(117), None);
}

#[test]
fn sanitizing_tidies_the_analysis() {
    let mut s = Sampler::new(Uuid::new_v4(), vec![]);
    s.analysis = Some(SampleAnalysis {
        bpm: Some(-3.0),
        bpm_confidence: 4.0,
        key_root: Some(12),
        key_scale: ScaleType::Minor,
        key_confidence: -1.0,
        onsets: vec![onset(3.0, 2.0), onset(9.0, 0.5), onset(1.0, 0.25), onset(f64::NAN, 1.0)],
    });
    let a = s.sanitized(8.0).unwrap().analysis.unwrap();
    assert_eq!((a.bpm, a.bpm_confidence), (None, 1.0));
    assert_eq!((a.key_root, a.key_scale, a.key_confidence), (None, ScaleType::Minor, 0.0));
    assert_eq!(a.onsets, vec![onset(1.0, 0.25), onset(3.0, 1.0)]);
}

#[test]
fn sanitizing_keeps_only_the_strongest_onsets_past_the_cap() {
    let mut s = Sampler::new(Uuid::new_v4(), vec![]);
    let onsets: Vec<Onset> = (0..MAX_ONSETS + 2)
        .map(|i| onset(i as f64 * 0.01, if i < 2 { 0.0 } else { 0.5 }))
        .collect();
    s.analysis = Some(SampleAnalysis { onsets, ..SampleAnalysis::default() });
    let kept = s.sanitized(60.0).unwrap().analysis.unwrap().onsets;
    assert_eq!(kept.len(), MAX_ONSETS);
    assert!(kept.iter().all(|o| o.strength == 0.5));
    assert!(kept.windows(2).all(|w| w[0].seconds < w[1].seconds));
}

// ---------------------------------------------------------------- chops_at

#[test]
fn chops_at_cuts_between_consecutive_points_and_always_at_zero() {
    let chops = chops_at(&[3.0, 1.0, 1.0 + MIN_CHOP_SECONDS / 2.0, 8.0, -2.0, f64::NAN], 8.0);
    assert_eq!(bounds(&chops), vec![(0.0, 1.0), (1.0, 3.0), (3.0, 8.0)]);
    let c = &chops[0];
    assert_eq!((c.gain, c.reverse, c.mode, c.choke), (1.0, false, ChopMode::OneShot, 0));
    assert_eq!((c.attack_seconds, c.release_seconds), (0.0, DEFAULT_CHOP_RELEASE_SECONDS));
    assert!(c.name.is_empty());
}

#[test]
fn chops_at_never_leaves_a_last_chop_shorter_than_the_minimum() {
    let chops = chops_at(&[4.0, 8.0 - MIN_CHOP_SECONDS / 2.0], 8.0);
    assert_eq!(bounds(&chops), vec![(0.0, 4.0), (4.0, 8.0)]);
}

#[test]
fn chops_at_with_no_points_is_one_chop_over_the_whole_source() {
    assert_eq!(bounds(&chops_at(&[], 8.0)), vec![(0.0, 8.0)]);
    assert!(chops_at(&[], 0.0).is_empty());
    assert!(chops_at(&[1.0], f64::NAN).is_empty());
}

#[test]
fn chops_at_stops_at_max_chops_with_the_last_running_to_the_end() {
    let points: Vec<f64> = (1..200).map(|i| i as f64 * 0.1).collect();
    let chops = chops_at(&points, 20.0);
    assert_eq!(chops.len(), MAX_CHOPS);
    assert_eq!(chops.last().unwrap().end_seconds, 20.0);
}

// ---------------------------------------------------------------- strongest_onsets

#[test]
fn strongest_onsets_picks_the_loudest_hits_in_time_order() {
    let onsets = [onset(0.5, 0.2), onset(1.0, 0.9), onset(2.0, 0.5), onset(3.0, 0.9), onset(4.0, 0.1)];
    assert_eq!(strongest_onsets(&onsets, 3), vec![1.0, 2.0, 3.0]);
    assert_eq!(strongest_onsets(&onsets, 0), Vec::<f64>::new());
    assert_eq!(strongest_onsets(&onsets, 99).len(), 5);
}

#[test]
fn strongest_onsets_breaks_ties_toward_the_earlier_hit() {
    let onsets = [onset(3.0, 0.5), onset(1.0, 0.5), onset(2.0, 0.5)];
    assert_eq!(strongest_onsets(&onsets, 2), vec![1.0, 2.0]);
}

// ---------------------------------------------------------------- region and chopper

fn region(start_seconds: f64, end_seconds: f64) -> SampleRegion {
    SampleRegion { start_seconds, end_seconds }
}

#[test]
fn sanitizing_holds_the_region_inside_the_source_and_drops_one_too_short_to_cut() {
    let with = |r: SampleRegion| {
        let mut s = Sampler::new(Uuid::new_v4(), vec![]);
        s.region = Some(r);
        s.sanitized(8.0).unwrap().region
    };
    assert_eq!(with(region(-1.0, 99.0)), Some(region(0.0, 8.0)));
    assert_eq!(with(region(2.0, 3.0)), Some(region(2.0, 3.0)));
    assert_eq!(with(region(7.995, 20.0)), None);
    assert_eq!(with(region(5.0, 3.0)), None);
    assert_eq!(with(region(f64::NAN, 3.0)), None);
}

#[test]
fn sanitizing_holds_the_chopper_count_between_one_and_max_chops_and_keeps_the_voice() {
    let with = |c: Chopper| {
        let mut s = Sampler::new(Uuid::new_v4(), vec![]);
        s.chopper = Some(c);
        s.voice = SamplerVoice::Poly;
        s.sanitized(8.0).unwrap()
    };
    assert_eq!(with(Chopper::Auto { count: 0 }).chopper, Some(Chopper::Auto { count: 1 }));
    assert_eq!(with(Chopper::Equal { count: 500 }).chopper, Some(Chopper::Equal { count: MAX_CHOPS as u32 }));
    assert_eq!(with(Chopper::Grid { step: GridStep::Half }).chopper, Some(Chopper::Grid { step: GridStep::Half }));
    assert_eq!(with(Chopper::Auto { count: 16 }).voice, SamplerVoice::Poly);
}

#[test]
fn a_new_sampler_is_mono_over_the_whole_source() {
    let s = Sampler::new(Uuid::new_v4(), vec![]);
    assert_eq!((s.voice, s.region, s.chopper), (SamplerVoice::Mono, None, None));
}

#[test]
fn the_suggested_region_starts_at_the_first_strong_hit_and_runs_four_bars_of_the_tempo() {
    let onsets = vec![onset(0.2, 0.3), onset(1.0, 0.8), onset(2.0, 1.0)];
    let timed = SampleAnalysis { bpm: Some(120.0), onsets: onsets.clone(), ..SampleAnalysis::default() };
    assert_eq!(suggested_region(&timed, 30.0, 3), region(1.0, 7.0));
    assert_eq!(suggested_region(&timed, 30.0, 4), region(1.0, 9.0));
    let untimed = SampleAnalysis { onsets, ..SampleAnalysis::default() };
    assert_eq!(suggested_region(&untimed, 30.0, 3), region(1.0, 9.0));
}

#[test]
fn the_suggested_region_is_held_inside_the_source() {
    let at = |onsets: Vec<Onset>, duration: f64| {
        suggested_region(&SampleAnalysis { onsets, ..SampleAnalysis::default() }, duration, 4)
    };
    assert_eq!(at(vec![onset(1.0, 0.8)], 5.0), region(1.0, 5.0));
    // Nothing strong: from the start.
    assert_eq!(at(vec![onset(0.5, 0.2)], 30.0), region(0.0, 8.0));
    assert_eq!(at(vec![], 30.0), region(0.0, 8.0));
    // A strong hit right at the end slides the region back rather than leaving a sliver.
    assert_eq!(at(vec![onset(9.999, 1.0)], 10.0), region(2.0, 10.0));
    assert_eq!(at(vec![onset(8.0, 1.0)], 8.0), region(0.0, 8.0));
}

/// Hits around and inside the region 1…9: one on its start, one too near its end, two outside.
fn region_hits() -> SampleAnalysis {
    SampleAnalysis {
        onsets: vec![
            onset(0.5, 1.0),
            onset(1.0, 0.9),
            onset(2.0, 0.5),
            onset(3.0, 0.8),
            onset(5.0, 0.7),
            onset(8.995, 1.0),
            onset(12.0, 1.0),
        ],
        ..SampleAnalysis::default()
    }
}

#[test]
fn auto_cuts_at_the_region_start_and_its_strongest_hits_inside_up_to_count_chops() {
    let a = region_hits();
    let auto = |count| bounds(&chop_region(&Chopper::Auto { count }, &region(1.0, 9.0), Some(&a), 4));
    assert_eq!(auto(3), vec![(1.0, 3.0), (3.0, 5.0), (5.0, 9.0)]);
    assert_eq!(auto(16), vec![(1.0, 2.0), (2.0, 3.0), (3.0, 5.0), (5.0, 9.0)]);
    assert_eq!(auto(1), vec![(1.0, 9.0)]);
    assert_eq!(auto(0), vec![(1.0, 9.0)]);
    let blind = chop_region(&Chopper::Auto { count: 16 }, &region(1.0, 9.0), None, 4);
    assert_eq!(bounds(&blind), vec![(1.0, 9.0)]);
}

#[test]
fn equal_splits_the_region_evenly() {
    let equal = |count, r| bounds(&chop_region(&Chopper::Equal { count }, &r, None, 4));
    assert_eq!(equal(4, region(2.0, 6.0)), vec![(2.0, 3.0), (3.0, 4.0), (4.0, 5.0), (5.0, 6.0)]);
    assert_eq!(equal(1, region(2.0, 6.0)), vec![(2.0, 6.0)]);
    assert_eq!(equal(500, region(0.0, 600.0)).len(), MAX_CHOPS);
}

#[test]
fn the_grid_counts_bars_halves_or_beats_of_the_tempo_from_the_region_start() {
    let a = SampleAnalysis { bpm: Some(120.0), ..SampleAnalysis::default() };
    let grid = |step, r, bpb| bounds(&chop_region(&Chopper::Grid { step }, &r, Some(&a), bpb));
    assert_eq!(grid(GridStep::Bar, region(0.25, 6.0), 4), vec![(0.25, 2.25), (2.25, 4.25), (4.25, 6.0)]);
    assert_eq!(grid(GridStep::Half, region(0.0, 2.0), 4), vec![(0.0, 1.0), (1.0, 2.0)]);
    assert_eq!(grid(GridStep::Beat, region(0.0, 2.0), 4).len(), 4);
    assert_eq!(grid(GridStep::Beat, region(0.0, 2.0), 3).len(), 4);
    assert_eq!(
        grid(GridStep::Half, region(0.0, 3.0), 3),
        vec![(0.0, 0.75), (0.75, 1.5), (1.5, 2.25), (2.25, 3.0)]
    );
    let fast = SampleAnalysis { bpm: Some(200.0), ..SampleAnalysis::default() };
    let many = chop_region(&Chopper::Grid { step: GridStep::Beat }, &region(0.0, 600.0), Some(&fast), 4);
    assert_eq!(many.len(), MAX_CHOPS);
}

#[test]
fn the_grid_needs_a_tempo_and_nothing_cuts_a_region_too_short() {
    let bar = Chopper::Grid { step: GridStep::Bar };
    assert!(chop_region(&bar, &region(0.0, 8.0), Some(&SampleAnalysis::default()), 4).is_empty());
    assert!(chop_region(&bar, &region(0.0, 8.0), None, 4).is_empty());
    let a = region_hits();
    for chopper in [Chopper::Auto { count: 16 }, Chopper::Equal { count: 4 }] {
        assert!(chop_region(&chopper, &region(1.0, 1.005), Some(&a), 4).is_empty());
    }
}

#[test]
fn rechop_keeps_each_chops_settings_on_the_new_cuts() {
    let mut old = chops_at(&[2.0], 8.0);
    old[0].name = "kick".into();
    old[0].gain = 2.0;
    old[0].reverse = true;
    old[0].mode = ChopMode::Gate;
    old[0].attack_seconds = 0.1;
    old[0].release_seconds = 0.2;
    old[0].choke = 3;
    old[1].name = "snare".into();
    let fresh = chops_at(&[1.0, 3.0], 8.0);
    let ids: Vec<Uuid> = fresh.iter().map(|c| c.id).collect();
    let out = rechop(&old, fresh);
    assert_eq!(bounds(&out), vec![(0.0, 1.0), (1.0, 3.0), (3.0, 8.0)]);
    let kick = &out[0];
    assert_eq!(
        (kick.name.as_str(), kick.gain, kick.reverse, kick.mode),
        ("kick", 2.0, true, ChopMode::Gate)
    );
    assert_eq!((kick.attack_seconds, kick.release_seconds, kick.choke), (0.1, 0.2, 3));
    assert_eq!(out[1].name, "snare");
    assert_eq!(out[2], Chop { id: out[2].id, ..Chop::new(3.0, 8.0) });
    assert_eq!(out.iter().map(|c| c.id).collect::<Vec<_>>(), ids);
}

// ---------------------------------------------------------------- commands

#[test]
fn adding_a_sampler_track_lands_the_source_the_track_and_a_pattern_in_one_undo_step() {
    let (mut d, id, src) = sampler_doc();
    let m = d.model();
    let t = m.track(&id).unwrap();
    assert_eq!(t.kind(), TrackKind::Sampler);
    assert_eq!(t.name, "flip");
    assert_eq!(bounds(&t.sampler.as_ref().unwrap().chops), vec![(0.0, 1.0), (1.0, 2.0), (2.0, 8.0)]);
    assert_eq!(m.audio_source(&src.id), Some(&src));
    assert_eq!(m.patterns_of(&id).map(|p| p.name.clone()).collect::<Vec<_>>(), vec!["flip 1"]);
    assert_eq!(d.undo_label(), Some("add sampler track"));
    let tracks = d.model().tracks.len();
    d.undo();
    assert_eq!(d.model().tracks.len(), tracks - 1);
    assert!(d.model().audio_source(&src.id).is_none());
}

#[test]
fn adding_a_sampler_track_with_a_blank_name_calls_it_sample() {
    let src = source();
    let mut d = Document::new(ProjectModel::starter());
    d.apply(
        Command::AddSamplerTrack { name: "  ".into(), sampler: Sampler::new(src.id, vec![]), source: src },
        false,
    )
    .unwrap();
    assert_eq!(d.model().tracks.last().unwrap().name, "sample");
}

#[test]
fn adding_a_sampler_track_refuses_a_mismatched_or_empty_source_or_too_many_chops() {
    let mut d = Document::new(ProjectModel::starter());
    let before = d.model().clone();
    let src = source();
    let add = |source: AudioSource, sampler: Sampler| Command::AddSamplerTrack {
        name: "x".into(),
        source,
        sampler,
    };
    let refused = [
        add(src.clone(), Sampler::new(Uuid::new_v4(), vec![])),
        add(AudioSource { frames: 0, ..src.clone() }, Sampler::new(src.id, vec![])),
        add(AudioSource { sample_rate: 0, ..src.clone() }, Sampler::new(src.id, vec![])),
        add(AudioSource { channels: 0, ..src.clone() }, Sampler::new(src.id, vec![])),
        add(
            src.clone(),
            Sampler::new(src.id, (0..=MAX_CHOPS).map(|_| Chop::new(0.0, 1.0)).collect()),
        ),
    ];
    for command in refused {
        assert_eq!(d.apply(command, false), Err(EditError::InvalidValue));
        assert_eq!(d.model(), &before);
    }
}

#[test]
fn adding_a_sampler_track_sanitizes_its_chops_against_the_source() {
    let src = source();
    let mut d = Document::new(ProjectModel::starter());
    d.apply(
        Command::AddSamplerTrack {
            name: "x".into(),
            sampler: Sampler::new(src.id, vec![Chop::new(6.0, 30.0), Chop::new(9.0, 10.0)]),
            source: src,
        },
        false,
    )
    .unwrap();
    let t = d.model().tracks.last().unwrap();
    assert_eq!(bounds(&t.sampler.as_ref().unwrap().chops), vec![(6.0, 8.0)]);
}

#[test]
fn set_sampler_replaces_the_chops_labelled_edit_sampler() {
    let (mut d, id, src) = sampler_doc();
    let mut next = Sampler::new(src.id, vec![Chop::new(0.5, 99.0)]);
    next.base_pitch = 60;
    d.apply(Command::SetSampler { id, sampler: next }, false).unwrap();
    let s = d.model().track(&id).unwrap().sampler.clone().unwrap();
    assert_eq!((s.base_pitch, bounds(&s.chops)), (60, vec![(0.5, 8.0)]));
    assert_eq!(d.undo_label(), Some("edit sampler"));
}

#[test]
fn set_sampler_refuses_what_it_cannot_apply_and_changes_nothing() {
    let (mut d, id, src) = sampler_doc();
    let melody = d.model().tracks[0].id;
    let before = d.model().clone();
    let ok = Sampler::new(src.id, vec![]);
    let cases = [
        (Command::SetSampler { id: Uuid::new_v4(), sampler: ok.clone() }, EditError::NoSuchTrack),
        (Command::SetSampler { id: melody, sampler: ok.clone() }, EditError::NotASamplerTrack),
        (
            Command::SetSampler { id, sampler: Sampler::new(Uuid::new_v4(), vec![]) },
            EditError::NoSuchAudioSource,
        ),
        (
            Command::SetSampler {
                id,
                sampler: Sampler::new(src.id, (0..=MAX_CHOPS).map(|_| Chop::new(0.0, 1.0)).collect()),
            },
            EditError::InvalidValue,
        ),
    ];
    for (command, err) in cases {
        assert_eq!(d.apply(command, false), Err(err));
        assert_eq!(d.model(), &before);
    }
    assert_eq!(EditError::NotASamplerTrack.to_string(), "not a sampler track");
}

// ---------------------------------------------------------------- normalized and sequence

#[test]
fn kind_prefers_audio_over_sampler_over_drum() {
    let mut t = Track::sampler("x", Sampler::new(Uuid::new_v4(), vec![]));
    assert_eq!(t.kind(), TrackKind::Sampler);
    t.is_drum = true;
    assert_eq!(t.kind(), TrackKind::Sampler);
    t.is_audio = true;
    assert_eq!(t.kind(), TrackKind::Audio);
}

#[test]
fn loading_a_file_resolves_a_sampler_track_that_claims_to_be_something_else() {
    let src = source();
    let mut m = ProjectModel::starter();
    let mut flip = Track::sampler(
        "flip",
        Sampler::new(src.id, vec![Chop::new(7.0, 20.0), Chop::new(30.0, 31.0)]),
    );
    flip.is_drum = true;
    flip.plugin = Some(TrackPlugin { uid: "clap:/x.clap:x".into(), name: "x".into(), state: None });
    flip.amp = Some(AmpRig::default());
    let mut guitar = Track::audio("guitar");
    guitar.sampler = Some(Sampler::new(src.id, vec![]));
    m.tracks.push(flip);
    m.tracks.push(guitar);
    m.audio_sources = vec![src];
    let m = m.normalized();
    let flip = m.tracks.iter().find(|t| t.name == "flip").unwrap();
    assert_eq!(flip.kind(), TrackKind::Sampler);
    assert!(!flip.is_drum && flip.plugin.is_none() && flip.amp.is_none());
    assert_eq!(bounds(&flip.sampler.as_ref().unwrap().chops), vec![(7.0, 8.0)]);
    let guitar = m.tracks.iter().find(|t| t.name == "guitar").unwrap();
    assert_eq!((guitar.kind(), guitar.sampler.is_none()), (TrackKind::Audio, true));
}

#[test]
fn a_sampler_whose_source_is_missing_is_kept_as_written() {
    let mut m = ProjectModel::starter();
    let written = Sampler::new(Uuid::new_v4(), vec![Chop::new(7.0, 20.0)]);
    m.tracks.push(Track::sampler("flip", written.clone()));
    let m = m.normalized();
    assert_eq!(m.tracks.last().unwrap().sampler.as_ref(), Some(&written));
}

#[test]
fn the_sequence_carries_the_chops_and_voice_but_not_the_editing_state() {
    let (mut d, id, src) = sampler_doc();
    let mut with_analysis = d.model().track(&id).unwrap().sampler.clone().unwrap();
    with_analysis.analysis = Some(SampleAnalysis { bpm: Some(120.0), ..SampleAnalysis::default() });
    with_analysis.voice = SamplerVoice::Poly;
    with_analysis.region = Some(region(1.0, 4.0));
    with_analysis.chopper = Some(Chopper::Equal { count: 4 });
    d.apply(Command::SetSampler { id, sampler: with_analysis.clone() }, false).unwrap();
    let pattern = d.model().patterns_of(&id).next().unwrap().id;
    let seq = Sequence::from_pattern(d.model(), &pattern, 0).unwrap();
    let track = seq.tracks.iter().find(|t| t.id == id).unwrap();
    let carried = track.sampler.as_ref().unwrap();
    assert_eq!(carried.source_id, src.id);
    assert_eq!(carried.chops, with_analysis.chops);
    assert_eq!(carried.voice, SamplerVoice::Poly);
    assert_eq!((carried.analysis.as_ref(), carried.region, carried.chopper), (None, None, None));
    assert!(seq.tracks.iter().filter(|t| t.id != id).all(|t| t.sampler.is_none()));
}
