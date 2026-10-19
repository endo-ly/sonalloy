use super::*;
use serde_json::json;
use time::{Quarter, q};

fn document() -> Value {
    json!({
        "meta":{"title":"Expression", "bpm":120, "meter":"4/4", "tuning":"12edo", "gate":"1/2"},
        "form":[{"section":"a", "bars":1, "tonic":"C"}],
        "sections":{"a":{"lines":{"main":{"instrument":"lead", "melody":"line"}}}},
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
fn bass_chord_tone_tokens_follow_the_active_chord() {
    let mut doc = document();
    doc["sections"]["a"] = json!({
        "chords": ["1:maj7"],
        "lines": {"main":{"instrument":"lead","melody":"line","register":"bass"}}
    });
    doc["melodies"]["line"] = json!(["c1 c3 c5 c7 | . . . . | . . . . | . . . ."]);
    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "lead")
            .iter()
            .map(|note| note.2)
            .collect::<Vec<_>>(),
        [36, 40, 43, 47]
    );

    doc["sections"]["a"]["chords"] = json!(["1:m7"]);
    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "lead")
            .iter()
            .map(|note| note.2)
            .collect::<Vec<_>>(),
        [36, 39, 43, 46]
    );

    doc["sections"]["a"]["chords"] = json!(["5:maj7/3"]);
    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "lead")
            .iter()
            .map(|note| note.2)
            .collect::<Vec<_>>(),
        [31, 35, 38, 42]
    );

    doc["sections"]["a"]["chords"] = json!(["1:maj"]);
    assert!(compile(&doc.to_string()).is_err());
}

#[test]
fn placements_accept_chord_tone_pitch_tokens() {
    let mut doc = document();
    doc["sections"]["a"] = json!({
        "chords": ["1:maj"],
        "placements": [{
            "instrument":"lead",
            "at":{"bar":1,"step":1},
            "degree":"c3",
            "duration_steps":4
        }]
    });

    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "lead")
            .iter()
            .map(|note| note.2)
            .collect::<Vec<_>>(),
        [64]
    );
}

#[test]
fn automatic_voicing_limits_upper_tones_and_explicit_voicing_keeps_large_chords() {
    let mut doc = document();
    doc["instruments"] = json!({
        "keys": {
            "external": true,
            "voicing_floor_midi": 0,
            "voicing_ceiling_midi": 127
        }
    });
    doc["sections"]["a"] = json!({
        "chords": ["1:(1,2,b3,3,4,b5,5,b6,6,b7,7)"],
        "comp": {"keys": "hold"}
    });

    let error = compile(&doc.to_string()).unwrap_err();
    assert_eq!(
        error.diagnostics[0].message,
        "automatic voicing supports at most 6 upper tones; specify an explicit voicing"
    );
    assert_eq!(
        error.diagnostics[0].path.as_deref(),
        Some("sections.a.comp.keys")
    );

    doc["sections"]["a"]["chords"] = json!(["1:(1,2,3,4,5,6,7,9,11,13)"]);
    doc["instruments"]["keys"]["voicing"] =
        json!(["1", "2", "3", "4", "5", "6", "7", "9", "11", "13"]);
    assert_eq!(notes(&compile(&doc.to_string()).unwrap(), "keys").len(), 10);
}

