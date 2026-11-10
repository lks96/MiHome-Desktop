export interface EntityPreference {
  entityId: string
  displayName: string | null
  areaIdOverride: string | null
  isFavorite: boolean
  isHidden: boolean
  sortOrder: number
}
export interface ServiceResult { accepted: boolean; message: string }
