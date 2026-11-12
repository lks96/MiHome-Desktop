<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { NAlert, NButton } from 'naive-ui'
import { call } from '../utils/ipc'
import type { AppError } from '../types/settings'
const props = defineProps<{ did: string; online: boolean }>()
interface Report { model: string; transport: string; cloudAuthorizationGranted: boolean; remotePlaybackVerified: boolean; observedAt: number }
const busy = ref(false)
const report = ref<Report | null>(null)
const error = ref<AppError | null>(null)
const copied = ref('')
let generation = 0
function reset() { generation++; busy.value = false; report.value = null; error.value = null; copied.value = '' }
watch(() => props.did, reset)
onUnmounted(reset)
const diagnostic = computed(() => {
  if (!report.value && !error.value) return ''
  return JSON.stringify({ diagnosticSchema: 1, model: 'chuangmi.camera.039a01',
    ...(report.value ? { transport: report.value.transport, cloudAuthorizationGranted: report.value.cloudAuthorizationGranted,
      remotePlaybackVerified: false, observedAt: report.value.observedAt } : { errorCode: error.value?.code }),
  }, null, 2)
})
async function probe() {
  if (busy.value || !props.online) return
  const current = ++generation
  busy.value = true; report.value = null; error.value = null; copied.value = ''
  const result = await call<Report>('mijia_camera_probe', { did: props.did })
  if (current !== generation) return
  if (result.ok) report.value = result.data; else error.value = result.error
  busy.value = false
}
async function copy() {
  try { await navigator.clipboard.writeText(diagnostic.value); copied.value = '已复制检测结果' }
  catch { copied.value = '请手动选中下方检测结果复制' }
}
</script>

<template>
  <section class="camera-probe">
    <h3>远程视频接入检测</h3>
    <p>检测当前米家账号能否取得这台摄像头的视频授权。检测不会播放画面，远程通道仍在适配中。</p>
    <NButton aria-label="检测视频授权" :aria-busy="busy" :loading="busy" :disabled="!online || busy" @click="probe">检测视频授权</NButton>
    <NAlert v-if="report" type="info" class="probe-notice">云端视频授权已取得 · 协议 {{ report.transport.toUpperCase() }}。尚未建立远程视频通道，本结果不代表已经可以播放。</NAlert>
    <NAlert v-if="error" type="warning" class="probe-notice" role="alert">{{ error.message }}</NAlert>
    <template v-if="diagnostic"><p>下方结果可用于继续适配，不包含账号令牌、视频密钥或播放地址。</p><pre data-camera-probe-result>{{ diagnostic }}</pre><NButton size="small" @click="copy">复制检测结果</NButton><small v-if="copied" role="status">{{ copied }}</small></template>
  </section>
</template>

<style scoped>
.camera-probe{margin:20px 0;padding:16px;border:1px solid #8792a533;border-radius:12px}.camera-probe h3{margin:0 0 10px;font-size:15px}.camera-probe p{font-size:12px;color:#8792a5;line-height:1.8}.probe-notice{margin-top:14px}.camera-probe pre{padding:12px;background:#8792a511;font-size:11px;white-space:pre-wrap;overflow-wrap:anywhere;user-select:text}.camera-probe small{display:block;margin-top:10px;color:#8792a5}
</style>
