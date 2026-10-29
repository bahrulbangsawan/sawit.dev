// Sawit's brainstorming canvas: Excalidraw (MIT) inside a WKWebView owned by
// the app (crates/app/src/canvas.rs). Everything, fonts included, is served
// from the binary over the `sawitcanvas://` protocol, so it works offline.
//
// JS → Rust: `window.ipc.postMessage(text)` with
//   "save\n<.excalidraw json>"     the scene, debounced
//   "library\n<.excalidrawlib json>" the personal shape library
//   "maximize"                     toggle filling the window
//   "result\n<id>\n<json>"         the answer to an `apply` (below)
// Rust → JS: `window.sawitCanvas.setMaximized(bool)`, `.flush()`,
// `.setTheme(tokens)` (Sawit's design tokens, see theme.ts), and
// `.apply(id, op)` for scripting the canvas (the AI agent). Ops queue until
// Excalidraw has loaded and run in order; each answers with one "result"
// message, json being `{ok: true, ...}` or `{ok: false, error}`. Ops that
// change the board save at once and scroll the changed elements into view.
//   {op:"get"}                       → {count, elements: [{id, type, x, y,
//                                       width, height, text?, strokeColor,
//                                       backgroundColor, groupIds, frameId,
//                                       containerId?, name?, startBinding?,
//                                       endBinding?}]} (non-deleted only)
//   {op:"add", elements: [skeleton]} → {added, ids}  convertToExcalidrawElements
//                                       skeletons; given ids are kept, so
//                                       arrows' start/end {id} can bind to them
//                                       or to elements already on the board
//   {op:"mermaid", source, x?, y?}   → {added, ids}  placed right of the board
//   {op:"update", elements: [{id, ...props, text?|label:{text}}]}
//                                    → {updated, missing}
//   {op:"delete", ids}               → {deleted, missing}  labels go too
//   {op:"clear"}                     → {deleted}
//   {op:"replace", scene}            → {count}  an .excalidraw object
//   {op:"export_svg"}                → {svg}
//   {op:"export_png"}                → {png_base64}
//   {op:"scroll_to_content"}         → {}
// `added`/`ids` count top-level elements, not containers' bound text.

import { createRoot } from 'react-dom/client'
import { useEffect, useState } from 'react'
import {
	Excalidraw,
	MainMenu,
	serializeAsJSON,
	serializeLibraryAsJSON,
} from '@excalidraw/excalidraw'
import '@excalidraw/excalidraw/index.css'
import { apply, attach } from './api'
import { FALLBACK, SawitTheme, applyTheme, canvasBackground } from './theme'
import './canvas.css'

declare global {
	interface Window {
		ipc?: { postMessage(message: string): void }
		__sawitCanvasMaximized?: boolean
		__sawitTheme?: SawitTheme
		sawitCanvas: {
			setMaximized(on: boolean): void
			setTheme(theme: SawitTheme): void
			flush(): void
			apply(id: string, op: object): void
		}
	}
}

const SAVE_DELAY_MS = 300

function post(message: string) {
	window.ipc?.postMessage(message)
}

let maximizedListener: ((on: boolean) => void) | null = null
let initiallyMaximized = window.__sawitCanvasMaximized ?? false
let pendingSave: (() => void) | null = null
let currentTheme: SawitTheme = window.__sawitTheme ?? FALLBACK
let themeListener: ((theme: SawitTheme) => void) | null = null
applyTheme(currentTheme)

window.sawitCanvas = {
	setMaximized(on) {
		initiallyMaximized = on
		maximizedListener?.(on)
	},
	setTheme(theme) {
		currentTheme = theme
		applyTheme(theme)
		themeListener?.(theme)
	},
	flush() {
		pendingSave?.()
	},
	apply(id, op) {
		apply(id, op as any)
	},
}

function MaximizeIcon({ maximized }: { maximized: boolean }) {
	// Lucide maximize-2 / minimize-2, matching the terminal's button.
	return (
		<svg
			width="16"
			height="16"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="2"
			strokeLinecap="round"
			strokeLinejoin="round"
		>
			{maximized ? (
				<>
					<polyline points="4 14 10 14 10 20" />
					<polyline points="20 10 14 10 14 4" />
					<line x1="14" y1="10" x2="21" y2="3" />
					<line x1="3" y1="21" x2="10" y2="14" />
				</>
			) : (
				<>
					<polyline points="15 3 21 3 21 9" />
					<polyline points="9 21 3 21 3 15" />
					<line x1="21" y1="3" x2="14" y2="10" />
					<line x1="3" y1="21" x2="10" y2="14" />
				</>
			)}
		</svg>
	)
}

