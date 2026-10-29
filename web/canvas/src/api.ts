// Scripted access to the canvas, for the AI agent (through the app):
// `window.sawitCanvas.apply(id, op)`. Ops queue until Excalidraw is up and
// run one at a time, in order. Each answers with the IPC message
// "result\n<id>\n<json>", where json is `{ok: true, ...}` or
// `{ok: false, error}`. Mutating ops save straight away and scroll the new
// content into view. See main.tsx's header for the op list.

import {
	CaptureUpdateAction,
	convertToExcalidrawElements,
	exportToBlob,
	exportToSvg,
	getCommonBounds,
	newElementWith,
	restoreElements,
} from '@excalidraw/excalidraw'

type Api = any
type El = any
type Result = { ok: boolean; [key: string]: unknown }
type Op = { op?: string; [key: string]: any }

const BOUND_TEXT_PADDING = 5
const MERMAID_GAP = 100

let api: Api | null = null
let ready = false
const queue: Array<{ id: string; op: Op }> = []
let running = false
let save: (() => void) | null = null

function post(message: string) {
	window.ipc?.postMessage(message)
}

function reply(id: string, result: Result) {
	let json: string
	try {
		json = JSON.stringify(result)
	} catch (e) {
		json = JSON.stringify({ ok: false, error: 'unserializable result: ' + message(e) })
	}
	post('result\n' + id + '\n' + json)
}

function message(e: unknown): string {
	if (e instanceof Error) return e.message || String(e)
	return typeof e === 'string' ? e : JSON.stringify(e) ?? String(e)
}

/** Called with the Excalidraw API; `saveNow` writes the scene immediately. */
export function attach(excalidrawAPI: Api, saveNow: () => void) {
	api = excalidrawAPI
	save = saveNow
	// The API arrives from Excalidraw's constructor, before it loads the
	// saved scene; wait for that so an early op isn't overwritten.
	const poll = () => {
		if (api !== excalidrawAPI) return
		if (api.getAppState().isLoading) {
			setTimeout(poll, 30)
			return
		}
		ready = true
		drain()
	}
	setTimeout(poll, 0)
}

export function apply(id: string, op: Op) {
	try {
		queue.push({ id: String(id), op })
		drain()
	} catch (e) {
		try {
			reply(String(id), { ok: false, error: message(e) })
		} catch {}
	}
}

async function drain() {
	if (running || !ready || !api) return
	running = true
	try {
		while (queue.length) {
			const { id, op } = queue.shift()!
			let result: Result
			try {
				result = await run(op ?? {})
			} catch (e) {
				result = { ok: false, error: message(e) }
			}
			reply(id, result)
		}
	} finally {
		running = false
	}
}

const live = () => api.getSceneElements() as readonly El[]
const all = () => api.getSceneElementsIncludingDeleted() as readonly El[]

async function run(op: Op): Promise<Result> {
	switch (op.op) {
		case 'get':
			return get()
		case 'add':
			return add(op)
		case 'mermaid':
			return mermaid(op)
		case 'update':
			return update(op)
		case 'delete':
			return remove(op)
		case 'clear':
			return clear()
		case 'replace':
			return replace(op)
		case 'export_svg':
			return exportSvg()
		case 'export_png':
			return exportPng()
		case 'scroll_to_content':
			fit(live())
			return { ok: true }
		default:
			return { ok: false, error: `unknown op ${JSON.stringify(op.op)}` }
	}
}

// ---- helpers -------------------------------------------------------------

function commit(elements: readonly El[]) {
	api.updateScene({ elements, captureUpdate: CaptureUpdateAction.IMMEDIATELY })
	save?.()
}

function fit(elements: readonly El[]) {
	const shown = elements.filter((e) => !e.isDeleted)
	if (!shown.length) return
	api.scrollToContent(shown, { fitToContent: true, animate: true })
}

function boundTextOf(container: El, byId: Map<string, El>): El | undefined {
	const ref = container.boundElements?.find((b: El) => b.type === 'text')
	const text = ref && byId.get(ref.id)
	return text && !text.isDeleted ? text : undefined
}

function summary(e: El, byId: Map<string, El>) {
	const out: Record<string, unknown> = {
		id: e.id,
		type: e.type,
		x: e.x,
		y: e.y,
		width: e.width,
		height: e.height,
		strokeColor: e.strokeColor,
		backgroundColor: e.backgroundColor,
		groupIds: e.groupIds,
		frameId: e.frameId,
	}
	if (e.type === 'text') {
		out.text = e.originalText ?? e.text
		if (e.containerId) out.containerId = e.containerId
	} else {
		const label = boundTextOf(e, byId)
		if (label) out.text = label.originalText ?? label.text
	}
	if (e.type === 'frame' || e.type === 'magicframe') out.name = e.name
	if (e.type === 'arrow') {
		out.startBinding = e.startBinding?.elementId ?? null
		out.endBinding = e.endBinding?.elementId ?? null
	}
	return out
}

