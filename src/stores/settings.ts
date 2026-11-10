import { defineStore } from 'pinia'
import { ref } from 'vue'
import { call } from '../utils/ipc'
import type { AppSettings, AppError, AppInfo } from '../types/settings'

export const useSettingsStore = defineStore('settings', () => {
  const settings = ref<AppSettings>({ theme: 'light', requestTimeout: 10, autoConnect: true, autoReconnect: true, closeToTray: false, autoStart: false })
  const info = ref<AppInfo | null>(null)
  const loading = ref(false)
  const saving = ref(false)
  const loaded = ref(false)
  const error = ref<AppError | null>(null)
  async function load() {
    if (loading.value || saving.value) return
    loading.value = true
    error.value = null
    const result = await call<AppSettings>('settings_get')
    if (result.ok) { settings.value = {...result.data,closeToTray:result.data.closeToTray ?? false,autoStart:result.data.autoStart ?? false}; loaded.value = true }
    else { error.value = result.error; loaded.value = false }
    const appInfo = await call<AppInfo>('settings_get_app_info')
    if (appInfo.ok) info.value = appInfo.data
    loading.value = false
  }
  async function save(value: AppSettings): Promise<boolean> {
    if (!loaded.value || saving.value || loading.value) return false
    saving.value = true
    error.value = null
    const result = await call<AppSettings>('settings_update', { settings: value })
    if (result.ok) settings.value = result.data
    else error.value = result.error
    saving.value = false
    return result.ok
  }
  return { settings, info, loading, saving, loaded, error, load, save }
})
