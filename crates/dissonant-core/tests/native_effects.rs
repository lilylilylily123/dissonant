//! The stock effects as the project file and the command layer see them: a chain that mixes a
//! native effect with a CLAP plugin has to survive a round trip intact, and a brand-new project
//! has to be mixable with no plugin installed at all.

use dissonant_core::effects::sanitize_params;
use dissonant_core::{Command, Document, EditError, Insert, InsertTarget, NativeKind, ProjectModel};

/// A plugin uid of the shape the scanner mints, so nothing here can be mistaken for native.
const PLUGIN_UID: &str = "clap:/Library/Audio/Plug-Ins/CLAP/Pro-Q.clap:com.fabfilter.proq3";

fn round_trip(m: &ProjectModel) -> ProjectModel {
    ProjectModel::from_json(&m.to_json().expect("serialize")).expect("deserialize")
}

#[test]
fn a_chain_mixing_a_native_effect_and_a_plugin_survives_a_round_trip() {
    let mut m = ProjectModel::starter();
    let track = m.tracks[0].id;

    let mut eq = Insert::new(NativeKind::Eq.uid(), "EQ");
    eq.params.insert(100, 180.0);
    eq.params.insert(104, -4.5);
    let mut plugin = Insert::new(PLUGIN_UID, "Pro-Q");
    plugin.state = Some("cHJvcQ==".into());
    plugin.enabled = false;
    let mut delay = Insert::new(NativeKind::Delay.uid(), "delay");
    delay.params.insert(500, 8.0);

    m.track_mut(&track).unwrap().inserts = vec![eq.clone(), plugin.clone(), delay.clone()];
    m.master_inserts = vec![Insert::new(NativeKind::Limiter.uid(), "limiter")];

    let back = round_trip(&m);
    let chain = &back.track(&track).unwrap().inserts;
    assert_eq!(chain, &vec![eq, plugin, delay], "the chain did not round trip");
    assert_eq!(back.master_inserts, m.master_inserts);

    // A slot nobody has touched carries no map at all, so adding stock effects does not grow
    // every saved file.
    let json = m.to_json().unwrap();
    assert!(
        json.contains(r#""uid": "native:eq""#),
        "the native uid is not what was written: {json}"
    );
    assert!(
        !json.contains(r#""params": {}"#),
        "an untouched effect wrote an empty parameter map"
    );
}

#[test]
fn loading_drops_parameters_that_belong_to_no_knob_and_holds_the_rest_in_range() {
    let mut m = ProjectModel::starter();
    let track = m.tracks[0].id;
    let mut comp = Insert::new(NativeKind::Compressor.uid(), "compressor");
    // Out of range, belonging to another effect, and not a number at all.
    comp.params.insert(201, 9_000.0);
    comp.params.insert(500, 4.0);
    comp.params.insert(200, -999.0);
    // A plugin has no parameters of ours, however a file was edited.
    let mut plugin = Insert::new(PLUGIN_UID, "Pro-Q");
    plugin.params.insert(100, 50.0);
    m.track_mut(&track).unwrap().inserts = vec![comp, plugin];

    let back = round_trip(&m);
    let chain = &back.track(&track).unwrap().inserts;
    assert_eq!(chain[0].params.get(&201), Some(&20.0), "ratio was not clamped");
    assert_eq!(chain[0].params.get(&200), Some(&-60.0));
    assert_eq!(chain[0].params.get(&500), None, "a foreign id survived");
    assert!(chain[1].params.is_empty(), "a plugin kept native parameters");
}

#[test]
fn a_new_project_can_be_mixed_with_no_plugin_installed() {
    // The acceptance case, end to end through the command layer: high-pass a pad, compress a
    // drum track, and put a tempo-synced delay and a reverb on the master.
    let mut d = Document::new(ProjectModel::starter());
    let pad = d.model().tracks[0].id;
    let drums = d.model().tracks[1].id;

    let adds: [(InsertTarget, NativeKind); 4] = [
        (InsertTarget::Track(pad), NativeKind::Eq),
        (InsertTarget::Track(drums), NativeKind::Compressor),
        (InsertTarget::Master, NativeKind::Delay),
        (InsertTarget::Master, NativeKind::Reverb),
    ];
    for (target, kind) in adds {
        d.apply(
            Command::AddInsert {
                target,
                uid: kind.uid(),
                name: kind.display_name().into(),
                index: None,
            },
            false,
        )
        .expect("add a stock effect");
    }

    let eq = d.model().track(&pad).unwrap().inserts[0].id;
    // A knob drag: a run of transient moves that ends as exactly one undo step.
    for hz in [120.0, 160.0, 200.0] {
        d.apply_labeled(
            Command::SetInsertParam {
                target: InsertTarget::Track(pad),
                insert_id: eq,
                param_id: 100,
                value: hz,
            },
            true,
            Some("EQ settings".into()),
        )
        .expect("drag the high-pass");
    }
    d.commit_gesture();
    assert_eq!(
        d.model().track(&pad).unwrap().inserts[0].params.get(&100),
        Some(&200.0)
    );
    assert!(d.undo(), "the drag recorded no undo step");
    assert!(
        d.model().track(&pad).unwrap().inserts[0].params.is_empty(),
        "a run of transient parameter moves was not one undo step"
    );

    // The delay's division is a choice, so it rounds; the mix is a continuum, so it does not.
    let delay = d.model().master_inserts[0].id;
    d.apply(
        Command::SetInsertParam {
            target: InsertTarget::Master,
            insert_id: delay,
            param_id: 500,
            value: 7.6,
        },
        false,
    )
    .unwrap();
    assert_eq!(d.model().master_inserts[0].params.get(&500), Some(&8.0));

    // And the whole mix survives being saved and opened.
    let back = round_trip(d.model());
    assert_eq!(back.track(&drums).unwrap().inserts.len(), 1);
    assert_eq!(back.master_inserts.len(), 2);
}

#[test]
fn a_parameter_command_refuses_a_plugin_a_stranger_id_and_a_value_that_is_not_a_number() {
    let mut m = ProjectModel::starter();
    let track = m.tracks[0].id;
    m.track_mut(&track).unwrap().inserts = vec![
        Insert::new(PLUGIN_UID, "Pro-Q"),
        Insert::new(NativeKind::Reverb.uid(), "reverb"),
    ];
    let mut d = Document::new(m);
    let target = InsertTarget::Track(track);
    let (plugin, reverb) = {
        let chain = &d.model().track(&track).unwrap().inserts;
        (chain[0].id, chain[1].id)
    };

    let refuse = |d: &mut Document, insert_id, param_id, value| {
        d.apply(
            Command::SetInsertParam {
                target,
                insert_id,
                param_id,
                value,
            },
            false,
        )
    };
    assert_eq!(
        refuse(&mut d, plugin, 600, 0.5),
        Err(EditError::NotANativeEffect)
    );
    assert_eq!(refuse(&mut d, reverb, 999, 0.5), Err(EditError::NoSuchParam));
    assert_eq!(
        refuse(&mut d, reverb, 600, f64::NAN),
        Err(EditError::InvalidValue)
    );
    assert!(!d.can_undo(), "a refused parameter move recorded an undo step");
}

#[test]
fn every_effects_catalogue_entry_is_addressable_by_its_uid() {
    for kind in NativeKind::ALL {
        let insert = Insert::new(kind.uid(), kind.display_name());
        assert_eq!(insert.native_kind(), Some(kind));
        // Defaults are never stored, so `value_of` is what the engine and the UI both read.
        for param in kind.params() {
            assert_eq!(kind.value_of(&insert.params, param.id), param.default);
        }
        let mut params = insert.params.clone();
        params.insert(param_of(kind), 1e9);
        sanitize_params(kind, &mut params);
        let id = param_of(kind);
        assert_eq!(params[&id], kind.param(id).unwrap().max);
    }
}

/// The first parameter of an effect, whatever it happens to be.
fn param_of(kind: NativeKind) -> u32 {
    kind.params()[0].id
}
