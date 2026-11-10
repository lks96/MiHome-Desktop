<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { NAlert, NButton, NCheckbox } from 'naive-ui'
import { call } from '../utils/ipc'
import { stateLabels, type ConnectionStatus } from '../types/connection'
interface Info {version:string;platform:string;connectionState:ConnectionStatus['state'];retryAttempt:number;errorCode:string|null;entityCount:number;cameraCount:number;sceneCount:number}
const info=ref<Info | null>(null)
const error=ref('')
const notice=ref('')
const acknowledged=ref(false)
const exporting=ref(false)
const loading=ref(false)
async function refresh() {
  if(loading.value)return
  loading.value=true;error.value=''
  const result=await call<Info>('settings_get_diagnostics')
  if(result.ok)info.value=result.data;else error.value=result.error.message
  loading.value=false
}
async function exportLogs() {
  if(!acknowledged.value || exporting.value)return
  exporting.value=true;error.value='';notice.value=''
  const result=await call<{cancelled:boolean;fileName:string|null}>('settings_export_logs',{acknowledged:true})
  if(result.ok)notice.value=result.data.cancelled?'已取消导出。':`已保存诊断包：${result.data.fileName ?? 'JSON 文件'}`
  else error.value=result.error.message
  exporting.value=false
}
onMounted(()=>{void refresh()})
</script>
<template>
  <section class="panel diagnostics-panel"><div class="diagnostics-heading"><h2>日志与诊断</h2><NButton size="small" :loading="loading" @click="refresh">刷新诊断</NButton></div>
    <NAlert v-if="error" type="info" class="diagnostics-alert">{{ error }}</NAlert><NAlert v-if="notice" type="success" class="diagnostics-alert">{{ notice }}</NAlert>
    <dl v-if="info" class="diagnostic-summary"><dt>版本 / 平台</dt><dd>{{ info.version }} / {{ info.platform }}</dd><dt>实时连接</dt><dd>{{ stateLabels[info.connectionState] }} · 重试 {{ info.retryAttempt }} 次</dd><dt>最近错误码</dt><dd>{{ info.errorCode ?? '无' }}</dd><dt>同步数量</dt><dd>{{ info.entityCount }} 个实体 · {{ info.sceneCount }} 个场景 · {{ info.cameraCount }} 个摄像头</dd></dl>
    <p class="diagnostics-note">导出内容为版本、连接状态、计数与经过白名单过滤的日志记录，不包含数据库、凭据、视频 URL 或 SDP。旧版未结构化日志及未审核字段会省略。</p>
    <NCheckbox v-model:checked="acknowledged" :disabled="exporting">我了解源日志可能包含设备名称和局域网地址，同意导出脱敏诊断包。</NCheckbox>
    <div class="diagnostics-actions"><NButton type="primary" :disabled="!acknowledged || !info" :loading="exporting" @click="exportLogs">导出诊断包</NButton></div>
  </section>
</template>
<style scoped>
.diagnostics-panel{max-width:760px;padding:24px;margin-bottom:18px}.diagnostics-heading{display:flex;justify-content:space-between;align-items:center;margin-bottom:16px}.diagnostics-heading h2{font-size:16px;margin:0}.diagnostics-alert{margin-bottom:14px}.diagnostic-summary{display:grid;grid-template-columns:120px 1fr;gap:12px;font-size:12px}.diagnostic-summary dt,.diagnostics-note{color:#8792a5}.diagnostic-summary dd{margin:0;overflow-wrap:anywhere}.diagnostics-note{font-size:12px;line-height:1.8}.diagnostics-actions{display:flex;justify-content:flex-end;margin-top:20px}
</style>
