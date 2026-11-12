# 米家账号直连（开发中）

默认首页提供米家 App 扫码登录，Home Assistant 位于侧栏“高级”入口。界面来源选择仅保存为非敏感显示偏好；两种来源的设备和操作不混用。

## 使用流程

1. 在 Windows 桌面客户端中选择与米家 App 相同的服务地区，默认中国大陆。
2. 点击“用米家 App 扫码登录”，在手机米家 App 使用扫一扫并确认授权。
3. 自动同步家庭、房间、设备名称、型号与在线状态。设备列表每分钟在页面可见时刷新，也可手动刷新。
4. 打开在线设备的“查看状态与控制”，客户端自动获取型号的 MIoT 能力并读取状态。支持已声明且在白名单中的布尔开关、枚举和数值范围控制。
5. 操作后重新读取设备状态，不把接口接受指令直接视为设备已改变。退出账号会删除本机保存的会话并清空设备列表。

未勾选“记住登录”时只保留进程内会话，下次启动重新扫码。勾选时，用户 ID、ssecurity 与 serviceToken 以一个独立凭据保存到 Windows Credential Manager；不保存密码、passToken、二维码或长轮询地址，不写入 SQLite、浏览器存储或日志。凭据保存失败时不会显示为已成功记住登录。

## 已实现范围

- 获取与取消二维码，串行长轮询、超时和过期处理。取消后返回的授权不能写入凭据。
- 校验扫码、长轮询、会话回调与每一次重定向的 HTTPS 主机；不跟随任意第三方回调。
- 米家云 RC4-drop1024 加密、SHA-256 nonce 派生和 SHA-1 协议签名。所有网络请求由 Rust 执行，前端只获得二维码图片及过滤后的设备摘要。
- `/home/device_list`、`/v2/homeroom/gethome_merged`、`/miotspec/prop/get`、`/miotspec/prop/set`。
- 基于 `miot-spec.org` 的型号与实例说明加载控制。后端再次校验设备属于当前账号、属性可写、类型、枚举、范围与步长；不暴露设备 token、IP 或安全属性。

较旧的非 MIoT 设备、仅蓝牙设备、网关子设备、跨地区或共享设备的能力仍需真实账号验证。登录本身不保证所有型号可控。没有公开能力的设备保留在列表中并提示待支持。

**摄像头：** 按型号自动识别。`chuangmi.camera.039a01` 提供 CS2 远程播放测试入口，包含会合/中继、MISS 和 Windows 视频组件；真实跨网与目标固件仍待实测，首版无音频。其他型号仍提示待适配。未实现云端录像或摄像头专用动作。原有 HA/go2rtc 视频接入仍可从高级入口使用。米家云场景尚未接入，HA 场景仍可在 HA 模式使用。

## 验证与限制

本地 Rust HTTP 测试覆盖二维码到会话的完整交换、恢复和退出、设备与房间同步、真实签名/加密请求、控制后读回、未知设备拒绝、取消中的授权、恶意回调、二维码过期及凭据保存失败。加密固定向量使用独立 PyCryptodome 生成，PyCryptodome 不是应用依赖。

Playwright 使用测试目录内的 IPC fixture 验证首页、手机确认后的设备展示、控制、摄像头能力提示、取消后的迟到响应和凭据故障。fixture 设备不会进入产品。

`v0.2.0-test.1` 已完成 Windows x64 Release 编译和 NSIS 安装包生成，并通过真实小米接口获取二维码的检查。小米会返回地区账号与长轮询子域名（例如 `sgp.lp.account.xiaomi.com`），本版已兼容这些官方账号子域名，同时继续拒绝第三方回调、明文 HTTP、异常端口与带用户信息的 URL。手机确认后的授权、真实设备控制和 Windows/WebView2 桌面运行仍待实机验收；不能把二维码获取成功视为完整登录及全部设备兼容验证。

真实联调需允许 `account.xiaomi.com` 及 `*.account.xiaomi.com`、`sts.api.io.mi.com`、`api.io.mi.com`（以及所选地区对应的 `de/us/ru/tw/sg/in/i2.api.io.mi.com`）和 `miot-spec.org`。Windows 交叉构建另外需要 `snapshot.debian.org` 与 `download.visualstudio.microsoft.com`。不需要把账号密码或令牌提供给开发者。

协议行为参考社区项目 [Xiaomi Cloud Tokens Extractor](https://github.com/PiotrMachowski/Xiaomi-cloud-tokens-extractor) 的 QR 流程和 [Xiaomi Miot](https://github.com/al-one/hass-xiaomi-miot) 的米家云/MIoT 请求。小米服务接口发生变化时需重新验证。

`v0.2.1-camera.1` 新增 `chuangmi.camera.039a01` 的视频授权检测，只输出经过筛选的协议与授权状态。该检测不代表跨网络直播已经实现，使用步骤与技术边界见 [039a01 远程适配记录](CAMERA_039A01_REMOTE.md)。

`v0.3.0-cs2.1` 的远程测试实现与验收边界见 [CS2 协议说明](CS2_PROTOCOL.md)。原有扫码与高级 HA/go2rtc 保留；关闭播放页、隐藏窗口或退出账号会释放远程会话。

`v0.3.1-cs2.1` 修正 `0x28` 非请求消息造成的认证阶段断线，并兼容认证响应的已知字节序与 NUL 填充格式；Schema 3 报告仅输出结构性诊断，实际出画仍待修正版实测。
