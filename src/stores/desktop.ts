import { ref } from 'vue'
import { defineStore } from 'pinia'
import { isTauri } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { call } from '../utils/ipc'
import type { AppError } from '../types/settings'
export interface DesktopStatus { trayAvailable: boolean; autoStartSupported: boolean; autoStartEnabled: boolean | null; windowStateEnabled: boolean }
export const useDesktopStore=defineStore('desktop',()=>{
  const status=ref<DesktopStatus | null>(null)
  const visible=ref(true)
  const error=ref<AppError | null>(null)
  let unlisten:UnlistenFn | null=null
  let disposed=false
  async function refresh() {
    if (!isTauri()) return
    const result=await call<DesktopStatus>('desktop_get_status')
    if (result.ok) {status.value=result.data;error.value=null} else error.value=result.error
  }
  async function initialize() {
    if (!isTauri()) return
    try {const stop=await listen<boolean>('app:window_visible',event=>{visible.value=event.payload});if(disposed)stop();else unlisten=stop}
    catch {error.value={code:'EVENTS_UNAVAILABLE',message:'无法订阅窗口可见状态，请重启客户端。',retryable:false}}
    await refresh()
  }
  function dispose() {disposed=true;unlisten?.();unlisten=null}
  return {status,visible,error,refresh,initialize,dispose}
})
