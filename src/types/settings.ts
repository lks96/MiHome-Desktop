export interface AppSettings {
  theme: 'light' | 'dark' | 'system'
  requestTimeout: number
  autoConnect: boolean
  autoReconnect: boolean
  closeToTray: boolean
  autoStart: boolean
}
export interface AppError {
  code: string
  message: string
  retryable: boolean
}
export type ApiResult<T> = { ok: true; data: T } | { ok: false; error: AppError }
export interface AppInfo { name: string; version: string; platform: string }
