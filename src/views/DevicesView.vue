<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { NAlert, NButton, NDrawer, NDrawerContent, NEmpty, NFormItem, NInput, NInputNumber, NSelect, NSwitch, NTag } from 'naive-ui'
import { useConnectionStore } from '../stores/connection'
import { useDevicesStore } from '../stores/devices'
import EntityControl from '../components/EntityControl.vue'
const connection = useConnectionStore()
const devices = useDevicesStore()
onMounted(() => { if (!devices.loaded) void devices.load() })
const search = ref('')
const area = ref<string | null>(null)
const domain = ref<string | null>(null)
const availability = ref<string | null>(null)
const onlyFavorites = ref(false)
const showHidden = ref(false)
const sort = ref('custom')
const groupRooms = ref(false)
const selectedId = ref<string | null>(null)
const detail = computed(() => devices.entities.find(entity => entity.entityId === selectedId.value))
const drawerOpen = computed({ get: () => selectedId.value !== null, set: value => { if (!value) selectedId.value = null } })
const local = reactive({ displayName: '', areaIdOverride: null as string | null, sortOrder: 0 })
const preferenceSaved = ref(false)
watch(selectedId, () => {
  const preference = detail.value?.preference
  local.displayName = preference?.displayName ?? ''; local.areaIdOverride = preference?.areaIdOverride ?? null; local.sortOrder = preference?.sortOrder ?? 0
  preferenceSaved.value = false
})
watch(local, () => { preferenceSaved.value = false })
const roomOptions = computed(() => [{label:'未分配房间',value:'__unassigned__'}, ...connection.catalog.areas.map(value => ({label:value.name,value:value.areaId}))])
const overrideOptions = computed(() => {
  const options = connection.catalog.areas.map(value => ({label:value.name,value:value.areaId}))
  if (local.areaIdOverride && !options.some(value => value.value === local.areaIdOverride)) options.push({ label:'已保存房间（当前不可用）', value:local.areaIdOverride })
  return options
})
const domainOptions = computed(() => [...new Set(devices.entities.map(entity => entity.domain))].sort().map(value => ({ label:value, value })))
const sortOptions = [{label:'本地排序',value:'custom'},{label:'名称',value:'name'},{label:'最近更新',value:'updated'}]
const availabilityOptions = [{label:'可用',value:'available'},{label:'离线 / 状态未知',value:'unavailable'}]
const filtered = computed(() => devices.entities.filter(entity => {
  const unavailable = ['unavailable','unknown'].includes(entity.state)
  return (showHidden.value || !entity.preference.isHidden) && (!onlyFavorites.value || entity.preference.isFavorite)
    && (!domain.value || domain.value === entity.domain)
    && (!area.value || (area.value === '__unassigned__' ? !entity.areaId : entity.areaId === area.value))
    && (!availability.value || (availability.value === 'unavailable' ? unavailable : !unavailable))
    && `${entity.displayName} ${entity.name} ${entity.entityId}`.toLowerCase().includes(search.value.trim().toLowerCase())
}).sort((a,b) => sort.value === 'updated' ? Date.parse(b.lastUpdated)-Date.parse(a.lastUpdated) : (sort.value === 'custom' ? a.preference.sortOrder-b.preference.sortOrder : 0) || a.displayName.localeCompare(b.displayName) || a.entityId.localeCompare(b.entityId)))
const groups = computed(() => {
  if (!groupRooms.value) return [{ key:'all',name:'全部实体',entities:filtered.value }]
  const map = new Map<string, {key:string;name:string;entities:typeof filtered.value}>()
  for (const entity of filtered.value) { const key = entity.areaId ?? '__unassigned__'; if (!map.has(key)) map.set(key,{key,name:entity.areaName,entities:[]}); map.get(key)!.entities.push(entity) }
  return [...map.values()]
})
async function saveLocal() {
  if (!detail.value) return
  preferenceSaved.value = await devices.update({ ...devices.preferenceFor(detail.value.entityId), displayName:local.displayName || null, areaIdOverride:local.areaIdOverride, sortOrder:local.sortOrder })
}
function toggleFavorite(id: string) { const preference = devices.preferenceFor(id); void devices.update({...preference,isFavorite:!preference.isFavorite}) }
function toggleHidden(id: string) { const preference = devices.preferenceFor(id); void devices.update({...preference,isHidden:!preference.isHidden}) }
function clearFilters() { search.value='';area.value=null;domain.value=null;availability.value=null;onlyFavorites.value=false;showHidden.value=false }
</script>
<template>
  <div class="page-heading"><div><div class="eyebrow">DEVICES & ENTITIES</div><h1>设备中心</h1><p>已同步 {{ devices.entities.length }} 个实体；一个物理设备可能对应多个实体。</p></div><NButton :disabled="!connection.saved || connection.busy" @click="connection.connect">重新同步</NButton></div>
  <NAlert v-if="connection.status.state !== 'connected'" type="warning" class="device-alert">{{ connection.label }}。列表为最近一次同步结果，控制已禁用。</NAlert>
  <NAlert v-if="connection.catalog.warnings.length" type="info" class="device-alert">部分房间、注册表或服务信息不可用；基本实体列表仍可查看。需要时检查 HA 权限并重新同步。</NAlert>
  <NAlert v-if="devices.error" type="error" class="device-alert">{{ devices.error.message }}<NButton v-if="!devices.loaded" size="small" :disabled="devices.loading" @click="devices.load">重试加载本地偏好</NButton></NAlert>
  <div class="device-filters panel">
    <NInput v-model:value="search" clearable placeholder="搜索名称或实体 ID" :input-props="{ 'aria-label': '搜索实体' }" />
    <NSelect v-model:value="area" clearable :options="roomOptions" placeholder="全部房间" />
    <NSelect v-model:value="domain" clearable :options="domainOptions" placeholder="全部类别" />
    <NSelect v-model:value="availability" clearable :options="availabilityOptions" placeholder="全部状态" />
    <NSelect v-model:value="sort" :options="sortOptions" />
    <div class="filter-toggles"><label><NSwitch aria-label="仅收藏" v-model:value="onlyFavorites" size="small" /> 仅收藏</label><label><NSwitch aria-label="显示隐藏" v-model:value="showHidden" size="small" /> 显示隐藏</label><label><NSwitch aria-label="按房间分组" v-model:value="groupRooms" size="small" /> 按房间分组</label><NButton text @click="clearFilters">清除筛选</NButton></div>
  </div>
  <NEmpty v-if="!filtered.length" :description="devices.entities.length ? '没有匹配的实体，可清除筛选或显示隐藏项' : '尚无实体，请先连接 Home Assistant'" style="padding:60px" />
  <section v-for="group in groups" :key="group.key" class="entity-group">
    <h2 v-if="groupRooms && group.entities.length">{{ group.name }} <small>{{ group.entities.length }}</small></h2>
    <div class="entity-grid"><article v-for="entity in group.entities" :key="entity.entityId" class="panel entity-card" :data-entity-id="entity.entityId">
      <div class="entity-heading"><strong>{{ entity.displayName }}</strong><NTag size="small" :bordered="false">{{ entity.domain }}</NTag></div>
      <p class="entity-id">{{ entity.entityId }}</p><p class="entity-room">{{ entity.areaName }} <span v-if="entity.preference.isHidden">· 已隐藏</span></p>
      <p class="entity-state">{{ entity.state }} <small>{{ entity.unit }}</small></p>
      <EntityControl :entity="entity" compact />
      <div class="entity-actions"><NButton size="small" :disabled="!devices.loaded || devices.saving[entity.entityId]" @click="toggleFavorite(entity.entityId)">{{ entity.preference.isFavorite ? '取消收藏' : '收藏' }}</NButton><NButton size="small" @click="selectedId = entity.entityId">详情</NButton></div>
      <small class="entity-time">更新于 {{ new Date(entity.lastUpdated).toLocaleString() }}</small>
    </article></div>
  </section>
  <NDrawer v-model:show="drawerOpen" :width="600"><NDrawerContent :title="detail?.displayName ?? '实体已移除'" closable>
    <template v-if="detail">
      <dl class="entity-details"><dt>实体 ID</dt><dd>{{ detail.entityId }}</dd><dt>HA 名称</dt><dd>{{ detail.name }}</dd><dt>房间</dt><dd>{{ detail.areaName }}</dd><dt>类别</dt><dd>{{ detail.domain }}</dd><dt>当前状态</dt><dd>{{ detail.state }} {{ detail.unit }}</dd><dt>更新时间</dt><dd>{{ new Date(detail.lastUpdated).toLocaleString() }}</dd></dl>
      <h3>设备操作</h3><EntityControl :entity="detail" />
      <h3>本地显示偏好</h3><p class="entity-id">仅调整本客户端的显示，不修改 Home Assistant。</p>
      <NFormItem label="自定义名称（留空使用 HA 名称）"><NInput :input-props="{ 'aria-label': '自定义显示名称' }" v-model:value="local.displayName" :maxlength="80" :disabled="!devices.loaded || devices.saving[detail.entityId]" /></NFormItem>
      <NFormItem label="房间覆盖（清空跟随 HA）"><NSelect v-model:value="local.areaIdOverride" clearable :options="overrideOptions" :disabled="!devices.loaded || devices.saving[detail.entityId]" /></NFormItem>
      <NFormItem label="排序（较小值优先）"><NInputNumber :value="local.sortOrder" :min="0" :max="10000" :precision="0" :disabled="!devices.loaded || devices.saving[detail.entityId]" @update:value="local.sortOrder = $event ?? 0" /></NFormItem>
      <div class="entity-actions"><NButton type="primary" :disabled="!devices.loaded" :loading="devices.saving[detail.entityId]" @click="saveLocal">保存显示偏好</NButton><NButton :disabled="!devices.loaded || devices.saving[detail.entityId]" @click="toggleHidden(detail.entityId)">{{ detail.preference.isHidden ? '取消隐藏' : '隐藏实体' }}</NButton></div>
      <p v-if="preferenceSaved" role="status">本地显示偏好已保存。</p>
      <h3>关键属性</h3><pre class="entity-attributes">{{ JSON.stringify(detail.attributes, null, 2) }}</pre>
    </template><NEmpty v-else description="实体已不在当前同步结果中" />
  </NDrawerContent></NDrawer>
