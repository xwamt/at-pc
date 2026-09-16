fn main() {
    at_pc_build_support::embed_windows_resources(
        env!("CARGO_MANIFEST_DIR"),
        "../../media/at-pc.ico",
        Some("app.manifest"),
    );
}
