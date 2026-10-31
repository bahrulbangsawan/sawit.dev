import { useEffect, useRef, useState } from 'react'
import { DMG, INSTALL, REPO } from '../content'

// Single-letter shortcuts shown as <kbd> hints: C copies the install command, D downloads the DMG, G opens the repo.
// Ignored while typing or with a modifier held, so ⌘C and form fields keep working.
export function useShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || e.repeat) return
      const t = e.target as HTMLElement | null
      if (t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName))) return
      const key = e.key.toLowerCase()
      if (key === 'c') document.getElementById('install-copy')?.click()
      else if (key === 'd') window.location.href = DMG
      else if (key === 'g') window.location.href = REPO
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])
}

export function InstallButton({ id, className = 'btn k pri' }: { id?: string; className?: string }) {
  const [copied, setCopied] = useState(false)
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(INSTALL)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1800)
    } catch {
      setCopied(false)
    }
  }
  return (
    <>
      <button type="button" id={id} className={className} onClick={copy} aria-keyshortcuts={id ? 'c' : undefined}>
        <svg aria-hidden="true">
          <use href={copied ? '#i-check' : '#i-copy'} />
        </svg>
        {copied ? 'Copied, paste in Terminal' : 'Copy install command'}
        {id && <kbd aria-hidden="true">C</kbd>}
      </button>
      <span className="sr-only" role="status">
        {copied ? 'Install command copied' : ''}
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
