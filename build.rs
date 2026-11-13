//! Embeds the application manifest. Windows only lets layered *child*
//! windows (the strip inside the taskbar) work for programs that declare
//! Windows 8+ compatibility in their manifest; embed-manifest writes the
//! resource object itself, so neither rc.exe nor windres is needed.

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest::embed_manifest(embed_manifest::new_manifest("Claudibeat")).expect("unable to embed the manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