</template>
<style scoped>
.device-alert{margin-bottom:16px}.device-filters{display:grid;grid-template-columns:2fr repeat(4,minmax(110px,1fr));gap:12px;margin-bottom:22px}.filter-toggles{grid-column:1/-1;display:flex;align-items:center;gap:20px;flex-wrap:wrap}.filter-toggles label{display:flex;gap:8px;align-items:center;font-size:12px}.entity-group h2{font-size:16px}.entity-group h2 small{color:#8792a5}.entity-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(260px,1fr));gap:16px}.entity-heading{display:flex;justify-content:space-between;align-items:center;gap:12px}.entity-heading strong{overflow-wrap:anywhere}.entity-id,.entity-room{font-size:11px;color:#8792a5;overflow-wrap:anywhere}.entity-state{font-size:22px;margin:12px 0}.entity-state small{font-size:12px}.entity-time{color:#8792a5;font-size:10px;display:block;margin-top:12px}.entity-actions{display:flex;gap:10px;margin:12px 0}.entity-details{display:grid;grid-template-columns:90px 1fr;gap:12px;font-size:12px}.entity-details dt{color:#8792a5}.entity-details dd{margin:0;overflow-wrap:anywhere}.entity-attributes{background:#8881;border-radius:8px;padding:14px;white-space:pre-wrap;overflow-wrap:anywhere;font-size:12px}@media(max-width:1100px){.device-filters{grid-template-columns:repeat(2,minmax(0,1fr))}}
</style>
