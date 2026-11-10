<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { NButton, NTag } from 'naive-ui'
import { call } from '../utils/ipc'
import type { CameraSession, CameraView } from '../types/cameras'
import { useCamerasStore } from '../stores/cameras'
import { useConnectionStore } from '../stores/connection'
const props = defineProps<{ camera: CameraView }>()
const cameras = useCamerasStore()
const connection = useConnectionStore()
const tile = ref<HTMLElement | null>(null)
const video = ref<HTMLVideoElement | null>(null)
const state = ref('尚未播放')
const message = ref('')
const playing = ref(false)
const screenshot = ref<string | null>(null)
const muted = ref(true)
const hasAudio = ref(false)
const busySnapshot = ref(false)
const live = computed(() => props.camera.profile.sourceType === 'go2rtc')
let mounted = false
let generation = 0
let sessionId: string | null = null
let pc: RTCPeerConnection | null = null
let media: MediaStream | null = null
let deadline: ReturnType<typeof setTimeout> | null = null
let retry: ReturnType<typeof setTimeout> | null = null
let watchdog: ReturnType<typeof setInterval> | null = null
let heartbeat: ReturnType<typeof setInterval> | null = null
let frameId: number | null = null
let attempts = 0
let lastFrame = 0
let stableSince = 0
let lastTime = -1
let queue = Promise.resolve()
const current = (token: number) => mounted && token === generation
function release(): Promise<void> {
  if (deadline) clearTimeout(deadline)
  if (retry) clearTimeout(retry)
  if (watchdog) clearInterval(watchdog)
  if (heartbeat) clearInterval(heartbeat)
  deadline = retry = null; watchdog = heartbeat = null
  if (frameId !== null && video.value?.cancelVideoFrameCallback) video.value.cancelVideoFrameCallback(frameId)
  frameId = null
  if (pc) { pc.ontrack = null; pc.onconnectionstatechange = null; pc.close(); pc = null }
  media?.getTracks().forEach(track => track.stop()); media = null
  if (video.value) { video.value.pause(); video.value.srcObject = null }
  hasAudio.value = false; playing.value = false; busySnapshot.value = false
  const id = sessionId; sessionId = null
  return id ? call<boolean>('camera_stop_session', { sessionId: id }).then(() => undefined) : Promise.resolve()
}
function fail(token: number, reason: string, automatic = true) {
  if (!current(token)) return
  generation++
  const after = generation
  void release()
  message.value = reason
  if (automatic && attempts < 5) {
    const wait = [1000, 2000, 5000, 10000, 30000][attempts++]!
    state.value = `视频重连中（${attempts}/5）`
    retry = setTimeout(() => { if (current(after)) restart(false) }, wait)
  } else state.value = '视频连接失败'
}
function onFrame(token: number) {
  if (!current(token) || !video.value) return
  // A connected peer alone is insufficient: only decoded video frames establish playback.
  lastFrame = performance.now()
  if (!stableSince) stableSince = lastFrame
  if (lastFrame - stableSince > 60000) attempts = 0
  playing.value = true; state.value = '正在播放'; message.value = ''
  if (deadline) { clearTimeout(deadline); deadline = null }
  if (video.value.requestVideoFrameCallback) frameId = video.value.requestVideoFrameCallback(() => onFrame(token))
}
async function gather(peer: RTCPeerConnection) {
  if (peer.iceGatheringState === 'complete') return
  await new Promise<void>((resolve, reject) => {
    const cleanup = () => { clearTimeout(timer); peer.removeEventListener('icegatheringstatechange', changed) }
    const changed = () => { if (peer.iceGatheringState === 'complete') { cleanup(); resolve() } }
    const timer = setTimeout(() => { cleanup(); reject(new Error('ICE_TIMEOUT')) }, 8000)
    peer.addEventListener('icegatheringstatechange', changed)
    changed()
  })
}
async function connect(token: number, previousRelease: Promise<void>) {
  try {
    await previousRelease
    if (!current(token)) return
    screenshot.value = null; message.value = ''; muted.value = true; lastFrame = stableSince = 0; lastTime = -1
    if (!props.camera.profile.enabled) { state.value = '摄像头已停用'; return }
    if (!live.value) { state.value = 'HA 截图模式'; message.value = '点击获取截图；截图不是实时视频。'; return }
    if (!cameras.gateway.enabled) { state.value = '暂不支持播放'; message.value = '请在设置中启用 go2rtc。'; return }
    if (typeof RTCPeerConnection === 'undefined') { state.value = '暂不支持播放'; message.value = '此运行环境不支持 WebRTC。'; return }
    state.value = '正在连接'
    const created = await call<CameraSession>('camera_start_session', { entityId: props.camera.entityId })
    if (!created.ok) { fail(token, `${created.error.code}：${created.error.message}`, created.error.retryable); return }
    sessionId = created.data.sessionId
    if (!current(token)) { await release(); return }
    const peer = new RTCPeerConnection({ iceServers: [] })
    pc = peer
    media = new MediaStream()
    if (video.value) { video.value.srcObject = media; video.value.muted = true }
    const track = peer.addTransceiver('video', { direction: 'recvonly' })
    peer.addTransceiver('audio', { direction: 'recvonly' })
    const codecs = RTCRtpReceiver.getCapabilities?.('video')?.codecs
    if (codecs?.length && track.setCodecPreferences) {
      track.setCodecPreferences([...codecs.filter(codec => codec.mimeType.toLowerCase() === 'video/h264'), ...codecs.filter(codec => codec.mimeType.toLowerCase() !== 'video/h264')])
    }
    peer.ontrack = event => {
      if (!current(token) || !media) return
      if (!media.getTracks().some(track => track.id === event.track.id)) media.addTrack(event.track)
      if (event.track.kind === 'audio') hasAudio.value = true
      void video.value?.play().catch(() => fail(token, 'VIDEO_DECODE_FAILED：无法开始播放，请检查编码或手动重试。'))
    }
    peer.onconnectionstatechange = () => {
      if (peer.connectionState === 'failed' || peer.connectionState === 'closed') fail(token, 'WebRTC 连接中断。')
    }
    deadline = setTimeout(() => fail(token, 'CAMERA_TIMEOUT：30 秒内未收到可解码的视频帧，请检查流、编解码与媒体端口。'), 30000)
    if (video.value?.requestVideoFrameCallback) frameId = video.value.requestVideoFrameCallback(() => onFrame(token))
    watchdog = setInterval(() => {
      if (!current(token) || document.hidden) return
      const element = video.value
      if (element && !element.requestVideoFrameCallback && element.readyState >= 2 && element.videoWidth > 0 && element.currentTime !== lastTime) { lastTime = element.currentTime; onFrame(token) }
      if (lastFrame && performance.now() - lastFrame > 10000) fail(token, '视频数据已中断，正在尝试恢复。')
    }, 1000)
    let keeping = false
    const ownedSession = sessionId
    heartbeat = setInterval(async () => {
      if (!current(token) || keeping) return
      keeping = true
      const result = await call<boolean>('camera_keep_session', { sessionId: ownedSession })
      keeping = false
      if (!result.ok) fail(token, result.error.message)
    }, 15000)
    await peer.setLocalDescription(await peer.createOffer())
    await gather(peer)
    if (!current(token)) { await release(); return }
    const offer = peer.localDescription?.sdp
    if (!offer) throw new Error('NO_SDP')
    const answer = await call<string>('camera_exchange_sdp', { sessionId, offer })
    if (!current(token)) { await release(); return }
    if (!answer.ok) { fail(token, `${answer.error.code}：${answer.error.message}`, answer.error.retryable); return }
    await peer.setRemoteDescription({ type: 'answer', sdp: answer.data })
  } catch {
    if (current(token)) fail(token, 'VIDEO_DECODE_FAILED：视频协商或解码失败，请检查媒体端口、编码与 go2rtc 配置。')
    else await release()
  }
}
function restart(reset = true) {
  if (reset) attempts = 0
  const token = ++generation
  const old = release()
  queue = queue.then(() => connect(token, old)).catch(() => { if (current(token)) fail(token, '视频会话初始化失败。', false) })
}
function stop() { generation++; void release(); state.value = '已停止'; message.value = ''; screenshot.value = null }
async function snapshot() {
  const token = ++generation
  await release()
  if (!current(token)) return
  busySnapshot.value = true; screenshot.value = null; state.value = '正在获取截图'; message.value = ''
  const result = await call<{ dataUrl: string }>('camera_get_snapshot', { entityId: props.camera.entityId })
  if (!current(token)) return
  busySnapshot.value = false
  if (result.ok) { screenshot.value = result.data.dataUrl; state.value = '截图（非实时）' }
  else { state.value = '截图获取失败'; message.value = `${result.error.code}：${result.error.message}` }
}
async function fullscreen() { try { if (tile.value?.requestFullscreen) await tile.value.requestFullscreen() } catch { message.value = '无法进入全屏，请检查窗口权限。' } }
function toggleMute() { muted.value = !muted.value; if (video.value) video.value.muted = muted.value }
watch(() => [props.camera.entityId, props.camera.profile.sourceType, props.camera.profile.streamName, props.camera.profile.enabled, cameras.gateway.enabled, cameras.gateway.baseUrl], () => { if (mounted) restart() })
onMounted(() => { mounted = true; restart() })
onBeforeUnmount(() => { mounted = false; generation++; screenshot.value = null; void release() })
</script>
<template>
  <article ref="tile" class="camera-tile" :data-camera-id="camera.entityId">
    <div class="camera-tile-heading"><strong>{{ camera.name }}</strong><NTag :type="playing ? 'success' : 'default'" size="small">{{ state }}</NTag></div>
    <div class="camera-stage">
      <video ref="video" autoplay playsinline muted :class="{ hidden: !live }" @error="fail(generation, 'VIDEO_DECODE_FAILED：视频无法解码。')"></video>
      <img v-if="screenshot" :src="screenshot" alt="摄像头截图（不是实时视频）" @error="screenshot = null; state = '截图无法解码'" />
      <div v-if="!playing && !screenshot" class="camera-placeholder"><span>◉</span><p>{{ state }}</p><small>{{ message || '摄像头兼容性待实机验证' }}</small></div>
    </div>
    <p v-if="playing && message" class="camera-note">{{ message }}</p>
    <div class="camera-tile-actions">
      <template v-if="live"><NButton size="small" :disabled="!camera.profile.enabled" @click="restart()">重试播放</NButton><NButton size="small" @click="stop">停止</NButton><NButton size="small" :disabled="!hasAudio" @click="toggleMute">{{ muted ? '取消静音' : '静音' }}</NButton></template>
      <NButton v-else size="small" :loading="busySnapshot" :disabled="!camera.profile.enabled || connection.status.state !== 'connected'" @click="snapshot">获取 HA 截图</NButton>
      <NButton size="small" @click="fullscreen">全屏</NButton>
    </div>
    <p class="camera-note">{{ camera.entityId }} · HA：{{ camera.haState ?? '状态未知' }} · {{ camera.haStreamDeclared ? 'HA 声明流能力（未验证）' : 'HA 流能力未知' }}</p>
  </article>
