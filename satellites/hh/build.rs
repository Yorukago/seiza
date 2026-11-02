// HueHaven's palette table is compiled as it is, straight from its own repo (HH_DIR, default
// ~/Projects/HueHeaven-CLI); nothing in that repo is touched. Without it the built-in list is just empty.
fn main() {
    println!("cargo:rustc-check-cfg=cfg(hh)");
    println!("cargo:rerun-if-env-changed=HH_DIR");
    let dir = std::env::var("HH_DIR").unwrap_or_else(|_| format!("{}/Projects/HueHeaven-CLI", std::env::var("HOME").unwrap_or_default()));
    let src = format!("{dir}/palettes.c");
    println!("cargo:rerun-if-changed={src}");
    if std::path::Path::new(&src).exists() {
        cc::Build::new().file(&src).include(&dir).warnings(false).compile("hhpalettes");
        println!("cargo:rustc-cfg=hh");
    } else {
        println!("cargo:warning=HueHaven not found at {dir} (set HH_DIR): seiza-hh will only list your own palettes");
    }
}
