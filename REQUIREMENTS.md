# Mi Home Desktop — 完整产品需求与技术开发规范

> 文档文件：`REQUIREMENTS.md`  
> 项目名称：Mi Home Desktop  
> 文档版本：1.0.0  
> 状态：开发基线  
> 更新时间：2026-11-10  
> 首发平台：Windows 10 / Windows 11 x64

------------------------------------------------------------------------

## 0. 项目决策摘要

本项目开发一款独立 Windows 桌面客户端，用于管理用户已经绑定在米家 App 中、并通过 Home Assistant 成功接入的智能家居设备，同时在 PC 上查看兼容摄像头的实时画面。

### 已确定的技术决策

| 项目       | 决策                                                           |
|:-----------|:---------------------------------------------------------------|
| 桌面框架   | Tauri 2                                                        |
| 核心业务   | Rust                                                           |
| 桌面 UI    | Vue 3 + TypeScript + Vite                                      |
| UI 组件    | Naive UI                                                       |
| 前端状态   | Pinia                                                          |
| 设备中枢   | Home Assistant，独立运行                                       |
| 米家接入   | Home Assistant 社区集成 Xiaomi Miot                            |
| 视频桥接   | go2rtc，独立运行，按需启用                                     |
| 本地数据库 | SQLite                                                         |
| 数据访问   | SQLx                                                           |
| 异步运行时 | Tokio                                                          |
| HTTP       | Reqwest                                                        |
| WebSocket  | tokio-tungstenite                                              |
| 凭据保存   | Windows Credential Manager，通过 Rust `keyring` 等安全封装访问 |
| 首发平台   | Windows 10/11 x64                                              |

### 必须遵守的架构边界

1.  Home Assistant 独立安装和运行，不打包进桌面客户端。
2.  Rust 客户端直接连接 Home Assistant REST API 和 WebSocket API。
3.  不增加 FastAPI、Node.js 或 Python 业务中间层。
4.  Xiaomi Miot 负责米家设备适配；Mi Home Desktop 不自行实现小米云端协议。
5.  go2rtc 是独立视频服务，不是桌面客户端内嵌的必需服务。
6.  摄像头视频能力必须以实际型号和实机测试结果为准。
7.  不把 Home Assistant Token、小米账号密码或摄像头密码写入前端代码、SQLite 明文或日志。
8.  V1.0 优先保证真实设备可发现、可控制、状态同步可靠、至少一个已验证视频源可播放。

------------------------------------------------------------------------

# 1. 项目概述

## 1.1 项目背景

用户日常通过米家 App 管理智能家居，但希望在 Windows PC 上获得统一的桌面控制与监控入口。

Mi Home Desktop 通过 Home Assistant 将米家设备统一暴露为标准实体，再由 Rust 桌面客户端连接 Home Assistant，提供设备管理、状态查看、控制操作、摄像头监控和场景执行功能。

## 1.2 产品目标

- 在 Windows 桌面端查看已接入 Home Assistant 的米家设备。
- 通过桌面客户端控制支持的设备功能。
- 接收设备状态变化并实时刷新界面。
- 按房间、设备类别和收藏状态组织设备。
- 在 PC 上查看已验证兼容的米家摄像头实时视频。
- 支持 1、4、9 宫格监控布局。
- 复用 Home Assistant 已有场景，不重复实现完整自动化引擎。
- 提供安全的连接配置、日志、重连和本地偏好设置。

## 1.3 非目标

V1.0 不承诺以下能力：

- 直接控制所有米家生态设备；未接入 HA 或 HA 不支持的设备不在保证范围内。
- 自行实现小米账号登录、云端鉴权或小米私有协议。
- 保证所有米家摄像头都能获取实时视频。
- 绕过摄像头厂商认证、DRM、云服务权限或付费限制。
- 默认提供云录像回放、云台控制、双向语音、AI 识别等功能。
- 通过公网直接暴露 Home Assistant 或 go2rtc。
- 将 Home Assistant 安装包和服务打包到 Mi Home Desktop 安装程序中。
- 开发移动端、macOS 或 Linux 客户端。

------------------------------------------------------------------------

# 2. 用户与使用场景

## 2.1 目标用户

- 已使用米家 App 绑定智能设备的个人用户。
- 已部署或愿意部署 Home Assistant 的用户。
- 希望通过 Windows PC 查看设备状态和摄像头画面的用户。

## 2.2 核心使用场景

1.  用户启动 Mi Home Desktop，客户端自动连接已配置的 Home Assistant。
2.  客户端同步实体、区域、设备状态和场景。
3.  用户在设备中心按房间查看设备。
4.  用户点击灯具卡片，开关灯或调整亮度、色温。
5.  用户在监控中心打开摄像头，查看实时画面。
6.  用户将多个摄像头放入 2×2 或 3×3 监控布局。
7.  Home Assistant 网络中断时，客户端显示离线状态并自动重连。
8.  用户执行一个 Home Assistant 场景，客户端反馈调用结果。

------------------------------------------------------------------------

# 3. 总体技术架构

## 3.1 架构图

``` text
┌───────────────────────────────────────────────────────────┐
│                    Mi Home Desktop                         │
│                                                           │
│  Vue 3 + TypeScript + Naive UI + Pinia                    │
│  首页 / 监控中心 / 设备中心 / 场景 / 设置                  │
└────────────────────────────┬──────────────────────────────┘
                             │ Tauri IPC
┌────────────────────────────▼──────────────────────────────┐
│                       Rust Core                            │
│                                                           │
│  HA Client       Device Service      Camera Service       │
│  WebSocket       Scene Service       Settings Service     │
│  SQLite          Credential Store    Event Dispatcher     │
│  Error Handler   Logging             Reconnect Manager    │
└──────────────────────┬───────────────────────┬────────────┘
                       │                       │
                 REST / WebSocket         go2rtc API / WebRTC
                       │                       │
┌──────────────────────▼──────────┐  ┌─────────▼────────────┐
│       Home Assistant            │  │       go2rtc         │
│                                 │  │                      │
│  Xiaomi Miot                    │  │ Xiaomi source        │
│  Entity / Device / Area         │  │ WebRTC / MSE / HLS   │
│  Scene / Service                │  │ Optional FFmpeg      │
└──────────────────────┬──────────┘  └─────────┬────────────┘
                       │                       │
                       ▼                       ▼
                 米家智能设备                兼容摄像头
```

## 3.2 职责划分

### Vue 前端

负责：

- 页面布局和交互。
- 设备卡片与详情面板。
- 监控画面布局。
- 用户输入和表单校验。
- Pinia 状态管理。
- 展示 Rust 发送的事件。
- 展示连接状态、错误和操作反馈。

