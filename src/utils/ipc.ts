import { invoke, isTauri } from '@tauri-apps/api/core'
import type { ApiResult } from '../types/settings'

export async function call<T>(command: string, args?: Record<string, unknown>): Promise<ApiResult<T>> {
  if (!isTauri()) {
    return { ok: false, error: { code: 'DESKTOP_REQUIRED', message: '本地配置需要在桌面客户端中读取和保存。当前为浏览器预览。', retryable: false } }
  }
  try {
    return await invoke<ApiResult<T>>(command, args)
  } catch {
    // Never display raw transport errors, which may include internal data.
    return { ok: false, error: { code: 'IPC_ERROR', message: '无法与桌面服务通信，请重新启动客户端。', retryable: true } }
  }
}
