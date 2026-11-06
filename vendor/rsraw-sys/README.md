# rsraw-sys（NexFilm vendored fork，捆绑 LibRaw 0.22.2）

本目录是 [rsraw-sys 0.1.1](https://crates.io/crates/rsraw-sys)（MIT，作者 hexilee）
的**构建系统 fork**：`build.rs`（cc 编译 LibRaw C++ 源文件 + bindgen 现场生成绑定）
与 `src/lib.rs` 取自上游 0.1.1，`LibRaw/` 源码树整体替换为上游
[LibRaw 0.22.2 tag](https://github.com/LibRaw/LibRaw/tree/0.22.2)（CDDL/LGPL，
许可文件随树保留），并按 0.22.2 的源文件清单增补了 4 个新文件
（`decoders/olympus14.cpp`、`decoders/pana8.cpp`、`decoders/sonycc.cpp`、
`decompressors/losslessjpeg.cpp`）。

## 为什么存在（M3）

v1.2 前 macOS 捆绑 crates.io 的 rsraw-sys 0.1.1（LibRaw 0.21.3），Windows 捆绑
rawlib 0.7.1（LibRaw 0.22.2 预编译库），两平台解码基线不一致。rsraw-sys 上游尚未
发布 0.22.x、rawlib 无 macOS 预编译，故以本 vendor 统一 macOS 到 0.22.2；Windows
侧继续用 rawlib，两平台 LibRaw 版本一致（`docs/v1.2-decode-matrix.md` §5）。

## 构建说明

- 仅 macOS 目标使用（根 `Cargo.toml` 的 `cfg(target_os = "macos")` 依赖指向本目录）；
  不引入 jpeg/lcms/zlib 外部依赖，与 rsraw-sys 0.1.1 的构建口径一致。
- bindgen 在 HOST==TARGET 时**现场**从 `LibRaw/libraw/libraw.h` 生成绑定，因此结构体
  布局永远与本目录源码树一致。上游 0.1.1 预生成的 0.21.3 时代 `src/bindings.rs`
  **已删除**（Phase 3.1）：它与 0.22.2 头文件的结构体布局错位，曾经作为交叉编译
  （HOST≠TARGET）的回退被 build.rs 复制，现已改为直接 `panic!`（提示只支持原生构建；
  交叉编译请先用本树 0.22.2 头文件重新生成绑定）。
- 升级 LibRaw：替换 `LibRaw/` 树 → 比对 `src/**.cpp` 清单并同步 build.rs →
  bump `RAW_DECODE_VERSION` → 跑解码矩阵 A/B（`docs/raw-backend-camera-support.md`
  §升级机制）。
