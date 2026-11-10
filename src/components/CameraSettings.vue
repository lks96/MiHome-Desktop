<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { NAlert, NButton, NFormItem, NInput, NSelect, NSwitch } from 'naive-ui'
import { useCamerasStore } from '../stores/cameras'
const cameras = useCamerasStore()
const gateway = reactive({ ...cameras.gateway })
const selected = ref<string | null>(null)
const profile = reactive({ entityId: '', sourceType: 'snapshot' as 'snapshot' | 'go2rtc', streamName: null as string | null, enabled: true })
const endpointPlaceholder = 'http://192.168.1.10:1984'
onMounted(() => { void cameras.load() })
watch(() => cameras.gateway, value => Object.assign(gateway, value))
watch([selected, () => cameras.profiles], () => {
  const value = cameras.list.find(camera => camera.entityId === selected.value)
  if (value) Object.assign(profile, value.profile)
})
const cameraOptions = computed(() => cameras.list.map(camera => ({ label: camera.name, value: camera.entityId })))
const sourceOptions = [{ label: 'HA 截图（不是实时视频）', value: 'snapshot' }, { label: 'go2rtc WebRTC（待实机验证）', value: 'go2rtc' }]
</script>
<template>
  <section class="panel camera-settings"><h2>摄像头与 go2rtc</h2>
    <NAlert v-if="cameras.error" type="error" class="camera-alert">{{ cameras.error.message }}<NButton v-if="!cameras.loaded" size="small" :disabled="cameras.loading" @click="cameras.load">重新读取</NButton></NAlert>
    <NAlert v-if="cameras.notice" type="info" class="camera-alert">{{ cameras.notice }}</NAlert>
    <p class="camera-note">go2rtc 需独立部署，米家认证和流来源由其管理。此处只保存无认证信息的接口地址与已配置流别名。</p>
    <NFormItem label="启用 go2rtc"><NSwitch v-model:value="gateway.enabled" :disabled="!cameras.loaded || cameras.saving" /></NFormItem>
    <NFormItem label="go2rtc API 地址"><NInput v-model:value="gateway.baseUrl" :input-props="{ 'aria-label': 'go2rtc API 地址' }" :placeholder="endpointPlaceholder" :disabled="!cameras.loaded || cameras.saving" /></NFormItem>
    <div class="camera-actions"><NButton :disabled="!cameras.loaded || cameras.saving || !gateway.baseUrl.trim()" @click="cameras.probe({ ...gateway })">检测接口与别名</NButton><NButton type="primary" :disabled="!cameras.loaded || cameras.saving" @click="cameras.saveGateway({ ...gateway })">保存视频服务</NButton></div>
    <p v-if="cameras.aliases.length" class="camera-note">可用别名：{{ cameras.aliases.join('、') }}</p>
    <NFormItem label="摄像头实体"><NSelect v-model:value="selected" :options="cameraOptions" clearable placeholder="先连接 HA 并同步摄像头实体" :disabled="!cameras.loaded || cameras.saving" /></NFormItem>
    <template v-if="selected">
      <NFormItem label="播放方式"><NSelect v-model:value="profile.sourceType" :options="sourceOptions" :disabled="cameras.saving" /></NFormItem>
      <NFormItem v-if="profile.sourceType === 'go2rtc'" label="go2rtc 流别名"><NInput v-model:value="profile.streamName" placeholder="例如 camera_living；不要填写 RTSP 或认证 URL" :disabled="cameras.saving" /></NFormItem>
      <NFormItem label="启用此摄像头"><NSwitch v-model:value="profile.enabled" :disabled="cameras.saving" /></NFormItem>
      <div class="camera-actions"><NButton type="primary" :disabled="cameras.saving || (profile.sourceType === 'go2rtc' && !profile.streamName)" @click="cameras.saveProfile({ ...profile })">保存摄像头配置</NButton></div>
    </template>
    <p class="camera-note">当前接入 HA 截图与 go2rtc WebRTC。HA 原生直播、MSE/HLS 回退和 FFmpeg 转码尚未接入。API 可访问不代表摄像头兼容；H.265/HEVC 需在目标 WebView2 中验证，必要时在视频服务侧评估 H.264 转码。</p>
  </section>
</template>
<style scoped>
.camera-settings{max-width:760px;margin-bottom:18px;padding:24px}.camera-settings h2{font-size:16px;margin:0 0 16px}.camera-alert{margin-bottom:14px}.camera-note{font-size:12px;line-height:1.8;color:#8792a5;overflow-wrap:anywhere}.camera-actions{display:flex;gap:12px;justify-content:flex-end;margin:16px 0}
</style>
