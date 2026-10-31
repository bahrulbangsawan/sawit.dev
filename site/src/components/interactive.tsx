import { useEffect, useRef, useState } from 'react'
import { INSTALL } from '../content'

// The element holding the install command on the page, which a copy the browser refuses selects.
export const COMMAND_ID = 'install-command'

// No single-letter shortcuts: C, D and G used to copy the command, download the DMG and open GitHub
// from anywhere on the page, which a stray key press or speech input set off and nothing could turn
// off (WCAG 2.1.4).
export function InstallButton({ id, className = 'btn pri' }: { id?: string; className?: string }) {
  // What the last click did: copied the command, or, when the browser refused to write the
  // clipboard (no permission, no focus, a page not served over HTTPS), selected it for ⌘C.
  const [done, setDone] = useState<'copied' | 'selected' | null>(null)
  const timer = useRef(0)
  const show = (what: 'copied' | 'selected') => {
    setDone(what)
    window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => setDone(null), what === 'copied' ? 1800 : 6000)
  }
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(INSTALL)
      show('copied')
    } catch {
      const command = document.getElementById(COMMAND_ID)
      const selection = window.getSelection()
      if (command && selection) {
        const range = document.createRange()
        range.selectNodeContents(command)
        selection.removeAllRanges()
        selection.addRange(range)
      }
      show('selected')
    }
  }
  return (
    <>
      <button type="button" id={id} className={className} onClick={copy}>
        <svg aria-hidden="true">
          <use href={done === 'copied' ? '#i-check' : '#i-copy'} />
        </svg>
        {done === 'copied' ? 'Copied, paste in Terminal' : done === 'selected' ? 'Selected, press ⌘C to copy' : 'Copy install command'}
      </button>
      <span className="sr-only" role="status">
        {done === 'copied' ? 'Install command copied' : done === 'selected' ? 'Install command selected. Press Command C to copy it.' : ''}
      </span>
    </>
  )
}

export function InstanceSwitch() {
  const [running, setRunning] = useState(true)
  return (
    <div className="inst">
      <div className="inst-row">
        <span className={`dot ${running ? 'on' : ''}`} aria-hidden="true" />
        <b>shop</b>
        <span className="muted">PostgreSQL 16 · :55432</span>
        <span className="inst-state">{running ? 'Running' : 'Stopped'}</span>
      </div>
      <fieldset className="seg">
        <legend className="sr-only">Instance shop</legend>
        <label>
          <input type="radio" name="instance" checked={running} onChange={() => setRunning(true)} />
          <span>Start</span>
        </label>
        <label>
          <input type="radio" name="instance" checked={!running} onChange={() => setRunning(false)} />
          <span>Stop</span>
        </label>
      </fieldset>
      <p className="inst-log" aria-live="polite">
        <code>
          {running ? 'LOG:  database system is ready to accept connections' : 'LOG:  database system is shut down'}
        </code>
      </p>
    </div>
  )
}

// The demo plays silently on a loop with no browser controls or context menu. A reader who asked for
// reduced motion gets the poster until they press play, and the corner chip pauses it for everyone.
// It starts from here, once React runs, not by autoplay: a video the browser started before then
// played for readers who asked for reduced motion, and the chip, which only follows play and pause
// events seen after hydration, offered Play on a playing video.
export function DemoVideo() {
  const ref = useRef<HTMLVideoElement>(null)
  const [playing, setPlaying] = useState(false)
  useEffect(() => {
    const v = ref.current
    if (!v) return
    if (!matchMedia('(prefers-reduced-motion: reduce)').matches) v.play().catch(() => {})
    setPlaying(!v.paused)
  }, [])
  const toggle = () => {
    const v = ref.current
    if (!v) return
    if (v.paused) v.play().catch(() => {})
    else v.pause()
  }
  return (
    <>
      <video
        ref={ref}
        src="/media/demo.mp4"
        poster="/media/demo-poster.jpg"
        width={1280}
        height={802}
        muted
        loop
        playsInline
        disablePictureInPicture
        disableRemotePlayback
        controlsList="nodownload nofullscreen noremoteplayback"
        // Only its size and first frame until it plays: none of it for a reader who never does.
        preload="metadata"
        onContextMenu={(e) => e.preventDefault()}
        onPlay={() => setPlaying(true)}
        onPause={() => setPlaying(false)}
        aria-label="Sawit demo: welcome screen, instances, a table, SQL, a schema diagram, code with a terminal, ports, git, dev tools, MCP and the canvas"
      />
      <button type="button" className="vid-toggle" onClick={toggle} aria-label={playing ? 'Pause the demo' : 'Play the demo'}>
        <svg aria-hidden="true">
          <use href={playing ? '#i-pause' : '#i-play'} />
        </svg>
      </button>
    </>
  )
}
