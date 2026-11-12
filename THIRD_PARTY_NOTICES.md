# Third-party components for CS2 remote playback

- **hls.js** — https://github.com/video-dev/hls.js — Apache-2.0. Full text: `licenses/hls.js-LICENSE.txt`. Runs locally in WebView2; no external CDN.
- **RustCrypto chacha20 / salsa20** — https://github.com/RustCrypto/stream-ciphers — MIT or Apache-2.0. MIT text: `licenses/RustCrypto-LICENSE-MIT.txt`. Cipher framing follows the Xiaomi MISS protocol; no proprietary SDK is included.
- **PPPP protocol documentation / dissector** — Magnus Ihse Bursie, https://github.com/magicus/pppp-dissector — MIT. Protocol reference; notice retained in `licenses/pppp-dissector-LICENSE.txt`.
- **FFmpeg 8.1.3** — https://ffmpeg.org — the distributed minimal configuration is LGPL-2.1-or-later, with shared libraries, without GPL/nonfree codec libraries. Invoked as a separate process. Its complete unmodified source archive, license, exact configure flags, checksums and rebuild script are included in `ffmpeg/`. Replacement/rebuilding of its DLLs, and reverse engineering to debug modifications to these LGPL components, are permitted. Windows H.264 encoding uses Media Foundation. See `ffmpeg/NOTICE.txt` and `ffmpeg/BUILD_INFO.json`.

The Rust CS2 transport is independently written from documented wire behavior. The unlicensed micam project is cited as a protocol research reference; its Java code, tests and compiled artifacts are not copied or distributed. No Xiaomi/MiLoCo native library, CS2 vendor SDK, extracted application secret, or hardcoded camera/rendezvous account is bundled.

Other existing dependencies retain their upstream licenses. This file records the newly introduced playback components and accompanies the installer and portable directory.
