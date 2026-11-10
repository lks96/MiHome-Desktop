import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { useConnectionStore } from './connection'
import { call } from '../utils/ipc'
import type { AppError } from '../types/settings'
import type { EntityPreference, ServiceResult } from '../types/devices'
import type { EntitySummary } from '../types/connection'
export const useDevicesStore = defineStore('devices', () => {
  const connection = useConnectionStore()
  const preferences = ref<Record<string, EntityPreference>>({})
  const loaded = ref(false)
  const loading = ref(false)
  const error = ref<AppError | null>(null)
  const pending = ref<Record<string, boolean>>({})
  const saving = ref<Record<string, boolean>>({})
  const feedback = ref<Record<string, { success: boolean; message: string }>>({})
  function preferenceFor(id: string): EntityPreference {
    return preferences.value[id] ?? { entityId: id, displayName: null, areaIdOverride: null, isFavorite: false, isHidden: false, sortOrder: 0 }
  }
  const entities = computed(() => connection.entities.map(entity => {
    const preference = preferenceFor(entity.entityId)
    const areaId = preference.areaIdOverride ?? connection.catalog.entityAreas[entity.entityId] ?? null
    return { ...entity, preference, displayName: preference.displayName ?? entity.name, areaId, areaName: connection.catalog.areas.find(area => area.areaId === areaId)?.name ?? (areaId ? '房间信息不可用' : '未分配房间') }
  }))
  async function load() {
    if (loading.value) return
    loading.value = true; error.value = null
    const result = await call<EntityPreference[]>('device_get_preferences')
    if (result.ok) { preferences.value = Object.fromEntries(result.data.map(value => [value.entityId, value])); loaded.value = true }
    else { error.value = result.error; loaded.value = false }
    loading.value = false
  }
  async function update(preference: EntityPreference): Promise<boolean> {
    if (!loaded.value || saving.value[preference.entityId]) return false
    saving.value[preference.entityId] = true
    const result = await call<EntityPreference>('device_update_preferences', { preference })
    if (result.ok) { preferences.value[preference.entityId] = result.data; error.value = null }
    else error.value = result.error
    saving.value[preference.entityId] = false
    return result.ok
  }
  function hasService(entity: EntitySummary, service: string) {
    return connection.catalog.servicesLoaded && !!connection.catalog.services[entity.domain]?.includes(service)
  }
  function canControl(entity: EntitySummary) {
    return connection.status.state === 'connected' && !['unavailable', 'unknown'].includes(entity.state) && !pending.value[entity.entityId]
  }
  async function action(entity: EntitySummary, service: string, serviceData: Record<string, unknown> = {}) {
    if (!canControl(entity) || !hasService(entity, service)) return
    pending.value[entity.entityId] = true
    delete feedback.value[entity.entityId]
    const result = await call<ServiceResult>('device_call_service', { request: { entityId: entity.entityId, domain: entity.domain, service, serviceData } })
    feedback.value[entity.entityId] = result.ok ? { success: true, message: result.data.message } : { success: false, message: `${result.error.code}：${result.error.message}` }
    pending.value[entity.entityId] = false
    // Do not modify entity state here. Only HA websocket events update it.
  }
  return { entities, preferences, loaded, loading, error, pending, saving, feedback, preferenceFor, load, update, hasService, canControl, action }
})