#[test]
fn bass_chord_tone_root_must_fit_the_instrument_range() {
    let mut doc = document();
    doc["sections"]["a"] = json!({
        "chords": ["1:maj"],
        "lines": {"main":{"instrument":"lead","melody":"line","register":"bass"}}
    });
    doc["melodies"]["line"] = json!(["c1 . . . | . . . . | . . . . | . . . ."]);
    doc["instruments"]["lead"]["bass_floor_midi"] = json!(28);
    doc["instruments"]["lead"]["bass_ceiling_midi"] = json!(34);

    let error = compile(&doc.to_string()).expect_err("C2 exceeds the bass ceiling");

    assert_eq!(
        error.diagnostics[0].message,
        "bass range contains no matching pitch"
    );
    assert_eq!(
        error.diagnostics[0].path.as_deref(),
        Some("melodies.line[0]")
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
    doc["sections"]["a"]["counter"] =
        json!({"instrument":"counter", "pattern":"answer", "follows":["main"]});
    doc["instruments"]["counter"] = json!({"external":true});
    doc["counters"] = json!({"answer":{"when":"silent_full_beat", "cell":". 3 - 5"}});
    let plan = compile(&doc.to_string()).unwrap();
    assert_eq!(
        notes(&plan, "counter"),
        [
            (q(5, 4), q(1, 4), 64, 100),
            (q(7, 4), q(1, 8), 67, 100),
            (q(13, 4), q(1, 4), 64, 100),
            (q(15, 4), q(1, 8), 67, 100)
        ]
    );
    doc["counters"]["answer"]["cell"] = json!("- 3");
    assert!(compile(&doc.to_string()).is_err());

    doc["counters"]["answer"]["cell"] = json!("3");
    doc["melodies"]["line"] = json!(["1 - - - | - - - - | . . . . | . . . ."]);
    doc["instruments"]["lead"]["gate"] = json!("1/4");
    doc["instruments"]["layer"] = json!({"external":true,"gate":1});
    doc["sections"]["a"]["lines"]["double"] = json!({"instrument":"layer","melody":"line"});
    doc["sections"]["a"]["counter"]["follows"] = json!(["main", "double"]);
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
    doc["sections"]["a"] = json!({"chords":["1:7"],
        "comp":{"comp":"pulse"},"pad":{"instrument":"pad","pattern":"pulse"},
        "arp":{"instrument":"arp","pattern":"pulse"},"counter":{"instrument":"counter","pattern":"pulse","follows":["silent"]},"lines":{"main":{"instrument":"lead","melody":"pulse","register":"bass"},"silent":{"instrument":"lead","melody":"silent"}}});
    doc["melodies"]["silent"] = json!([".:16"]);
    for id in ["comp", "pad", "arp", "counter"] {
        doc["instruments"][id] = json!({"external":true});
    }
    doc["melodies"]["pulse"] = json!(["1~ 3 - . | . . . . | . . . . | . . . ."]);
    doc["comp"] = json!({"pulse":"x x . . | . . . . | . . . . | . . . ."});
    doc["arps"] = json!({"pulse":{"order":"0 1 . . | . . . . | . . . . | . . . ."}});
    doc["counters"] = json!({"pulse":{"when":"silent_full_beat","cell":"1 3 - ."}});
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

#[test]
fn bar_conditions_select_drum_overrides() {
    let mut doc = document();
    doc["form"] = json!([{"section":"a","bars":4,"tonic":"C"}]);
    doc["sections"]["a"] = json!({
        "chords":["1:maj","1:maj","1:maj","1:maj"],
        "drums":"beat"});
    doc["instruments"]["drum"] = json!({"external":true,"trigger_note_midi":36});
    doc["drums"] = json!({"beat":{"parts":{"drum":"x...|....|....|...."},
        "overrides":[{"when":{"even":true},"parts":{"drum":"X...|....|....|...."}}]}});
    let plan = compile(&doc.to_string()).unwrap();
    assert_eq!(
        notes(&plan, "drum")
            .iter()
            .map(|n| (n.0.clone(), n.2, n.3))
            .collect::<Vec<_>>(),
        [
            (q(0, 1), 36, 96),
            (q(4, 1), 36, 120),
            (q(8, 1), 36, 96),
            (q(12, 1), 36, 120)
        ]
    );
}

#[test]
fn pad_hold_is_builtin_and_refollows_chord_changes() {
    let mut doc = document();
    doc["sections"]["a"] =
        json!({"chords":["1:maj 4:maj"], "pad":{"instrument":"pad","pattern":"hold"}});
    doc["instruments"]["pad"] = json!({"external":true});
    let plan = compile(&doc.to_string()).unwrap();
    assert_eq!(
        notes(&plan, "pad"),
        [
            (q(0, 1), q(1, 1), 48, 100),
            (q(0, 1), q(1, 1), 52, 100),
            (q(0, 1), q(1, 1), 55, 100),
            (q(2, 1), q(1, 1), 53, 100),
            (q(2, 1), q(1, 1), 57, 100),
            (q(2, 1), q(1, 1), 60, 100)
        ]
    );
    doc["form"] =
        json!([{"section":"a","bars":4,"tonic":"C"},{"section":"a","bars":4,"tonic":"C"}]);
    doc["sections"]["a"]["chords"] = json!(["1:maj", "5:maj", "NC", "1:maj"]);
    let reset = compile(&doc.to_string()).unwrap();
    for at in [q(0, 1), q(12, 1), q(16, 1), q(28, 1)] {
        assert_eq!(
            notes(&reset, "pad")
                .into_iter()
                .filter(|n| n.0 == at)
                .map(|n| n.2)
                .collect::<Vec<_>>(),
            [48, 52, 55]
        );
    }
    doc["comp"] = json!({"hold":"x - - - | . . . . | . . . . | . . . ."});
    let error = compile(&doc.to_string()).unwrap_err();
    assert_eq!(error.diagnostics[0].path.as_deref(), Some("comp.hold"));
}

#[test]
fn pattern_lengths_assertions_and_swing_share_the_section_timeline() {
    let mut doc = document();
    let expected = notes(&compile(&doc.to_string()).unwrap(), "lead");
    doc["melodies"]["line"] = json!(["1@80~ 3@115~ 5:2 | 6:2 .:10"]);
    assert_eq!(notes(&compile(&doc.to_string()).unwrap(), "lead"), expected);

    doc["melodies"]["line"] = json!(["1 | .:15"]);
    let error = compile(&doc.to_string()).unwrap_err();
    assert_eq!(
        error.diagnostics[0].path.as_deref(),
        Some("melodies.line[0]")
    );
    assert_eq!(
        error.diagnostics[0].message,
        "boundary assertion is not at the requested boundary"
    );
    let detail: Value =
        serde_json::from_str(error.diagnostics[0].detail.as_deref().unwrap()).unwrap();
    assert_eq!(detail["actual"], "|");
    assert!(detail["column"].as_u64().unwrap() > 0);

    doc["form"][0]["bars"] = json!(2);
    doc["instruments"]["lead"]["gate"] = json!(1);
    doc["meta"]["swing"] = json!({"subdivision":8,"ratio":"2/3"});
    doc["melodies"]["line"] = json!(["1:18 3:14"]);
    let plan = compile(&doc.to_string()).unwrap();
    assert_eq!(
        notes(&plan, "lead"),
        [(q(0, 1), q(14, 3), 60, 100), (q(14, 3), q(10, 3), 64, 100)]
    );

    doc["meta"]["meter"] = json!("3/4");
    doc["form"][0]["bars"] = json!(1);
    doc["sections"]["a"]["lines"]["main"]["loop"] = json!(true);
    doc["melodies"]["line"] = json!(["[1 3]:6"]);
    let plan = compile(&doc.to_string()).unwrap();
    assert_eq!(
        notes(&plan, "lead")
            .iter()
            .map(|n| n.0.clone())
            .collect::<Vec<_>>(),
        [q(0, 1), q(5, 6), q(5, 3), q(7, 3)]
    );
    doc["melodies"]["line"] = json!(["[1 3] .:2"]);
    let error = compile(&doc.to_string()).unwrap_err();
    assert_eq!(
        error.diagnostics[0].path.as_deref(),
        Some("melodies.line[0]")
    );
    assert_eq!(
        error.diagnostics[0].message,
        "tuplet must start at an interval boundary"
    );
    doc["melodies"]["line"] = json!(["1:5"]);
    let error = compile(&doc.to_string()).unwrap_err();
    assert_eq!(
        error.diagnostics[0].path.as_deref(),
        Some("sections.a.lines.main")
    );
    assert_eq!(
        error.diagnostics[0].message,
        "melody length must match section length or divide it with loop"
    );
}

#[test]
fn melody_items_preserve_references_replacement_lengths_and_pickups() {
    let mut doc = document();
    doc["instruments"]["lead"]["gate"] = json!(1);
    doc["melodies"] = json!({"base":["1:8","3:8"],"changed":{"from":"base","replace":{"2":"5:8"}},"line":[{"use":"changed","items":[2,2]},{"use":"changed","items":[2,2]}]});
    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "lead"),
        [(q(0, 1), q(2, 1), 67, 100), (q(2, 1), q(2, 1), 67, 100)]
    );
    doc["melodies"]["changed"]["replace"]["2"] = json!("5:7");
    let error = compile(&doc.to_string()).unwrap_err();
    assert_eq!(
        error.diagnostics[0].path.as_deref(),
        Some("melodies.changed.replace.2")
    );
    assert_eq!(
        error.diagnostics[0].message,
        "replacement item length must equal source length"
    );

    doc["melodies"]["changed"]["replace"]["2"] = json!("5:8");
    doc["melodies"]["line"] = json!([{"use":"changed","items":[2,2],"shift_steps":-2},".:8"]);
    doc["sections"]["a"]["placements"] =
        json!([{ "instrument":"lead","melody":"line","at":{"bar":1,"step":5},"gain_db":-6 }]);
    doc["sections"]["a"]
        .as_object_mut()
        .unwrap()
        .remove("lines");
    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "lead"),
        [(q(1, 2), q(2, 1), 67, 100)]
    );
    doc["sections"]["a"]
        .as_object_mut()
        .unwrap()
        .remove("placements");
    doc["sections"]["a"]["lines"] = json!({"main":{"instrument":"lead","melody":"line"}});

    doc["form"] =
        json!([{"section":"intro","bars":1,"tonic":"C"},{"section":"a","bars":2,"tonic":"C"}]);
    doc["sections"]["intro"] = json!({});
    doc["sections"]["a"]["lines"]["main"]["loop"] = json!(true);
    doc["melodies"] = json!({"line":{"pickup_steps":3,"items":["5 6 7 || 1:16"]}});
    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "lead"),
        [
            (q(13, 4), q(1, 4), 67, 100),
            (q(7, 2), q(1, 4), 69, 100),
            (q(15, 4), q(1, 4), 71, 100),
            (q(4, 1), q(4, 1), 60, 100),
            (q(8, 1), q(4, 1), 60, 100)
        ]
    );
    doc["form"] = json!([{"section":"a","bars":1,"tonic":"C"}]);
    let error = compile(&doc.to_string()).unwrap_err();
    assert_eq!(
        error.diagnostics[0].path.as_deref(),
        Some("melodies.line.items[0]")
    );
    assert_eq!(
        error.diagnostics[0].message,
        "melody note begins before score"
    );

    doc["melodies"]["line"]["items"] = json!(["5 6 7 || 1:16", "3:16"]);
    doc["sections"]["a"]["lines"]["main"]["slice_items"] = json!([2, 2]);
    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "lead"),
        [(q(0, 1), q(4, 1), 64, 100)]
    );
}

