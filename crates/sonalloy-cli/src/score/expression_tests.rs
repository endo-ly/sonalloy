use super::*;
use serde_json::json;
use time::{Quarter, q};

fn document() -> Value {
    json!({
        "meta":{"title":"Expression", "bpm":120, "meter":"4/4", "tuning":"12edo", "gate":"1/2"},
        "form":[{"section":"a", "bars":1, "tonic":"C"}],
        "sections":{"a":{"lead":{"instrument":"lead", "melody":"line"}}},
        "melodies":{"line":["1@80~ 3@115~ 5 - | 6 - . . | . . . . | . . . ."]},
        "instruments":{"lead":{"external":true, "gate":"3/4"}}
    })
}

fn notes(plan: &ScorePlan, id: &str) -> Vec<(Quarter, Quarter, u8, u8)> {
    plan.lanes
        .iter()
        .filter(|lane| lane.id == id)
        .flat_map(|lane| &lane.pattern.events)
        .filter_map(|event| match event {
            MusicalEvent::Note {
                tick,
                duration_ticks,
                note,
                velocity,
            } => Some((
                q(
                    i64::try_from(*tick).unwrap(),
                    i64::from(plan.ticks_per_beat),
                ),
                q(
                    i64::try_from(*duration_ticks).unwrap(),
                    i64::from(plan.ticks_per_beat),
                ),
                *note,
                *velocity,
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn note_attributes_gate_and_legato_resolve_on_the_exact_timeline() {
    let doc = document();
    let plan = compile(&doc.to_string()).unwrap();
    assert_eq!(
        notes(&plan, "lead"),
        [
            (q(0, 1), q(17, 64), 60, 80),
            (q(1, 4), q(17, 64), 64, 115),
            (q(1, 2), q(3, 8), 67, 100),
            (q(1, 1), q(3, 8), 69, 100),
        ]
    );
    for line in [
        "1~ . . . | . . . . | . . . . | . . . .",
        ". . . . | . . . . | . . . . | . . . 1~",
    ] {
        let mut invalid = doc.clone();
        invalid["melodies"]["line"] = json!([line]);
        assert!(compile(&invalid.to_string()).is_err());
    }
    let mut short_gate = doc;
    short_gate["instruments"]["lead"]["gate"] = json!("1/100");
    short_gate["melodies"]["line"] = json!(["1~ 3 . . | . . . . | . . . . | . . . ."]);
    let connected = notes(&compile(&short_gate.to_string()).unwrap(), "lead");
    assert!(connected[0].0.clone() + &connected[0].1 > connected[1].0);
    assert!(connected[0].0.clone() + &connected[0].1 < &connected[1].0 + &connected[1].1);
}

#[test]
fn bass_degrees_follow_tonic_or_slash_bass_after_choices_and_overrides() {
    let mut doc = document();
    doc["sections"]["a"] = json!({"chords":["1:m/5"], "bass":{"pattern":"line"},
        "bass_overrides":[{"bar":1,"parts":{"lead":"T - 5@127 . | b3, - . . | . . . . | . . . ."}}]});
    doc["bass"] = json!({"line":{"parts":{"lead":"1 - - - | . . . . | . . . . | . . . ."}, "choices":{"T":{"odd":"1@1", "even":"3"}}}});
    let tonic = compile(&doc.to_string()).unwrap();
    assert_eq!(
        notes(&tonic, "lead"),
        [
            (q(0, 1), q(3, 8), 36, 1),
            (q(1, 2), q(3, 16), 43, 127),
            (q(1, 1), q(3, 8), 27, 100)
        ]
    );
    doc["bass"]["line"]["relative_to"] = json!("chord_bass");
    let chord = compile(&doc.to_string()).unwrap();
    assert_eq!(
        notes(&chord, "lead")
            .iter()
            .map(|n| n.2)
            .collect::<Vec<_>>(),
        [31, 38, 22]
    );
    doc["bass"]["line"]["relative_to"] = json!("other");
    assert!(compile(&doc.to_string()).is_err());

    doc["meta"]["gate"] = json!(1);
    doc["instruments"]["lead"]["gate"] = json!(1);
    doc["form"] =
        json!([{"section":"a","bars":2,"tonic":"C"},{"section":"b","bars":1,"tonic":"D"}]);
    doc["sections"] = json!({"a":{"velocity":84,"chords":["1:maj","4:maj/6"],"bass":{"pattern":"line"},
        "bass_overrides":[{"bar":2,"parts":{"lead":"5 - . . | . . . . | 1' - . . | . . . ."}}],
        "placements":[
            {"instrument":"lead","degree":"6,,,","at":{"bar":1,"step":5},"duration_quarters":"1/2"},
            {"instrument":"lead","degree":"#1,,","at":{"bar":2,"step":5},"duration_quarters":1}]},
        "b":{"chords":["5:7/7"],"placements":[{"instrument":"lead","degree":"1","at":{"bar":1,"step":1},"duration_quarters":1}]}});
    doc["bass"] = json!({"line":{"relative_to":"chord_bass","parts":{"lead":"1 - 5 - | T . . . | . . . . | . . . ."},"choices":{"T":{"odd":".","even":"1'"}}}});
    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "lead"),
        [
            (q(0, 1), q(1, 2), 36, 84),
            (q(1, 2), q(1, 2), 43, 84),
            (q(1, 1), q(1, 2), 33, 84),
            (q(4, 1), q(1, 2), 40, 84),
            (q(5, 1), q(1, 1), 37, 84),
            (q(6, 1), q(1, 2), 45, 84),
            (q(8, 1), q(1, 1), 62, 100),
        ]
    );
}

#[test]
fn automatic_accompaniment_preserves_compound_chord_intervals() {
    for (quality, expected) in [
        ("9", vec![48, 52, 55, 58, 62]),
        ("11", vec![48, 52, 55, 58, 62, 65]),
        ("13", vec![48, 52, 55, 58, 62, 65, 69]),
        ("7(b9,#11,omit5)", vec![48, 52, 58, 61, 66]),
        ("(1,b3,5,b7,#11)", vec![48, 51, 55, 58, 66]),
    ] {
        let mut doc = document();
        doc["sections"]["a"] = json!({"chords":[format!("1:{quality}")],
            "comp":{"lead":"pulse"},"pad":{"instrument":"pad","pattern":"pulse"},
            "arp":{"instrument":"arp","pattern":"up"}});
        for id in ["pad", "arp"] {
            doc["instruments"][id] = json!({"external":true});
        }
        doc["comp"] = json!({"pulse":"x - - - | . . . . | . . . . | . . . ."});
        let cells: Vec<_> = (0..16)
            .map(|index| {
                if index < expected.len() {
                    index.to_string()
                } else {
                    ".".into()
                }
            })
            .collect();
        let order = cells
            .chunks(4)
            .map(|beat| beat.join(" "))
            .collect::<Vec<_>>()
            .join(" | ");
        doc["arps"] = json!({"up":{"order":order}});

        let plan = compile(&doc.to_string()).unwrap();

        for id in ["lead", "pad", "arp"] {
            assert_eq!(
                notes(&plan, id)
                    .iter()
                    .map(|note| note.2)
                    .collect::<Vec<_>>(),
                expected,
                "{quality}: {id}"
            );
        }
    }
}

#[test]
fn voicing_precedence_is_shared_by_comp_pad_arp_and_placements() {
    let mut doc = document();
    doc["instruments"]["lead"]["voicing"] = json!(["3", "7"]);
    doc["sections"]["a"] = json!({"chords":["1:m9"],
        "comp":{"lead":{"pattern":"pulse", "voicing":["9'"]}},
        "pad":{"instrument":"pad", "pattern":"pulse"},
        "arp":{"instrument":"arp", "pattern":"up", "voicing":["3", "9"]},
        "placements":[{"instrument":"lead", "at":{"bar":1,"step":9}, "chord":"1:m9", "voicing":["5,"], "duration_quarters":1}]});
    doc["instruments"]["pad"] = json!({"external":true, "voicing":["3", "7"]});
    doc["instruments"]["arp"] = json!({"external":true});
    doc["comp"] = json!({"pulse":"x - - - | . . . . | . . . . | . . . ."});
    doc["arps"] = json!({"up":{"order":"0 1 . . | . . . . | . . . . | . . . ."}});
    let plan = compile(&doc.to_string()).unwrap();
    assert_eq!(
        notes(&plan, "lead").iter().map(|n| n.2).collect::<Vec<_>>(),
        [74, 43]
    );
    assert_eq!(
        notes(&plan, "pad").iter().map(|n| n.2).collect::<Vec<_>>(),
        [51, 58]
    );
    assert_eq!(
        notes(&plan, "arp").iter().map(|n| n.2).collect::<Vec<_>>(),
        [51, 62]
    );
    doc["sections"]["a"]["comp"]["lead"] = json!("pulse");
    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "lead")
            .iter()
            .map(|n| n.2)
            .collect::<Vec<_>>(),
        [51, 58, 43]
    );
    let placement = &mut doc["sections"]["a"]["placements"][0];
    placement.as_object_mut().unwrap().remove("chord");
    placement["degree"] = json!("1");
    assert!(
        compile(&doc.to_string()).unwrap_err().diagnostics[0]
            .message
            .contains("voicing fields require chord")
    );
    doc["sections"]["a"] = json!({"chords":["1:maj/3"],
        "comp":{"lead":{"pattern":"pulse","voicing":["1,"]}}});
    let invalid = compile(&doc.to_string()).unwrap_err();
    assert!(
        invalid.diagnostics[0]
            .message
            .contains("below the slash bass")
    );
    assert_eq!(
        invalid.diagnostics[0].path.as_deref(),
        Some("sections.a.comp.lead")
    );
    doc["sections"]["a"]["comp"]["lead"]["voicing"] = json!(["1"]);
    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "lead")
            .iter()
            .map(|note| note.2)
            .collect::<Vec<_>>(),
        [52, 60]
    );
}

