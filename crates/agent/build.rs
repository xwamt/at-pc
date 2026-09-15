use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../../media/at-pc.ico");
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let icon_path = manifest_dir.join("../../media/at-pc.ico");

        if icon_path.exists() {
            let canonical_icon = icon_path.canonicalize().unwrap_or(icon_path);
            let manifest_path = manifest_dir.join("app.manifest");
            let canonical_manifest = manifest_path.canonicalize().unwrap_or(manifest_path);

            let mut rc_content = format!("1 ICON \"{}\"\n", canonical_icon.display().to_string().replace('\\', "/"));
            if canonical_manifest.exists() {
                rc_content.push_str(&format!("1 24 \"{}\"\n", canonical_manifest.display().to_string().replace('\\', "/")));
            }

            let rc_path = out_dir.join("app.rc");
            if std::fs::write(&rc_path, rc_content).is_ok() {
                let res_path = out_dir.join("app_icon.o");
                if let Some(bin) = which_windres() {
                    let status = Command::new(bin)
                        .args(["-i", rc_path.to_str().unwrap(), "-O", "coff", "-o", res_path.to_str().unwrap()])
                        .status();

                    if let Ok(s) = status {
                        if s.success() {
                            println!("cargo:rustc-link-arg={}", res_path.display());
                        }
                    }
                }
            }
        }
    }
}

fn which_windres() -> Option<String> {
    for name in &["x86_64-w64-mingw32-windres", "windres"] {
        if let Ok(out) = Command::new("which").arg(name).output() {
            if out.status.success() {
                return Some(name.to_string());
            }
        }
    }
    None
}
