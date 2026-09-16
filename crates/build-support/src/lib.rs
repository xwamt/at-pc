//! Shared Windows resource embedding used by agent and server `build.rs`.
//!
//! The old per-crate scripts only looked up mingw `windres`, so
//! `x86_64-pc-windows-msvc` silently skipped icons and the agent manifest.
//! This crate always plans an MSVC `rc.exe` compile instead of skipping.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Which Windows resource compiler a rustc `target_env` should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceCompilerKind {
    Windres,
    Rc,
}

/// Whether this target should compile a `.rc` file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedAction {
    SkipNonWindows,
    Compile(ResourceCompilerKind),
}

/// argv + output artifact for one resource-compiler invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerInvocation {
    pub program: String,
    pub args: Vec<String>,
    pub output: PathBuf,
}

const GNU_CANDIDATES: &[&str] = &["x86_64-w64-mingw32-windres", "windres"];

/// Decide embed action from rustc cfg values (`CARGO_CFG_TARGET_OS` / `_ENV`).
pub fn embed_action(target_os: &str, target_env: &str) -> EmbedAction {
    if target_os != "windows" {
        EmbedAction::SkipNonWindows
    } else if target_env == "msvc" {
        EmbedAction::Compile(ResourceCompilerKind::Rc)
    } else {
        EmbedAction::Compile(ResourceCompilerKind::Windres)
    }
}

/// MSVC always attempts compile (even if `rc.exe` is not on PATH).
/// GNU only invokes when `windres` was actually found.
pub fn should_invoke(kind: ResourceCompilerKind, found_on_path: bool) -> bool {
    match kind {
        ResourceCompilerKind::Rc => true,
        ResourceCompilerKind::Windres => found_on_path,
    }
}

/// `.rc` body: icon always; RT_MANIFEST (type 24) when a manifest path is given.
pub fn windows_rc_source(icon: &Path, manifest: Option<&Path>) -> String {
    let mut source = format!("1 ICON \"{}\"\n", path_for_rc(icon));
    if let Some(manifest) = manifest {
        source.push_str(&format!("1 24 \"{}\"\n", path_for_rc(manifest)));
    }
    source
}

/// Build the compiler command. MSVC with an empty tool list still uses `rc.exe`.
pub fn compiler_invocation(
    kind: ResourceCompilerKind,
    available: &[String],
    rc_path: &Path,
    out_dir: &Path,
) -> CompilerInvocation {
    match kind {
        ResourceCompilerKind::Windres => {
            let output = out_dir.join("app_icon.o");
            let program = pick(available, GNU_CANDIDATES, "windres");
            CompilerInvocation {
                program,
                args: vec![
                    "-i".into(),
                    rc_path.display().to_string(),
                    "-O".into(),
                    "coff".into(),
                    "-o".into(),
                    output.display().to_string(),
                ],
                output,
            }
        }
        ResourceCompilerKind::Rc => {
            let output = out_dir.join("app_icon.res");
            let program = available
                .first()
                .cloned()
                .unwrap_or_else(|| "rc.exe".to_string());
            CompilerInvocation {
                program,
                args: vec![
                    "/nologo".into(),
                    format!("/fo{}", output.display()),
                    rc_path.display().to_string(),
                ],
                output,
            }
        }
    }
}