#[test]
fn counter_rests_and_ties_respect_lead_silence_and_instrument_gate() {
    let mut doc = document();
    doc["melodies"]["line"] = json!(["1 - - - | . . . . | 1 - - - | . . . ."]);
    doc["sections"]["a"]["counter"] = json!({"instrument":"counter", "pattern":"answer"});
    doc["instruments"]["counter"] = json!({"external":true});
    doc["counters"] = json!({"answer":{"when":"lead_silent_full_beat", "cell":". 3@90 - 5"}});
    let plan = compile(&doc.to_string()).unwrap();
    assert_eq!(
        notes(&plan, "counter"),
        [
            (q(5, 4), q(1, 4), 64, 90),
            (q(7, 4), q(1, 8), 67, 100),
            (q(13, 4), q(1, 4), 64, 90),
            (q(15, 4), q(1, 8), 67, 100)
        ]
    );
    doc["counters"]["answer"]["cell"] = json!("- 3");
    assert!(compile(&doc.to_string()).is_err());

    doc["counters"]["answer"]["cell"] = json!("3");
    doc["melodies"]["line"] = json!(["1 - - - | - - - - | . . . . | . . . ."]);
    doc["instruments"]["lead"]["gate"] = json!("1/4");
    doc["instruments"]["layer"] = json!({"external":true,"gate":1});
    doc["sections"]["a"]["lead"]["layers"] = json!([{"instrument":"layer"}]);
    let layered = compile(&doc.to_string()).unwrap();
    assert_eq!(notes(&layered, "lead")[0].1, q(1, 2));
    assert_eq!(notes(&layered, "layer")[0].1, q(2, 1));
    assert_eq!(
        notes(&layered, "counter")
            .iter()
            .map(|n| n.0.clone())
            .collect::<Vec<_>>(),
        [q(2, 1), q(3, 1)]
    );
}

