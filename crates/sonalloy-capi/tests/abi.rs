use std::ptr;

use sonalloy_capi::{
    SonalloyCompiledInstrument, SonalloyDiagnosticView, SonalloyDiagnostics, SonalloyProcessSpec,
    SonalloyResult, SonalloyStringView, sonalloy_c_api_version, sonalloy_compile_json,
    sonalloy_compiled_destroy, sonalloy_diagnostics_count, sonalloy_diagnostics_destroy,
    sonalloy_diagnostics_get, sonalloy_has_capability,
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

#[test]
fn abi_version_capability_and_pointer_validation_are_stable() {
    assert_eq!(sonalloy_c_api_version(), 1);
    let mut supported = 0;
    assert_eq!(
        sonalloy_has_capability(1, &raw mut supported),
        SonalloyResult::Ok
    );
    assert_eq!(supported, 1);
    assert_eq!(
        sonalloy_has_capability(7, &raw mut supported),
        SonalloyResult::Ok
    );
    assert_eq!(supported, 0);
    assert_eq!(
        sonalloy_has_capability(99, &raw mut supported),
        SonalloyResult::InvalidArgument
    );
    assert_eq!(
        sonalloy_has_capability(1, ptr::null_mut()),
        SonalloyResult::InvalidArgument
    );

    let invalid = SonalloyStringView {
        data: ptr::null(),
        length: 1,
    };
    let mut compiled = ptr::null_mut::<SonalloyCompiledInstrument>();
    let mut diagnostics = ptr::null_mut::<SonalloyDiagnostics>();
    assert_eq!(
        sonalloy_compile_json(
            invalid,
            view("."),
            spec(),
            &raw mut compiled,
            &raw mut diagnostics,
        ),
        SonalloyResult::InvalidArgument
    );
    assert!(compiled.is_null());
    assert!(diagnostics.is_null());

    let invalid_utf8 = SonalloyStringView {
        data: [0xff_u8].as_ptr().cast(),
        length: 1,
    };
    assert_eq!(
        sonalloy_compile_json(
            invalid_utf8,
            view("."),
            spec(),
            &raw mut compiled,
            &raw mut diagnostics,
        ),
        SonalloyResult::InvalidArgument
    );

    assert_eq!(
        sonalloy_compile_json(
            view("{"),
            view("."),
            spec(),
            &raw mut compiled,
            &raw mut diagnostics,
        ),
        SonalloyResult::CompileFailed
    );
    assert!(compiled.is_null());
    assert_eq!(sonalloy_diagnostics_count(diagnostics), 1);
    let mut diagnostic = SonalloyDiagnosticView {
        code: 0,
        severity: 0,
        path: SonalloyStringView {
            data: ptr::null(),
            length: 0,
        },
        message: SonalloyStringView {
            data: ptr::null(),
            length: 0,
        },
        detail: SonalloyStringView {
            data: ptr::null(),
            length: 0,
        },
    };
    assert_eq!(
        sonalloy_diagnostics_get(diagnostics, 0, &raw mut diagnostic),
        SonalloyResult::Ok
    );
    assert_eq!(diagnostic.code, 2);
    assert_eq!(diagnostic.severity, 0);
    sonalloy_diagnostics_destroy(diagnostics);

    sonalloy_compiled_destroy(ptr::null_mut());
    sonalloy_diagnostics_destroy(ptr::null_mut());
    sonalloy_diagnostics_get(ptr::null(), 0, ptr::null_mut());
}

#[test]
fn compile_diagnostics_and_parameter_catalog_use_borrowed_views() {
    let json = include_str!("../../../testdata/instruments/basic-poly-synth.json");
    let mut compiled = ptr::null_mut::<SonalloyCompiledInstrument>();
    let mut diagnostics = ptr::null_mut::<SonalloyDiagnostics>();
    assert_eq!(
        sonalloy_compile_json(
            view(json),
            view("../../../testdata/instruments"),
            spec(),
            &raw mut compiled,
            &raw mut diagnostics,
        ),
        SonalloyResult::Ok
    );
    assert!(!compiled.is_null());
    assert!(!diagnostics.is_null());
    assert_eq!(sonalloy_diagnostics_count(diagnostics), 0);

    let count = sonalloy_c_api_version();
    assert_eq!(count, 1);
    let mut handle = 0;
    assert_eq!(
        sonalloy_capi::sonalloy_compiled_parameter_handle(
            compiled,
            view("layer.body.gain"),
            &raw mut handle,
        ),
        SonalloyResult::Ok
    );
    let mut descriptor = sonalloy_capi::SonalloyParameterDescriptor {
        id: SonalloyStringView {
            data: ptr::null(),
            length: 0,
        },
        owner_kind: 0,
        owner_index: 0,
        owner_sub_index: 0,
        owner_axis: 0,
        unit: 0,
        scale: 0,
        min: 0.0,
        max: 0.0,
        default: 0.0,
        smoothing_seconds: 0.0,
    };
    assert_eq!(
        sonalloy_capi::sonalloy_compiled_parameter_descriptor(
            compiled,
            handle,
            &raw mut descriptor,
        ),
        SonalloyResult::Ok
    );
    let id = unsafe {
        std::slice::from_raw_parts(descriptor.id.data.cast::<u8>(), descriptor.id.length)
    };
    assert_eq!(
        std::str::from_utf8(id).expect("descriptor id is utf8"),
        "layer.body.gain"
    );
    let mut normalized = 0.0;
    assert_eq!(
        sonalloy_capi::sonalloy_compiled_parameter_normalize(
            compiled,
            handle,
            descriptor.default,
            &raw mut normalized,
        ),
        SonalloyResult::Ok
    );
    assert!((0.0..=1.0).contains(&normalized));
    let mut native = 0.0;
    assert_eq!(
        sonalloy_capi::sonalloy_compiled_parameter_denormalize(
            compiled,
            handle,
            normalized,
            &raw mut native,
        ),
        SonalloyResult::Ok
    );
    assert!((native - descriptor.default).abs() < 1.0e-5);

    sonalloy_compiled_destroy(compiled);
    sonalloy_diagnostics_destroy(diagnostics);
}
