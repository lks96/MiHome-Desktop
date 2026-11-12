# chuangmi.camera.039a01 远程播放适配记录

调研日期：2026-11-12。用户设备固件为 `5.1.6_0433`；电脑与摄像头不在同一局域网，用户已确认米家 App 在移动网络下可以正常直播。这证明原厂客户端的远程观看路径可用，不等于第三方客户端已经取得相同的连接能力。

用户已在检测版实测并回报：`transport=cs2`、`cloudAuthorizationGranted=true`、`remotePlaybackVerified=false`。账号的视频云授权阶段已通过。检测版将 `remotePlaybackVerified` 固定为 false，所以这里表示“未实现/未验证”，不是跨网连接失败或摄像头不支持的证据。报告不包含建立实际视频会话所需的私密材料，也不应要求用户把这些材料发给开发者。

用户进一步明确：摄像头侧没有可用常开设备，只接受观看电脑扫码后直接远程播放。后续不以局域网网关、NAS、HA 或家中电脑中转替代这个目标。

## 已核实的接口与限制

- 型号公开 MIoT v2 规格包含 siid=6 的 `p2p-stream`，aiid=1 为 `start-p2p-stream`，没有输出地址；没有声明可直接读取的直播 HLS/RTSP 地址。不能仅调用这个动作就视为拿到了可播放的视频流。[公开规格](https://miot-spec.org/miot-spec-v2/instance?type=urn%3Amiot-spec-v2%3Adevice%3Acamera%3A0000A01C%3Achuangmi-039a01%3A2)
- 小米官方 MiLoCo 的历史支持列表回复包含该型号；社区 MiBeeNvr 有该型号解析为 CS2 的记录。它们提供适配线索，不能替代同固件、同账号的远程播放实测。[官方回复](https://github.com/XiaoMi/xiaomi-miloco/issues/176#issuecomment-3678836300) · [型号实例记录](https://github.com/Mi-Bee-Studio/MiBeeNvr/issues/48)
- go2rtc 当前 CS2 实现对目标主机 32108 端口发送 LAN search，然后协商 UDP/TCP。该代码未提供这个型号跨互联网的云端会合和中继连接流程，因此把本地摄像头 IP 交给 go2rtc 不能满足本次远程需求。[固定版本源码](https://github.com/AlexxIT/go2rtc/blob/990f4de3cc21d1cca917d02d0bc40f2fabe75a49/pkg/xiaomi/miss/cs2/conn.go)
- go2rtc 从 `/v2/device/miss_get_vendor` 取得协议、设备公钥和签名。请求包含一次性 Curve25519 客户端公钥。协议应以登录账号的实际响应为准；本次用户已实测返回 CS2，客户端仍不能只按型号固定该值。[授权请求实现](https://github.com/AlexxIT/go2rtc/blob/990f4de3cc21d1cca917d02d0bc40f2fabe75a49/internal/xiaomi/xiaomi.go)
- 官方 SDK 包装代码存在 Windows 加载分支，但本次查看的仓库实际库文件只有 Linux/macOS `.so`/`.dylib`，没有可直接随桌面客户端打包的 Windows DLL。其 LICENSE 也没有授予在其他 APP 中复用整个授权作品的许可，因此本项目没有复制或捆绑该 SDK。[库目录](https://github.com/XiaoMi/xiaomi-miloco/tree/05cdb1c77310afcfb77e64349bfb5efbeebded05/backend/miot/src/miot/libs) · [许可证](https://github.com/XiaoMi/xiaomi-miloco/blob/05cdb1c77310afcfb77e64349bfb5efbeebded05/LICENSE.md)

## 本轮已实现：云端视频授权检测

摄像头详情中的“检测视频授权”复用已登录米家会话。后端确认 did 存在于当前账号、型号匹配且在线，生成一次性公钥，调用视频授权接口。检测不调用开始播放动作，不拨打摄像头局域网地址，也不启动采集或转发。

界面和可复制报告仅包含型号、协议分类、云端授权是否取得、检测时间，以及固定为 false 的 `remotePlaybackVerified`。不包含账号 ID、设备 ID、服务令牌、视频签名、私钥、P2P UID 或播放地址。一次性私钥用后清除，授权响应不写入配置或日志。关闭详情后返回的迟到结果不能出现在其他设备详情中。

这个检测版本**没有实现远程播放**。它的用途是让真实账号和这台摄像头确认接口授权及协议选择，作为后续远程传输适配的依据。

## 后续接入与验收

1. 已完成：用户在 Windows 客户端实测云端视频授权通过，实际协议为 CS2；未要求用户提供账号密码或令牌。
2. 接入对应协议的跨网络会合、NAT 穿透及必要中继；不能用局域网成功代替远程验证。需要通过真实设备进行握手、认证及持续保活测试。
3. 接收并解密媒体帧，验证 H.264/H.265 与音频格式；按 WebView2 实际解码能力选择直通或转换，真实解码帧出现后才能显示播放成功。
4. 在电脑与摄像头分处不同网络时验收首次出画、持续播放、断线重连、关闭详情释放连接与退出账号清理。保留现有 HA/go2rtc 高级功能，不把它当成米家直连远程播放已经实现。

## 授权实测后的进一步核查

- Rust 项目 lite-nvr 的 CS2 模块仍是 go2rtc 局域网握手的移植，使用目标主机 32108 端口；源码也说明尚未经过真实摄像头验证。源码注释出现 relay 字样不代表已实现小米云跨网会合或中继。[实现](https://github.com/BenLocal/lite-nvr/blob/59237b00aef860bb9a44fa85dae02ce94f2fc923/crates/xiaomi/src/cs2.rs)
- Windows 项目 xiaomi-camera-viewer 明确要求摄像头与电脑在同一局域网，不能直接作为本需求的远程实现。[项目要求](https://github.com/lubos-hnanicek/xiaomi-camera-viewer/blob/581debe0bca0e90a1d0494bca961af08a4166a29/README.md#requirements)
- CS2 厂商公开 PPCS API 文档描述了远程 P2P 与 relay 模式，初始化需要服务器参数，连接需要目标 P2P ID，`PPCS_ConnectByServer` 还接收 ServerString。公开示例/头文件并不等于已经获得与小米这个设备和服务兼容的 Windows 运行库或远程线上协议实现。[厂商 API 文档](https://github.com/cs2-network/P2P-solution/blob/2dafd8a62dd89f5a1ff6b60fabda535058d7ecbb/Doc/PPCS%20P2P%20API.pdf)

当前实际阻塞是：尚未取得可集成且与该设备兼容的 Windows CS2 远程连接实现，也未独立实现小米环境的会合、穿透与中继流程。现有检测报告没有导出私密连接材料，不能据此推断服务端没有提供相应参数；也不能把授权成功转写成远程播放可用。继续开发需要解决这部分实现与接口资料，之后才有可进行真实跨网测试的播放器。此轮不新增只有检测功能的安装包。