#[test]
fn swing_warps_all_grid_generators_but_keeps_explicit_events_and_tuplets() {
    let mut doc = document();
    doc["meta"]["swing"] = json!({"subdivision":8,"ratio":"2/3"});
    doc["meta"]["gate"] = json!(1);
    doc["instruments"]["lead"]["gate"] = json!(1);
    doc["instruments"]["drum"] = json!({"external":true,"trigger_note_midi":36,"gate":"1/2"});
    doc["sections"]["a"]["drums"] = json!("beat");
    doc["drums"] = json!({"beat":{"parts":{"drum":"xxxx | .... | .... | ...."}}});
    doc["melodies"]["line"] = json!(["1 - 3 - | [1 2 3] | . . . . | . . . ."]);
    doc["sections"]["a"]["placements"] = json!([{"instrument":"lead", "degree":"5", "at":{"bar":1,"offset_quarters":"1/2"},"duration_quarters":"1/2"}]);
    let plan = compile(&doc.to_string()).unwrap();
    let lead = notes(&plan, "lead");
    assert!(lead.contains(&(q(0, 1), q(2, 3), 60, 100)));
    assert!(lead.contains(&(q(2, 3), q(1, 3), 64, 100)));
    assert!(lead.contains(&(q(1, 2), q(1, 2), 67, 100)));
    let tuplets: Vec<_> = lead
        .iter()
        .filter(|n| n.0 >= q(1, 1) && n.0 < q(2, 1))
        .map(|n| (n.0.clone(), n.1.clone()))
        .collect();
    assert_eq!(
        tuplets,
        [(q(1, 1), q(1, 3)), (q(4, 3), q(1, 3)), (q(5, 3), q(1, 3))]
    );
    assert_eq!(
        notes(&plan, "drum")
            .iter()
            .map(|n| (n.0.clone(), n.1.clone()))
            .collect::<Vec<_>>(),
        [
            (q(0, 1), q(1, 6)),
            (q(1, 3), q(1, 6)),
            (q(2, 3), q(1, 12)),
            (q(5, 6), q(1, 12))
        ]
    );
    assert_eq!(plan.length_ticks, 4 * u64::from(plan.ticks_per_beat));
}
#[test]
fn swing_preserves_meters_and_form_overrides_without_affecting_equal_timing() {
    for (meter, beats, slots) in [("3/4", 3, 4), ("6/8", 6, 2), ("5/8", 5, 2), ("7/16", 7, 1)] {
        let mut doc = document();
        doc["meta"]["meter"] = json!(meter);
        let pattern = (0..beats)
            .map(|_| vec!["1"; slots].join(" "))
            .collect::<Vec<_>>()
            .join(" | ");
        doc["melodies"]["line"] = json!([pattern]);
        let straight = compile(&doc.to_string()).unwrap();
        for subdivision in [8, 16] {
            doc["meta"]["swing"] = json!({"subdivision":subdivision,"ratio":"1/2"});
            assert_eq!(
                notes(&compile(&doc.to_string()).unwrap(), "lead"),
                notes(&straight, "lead")
            );
            doc["meta"]["swing"]["ratio"] = json!("2/3");
            let swung = compile(&doc.to_string()).unwrap();
            assert_eq!(
                q(
                    i64::try_from(swung.length_ticks).unwrap(),
                    i64::from(swung.ticks_per_beat)
                ),
                q(
                    i64::try_from(straight.length_ticks).unwrap(),
                    i64::from(straight.ticks_per_beat)
                )
            );
            let end = q(
                i64::try_from(swung.length_ticks).unwrap(),
                i64::from(swung.ticks_per_beat),
            );
            assert!(
                notes(&swung, "lead")
                    .iter()
                    .all(|(at, duration, ..)| at + duration <= end)
            );
        }
    }
    let mut doc = document();
    doc["melodies"]["line"] = json!(["1 - 3 - | . . . . | . . . . | . . . ."]);
    doc["meta"]["swing"] = json!({"subdivision":8,"ratio":"2/3"});
    doc["form"] = json!([
        {"section":"a","bars":1,"tonic":"C"},
        {"section":"a","bars":1,"tonic":"C","swing":{"subdivision":8,"ratio":"1/2"}},
        {"section":"a","bars":1,"tonic":"C"}
    ]);
    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "lead")
            .iter()
            .map(|n| n.0.clone())
            .collect::<Vec<_>>(),
        [q(0, 1), q(2, 3), q(4, 1), q(9, 2), q(8, 1), q(26, 3)]
    );
    for invalid in [
        json!({"subdivision":4,"ratio":"2/3"}),
        json!({"subdivision":8,"ratio":0}),
        json!({"subdivision":8,"ratio":1}),
    ] {
        doc["meta"]["swing"] = invalid;
        assert!(compile(&doc.to_string()).is_err());
    }
}

