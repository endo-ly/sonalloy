use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=../../native/daisysp-wrapper/CMakeLists.txt");
    println!("cargo:rerun-if-changed=../../native/daisysp-wrapper/include/sonalloy_dsp.h");
    println!("cargo:rerun-if-changed=../../native/daisysp-wrapper/src/daisysp_wrapper.cpp");
    println!("cargo:rerun-if-changed=../../native/signalsmith-stretch-wrapper/CMakeLists.txt");
    println!(
        "cargo:rerun-if-changed=../../native/signalsmith-stretch-wrapper/include/sonalloy_stretch.h"
    );
    println!(
        "cargo:rerun-if-changed=../../native/signalsmith-stretch-wrapper/src/signalsmith_stretch_wrapper.cpp"
    );
    println!(
        "cargo:rerun-if-changed=../../native/signalsmith-stretch-wrapper/third_party/signalsmith-stretch/signalsmith-stretch.h"
    );
    println!(
        "cargo:rerun-if-changed=../../native/signalsmith-stretch-wrapper/third_party/signalsmith-linear/linear.h"
    );
    println!(
        "cargo:rerun-if-changed=../../native/signalsmith-stretch-wrapper/third_party/signalsmith-linear/stft.h"
    );
    println!(
        "cargo:rerun-if-changed=../../native/signalsmith-stretch-wrapper/third_party/signalsmith-linear/fft.h"
    );
    println!("cargo:rerun-if-env-changed=CFLAGS");
    println!("cargo:rerun-if-env-changed=CXXFLAGS");
    println!("cargo:rerun-if-env-changed=RUSTFLAGS");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    println!("cargo:rerun-if-env-changed=CMAKE_OSX_DEPLOYMENT_TARGET");
    println!("cargo:rerun-if-env-changed=SONALLOY_DSP_MSVC_RUNTIME");
    println!("cargo:rerun-if-env-changed=SONALLOY_DSP_TEST_HOOKS");
    println!("cargo:rustc-check-cfg=cfg(sonalloy_test_hooks)");

    let native_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("native")
        .join("daisysp-wrapper");

    let mut config = cmake::Config::new(native_dir);
    configure_native(&mut config);
    if env::var_os("SONALLOY_DSP_TEST_HOOKS").is_some() {
        config.define("SONALLOY_DSP_TEST_HOOKS", "ON");
        println!("cargo:rustc-cfg=sonalloy_test_hooks");
    }
    let destination = config.build();

    println!(
        "cargo:rustc-link-search=native={}",
        destination.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=sonalloy_daisysp_wrapper");
    println!("cargo:rustc-link-lib=static=DaisySP");

    let stretch_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("native")
        .join("signalsmith-stretch-wrapper");
    let mut stretch_config = cmake::Config::new(stretch_dir);
    configure_native(&mut stretch_config);
    if env::var_os("SONALLOY_DSP_TEST_HOOKS").is_some() {
        stretch_config.define("SONALLOY_DSP_TEST_HOOKS", "ON");
    }
    let stretch_destination = stretch_config.build();
    println!(
        "cargo:rustc-link-search=native={}",
        stretch_destination.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=sonalloy_signalsmith_stretch_wrapper");

    if env::var("CARGO_CFG_TARGET_FAMILY").as_deref() == Ok("unix") {
        let cxx_runtime = match env::var("CARGO_CFG_TARGET_OS").as_deref() {
            Ok("macos") => "c++",
            _ => "stdc++",
        };
        println!("cargo:rustc-link-lib=dylib={cxx_runtime}");
    }
}

fn configure_native(config: &mut cmake::Config) {
    config.define("CMAKE_INSTALL_LIBDIR", "lib");

    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // cmake-rs otherwise adds /MD to CMAKE_<LANG>_FLAGS_<CONFIG> for every
        // configuration. CMake owns the configuration-specific runtime choice.
        config.no_default_flags(true);
        let runtime_option = match env::var("SONALLOY_DSP_MSVC_RUNTIME") {
            Ok(value) => value,
            Err(env::VarError::NotPresent) => "release".to_owned(),
            Err(env::VarError::NotUnicode(_)) => {
                panic!("SONALLOY_DSP_MSVC_RUNTIME must be 'debug' or 'release'")
            }
        };
        let (runtime_library, iterator_debug_level) = match runtime_option.as_str() {
            "debug" => ("MultiThreaded$<$<CONFIG:Debug>:Debug>DLL", "2"),
            "release" => ("MultiThreadedDLL", "0"),
            other => {
                panic!("SONALLOY_DSP_MSVC_RUNTIME must be 'debug' or 'release', got '{other}'")
            }
        };
        config.define("CMAKE_MSVC_RUNTIME_LIBRARY", runtime_library);
        config.define("SONALLOY_MSVC_ITERATOR_DEBUG_LEVEL", iterator_debug_level);
    }

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        if let Some(deployment_target) = env::var_os("MACOSX_DEPLOYMENT_TARGET")
            .or_else(|| env::var_os("CMAKE_OSX_DEPLOYMENT_TARGET"))
        {
            config.define("CMAKE_OSX_DEPLOYMENT_TARGET", deployment_target);
        }
    }
}