// Where a container's label goes, as Excalidraw lays it out (non-arrows).
function labelBox(c: El) {
	const P = BOUND_TEXT_PADDING
	let ox = 0
	let oy = 0
	let w = c.width
	let h = c.height
	if (c.type === 'ellipse') {
		w = Math.round((c.width / 2) * Math.SQRT2)
		h = Math.round((c.height / 2) * Math.SQRT2)
		ox = (c.width - w) / 2
		oy = (c.height - h) / 2
	} else if (c.type === 'diamond') {
		w = Math.round(c.width / 2)
		h = Math.round(c.height / 2)
		ox = c.width / 4
		oy = c.height / 4
	}
	return { x: c.x + ox + P, y: c.y + oy + P, w: w - 2 * P, h: h - 2 * P }
}

// Height a container needs so a label `textHeight` tall fits inside.
function containerHeightFor(c: El, textHeight: number) {
	const inner = textHeight + 2 * BOUND_TEXT_PADDING
	if (c.type === 'ellipse') return Math.ceil(inner * Math.SQRT2)
	if (c.type === 'diamond') return Math.ceil(inner * 2)
	return inner
}

/**
 * Re-measures (and re-wraps) the given text elements, grows their containers
 * when the label no longer fits, and centers labels in their containers.
 * Returns the whole element list with the changes applied.
 */
function relayout(elements: El[], textIds: Set<string>): El[] {
	if (!textIds.size) return elements
	const byId = new Map(elements.map((e) => [e.id, e]))
	// Re-wrap from the unwrapped text, so a wider container un-wraps it.
	const subset = [...textIds]
		.map((id) => byId.get(id))
		.filter((t) => t && !t.isDeleted)
		.map((t) => ({ ...t, text: t.originalText ?? t.text })) as El[]
	const containers = new Map<string, El>()
	for (const t of subset) {
		const c = t.containerId && byId.get(t.containerId)
		if (c) containers.set(c.id, c)
	}
	// restoreElements refreshes every text it's given (only together with
	// repairBindings); give it only ours and their containers, to wrap
	// against, and keep just the measurements: its repairs of this partial
	// list (frames, bindings) don't apply to the scene.
	const refreshed = restoreElements([...containers.values(), ...subset], null, {
		refreshDimensions: true,
		repairBindings: true,
	}) as El[]
	for (const e of refreshed) {
		const original = textIds.has(e.id) && byId.get(e.id)
		if (!original) continue
		const { text, x, y, width, height } = e
		byId.set(e.id, newElementWith(original, { text, x, y, width, height }))
	}
	for (const t of subset) {
		const text = byId.get(t.id)
		const c = text?.containerId && byId.get(text.containerId)
		if (!text || !c || c.type === 'arrow' || c.isDeleted) continue
		let container = c
		const box0 = labelBox(container)
		if (text.height > box0.h) {
			container = newElementWith(container, { height: containerHeightFor(container, text.height) })
			byId.set(container.id, container)
		}
		const box = labelBox(container)
		const x =
			text.textAlign === 'left'
				? box.x
				: text.textAlign === 'right'
					? box.x + box.w - text.width
					: box.x + (box.w - text.width) / 2
		const y =
			text.verticalAlign === 'top'
				? box.y
				: text.verticalAlign === 'bottom'
					? box.y + box.h - text.height
					: box.y + (box.h - text.height) / 2
		byId.set(text.id, newElementWith(text, { x, y }))
	}
	return elements.map((e) => byId.get(e.id) ?? e)
}

const ARROW_GAP = 6

// Where the ray from `el`'s center toward (tx, ty) leaves its outline, plus a
// small gap. Ignores rotation.
function edgePoint(el: El, tx: number, ty: number): [number, number] | null {
	const hw = el.width / 2
	const hh = el.height / 2
	const cx = el.x + hw
	const cy = el.y + hh
	const dx = tx - cx
	const dy = ty - cy
	const len = Math.hypot(dx, dy)
	if (!len || !hw || !hh) return null
	let t: number
	if (el.type === 'ellipse') t = 1 / Math.hypot(dx / hw, dy / hh)
	else if (el.type === 'diamond') t = 1 / (Math.abs(dx) / hw + Math.abs(dy) / hh)
	else t = Math.min(dx ? hw / Math.abs(dx) : Infinity, dy ? hh / Math.abs(dy) : Infinity)
	return [cx + dx * t + (dx / len) * ARROW_GAP, cy + dy * t + (dy / len) * ARROW_GAP]
}

