# Windows 测试包构建

> 2026-11-12：`v0.2.0-test.1`（米家扫码直连测试版）已完成 Windows x64 Release 编译与 NSIS 打包。微软 CRT/SDK 与构建依赖已恢复；真实二维码获取通过，手机确认后的登录、真实设备控制和 Windows 桌面仍需实机验收。安装包未签名。

## 已验证范围

已在 Debian 云机完成 Windows x64 MSVC 目标的完整编译、Release 链接和 NSIS 安装包生成。异步 Tauri State 命令采用原生 `Result<ApiResult<T>, ()>`，由 Tauri 解包后向前端返回统一的 ApiResult。

主程序为 PE32+ x86-64 GUI，可独立复制，但运行仍需 WebView2 Runtime。导入表使用 Windows 系统 DLL/UCRT，没有额外的 VC++ Redistributable DLL。NSIS 安装包负责安装和 WebView2 检查，优先使用安装包。

这是未签名测试包，不是已经通过 Windows 实机验收的正式版本。真实窗口、托盘、开机启动、系统凭据、HA 设备与摄像头须按 WINDOWS_TESTING.md 验收。HA 与 go2rtc 独立运行，不包含在安装包内。

## Windows 主机构建

准备 Node.js 24 LTS、pnpm 11.25.0、Rust stable MSVC、Visual Studio C++ Build Tools（C++ 桌面工作负载和 Windows SDK）和 WebView2 Runtime：

```powershell
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
