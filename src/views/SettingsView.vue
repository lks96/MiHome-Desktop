<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { NAlert, NButton, NForm, NFormItem, NInputNumber, NSelect, NSwitch, NSpin } from 'naive-ui'
import { useSettingsStore } from '../stores/settings'
import ConnectionSettings from '../components/ConnectionSettings.vue'
import CameraSettings from '../components/CameraSettings.vue'
import DiagnosticsPanel from '../components/DiagnosticsPanel.vue'
import { useDesktopStore } from '../stores/desktop'
defineProps<{ advanced?: boolean }>()
const store = useSettingsStore()
const desktop = useDesktopStore()
const draft = reactive({ ...store.settings })
const saved = ref(false)
watch(() => store.settings, value => { Object.assign(draft, value) })
watch(draft, () => { saved.value = false })
const dirty = computed(() => JSON.stringify(draft) !== JSON.stringify(store.settings))
const timeoutValid = computed(() => Number.isInteger(draft.requestTimeout) && draft.requestTimeout >= 1 && draft.requestTimeout <= 120)
async function save() {
  if (timeoutValid.value) { saved.value = await store.save({ ...draft }); if (saved.value) await desktop.refresh() }
}
const themes = [{ label: '浅色', value: 'light' }, { label: '深色', value: 'dark' }, { label: '跟随系统', value: 'system' }]
</script>

<template>
  <div class="page-heading"><div><div class="eyebrow">PREFERENCES</div><h1>系统设置</h1><p>管理本地偏好与应用配置。</p></div></div>
  <NAlert v-if="store.error" class="settings-alert" :type="store.error.code === 'DESKTOP_REQUIRED' ? 'info' : 'error'" :title="store.error.code">
    {{ store.error.message }}
    <NButton v-if="store.error.retryable" size="small" :disabled="store.loading || store.saving" @click="store.load">重新读取</NButton>
  </NAlert>
  <NAlert v-if="saved" class="settings-alert" type="success">设置已保存到本地数据库。</NAlert>
  <details class="panel settings-panel" :open="advanced"><summary>高级接入 · Home Assistant 与视频网关</summary><ConnectionSettings /><CameraSettings /></details>
  <NSpin :show="store.loading">
    <section class="panel settings-panel">
      <h2>{{ advanced ? '外观与连接偏好' : '外观与桌面偏好' }}</h2><p class="settings-note">{{ advanced ? '外观保存后立即应用；HA 连接偏好将在下次建立连接时生效。' : '外观保存后立即应用。米家登录页勾选“记住登录”即可在下次启动时自动连接。' }}</p>
      <NForm label-placement="left" label-width="150" :disabled="!store.loaded || store.saving || store.loading">
        <NFormItem label="外观主题"><NSelect v-model:value="draft.theme" :options="themes" /></NFormItem>
        <template v-if="advanced"><NFormItem label="请求超时（秒）" :validation-status="timeoutValid ? undefined : 'error'" :feedback="timeoutValid ? undefined : '请输入 1 至 120 之间的整数'">
          <NInputNumber :value="draft.requestTimeout" :min="1" :max="120" :precision="0" @update:value="draft.requestTimeout = $event ?? 0" />
        </NFormItem>
        <NFormItem label="启动时自动连接"><NSwitch v-model:value="draft.autoConnect" /></NFormItem>
        <NFormItem label="断线后自动重连"><NSwitch v-model:value="draft.autoReconnect" /></NFormItem></template>
        <NFormItem label="关闭窗口后留在托盘"><NSwitch v-model:value="draft.closeToTray" aria-label="关闭窗口后留在托盘" :disabled="!desktop.status?.trayAvailable && !draft.closeToTray" /></NFormItem>
        <NFormItem label="Windows 开机启动"><NSwitch v-model:value="draft.autoStart" aria-label="Windows 开机启动" :disabled="!desktop.status?.autoStartSupported" /></NFormItem>
      </NForm>
      <p class="settings-note">窗口大小、位置和最大化状态自动保存。关闭到托盘后，视频会话会释放；可从托盘恢复窗口或退出应用。</p>
      <p v-if="desktop.status" class="settings-note">托盘：{{ desktop.status.trayAvailable ? '可用' : '不可用，关闭窗口将退出' }} · 系统启动注册：{{ desktop.status.autoStartEnabled === null ? '无法读取 / 平台不支持' : desktop.status.autoStartEnabled ? '已启用' : '未启用' }}</p>
      <NAlert v-if="desktop.error" type="warning" class="settings-alert">{{ desktop.error.message }}<NButton size="small" @click="desktop.refresh">重试读取桌面状态</NButton></NAlert>
      <div class="settings-actions"><NButton type="primary" :loading="store.saving" :disabled="!store.loaded || !dirty || !timeoutValid || store.loading" @click="save">保存设置</NButton><NButton :disabled="!dirty || store.saving" @click="Object.assign(draft, store.settings)">撤销更改</NButton></div>
    </section>
    <DiagnosticsPanel />
    <section class="panel settings-panel"><h2>关于 Mi Home Desktop</h2><p class="settings-note">{{ store.info ? `版本 ${store.info.version} · ${store.info.platform}` : '桌面客户端启动后显示版本信息' }}</p><p class="settings-note">本地设置存储于应用数据目录；访问令牌存入 Windows 凭据管理器，不写入本地数据库。</p></section>
  </NSpin>
</template>

<style scoped>
.settings-alert{margin-bottom:16px}.settings-panel{max-width:760px;margin-bottom:18px;padding:24px}.settings-panel h2{font-size:16px;margin:0 0 10px}.settings-note{color:#8792a5;font-size:12px;line-height:1.8;margin:0 0 22px}.settings-actions{display:flex;gap:12px;justify-content:flex-end}.n-select{max-width:260px}
</style>
