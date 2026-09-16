# 构建产物目录：host 与交叉编译隔离

本机 `target/` 曾把 debug / release / `x86_64-pc-windows-gnu` / `x86_64-pc-windows-msvc` 混在同一棵指纹树里（审查时约 27 GB，本节点测量时 `du -sh target` 为 **42G**）。仓库源码只有约 1 MB 量级。**不要用 `cargo clean` 当测量手段。**

## 约定

不要设置 `[build] target-dir`（单一目录仍会把 `--target` 产物堆在同一棵树下）。用环境变量拆开 fingerprint：

| 场景 | `CARGO_TARGET_DIR` |
| :--- | :--- |
| macOS 日常开发 | `target/mac` |
| Linux 日常开发 / CI quality job | `target/linux` |
| Windows 本机，或交叉编译到 Windows | `target/win` |

```bash
# macOS 主机
CARGO_TARGET_DIR=target/mac cargo test --workspace --locked

# 交叉到 Windows GNU / MSVC（先设 win 目录，再用 .cargo/config.toml 里的 alias）
CARGO_TARGET_DIR=target/win cargo build-win-gnu --release --locked
CARGO_TARGET_DIR=target/win cargo build-win-msvc --release --locked
```

`.gitignore` 的 `target/` 已覆盖 `target/mac`、`target/linux`、`target/win`。

CI 用 `Swatinem/rust-cache@v2` 缓存编译 job（`quality` / `platform-check`）；job 级 `CARGO_TARGET_DIR`（`target/linux`、`target/mac`、`target/win`）必须保留，由 rust-cache 按隔离目录建 key，不要再叠一层 `actions/cache`。

## 清理（可选，需确认）

- 优先：`cargo install cargo-sweep`，在已隔离的 `CARGO_TARGET_DIR` 内 `cargo sweep -s`。
- 或：`cargo clean -p <crate>`，只清单个 crate。
- 禁止未经确认对遗留的混合 `target/` 执行全量 `cargo clean`。

## Integration test target（仅评估，不在本节点合并）

每个 `tests/*.rs` 都是独立 crate。当前工作区有 34 个 integration test 文件（agent + server + protocol），`cargo test --workspace` 会为 lib + 每个文件各链接一份可执行文件，放大磁盘与链接开销。把测试收到更少 target / 独立 e2e crate 属于 **P2-8**（还要解开 agent ↔ server 的 dev-dependency 环）。**P2-2 不移动、不合并测试文件。**

约定检查解析 `.cargo/config.toml` 与 CI YAML 的结构（注释 / `echo` 字符串不算 rust-cache 或 `CARGO_TARGET_DIR`），并禁止烘焙 `[env] CARGO_TARGET_DIR` 与 `[build] target-dir`：

```bash
python3 scripts/check_target_dir_convention.py
python3 scripts/test_check_target_dir_convention.py
```
