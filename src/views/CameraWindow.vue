<script setup lang="ts">
import { onMounted, ref, computed } from 'vue'
import { NAlert, NConfigProvider, NSpin, darkTheme } from 'naive-ui'
import RemoteCameraPlayer from '../components/RemoteCameraPlayer.vue'
import { call } from '../utils/ipc'
import { useSettingsStore } from '../stores/settings'
import type { MiDevice } from '../types/mijia'
const settings = useSettingsStore()
const device = ref<MiDevice | null>(null)
const loading = ref(true)
const error = ref('')
const dark = computed(() => settings.settings.theme === 'dark' || (settings.settings.theme === 'system' && matchMedia('(prefers-color-scheme: dark)').matches))
onMounted(async () => {
  await settings.load()
  const result = await call<MiDevice>('camera_player_context')
  if (result.ok) device.value = result.data; else error.value = result.error.message
  loading.value = false
})
</script>
<template>
  <NConfigProvider :theme="dark ? darkTheme : null"><main class="camera-window">
    <NSpin :show="loading"><NAlert v-if="error" type="warning">{{ error }}</NAlert>
      <RemoteCameraPlayer v-if="device" :did="device.did" :online="device.online" :name="device.name" standalone auto-start />
    </NSpin>
  </main></NConfigProvider>
</template>
<style scoped>
.camera-window{min-height:100vh;background:#0b0f16;color:#d7e2f2}.camera-window :deep(.remote-camera){margin:0;padding:0;border:0;background:transparent}
</style>
<style>body.camera-window-body{min-width:0}</style>