禁止：

- 在前端保存 HA Token 或小米账号密码。
- 直接实现小米账号登录。
- 将设备控制逻辑散落在组件中。
- 把模拟数据当成真实设备状态。
- 在前端绕过 Rust 安全边界直接执行系统命令。

### Rust Core

负责：

- Home Assistant REST / WebSocket 通信。
- Token 读取与凭据管理。
- 设备数据归一化。
- 设备控制参数校验。
- 状态缓存与事件分发。
- 摄像头服务发现和播放会话管理。
- SQLite 读写。
- 日志、错误处理和重连。
- Tauri Commands 与 Events。

### Home Assistant

负责：

- 米家设备接入。
- Xiaomi Miot 集成。
- 设备实体和服务暴露。
- 设备状态更新。
- 区域和场景管理。
- 具体设备协议、云端或局域网通信。

### go2rtc

按摄像头适配结果决定是否启用，负责：

- Xiaomi 摄像头源接入（若型号和协议兼容）。
- RTSP 等已有视频源接入。
- 视频流转发。
- WebRTC / MSE / HLS 输出。
- 必要时配合 FFmpeg 转码。

Mi Home Desktop 不应把”已安装 go2rtc”误认为”摄像头必然兼容”。

------------------------------------------------------------------------

# 4. 技术栈与依赖规范

## 4.1 前端

- Vue 3
- TypeScript
- Vite
- Naive UI
- Pinia
- Vue Router
- ESLint
- Prettier

前端统一使用 Composition API 和 `<script setup lang="ts">`。

## 4.2 Rust

建议依赖类别：

| 类别       | 推荐依赖                        | 用途                 |
|:-----------|:--------------------------------|:---------------------|
| 桌面框架   | tauri 2                         | 桌面窗口与 IPC       |
| 异步运行时 | tokio                           | 异步任务             |
| HTTP       | reqwest                         | HA REST、go2rtc HTTP |
| WebSocket  | tokio-tungstenite               | HA WebSocket         |
| 序列化     | serde、serde_json               | JSON                 |
| 数据库     | sqlx + sqlite                   | SQLite               |
| 凭据       | keyring                         | Windows 凭据管理     |
| 日志       | tracing、tracing-subscriber     | 日志                 |
| 时间       | time 或 chrono                  | 时间处理             |
| 错误       | thiserror、anyhow               | 错误管理             |
| ID         | uuid                            | 本地记录和会话 ID    |
| 配置       | serde + TOML/JSON（非秘密配置） | 配置读取             |
| URL        | url                             | 地址解析与校验       |

依赖版本使用 Cargo.lock 固定。升级依赖必须经过构建和回归测试。

## 4.3 视频播放

- 首选 WebRTC。
- go2rtc 作为独立视频源服务。
- 只有当原始编码在目标 Windows WebView2 环境中无法稳定播放时，才增加 FFmpeg 转码。
- H.265/HEVC 的播放能力必须在实际 Windows 系统、WebView2 版本和编解码组件上验证。
- H.264 作为兼容性回退方向。
- 不允许将静态截图伪装成实时视频。

## 4.4 平台要求

- Windows 10 22H2 x64。
- Windows 11 x64。
- 安装包优先采用 Tauri 支持的 NSIS 或 MSI。
- 使用 WebView2 Runtime；安装程序应检查或引导安装运行环境。
- 首发不要求 ARM64。

------------------------------------------------------------------------

# 5. Home Assistant 接入需求

## 5.1 接入前置条件

用户必须先独立部署 Home Assistant，并完成：

1.  Home Assistant 可正常访问。
2.  安装 HACS（如果 Xiaomi Miot 版本和安装方式要求）。
3.  安装 Xiaomi Miot 社区集成。
4.  使用正确的小米账号地区完成设备接入。
5.  至少有一台设备成功生成 HA Entity。
6.  在 HA 用户资料中创建长期访问令牌。

Xiaomi Miot 为社区集成，不是小米官方 API。设备是否可用由具体型号、账号地区、固件和集成版本决定。

## 5.2 连接配置

设置项：

| 字段            | 类型    | 规则                   |
|:----------------|:--------|:-----------------------|
| connection_name | string  | 默认”Home Assistant”   |
| base_url        | URL     | 必填，支持 http/https  |
| token           | secret  | 必填，保存到系统凭据库 |
| request_timeout | integer | 默认 10 秒             |
| auto_connect    | boolean | 默认 true              |
| auto_reconnect  | boolean | 默认 true              |
| tls_verify      | boolean | 默认 true              |

连接流程：

1.  校验 URL 格式。
2.  读取用户输入 Token。
3.  调用 HA API 根路径进行连通性测试。
4.  验证 HTTP 状态码与 JSON 响应。
5.  成功后将 Token 写入 Windows Credential Manager。
6.  SQLite 仅保存 credential_ref，不保存 Token。
7.  建立 WebSocket。
8.  完成认证并开始同步数据。

不得在日志中输出完整 Token、Authorization Header 或密码。

## 5.3 REST API 使用范围

客户端主要使用：

| API                                     | 用途         |
|:----------------------------------------|:-------------|
| `GET /api/`                             | 连通性检查   |
| `GET /api/states`                       | 获取实体状态 |
| `GET /api/states/{entity_id}`           | 获取指定实体 |
| `POST /api/services/{domain}/{service}` | 调用 HA 服务 |

具体接口行为以部署的 Home Assistant 版本官方文档为准。

## 5.4 WebSocket 使用范围

连接地址：

`ws(s)://{HA_HOST}/api/websocket`

连接步骤：

1.  建立 WebSocket。
2.  接收 `auth_required`。
3.  发送 `auth` 消息和 Token。
4.  接收 `auth_ok`。
5.  订阅实体状态变化。
6.  接收并解析事件。
7.  维护 subscription ID。
8.  断线后重连并重新订阅。

优先使用 HA 支持的实体订阅命令；如果目标版本不支持或不满足需求，使用 `state_changed` 事件订阅作为兼容路径。

区域、实体注册表、设备注册表和服务信息，按 HA 当前 WebSocket API 能力获取；某项数据无法读取时，不得阻塞基本实体列表加载。

## 5.5 连接状态

Rust 内部统一维护：

``` rust
enum ConnectionState {
    Disconnected,
    Connecting,
    Authenticating,
    Connected,
    Reconnecting,
    Error,
}
```

前端显示：

- 未连接
- 正在连接
- 正在认证
- 已连接
- 正在重连
- 连接错误

## 5.6 自动重连

采用指数退避：

| 次数     |  等待时间  |
|:---------|:----------:|
| 1        |    1 秒    |
| 2        |    2 秒    |
| 3        |    5 秒    |
| 4        |   10 秒    |
| 5 及以后 | 最长 30 秒 |

