function encodeAlaw(sample) {
  const pcm = Math.max(-32768, Math.min(32767, Math.round(sample * 32768)))
  const magnitude = pcm >= 0 ? pcm : -pcm - 1
  const segment = Math.max(0, 24 - Math.clz32(magnitude))
  const shift = segment === 0 ? 4 : segment + 3
  return ((segment << 4) | ((magnitude >> shift) & 15)) ^ (pcm >= 0 ? 0xd5 : 0x55)
}

class CameraMicrophone extends AudioWorkletProcessor {
  phase = 0
  sum = 0
  count = 0
  frame = new Uint8Array(320)
  offset = 0

  process(inputs) {
    const channel = inputs[0]?.[0]
    if (!channel) return true
    for (const sample of channel) {
      this.sum += sample
      this.count++
      this.phase += 8000
      if (this.phase < sampleRate) continue
      this.phase -= sampleRate
      this.frame[this.offset++] = encodeAlaw(this.sum / this.count)
      this.sum = 0
      this.count = 0
      if (this.offset === this.frame.length) {
        this.port.postMessage(this.frame, [this.frame.buffer])
        this.frame = new Uint8Array(320)
        this.offset = 0
      }
    }
    return true
  }
}

registerProcessor('camera-microphone', CameraMicrophone)
