# chuangmi.camera.039a01 远程播放适配记录

调研日期：2026-11-12。用户设备固件为 `5.1.6_0433`；电脑与摄像头不在同一局域网，用户已确认米家 App 在移动网络下可以正常直播。这证明原厂客户端的远程观看路径可用，不等于第三方客户端已经取得相同的连接能力。

## 已核实的接口与限制

- 型号公开 MIoT v2 规格包含 siid=6 的 `p2p-stream`，aiid=1 为 `start-p2p-stream`，没有输出地址；没有声明可直接读取的直播 HLS/RTSP 地址。不能仅调用这个动作就视为拿到了可播放的视频流。[公开规格](https://miot-spec.org/miot-spec-v2/instance?type=urn%3Amiot-spec-v2%3Adevice%3Acamera%3A0000A01C%3Achuangmi-039a01%3A2)
- 小米官方 MiLoCo 的历史支持列表回复包含该型号；社区 MiBeeNvr 有该型号解析为 CS2 的记录。它们提供适配线索，不能替代同固件、同账号的远程播放实测。[官方回复](https://github.com/XiaoMi/xiaomi-miloco/issues/176#issuecomment-3678836300) · [型号实例记录](https://github.com/Mi-Bee-Studio/MiBeeNvr/issues/48)
- go2rtc 当前 CS2 实现对目标主机 32108 端口发送 LAN search，然后协商 UDP/TCP。该代码未提供这个型号跨互联网的云端会合和中继连接流程，因此把本地摄像头 IP 交给 go2rtc 不能满足本次远程需求。[固定版本源码](https://github.com/AlexxIT/go2rtc/blob/990f4de3cc21d1cca917d02d0bc40f2fabe75a49/pkg/xiaomi/miss/cs2/conn.go)
- go2rtc 从 `/v2/device/miss_get_vendor` 取得协议、设备公钥和签名。请求包含一次性 Curve25519 客户端公钥。当前设备实际返回哪个 vendor，必须通过登录后的账号验证，不能只按型号固定为 CS2。[授权请求实现](https://github.com/AlexxIT/go2rtc/blob/990f4de3cc21d1cca917d02d0bc40f2fabe75a49/internal/xiaomi/xiaomi.go)
- 官方 SDK 包装代码存在 Windows 加载分支，但本次查看的仓库实际库文件只有 Linux/macOS `.so`/`.dylib`，没有可直接随桌面客户端打包的 Windows DLL。其 LICENSE 也没有授予在其他 APP 中复用整个授权作品的许可，因此本项目没有复制或捆绑该 SDK。[库目录](https://github.com/XiaoMi/xiaomi-miloco/tree/05cdb1c77310afcfb77e64349bfb5efbeebded05/backend/miot/src/miot/libs) · [许可证](https://github.com/XiaoMi/xiaomi-miloco/blob/05cdb1c77310afcfb77e64349bfb5efbeebded05/LICENSE.md)

## 本轮已实现：云端视频授权检测

摄像头详情中的“检测视频授权”复用已登录米家会话。后端确认 did 存在于当前账号、型号匹配且在线，生成一次性公钥，调用视频授权接口。检测不调用开始播放动作，不拨打摄像头局域网地址，也不启动采集或转发。

界面和可复制报告仅包含型号、协议分类、云端授权是否取得、检测时间，以及固定为 false 的 `remotePlaybackVerified`。不包含账号 ID、设备 ID、服务令牌、视频签名、私钥、P2P UID 或播放地址。一次性私钥用后清除，授权响应不写入配置或日志。关闭详情后返回的迟到结果不能出现在其他设备详情中。

这个检测版本**没有实现远程播放**。它的用途是让真实账号和这台摄像头确认接口授权及协议选择，作为后续远程传输适配的依据。

## 后续接入与验收

1. 在用户的 Windows 客户端运行检测，确认云端是否授予视频授权和实际协议，不要求用户提供账号密码或令牌。
2. 接入对应协议的跨网络会合、NAT 穿透及必要中继；不能用局域网成功代替远程验证。需要通过真实设备进行握手、认证及持续保活测试。
3. 接收并解密媒体帧，验证 H.264/H.265 与音频格式；按 WebView2 实际解码能力选择直通或转换，真实解码帧出现后才能显示播放成功。
4. 在电脑与摄像头分处不同网络时验收首次出画、持续播放、断线重连、关闭详情释放连接与退出账号清理。保留现有 HA/go2rtc 高级功能，不把它当成米家直连远程播放已经实现。