加入随机抖动，避免重连风暴。

连接恢复后：

- 重新认证。
- 重新订阅事件。
- 获取必要实体快照。
- 合并状态缓存。
- 向前端发送连接恢复和同步完成事件。

------------------------------------------------------------------------

# 6. 设备管理需求

## 6.1 设备数据来源

Home Assistant Entity 是设备功能的主要数据来源。

客户端不得把”一个物理设备”等同于”一个 Entity”。一个物理设备可能暴露多个实体，例如开关、传感器、电量、状态等。

数据模型需区分：

- Device：物理设备（HA Device Registry 中的设备）。
- Entity：可读写的实体。
- Area：HA 房间或区域。
- Service：可调用的服务。
- Scene：场景实体。

## 6.2 设备类别

首版支持以下常见实体类别的通用呈现：

| Entity Domain | 功能                             |
|:--------------|:---------------------------------|
| light         | 开关、亮度、色温、颜色（按能力） |
| switch        | 开关                             |
| climate       | 温度、模式、风速（按能力）       |
| fan           | 开关、百分比、预设模式（按能力） |
| vacuum        | 启动、暂停、回充（按服务）       |
| sensor        | 数值、单位、更新时间             |
| binary_sensor | 开/关状态                        |
| cover         | 开关、位置（按能力）             |
| lock          | 上锁、解锁                       |
| camera        | 截图/视频能力信息                |
| media_player  | 播放控制（按能力）               |
| scene         | 执行场景                         |
| button        | 执行按钮实体                     |

不支持的 Domain 仍允许显示基础信息，不允许直接隐藏所有未知实体。

## 6.3 动态能力识别

控制 UI 必须基于：

- entity domain；
- entity state；
- entity attributes；
- supported_features；
- 可用 service；
- 设备当前状态。

不能仅根据设备名称或品牌推断功能。

例如：

- 灯具没有 brightness 属性时，不显示亮度滑块。
- 空调不支持 fan_mode 时，不显示风速选择。
- 摄像头没有可用 stream 时，不显示”正在直播”。
- 设备 unavailable 时，控制按钮应禁用或要求用户确认重试。

## 6.4 设备列表

支持：

- 全部设备。
- 按 HA Area 分组。
- 按 Domain 分类。
- 收藏设备。
- 隐藏设备。
- 自定义显示名称。
- 自定义排序。
- 关键词搜索。
- 在线/离线筛选。

房间优先使用 Home Assistant Area。用户在客户端设置的房间覆盖值仅作为本地显示覆盖，不修改 HA 原始数据。

## 6.5 设备详情

设备详情至少显示：

- 名称。
- 实体 ID。
- 所属区域。
- 设备类别。
- 当前状态。
- 关键属性。
- 最近更新时间。
- 支持的操作。
- 操作反馈。

## 6.6 设备控制流程

1.  用户操作控件。
2.  前端调用 Tauri Command。
3.  Rust 验证 entity_id、domain、service 和参数。
4.  Rust 通过 HA API 调用服务。
5.  HA 返回执行结果。
6.  WebSocket 状态事件更新实体缓存。
7.  前端根据真实状态更新界面。
8.  若服务失败，显示明确错误。

不能在用户点击按钮时直接乐观地把设备状态标记为成功。可以显示”执行中”，但成功状态必须由服务响应或后续实体状态确认。

------------------------------------------------------------------------

# 7. 摄像头监控需求

## 7.1 用户现有摄像头

目标设备：

**小米智能摄像机 2 云台版，用户于 2022 年购买。**

具体硬件型号编码尚未实机确认。

重要说明：

- 购买年份不能唯一确定硬件型号。
- 不得直接把设备认定为 `chuangmi.camera.039a01` 或 `chuangmi.camera.039c01`。
- 必须先从 Home Assistant 设备信息、Xiaomi Miot 诊断信息或设备规格中读取实际 model。
- go2rtc 已公开的兼容列表中列有 2023 年版 `chuangmi.camera.039c01`，但这不等于 2022 年购买的设备就是该型号。
- `039a01` 出现在 Xiaomi Miot 相关设备型号列表中，不代表 go2rtc 已确认支持该型号的视频流。
- go2rtc 对较早的小米摄像头协议存在兼容性差异；未列入兼容名单的型号必须实测。

## 7.2 摄像头接入优先级

### 路线 A：Home Assistant Camera

先检查 Xiaomi Miot 是否为摄像头创建实体，以及该实体是否提供：

- entity state；
- snapshot；
- stream source；
- stream type；
- 可用服务；
- 设备属性。

如果只提供截图，则客户端只能显示截图，不能标记为实时直播。

### 路线 B：go2rtc Xiaomi Source

若 Home Assistant 没有可用实时流，尝试 go2rtc 的 Xiaomi Source。

要求：

- 单独部署 go2rtc。
- 使用经过测试的稳定版本并记录版本号。
- 在 go2rtc 中完成小米账号认证。
- 处理短信/邮件验证码或验证码挑战（若出现）。
- 获取摄像头设备列表。
- 根据真实 model、did、账号地区配置流。
- 验证握手、取流、视频解码和持续播放。

go2rtc 文档说明，连接小米摄像头时可能需要联网获取加密密钥，视频连接本身为本地连接；并非所有型号均受支持。

小米账号认证应在 go2rtc 自身配置/管理流程中完成。Mi Home Desktop V1.0 不实现小米账号密码登录，也不保存小米账号密码。

### 路线 C：其他桥接

仅当前两条路线均失败时，才评估：

- micam；
- miloco；
- 设备专用社区桥接；
- 其他可维护的 RTSP/HTTP 视频源。

任何桥接方案都必须先确认项目来源、许可证、设备兼容性和安全风险。

不允许一开始就开发固件破解或私有协议逆向。

## 7.3 视频协议和解码

播放优先级：

1.  WebRTC。
2.  MSE（如果 go2rtc 与目标 WebView2 环境支持）。
3.  HLS 作为兼容性回退。
4.  仅截图模式。

HEVC/H.265 在不同 Windows/WebView2 环境中的可用性可能不同。

处理策略：

- 先测试原始编码是否能稳定播放。
- 如无法播放，评估 FFmpeg 转 H.264。
- 转码属于可选开销，不默认启用。
- 记录转码前后 CPU 占用、延迟和画质。
- 任何视频播放成功必须通过真实摄像头验证。

## 7.4 监控中心布局

支持：

- 单画面。
- 2×2 四宫格。
- 3×3 九宫格。
- 单路全屏。
- 摄像头列表。
- 在线/离线状态。
- 视频连接状态。
- 手动刷新。
- 自动重连。
- 静音（仅当音频通道可用时）。

