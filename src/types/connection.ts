import type { AppError } from './settings'
export interface ConnectionSummary { id: string; name: string; baseUrl: string }
export interface ConnectionStatus {
  state: 'disconnected' | 'connecting' | 'authenticating' | 'connected' | 'reconnecting' | 'error'
  attempt: number
  error: AppError | null
}
export interface EntitySummary { entityId: string; name: string; domain: string; state: string; unit: string | null; lastUpdated: string; attributes: Record<string, unknown> }
export const stateLabels: Record<ConnectionStatus['state'], string> = {
  disconnected: '未连接', connecting: '正在连接', authenticating: '正在认证', connected: '已连接', reconnecting: '正在重连', error: '连接错误',
}

export interface DeviceCatalog {
  areas: { areaId: string; name: string }[]
  services: Record<string, string[]>
  entityAreas: Record<string, string | null>
  warnings: string[]
  servicesLoaded: boolean
}
