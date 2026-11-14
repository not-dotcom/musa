use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let target = out.join("icons.gresource");
    let status = Command::new("glib-compile-resources")
        .arg("--target")
        .arg(&target)
        .arg("--sourcedir")
        .arg(manifest.join("assets/icons"))
        .arg(manifest.join("resources/icons.gresource.xml"))
        .status()
        .expect("no se pudo ejecutar glib-compile-resources");
    assert!(status.success(), "glib-compile-resources falló");
    println!("cargo:rerun-if-changed=resources/icons.gresource.xml");
    println!("cargo:rerun-if-changed=assets/icons");
}
