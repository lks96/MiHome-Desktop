# 开发说明

## 当前范围

- Tauri 2 桌面框架、Vue 3 与 Naive UI 界面。
- Pinia 设置 Store，加载、错误、保存中、保存成功和撤销更改状态。
- 浅色、深色和跟随系统主题（保存后生效）。
- SQLx SQLite 数据库，位于 Tauri `app_data_dir/mihome.sqlite`，使用 WAL 和版本迁移。
- 统一 `ApiResult<T>` 与稳定错误码。请求超时限制为 1 至 120 秒，Rust 再次校验。
- `tracing` 日志位于 Tauri `app_log_dir`，按日轮换，保留最近 7 个日志文件。只记录初始化摘要和数据库失败的操作名，不记录设置值或 SQL 参数。

现已实现 HA REST 连接检查、Windows 凭据读写、WebSocket 认证/状态订阅/快照同步及自动重连。设备中心已支持区域/注册表元数据、搜索分组、收藏与本地偏好以及 light/switch/climate/fan 能力控制；已接入 HA 截图、go2rtc WebRTC、监控单画面/四宫格和会话管理；已接入场景收藏/执行、托盘/启动/窗口状态插件和诊断导出；其他 domain 专用控制及 MSE/HLS 回退尚未实现。协议测试使用本地服务，不代表真实 HA、Windows 凭据或摄像头已经验收。

## 目录

- `src/views/SettingsView.vue`：设置表单。
- `src/stores/settings.ts`：设置状态与持久化请求。
- `src/utils/ipc.ts`、`src/types/settings.ts`：统一 IPC 和 TypeScript 类型。
- `src-tauri/src/commands.rs`：Tauri Commands 入口。
- `src-tauri/src/models.rs`、`errors.rs`：数据模型和统一响应。
- `src-tauri/src/database/`：SQLx 数据访问和迁移。
- `src-tauri/src/logging.rs`：文件日志初始化。

## 平台依赖

Windows 10/11 x64：Node.js LTS、pnpm、Rust stable 的 MSVC 工具链、Visual Studio C++ Build Tools（C++ 桌面开发工作负载及 Windows SDK）、WebView2 Runtime。

Linux 上运行桌面客户端另外需要 GTK/WebKit 开发库。Debian 通常包括 `libgtk-3-dev`、`libwebkit2gtk-4.1-dev`、`librsvg2-dev`，具体以 Tauri 2 对当前发行版的说明为准。Linux 构建结果不能证明 Windows/WebView2 兼容性。

当前云机的 Rust 位于可写的工作区；每个 shell 使用：

```sh
export RUSTUP_HOME=/workspace/.toolchains/rustup
export CARGO_HOME=/workspace/.toolchains/cargo
export PATH="$CARGO_HOME/bin:$PATH"
export XDG_CACHE_HOME=/workspace/.cache
export PNPM_HOME=/workspace/.local/share/pnpm
cd /workspace/MiHome-Desktop
```

## 验证与启动

```sh
pnpm install --frozen-lockfile
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib --locked
pnpm tauri dev
```

`--no-default-features --lib` 仅运行数据库及其他不依赖 GUI 的模块，用于没有 WebView 系统库的环境；默认构建始终保留桌面功能。

Windows 实机检查：进入设置页，修改主题与超时时间，保存后关闭并重新启动；确认主题与字段恢复，并检查应用数据目录数据库和日志目录。首次启动失败需结合启动输出和日志定位，数据库损坏时不会自动覆盖已有配置。

新增数据库变更应使用新迁移文件，不修改已经执行过的迁移。不得向 `app_settings` 加入令牌、密码或含认证信息的 URL。Token 通过 `security::CredentialStore` 交给 Windows Credential Manager；非 Windows 平台明确返回凭据服务不支持，不会回退到文件或内存模拟持久化。


[HA 联调](HOME_ASSISTANT_SETUP.md) · [Windows 打包](WINDOWS_BUILD.md)


## 浏览器功能测试

```sh
pnpm exec playwright install chromium
pnpm test:ui
```

测试自动启动 Vite。若环境已有 Chromium，可通过 `PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH` 指定，例如当前云机：

```sh
PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/usr/bin/chromium pnpm test:ui
```

测试数据仅在 `tests/devices.spec.ts` 的浏览器上下文中注入，不进入应用。它验证前端 IPC 交互；真实服务协议和数据库由 Rust 测试另行验证。详情见 [设备管理](DEVICE_CONTROL.md)。


摄像头测试使用独立的本地协议与浏览器 fixture，详见 [摄像头文档](CAMERA_COMPATIBILITY.md)。不以 fixture 通过替代 Windows 实机解码和型号兼容性验证。


[场景、桌面功能与日志导出](DESKTOP_AND_SCENES.md)说明了代码范围、隐私策略及 Windows 验证边界。日志已迁移为 JSON；旧版文本日志在诊断包中省略，不会直接导出原文。