V1.0 优先验收单画面和四宫格；九宫格属于增强项，需进行性能测试。

## 7.5 摄像头状态

``` rust
enum CameraStatus {
    Unknown,
    Offline,
    SnapshotOnly,
    Connecting,
    Playing,
    Reconnecting,
    Unsupported,
    Error,
}
```

界面状态文案：

| 状态         | 文案         |
|:-------------|:-------------|
| Unknown      | 状态未知     |
| Offline      | 摄像头离线   |
| SnapshotOnly | 仅支持截图   |
| Connecting   | 正在连接     |
| Playing      | 正在播放     |
| Reconnecting | 视频重连中   |
| Unsupported  | 暂不支持播放 |
| Error        | 视频连接失败 |

## 7.6 播放会话管理

- 仅为当前可见画面创建播放会话。
- 切换页面时停止不需要的会话。
- 关闭窗口时释放所有会话。
- 单画面与宫格切换时复用可复用的连接。
- 同一路摄像头不允许无意创建多个重复会话。
- 对连接超时和媒体无数据设置超时。
- 自动重连需限制最大重试频率。
- 记录播放失败原因，但不记录认证秘密。

## 7.7 云台及双向语音

云台方向控制、巡航、预置位、双向语音不列入 V1.0 必须交付项。

只有当 HA 或其他已验证接口明确暴露对应服务，并且实机测试成功后，才加入功能列表。

------------------------------------------------------------------------

# 8. Tauri IPC 接口规范

## 8.1 通用规则

- Command 使用 snake_case。
- TypeScript 类型使用 PascalCase。
- 所有 Command 返回统一 Result 结构。
- 所有输入参数必须校验。
- 错误返回稳定错误码，不把内部堆栈直接暴露给 UI。
- 长耗时任务应返回任务状态或通过 Event 汇报。
- 前端不得直接持有 HA Token。

统一响应示例：

``` ts
type ApiResult<T> =
  | { ok: true; data: T }
  | {
      ok: false;
      error: {
        code: string;
        message: string;
        retryable: boolean;
      };
    };
```

## 8.2 Home Assistant Commands

| Command                    | 参数                           | 返回                 |
|:---------------------------|:-------------------------------|:---------------------|
| `ha_test_connection`       | baseUrl、token（仅调用时传入） | ConnectionTestResult |
| `ha_save_connection`       | name、baseUrl、token           | ConnectionSummary    |
| `ha_get_connection_status` | 无                             | ConnectionStatus     |
| `ha_connect`               | connectionId                   | ConnectionStatus     |
| `ha_disconnect`            | 无                             | boolean              |
| `ha_sync_entities`         | 无                             | SyncResult           |
| `ha_get_entities`          | filter                         | EntitySummary\[\]    |
| `ha_get_entity`            | entityId                       | EntityDetail         |
| `ha_get_areas`             | 无                             | Area\[\]             |
| `ha_get_services`          | 无                             | ServiceRegistry      |

## 8.3 Device Commands

| Command                    | 参数                                 | 返回              |
|:---------------------------|:-------------------------------------|:------------------|
| `device_get_list`          | filter                               | DeviceView\[\]    |
| `device_get_detail`        | entityId                             | EntityDetail      |
| `device_call_service`      | domain、service、target、serviceData | ServiceCallResult |
| `device_set_favorite`      | entityId、favorite                   | boolean           |
| `device_set_hidden`        | entityId、hidden                     | boolean           |
| `device_set_display_name`  | entityId、displayName                | boolean           |
| `device_set_room_override` | entityId、roomId                     | boolean           |
| `device_refresh`           | entityId                             | EntityDetail      |

所有通用控制都应收敛到经过校验的 `device_call_service`，避免为每种设备创建大量重复命令。

## 8.4 Camera Commands

| Command                     | 参数              | 返回                |
|:----------------------------|:------------------|:--------------------|
| `camera_get_list`           | 无                | CameraSummary\[\]   |
| `camera_get_detail`         | entityId          | CameraDetail        |
| `camera_get_capabilities`   | entityId          | CameraCapabilities  |
| `camera_get_snapshot`       | entityId          | SnapshotResult      |
| `camera_get_stream_config`  | entityId          | StreamConfig        |
| `camera_start_session`      | entityId、quality | CameraSession       |
| `camera_stop_session`       | sessionId         | boolean             |
| `camera_get_session_status` | sessionId         | CameraSessionStatus |
| `camera_refresh`            | entityId          | CameraDetail        |

`StreamConfig` 不得包含长期有效的秘密凭据。若视频服务需要鉴权，应采用受控会话或本地代理方式。

## 8.5 Scene Commands

| Command              | 参数                    | 返回              |
|:---------------------|:------------------------|:------------------|
| `scene_get_list`     | 无                      | SceneSummary\[\]  |
| `scene_execute`      | sceneEntityId           | ServiceCallResult |
| `scene_set_favorite` | sceneEntityId、favorite | boolean           |

## 8.6 Settings Commands

| Command                 | 参数     | 返回         |
|:------------------------|:---------|:-------------|
| `settings_get`          | 无       | AppSettings  |
| `settings_update`       | settings | AppSettings  |
| `settings_export_logs`  | filter   | ExportResult |
| `settings_clear_cache`  | 无       | boolean      |
| `settings_get_app_info` | 无       | AppInfo      |

------------------------------------------------------------------------

# 9. Tauri Events 规范

Rust 通过事件向前端推送变化。

| Event                     | Payload              | 说明         |
|:--------------------------|:---------------------|:-------------|
| `ha:connection_changed`   | ConnectionStatus     | HA 连接状态  |
| `ha:sync_started`         | SyncInfo             | 开始同步     |
| `ha:sync_completed`       | SyncResult           | 同步完成     |
| `device:state_changed`    | EntityStateChanged   | 实体变化     |
| `device:removed`          | entityId             | 实体移除     |
| `device:action_started`   | ActionInfo           | 控制开始     |
| `device:action_completed` | ActionResult         | 控制完成     |
| `camera:status_changed`   | CameraStatusChanged  | 摄像头状态   |
| `camera:session_error`    | CameraError          | 视频会话错误 |
| `scene:executed`          | SceneExecutionResult | 场景执行结果 |
| `app:notification`        | Notification         | 通知         |
| `app:error`               | AppError             | 错误         |

前端监听事件后更新 Pinia Store。

事件 payload 必须有 TypeScript 类型定义，并与 Rust Serde 类型保持一致。

------------------------------------------------------------------------

# 10. 数据模型与 SQLite 设计

## 10.1 存储原则

