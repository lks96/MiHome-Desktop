<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import Hls from 'hls.js'
import { NAlert, NTag } from 'naive-ui'
import { BrowsersOutline, ContractOutline, CopyOutline, EllipsisHorizontalOutline, ExpandOutline, CloseOutline, MicOffOutline, MicOutline, PauseOutline, PlayOutline, StopOutline, VolumeHighOutline, VolumeMuteOutline } from '@vicons/ionicons5'
import { call } from '../utils/ipc'
import { CameraAudio, type CameraAudioPacket } from '../utils/camera-audio'
import microphoneWorklet from '../utils/camera-mic.worklet.js?url&no-inline'
import type { AppError } from '../types/settings'
import { useDesktopStore } from '../stores/desktop'
import { version as clientVersion } from '../../package.json'
const props = withDefaults(defineProps<{ did: string; online: boolean; name?: string; standalone?: boolean; autoStart?: boolean }>(), { name:'',standalone:false,autoStart:false })
const desktop = useDesktopStore()
interface AuthDiagnostics { messages: number; messageKind: string; commandByteOrder: string | null; bodyBytes: number; trailingPaddingBytes: number; encrypted: boolean; jsonShape: string; jsonErrorCategory: string | null }
interface TransportDiagnostics { linkRequests: number; linkNonRequests: number; commandMessages: number; mediaMessages: number; streamErrorChannel: number | null }
interface TimingDiagnostics { timestampSamples:number;timestampFallbacks:number;sourceFps:number|null;receiveFps:number|null;largestArrivalGapMs:number;encoderWriteMaxMs:number }
interface Status { sessionId: string; stage: string; failedStage: string | null; elapsedMs: number; attempt: number; mediaPackets: number; keyframes: number; codec: string | null; playbackUrl: string | null; error: AppError | null; authDiagnostics?: AuthDiagnostics; transportDiagnostics?: TransportDiagnostics | null; timingDiagnostics?:TimingDiagnostics|null;requestedQuality?:number }
const status = ref<Status | null>(null)
const video = ref<HTMLVideoElement | null>(null)
const running = ref(false)
const playing = ref(false)
const paused = ref(false)
const controlsVisible = ref(true)
const diagnosticsVisible = ref(false)
let controlsTimer: ReturnType<typeof setTimeout> | undefined