</template>
<style scoped>
.camera-tile{padding:16px;background:var(--n-color,#fff);border:1px solid #8883;border-radius:12px;min-width:0}.camera-tile:fullscreen{background:#10151e;display:flex;flex-direction:column}.camera-tile-heading{display:flex;gap:12px;justify-content:space-between;align-items:center;margin-bottom:12px}.camera-tile-heading strong{overflow-wrap:anywhere}.camera-stage{aspect-ratio:16/9;position:relative;background:#121a29;border-radius:8px;overflow:hidden;color:#c8d3e7}.camera-tile:fullscreen .camera-stage{flex:1;aspect-ratio:auto}.camera-stage video,.camera-stage img{width:100%;height:100%;object-fit:contain;position:absolute;inset:0}.camera-stage .hidden{opacity:0}.camera-placeholder{position:absolute;inset:0;display:flex;align-items:center;justify-content:center;flex-direction:column;padding:20px;text-align:center}.camera-placeholder>span{font-size:35px;color:#607294}.camera-placeholder p{font-size:14px}.camera-placeholder small{font-size:11px;line-height:1.8;color:#98abc9}.camera-tile-actions{display:flex;gap:8px;flex-wrap:wrap;margin-top:12px}.camera-note{font-size:10px;color:#8792a5;overflow-wrap:anywhere;line-height:1.7;margin-bottom:0}
</style>
