<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref, watch } from 'vue'
import { NAlert, NButton, NEmpty, NSelect } from 'naive-ui'
import { useCamerasStore } from '../stores/cameras'
import CameraPlayer from '../components/CameraPlayer.vue'
const props = withDefaults(defineProps<{ active?: boolean }>(), { active:true })
const documentVisible = ref(!document.hidden)
const updateVisibility = () => { documentVisible.value = !document.hidden }
onMounted(() => document.addEventListener('visibilitychange',updateVisibility))
onBeforeUnmount(() => document.removeEventListener('visibilitychange',updateVisibility))
const cameras = useCamerasStore()
const layout = ref<1 | 4>(1)
const assignments = ref<(string | null)[]>([null,null,null,null])
const visible = computed(() => assignments.value.slice(0,layout.value).map((id,index) => ({ id, index, camera: cameras.list.find(camera => camera.entityId === id) })))
let initialized = false
watch(() => [cameras.loaded, cameras.list.length], () => {
  if (!initialized && cameras.loaded && cameras.list.length) { assignments.value = [0,1,2,3].map(index => cameras.list.filter(camera => camera.profile.enabled)[index]?.entityId ?? null); initialized = true }
}, { immediate:true })
onMounted(() => { void cameras.load() })
function selectCamera(index: number, id: string | null) {
  const previous = assignments.value[index] ?? null
  const other = id ? assignments.value.findIndex((value, slot) => slot !== index && value === id) : -1
  if (other >= 0) assignments.value[other] = previous
  assignments.value[index] = id
}
function options(index: number) { return cameras.list.map(camera => ({ label: camera.name, value: camera.entityId, disabled: !camera.profile.enabled || assignments.value.slice(0,layout.value).some((id,slot) => slot !== index && id === camera.entityId) })) }
</script>
<template>
  <div class="page-heading"><div><div class="eyebrow">CAMERA MONITOR</div><h1>监控中心</h1><p>{{ cameras.list.length }} 个摄像头实体 · 视频兼容性需实机验证</p></div><div class="monitor-layout"><NButton :type="layout === 1 ? 'primary' : 'default'" @click="layout = 1">单画面</NButton><NButton :type="layout === 4 ? 'primary' : 'default'" @click="layout = 4">四宫格</NButton></div></div>
  <NAlert v-if="cameras.error" type="error" class="monitor-alert">{{ cameras.error.message }}<NButton size="small" :disabled="cameras.loading" @click="cameras.load">重新读取配置</NButton></NAlert>
  <NAlert type="info" class="monitor-alert">HA 截图不是实时视频。直播需先在设置中配置 go2rtc 和流别名；“正在播放”仅在收到已解码视频帧后显示。</NAlert>
  <NEmpty v-if="!cameras.list.length" description="尚未发现摄像头，请先连接 Home Assistant 并同步实体" style="padding:60px" />
  <div v-else class="monitor-grid" :class="{ quad: layout === 4 }">
    <section v-for="slot in visible" :key="slot.id ?? `empty-${slot.index}`" class="monitor-slot">
      <NSelect :value="slot.id" clearable :options="options(slot.index)" placeholder="选择摄像头" @update:value="selectCamera(slot.index, $event)" />
      <CameraPlayer v-if="slot.camera && props.active && documentVisible" :camera="slot.camera" />
      <div v-else class="monitor-empty">{{ slot.camera ? '窗口隐藏，播放已暂停' : '请选择摄像头' }}</div>
    </section>
  </div>
</template>
<style scoped>
.monitor-layout{display:flex;gap:10px}.monitor-alert{margin-bottom:18px}.monitor-grid{display:grid;gap:18px;grid-template-columns:minmax(0,1fr)}.monitor-grid.quad{grid-template-columns:repeat(2,minmax(0,1fr))}.monitor-slot{min-width:0;display:flex;flex-direction:column;gap:10px}.monitor-empty{aspect-ratio:16/9;border:1px dashed #8792a5;border-radius:12px;display:grid;place-items:center;color:#8792a5}@media(max-width:1000px){.monitor-grid.quad{grid-template-columns:minmax(0,1fr)}}
</style>
