//! Embeds `docs/icon/sonisub.ico` as the icon of `sonisub.exe`: only for the command line (`cli`) built on
//! Windows. The library alone and other systems build exactly as before.

fn main() {
    println!("cargo:rerun-if-changed=docs/icon/sonisub.ico");
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    embed_icon();
}

#[cfg(windows)]
fn embed_icon() {
    let windows_target = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    if !windows_target || std::env::var_os("CARGO_FEATURE_CLI").is_none() {
        return;
    }
    winresource::WindowsResource::new()
        .set_icon("docs/icon/sonisub.ico")
        .compile()
        .expect("cannot embed docs/icon/sonisub.ico in the executable");
}
