# Windows 测试包构建

> 0.3.1 修正用户首轮跨网测试中暴露的连接确认处理缺陷，增强认证响应兼容与分层诊断；55 项 Rust、13 项米家界面测试通过。实际摄像头出画仍需修正版复测。

> 0.3.0 CS2 远程测试实现：53 项 Rust 测试通过；米家界面 12 项通过，包含实际生成 HLS 的浏览器解码。Windows MSVC 目标检查通过。真实摄像头跨网与 Windows 出画仍待实测。

> 2026-11-12：`v0.2.1-camera.1`（039a01 视频授权检测版）已完成 Windows x64 Release 编译与 NSIS 打包，39 项 Rust 测试通过，浏览器测试 20 项通过、1 项跳过。该版新增云端视频授权检测，远程摄像头直播仍未实现。安装包未签名。

## 已验证范围

已在 Debian 云机完成 Windows x64 MSVC 目标的完整编译、Release 链接和 NSIS 安装包生成。异步 Tauri State 命令采用原生 `Result<ApiResult<T>, ()>`，由 Tauri 解包后向前端返回统一的 ApiResult。

主程序为 PE32+ x86-64 GUI，运行仍需 WebView2 Runtime。自 0.3.0 起远程视频依赖随包 `ffmpeg/` 目录，不可只复制主 EXE。导入表使用 Windows 系统 DLL/UCRT，没有额外的 VC++ Redistributable DLL。NSIS 安装包负责安装和 WebView2 检查，优先使用安装包。

这是未签名测试包，不是已经通过 Windows 实机验收的正式版本。真实窗口、托盘、开机启动、系统凭据、HA 设备与摄像头须按 WINDOWS_TESTING.md 验收。HA 与 go2rtc 独立运行，不包含在安装包内。

## Windows 主机构建

准备 Node.js 24 LTS、pnpm 11.25.0、Rust stable MSVC、Visual Studio C++ Build Tools（C++ 桌面工作负载和 Windows SDK）和 WebView2 Runtime：

```powershell
# 先将下节生成的视频组件放入 src-tauri/resources/ffmpeg/
pnpm install --frozen-lockfile
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib --locked
pnpm tauri build --bundles nsis -- --locked
```

主程序：`src-tauri/target/release/mi-home-desktop.exe`。

安装包：`src-tauri/target/release/bundle/nsis/*-setup.exe`。

## 本云机交叉构建

工具位于仓库外 `/workspace/.toolchains`，包括 Rust Windows target、cargo-xwin 0.23.1、MSVC CRT/Windows SDK、LLVM/LLD/Clang 19 和 NSIS 3.11。Debian 软件包通过带签名索引下载并解包到用户目录，没有修改系统包或关闭包签名、TLS、下载校验。

本机入口：

```sh
scripts/build-windows-cloud.sh
```

脚本分两步：先构建 Release exe，再复用该 exe 打包 NSIS。产物位于：

```text
src-tauri/target/x86_64-pc-windows-msvc/release/mi-home-desktop.exe
src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/*-setup.exe
```

Tauri CLI 在 Linux 主机上即使打包 Windows 目标也会检测 AppIndicator。已通过真实的 Ayatana/GTK 开发包及 pkg-config sysroot 满足该检查；没有替换检测结果。CLI 会移除 NSISDIR，因此本机 makensis 包装脚本为真实 NSIS 二进制设置其用户安装资源目录。

链接器报告的微软 CRT PDB 缺失警告仅涉及调试符号，Release 链接已成功。重复打包已被写入 NSIS 类型标记的同一 exe 时，CLI 可能提示找不到原始 bundle-type 标记；本版不含自动更新插件，标记的首次写入已完成，不能把该提示视作 Windows 运行验收。

若在新环境重建，仍需允许微软 SDK 和固定 Debian 软件源的实际下载主机（本次使用 `download.visualstudio.microsoft.com` 和 `snapshot.debian.org`）。过去的 403 已恢复，本次编译和打包不再被网络阻塞。

## GitHub Actions 备选

`.github/workflows/windows-build.yml` 提供手动 Windows 构建任务，会生成 exe、安装包、SHA256SUMS.txt 与测试清单。该工作流尚未在 GitHub 执行，不会自动发布 Release。代码推送后，才能在仓库 Actions 使用它。

官方交叉构建依据：https://v2.tauri.app/distribute/windows-installer/#build-windows-apps-on-linux-and-macos

## 0.3.0 的视频组件

云机构建入口会先运行 `python3 scripts/prepare-windows-media.py`。它校验 FFmpeg 8.1.3 官方源码的固定 SHA-256，交叉编译一个只支持本地 H.264/H.265 输入、Media Foundation H.264 编码和 HLS 的 LGPL 共享库构建；不会使用下载的闭源 SDK或现成大型 FFmpeg 二进制。

Linux 构建依赖：`gcc-mingw-w64-x86-64-posix`、`nasm`、`make`、Python 3。可通过系统包管理器安装。此云机将签名 Debian 包解包在 `/workspace/.toolchains/ffmpeg-cross`，未修改系统目录。其他安装位置用 `FFMPEG_CROSS_ROOT` 指定（包含 `usr/bin` 的工具链根），或让工具在 PATH 中可用。

```sh
python3 scripts/prepare-windows-media.py
```

输出 `src-tauri/resources/ffmpeg/`，含 EXE、DLL、源码原包、许可、重建脚本与配置/哈希清单；构建产物被 gitignore 排除，Tauri 将其映射到安装目录的 `ffmpeg/`。源码下载需要 `ffmpeg.org`，交叉编译包源沿用环境软件源。Windows 原生构建前可复制 Linux 生成的该目录，或使用已增加 media 前置任务的 GitHub Actions 工作流。

便携分发须将主 EXE 与 `ffmpeg/`、`licenses/`、`THIRD_PARTY_NOTICES.md` 一起打包。H.265 路径需要 Windows Media Foundation H.264 编码器；Windows N/裁剪版可能需要微软 Media Feature Pack。用户不需要自行安装 Java、FFmpeg 或部署网关。真实 Windows 解码、目标摄像头跨网与持续播放仍待验收。
