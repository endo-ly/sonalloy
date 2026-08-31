use std::ptr;

use sonalloy_capi::{
    SonalloyCompiledInstrument, SonalloyEvent, SonalloyProcessContext, SonalloyProcessSpec,
    SonalloyPublishOutcome, SonalloyResult, SonalloyStringView, sonalloy_compile_json,
    sonalloy_compiled_destroy, sonalloy_reclaimable_destroy, sonalloy_runtime_activate,
    sonalloy_runtime_create, sonalloy_runtime_deactivate, sonalloy_runtime_destroy,
    sonalloy_runtime_process, sonalloy_runtime_publish, sonalloy_runtime_reset,
    sonalloy_runtime_state, sonalloy_runtime_take_reclaimable, sonalloy_update_destroy,
    sonalloy_update_prepare,
};

fn view(value: &str) -> SonalloyStringView {
    SonalloyStringView {
        data: value.as_ptr().cast(),
        length: value.len(),
    }
}

fn spec() -> SonalloyProcessSpec {
    SonalloyProcessSpec {
        sample_rate: 48_000.0,
        max_block_size: 64,
        input_channels: 0,
        output_channels: 2,
    }
}

fn context(frame: u64) -> SonalloyProcessContext {
    let frame_f64 = f64::from(u32::try_from(frame).expect("test frame fits u32"));
    SonalloyProcessContext {
        absolute_frame: frame,
        tempo_bpm: 120.0,
        beat_position: frame_f64 * 120.0 / (60.0 * 48_000.0),
        bar_position: frame_f64 / (4.0 * 48_000.0),
        time_signature_numerator: 4,
        time_signature_denominator: 4,
        transport_state: 1,
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn c_api_lifecycle_process_update_and_reclaim() {
    let json_a = include_str!("../../../testdata/instruments/basic-poly-synth.json");
    let mut compiled = ptr::null_mut::<SonalloyCompiledInstrument>();
    let mut diagnostics = ptr::null_mut();
    assert_eq!(
        sonalloy_compile_json(
            view(json_a),
            view("../../../testdata/instruments"),
            spec(),
            &raw mut compiled,
            &raw mut diagnostics,
        ),
        SonalloyResult::Ok
    );
    sonalloy_capi::sonalloy_diagnostics_destroy(diagnostics);

    let mut runtime = ptr::null_mut();
    assert_eq!(
        sonalloy_runtime_create(compiled, &raw mut runtime),
        SonalloyResult::Ok
    );
    assert_eq!(sonalloy_runtime_state(runtime), 0);
    assert_eq!(
        sonalloy_runtime_activate(runtime),
        SonalloyResult::InvalidState
    );
    assert_eq!(
        sonalloy_capi::sonalloy_runtime_prepare(runtime, spec()),
        SonalloyResult::Ok
    );
    assert_eq!(sonalloy_runtime_state(runtime), 1);

    let mut left = [0.0_f32; 64];
    let mut right = [0.0_f32; 64];
    let mut output = [left.as_mut_ptr(), right.as_mut_ptr()];
    let context_zero = context(0);
    let note_on = SonalloyEvent {
        sample_offset: 0,
        event_type: 1,
        note_id: 1,
        parameter_catalog_revision: 0,
        parameter_handle: 0,
        note_number: 60,
        velocity: 110,
        bool_value: 0,
        reserved: 0,
        value: 0.0,
    };
    assert_eq!(
        sonalloy_runtime_process(
            runtime,
            &raw const context_zero,
            &raw const note_on,
            1,
            ptr::null(),
            0,
            output.as_mut_ptr(),
            2,
            64,
        ),
        SonalloyResult::InvalidState
    );
    assert_eq!(sonalloy_runtime_activate(runtime), SonalloyResult::Ok);
    assert_eq!(sonalloy_runtime_state(runtime), 2);
    let mut aliased_output = [left.as_mut_ptr(), left.as_mut_ptr()];
    assert_eq!(
        sonalloy_runtime_process(
            runtime,
            &raw const context_zero,
            ptr::null(),
            0,
            ptr::null(),
            0,
            aliased_output.as_mut_ptr(),
            2,
            64,
        ),
        SonalloyResult::InvalidArgument
    );
    let input_alias = [left.as_ptr()];
    assert_eq!(
        sonalloy_runtime_process(
            runtime,
            &raw const context_zero,
            ptr::null(),
            0,
            input_alias.as_ptr(),
            1,
            output.as_mut_ptr(),
            2,
            64,
        ),
        SonalloyResult::InvalidArgument
    );
    assert_eq!(
        sonalloy_runtime_process(
            runtime,
            &raw const context_zero,
            ptr::null(),
            0,
            ptr::null(),
            0,
            output.as_mut_ptr(),
            2,
            65,
        ),
        SonalloyResult::InvalidArgument
    );
    assert_eq!(
        sonalloy_runtime_process(
            runtime,
            &raw const context_zero,
            ptr::null(),
            0,
            ptr::null(),
            1,
            output.as_mut_ptr(),
            2,
            64,
        ),
        SonalloyResult::InvalidArgument
    );
    let invalid_event = SonalloyEvent {
        event_type: 99,
        ..note_on
    };
    assert_eq!(
        sonalloy_runtime_process(
            runtime,
            &raw const context_zero,
            &raw const invalid_event,
            1,
            ptr::null(),
            0,
            output.as_mut_ptr(),
            2,
            64,
        ),
        SonalloyResult::InvalidArgument
    );
    assert_eq!(
        sonalloy_runtime_process(
            runtime,
            &raw const context_zero,
            &raw const note_on,
            1,
            ptr::null(),
            0,
            output.as_mut_ptr(),
            2,
            64,
        ),
        SonalloyResult::Ok
    );
    assert!(
        left.iter()
            .chain(&right)
            .any(|sample| sample.abs() > 1.0e-6)
    );

    let json_b = json_a.replace("\"gain_db\": -14.0", "\"gain_db\": -8.0");
    let mut compiled_b = ptr::null_mut::<SonalloyCompiledInstrument>();
    let mut diagnostics_b = ptr::null_mut();
    assert_eq!(
        sonalloy_compile_json(
            view(&json_b),
            view("../../../testdata/instruments"),
            spec(),
            &raw mut compiled_b,
            &raw mut diagnostics_b,
        ),
        SonalloyResult::Ok
    );
    sonalloy_capi::sonalloy_diagnostics_destroy(diagnostics_b);

    let mut update = ptr::null_mut();
    assert_eq!(
        sonalloy_update_prepare(compiled_b, spec(), &raw mut update),
        SonalloyResult::Ok
    );
    let mut outcome = SonalloyPublishOutcome {
        generation_id: 0,
        parameter_catalog_revision: 0,
        reported_latency_frames: 0,
        required_input_channels: 0,
    };
    assert_eq!(
        sonalloy_runtime_publish(runtime, update, &raw mut outcome),
        SonalloyResult::Ok
    );
    assert_eq!(outcome.generation_id, 2);
    sonalloy_update_destroy(update);

    let note_off = SonalloyEvent {
        event_type: 2,
        ..note_on
    };
    let note_on_b = SonalloyEvent {
        note_id: 2,
        note_number: 67,
        ..note_on
    };
    let update_events = [note_off, note_on_b];
    let context_64 = context(64);
    assert_eq!(
        sonalloy_runtime_process(
            runtime,
            &raw const context_64,
            update_events.as_ptr(),
            2,
            ptr::null(),
            0,
            output.as_mut_ptr(),
            2,
            64,
        ),
        SonalloyResult::Ok
    );
    assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));

    assert_eq!(sonalloy_runtime_reset(runtime), SonalloyResult::Ok);
    let mut reclaimable = ptr::null_mut();
    assert_eq!(
        sonalloy_runtime_take_reclaimable(runtime, &raw mut reclaimable),
        SonalloyResult::Ok
    );
    assert!(!reclaimable.is_null());
    sonalloy_reclaimable_destroy(reclaimable);
    sonalloy_reclaimable_destroy(reclaimable);
    assert_eq!(sonalloy_runtime_deactivate(runtime), SonalloyResult::Ok);
    assert_eq!(sonalloy_runtime_state(runtime), 1);
    assert_eq!(
        sonalloy_runtime_process(
            runtime,
            &raw const context_zero,
            ptr::null(),
            0,
            ptr::null(),
            0,
            output.as_mut_ptr(),
            2,
            64,
        ),
        SonalloyResult::InvalidState
    );
    sonalloy_runtime_destroy(runtime);
    sonalloy_compiled_destroy(compiled);
    sonalloy_compiled_destroy(compiled_b);
}
