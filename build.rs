// Ajoute l'icône au fichier .exe (Windows uniquement).
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let out = std::env::var("OUT_DIR").unwrap();
        let windres = if cfg!(windows) { "windres" } else { "x86_64-w64-mingw32-windres" };
        let res = format!("{out}/icone.o");
        let ok = std::process::Command::new(windres)
            .args(["icone.rc", "-O", "coff", "-o", &res])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            println!("cargo:rustc-link-arg-bins={res}");
        }
        println!("cargo:rerun-if-changed=icone.rc");
        println!("cargo:rerun-if-changed=icone.ico");
    }
}