- Home Assistant 是设备状态的权威来源。
- SQLite 只保存客户端本地配置和用户偏好。
- 实时设备状态优先保存在内存缓存。
- 不长期复制所有 HA 实体状态到本地数据库。
- 所有表必须有迁移脚本。
- 使用 WAL 模式。
- 数据库文件放在 Tauri `app_data_dir`。
- Token、密码等秘密放入 Windows Credential Manager。

## 10.2 表结构

### `ha_connections`

| 字段           | 类型    | 说明         |
|:---------------|:--------|:-------------|
| id             | TEXT PK | UUID         |
| name           | TEXT    | 连接名称     |
| base_url       | TEXT    | HA 地址      |
| credential_ref | TEXT    | 凭据库键名   |
| is_active      | INTEGER | 是否启用     |
| tls_verify     | INTEGER | 是否验证证书 |
| created_at     | TEXT    | 创建时间     |
| updated_at     | TEXT    | 更新时间     |

### `app_settings`

| 字段       | 类型    | 说明         |
|:-----------|:--------|:-------------|
| key        | TEXT PK | 配置键       |
| value      | TEXT    | 非秘密配置值 |
| updated_at | TEXT    | 更新时间     |

### `entity_overrides`

| 字段             | 类型      | 说明         |
|:-----------------|:----------|:-------------|
| entity_id        | TEXT PK   | HA 实体 ID   |
| display_name     | TEXT NULL | 自定义名称   |
| area_id_override | TEXT NULL | 本地房间覆盖 |
| icon             | TEXT NULL | 图标         |
| is_favorite      | INTEGER   | 是否收藏     |
| is_hidden        | INTEGER   | 是否隐藏     |
| sort_order       | INTEGER   | 排序         |
| updated_at       | TEXT      | 更新时间     |

### `camera_profiles`

| 字段                | 类型      | 说明                          |
|:--------------------|:----------|:------------------------------|
| id                  | TEXT PK   | UUID                          |
| entity_id           | TEXT      | 摄像头实体 ID                 |
| source_type         | TEXT      | ha / go2rtc / manual          |
| stream_name         | TEXT NULL | go2rtc 流名称                 |
| preferred_transport | TEXT      | webrtc / mse / hls / snapshot |
| enabled             | INTEGER   | 是否启用                      |
| sort_order          | INTEGER   | 排序                          |
| created_at          | TEXT      | 创建时间                      |
| updated_at          | TEXT      | 更新时间                      |

不得在此表存储明文用户名、密码、Token 或完整认证 URL。

### `scene_favorites`

| 字段            | 类型    | 说明        |
|:----------------|:--------|:------------|
| scene_entity_id | TEXT PK | 场景实体 ID |
| is_favorite     | INTEGER | 是否收藏    |
| sort_order      | INTEGER | 排序        |

### `action_logs`

| 字段       | 类型       | 说明             |
|:-----------|:-----------|:-----------------|
| id         | INTEGER PK | 自增 ID          |
| entity_id  | TEXT NULL  | 实体 ID          |
| action     | TEXT       | 操作类型         |
| result     | TEXT       | success / failed |
| error_code | TEXT NULL  | 错误码           |
| message    | TEXT NULL  | 脱敏后的消息     |
| created_at | TEXT       | 时间             |

不得记录认证头、Token、密码或敏感视频 URL。

### `schema_migrations`

保存迁移版本和执行时间。

## 10.3 索引

至少为以下字段建立索引：

- `entity_overrides.is_favorite`
- `entity_overrides.sort_order`
- `camera_profiles.sort_order`
- `action_logs.created_at`
- `action_logs.entity_id`

------------------------------------------------------------------------

# 11. UI/UX 规范

## 11.1 整体风格

采用简洁、现代、轻量化的桌面控制中心风格。

默认浅色主题，可扩展深色主题。

避免：

- 传统后台系统的密集表格。
- 大面积渐变。
- 过多彩色徽标。
- 过度装饰。
- 把每个实体都显示成同样的卡片。
- 同一页面重复展示相同状态信息。

## 11.2 主窗口

- 默认尺寸：1440 × 900。
- 最小尺寸：1100 × 700。
- 左侧导航：约 220 px。
- 顶部栏：约 60 px。
- 页面内容边距：24 px。
- 卡片圆角：12 px。
- 卡片间距：16 px。

布局必须适配窗口缩放，不允许固定宽度导致内容溢出。

## 11.3 导航结构

左侧导航：

1.  首页
2.  监控中心
3.  设备中心
4.  智能场景
5.  系统设置

顶部栏：

- 当前页面标题。
- HA 连接状态。
- 同步按钮。
- 通知入口。
- 设置入口。

## 11.4 首页

显示：

- 设备总数。
- 在线数量。
- 离线数量。
- 摄像头数量。
- 收藏设备快捷控制。
- 摄像头预览。
- 房间概览。
- 最近操作记录。

统计数据必须来自真实同步结果。

## 11.5 监控中心

顶部：

- 摄像头数量。
- 布局切换（1 / 4 / 9）。
- 刷新。
- 全屏。

主体：

- 视频网格。
- 每个画面显示摄像头名称和状态。
- 加载、离线、错误状态有明确占位。
- 点击画面进入单路放大。

## 11.6 设备中心

左侧筛选：

- 全部设备。
- 房间。
- 设备类别。
- 收藏。
- 离线设备。

右侧：

- 搜索框。
- 排序。
- 设备卡片。
- 设备详情抽屉。

设备卡片只展示最重要的信息和常用操作，复杂控制进入详情。

## 11.7 智能场景

- 显示 HA 场景。
- 收藏场景。
- 一键执行。
- 显示执行结果。
- 场景列表为空时提供清晰说明。

## 11.8 系统设置

分组：

1.  Home Assistant 连接。
2.  摄像头与 go2rtc。
3.  外观主题。
4.  系统托盘。
5.  开机启动。
6.  日志与诊断。
7.  关于软件。

## 11.9 视觉变量

| 变量     | 默认值    |
|:---------|:----------|
| 页面背景 | `#F5F6F8` |
| 卡片背景 | `#FFFFFF` |
| 主文字   | `#20242B` |
| 次级文字 | `#8A9099` |
| 主色     | `#3478F6` |
| 成功     | `#22A06B` |
| 警告     | `#E6A23C` |
| 错误     | `#E05252` |
| 分割线   | `#E8EAED` |

颜色不能作为唯一状态表达方式，必须同时有文字或图标。

------------------------------------------------------------------------

# 12. Rust 项目目录建议