/// Embed icon (and optional application manifest) into a Windows binary.
///
/// Called from crate `build.rs` files. Non-Windows targets return immediately.
/// MSVC never silent-skips: missing `rc.exe` panics instead of dropping the icon.
pub fn embed_windows_resources(
    crate_manifest_dir: impl AsRef<Path>,
    icon_relative: &str,
    windows_manifest_relative: Option<&str>,
) {
    println!("cargo:rerun-if-changed={icon_relative}");
    println!("cargo:rerun-if-changed=build.rs");
    if let Some(manifest) = windows_manifest_relative {
        println!("cargo:rerun-if-changed={manifest}");
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let kind = match embed_action(&target_os, &target_env) {
        EmbedAction::SkipNonWindows => return,
        EmbedAction::Compile(kind) => kind,
    };

    let crate_manifest_dir = crate_manifest_dir.as_ref();
    let icon_path = crate_manifest_dir.join(icon_relative);
    let specified_manifest = windows_manifest_relative.map(|rel| crate_manifest_dir.join(rel));
    let (icon_path, manifest_path) =
        match resolve_windows_assets(&icon_path, specified_manifest.as_deref()) {
            Ok(assets) => assets,
            Err(WindowsAssetError::MissingIcon(path)) => {
                panic!(
                    "Windows icon not found at {}; cannot skip resource embed",
                    path.display()
                )
            }
            Err(WindowsAssetError::MissingManifest(path)) => {
                panic!(
                    "Windows manifest not found at {}; cannot skip resource embed",
                    path.display()
                )
            }
        };

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR set by cargo"));
    let rc_path = out_dir.join("app.rc");
    let rc_content = windows_rc_source(&icon_path, manifest_path.as_deref());
    if let Err(err) = std::fs::write(&rc_path, rc_content) {
        panic!("failed to write {}: {err}", rc_path.display());
    }

    match kind {
        ResourceCompilerKind::Windres => {
            let available: Vec<String> = GNU_CANDIDATES
                .iter()
                .filter(|name| tool_is_available(name))
                .map(|name| (*name).to_string())
                .collect();
            if !should_invoke(kind, !available.is_empty()) {
                println!(
                    "cargo:warning=Windows resource compiler not found ({}); GNU icon embed skipped",
                    GNU_CANDIDATES.join(", ")
                );
                return;
            }
            run_resource_compiler(compiler_invocation(kind, &available, &rc_path, &out_dir));
        }
        ResourceCompilerKind::Rc => {
            let (path_dirs, extra_roots) = msvc_rc_search();
            match find_rc_exe(&path_dirs, &extra_roots) {
                Some(program) => {
                    run_resource_compiler(compiler_invocation(
                        kind,
                        &[program.to_string_lossy().into_owned()],
                        &rc_path,
                        &out_dir,
                    ));
                }
                None => panic!(
                    "failed to find Windows resource compiler rc.exe (MSVC must not silent-skip). \
                     searched PATH dirs: {path_dirs:?}; extra roots ($RC, Windows Kits, \
                     WindowsSdkVerBinPath, vswhere): {extra_roots:?}"
                ),
            }
        }
    }
}

/// Missing icon or a caller-specified manifest that is not on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsAssetError {
    MissingIcon(PathBuf),
    MissingManifest(PathBuf),
}

/// Fail-closed asset resolution for Windows targets (GNU and MSVC).
pub fn resolve_windows_assets(
    icon: &Path,
    specified_manifest: Option<&Path>,
) -> Result<(PathBuf, Option<PathBuf>), WindowsAssetError> {
    if !icon.exists() {
        return Err(WindowsAssetError::MissingIcon(icon.to_path_buf()));
    }
    let icon = icon.canonicalize().unwrap_or_else(|_| icon.to_path_buf());
    let manifest = match specified_manifest {
        None => None,
        Some(path) if path.exists() => {
            Some(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
        }
        Some(path) => return Err(WindowsAssetError::MissingManifest(path.to_path_buf())),
    };
    Ok((icon, manifest))
}

/// Locate `rc.exe` given PATH directories and extra roots (Windows SDK / `$RC`).
pub fn find_rc_exe(path_dirs: &[PathBuf], extra_roots: &[PathBuf]) -> Option<PathBuf> {
    for dir in path_dirs {
        if let Some(found) = rc_in_dir(dir) {
            return Some(found);
        }
    }
    for root in extra_roots {
        if root.is_file() {
            return Some(root.clone());
        }
        if let Some(found) = rc_in_dir(root) {
            return Some(found);
        }
        if let Some(found) = rc_in_sdk_tree(root) {
            return Some(found);
        }
    }
    None
}

fn path_for_rc(path: &Path) -> String {
    strip_verbatim_prefix(&path.to_string_lossy()).replace('\\', "/")
}

fn strip_verbatim_prefix(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = raw.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        raw.to_string()
    }
}

