export interface CameraAudioPacket {
  codec: number
  sampleRate: number
  bytes: number[]
}

function decodeAlaw(byte: number) {
  const value = byte ^ 0x55
  const segment = (value >> 4) & 7
  const mantissa = (value & 15) << 4
  const magnitude = segment === 0 ? mantissa + 8 : (mantissa + 264) << (segment - 1)
  return (value & 128 ? magnitude : -magnitude) / 32768
}

function decodeMulaw(byte: number) {
  const value = byte ^ 255
  const magnitude = (((value & 15) * 8 + 132) << ((value >> 4) & 7)) - 132
  return (value & 128 ? -magnitude : magnitude) / 32768
}

export class CameraAudio {
  private decoder: AudioDecoder | null = null
  private nextTime = 0
  private timestamp = 0

  private output: GainNode

  constructor(private context: AudioContext, private onError: () => void) {
    this.output = context.createGain()
    this.output.connect(context.destination)
  }

  setMuted(muted: boolean) {
    this.output.gain.value = muted ? 0 : 1
  }

  play(packet: CameraAudioPacket) {
    const { codec, sampleRate, bytes } = packet
    if (!bytes.length) return
    if (codec === 1032) {
      if (!this.decoder) {
        this.decoder = new AudioDecoder({
          output: (data) => {
            try {
              const buffer = this.context.createBuffer(data.numberOfChannels, data.numberOfFrames, data.sampleRate)
              for (let i = 0; i < data.numberOfChannels; i++) {
                data.copyTo(buffer.getChannelData(i), { planeIndex: i, format: 'f32-planar' })
              }
              this.schedule(buffer)
            } finally {
              data.close()
            }
          },
          error: this.onError,
        })
        this.decoder.configure({ codec: 'opus', sampleRate: 48000, numberOfChannels: 2 })
      }
      this.decoder.decode(new EncodedAudioChunk({
        type: 'key', timestamp: this.timestamp++, data: Uint8Array.from(bytes),
      }))
      return
    }
    const frames = codec === 1024 ? bytes.length >> 1 : bytes.length
    if (!frames) return
    const buffer = this.context.createBuffer(1, frames, sampleRate)
    const output = buffer.getChannelData(0)
    for (let i = 0; i < frames; i++) {
      if (codec === 1024) {
        const pcm = bytes[i * 2]! | (bytes[i * 2 + 1]! << 8)
        output[i] = ((pcm << 16) >> 16) / 32768
      } else {
        output[i] = codec === 1027 ? decodeAlaw(bytes[i]!) : decodeMulaw(bytes[i]!)
      }
    }
    this.schedule(buffer)
  }

  private schedule(buffer: AudioBuffer) {
    if (this.context.state !== 'running') return
    const now = this.context.currentTime
    if (this.nextTime > now + 0.6) return
    if (this.nextTime < now) this.nextTime = now + 0.08
    const source = this.context.createBufferSource()
    source.buffer = buffer
    source.connect(this.output)
    source.onended = () => source.disconnect()
    source.start(this.nextTime)
    this.nextTime += buffer.duration
  }

  close() {
    if (this.decoder && this.decoder.state !== 'closed') this.decoder.close()
    void this.context.close().catch(() => {})
  }
}