``` text
mi-home-desktop/
├─ src/
│  ├─ assets/
│  ├─ components/
│  │  ├─ DeviceCard.vue
│  │  ├─ EntityControl.vue
│  │  ├─ CameraPlayer.vue
│  │  ├─ CameraGrid.vue
│  │  └─ ConnectionBadge.vue
│  ├─ views/
│  │  ├─ DashboardView.vue
│  │  ├─ MonitorView.vue
│  │  ├─ DevicesView.vue
│  │  ├─ ScenesView.vue
│  │  └─ SettingsView.vue
│  ├─ stores/
│  │  ├─ connection.ts
│  │  ├─ devices.ts
│  │  ├─ cameras.ts
│  │  └─ settings.ts
│  ├─ router/
│  ├─ types/
│  ├─ utils/
│  ├─ App.vue
│  └─ main.ts
│
├─ src-tauri/
│  ├─ capabilities/
│  │  └─ main.json
│  ├─ src/
│  │  ├─ main.rs
│  │  ├─ lib.rs
│  │  ├─ app_state.rs
│  │  ├─ commands/
│  │  │  ├─ mod.rs
│  │  │  ├─ ha.rs
│  │  │  ├─ devices.rs
│  │  │  ├─ cameras.rs
│  │  │  ├─ scenes.rs
│  │  │  └─ settings.rs
│  │  ├─ services/
│  │  │  ├─ mod.rs
│  │  │  ├─ ha_client.rs
│  │  │  ├─ ha_websocket.rs
│  │  │  ├─ device_service.rs
│  │  │  ├─ camera_service.rs
│  │  │  ├─ go2rtc_client.rs
│  │  │  ├─ scene_service.rs
│  │  │  └─ sync_service.rs
│  │  ├─ models/
│  │  ├─ database/
│  │  │  ├─ mod.rs
│  │  │  ├─ repository.rs
│  │  │  └─ migrations/
│  │  ├─ security/
│  │  ├─ events/
│  │  ├─ errors/
│  │  └─ utils/
│  ├─ Cargo.toml
│  └─ tauri.conf.json
│
├─ docs/
│  ├─ ARCHITECTURE.md
│  ├─ HOME_ASSISTANT_SETUP.md
│  ├─ CAMERA_COMPATIBILITY.md
│  ├─ API_CONTRACTS.md
│  └─ DEVELOPMENT.md
│
├─ package.json
├─ pnpm-lock.yaml
├─ vite.config.ts
├─ tsconfig.json
├─ README.md
└─ REQUIREMENTS.md
```

目录可以随实现调整，但必须保持 UI、Commands、Services、Models、Database 和 Security 的职责分离。不得把所有业务代码堆进 `main.rs`。

------------------------------------------------------------------------

# 13. 错误处理规范

统一错误类型：

| 错误码                    | 含义           | 用户提示                       |
|:--------------------------|:---------------|:-------------------------------|
| `HA_UNREACHABLE`          | HA 无法访问    | 检查地址和网络                 |
| `HA_AUTH_INVALID`         | Token 无效     | 重新配置访问令牌               |
| `HA_WS_DISCONNECTED`      | WebSocket 断开 | 正在重连                       |
| `HA_SYNC_FAILED`          | 同步失败       | 查看日志并重试                 |
| `ENTITY_NOT_FOUND`        | 实体不存在     | 重新同步设备                   |
| `SERVICE_UNSUPPORTED`     | 服务不支持     | 当前设备不支持此操作           |
| `SERVICE_CALL_FAILED`     | 服务调用失败   | 显示脱敏错误详情               |
| `CAMERA_OFFLINE`          | 摄像头离线     | 检查设备网络                   |
| `CAMERA_NO_STREAM`        | 无可用视频流   | 当前型号或接入方式未提供实时流 |
| `CAMERA_UNSUPPORTED`      | 视频协议不支持 | 查看兼容性说明                 |
| `CAMERA_TIMEOUT`          | 视频超时       | 重试连接                       |
| `GO2RTC_UNREACHABLE`      | go2rtc 不可用  | 检查视频服务地址               |
| `VIDEO_DECODE_FAILED`     | 解码失败       | 尝试兼容编码或转码             |
| `CREDENTIAL_STORE_FAILED` | 凭据保存失败   | 检查 Windows 凭据服务          |
| `DATABASE_ERROR`          | 数据库错误     | 记录日志并提示恢复             |

错误必须有：

- 稳定错误码。
- 用户可理解的说明。
- 是否可重试。
- 诊断日志上下文。

不允许把 Rust panic、完整堆栈或秘密数据直接展示给用户。

------------------------------------------------------------------------

# 14. 安全要求

1.  HA Token 只能在 Rust 后端读取和使用。
2.  Token 保存到 Windows Credential Manager。
3.  SQLite 只存 credential_ref。
4.  小米账号密码不由 Mi Home Desktop 管理。
5.  go2rtc 登录凭据由 go2rtc 独立管理。
6.  日志必须脱敏。
7.  HTTPS 证书验证默认开启。
8.  禁止默认关闭 TLS 验证。
9.  Tauri capabilities 遵循最小权限原则。
10. 不随意授予前端文件系统、Shell、进程执行权限。
11. Tauri Commands 必须校验参数和调用范围。
12. 视频服务默认仅在可信局域网开放。
13. 不将 go2rtc 管理端口或摄像头端口暴露到公网。
14. 远程访问优先使用 VPN。
15. 用户导出日志前提示日志可能包含设备名称和局域网地址。
16. 删除连接时提供是否同时删除系统凭据的选项。

------------------------------------------------------------------------

# 15. 日志与诊断

日志分级：

- ERROR：不可恢复错误。
- WARN：可恢复异常。
- INFO：连接、同步、控制操作摘要。
- DEBUG：开发调试信息。

默认日志级别 INFO。

日志应记录：

- 应用版本。
- HA 连接状态变化。
- 同步开始和结束。
- 设备操作结果。
- 摄像头会话建立和结束。
- 视频错误代码。
- 自动重连次数。

日志不得记录：

- Token。
- 密码。
- Authorization Header。
- 验证码。
- 完整的敏感视频 URL。
- 不必要的用户个人信息。

提供”导出诊断包”功能，导出前进行脱敏。

------------------------------------------------------------------------

# 16. 性能与稳定性目标

以下是验收目标，最终以目标硬件和实际网络测试为准。

| 指标             | 目标                                |
|:-----------------|:------------------------------------|
| 冷启动           | 普通 PC 上 5 秒内进入首页           |
| HA 连接测试      | 局域网正常时 3 秒内返回结果         |
| 首次设备同步     | 100 个实体规模下目标 5 秒内         |
| 状态事件到 UI    | 正常局域网下目标 1 秒内             |
| 普通设备服务调用 | 正常局域网下目标 2 秒内收到服务响应 |
| WebSocket        | 连续运行 24 小时无资源泄漏          |
| 四宫格监控       | 已验证视频源连续运行 30 分钟        |
| 页面切换         | 离开监控页后释放非必要播放会话      |
| 异常恢复         | HA 恢复后自动重新认证和订阅         |
| 数据持久化       | 正常退出后本地设置不丢失            |

