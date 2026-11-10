<script setup lang="ts">
import { computed, onUnmounted, reactive, watch } from 'vue'
import { isTauri } from '@tauri-apps/api/core'
import { NAlert, NButton, NForm, NFormItem, NInput, NTag } from 'naive-ui'
import { useConnectionStore } from '../stores/connection'
const store = useConnectionStore()
const urlPlaceholder = 'http://homeassistant.local:8123'
const draft = reactive({ name: store.saved?.name ?? 'Home Assistant', baseUrl: store.saved?.baseUrl ?? '', token: '' })
watch(() => store.saved, saved => { if (saved) { draft.name = saved.name; draft.baseUrl = saved.baseUrl } })
const canSubmit = computed(() => store.ready && !store.busy && draft.name.trim().length > 0 && draft.name.length <= 80 && draft.token.trim().length > 0 && /^https?:\/\//i.test(draft.baseUrl.trim()))
function setName(value: string) { draft.name = value }
function setUrl(value: string) { draft.baseUrl = value }
function setToken(value: string) { draft.token = value }
const canDisconnect = computed(() => ['connecting', 'authenticating', 'connected', 'reconnecting'].includes(store.status.state))
async function save() { try { await store.save(draft.name, draft.baseUrl, draft.token) } finally { draft.token = '' } }
onUnmounted(() => { draft.token = '' })
</script>
<template>
  <section class="panel connection-settings">
    <div class="connection-heading"><h2>Home Assistant 连接</h2><NTag :type="store.status.state === 'connected' ? 'success' : 'default'" size="small">{{ store.label }}</NTag></div>
    <NAlert v-if="!isTauri()" type="info" class="connection-alert">请在 Windows 桌面客户端中测试和保存连接，浏览器预览不会存储令牌。</NAlert>
    <NAlert v-if="store.displayError" type="error" class="connection-alert" :title="store.displayError.code">{{ store.displayError.message }}<span v-if="store.status.state === 'reconnecting'">（第 {{ store.status.attempt }} 次重试）</span></NAlert>
    <NAlert v-if="store.notice" type="success" class="connection-alert">{{ store.notice }}</NAlert>
    <p class="connection-note">先在 Home Assistant 安装 Xiaomi Miot 并接入设备，然后填写实例地址与长期访问令牌。保存前会验证 API，令牌仅交给 Windows 凭据管理器。</p>
    <NForm label-placement="top" :disabled="!store.ready || store.busy" @submit.prevent="save">
      <NFormItem label="连接名称"><NInput :value="draft.name" @update:value="setName" :maxlength="80" placeholder="Home Assistant" /></NFormItem>
      <NFormItem label="实例地址"><NInput :value="draft.baseUrl" @update:value="setUrl" :placeholder="urlPlaceholder" autocomplete="off" /></NFormItem>
      <NFormItem label="长期访问令牌"><NInput :value="draft.token" @update:value="setToken" type="password" show-password-on="click" autocomplete="off" :maxlength="8192" placeholder="填写令牌；不会在界面中回显已保存的值" /></NFormItem>
      <p v-if="draft.baseUrl.trim().toLowerCase().startsWith('http:')" class="connection-note">HTTP 使用明文传输，请仅用于可信局域网；远程连接请使用 HTTPS 或 VPN。</p>
      <div class="connection-actions"><NButton :disabled="!canSubmit" :loading="store.busy" @click="store.test(draft.baseUrl, draft.token)">测试连接</NButton><NButton type="primary" :disabled="!canSubmit" :loading="store.busy" @click="save">保存并连接</NButton></div>
    </NForm>
    <div v-if="store.saved" class="saved-connection"><strong>已保存：{{ store.saved.name }}</strong><span>{{ store.saved.baseUrl }}</span><div class="connection-actions"><NButton size="small" :disabled="store.busy || !store.ready" @click="store.connect">{{ store.status.state === 'connected' ? '重新连接并同步' : '连接已保存配置' }}</NButton><NButton size="small" :disabled="store.busy || !canDisconnect" @click="store.disconnect">断开连接</NButton></div></div>
  </section>
</template>
<style scoped>
.connection-settings{max-width:760px;margin-bottom:18px;padding:24px}.connection-heading{display:flex;justify-content:space-between;align-items:center;margin-bottom:14px}.connection-heading h2{margin:0;font-size:16px}.connection-note{color:#8792a5;font-size:12px;line-height:1.8}.connection-alert{margin-bottom:14px}.connection-actions{display:flex;gap:10px;justify-content:flex-end}.saved-connection{border-top:1px solid #e8eaed;margin-top:22px;padding-top:18px}.saved-connection strong,.saved-connection span{display:block;font-size:12px;margin-bottom:8px}.saved-connection span{overflow-wrap:anywhere;color:#8792a5}
</style>
