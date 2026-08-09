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
    println!("cargo:rerun-if-env-changed=SONALLOY_DSP_TEST_HOOKS");
    println!("cargo:rustc-check-cfg=cfg(sonalloy_test_hooks)");

    let native_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("native")
        .join("daisysp-wrapper");

    let mut config = cmake::Config::new(native_dir);
    config.define("CMAKE_INSTALL_LIBDIR", "lib");
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
    stretch_config.define("CMAKE_INSTALL_LIBDIR", "lib");
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
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }
}
