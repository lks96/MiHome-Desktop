# CS2 远程实现与验证边界

本项目以用户提供的技术方案为线索独立实现 Rust 协议，不复制无许可证的 micam Java 源码、测试或二进制。协议研究依据固定为 [micam 2cc9d9e](https://github.com/eugeneRover/micam/tree/2cc9d9e7607014d1d05c17bc5676544fbb0d5d24)、[MIT PPPP 文档](https://github.com/magicus/pppp-dissector/tree/3ea9a7261f028fe2c75d141cb430bc29a162e2b0) 与既有 go2rtc MISS 资料。测试使用本项目生成的协议材料和视频，不含真实账号、摄像头标识或流量。

## 已编写的链路

1. 复用当前扫码会话，校验设备属于账号、在线、型号为 `chuangmi.camera.039a01`。为播放生成新 X25519 密钥对，用同一公钥申请 `/v2/device/miss_get_vendor`；严格解析 `vendor.vendor_params.p2p_id` / `init_string` 与结果层的 `public_key` / `sign`，不复用旧探测报告。
2. 解码 A–P nibble / 54 字节协议表 / running XOR 的 init string，校验 IPv4、数量和长度。拒绝私网、回环、多播、非法地址；不编译固定服务器列表。服务器参数不出后端。
3. 向授权参数中的会合节点 UDP 32100 发 `0x67`，收集合法来源的 `0x82` 候选及每份 token，分别注册 `0x83`、Hello `0x70` / `0x71`、端口分配 `0x72` / `0x73`。向会合节点提交 `0x80`，向协商会话端口发注册包打开 NAT 映射，等候选会话端口返回 `0x84`。每阶段有限重试，总握手有时限。仅实现 Relay，未实现直连打洞或 TCP fallback。
4. UDP socket 固定连接到所选会话端点；完成 `0x28` 的 88 字节会话连接回复，处理重复请求。DRW 外层大端长度、通道与序号；通道 0 控制、通道 2 媒体。支持 1024 字节发送分片、乱序重组、重复 ACK、16 位回绕、发送重发、ACK 超时、缓冲和队列上限、保活与关闭。
5. MISS `0x100` 明文认证，解析 `0x101` 的 JSON result，不按子串认定成功。X25519 后执行 HSalsa20（NaCl box precompute），拒绝退化共享密钥；命令按 ChaCha20 随机 8 字节 nonce 封装为 `0x1001`。启动 `0x102`，视频质量 2、声音关闭。
6. 媒体头为 32 字节、字段小端，与 CS2 外层分开处理。校验长度、codec 4/5、nonce、Annex-B 前缀；从关键帧启动。H.264 重封装，H.265 在 Windows 以 FFmpeg 软件解码并调用 `h264_mf` 编成 H.264，输出 HLS。原始帧率首版按 20fps 重建单调播放时间轴，尚未精确复现摄像头可变帧率。
7. HLS 仅在随机 `127.0.0.1` 端口提供，独立随机路径 token；校验 Host、Origin、文件名与大小，不允许目录遍历。页面通过 IPC 只取得安全状态与本机媒体地址。hls.js 在 WebView2 中解码；只有视频帧回调才显示“正在播放”。
8. 停止、抽屉关闭、页面切换、隐藏窗口、退出账号和退出程序会停止会话；后端 lease 防止页面丢失后常驻取流。发送 VIDEO_STOP / CLOSE，杀死并等待 FFmpeg，关闭 HTTP/UDP 与缓存；异常强制中止另由 kill-on-drop 兜底。前端故障最多自动重试三次，每次重新取得授权。

## 使用与诊断

安装完整包后扫码 → 打开目标摄像头 → “远程播放”。不要只拷贝主 EXE，便携版须保留同目录 `ffmpeg/`；Windows N/精简系统可能缺少 Media Foundation，需要安装微软 Media Feature Pack。其他型号仍显示待适配。

“连接结果”可复制阶段、失败阶段、错误码、耗时、媒体包/关键帧数、codec 和前端已解码帧数，不包含 did、签名、私钥、UID、中继地址/token 或媒体 URL。`remotePlaybackVerified` 为本次尝试是否观察到解码帧；旧“检测视频授权”报告仍固定为 false。

## 验证范围

已加入模拟 UDP 会合/Relay/`0x28`/丢包重发测试、乱序/回绕/长度边界测试、NaCl 公开固定向量、授权材料绑定与账号归属测试，以及生成 H.264/H.265 → 实际 HLS → 独立解码测试。浏览器测试覆盖阶段显示、失败、迟到响应与关闭清理。

这些不能替代用户摄像头的真实跨网测试。`039a01` 固件 `5.1.6_0433` 的完整远程参数、服务端握手、真实 MISS/媒体、Windows Media Foundation 与 WebView2 运行、30 分钟稳定性尚未实测；不声明目标设备已出画，也不声明其他 CS2 型号兼容。首版无音频、对讲、云台、录像回放或多画面直连。
