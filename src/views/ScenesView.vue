<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { NAlert, NButton, NEmpty, NInput, NSwitch, NTag } from 'naive-ui'
import { useScenesStore } from '../stores/scenes'
import { useConnectionStore } from '../stores/connection'
const scenes=useScenesStore()
const connection=useConnectionStore()
const search=ref('')
const onlyFavorites=ref(false)
onMounted(()=>{void scenes.load()})
const filtered=computed(()=>scenes.list.filter(scene=>(!onlyFavorites.value || scene.favorite) && `${scene.name} ${scene.entityId}`.toLowerCase().includes(search.value.trim().toLowerCase())).sort((a,b)=>Number(b.favorite)-Number(a.favorite) || a.name.localeCompare(b.name)))
function activated(state: string) {const date=new Date(state);return Number.isNaN(date.getTime())?'尚未提供激活时间':date.toLocaleString()}
</script>
<template>
  <div class="page-heading"><div><div class="eyebrow">HOME ASSISTANT SCENES</div><h1>智能场景</h1><p>复用 Home Assistant 的 {{ scenes.list.length }} 个场景。</p></div><NButton :disabled="!connection.saved || connection.busy" @click="connection.connect">重新同步</NButton></div>
  <NAlert v-if="connection.status.state!=='connected'" type="warning" class="scene-alert">{{ connection.label }}。恢复连接后才能执行场景。</NAlert>
  <NAlert v-else-if="!connection.catalog.servicesLoaded || !connection.catalog.services.scene?.includes('turn_on')" type="info" class="scene-alert">尚未获取 scene.turn_on 服务，请检查 HA 服务信息或重新同步。</NAlert>
  <NAlert v-if="scenes.error" type="error" class="scene-alert">{{ scenes.error.message }}<NButton v-if="!scenes.loaded" size="small" :disabled="scenes.loading" @click="scenes.load">重试读取收藏</NButton></NAlert>
  <div class="scene-filters"><NInput v-model:value="search" clearable placeholder="搜索场景名称或 ID" :input-props="{ 'aria-label':'搜索场景' }" /><label><NSwitch v-model:value="onlyFavorites" aria-label="仅收藏场景" /> 仅收藏</label></div>
  <NEmpty v-if="!filtered.length" :description="scenes.list.length?'没有匹配的场景':'尚无 HA 场景，请先连接并在 Home Assistant 配置场景'" style="padding:60px" />
  <div class="scene-grid"><article v-for="scene in filtered" :key="scene.entityId" class="panel" :data-scene-id="scene.entityId">
    <div class="scene-heading"><h2>{{ scene.name }}</h2><NTag size="small" :type="scene.state==='unavailable'?'warning':'default'">{{ scene.state==='unavailable'?'不可用':'HA 场景' }}</NTag></div><p class="scene-id">{{ scene.entityId }}</p><p class="scene-id">最近激活：{{ activated(scene.state) }}</p>
    <div class="scene-actions"><NButton :disabled="!scenes.loaded || scenes.saving[scene.entityId]" @click="scenes.favorite(scene.entityId,!scene.favorite)">{{ scene.favorite?'取消收藏':'收藏' }}</NButton><NButton type="primary" :loading="scenes.pending[scene.entityId]" :disabled="!scenes.canExecute(scene.entityId)" @click="scenes.execute(scene.entityId)">执行场景</NButton></div>
    <p v-if="scenes.feedback[scene.entityId]" role="status" :class="['scene-feedback',{failed:!scenes.feedback[scene.entityId].success}]">{{ scenes.feedback[scene.entityId].message }}</p>
  </article></div>
</template>
<style scoped>
.scene-alert{margin-bottom:18px}.scene-filters{display:flex;align-items:center;gap:22px;margin:20px 0}.scene-filters>.n-input{max-width:430px}.scene-filters label{white-space:nowrap;display:flex;align-items:center;gap:8px}.scene-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(280px,1fr));gap:16px}.scene-heading{display:flex;justify-content:space-between;align-items:center;gap:12px}.scene-heading h2{margin:0;font-size:16px;overflow-wrap:anywhere}.scene-id{font-size:11px;color:#8792a5;overflow-wrap:anywhere}.scene-actions{display:flex;gap:10px;justify-content:flex-end;margin-top:20px}.scene-feedback{font-size:11px;line-height:1.8;color:#22a06b}.scene-feedback.failed{color:#e05252}
</style>
