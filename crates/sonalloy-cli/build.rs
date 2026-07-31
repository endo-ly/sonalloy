use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=SONALLOY_DSP_TEST_HOOKS");
    println!("cargo:rustc-check-cfg=cfg(sonalloy_test_hooks)");
    if env::var_os("SONALLOY_DSP_TEST_HOOKS").is_some() {
        println!("cargo:rustc-cfg=sonalloy_test_hooks");
    }
}
