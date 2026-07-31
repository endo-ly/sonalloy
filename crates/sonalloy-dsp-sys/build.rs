use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=../../native/daisysp-wrapper/CMakeLists.txt");
    println!("cargo:rerun-if-changed=../../native/daisysp-wrapper/include/sonalloy_dsp.h");
    println!("cargo:rerun-if-changed=../../native/daisysp-wrapper/src/daisysp_wrapper.cpp");
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

    if env::var("CARGO_CFG_TARGET_FAMILY").as_deref() == Ok("unix") {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }
}
