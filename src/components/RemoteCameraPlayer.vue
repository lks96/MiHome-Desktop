<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import Hls from 'hls.js'
import { NAlert, NButton, NTag } from 'naive-ui'
import { call } from '../utils/ipc'
import type { AppError } from '../types/settings'
import { useDesktopStore } from '../stores/desktop'
import { version as clientVersion } from '../../package.json'
const props = defineProps<{ did: string; online: boolean }>()
const desktop = useDesktopStore()
interface AuthDiagnostics { messages: number; messageKind: string; commandByteOrder: string | null; bodyBytes: number; trailingPaddingBytes: number; encrypted: boolean; jsonShape: string; jsonErrorCategory: string | null }
interface TransportDiagnostics { linkRequests: number; linkNonRequests: number; commandMessages: number; mediaMessages: number; streamErrorChannel: number | null }
interface Status { sessionId: string; stage: string; failedStage: string | null; elapsedMs: number; attempt: number; mediaPackets: number; keyframes: number; codec: string | null; playbackUrl: string | null; error: AppError | null; authDiagnostics?: AuthDiagnostics; transportDiagnostics?: TransportDiagnostics | null }
const status = ref<Status | null>(null)
const video = ref<HTMLVideoElement | null>(null)
const running = ref(false)
const playing = ref(false)
const decodedFrames = ref(0)
const localError = ref('')
const clientErrorCode = ref<string | null>(null)
const copied = ref('')
const labels: Record<string, string> = { provisioning: '正在获取视频授权', rendezvous: '正在连接会合服务', relay_candidate: '正在选择远程节点', relay_request: '正在等待摄像头加入', relay_ready: '远程节点已就绪', session_linking: '正在连接摄像头会话', session_established: '远程会话已建立', miss_authenticating: '正在验证视频权限', miss_authenticated: '视频权限已确认', waiting_media: '正在等待视频', preparing_video: '正在处理视频', media_ready: '正在等待画面', failed: '连接失败', stopped: '已停止' }
const label = computed(() => playing.value ? '正在播放' : status.value ? labels[status.value.stage] ?? '正在连接' : '尚未播放')
const diagnostic = computed(() => {
  const a = status.value?.authDiagnostics
  const t = status.value?.transportDiagnostics
  return JSON.stringify({ diagnosticSchema: 3, clientVersion, model: 'chuangmi.camera.039a01', transport: 'cs2', stage: status.value?.stage ?? 'idle', failedStage: status.value?.failedStage ?? null, errorCode: status.value?.error?.code ?? clientErrorCode.value, elapsedMs: status.value?.elapsedMs ?? 0, mediaPackets: status.value?.mediaPackets ?? 0, keyframes: status.value?.keyframes ?? 0, codec: status.value?.codec ?? null, decodedFrames: decodedFrames.value, remotePlaybackVerified: decodedFrames.value > 0,
    authDiagnostics: a ? { messages: a.messages, messageKind: a.messageKind, commandByteOrder: a.commandByteOrder, bodyBytes: a.bodyBytes, trailingPaddingBytes: a.trailingPaddingBytes, encrypted: a.encrypted, jsonShape: a.jsonShape, jsonErrorCategory: a.jsonErrorCategory } : null,
    transportDiagnostics: t ? { linkRequests: t.linkRequests, linkNonRequests: t.linkNonRequests, commandMessages: t.commandMessages, mediaMessages: t.mediaMessages, streamErrorChannel: t.streamErrorChannel } : null,
  }, null, 2)
})
let generation = 0
let alive = true
let id: string | null = null
let hls: Hls | null = null
let attachedUrl: string | null = null
let timer: ReturnType<typeof setTimeout> | null = null
let watchdog: ReturnType<typeof setInterval> | null = null
let frame: number | null = null
let attempts = 0
let lastFrame = 0
const current = (token: number) => alive && token === generation
async function release() {
  if (timer) clearTimeout(timer)
  if (watchdog) clearInterval(watchdog)
  timer = null; watchdog = null
  if (frame !== null && video.value?.cancelVideoFrameCallback) video.value.cancelVideoFrameCallback(frame)
  frame = null; hls?.destroy(); hls = null; attachedUrl = null
  if (video.value) { video.value.pause(); video.value.removeAttribute('src'); video.value.load() }
  playing.value = false; running.value = false
  const owned = id; id = null
  if (owned) await call<boolean>('camera_remote_stop', { sessionId: owned })
}
function stop() { generation++; void release(); if (status.value) status.value = { ...status.value, stage: 'stopped', playbackUrl: null } }
async function failed(token: number, error: string, retryable = false, code = 'VIDEO_PLAYBACK_FAILED') {
  if (!current(token) || !running.value) return
  running.value = false
  await release()
  if (!current(token)) return
  localError.value = error
  clientErrorCode.value = code
  if (status.value?.stage !== 'failed' && status.value) status.value = { ...status.value, failedStage: status.value.stage, stage: 'failed', playbackUrl: null }
  if (retryable && attempts < 3) {
    const delay = [2000, 5000, 10000][attempts++]!
    localError.value += ` · ${delay / 1000} 秒后重试（${attempts}/3）`
    running.value = true
    timer = setTimeout(() => { if (current(token)) void start(false) }, delay)
  }
}
function observed(token: number) {
  if (!current(token) || !video.value || video.value.videoWidth === 0) return
  lastFrame = performance.now(); decodedFrames.value++; playing.value = true; localError.value = ''
  if (video.value.requestVideoFrameCallback) frame = video.value.requestVideoFrameCallback(() => observed(token))
}
async function attach(token: number, url: string) {
  attachedUrl = url
  await nextTick()
  if (!current(token) || !video.value) return
  const element = video.value
  if (Hls.isSupported()) {
    hls = new Hls({ enableWorker: false, lowLatencyMode: false, maxBufferLength: 8, backBufferLength: 4, manifestLoadingMaxRetry: 2, levelLoadingMaxRetry: 2, fragLoadingMaxRetry: 2 })
    hls.on(Hls.Events.ERROR, (_event, data) => { if (data.fatal) void failed(token, '无法播放视频，请重新连接或复制连接结果。') })
    hls.on(Hls.Events.MANIFEST_PARSED, () => { if (current(token)) void element.play().catch(() => failed(token, '无法开始播放，请重新连接。')) })
    hls.loadSource(url); hls.attachMedia(element)
  } else if (element.canPlayType('application/vnd.apple.mpegurl')) { element.src = url; void element.play().catch(() => failed(token, '此系统无法播放视频。')) }
  else { void failed(token, '当前系统不支持视频播放，请更新 WebView2。'); return }
  lastFrame = performance.now()
  if (element.requestVideoFrameCallback) frame = element.requestVideoFrameCallback(() => observed(token))
  let previousTime = -1
  watchdog = setInterval(() => {
    if (!current(token)) return
    if (!element.requestVideoFrameCallback && element.readyState >= 2 && element.videoWidth > 0 && previousTime !== element.currentTime) { previousTime = element.currentTime; observed(token) }
    if (performance.now() - lastFrame > 20000) void failed(token, '20 秒内未显示新画面，请检查网络后重试。', true)
  }, 1000)
}
async function poll(token: number, sessionId: string) {
  const result = await call<Status>('camera_remote_status', { sessionId })
  if (!current(token)) return
  if (!result.ok) { await failed(token, result.error.message, result.error.retryable, result.error.code); return }
  status.value = result.data
  if (result.data.stage === 'failed') { await failed(token, result.data.error?.message ?? '远程连接失败', result.data.error?.retryable); return }
  if (result.data.stage === 'stopped') { await release(); return }
  if (result.data.playbackUrl && result.data.playbackUrl !== attachedUrl) await attach(token, result.data.playbackUrl)
  if (current(token) && id === sessionId) timer = setTimeout(() => { void poll(token, sessionId) }, 1000)
}
async function start(reset = true) {
  const token = ++generation
  await release()
  if (!current(token) || !props.online || !desktop.visible || document.hidden) return
  if (reset) attempts = 0
  decodedFrames.value = 0
  copied.value = ''; localError.value = ''; clientErrorCode.value = null; status.value = null; running.value = true
  const result = await call<Status>('camera_remote_start', { did: props.did })
  if (!current(token)) { if (result.ok) await call<boolean>('camera_remote_stop', { sessionId: result.data.sessionId }); return }
  if (!result.ok) { running.value = false; localError.value = result.error.message; clientErrorCode.value = result.error.code; return }
  id = result.data.sessionId; status.value = result.data
  void poll(token, id)
}
async function copy() { try { await navigator.clipboard.writeText(diagnostic.value); copied.value = '已复制连接结果' } catch { copied.value = '请手动复制下方结果' } }
const visibility = () => { if (document.hidden) stop() }
document.addEventListener('visibilitychange', visibility)
watch(() => [props.did, props.online], stop)
watch(() => desktop.visible, visible => { if (!visible) stop() })
onBeforeUnmount(() => { alive = false; generation++; document.removeEventListener('visibilitychange', visibility); void release() })
</script>
<template>
  <section class="remote-camera">
    <div class="remote-title"><h3>远程观看</h3><NTag :type="playing ? 'success' : 'default'" size="small">{{ running && !status ? '正在连接' : label }}</NTag></div>
    <p>直接连接摄像头。当前为此型号的远程播放测试版，首版仅视频。</p>
    <div v-if="status?.playbackUrl" class="remote-stage"><video ref="video" autoplay muted playsinline aria-label="米家远程摄像头画面"></video><p v-if="!playing">等待画面解码…</p></div>
    <NAlert v-if="localError" type="warning" class="remote-notice">{{ localError }}</NAlert>
    <div class="remote-actions"><NButton type="primary" :disabled="!online || running || !desktop.visible" @click="start()">远程播放</NButton><NButton :disabled="!running" @click="stop">停止播放</NButton><NButton v-if="status || localError" @click="copy">复制连接结果</NButton></div>
    <small v-if="copied" role="status">{{ copied }}</small>
    <details v-if="status"><summary>连接结果</summary><pre data-remote-camera-result>{{ diagnostic }}</pre></details>
  </section>
</template>
<style scoped>
.remote-camera{margin:20px 0;padding:16px;border:1px solid #8792a533;border-radius:12px}.remote-title{display:flex;align-items:center;justify-content:space-between}.remote-title h3{margin:0;font-size:15px}.remote-camera>p,.remote-camera small{font-size:12px;color:#8792a5;line-height:1.8}.remote-actions{display:flex;gap:10px;flex-wrap:wrap;margin-top:14px}.remote-stage{position:relative;aspect-ratio:16/9;background:#10151e;border-radius:8px;overflow:hidden}.remote-stage video{width:100%;height:100%;object-fit:contain}.remote-stage>p{position:absolute;bottom:8px;left:12px;color:white;font-size:12px}.remote-notice{margin:12px 0}.remote-camera details{margin-top:14px;font-size:12px;color:#8792a5}.remote-camera pre{white-space:pre-wrap;overflow-wrap:anywhere;user-select:text;font-size:11px}.remote-camera small{display:block;margin-top:8px}
</style>