const center = (el: El): [number, number] => [el.x + el.width / 2, el.y + el.height / 2]

/**
 * Points a straight (two-point) bound arrow from its start element's edge to
 * its end element's, as Excalidraw does when you drag a bound shape; the
 * conversion and scripted moves don't. Moves the arrow's label along.
 */
function routeArrows(elements: El[], arrowIds: Set<string>): El[] {
	if (!arrowIds.size) return elements
	const byId = new Map(elements.map((e) => [e.id, e]))
	for (const id of arrowIds) {
		const arrow = byId.get(id)
		if (!arrow || arrow.isDeleted || arrow.type !== 'arrow' || arrow.points?.length !== 2) continue
		const s = arrow.startBinding && byId.get(arrow.startBinding.elementId)
		const e = arrow.endBinding && byId.get(arrow.endBinding.elementId)
		if (!s && !e) continue
		const [p0x, p0y] = arrow.points[0]
		const [p1x, p1y] = arrow.points[1]
		const abs0: [number, number] = [arrow.x + p0x, arrow.y + p0y]
		const abs1: [number, number] = [arrow.x + p1x, arrow.y + p1y]
		const toward0 = s ? center(s) : abs0
		const toward1 = e ? center(e) : abs1
		const a = (s && edgePoint(s, ...toward1)) || abs0
		const b = (e && edgePoint(e, ...toward0)) || abs1
		const w = b[0] - a[0]
		const h = b[1] - a[1]
		const next = newElementWith(arrow, {
			x: a[0],
			y: a[1],
			points: [
				[0, 0],
				[w, h],
			],
			width: Math.abs(w),
			height: Math.abs(h),
		} as any)
		byId.set(id, next)
		const label = boundTextOf(next, byId)
		if (label) {
			byId.set(
				label.id,
				newElementWith(label, {
					x: a[0] + w / 2 - label.width / 2,
					y: a[1] + h / 2 - label.height / 2,
				})
			)
		}
	}
	return elements.map((e) => byId.get(e.id) ?? e)
}

// Arrows bound to any of `ids`.
function arrowsBoundTo(elements: readonly El[], ids: Set<string>): Set<string> {
	const out = new Set<string>()
	for (const e of elements) {
		if (e.type !== 'arrow' || e.isDeleted) continue
		if (ids.has(e.startBinding?.elementId) || ids.has(e.endBinding?.elementId)) out.add(e.id)
	}
	return out
}

// ---- ops -----------------------------------------------------------------

function get(): Result {
	const elements = live()
	const byId = new Map(elements.map((e) => [e.id, e]))
	const list = elements.map((e) => summary(e, byId))
	return { ok: true, count: list.length, elements: list }
}

function add(op: Op): Result {
	const skeletons = op.elements
	if (!Array.isArray(skeletons) || !skeletons.length) {
		return { ok: false, error: 'add: `elements` must be a non-empty array' }
	}
	const scene = all()
	const sceneById = new Map(scene.map((e) => [e.id, e]))
	const batchIds = new Set(skeletons.map((s: El) => s?.id).filter(Boolean))
	for (const id of batchIds) {
		const existing = sceneById.get(id)
		if (existing && !existing.isDeleted) {
			return { ok: false, error: `add: id already exists: ${id}` }
		}
	}
	// Arrows may bind to shapes already on the canvas: lend copies of those
	// to the conversion so it can bind, then keep only their new
	// boundElements.
	const borrowed = new Map<string, El>()
	for (const s of skeletons) {
		if (s?.type !== 'arrow') continue
		for (const end of [s.start, s.end]) {
			const id = end?.id
			if (!id || batchIds.has(id) || borrowed.has(id)) continue
			const existing = sceneById.get(id)
			if (!existing || existing.isDeleted) {
				return { ok: false, error: `add: arrow binds to unknown id ${id}` }
			}
			borrowed.set(id, existing)
		}
	}
	const converted = convertToExcalidrawElements([...borrowed.values(), ...skeletons], {
		regenerateIds: false,
	}) as El[]
	const added = converted.filter((e) => !borrowed.has(e.id))
	const addedIds = new Set(added.map((e) => e.id))
	const next = scene
		.filter((e) => !addedIds.has(e.id)) // drop deleted elements we reuse the id of
		.map((e) => {
			if (!borrowed.has(e.id)) return e
			const copy = converted.find((c) => c.id === e.id)
			return copy ? newElementWith(e, { boundElements: copy.boundElements }) : e
		})
	// Bound arrows run edge to edge, unless the skeleton gave its own points.
	const ownPoints = new Set(
		skeletons.filter((s: El) => s?.id && Array.isArray(s.points)).map((s: El) => s.id)
	)
	const route = new Set(
		added.filter((e) => e.type === 'arrow' && !ownPoints.has(e.id)).map((e) => e.id)
	)
	const placed = routeArrows([...next, ...added], route)
	commit(placed)
	fit(placed.filter((e) => addedIds.has(e.id)))
	// Labels (bound text) come along but aren't counted.
	const ids = added.filter((e) => !e.containerId).map((e) => e.id)
	return { ok: true, added: ids.length, ids }
}

