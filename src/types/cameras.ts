export interface CameraGateway { baseUrl: string; enabled: boolean }
export interface CameraProfile { entityId: string; sourceType: 'snapshot' | 'go2rtc'; streamName: string | null; enabled: boolean }
export interface CameraConfiguration { gateway: CameraGateway; profiles: CameraProfile[] }
export interface CameraSession { sessionId: string; entityId: string }
export interface CameraView {
  entityId: string
  name: string
  haState: string | null
  haStreamDeclared: boolean
  profile: CameraProfile
}
