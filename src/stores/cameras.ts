import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { useConnectionStore } from './connection'
import { useDevicesStore } from './devices'
import { call } from '../utils/ipc'
import type { AppError } from '../types/settings'
import type { CameraConfiguration, CameraGateway, CameraProfile, CameraView } from '../types/cameras'
export const useCamerasStore = defineStore('cameras', () => {
  const connection = useConnectionStore()
  const devices = useDevicesStore()
  const gateway = ref<CameraGateway>({ baseUrl: '', enabled: false })
  const profiles = ref<CameraProfile[]>([])
  const loaded = ref(false)
  const loading = ref(false)
  const saving = ref(false)
  const error = ref<AppError | null>(null)
  const notice = ref('')
  const aliases = ref<string[]>([])
  const list = computed<CameraView[]>(() => {
    const ids = new Set([...connection.entities.filter(entity => entity.domain === 'camera').map(entity => entity.entityId), ...profiles.value.map(profile => profile.entityId)])
    return [...ids].sort().map(entityId => {
      const entity = connection.entities.find(value => value.entityId === entityId)
      const name = devices.preferenceFor(entityId).displayName ?? entity?.name ?? entityId
      const features = entity?.attributes.supported_features
      return { entityId, name, haState: entity?.state ?? null, haStreamDeclared: typeof features === 'number' && (features & 2) !== 0, profile: profiles.value.find(profile => profile.entityId === entityId) ?? { entityId, sourceType: 'snapshot', streamName: null, enabled: true } }
    })
  })
  async function load() {
    if (loading.value) return
    loading.value = true; error.value = null
    const result = await call<CameraConfiguration>('camera_get_configuration')
    if (result.ok) { gateway.value = result.data.gateway; profiles.value = result.data.profiles; loaded.value = true }
    else { error.value = result.error; loaded.value = false }
    loading.value = false
  }
  async function saveGateway(value: CameraGateway) {
    if (!loaded.value || saving.value) return
    saving.value = true; error.value = null; notice.value = ''
    const result = await call<CameraGateway>('camera_save_gateway', { gateway: value })
    if (result.ok) { gateway.value = result.data; aliases.value = []; notice.value = 'go2rtc 配置已保存。' } else error.value = result.error
    saving.value = false
  }
  async function probe(value: CameraGateway) {
    if (saving.value) return
    saving.value = true; error.value = null; notice.value = ''; aliases.value = []
    const result = await call<{ streamNames: string[] }>('camera_probe_gateway', { gateway: value })
    if (result.ok) { aliases.value = result.data.streamNames; notice.value = `API 可访问，发现 ${aliases.value.length} 个可用流别名；尚未验证视频解码。` } else error.value = result.error
    saving.value = false
  }
  async function saveProfile(profile: CameraProfile) {
    if (!loaded.value || saving.value) return
    saving.value = true; error.value = null; notice.value = ''
    const result = await call<CameraProfile>('camera_save_profile', { profile })
    if (result.ok) {
      profiles.value = [...profiles.value.filter(value => value.entityId !== profile.entityId), result.data]
      notice.value = '摄像头配置已保存，直播兼容性仍需实机验证。'
    } else error.value = result.error
    saving.value = false
  }
  return { gateway, profiles, list, loaded, loading, saving, error, notice, aliases, load, saveGateway, saveProfile, probe }
})