视频延迟、帧率和 CPU 占用必须在实际摄像头和目标 PC 上测量，不能在未测试时写成保证值。

------------------------------------------------------------------------

# 17. 测试计划

## 17.1 单元测试

Rust：

- URL 校验。
- 错误码映射。
- 实体类型归一化。
- 控制参数校验。
- 数据库 Repository。
- 凭据引用管理。
- 摄像头状态机。
- 重连退避逻辑。

Vue：

- 设备卡片状态。
- 控制控件能力判断。
- 连接状态展示。
- 空状态和错误状态。
- 监控布局切换。

## 17.2 集成测试

- HA Token 正确/错误。
- HA 服务不可达。
- WebSocket 断线和恢复。
- 实体新增、更新、删除。
- 设备服务成功和失败。
- 数据库升级。
- go2rtc 可用/不可用。
- 摄像头连接超时。
- 摄像头流中断后恢复。

## 17.3 实机验收设备

至少准备：

- 一台已接入 HA 的灯或插座。
- 一台支持温度调节的设备（如有）。
- 用户自己的小米智能摄像机 2 云台版。
- Windows 10/11 测试环境。

## 17.4 摄像头兼容性专项测试

必须记录：

| 项目             | 实测值                    |
|:-----------------|:--------------------------|
| 米家商品名称     | 小米智能摄像机 2 云台版   |
| 购买年份         | 2022                      |
| HA Device Model  | 待实测                    |
| MIoT Model       | 待实测                    |
| 固件版本         | 待实测                    |
| Xiaomi Miot 版本 | 待记录                    |
| go2rtc 版本      | 待记录                    |
| 视频源方式       | HA / Xiaomi Source / 其他 |
| P2P 协议         | 待实测                    |
| 编码             | 待实测                    |
| WebRTC 播放      | 通过 / 失败               |
| 连续播放 30 分钟 | 通过 / 失败               |
| 断线自动恢复     | 通过 / 失败               |
| 云台控制         | 待验证                    |

不得仅根据购买年份推断 `chuangmi.camera.039a01` 或其他 model。

------------------------------------------------------------------------

# 18. 开发阶段与任务拆分

## Phase 0：环境与设备验证

目标：确认技术路线可行。

任务：

- [ ] 安装 Rust stable。
- [ ] 安装 Node.js LTS。
- [ ] 安装 pnpm。
- [ ] 安装 Visual Studio C++ Build Tools。
- [ ] 安装 WebView2 Runtime。
- [ ] 创建 Tauri 2 + Vue 3 项目。
- [ ] Home Assistant 独立部署完成。
- [ ] 安装 Xiaomi Miot。
- [ ] 创建 HA Token。
- [ ] 从 HA 读取目标摄像头实际 Model。
- [ ] 记录摄像头固件版本。
- [ ] 测试普通设备控制。
- [ ] 测试 go2rtc Xiaomi Source。
- [ ] 测试摄像头视频播放。

交付物：

- `docs/CAMERA_COMPATIBILITY.md`
- `docs/HOME_ASSISTANT_SETUP.md`
- 技术验证记录。

Phase 0 未完成前，不得宣称目标摄像头已兼容。

## Phase 1：桌面基础框架

- [ ] 初始化 Tauri 2。
- [ ] 初始化 Vue 3 + TypeScript + Vite。
- [ ] 集成 Naive UI。
- [ ] 建立主窗口和导航。
- [ ] 建立 Pinia Store。
- [ ] 建立 Rust 模块目录。
- [ ] 建立统一错误结构。
- [ ] 建立 tracing 日志。
- [ ] 建立 SQLite 和迁移。
- [ ] 建立 Tauri capabilities。

验收：客户端能启动，页面可导航，数据库可读写。

## Phase 2：Home Assistant 连接

- [ ] 连接配置页。
- [ ] URL 校验。
- [ ] Token 保存到 Credential Manager。
- [ ] REST 连通性测试。
- [ ] WebSocket 认证。
- [ ] 设备状态同步。
- [ ] 事件订阅。
- [ ] 自动重连。
- [ ] 连接状态 UI。

验收：能连接真实 HA，读取实体，并在状态变化时刷新 UI。

## Phase 3：设备管理与控制

- [ ] Entity 数据模型。
- [ ] Area 数据模型。
- [ ] 设备列表。
- [ ] 房间分组。
- [ ] 搜索和筛选。
- [ ] 收藏和隐藏。
- [ ] 设备详情。
- [ ] light 控制。
- [ ] switch 控制。
- [ ] climate 控制。
- [ ] fan 控制。
- [ ] sensor 展示。
- [ ] 操作结果反馈。

验收：至少真实控制一台灯、一台插座；若有空调，再完成 climate 控制。

## Phase 4：摄像头

- [ ] 获取 HA camera 实体。
- [ ] 获取截图能力。
- [ ] 获取视频流能力。
- [ ] 配置 go2rtc 地址。
- [ ] 配置摄像头 stream alias。
- [ ] 建立 Camera Provider 抽象。
- [ ] WebRTC 单路播放。
- [ ] 视频状态管理。
- [ ] 视频断线重连。
- [ ] 退出释放会话。
- [ ] 四宫格。
- [ ] H.265 / H.264 兼容性验证。
- [ ] 必要时增加 FFmpeg 转码。

验收：至少一个已验证视频源在目标 Windows 环境连续播放 30 分钟。

## Phase 5：场景与完善

- [ ] HA 场景同步。
- [ ] 场景收藏。
- [ ] 场景执行。
- [ ] 系统托盘。
- [ ] 开机自启动。
- [ ] 窗口状态保存。
- [ ] 深色主题。
- [ ] 日志导出。
- [ ] 诊断信息页面。

## Phase 6：发布

- [ ] Release 构建。
- [ ] Windows 安装包。
- [ ] 安装/卸载测试。
- [ ] WebView2 环境检查。
- [ ] 数据库升级测试。
- [ ] 安全检查。
- [ ] 完整回归测试。
- [ ] README 和用户部署说明。
- [ ] 发布 V1.0.0。

------------------------------------------------------------------------

# 19. V1.0 验收标准

## P0：必须完成

