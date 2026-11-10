import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { useConnectionStore } from './connection'
import { useDevicesStore } from './devices'
import { call } from '../utils/ipc'
import type { AppError } from '../types/settings'
import type { ServiceResult } from '../types/devices'
interface SceneFavorite { sceneEntityId: string; isFavorite: boolean }
export const useScenesStore = defineStore('scenes', () => {
  const connection = useConnectionStore()
  const devices = useDevicesStore()
  const favorites = ref<Record<string, boolean>>({})
  const loaded = ref(false)
  const loading = ref(false)
  const error = ref<AppError | null>(null)
  const pending = ref<Record<string, boolean>>({})
  const saving = ref<Record<string, boolean>>({})
  const feedback = ref<Record<string, { success: boolean; message: string }>>({})
  const list = computed(() => connection.entities.filter(entity => entity.domain === 'scene').map(entity => ({...entity,name:devices.preferenceFor(entity.entityId).displayName ?? entity.name,favorite:!!favorites.value[entity.entityId]})))
  async function load() {
    if (loading.value) return
    loading.value=true;error.value=null
    const result=await call<SceneFavorite[]>('scene_get_favorites')
    if (result.ok) {favorites.value=Object.fromEntries(result.data.map(item=>[item.sceneEntityId,item.isFavorite]));loaded.value=true} else {error.value=result.error;loaded.value=false}
    loading.value=false
  }
  async function favorite(id: string, value: boolean) {
    if (!loaded.value || saving.value[id]) return
    saving.value[id]=true
    const result=await call<SceneFavorite>('scene_set_favorite',{sceneEntityId:id,favorite:value})
    if (result.ok) {favorites.value[id]=result.data.isFavorite;error.value=null} else error.value=result.error
    saving.value[id]=false
  }
  function canExecute(id: string) {
    return connection.status.state==='connected' && connection.catalog.servicesLoaded && !!connection.catalog.services.scene?.includes('turn_on') && list.value.some(entity=>entity.entityId===id && entity.state!=='unavailable') && !pending.value[id]
  }
  async function execute(id: string) {
    if (!canExecute(id)) return
    pending.value[id]=true;delete feedback.value[id]
    const result=await call<ServiceResult>('scene_execute',{sceneEntityId:id})
    feedback.value[id]=result.ok?{success:true,message:'HA 已处理场景请求；实际设备变化请查看实时状态。'}:{success:false,message:`${result.error.code}：${result.error.message}`}
    pending.value[id]=false
  }
  return {list,loaded,loading,error,pending,saving,feedback,load,favorite,canExecute,execute}
})