function showControls() {
  controlsVisible.value = true
  clearTimeout(controlsTimer)
  if (props.standalone && playing.value && !paused.value) {
    controlsTimer = setTimeout(() => { controlsVisible.value = false }, 2500)
  }
}
const decodedFrames = ref(0)
const localError = ref('')
const clientErrorCode = ref<string | null>(null)
const controlError = ref('')
const listening = ref(false)
const talking = ref(false)
const audioBusy = ref(false)
const talkBusy = ref(false)
const ptzBusy = ref(false)
const ptzErrorCode = ref<string | null>(null)
const ptzAcknowledged = ref(false)
const cameraControlsReady = computed(() => running.value && status.value?.stage === 'media_ready')
const keyboardDirections: Record<string, string> = {
  ArrowUp: 'up',
  ArrowLeft: 'left',
  ArrowDown: 'down',
  ArrowRight: 'right',
}
let gesture: { id: number; x: number; y: number; moved: boolean } | null = null
let cameraAudio: CameraAudio | null = null
let audioTimer: ReturnType<typeof setTimeout> | undefined
let micStream: MediaStream | null = null
let micContext: AudioContext | null = null
let micNode: AudioWorkletNode | null = null
const copied = ref('')
const stageElement=ref<HTMLElement|null>(null)
const mini=ref(false)
const nativeFullscreen=ref(false)
const webFullscreen = ref(false)
const isFullscreen = computed(() => nativeFullscreen.value || webFullscreen.value)
const quality=ref(2)
const qualityOptions=[{label:'自动',value:0},{label:'流畅',value:1},{label:'高清',value:2},{label:'超清',value:3}]
const playback=ref({width:0,height:0,decodedFps:null as number|null,droppedFrames:0,waitingEvents:0,bufferedAheadSeconds:0,hlsLatencySeconds:null as number|null})
let recentFrames:number[]=[]
const labels: Record<string, string> = { provisioning: '正在获取视频授权', rendezvous: '正在连接会合服务', relay_candidate: '正在选择远程节点', relay_request: '正在等待摄像头加入', relay_ready: '远程节点已就绪', session_linking: '正在连接摄像头会话', session_established: '远程会话已建立', miss_authenticating: '正在验证视频权限', miss_authenticated: '视频权限已确认', waiting_media: '正在等待视频', preparing_video: '正在处理视频', media_ready: '正在等待画面', failed: '连接失败', stopped: '已停止' }
const pictureRatio = computed(() => playback.value.height ? playback.value.width / playback.value.height : 16 / 9)
const playbackAction = computed(() => paused.value ? '继续播放' : playing.value ? '暂停播放' : '远程播放')
const playbackAvailable = computed(() => playing.value || (!running.value && props.online && (props.standalone || desktop.visible)))
const label = computed(() => paused.value ? '已暂停' : playing.value ? '正在播放' : status.value ? labels[status.value.stage] ?? '正在连接' : '尚未播放')
const diagnostic = computed(() => {
  const a = status.value?.authDiagnostics
  const t = status.value?.transportDiagnostics
  const timing=status.value?.timingDiagnostics
  return JSON.stringify({ diagnosticSchema: 5, clientVersion, model: 'chuangmi.camera.039a01', transport: 'cs2', stage: status.value?.stage ?? 'idle', failedStage: status.value?.failedStage ?? null, errorCode: status.value?.error?.code ?? clientErrorCode.value, elapsedMs: status.value?.elapsedMs ?? 0, mediaPackets: status.value?.mediaPackets ?? 0, keyframes: status.value?.keyframes ?? 0, codec: status.value?.codec ?? null, decodedFrames: decodedFrames.value, remotePlaybackVerified: decodedFrames.value > 0,requestedQuality:status.value?.requestedQuality??quality.value,
    ptzDiagnostics: { acknowledged: ptzAcknowledged.value, errorCode: ptzErrorCode.value },
    authDiagnostics: a ? { messages: a.messages, messageKind: a.messageKind, commandByteOrder: a.commandByteOrder, bodyBytes: a.bodyBytes, trailingPaddingBytes: a.trailingPaddingBytes, encrypted: a.encrypted, jsonShape: a.jsonShape, jsonErrorCategory: a.jsonErrorCategory } : null,
    transportDiagnostics: t ? { linkRequests: t.linkRequests, linkNonRequests: t.linkNonRequests, commandMessages: t.commandMessages, mediaMessages: t.mediaMessages, streamErrorChannel: t.streamErrorChannel } : null,
    timingDiagnostics:timing?{timestampSamples:timing.timestampSamples,timestampFallbacks:timing.timestampFallbacks,sourceFps:timing.sourceFps,receiveFps:timing.receiveFps,largestArrivalGapMs:timing.largestArrivalGapMs,encoderWriteMaxMs:timing.encoderWriteMaxMs}:null,
    playbackDiagnostics:{width:playback.value.width,height:playback.value.height,decodedFps:playback.value.decodedFps,droppedFrames:playback.value.droppedFrames,waitingEvents:playback.value.waitingEvents,bufferedAheadSeconds:playback.value.bufferedAheadSeconds,hlsLatencySeconds:playback.value.hlsLatencySeconds},
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
function closeAudio() {
  clearTimeout(audioTimer)
  cameraAudio?.close()
  cameraAudio = null
  listening.value = false
}

function closeMicrophone() {
  micStream?.getTracks().forEach(track => track.stop())
  micStream = null
  micNode?.disconnect()
  if (micNode) micNode.port.onmessage = null
  micNode = null
  if (micContext) void micContext.close().catch(() => {})
  micContext = null
  talking.value = false
}

async function pollAudio(sessionId: string, player: CameraAudio) {
  const result = await call<CameraAudioPacket[]>('camera_remote_audio', { sessionId })
  if (id !== sessionId || player !== cameraAudio) return
  if (!result.ok) {
    closeAudio()
    controlError.value = result.error.message
    return
  }
  try {
    if (!paused.value) for (const packet of result.data) player.play(packet)
  } catch {
    closeAudio()
    controlError.value = '当前 WebView2 无法解码摄像头声音'
    void call('camera_remote_control', { sessionId, action: 'listen_off' })
    return
  }
  audioTimer = setTimeout(() => void pollAudio(sessionId, player), 80)
}

async function toggleAudio() {
  const sessionId = id
  if (!sessionId || audioBusy.value) return
  audioBusy.value = true
  controlError.value = ''
  const enable = !listening.value
  try {
    if (enable) {
      const context = new AudioContext()
      cameraAudio = new CameraAudio(context, () => {
        if (id !== sessionId || cameraAudio !== player) return
        closeAudio()
        controlError.value = '无法解码摄像头声音'
        void call('camera_remote_control', { sessionId, action: 'listen_off' })
      })
      const player = cameraAudio
      player.setMuted(paused.value)
      await context.resume()
      if (id !== sessionId || player !== cameraAudio) return
    } else {
      closeAudio()
    }
    const result = await call<boolean>('camera_remote_control', {
      sessionId, action: enable ? 'listen_on' : 'listen_off',
    })
    if (id !== sessionId) return
    if (!result.ok) {
      closeAudio()
      controlError.value = result.error.message
      return
    }
    listening.value = enable
    if (cameraAudio) void pollAudio(sessionId, cameraAudio)
  } catch {
    if (id !== sessionId) return
    closeAudio()
    controlError.value = '无法打开现场声音，请检查系统音频设置'
  } finally {
    if (id === sessionId) audioBusy.value = false
  }
}

async function toggleTalk() {
  const sessionId = id
  if (!sessionId || talkBusy.value) return
  talkBusy.value = true
  controlError.value = ''
  const enable = !talking.value
  try {
    if (enable) {
      const context = new AudioContext()
      micContext = context
      await context.resume()
      if (id !== sessionId) return
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation: true, noiseSuppression: true, channelCount: 1 },
      })
      if (id !== sessionId) {
        stream.getTracks().forEach(track => track.stop())
        return
      }
      micStream = stream
      await context.audioWorklet.addModule(microphoneWorklet)
      if (id !== sessionId) return
      const node = new AudioWorkletNode(context, 'camera-microphone')
      micNode = node
      context.createMediaStreamSource(stream).connect(node).connect(context.destination)
      let sending = false
      node.port.onmessage = async ({ data }: MessageEvent<Uint8Array>) => {
        if (!talking.value || id !== sessionId || node !== micNode || sending) return
        sending = true
        try {
          const result = await call<boolean>('camera_remote_speaker', { sessionId, samples: Array.from(data) })
          if (!result.ok && id === sessionId && talking.value) {
            await toggleTalk()
            if (id === sessionId) controlError.value = result.error.message
          }
        } finally {
          sending = false
        }
      }
      for (const track of stream.getTracks()) track.addEventListener('ended', () => {
        if (id === sessionId && talking.value) void toggleTalk()
      })
    } else {
      closeMicrophone()
    }
    const result = await call<boolean>('camera_remote_control', {
      sessionId, action: enable ? 'talk_on' : 'talk_off',
    })
    if (id !== sessionId) return
    if (!result.ok) {
      closeMicrophone()
      controlError.value = result.error.message
      return
    }
    talking.value = enable
  } catch {
    if (id !== sessionId) return
    closeMicrophone()
    controlError.value = '无法打开麦克风，请检查设备和 Windows 麦克风权限'
  } finally {
    if (id === sessionId) talkBusy.value = false
  }
}

async function moveCamera(direction: string) {
  const sessionId = id
  if (!sessionId || !cameraControlsReady.value || ptzBusy.value) return
  ptzBusy.value = true
  controlError.value = ''
  ptzErrorCode.value = null
  ptzAcknowledged.value = false
  const result = await call<boolean>('camera_remote_ptz', { sessionId, direction })
  if (id !== sessionId) return
  ptzBusy.value = false
  if (result.ok) {
    ptzAcknowledged.value = result.data
  } else {
    ptzErrorCode.value = result.error.code
    controlError.value = result.error.message
  }
}

async function togglePlayback() {
  if (!playbackAvailable.value) return
  if (!running.value) {
    await start()
    return
  }
  const element = video.value
  if (!element) return
  if (!paused.value) {
    paused.value = true
    element.pause()
    cameraAudio?.setMuted(true)
    showControls()
    return
  }
  const token = generation
  const livePosition = hls?.liveSyncPosition
  if (livePosition != null && element.seekable.length && livePosition >= element.seekable.start(0)) {
    element.currentTime = Math.min(livePosition, element.seekable.end(element.seekable.length - 1))
  }
  lastFrame = performance.now()
  recentFrames = []
  paused.value = false
  cameraAudio?.setMuted(false)
  showControls()
  try {
    await element.play()
  } catch {
    if (!paused.value && current(token)) await failed(token, '无法继续播放，请重新连接。')
  }
}

function beginGesture(event: PointerEvent) {
  if (!event.isPrimary || event.button !== 0) return
  gesture = { id: event.pointerId, x: event.clientX, y: event.clientY, moved: false }
  const surface = event.currentTarget as HTMLElement
  surface.setPointerCapture(event.pointerId)
}

function updateGesture(event: PointerEvent) {
  if (gesture?.id !== event.pointerId) return
  const distance = Math.max(Math.abs(event.clientX - gesture.x), Math.abs(event.clientY - gesture.y))
  if (distance >= 28) gesture.moved = true
}

function endGesture(event: PointerEvent) {
  if (gesture?.id !== event.pointerId) return
  updateGesture(event)
  const { x, y, moved } = gesture
  gesture = null
  const dx = event.clientX - x
  const dy = event.clientY - y
  if (Math.max(Math.abs(dx), Math.abs(dy)) >= 28) {
    const direction = Math.abs(dx) >= Math.abs(dy)
      ? (dx < 0 ? 'left' : 'right')
      : (dy < 0 ? 'up' : 'down')
    void moveCamera(direction)
  } else if (!moved) {
    void togglePlayback()
  }
}

function cancelGesture(event: PointerEvent) {
  if (gesture?.id === event.pointerId) gesture = null
}

function cameraKey(event: KeyboardEvent) {
  const direction = keyboardDirections[event.key]
  if (!direction) return
  event.preventDefault()
  if (!event.repeat) void moveCamera(direction)
}

async function release() {
  closeAudio()
  closeMicrophone()
  audioBusy.value = false
  talkBusy.value = false
  ptzBusy.value = false
  ptzErrorCode.value = null
  ptzAcknowledged.value = false
  controlError.value = ''
  paused.value = false
  diagnosticsVisible.value = false
  clearTimeout(controlsTimer)
  controlsVisible.value = true
  gesture = null

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
  const firstFrame = !playing.value
  lastFrame = performance.now(); decodedFrames.value++; playing.value = true; localError.value = ''
  if (firstFrame) showControls()
  recentFrames.push(lastFrame)
  while(recentFrames.length>2&&recentFrames[0]!<lastFrame-5000)recentFrames.shift()
  if (video.value.requestVideoFrameCallback) frame = video.value.requestVideoFrameCallback(() => observed(token))
}
async function attach(token: number, url: string) {
  attachedUrl = url
  await nextTick()
  if (!current(token) || !video.value) return
  const element = video.value
  if (Hls.isSupported()) {
    hls = new Hls({ enableWorker: false, lowLatencyMode: false, liveSyncDurationCount:2,liveMaxLatencyDurationCount:4,maxLiveSyncPlaybackRate:1,maxBufferLength:6,backBufferLength:3, manifestLoadingMaxRetry: 2, levelLoadingMaxRetry: 2, fragLoadingMaxRetry: 2 })
    hls.on(Hls.Events.ERROR, (_event, data) => { if (data.fatal) void failed(token, '无法播放视频，请重新连接或复制连接结果。') })
    hls.on(Hls.Events.MANIFEST_PARSED, () => { if (current(token)) void element.play().catch(() => failed(token, '无法开始播放，请重新连接。')) })
    hls.loadSource(url); hls.attachMedia(element)
  } else if (element.canPlayType('application/vnd.apple.mpegurl')) { element.src = url; void element.play().catch(() => failed(token, '此系统无法播放视频。')) }
  else { void failed(token, '当前系统不支持视频播放，请更新 WebView2。'); return }
  lastFrame = performance.now()
  if (element.requestVideoFrameCallback) frame = element.requestVideoFrameCallback(() => observed(token))
  let previousTime = -1
  watchdog = setInterval(() => {
    if (!current(token) || paused.value) return
    if (!element.requestVideoFrameCallback && element.readyState >= 2 && element.videoWidth > 0 && previousTime !== element.currentTime) { previousTime = element.currentTime; observed(token) }
    let buffered=0
    for(let i=0;i<element.buffered.length;i++)if(element.buffered.start(i)<=element.currentTime&&element.buffered.end(i)>=element.currentTime)buffered=element.buffered.end(i)-element.currentTime
    const span=recentFrames.length>1?recentFrames[recentFrames.length-1]!-recentFrames[0]!:0
    const latency=hls?.latency
    playback.value={...playback.value,width:element.videoWidth,height:element.videoHeight,decodedFps:span>0?Math.round((recentFrames.length-1)*10000/span)/10:null,droppedFrames:element.getVideoPlaybackQuality?.().droppedVideoFrames??0,bufferedAheadSeconds:Math.round(buffered*100)/100,hlsLatencySeconds:latency!==undefined&&Number.isFinite(latency)&&latency>=0?Math.round(latency*100)/100:null}
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
  if (!current(token) || !props.online || (!props.standalone&&!desktop.visible) || document.hidden) return
  if (reset) attempts = 0
  decodedFrames.value = 0
  recentFrames=[];playback.value={width:0,height:0,decodedFps:null,droppedFrames:0,waitingEvents:0,bufferedAheadSeconds:0,hlsLatencySeconds:null}
  copied.value = ''; localError.value = ''; clientErrorCode.value = null; status.value = null; running.value = true
  const result = await call<Status>('camera_remote_start', { did: props.did,quality:quality.value })
  if (!current(token)) { if (result.ok) await call<boolean>('camera_remote_stop', { sessionId: result.data.sessionId }); return }
  if (!result.ok) { running.value = false; localError.value = result.error.message; clientErrorCode.value = result.error.code; return }
  id = result.data.sessionId; status.value = result.data
  void poll(token, id)
}
async function copy() { try { await navigator.clipboard.writeText(diagnostic.value); copied.value = '已复制连接结果' } catch { copied.value = '请手动复制下方结果' } }
function changeQuality(value:number) {quality.value=value;if(running.value)void start()}
function selectQuality(event:Event){changeQuality(Number((event.target as HTMLSelectElement).value))}
async function fullscreen(){try{if(props.standalone){const result=await call<boolean>('camera_player_fullscreen',{enabled:!nativeFullscreen.value});if(result.ok)nativeFullscreen.value=result.data;else localError.value=result.error.message}else if(document.fullscreenElement)await document.exitFullscreen();else await stageElement.value?.requestFullscreen()}catch{localError.value='无法进入全屏，请重试'}}
async function toggleMini(){if(document.fullscreenElement)await document.exitFullscreen();if(nativeFullscreen.value)await fullscreen();const result=await call<boolean>('camera_player_mode',{mini:!mini.value});if(result.ok)mini.value=result.data;else localError.value=result.error.message}
function escapeFullscreen(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  if (diagnosticsVisible.value) diagnosticsVisible.value = false
  else if (nativeFullscreen.value) void fullscreen()
}
function waiting(){if(!paused.value && decodedFrames.value>0)playback.value.waitingEvents++}
const visibility = () => { if (document.hidden) stop() }
const updateWebFullscreen = () => { webFullscreen.value = document.fullscreenElement === stageElement.value }
document.addEventListener('fullscreenchange', updateWebFullscreen)
document.addEventListener('visibilitychange', visibility)
document.addEventListener('keydown',escapeFullscreen)
watch(() => [props.did, props.online], stop)
watch(() => desktop.visible, visible => { if (!props.standalone&&!visible) stop() })
onMounted(()=>{if(props.autoStart)void start()})
onBeforeUnmount(() => { alive = false; generation++; document.removeEventListener('visibilitychange', visibility); document.removeEventListener('fullscreenchange', updateWebFullscreen);document.removeEventListener('keydown',escapeFullscreen); void release() })
</script>
<template>
  <section class="remote-camera" :class="{ standalone, mini }" :aria-label="standalone ? name : undefined">
    <div v-if="!standalone" class="remote-title">
      <h3>{{ name || '远程观看' }}</h3>
      <NTag :type="playing && !paused ? 'success' : 'default'" size="small">{{ running && !status ? '正在连接' : label }}</NTag>
    </div>

    <div ref="stageElement" class="remote-stage"
      :class="{ 'controls-hidden': !controlsVisible && !diagnosticsVisible && !controlError && !localError }"
      @pointermove="showControls" @pointerdown="showControls">
      <div class="camera-picture" :style="{ '--camera-ratio': pictureRatio }">
        <video v-if="status?.playbackUrl" ref="video" autoplay muted playsinline
          aria-label="米家远程摄像头画面" @waiting="waiting"></video>
        <button
          type="button" class="camera-surface" aria-label="视频画面"
          :title="playing ? '点击暂停或播放，按住拖动控制云台' : playbackAction"
          :aria-disabled="!playbackAvailable && !cameraControlsReady"
          @pointerdown="beginGesture" @pointermove="updateGesture" @pointerup="endGesture"
          @pointercancel="cancelGesture" @lostpointercapture="cancelGesture"
          @keydown="cameraKey" @click="event => { if (event.detail === 0) void togglePlayback() }"
        >
          <span v-if="paused || (!running && playbackAvailable)" class="camera-center-icon" aria-hidden="true"><PlayOutline /></span>
          <span v-else-if="!online" class="camera-stage-status">摄像头离线</span>
          <span v-else-if="running && !playing" class="camera-loading" role="status">
            <span class="camera-spinner" aria-hidden="true"></span>
            <span v-if="standalone">{{ label }}</span>
          </span>
        </button>
        <span v-if="playback.width && playback.height" class="camera-resolution" :title="'实际解码分辨率'">{{ playback.width }} × {{ playback.height }}</span>
        <div class="camera-toolbar" role="group" aria-label="摄像头控制">
          <button type="button" :aria-label="playbackAction" :title="playbackAction" :disabled="!playbackAvailable" @click="togglePlayback">
            <PauseOutline v-if="playing && !paused" /><PlayOutline v-else />
          </button>
          <button type="button" aria-label="停止播放" title="停止播放" :disabled="!running" @click="stop"><StopOutline /></button>
          <button type="button" :aria-label="listening ? '现场声音开' : '现场声音关'"
            :title="listening ? '关闭现场声音' : '开启现场声音'"
            :disabled="!cameraControlsReady || audioBusy" :aria-pressed="listening" @click="toggleAudio">
            <VolumeHighOutline v-if="listening" /><VolumeMuteOutline v-else />
          </button>
          <button type="button" :aria-label="talking ? '麦克风对讲开' : '麦克风对讲关'"
            :title="talking ? '关闭麦克风对讲' : '开启麦克风对讲'"
            :disabled="!cameraControlsReady || talkBusy" :aria-pressed="talking" @click="toggleTalk">
            <MicOutline v-if="talking" /><MicOffOutline v-else />
          </button>
          <select aria-label="视频画质" title="视频画质" :value="quality" @change="selectQuality">
            <option v-for="option in qualityOptions" :key="option.value" :value="option.value">{{ option.label }}</option>
          </select>
          <button v-if="standalone" type="button" :aria-label="mini ? '退出小窗' : '小窗模式'"
            :title="mini ? '退出小窗' : '小窗模式'" :aria-pressed="mini" @click="toggleMini"><BrowsersOutline /></button>
          <button v-if="status?.playbackUrl" type="button" :aria-label="isFullscreen ? '退出全屏' : '全屏'"
            :title="isFullscreen ? '退出全屏' : '全屏'" @click="fullscreen">
            <ContractOutline v-if="isFullscreen" /><ExpandOutline v-else />
          </button>
          <button v-if="standalone && (status || localError)" type="button" aria-label="连接结果" title="连接结果"
            :aria-expanded="diagnosticsVisible" @click="diagnosticsVisible = !diagnosticsVisible"><EllipsisHorizontalOutline /></button>
          <button v-if="!standalone && (status || localError)" type="button" aria-label="复制连接结果" title="复制连接结果" @click="copy"><CopyOutline /></button>
        </div>

        <div v-if="standalone && (localError || controlError)" class="camera-notice" role="alert">{{ localError || controlError }}</div>
        <section v-if="standalone && diagnosticsVisible" class="camera-diagnostics" aria-label="连接结果">
          <div class="camera-diagnostics-header">
            <button type="button" @click="copy"><CopyOutline />复制连接结果</button>
            <button type="button" aria-label="关闭连接结果" title="关闭连接结果" @click="diagnosticsVisible = false"><CloseOutline /></button>
          </div>
          <small v-if="copied" role="status">{{ copied }}</small>
          <pre data-remote-camera-result>{{ diagnostic }}</pre>
        </section>
      </div>
    </div>
    <NAlert v-if="!standalone && localError" type="warning" class="remote-notice">{{ localError }}</NAlert>
    <NAlert v-if="!standalone && controlError" type="warning" class="remote-notice">{{ controlError }}</NAlert>
    <small v-if="!standalone && copied" role="status">{{ copied }}</small>
    <details v-if="!standalone && status"><summary>连接结果</summary><pre data-remote-camera-result>{{ diagnostic }}</pre></details>
  </section>
</template>

<style scoped>
.remote-camera {
  margin: 20px 0;
  padding: 16px;
  border: 1px solid #8792a533;
  border-radius: 12px;
}
.remote-title { display: flex; align-items: center; justify-content: space-between; margin-bottom: 10px; }
.remote-title h3 { margin: 0 12px 0 0; font-size: 15px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.remote-stage {
  display: grid;
  place-items: center;
  aspect-ratio: 16 / 9;
  container-type: size;
  background: #0b0f16;
  border-radius: 8px;
  overflow: hidden;
}
.standalone .remote-stage { height: 100vh; aspect-ratio: auto; border-radius: 0; }
.camera-loading { display: grid; justify-items: center; gap: 10px; font-size: 12px; color: #d3dce7; }
.remote-stage:fullscreen { width: 100vw; height: 100vh; aspect-ratio: auto; border-radius: 0; }
.camera-picture {
  width: min(100%, calc(100cqh * var(--camera-ratio)));
  aspect-ratio: var(--camera-ratio);
  position: relative;
  isolation: isolate;
}
.camera-picture video { display: block; width: 100%; height: 100%; object-fit: contain; }
.camera-surface {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  color: white;
  border: 0;
  background: transparent;
  touch-action: none;
  user-select: none;
  cursor: grab;
}
.camera-surface:active { cursor: grabbing; }
.camera-surface:focus-visible { outline: 2px solid #85e1c1; outline-offset: -2px; }
.camera-center-icon {
  width: 52px;
  height: 52px;
  display: grid;
  place-items: center;
  border: 1px solid #ffffff40;
  background: #0b0f16bb;
  border-radius: 50%;
}
.camera-center-icon :deep(svg) { width: 24px; height: 24px; }
.camera-stage-status { color: #d3dce7; font-size: 13px; }
.camera-spinner {
  width: 26px;
  height: 26px;
  border-radius: 50%;
  border: 2px solid #ffffff36;
  border-top-color: white;
  animation: camera-loading .8s linear infinite;
}
@keyframes camera-loading { to { transform: rotate(360deg); } }
@media (prefers-reduced-motion: reduce) { .camera-spinner { animation: none; } }
.camera-resolution {
  position: absolute;
  right: 8px;
  top: 8px;
  padding: 4px 7px;
  border-radius: 5px;
  background: #0b0f16ca;
  color: white;
  font-size: 11px;
  pointer-events: none;
  transition: opacity .2s ease;
}
.camera-toolbar {
  position: absolute;
  inset: auto 0 0;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 3px;
  padding: 8px;
  background: linear-gradient(transparent, #090c12de 55%);
  color: white;
  transition: opacity .2s ease;
}
.standalone .controls-hidden .camera-toolbar:not(:hover, :has(:focus-visible)),
.standalone .controls-hidden .camera-resolution { opacity: 0; }
@media (hover: none) {
  .standalone .controls-hidden .camera-toolbar:not(:hover, :has(:focus-visible)),
  .standalone .controls-hidden .camera-resolution { opacity: 1; }
}
.camera-toolbar button {
  display: grid;
  place-items: center;
  width: 32px;
  height: 32px;
  border: 1px solid transparent;
  border-radius: 6px;
  background: transparent;
  color: inherit;
  cursor: pointer;
}
.camera-toolbar button :deep(svg) { width: 20px; height: 20px; }
.camera-toolbar button:not(:disabled):hover { background: #ffffff30; }
.camera-toolbar button[aria-pressed="true"] { color: #8de4c6; background: #103b36d9; }
.camera-toolbar button:disabled { opacity: .42; cursor: default; }
.camera-toolbar select {
  margin-left: auto;
  height: 32px;
  padding: 0 5px;
  color: inherit;
  color-scheme: dark;
  border: 1px solid #ffffff45;
  border-radius: 6px;
  background: #0b0f16bd;
  font: inherit;
  font-size: 12px;
}
.camera-notice {
  position: absolute;
  inset: auto 12px 58px;
  padding: 8px 12px;
  border: 1px solid #ffcc7c55;
  border-radius: 7px;
  background: #231c10ee;
  color: #ffe0ad;
  font-size: 12px;
  pointer-events: none;
}
.camera-diagnostics {
  position: absolute;
  inset: 8px 8px auto auto;
  width: min(370px, calc(100% - 16px));
  max-height: calc(100% - 64px);
  overflow: auto;
  padding: 8px 12px;
  border: 1px solid #ffffff35;
  border-radius: 8px;
  background: #10151ef5;
  color: #f0f3f7;
}
.camera-diagnostics-header { display: flex; justify-content: space-between; align-items: center; gap: 8px; }
.camera-diagnostics-header button {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px;
  border: 0;
  border-radius: 4px;
  color: inherit;
  background: #ffffff18;
  cursor: pointer;
}
.camera-diagnostics-header :deep(svg) { width: 16px; height: 16px; }
.remote-notice { margin: 12px 0; }
.remote-camera details { margin-top: 12px; font-size: 12px; color: #8792a5; }
.remote-camera pre { white-space: pre-wrap; overflow-wrap: anywhere; user-select: text; font-size: 11px; }
.remote-camera small { display: block; margin-top: 8px; font-size: 12px; }
</style>
