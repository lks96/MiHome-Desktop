import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { isTauri } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { call } from '../utils/ipc'
import type { DeviceCatalog } from '../types/connection'
import type { AppError } from '../types/settings'
import { stateLabels, type ConnectionStatus, type ConnectionSummary, type EntitySummary } from '../types/connection'

export const useConnectionStore = defineStore('connection', () => {
  const saved = ref<ConnectionSummary | null>(null)
  const status = ref<ConnectionStatus>({ state: 'disconnected', attempt: 0, error: null })
  const entities = ref<EntitySummary[]>([])
  const catalog = ref<DeviceCatalog>({ areas: [], services: {}, entityAreas: {}, warnings: [], servicesLoaded: false })
  const busy = ref(false)
  const syncing = ref(false)
  const ready = ref(false)
  const error = ref<AppError | null>(null)
  const notice = ref('')
  const label = computed(() => stateLabels[status.value.state])
  const displayError = computed(() => error.value ?? status.value.error)
  const listeners: UnlistenFn[] = []
  let disposed = false
  let statusRevision = 0
  let entityRevision = 0
  let catalogRevision = 0
  async function register<T>(name: string, handler: (payload: T) => void) {
    const unlisten = await listen<T>(name, event => handler(event.payload))
    if (disposed) unlisten(); else listeners.push(unlisten)
  }
  async function initialize() {
    if (!isTauri()) return
    try {
      await register<ConnectionStatus>('ha:connection_changed', value => { statusRevision++; status.value = value; if (value.state !== 'authenticating') syncing.value = false })
      await register<DeviceCatalog>('ha:catalog_changed', value => { catalogRevision++; catalog.value = value })
      await register<null>('ha:sync_started', () => { syncing.value = true })
      await register<EntitySummary[]>('ha:sync_completed', value => { entityRevision++; entities.value = value; syncing.value = false })
      await register<EntitySummary>('device:state_changed', value => {
        entityRevision++
        const index = entities.value.findIndex(entity => entity.entityId === value.entityId)
        if (index < 0) entities.value.push(value); else entities.value[index] = value
      })
      await register<string>('device:removed', id => { entityRevision++; entities.value = entities.value.filter(entity => entity.entityId !== id) })
      const revision = statusRevision
      const entityVersion = entityRevision
      const catalogVersion = catalogRevision
      const [connection, state, list, metadata] = await Promise.all([
        call<ConnectionSummary | null>('ha_get_saved_connection'), call<ConnectionStatus>('ha_get_connection_status'), call<EntitySummary[]>('ha_get_entities'), call<DeviceCatalog>('ha_get_catalog'),
      ])
      if (connection.ok) saved.value = connection.data; else error.value = connection.error
      if (state.ok && revision === statusRevision) status.value = state.data
      if (list.ok && entityVersion === entityRevision) entities.value = list.data
      if (metadata.ok && catalogVersion === catalogRevision) catalog.value = metadata.data
      if (!state.ok) error.value = state.error
      if (!list.ok) error.value = list.error
      ready.value = connection.ok && state.ok && list.ok
    } catch {
      listeners.splice(0).forEach(fn => fn())
      error.value = { code: 'EVENTS_UNAVAILABLE', message: '无法订阅实时状态，请重启桌面客户端。', retryable: false }
    }
  }
  function dispose() { disposed = true; listeners.splice(0).forEach(fn => fn()) }
  async function test(baseUrl: string, token: string) {
    if (busy.value) return
    busy.value = true; error.value = null; notice.value = ''
    const result = await call<{ message: string }>('ha_test_connection', { baseUrl, token })
    if (result.ok) notice.value = 'REST API 认证成功。连接配置尚未保存。'; else error.value = result.error
    busy.value = false
  }
  async function save(name: string, baseUrl: string, token: string) {
    if (busy.value) return
    busy.value = true; error.value = null; notice.value = ''
    const result = await call<ConnectionSummary>('ha_save_connection', { name, baseUrl, token })
    if (result.ok) {
      saved.value = result.data
      notice.value = '连接配置已保存。实时连接状态见上方标签。'
      const connected = await call<ConnectionStatus>('ha_connect', { connectionId: result.data.id })
      if (!connected.ok) error.value = connected.error
    } else error.value = result.error
    busy.value = false
  }
  async function connect() {
    if (!saved.value || busy.value) return
    busy.value = true; error.value = null; notice.value = ''
    const result = await call<ConnectionStatus>('ha_connect', { connectionId: saved.value.id })
    if (!result.ok) error.value = result.error
    busy.value = false
  }
  async function disconnect() {
    if (busy.value) return
    busy.value = true; error.value = null; notice.value = ''
    const result = await call<boolean>('ha_disconnect')
    if (!result.ok) error.value = result.error
    busy.value = false
  }
  return { saved, status, entities, catalog, busy, syncing, ready, error, notice, label, displayError, initialize, dispose, test, save, connect, disconnect }
})