#[test]
fn melody_registers_tonic_overrides_and_chord_tones_resolve_pitch() {
    let mut doc = document();
    doc["sections"]["a"]["chords"] = json!(["1:m7/5"]);
    doc["sections"]["a"]["lines"]["main"]["register"] = json!("bass");
    doc["melodies"]["line"] = json!({"relative_to":"chord_bass","items":["1:4 b3:4 c3:4 c7:4"]});
    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "lead")
            .iter()
            .map(|n| n.2)
            .collect::<Vec<_>>(),
        [31, 34, 39, 46]
    );
    doc["sections"]["a"]["lines"]["main"]["tonic"] = json!("D");
    let error = compile(&doc.to_string()).unwrap_err();
    assert_eq!(
        error.diagnostics[0].path.as_deref(),
        Some("sections.a.lines.main")
    );
    assert_eq!(
        error.diagnostics[0].message,
        "tonic cannot override chord_bass material"
    );
    doc["melodies"]["line"]["relative_to"] = json!("tonic");
    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "lead")
            .iter()
            .map(|n| n.2)
            .collect::<Vec<_>>(),
        [38, 41, 39, 46]
    );
}

#[test]
fn articulation_and_dynamics_combine_before_velocity_validation() {
    let mut doc = document();
    doc["melodies"]["line"] = json!(["1^!@110:4 3_:4 5:8"]);
    doc["instruments"]["lead"]["articulations"] =
        json!({"staccato":{"gate":"1/3"},"tenuto":{"velocity":-4}});
    doc["sections"]["a"]["events"] = json!([{"kind":"dynamics","instrument":"lead","at":{"bar":1,"step":1},"duration_quarters":4,"from":-20,"to":-10}]);
    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "lead"),
        [
            (q(0, 1), q(1, 3), 60, 106),
            (q(1, 1), q(1, 1), 64, 78),
            (q(2, 1), q(3, 2), 67, 85)
        ]
    );
    doc["sections"]["a"]["events"][0]["from"] = json!(10);
    let error = compile(&doc.to_string()).unwrap_err();
    assert_eq!(
        error.diagnostics[0].path.as_deref(),
        Some("melodies.line[0]")
    );
    assert_eq!(
        error.diagnostics[0].message,
        "final velocity must be 1..127"
    );
    doc["sections"]["a"]["events"][0]["from"] = json!(-20);
    let duplicate = doc["sections"]["a"]["events"][0].clone();
    doc["sections"]["a"]["events"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    let error = compile(&doc.to_string()).unwrap_err();
    assert_eq!(
        error.diagnostics[0].path.as_deref(),
        Some("sections.a.events[0]")
    );
    assert_eq!(
        error.diagnostics[0].message,
        "overlapping dynamics for instrument"
    );
}

