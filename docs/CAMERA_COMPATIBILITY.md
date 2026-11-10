# 摄像头接入与实机验证

## 当前支持范围

客户端已接入 HA 摄像头截图、go2rtc WebRTC SDP 协商、单画面/四宫格、单路全屏、音轨存在时静音切换、断流重试和会话释放。

这些是客户端代码能力，不代表用户的小米摄像头已经兼容。当前没有真实摄像头、目标 Windows/WebView2 或 30 分钟连续播放的通过记录。

HA 声明 `supported_features` 中的流能力时，界面只标记“HA 声明流能力（未验证）”。当前 HA 路线提供手动截图，HA 原生 WebRTC/Stream 播放尚未接入。MSE/HLS 回退、九宫格、录像、云台、双向语音与自动 FFmpeg 转码也尚未实现。

## HA 截图

先连接并同步 Home Assistant。监控中心可选择 `camera.*` 实体；默认是 HA 截图模式。点击“获取 HA 截图”后，Rust 使用系统凭据中的 HA Token 请求 `GET /api/camera_proxy/{entity_id}`，验证状态、图片类型和大小，再返回图片内容。

只接受 JPEG、PNG、WebP，最大 8 MiB；不接受 SVG/HTML 或认证 URL。界面始终标记“截图（非实时）”，不自动轮询或伪装成视频。

## go2rtc WebRTC

1. 独立部署合适版本的 go2rtc，并记录版本。
2. 在 go2rtc 自身的认证/配置流程接入 Xiaomi Source 或其他已有兼容源。客户端不保存小米账号和摄像头密码。
3. 在 go2rtc 中先验证来源和编码；为来源配置固定别名，例如 `camera_living`。
4. 在客户端“系统设置 → 摄像头与 go2rtc”填写 API 基地址，点击“检测接口与别名”，然后保存并启用服务。
5. 选中 HA 摄像头实体，播放方式改为 go2rtc，填入对应别名并保存。
6. 在监控中心选择单画面或四宫格，验证真实画面和状态。

API 基地址禁止包含用户名、密码、查询参数或 URL 片段；流别名只允许字母、数字、下划线、点和连字符，不能填入 RTSP URL。`GET /api/streams` 的源地址和认证信息不会传到前端，只返回合法流别名。API 可访问仅证明接口响应，不证明取流或视频解码成功。

当前未接入 go2rtc API 认证。如果接口返回 401/403，会明确报告 `GO2RTC_AUTH_REQUIRED`；不会自动关闭认证或 TLS 验证。该版本仅适用于已有受控可信网络访问路径的 API。需要认证的部署须后续扩展受控鉴权支持。

go2rtc API 常用端口为 1984，WebRTC 常用 8555 TCP/UDP，以实际部署为准。WebRTC 媒体由 WebView2 直接与 go2rtc 建立连接，API 走 Rust。API 可达而媒体端口不可达、候选地址不正确或编解码不兼容，都可能导致无画面。默认不配置公网 STUN/TURN；远程场景需要另行验证 VPN/网络路径。不要将管理接口和摄像头端口直接暴露公网。

## 播放状态与资源

只有收到已解码的视频帧才显示“正在播放”；SDP 返回或 PeerConnection 进入 connected 状态均不作为播放成功依据。

- 首帧超时：30 秒。
- 已播放后的无帧超时：10 秒（后台页面暂停检测）。
- 自动重试等待：1、2、5、10、30 秒，最多 5 次，之后等待手动重试。
- 连续有帧超过 60 秒后重置重试预算。
- 同一摄像头最多一个本地会话，最多四路并发。
- 单画面/四宫格切换复用仍可见的播放器；隐藏或离开监控页时关闭 PC、停止媒体轨道、清空视频对象并释放租约。
- 协商取消、延迟创建的会话会释放，过期会话不能继续协商。

Rust 会话租约有效期 45 秒，客户端每 15 秒续约。媒体资源由浏览器的 RTCPeerConnection 管理，关闭 PC 后 go2rtc 会清理断开的消费者；当前 SDP 接口没有额外的可用消费端 DELETE 句柄。不会把前端报告的状态当作实机验收记录。

## 编码

协商时优先选择浏览器支持的 H.264。H.265/HEVC 的可用性仍依赖 Windows/WebView2、摄像头和 go2rtc，不做预先保证。若原始编码失败，应先记录型号、固件、编码及错误阶段；确认必要后，在视频服务侧评估 FFmpeg 转 H.264 的开销。

## 用户摄像头验证记录

| 项目 | 当前记录 |
|---|---|
| 商品名称 | 小米智能摄像机 2 云台版（需求文档提供） |
| 购买年份 | 2022（需求文档提供） |
| HA Device Model / MIoT Model | 待实测，不根据年份推断 |
| 固件、账号地区 | 待实测 |
| Xiaomi Miot、go2rtc 版本 | 待记录 |
| 截图与实际视频来源 | 待验证 |
| 原始编码、分辨率、帧率 | 待验证 |
| Windows / WebView2 版本 | 待记录 |
| WebRTC 单路与四宫格 | 待实机验证 |
| 连续播放 30 分钟、断流恢复 | 待实机验证 |
| CPU、延迟和内存 | 待测量 |

## 代码验证

Rust 使用本地 HTTP fixture 检查 API、SDP 协商、认证错误、取消、过期、并发上限和图片响应。浏览器测试用 IPC/PeerConnection fixture 检查布局复用、隐藏释放、迟到会话释放和无视频帧时不显示播放。这些测试不等同于真实摄像头视频解码或具体型号兼容性。另有本地 canvas MediaStream 测试，验证真实视频元素的接帧、播放状态和轨道释放；它不覆盖编码与网络传输。

原生双 PeerConnection 测试会先确认 video offer/answer 被接受，并检查是否收集到本地 ICE 候选。当前托管 Chromium 未提供本地 ICE 候选，因此该媒体互连测试明确跳过（不是通过）；在支持本地 WebRTC 的环境中仍会执行实际接帧断言。最终应在 Windows/WebView2 与真实 go2rtc 路径上补验。

接口依据：

- [HA Camera 源码](https://github.com/home-assistant/core/tree/dev/homeassistant/components/camera)
- [go2rtc WebRTC 同步接口](https://github.com/AlexxIT/go2rtc/blob/master/internal/webrtc/server.go)
- [go2rtc 摄像头兼容讨论](https://github.com/AlexxIT/go2rtc/issues/1982)