function MaximizeButton() {
	const [maximized, setMaximized] = useState(initiallyMaximized)
	useEffect(() => {
		maximizedListener = setMaximized
		return () => {
			maximizedListener = null
		}
	}, [])
	const title = maximized ? 'Restore Layout (⇧⌘↩)' : 'Fill Window (⇧⌘↩)'
	return (
		<button
			className="sawit-maximize"
			title={title}
			aria-label={title}
			onClick={() => post('maximize')}
		>
			<MaximizeIcon maximized={maximized} />
		</button>
	)
}

// Saves only when the serialized scene actually changed: Excalidraw calls
// onChange for every pointer move and hover too.
let timer: ReturnType<typeof setTimeout> | undefined
let lastSaved = ''
function scheduleSave(serialize: () => string) {
	clearTimeout(timer)
	const save = () => {
		clearTimeout(timer)
		timer = undefined
		pendingSave = null
		const json = serialize()
		if (json !== lastSaved) {
			lastSaved = json
			post('save\n' + json)
		}
	}
	pendingSave = save
	timer = setTimeout(save, SAVE_DELAY_MS)
}

// Saves the scene now, skipping the debounce (after scripted changes).
function saveNow(api: any) {
	clearTimeout(timer)
	timer = undefined
	pendingSave = null
	const json = serializeAsJSON(
		api.getSceneElementsIncludingDeleted(),
		api.getAppState(),
		api.getFiles(),
		'local'
	)
	if (json !== lastSaved) {
		lastSaved = json
		post('save\n' + json)
	}
}

type Saved = { scene: any; library: any }

// New shapes and text start in Sawit's clean style: straight lines and a
// plain sans-serif (Nunito) instead of the hand-drawn look. Both stay one
// click away in the properties panel.
const SAWIT_DEFAULTS = {
	currentItemRoughness: 0,
	currentItemFontFamily: 6, // FONT_FAMILY.Nunito
	currentItemStrokeWidth: 1,
}

function App({ scene, library }: Saved) {
	const [theme, setTheme] = useState(currentTheme)
	const [api, setApi] = useState<any>(null)
	useEffect(() => {
		themeListener = setTheme
		return () => {
			themeListener = null
		}
	}, [])
	// The canvas follows the app's theme; its background is the app's too.
	useEffect(() => {
		api?.updateScene({ appState: { viewBackgroundColor: canvasBackground(theme) } })
	}, [api, theme])
	return (
		<Excalidraw
			theme={theme.mode}
			langCode="en"
			excalidrawAPI={(a) => {
				setApi(a)
				attach(a, () => saveNow(a))
			}}
			initialData={{
				elements: scene?.elements ?? [],
				appState: {
					...SAWIT_DEFAULTS,
					...(scene?.appState ?? {}),
					theme: theme.mode,
					viewBackgroundColor: canvasBackground(theme),
				},
				files: scene?.files ?? {},
				libraryItems: library?.libraryItems ?? library?.library ?? [],
			}}
			onChange={(elements, appState, files) =>
				scheduleSave(() => serializeAsJSON(elements, appState, files, 'local'))
			}
			onLibraryChange={(items) => post('library\n' + serializeLibraryAsJSON(items))}
			renderTopRightUI={() => <MaximizeButton />}
			UIOptions={{ canvasActions: { toggleTheme: false, changeViewBackgroundColor: false } }}
		>
			{/* The default menu minus the links to excalidraw.com, Discord,
			    GitHub and Excalidraw+ (the canvas stays local), and minus the
			    theme and background pickers (the canvas follows Sawit's theme). */}
			<MainMenu>
				<MainMenu.DefaultItems.LoadScene />
				<MainMenu.DefaultItems.SaveToActiveFile />
				<MainMenu.DefaultItems.Export />
				<MainMenu.DefaultItems.SaveAsImage />
				<MainMenu.DefaultItems.CommandPalette />
				<MainMenu.DefaultItems.SearchMenu />
				<MainMenu.DefaultItems.Help />
				<MainMenu.DefaultItems.ClearCanvas />
			</MainMenu>
		</Excalidraw>
	)
}

// ⇧⌘↩ toggles filling the window, as it does for the terminal. The webview
// has keyboard focus while drawing, so the app's own binding can't see it.
window.addEventListener(
	'keydown',
	(e) => {
		if (e.key === 'Enter' && e.metaKey && e.shiftKey && !e.altKey && !e.ctrlKey) {
			e.preventDefault()
			e.stopPropagation()
			post('maximize')
		}
	},
	true
)

async function load(path: string): Promise<any> {
	try {
		const res = await fetch(path)
		if (!res.ok) return undefined
		const text = await res.text()
		return text.trim() ? JSON.parse(text) : undefined
	} catch {
		return undefined
	}
}

Promise.all([load('/scene.excalidraw'), load('/library.excalidrawlib')]).then(
	([scene, library]) => {
		lastSaved = scene ? JSON.stringify(scene, null, 2) : ''
		createRoot(document.getElementById('root')!).render(
			<App scene={scene} library={library} />
		)
	}
)