#[test]
fn control_automation_samples_exact_positions_and_detects_conflicts() {
    let mut doc = document();
    doc["sections"]["a"]["events"] = json!([{"kind":"automation","instrument":"lead","at":{"bar":1,"step":1},"duration_quarters":"1/32","control":"pitch_bend","from_value":-1,"to_value":1}]);
    let plan = compile(&doc.to_string()).unwrap();
    let controls = plan.lanes[0]
        .pattern
        .events
        .iter()
        .filter_map(|e| {
            if let MusicalEvent::PitchBend { tick, value } = e {
                Some((
                    q(
                        i64::try_from(*tick).unwrap(),
                        i64::from(plan.ticks_per_beat),
                    ),
                    *value,
                ))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        controls,
        [(q(0, 1), -1.0), (q(1, 64), 0.0), (q(1, 32), 1.0)]
    );
    doc["sections"]["a"]["events"].as_array_mut().unwrap().push(json!({"kind":"pitch_bend","instrument":"lead","at":{"bar":1,"offset_quarters":"1/128"},"value":0}));
    let error = compile(&doc.to_string()).unwrap_err();
    assert_eq!(
        error.diagnostics[0].path.as_deref(),
        Some("sections.a.events[1]")
    );
    assert_eq!(
        error.diagnostics[0].message,
        "control conflicts with automation interval"
    );
}

#[test]
fn instrument_swing_and_offsets_apply_after_notated_expressions() {
    let mut doc = document();
    doc["meta"]["swing"] = json!({"subdivision":8,"ratio":"2/3"});
    doc["form"][0]["swing"] = json!({"subdivision":8,"ratio":"3/4"});
    doc["instruments"]["lead"]["swing"] = json!({"subdivision":8,"ratio":"1/2"});
    doc["instruments"]["lead"]["gate"] = json!(1);
    doc["instruments"]["lead"]["timing_offset_steps"] = json!("1/2");
    doc["melodies"]["line"] = json!(["1:2 3:14"]);
    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "lead"),
        [(q(1, 8), q(1, 2), 60, 100), (q(5, 8), q(27, 8), 64, 100)]
    );
    doc["instruments"]["lead"]["timing_offset_steps"] = json!("-1/2");
    let error = compile(&doc.to_string()).unwrap_err();
    assert_eq!(
        error.diagnostics[0].path.as_deref(),
        Some("melodies.line[0]")
    );
    assert_eq!(
        error.diagnostics[0].message,
        "timing offset places event outside score"
    );
}

