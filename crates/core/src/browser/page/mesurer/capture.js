  // ---- Capture (lane 1: mesurer-capture) ----
  //
  // Screenshots: `captureVisibleTab` is a WKWebView snapshot at the display's scale. Recordings:
  // this world's `getDisplayMedia` is a canvas stream fed by the app's JPEG frames, pulled one at
  // a time and drawn through `createImageBitmap` (no data: URL a CSP could block). WebKit's WebM
  // can't be seeked, so MediaRecorder has none and records MP4; VideoEncoder gets an H.264 level
  // that fits the frame (Rust: `mesurer::files::avc_codec_for`).
  PROPS.captureVisibleTab = async () => {
    const shot = await bridge("capture.visible");
    return new Blob([base64ToBytes(shot.png_base64)], { type: "image/png" });
  };

  const FRAME_MS = 1000 / 30;
  // `took` adds up the app's time a frame (snapshot, encoding) and its size.
  const frameBitmap = async (maxWidth, took) => {
    const frame = await bridge("capture.frame", { max_width: maxWidth, quality: 0.8 });
    took[0] += frame.snap_ms || 0;
    took[1] += frame.encode_ms || 0;
    took[2] += frame.jpeg_base64.length * 0.75;
    return createImageBitmap(new Blob([base64ToBytes(frame.jpeg_base64)], { type: "image/jpeg" }));
  };
  const getDisplayMedia = async function getDisplayMedia() {
    const maxWidth = Math.min(1920, Math.round(innerWidth * (devicePixelRatio || 1)));
    const took = [0, 0, 0];
    const first = await frameBitmap(maxWidth, took);
    const canvas = document.createElement("canvas");
    canvas.width = first.width;
    canvas.height = first.height;
    const context = canvas.getContext("2d", { alpha: false });
    context.drawImage(first, 0, 0);
    first.close();
    const stream = canvas.captureStream(30);
    const track = stream.getVideoTracks()[0];
    const settings = track.getSettings.bind(track);
    const stop = track.stop.bind(track);
    const since = performance.now();
    let live = true;
    let frames = 1;
    let failures = 0;
    const end = () => {
      if (!live) return;
      live = false;
      const s = (performance.now() - since) / 1000;
      const each = (v) => (v / frames).toFixed(1);
      log("info", `mesurer recording: ${frames} frames in ${s.toFixed(1)} s, ${(frames / Math.max(s, 0.001)).toFixed(1)} fps, ${canvas.width}x${canvas.height}; a frame: ${each(took[0])} ms snapshot, ${each(took[1])} ms encoding, ${each(took[2] / 1024)} KiB`);
    };
    Object.defineProperties(track, {
      getSettings: method(() => Object.assign(settings(), { displaySurface: "browser", width: canvas.width, height: canvas.height })),
      stop: method(() => {
        end();
        stop();
      }),
    });
    (async () => {
      while (live) {
        const at = performance.now();
        try {
          const bitmap = await frameBitmap(maxWidth, took);
          if (live) context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
          bitmap.close();
          frames++;
          failures = 0;
        } catch (e) {
          // The page went, or the app can't take it: the recording ends as a stopped share does.
          if (++failures >= 5 && live) {
            log("warn", "mesurer's recording lost its frames:", e);
            end();
            stop();
            track.dispatchEvent(new Event("ended"));
          }
        }
        const rest = FRAME_MS - (performance.now() - at);
        if (live && rest > 0) await new Promise((r) => setTimeout(r, rest));
      }
    })();
    return stream;
  };
  if (typeof MediaDevices === "function" && MediaDevices.prototype) {
    Object.defineProperty(MediaDevices.prototype, "getDisplayMedia", method(getDisplayMedia));
  }
  if (!navigator.mediaDevices && typeof Navigator === "function") {
    const devices = Object.freeze({ getDisplayMedia });
    Object.defineProperty(Navigator.prototype, "mediaDevices", { get: () => devices, configurable: true });
  }

  if (typeof MediaRecorder === "function" && typeof MediaRecorder.isTypeSupported === "function") {
    const supported = MediaRecorder.isTypeSupported.bind(MediaRecorder);
    Object.defineProperty(MediaRecorder, "isTypeSupported", method((type) => !/webm/i.test(String(type)) && supported(type)));
  }
  // H.264's levels (Table A-1): level_idc, the most macroblocks in a frame and in a second.
  const AVC_LEVELS = [[10, 99, 1485], [11, 396, 3000], [12, 396, 6000], [13, 396, 11880], [20, 396, 11880],
    [21, 792, 19800], [22, 1620, 20250], [30, 1620, 40500], [31, 3600, 108000], [32, 5120, 216000],
    [40, 8192, 245760], [41, 8192, 245760], [42, 8704, 522240], [50, 22080, 589824], [51, 36864, 983040],
    [52, 36864, 2073600], [60, 139264, 4177920], [61, 139264, 8355840], [62, 139264, 16711680]];
  const avcFits = (level, width, height, fps) => {
    const row = AVC_LEVELS.find((l) => l[0] === level);
    if (!row) return false;
    const w = Math.ceil(width / 16);
    const h = Math.ceil(height / 16);
    const side = Math.sqrt(row[1] * 8);
    return w * h <= row[1] && w <= side && h <= side && w * h * (fps > 0 ? fps : 30) <= row[2];
  };
  // `codec` itself when it isn't AVC or fits; else its profile at the lowest level that fits.
  const avcCodecFor = (codec, width, height, fps) => {
    const m = /^avc1\.([0-9a-f]{4})([0-9a-f]{2})$/i.exec(String(codec));
    if (!m) return codec;
    const level = parseInt(m[2], 16);
    if (avcFits(level, width, height, fps)) return codec;
    const up = AVC_LEVELS.find((l) => l[0] > level && avcFits(l[0], width, height, fps));
    return up ? `avc1.${m[1]}${up[0].toString(16).padStart(2, "0")}` : null;
  };
  if (typeof VideoEncoder === "function" && typeof VideoEncoder.isConfigSupported === "function") {
    const ask = VideoEncoder.isConfigSupported.bind(VideoEncoder);
    Object.defineProperty(VideoEncoder, "isConfigSupported", method(async (config) => {
      if (!config) return ask(config);
      const codec = avcCodecFor(config.codec, config.width, config.height, config.framerate);
      if (codec === null) return { supported: false, config };
      return ask(codec === config.codec ? config : Object.assign({}, config, { codec }));
    }));
  }
