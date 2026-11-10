import type { EntitySummary } from '../types/connection'
export function numberAttribute(entity: EntitySummary, key: string): number | null {
  const value = entity.attributes[key]
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}
export function modes(entity: EntitySummary, key: string): string[] {
  const value = entity.attributes[key]
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []
}
export function feature(entity: EntitySummary, flag: number) { return ((numberAttribute(entity, 'supported_features') ?? 0) & flag) !== 0 }
export function brightnessSupported(entity: EntitySummary) {
  return 'brightness' in entity.attributes && modes(entity, 'supported_color_modes').some(mode => ['brightness','color_temp','hs','xy','rgb','rgbw','rgbww','white'].includes(mode))
}
export function colorSupported(entity: EntitySummary) { return modes(entity, 'supported_color_modes').some(mode => ['hs','xy','rgb','rgbw','rgbww'].includes(mode)) }