#[test]
fn drum_phrases_repeat_and_bar_lists_select_overrides() {
    let mut doc = document();
    doc["form"][0]["bars"] = json!(4);
    doc["sections"]["a"] = json!({"drums":"beat"});
    doc["instruments"]["drum"] = json!({"external":true,"trigger_note_midi":36});
    doc["drums"] = json!({"beat":{"parts":{"drum":["x... .... .... ....",".... x... .... ...."]},"overrides":[{"when":{"bars":[3,"last"]},"parts":{"drum":".... .... X... ...."}}]}});
    assert_eq!(
        notes(&compile(&doc.to_string()).unwrap(), "drum")
            .iter()
            .map(|n| (n.0.clone(), n.3))
            .collect::<Vec<_>>(),
        [
            (q(0, 1), 96),
            (q(5, 1), 96),
            (q(10, 1), 120),
            (q(14, 1), 120)
        ]
    );
    doc["form"][0]["bars"] = json!(3);
    let error = compile(&doc.to_string()).unwrap_err();
    assert_eq!(
        error.diagnostics[0].path.as_deref(),
        Some("drums.beat.parts.drum")
    );
    assert_eq!(
        error.diagnostics[0].message,
        "section bars must be a multiple of drum phrase length"
    );
}

#[test]
fn anticipated_harmony_holds_across_bar_heads_and_guides_bass() {
    let mut doc = document();
    doc["form"][0]["bars"] = json!(2);
    doc["sections"]["a"] = json!({"chords":["1:maj","<4:maj"],"pad":{"instrument":"pad","pattern":"hold"},"lines":{"main":{"instrument":"lead","melody":"line","register":"bass"}}});
    doc["instruments"]["pad"] = json!({"external":true,"gate":1});
    doc["instruments"]["lead"]["gate"] = json!(1);
    doc["melodies"]["line"] = json!({"relative_to":"chord_bass","items":[".:15 1:17"]});
    let plan = compile(&doc.to_string()).unwrap();
    assert_eq!(notes(&plan, "lead"), [(q(15, 4), q(17, 4), 29, 100)]);
    assert_eq!(
        notes(&plan, "pad"),
        [
            (q(0, 1), q(15, 4), 48, 100),
            (q(0, 1), q(15, 4), 52, 100),
            (q(0, 1), q(15, 4), 55, 100),
            (q(15, 4), q(17, 4), 53, 100),
            (q(15, 4), q(17, 4), 57, 100),
            (q(15, 4), q(17, 4), 60, 100)
        ]
    );
    assert_eq!(
        plan.sections[0].chords[1].tick,
        15 * u64::from(plan.ticks_per_beat) / 4
    );
    assert_eq!(
        plan.lanes.iter().find(|l| l.id == "pad").unwrap().sources[3]
            .spelling
            .as_deref(),
        Some("4:maj")
    );
    let mut crossing = doc.clone();
    crossing["form"] =
        json!([{"section":"before","bars":1,"tonic":"C"},{"section":"after","bars":1,"tonic":"C"}]);
    crossing["sections"] = json!({"before":{"chords":["1:maj"],"pad":{"instrument":"pad","pattern":"hold"}},"after":{"chords":["<4:maj"],"pad":{"instrument":"pad","pattern":"hold"}}});
    let crossing = compile(&crossing.to_string()).unwrap();
    assert_eq!(
        crossing.sections[1].chords[0].tick,
        15 * u64::from(crossing.ticks_per_beat) / 4
    );
    assert_eq!(
        notes(&crossing, "pad")
            .iter()
            .filter(|n| n.0 == q(4, 1))
            .map(|n| n.2)
            .collect::<Vec<_>>(),
        [53, 57, 60]
    );
    doc["sections"]["a"]["chords"] = json!(["1:maj", "<<<<<<<<<<<<<<<<4:maj"]);
    let error = compile(&doc.to_string()).unwrap_err();
    assert_eq!(error.diagnostics[0].path.as_deref(), Some("sections"));
    assert_eq!(
        error.diagnostics[0].message,
        "chord anticipation must follow previous chord change and remain within score"
    );
}

#[test]
fn tempo_ramps_expand_to_steps_and_reject_intervening_changes() {
    let mut doc = document();
    doc["time_changes"] = json!([{"at_quarters":1,"bpm":60,"over_quarters":1},{"at_quarters":2,"bpm":120,"over_quarters":"1/2"}]);
    let plan = compile(&doc.to_string()).unwrap();
    let tempos = plan
        .timeline
        .tempo_changes
        .iter()
        .map(|t| {
            (
                q(
                    i64::try_from(t.tick).unwrap(),
                    i64::from(plan.ticks_per_beat),
                ),
                t.bpm,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tempos,
        [
            (q(0, 1), 120.0),
            (q(1, 1), 105.0),
            (q(5, 4), 90.0),
            (q(3, 2), 75.0),
            (q(7, 4), 60.0),
            (q(2, 1), 90.0),
            (q(9, 4), 120.0)
        ]
    );
    doc["time_changes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"at_quarters":"3/2","bpm":100}));
    let error = compile(&doc.to_string()).unwrap_err();
    assert_eq!(
        error.diagnostics[0].path.as_deref(),
        Some("time_changes[0]")
    );
    assert_eq!(
        error.diagnostics[0].message,
        "tempo change overlaps tempo ramp"
    );
}