async function mermaid(op: Op): Promise<Result> {
	const source = op.source
	if (typeof source !== 'string' || !source.trim()) {
		return { ok: false, error: 'mermaid: `source` must be a non-empty string' }
	}
	// Excalidraw loads this lazily too; the bundle shares its chunk.
	const { parseMermaidToExcalidraw } = await import('@excalidraw/mermaid-to-excalidraw')
	let parsed: { elements: El[]; files?: Record<string, El> }
	try {
		parsed = await parseMermaidToExcalidraw(source)
	} catch (first) {
		// Excalidraw's own retry: double quotes trip up some diagram types.
		try {
			parsed = await parseMermaidToExcalidraw(source.replace(/"/g, "'"))
		} catch {
			return { ok: false, error: 'mermaid: ' + message(first) }
		}
	}
	const elements = convertToExcalidrawElements(parsed.elements as El[], {
		regenerateIds: true,
	}) as El[]
	if (!elements.length) return { ok: false, error: 'mermaid: the diagram is empty' }

	const [minX, minY] = getCommonBounds(elements)
	const existing = live()
	let x: number
	let y: number
	if (existing.length) {
		const [, eMinY, eMaxX] = getCommonBounds(existing)
		x = eMaxX + MERMAID_GAP
		y = eMinY
	} else {
		x = 0
		y = 0
	}
	if (typeof op.x === 'number') x = op.x
	if (typeof op.y === 'number') y = op.y
	const dx = x - minX
	const dy = y - minY
	const placed = elements.map((e) => ({ ...e, x: e.x + dx, y: e.y + dy }))

	const files = Object.values(parsed.files ?? {})
	if (files.length) api.addFiles(files)
	commit([...all(), ...placed])
	fit(placed)
	const ids = placed.filter((e) => !e.containerId).map((e) => e.id)
	return { ok: true, added: ids.length, ids }
}

const FIXED = new Set(['id', 'type', 'version', 'versionNonce', 'updated', 'isDeleted', 'index'])

function update(op: Op): Result {
	const patches = op.elements
	if (!Array.isArray(patches)) {
		return { ok: false, error: 'update: `elements` must be an array' }
	}
	let elements = [...all()] as El[]
	const index = new Map(elements.map((e, i) => [e.id, i]))
	const at = (id: string) => {
		const i = index.get(id)
		return i === undefined ? undefined : elements[i]
	}
	const put = (e: El) => {
		const i = index.get(e.id)
		if (i === undefined) {
			index.set(e.id, elements.length)
			elements.push(e)
		} else {
			elements[i] = e
		}
	}
	const missing: string[] = []
	const touched: string[] = []
	const relayoutIds = new Set<string>()

	for (const patch of patches) {
		const id = patch?.id
		const current = id && at(id)
		if (!current || current.isDeleted) {
			missing.push(String(id))
			continue
		}
		const props: Record<string, unknown> = {}
		for (const [k, v] of Object.entries(patch)) {
			if (!FIXED.has(k) && k !== 'text' && k !== 'label') props[k] = v
		}
		const text: unknown =
			typeof patch.text === 'string'
				? patch.text
				: typeof patch.label?.text === 'string'
					? patch.label.text
					: undefined

		if (current.type === 'text') {
			if (typeof text === 'string') {
				props.text = text
				props.originalText = text
			}
			const next = newElementWith(current, props as any)
			put(next)
			if (typeof text === 'string' || 'fontSize' in props || 'fontFamily' in props) {
				relayoutIds.add(next.id)
			}
			if (next.containerId) relayoutIds.add(next.id)
		} else {
			const next = newElementWith(current, props as any)
			put(next)
			// Moves the container's label along and re-wraps it to a new size.
			let label = boundTextOf(next, new Map(elements.map((e) => [e.id, e])))
			if (typeof text === 'string') {
				if (label) {
					label = newElementWith(label, { text, originalText: text })
					put(label)
				} else if (next.type !== 'line' && next.type !== 'freedraw') {
					label = createLabel(next, text)
					put(label)
					put(
						newElementWith(next, {
							boundElements: [...(next.boundElements ?? []), { type: 'text', id: label.id }],
						})
					)
				}
			}
			if (label) relayoutIds.add(label.id)
		}
		touched.push(id)
	}

	if (touched.length) {
		elements = relayout(elements, relayoutIds)
		// Arrows follow the shapes they're bound to (arrows the patch itself
		// moved stay where it put them).
		const follow = arrowsBoundTo(elements, new Set(touched))
		for (const id of touched) follow.delete(id)
		elements = routeArrows(elements, follow)
		commit(elements)
		const ids = new Set(touched)
		fit(elements.filter((e) => ids.has(e.id)))
	}
	return { ok: true, updated: touched.length, missing }
}

function createLabel(container: El, text: string): El {
	const [label] = convertToExcalidrawElements(
		[
			{
				type: 'text',
				text,
				x: container.x,
				y: container.y,
				strokeColor: container.strokeColor,
				textAlign: 'center',
				verticalAlign: 'middle',
			} as El,
		],
		{ regenerateIds: true }
	) as El[]
	return newElementWith(label, {
		containerId: container.id,
		frameId: container.frameId ?? null,
		groupIds: container.groupIds ?? [],
	} as any)
}

function remove(op: Op): Result {
	const ids = op.ids
	if (!Array.isArray(ids)) return { ok: false, error: 'delete: `ids` must be an array' }
	const scene = all()
	const doomed = new Set<string>()
	const missing: string[] = []
	const liveIds = new Set(scene.filter((e) => !e.isDeleted).map((e) => e.id))
	for (const id of ids) {
		if (liveIds.has(id)) doomed.add(id)
		else missing.push(String(id))
	}
	if (!doomed.size) return { ok: true, deleted: 0, missing }
	const deleted = doomed.size
	// Labels go with their containers.
	for (const e of scene) {
		if (!e.isDeleted && e.containerId && doomed.has(e.containerId)) doomed.add(e.id)
	}
	// Arrows that stay lose their bindings to what's gone.
	const next = scene.map((e) => {
		if (doomed.has(e.id)) return newElementWith(e, { isDeleted: true })
		if (e.isDeleted || e.type !== 'arrow') return e
		const start = doomed.has(e.startBinding?.elementId)
		const end = doomed.has(e.endBinding?.elementId)
		if (!start && !end) return e
		return newElementWith(e, {
			...(start ? { startBinding: null } : {}),
			...(end ? { endBinding: null } : {}),
		} as any)
	})
	commit(next)
	fit(next)
	return { ok: true, deleted, missing }
}

function clear(): Result {
	const scene = all()
	const removed = scene.filter((e) => !e.isDeleted).length
	commit(scene.map((e) => (e.isDeleted ? e : newElementWith(e, { isDeleted: true }))))
	return { ok: true, deleted: removed }
}

function replace(op: Op): Result {
	const scene = op.scene
	if (!scene || typeof scene !== 'object' || !Array.isArray(scene.elements)) {
		return { ok: false, error: 'replace: `scene` must be an .excalidraw object with `elements`' }
	}
	const files = Object.values(scene.files ?? {})
	if (files.length) api.addFiles(files)
	const elements = restoreElements(scene.elements, null, { repairBindings: true, refreshDimensions: false })
	const { collaborators: _c, ...appState } = scene.appState ?? {}
	api.updateScene({
		elements,
		appState: { ...appState, theme: 'dark' },
		captureUpdate: CaptureUpdateAction.IMMEDIATELY,
	})
	save?.()
	fit(elements)
	return { ok: true, count: elements.filter((e: El) => !e.isDeleted).length }
}

function exportOpts() {
	return {
		elements: live(),
		appState: { ...api.getAppState(), exportWithDarkMode: true, exportBackground: true },
		files: api.getFiles(),
	}
}

async function exportSvg(): Promise<Result> {
	const svg = await exportToSvg(exportOpts())
	return { ok: true, svg: svg.outerHTML }
}

async function exportPng(): Promise<Result> {
	const blob = await exportToBlob({ ...exportOpts(), mimeType: 'image/png' })
	const dataUrl: string = await new Promise((resolve, reject) => {
		const reader = new FileReader()
		reader.onload = () => resolve(String(reader.result))
		reader.onerror = () => reject(reader.error ?? new Error('could not read the PNG'))
		reader.readAsDataURL(blob)
	})
	return { ok: true, png_base64: dataUrl.slice(dataUrl.indexOf(',') + 1) }
}