#[test]
fn swing_gate_and_legato_are_shared_by_the_accompaniment_generators() {
    let mut doc = document();
    doc["meta"]["swing"] = json!({"subdivision":16,"ratio":"2/3"});
    doc["sections"]["a"] = json!({"chords":["1:7"],"bass":{"pattern":"pulse"},
        "comp":{"comp":"pulse"},"pad":{"instrument":"pad","pattern":"pulse"},
        "arp":{"instrument":"arp","pattern":"pulse"},"counter":{"instrument":"counter","pattern":"pulse"}});
    for id in ["comp", "pad", "arp", "counter"] {
        doc["instruments"][id] = json!({"external":true});
    }
    doc["bass"] = json!({"pulse":{"parts":{"lead":"1~ 3 - . | . . . . | . . . . | . . . ."}}});
    doc["comp"] = json!({"pulse":"x x . . | . . . . | . . . . | . . . ."});
    doc["arps"] = json!({"pulse":{"order":"0 1 . . | . . . . | . . . . | . . . ."}});
    doc["counters"] = json!({"pulse":{"when":"lead_silent_full_beat","cell":"1 3 - ."}});
    let plan = compile(&doc.to_string()).unwrap();
    assert_eq!(notes(&plan, "lead")[0].1, q(1, 3) + q(1, 64));
    assert_eq!(notes(&plan, "lead")[1].0, q(1, 3));
    for id in ["comp", "pad", "arp"] {
        let events = notes(&plan, id);
        assert!(
            events.iter().any(|n| n.0 == q(0, 1) && n.1 == q(1, 6)),
            "{id}"
        );
        assert!(
            events.iter().any(|n| n.0 == q(1, 3) && n.1 == q(1, 12)),
            "{id}"
        );
    }
    assert_eq!(notes(&plan, "counter")[1].0, q(1, 3));
    assert_eq!(notes(&plan, "counter")[1].1, q(1, 4));
}
