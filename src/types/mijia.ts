export interface MiDevice { did: string; name: string; model: string; home: string; room: string; online: boolean; camera: boolean }
export interface MiSnapshot { loggedIn: boolean; region: string; accountLabel: string | null; devices: MiDevice[]; warnings: string[]; updatedAt: number | null }
export interface QrLogin { id: string; image: string; expiresIn: number }
export interface MiProperty { siid: number; piid: number; name: string; format: string; readable: boolean; writable: boolean; range: number[] | null; choices: { value: number; description: string }[]; value: unknown; unavailable: boolean }
export const regions = [
  { label: '中国大陆', value: 'cn' }, { label: '欧洲', value: 'de' }, { label: '美国', value: 'us' },
  { label: '新加坡', value: 'sg' }, { label: '中国台湾', value: 'tw' }, { label: '俄罗斯', value: 'ru' },
  { label: '印度', value: 'in' }, { label: '印度（i2）', value: 'i2' },
]