fn rc_in_dir(dir: &Path) -> Option<PathBuf> {
    for name in ["rc.exe", "llvm-rc", "llvm-rc.exe"] {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn rc_in_sdk_tree(bin_root: &Path) -> Option<PathBuf> {
    let mut versions = match std::fs::read_dir(bin_root) {
        Ok(entries) => entries.filter_map(Result::ok).collect::<Vec<_>>(),
        Err(_) => return None,
    };
    versions.sort_by_key(|entry| entry.file_name());
    versions.reverse();
    for version in versions {
        let path = version.path();
        if !path.is_dir() {
            continue;
        }
        for arch in ["x64", "x86", "arm64"] {
            let candidate = path.join(arch).join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if let Some(found) = rc_in_dir(&path) {
            return Some(found);
        }
    }
    None
}

fn msvc_rc_search() -> (Vec<PathBuf>, Vec<PathBuf>) {
    let path_dirs = env::var_os("PATH")
        .map(|value| env::split_paths(&value).collect())
        .unwrap_or_default();
    let mut extra_roots = Vec::new();
    if let Some(rc) = env::var_os("RC") {
        extra_roots.push(PathBuf::from(rc));
    }
    extra_roots.extend(windows_sdk_bin_roots());
    extra_roots.extend(vswhere_rc_candidates());
    (path_dirs, extra_roots)
}

fn windows_sdk_bin_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for key in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Ok(pf) = env::var(key) {
            let kits = PathBuf::from(pf).join("Windows Kits");
            roots.push(kits.join("10").join("bin"));
            roots.push(kits.join("8.1").join("bin"));
        }
    }
    if let Ok(ver_bin) = env::var("WindowsSdkVerBinPath") {
        roots.push(PathBuf::from(ver_bin));
    }
    roots
}

fn vswhere_rc_candidates() -> Vec<PathBuf> {
    let mut vswhere = Vec::new();
    if let Ok(pf) = env::var("ProgramFiles(x86)") {
        vswhere.push(
            PathBuf::from(pf)
                .join("Microsoft Visual Studio")
                .join("Installer")
                .join("vswhere.exe"),
        );
    }
    vswhere.push(PathBuf::from(
        r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe",
    ));
    let Some(exe) = vswhere.into_iter().find(|path| path.is_file()) else {
        return Vec::new();
    };
    let Ok(output) = Command::new(exe)
        .args(["-latest", "-products", "*", "-find", "RC.exe"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn run_resource_compiler(invocation: CompilerInvocation) {
    match Command::new(&invocation.program)
        .args(&invocation.args)
        .status()
    {
        Ok(status) if status.success() => {
            println!("cargo:rustc-link-arg={}", invocation.output.display());
        }
        Ok(status) => panic!(
            "Windows resource compiler {} failed with status {status}",
            invocation.program
        ),
        Err(err) => panic!(
            "failed to invoke Windows resource compiler {} (MSVC must not silent-skip): {err}",
            invocation.program
        ),
    }
}

fn pick(available: &[String], candidates: &[&str], fallback: &str) -> String {
    for candidate in candidates {
        if available.iter().any(|name| name == candidate) {
            return (*candidate).to_string();
        }
    }
    fallback.to_string()
}

fn tool_is_available(name: &str) -> bool {
    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path_var).any(|dir| {
        if dir.join(name).is_file() {
            return true;
        }
        if cfg!(windows) && !name.to_ascii_lowercase().ends_with(".exe") {
            dir.join(format!("{name}.exe")).is_file()
        } else {
            false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn windows_msvc_cfg_selects_rc_not_a_skip() {
        assert_eq!(
            embed_action("windows", "msvc"),
            EmbedAction::Compile(ResourceCompilerKind::Rc)
        );
    }

    #[test]
    fn windows_gnu_cfg_selects_windres() {
        assert_eq!(
            embed_action("windows", "gnu"),
            EmbedAction::Compile(ResourceCompilerKind::Windres)
        );
    }

    #[test]
    fn non_windows_is_noop_even_if_env_looks_like_msvc() {
        assert_eq!(embed_action("macos", "msvc"), EmbedAction::SkipNonWindows);
        assert_eq!(embed_action("linux", "gnu"), EmbedAction::SkipNonWindows);
    }

    #[test]
    fn msvc_attempts_embed_when_compiler_is_not_on_path() {
        assert!(should_invoke(ResourceCompilerKind::Rc, false));
    }

    #[test]
    fn gnu_does_not_invoke_windres_when_missing() {
        assert!(!should_invoke(ResourceCompilerKind::Windres, false));
        assert!(should_invoke(ResourceCompilerKind::Windres, true));
    }

    #[test]
    fn rc_source_includes_icon_and_optional_manifest() {
        let icon = Path::new(r"C:\icons\at-pc.ico");
        let manifest = Path::new(r"C:\app\app.manifest");
        let icon_only = windows_rc_source(icon, None);
        assert_eq!(icon_only, "1 ICON \"C:/icons/at-pc.ico\"\n");
        let with_manifest = windows_rc_source(icon, Some(manifest));
        assert_eq!(
            with_manifest,
            "1 ICON \"C:/icons/at-pc.ico\"\n1 24 \"C:/app/app.manifest\"\n"
        );
    }

    #[test]
    fn msvc_invocation_defaults_to_rc_exe_even_with_empty_tool_list() {
        let inv = compiler_invocation(
            ResourceCompilerKind::Rc,
            &[],
            Path::new("app.rc"),
            Path::new("out"),
        );
        assert_eq!(inv.program, "rc.exe");
        assert!(inv.output.ends_with("app_icon.res"));
        assert!(inv.args.iter().any(|a| a == "/nologo"));
        assert!(inv
            .args
            .iter()
            .any(|a| a == "/foout/app_icon.res" || a == r"/foout\app_icon.res"));
        assert!(inv.args.iter().any(|a| a == "app.rc"));
    }

    #[test]
    fn path_for_rc_strips_windows_verbatim_disk_prefix() {
        use std::ffi::OsString;
        let os = OsString::from(r"\\?\C:\foo\bar.ico");
        let path = Path::new(&os);
        assert_eq!(path_for_rc(path), "C:/foo/bar.ico");
    }

    #[test]
    fn path_for_rc_strips_windows_verbatim_unc_prefix() {
        let path = Path::new(r"\\?\UNC\server\share\foo.ico");
        assert_eq!(path_for_rc(path), "//server/share/foo.ico");
    }

    #[test]
    fn find_rc_exe_sdk_hit_when_path_misses() {
        let tmp = unique_temp_dir("sdk-hit");
        let path_dir = tmp.join("path");
        std::fs::create_dir_all(&path_dir).unwrap();
        let sdk_root = tmp.join("kits").join("10").join("bin");
        let rc = sdk_root.join("10.0.22621.0").join("x64");
        std::fs::create_dir_all(&rc).unwrap();
        std::fs::write(rc.join("rc.exe"), []).unwrap();

        let found = find_rc_exe(&[path_dir], &[sdk_root]);
        let found = found.expect("SDK rc.exe must be found when PATH misses");
        assert!(found.ends_with("rc.exe"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn find_rc_exe_none_when_path_and_sdk_miss() {
        let tmp = unique_temp_dir("sdk-miss");
        let path_dir = tmp.join("path");
        let sdk_root = tmp.join("kits");
        std::fs::create_dir_all(&path_dir).unwrap();
        std::fs::create_dir_all(&sdk_root).unwrap();

        assert_eq!(find_rc_exe(&[path_dir], &[sdk_root]), None);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn missing_icon_is_error_not_a_skip() {
        let err = resolve_windows_assets(Path::new("/no/such/at-pc.ico"), None).unwrap_err();
        assert!(matches!(err, WindowsAssetError::MissingIcon(_)));
    }

    #[test]
    fn missing_specified_manifest_is_error_not_a_skip() {
        let tmp = unique_temp_dir("manifest-miss");
        let icon = tmp.join("at-pc.ico");
        std::fs::write(&icon, []).unwrap();
        let err =
            resolve_windows_assets(&icon, Some(Path::new("/no/such/app.manifest"))).unwrap_err();
        assert!(matches!(err, WindowsAssetError::MissingManifest(_)));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn unique_temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "at-pc-build-support-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn gnu_invocation_prefers_mingw_windres_when_listed() {
        let inv = compiler_invocation(
            ResourceCompilerKind::Windres,
            &["x86_64-w64-mingw32-windres".to_string()],
            Path::new("app.rc"),
            Path::new("out"),
        );
        assert_eq!(inv.program, "x86_64-w64-mingw32-windres");
        assert!(inv.output.ends_with("app_icon.o"));
        let expected_out = Path::new("out").join("app_icon.o");
        assert_eq!(
            inv.args,
            vec![
                "-i",
                "app.rc",
                "-O",
                "coff",
                "-o",
                expected_out.to_str().expect("utf-8 path")
            ]
        );
    }
}
