# Mi Home Desktop

Windows 智能家居桌面客户端，通过 Home Assistant 管理已接入的米家设备，并为兼容摄像头提供可选监控能力。

## 开发环境

- Node.js LTS、pnpm
- Rust stable、Cargo
- Windows 开发与打包需要 Visual Studio C++ Build Tools 和 WebView2 Runtime

## 本地开发

```sh
pnpm install --frozen-lockfile
pnpm tauri dev
```

当前版本包含桌面导航骨架、设置页、可持久化外观主题和连接偏好。Rust 启动时在应用数据目录创建 SQLite 数据库并执行迁移，按日写入日志（最多保留 7 个日志文件）。已接入 Home Assistant 连接测试、Windows 凭据管理、WebSocket 实体同步和断线重连，设备中心可查看真实实体。设备中心支持搜索分组、本地收藏偏好以及按实际能力显示的 light/switch/climate/fan 控制。已加入 HA 截图、go2rtc WebRTC 协商与单画面/四宫格监控，具体摄像头兼容性待实机验证。已加入 HA 场景收藏与执行，以及托盘、开机启动、窗口恢复和脱敏诊断导出的桌面集成代码，Windows 原生行为待最终验收。不要将 HA Token 或其他凭据提交到仓库。


## 验证

```sh
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib --locked
cargo check --manifest-path src-tauri/Cargo.toml --locked
```

第二条仅验证不依赖桌面 WebView 的 Rust 模块。最后一条要求当前操作系统具备 Tauri 桌面依赖。浏览器运行 `pnpm dev` 可预览界面；保存本地配置需要 `pnpm tauri dev` 启动的桌面客户端。

详见 [开发说明](docs/DEVELOPMENT.md)。

[Home Assistant 连接说明](docs/HOME_ASSISTANT_SETUP.md) · [Windows exe 打包说明](docs/WINDOWS_BUILD.md)

[设备管理说明](docs/DEVICE_CONTROL.md)

[摄像头接入与兼容性记录](docs/CAMERA_COMPATIBILITY.md)

[场景与桌面功能](docs/DESKTOP_AND_SCENES.md) · [Windows 验收清单](docs/WINDOWS_TESTING.md)