- [ ] Windows 客户端正常安装和启动。
- [ ] 能保存 HA 连接配置。
- [ ] Token 不以明文存储。
- [ ] 能连接 HA。
- [ ] 能读取真实设备实体。
- [ ] 能显示设备在线/离线状态。
- [ ] 能控制至少一种 light 或 switch。
- [ ] WebSocket 状态变化能更新 UI。
- [ ] HA 断线后能够自动重连。
- [ ] 数据库配置持久化。
- [ ] 异常不会导致应用整体崩溃。

## P1：V1.0 核心目标

- [ ] 房间分组。
- [ ] 设备搜索。
- [ ] 收藏设备。
- [ ] 设备详情。
- [ ] 场景列表和执行。
- [ ] 摄像头兼容性检测。
- [ ] 至少一个真实摄像头视频源可播放（若 Phase 0 验证成功）。
- [ ] 单画面和四宫格监控。
- [ ] 视频断流提示和重连。
- [ ] 关闭监控时释放会话。

如果目标摄像头经验证无法通过当前可维护方案取流，必须在发布说明中明确列出限制，不得伪造”实时监控已完成”。

## P2：后续增强

- [ ] 九宫格优化。
- [ ] 云台控制。
- [ ] 双向语音。
- [ ] 视频截图。
- [ ] 本地录像。
- [ ] 历史数据图表。
- [ ] 更多设备类型专用控制面板。
- [ ] 自动更新。
- [ ] 多 Home Assistant 实例。

------------------------------------------------------------------------

# 20. Git 仓库与协作规范

## 20.1 建议仓库文件

- `README.md`：项目介绍、截图、功能和快速开始。
- `REQUIREMENTS.md`：本需求文档。
- `CHANGELOG.md`：版本变更。
- `LICENSE`：项目许可证。
- `.gitignore`：忽略构建产物、数据库、密钥和本地配置。
- `docs/`：架构、HA 部署、摄像头兼容性和开发说明。

## 20.2 Git 提交规范

建议采用 Conventional Commits：

- `feat:` 新功能。
- `fix:` 修复问题。
- `refactor:` 重构。
- `docs:` 文档。
- `test:` 测试。
- `chore:` 工程维护。

示例：

``` text
feat(ha): add Home Assistant connection
feat(device): implement entity state synchronization
feat(camera): add go2rtc WebRTC player
fix(ws): reconnect after Home Assistant disconnect
docs: add camera compatibility report
```

## 20.3 禁止提交

- HA Token。
- 小米账号密码。
- 摄像头密码。
- 真实用户数据库。
- 日志中的敏感信息。
- `.env` 中的秘密配置。
- 本地构建目录和临时文件。

------------------------------------------------------------------------

# 21. 第一周实际开工清单

按以下顺序执行，不要先做完整 UI：

### 第 1 步：创建项目

- [ ] 创建 Git 仓库。
- [ ] 初始化 Tauri 2 + Vue 3 + TypeScript。
- [ ] 确认 Windows Debug 构建成功。
- [ ] 建立目录结构。
- [ ] 将本文件提交为 `REQUIREMENTS.md`。

### 第 2 步：打通 Home Assistant

- [ ] 独立部署 Home Assistant。
- [ ] 安装 Xiaomi Miot。
- [ ] 接入现有米家设备。
- [ ] 创建 HA Token。
- [ ] 用 Rust Reqwest 测试 `GET /api/`。
- [ ] 获取 `/api/states`。
- [ ] 控制一台真实灯或插座。

### 第 3 步：确认摄像头型号

- [ ] 在 HA 设备页查看摄像头。
- [ ] 记录 Manufacturer、Model、Firmware。
- [ ] 记录 Xiaomi Miot 生成的 Entity。
- [ ] 记录摄像头实体是否支持截图和视频流。
- [ ] 不根据 2022 购买年份猜测 model。

### 第 4 步：单独验证视频

- [ ] 安装 go2rtc。
- [ ] 记录 go2rtc 版本。
- [ ] 使用官方 Xiaomi Source 测试登录。
- [ ] 获取目标摄像头列表。
- [ ] 测试视频流。
- [ ] 测试 WebRTC。
- [ ] 在 Tauri WebView2 中播放。
- [ ] 若失败，保留日志并记录失败阶段。
- [ ] 只有确认必要时才增加 FFmpeg 转码。

### 第 5 步：开始正式客户端功能

- [ ] 实现连接设置。
- [ ] 实现设备列表。
- [ ] 实现 WebSocket 状态同步。
- [ ] 实现设备控制。
- [ ] 实现监控单画面。
- [ ] 实现四宫格。

------------------------------------------------------------------------

# 22. 开发完成定义（Definition of Done）

一个功能只有满足以下条件，才能标记为完成：

1.  代码已合并到主分支。
2.  有对应 Rust 或前端类型定义。
3.  有必要的输入校验和错误处理。
4.  不泄露凭据。
5.  有基本测试。
6.  真实环境验证通过（涉及 HA 或摄像头时）。
7.  UI 有加载、空状态和错误状态。
8.  日志可用于定位问题。
9.  文档已更新。
10. 不破坏现有功能。

------------------------------------------------------------------------

# 23. 参考资料

以下链接作为实现时的主要技术依据，开发前应再次核对对应版本：

- Tauri 2 官方文档：https://v2.tauri.app/
- Tauri Capabilities：https://tauri.app/security/capabilities/
- Vue 3：https://vuejs.org/
- Home Assistant：https://www.home-assistant.io/
- Home Assistant REST API：https://developers.home-assistant.io/docs/api/rest/
- Home Assistant WebSocket API：https://developers.home-assistant.io/docs/api/websocket/
- Xiaomi Miot：https://github.com/al-one/hass-xiaomi-miot
- go2rtc：https://github.com/AlexxIT/go2rtc
- go2rtc Xiaomi 摄像头兼容列表：https://github.com/AlexxIT/go2rtc/issues/1982

------------------------------------------------------------------------

# 24. 最终项目定义

Mi Home Desktop 是一款基于 Rust、Tauri 2 和 Vue 3 的 Windows 智能家居桌面客户端。

它通过 Home Assistant 统一接入米家设备，通过 REST API 和 WebSocket 实现设备查询、控制和实时状态同步；通过可选的 go2rtc 视频服务接入兼容摄像头，并在桌面端提供实时监控、多画面布局和视频状态管理。

**V1.0 的核心交付目标：**

- 一个可安装、可运行的 Windows 桌面客户端。
- 一个稳定的 Home Assistant 连接模块。
- 一套可扩展的设备管理和控制模块。
- 一套可靠的实时状态同步机制。
- 一个经过实机验证的摄像头接入与播放方案。
- 一套可维护的 SQLite 本地配置和安全凭据管理机制。

项目优先保证真实设备、真实视频源和真实操作结果，不以静态演示数据代替功能实现。
