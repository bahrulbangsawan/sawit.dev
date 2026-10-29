// Mermaid → Excalidraw with Pele, Sawit's Mermaid renderer, standing in for
// @excalidraw/mermaid-to-excalidraw (build.mjs aliases that package here).
// Excalidraw calls `parseMermaidToExcalidraw` from its Mermaid dialog and
// when Mermaid text is pasted; api.ts's `mermaid` op (the AI agent) does too.
//
// Pele draws the diagram as SVG; the SVG is laid out in a hidden <div> so
// every shape's position can be read back with transforms applied, then
// turned into element skeletons:
//   .pele-node         → rectangle / ellipse / diamond with its label bound;
//                        multi-text nodes (classes, entities) become a box
//                        plus texts and divider lines, grouped
//   .pele-edge         → arrow along the edge's route, arrowheads from its
//                        markers, bound to the nodes at either end
//   .pele-edge-label   → the arrow's label (or a text where it sits apart)
//   anything else      → rectangles, ellipses, lines and texts as drawn
// Charts (pie, gantt, …) have no shapes worth editing: they come in as one
// SVG image, as they did with mermaid-to-excalidraw.
//
// Pele itself is served from the binary (`/pele.js`, the copy that renders
// diagrams in the app), so the canvas bundle carries no Mermaid engine.

type Point = [number, number]
type Box = { x: number; y: number; w: number; h: number }
type Skeleton = Record<string, any>

declare global {
	interface Window {
		pele?: any
	}
}

/** Diagram types that become editable shapes; the rest become an image. */
const EDITABLE = new Set([
	'flowchart',
	'sequence',
	'class',
	'state',
	'er',
	'mindmap',
	'requirement',
	'block',
	'kanban',
	'c4',
	'gitGraph',
	'timeline',
])

// Pele's default palette (`--pele-*` unset), mapped onto Excalidraw's.
const INK = '#1e1e1e'
const NEUTRAL_STROKES = new Set(['#1f1f1f', '#8a8a8a'])
const NEUTRAL_FILLS = new Set(['#f3f3f3', '#ffffff'])
const PELE_BG = '#ffffff'
const PELE_SURFACE_ALT = '#e6e6e6'

// The canvas's defaults (main.tsx SAWIT_DEFAULTS): Nunito, crisp, thin.
const FONT_FAMILY = 6
const FONT_CSS = 'Nunito, sans-serif'
const ROUGHNESS = 0
const LINE_HEIGHT = 1.25

/** Excalidraw's padding between a container and its label. */
const LABEL_PADDING = 5

/** A box that only holds a label and arrow bindings. */
const INVISIBLE = {
	strokeColor: 'transparent',
	backgroundColor: 'transparent',
	strokeWidth: 1,
	roughness: ROUGHNESS,
}

/** How far an edge's end may sit from a node's outline and still bind. */
const BIND_DISTANCE = 14

let loading: Promise<any> | null = null

function loadPele(): Promise<any> {
	if (window.pele) return Promise.resolve(window.pele)
	loading ??= new Promise((resolve, reject) => {
		const script = document.createElement('script')
		script.src = '/pele.js'
		script.onload = () =>
			window.pele ? resolve(window.pele) : reject(new Error('Pele did not load'))
		script.onerror = () => {
			loading = null
			reject(new Error('Pele could not be loaded'))
		}
		document.head.appendChild(script)
	})
	return loading
}

export async function parseMermaidToExcalidraw(
	definition: string,
	_config?: unknown
): Promise<{ elements: Skeleton[]; files?: Record<string, any> }> {
	const pele = await loadPele()
	let rendered: { svg: string; width: number; height: number; type: string }
	try {
		rendered = pele.render(definition, { responsive: false, idPrefix: 'sawit-mm' })
	} catch (e: any) {
		throw new Error(e?.message || String(e))
	}
	if (!EDITABLE.has(rendered.type)) return asImage(rendered)

	const host = document.createElement('div')
	host.style.cssText =
		// Off screen and transparent, but not visibility:hidden: that is
		// inherited, and `visible()` reads it to skip hidden shapes.
		'position:fixed;left:-100000px;top:0;opacity:0;pointer-events:none;' +
		`--pele-font:${FONT_CSS}`
	host.innerHTML = rendered.svg
	document.body.appendChild(host)
	try {
		const svg = host.querySelector('svg') as SVGSVGElement
		return { elements: new Converter(svg).run() }
	} finally {
		host.remove()
	}
}

function asImage(rendered: { svg: string; width: number; height: number }) {
	const fileId = 'pele-' + uid()
	const bytes = new TextEncoder().encode(rendered.svg)
	let binary = ''
	for (const b of bytes) binary += String.fromCharCode(b)
	return {
		elements: [
			{
				type: 'image',
				id: uid(),
				fileId,
				x: 0,
				y: 0,
				width: rendered.width,
				height: rendered.height,
			},
		],
		files: {
			[fileId]: {
				id: fileId,
				mimeType: 'image/svg+xml',
				dataURL: 'data:image/svg+xml;base64,' + btoa(binary),
				created: Date.now(),
			},
		},
	}
}

let counter = 0
function uid(): string {
	return Math.random().toString(36).slice(2, 10) + (counter++).toString(36)
}

const SHAPES = 'rect, circle, ellipse, polygon, polyline, path, line'

class Converter {
	private out: Skeleton[] = []
	private done = new Set<Element>()
	/** Shapes an arrow may bind to: the node outlines. */
	private targets: { id: string; box: Box }[] = []
	private measure = document.createElement('canvas').getContext('2d')!

	constructor(private svg: SVGSVGElement) {}

	run(): Skeleton[] {
		const q = (s: string) => Array.from(this.svg.querySelectorAll(s))
		// Clusters first so they sit under their nodes; nodes before edges so
		// edges can bind.
		for (const g of q('.pele-cluster, .pele-frame, .pele-column')) this.group(g, false)
		for (const g of q('.pele-node, .pele-commit, .pele-note')) this.group(g, true)
		for (const g of q('.pele-edge')) this.edge(g)
		for (const el of q(SHAPES + ', text')) this.loose(el)
		return this.out
	}

	// --- Nodes ------------------------------------------------------------

	/** A node (or cluster): its outline, labels and decorations, grouped. */
	private group(g: Element, bindLabel: boolean) {
		if (this.done.has(g) || !visible(g)) return
		const shapes = Array.from(g.querySelectorAll(SHAPES)).filter(
			(s) => !this.done.has(s) && visible(s) && !isMask(s)
		)
		const texts = Array.from(g.querySelectorAll('text')).filter(
			(t) => !this.done.has(t) && (t.textContent ?? '').trim()
		)
		this.done.add(g)
		// The outline is the largest shape. Excalidraw draws rectangles,
		// ellipses and diamonds itself; any other outline (cylinder, hexagon,
		// a mindmap leaf with none) is drawn as lines under an invisible box
		// that holds the label and what arrows bind to.
		const outline = shapes
			.map((s) => ({ s, box: this.box(s) }))
			.sort((a, b) => b.box.w * b.box.h - a.box.w * a.box.h)[0]
		let container: Skeleton | null = null
		if (outline && containerType(outline.s)) {
			const { s, box } = outline
			container = { ...this.shapeStyle(s), type: containerType(s), ...frame(box) }
			if (isRounded(s, g)) container.roundness = { type: 3 }
			this.done.add(s)
		} else if (outline || (bindLabel && texts.length === 1)) {
			const box = outline ? outline.box : pad(this.box(texts[0]), 4)
			container = { ...INVISIBLE, type: 'rectangle', ...frame(box) }
		}
		const groupId = 'g' + uid()
		const members: Skeleton[] = []
		for (const s of shapes) if (!this.done.has(s)) members.push(...this.shape(s))
		if (container) {
			const box: Box = { x: container.x, y: container.y, w: container.width, h: container.height }
			const label =
				bindLabel && texts.length === 1 && inside(this.box(texts[0]), box) ? texts[0] : null
			if (label) {
				container.label = {
					text: textOf(label),
					fontSize: fontSize(label),
					fontFamily: FONT_FAMILY,
					strokeColor: textColor(label),
				}
				this.fit(container)
				this.done.add(label)
			}
			members.push(container)
			if (bindLabel || g.classList.contains('pele-cluster')) {
				this.targets.push({
					id: container.id,
					box: { x: container.x, y: container.y, w: container.width, h: container.height },
				})
			}
		}
		for (const t of texts) if (!this.done.has(t)) members.push(this.text(t))
		if (members.length > 1) for (const m of members) m.groupIds = [groupId]
		this.out.push(...members)
	}

	/**
	 * Grows a container around its centre until Excalidraw can lay out the
	 * label without wrapping: diamonds and ellipses keep text in a smaller
	 * inner area than rectangles, and Nunito runs wider than Pele's font.
	 */
	private fit(c: Skeleton) {
		const { text, fontSize: size } = c.label
		this.measure.font = `${size}px ${FONT_CSS}`
		const lines = String(text).split('\n')
		const tw = Math.max(...lines.map((l) => this.measure.measureText(l).width)) + 2 * LABEL_PADDING + 2
		const th = lines.length * size * LINE_HEIGHT + 2 * LABEL_PADDING
		const k = c.type === 'diamond' ? 2 : c.type === 'ellipse' ? Math.SQRT2 : 1
		const w = Math.max(c.width, tw * k)
		const h = Math.max(c.height, th * k)
		c.x -= (w - c.width) / 2
		c.y -= (h - c.height) / 2
		c.width = w
		c.height = h
	}

	// --- Edges ------------------------------------------------------------

	private edge(g: Element) {
		if (this.done.has(g)) return
		this.done.add(g)
		// An edge is a group (route + markers) or, in mindmaps and git
		// graphs, the bare route.
		const bare = g.matches('path, line, polyline')
		const paths = (bare ? [g] : Array.from(g.querySelectorAll('path, line, polyline'))) as SVGGraphicsElement[]
		const route = paths.find((p) => !p.classList.contains('pele-marker'))
		if (!route) return
		const points = this.points(route)
		this.done.add(route)
		if (points.length < 2) return
		const markers = Array.from(g.querySelectorAll('.pele-marker'))
		markers.forEach((m) => this.done.add(m))

		// Each marker belongs to the end it sits nearest to.
		const start = points[0]
		const end = points[points.length - 1]
		const atStart: Element[] = []
		const atEnd: Element[] = []
		for (const m of markers) {
			const c = center(this.box(m))
			;(dist(c, start) < dist(c, end) ? atStart : atEnd).push(m)
		}
		// Pele stops the line where the marker begins; reach the tip again.
		if (atEnd.length) points[points.length - 1] = this.tip(atEnd, points[points.length - 2])
		if (atStart.length) points[0] = this.tip(atStart, points[1])

		const style = this.strokeStyle(route)
		const [x, y] = points[0]
		const arrow: Skeleton = {
			type: 'arrow',
			id: uid(),
			x,
			y,
			points: points.map(([px, py]) => [px - x, py - y]),
			strokeColor: style.strokeColor,
			strokeStyle: style.strokeStyle,
			strokeWidth: style.strokeWidth,
			roughness: ROUGHNESS,
			startArrowhead: arrowhead(atStart),
			endArrowhead: arrowhead(atEnd),
		}
		// Curves arrive densely sampled: straight segments follow them exactly,
		// where Excalidraw's rounding would overshoot at the bends.
		arrow.roundness = null
		const from = this.target(points[0])
		const to = this.target(points[points.length - 1])
		if (from) arrow.start = { id: from }
		if (to) arrow.end = { id: to }

		const id = g.getAttribute('data-id')
		const labelGroup = id
			? this.svg.querySelector(`.pele-edge-label[data-id="${CSS.escape(id)}"]`)
			: null
		const labelText = labelGroup?.querySelector('text')
		if (labelGroup && labelText && (labelText.textContent ?? '').trim()) {
			this.done.add(labelGroup)
			labelGroup.querySelectorAll('*').forEach((e) => this.done.add(e))
			// On the line (flowcharts): the arrow's own label. Beside it
			// (sequence messages sit above theirs): a text where Pele put it.
			if (distanceToRoute(center(this.box(labelText)), points) < 6) {
				arrow.label = {
					text: textOf(labelText),
					fontSize: fontSize(labelText),
					fontFamily: FONT_FAMILY,
				}
			} else {
				this.out.push(arrow)
				this.out.push(this.text(labelText))
				return
			}
		}
		this.out.push(arrow)
	}

	/** The node outline an edge's end touches, if any (the smallest wins). */
	private target(p: Point): string | undefined {
		let best: { id: string; area: number } | undefined
		for (const t of this.targets) {
			if (edgeDistance(p, t.box) > BIND_DISTANCE) continue
			const area = t.box.w * t.box.h
			if (!best || area < best.area) best = { id: t.id, area }
		}
		return best?.id
	}

	/** The marker point farthest from the line: where the arrow points. */
	private tip(markers: Element[], from: Point): Point {
		let best: Point = from
		let far = -1
		for (const m of markers) {
			for (const p of this.points(m as SVGGraphicsElement)) {
				const d = dist(p, from)
				if (d > far) {
					far = d
					best = p
				}
			}
		}
		return best
	}

	// --- Everything else ----------------------------------------------------

	private loose(el: Element) {
		if (this.done.has(el) || !visible(el)) return
		this.done.add(el)
		if (el.tagName === 'text') {
			if ((el.textContent ?? '').trim()) this.out.push(this.text(el))
		} else if (!isMask(el)) {
			this.out.push(...this.shape(el))
		}
	}

	private shape(el: Element): Skeleton[] {
		this.done.add(el)
		const type = containerType(el)
		const style = this.shapeStyle(el)
		if (type) {
			const box = this.box(el)
			const s: Skeleton = { ...style, type, id: uid(), x: box.x, y: box.y, width: box.w, height: box.h }
			if (isRounded(el, el.parentElement)) s.roundness = { type: 3 }
			return [s]
		}
		// Polygons and paths: one line per subpath, closed ones filled.
		return this.subpaths(el)
			.filter((pts) => pts.length >= 2)
			.map((pts) => {
				const closed = pts.length > 2 && dist(pts[0], pts[pts.length - 1]) < 0.5
				const [x, y] = pts[0]
				return {
					...style,
					type: 'line',
					id: uid(),
					x,
					y,
					points: pts.map(([px, py]) => [px - x, py - y]),
					backgroundColor: closed ? style.backgroundColor : 'transparent',
					roundness: null,
				}
			})
	}

	private text(el: Element): Skeleton {
		this.done.add(el)
		const box = this.box(el)
		const size = fontSize(el)
		const text = textOf(el)
		const anchor = textAnchor(el)
		const lines = text.split('\n').length
		// Excalidraw places text by its alignment: x is the left edge, the
		// centre or the right edge, so Nunito's own width needs no guessing.
		const x = anchor === 'middle' ? box.x + box.w / 2 : anchor === 'end' ? box.x + box.w : box.x
		const height = lines * size * LINE_HEIGHT
		return {
			type: 'text',
			id: uid(),
			x,
			y: box.y + box.h / 2 - height / 2,
			text,
			fontSize: size,
			fontFamily: FONT_FAMILY,
			textAlign: anchor === 'middle' ? 'center' : anchor === 'end' ? 'right' : 'left',
			strokeColor: textColor(el),
			roughness: ROUGHNESS,
		}
	}

	// --- Styles -----------------------------------------------------------

	private shapeStyle(el: Element): Skeleton {
		const cs = getComputedStyle(el)
		const fill = color(cs.fill)
		const alpha = parseFloat(cs.fillOpacity || '1')
		let backgroundColor = 'transparent'
		if (fill && !NEUTRAL_FILLS.has(fill)) {
			backgroundColor =
				fill === PELE_SURFACE_ALT ? '#e9ecef' : NEUTRAL_STROKES.has(fill) ? INK : fill
			// A tint (mindmap branches, clusters): the color as seen on white.
			if (alpha < 1 && backgroundColor.startsWith('#')) backgroundColor = tint(backgroundColor, alpha)
		}
		return {
			...this.strokeStyle(el),
			backgroundColor,
			fillStyle: 'solid',
			opacity: Math.round(Math.min(1, parseFloat(cs.opacity || '1')) * 100),
			roughness: ROUGHNESS,
		}
	}

	private strokeStyle(el: Element) {
		const cs = getComputedStyle(el)
		const stroke = color(cs.stroke)
		const width = parseFloat(cs.strokeWidth || '1')
		const dash = (cs.strokeDasharray || 'none').split(/[ ,]+/).map(parseFloat)
		return {
			strokeColor: !stroke ? 'transparent' : NEUTRAL_STROKES.has(stroke) ? INK : stroke,
			strokeWidth: width > 1.5 ? 2 : 1,
			strokeStyle: isNaN(dash[0]) ? 'solid' : dash[0] <= 3 ? 'dotted' : 'dashed',
		}
	}

	// --- Geometry -----------------------------------------------------------

	/** The element's bounding box in the SVG's coordinates. */
	private box(el: Element): Box {
		const g = el as SVGGraphicsElement
		const b = g.getBBox()
		const m = this.matrix(g)
		const corners = [
			[b.x, b.y],
			[b.x + b.width, b.y],
			[b.x, b.y + b.height],
			[b.x + b.width, b.y + b.height],
		].map(([x, y]) => apply(m, x, y))
		const xs = corners.map((p) => p[0])
		const ys = corners.map((p) => p[1])
		const x = Math.min(...xs)
		const y = Math.min(...ys)
		return { x, y, w: Math.max(...xs) - x, h: Math.max(...ys) - y }
	}

	private matrix(el: SVGGraphicsElement): DOMMatrix {
		const root = this.svg.getCTM()
		const own = el.getCTM()
		if (!own) return new DOMMatrix()
		return root ? root.inverse().multiply(own) : own
	}

	/** A route's points: path vertices, curves sampled. */
	private points(el: SVGGraphicsElement): Point[] {
		return this.subpaths(el).flat()
	}

	private subpaths(el: Element): Point[][] {
		const m = this.matrix(el as SVGGraphicsElement)
		const tag = el.tagName
		let raw: Point[][]
		if (tag === 'line') {
			const n = (a: string) => parseFloat(el.getAttribute(a) ?? '0')
			raw = [
				[
					[n('x1'), n('y1')],
					[n('x2'), n('y2')],
				],
			]
		} else if (tag === 'polygon' || tag === 'polyline') {
			const nums = (el.getAttribute('points') ?? '').trim().split(/[\s,]+/).map(parseFloat)
			const pts: Point[] = []
			for (let i = 0; i + 1 < nums.length; i += 2) pts.push([nums[i], nums[i + 1]])
			if (tag === 'polygon' && pts.length) pts.push([...pts[0]] as Point)
			raw = [pts]
		} else if (tag === 'path') {
			raw = parsePath(el.getAttribute('d') ?? '')
		} else {
			const b = (el as SVGGraphicsElement).getBBox()
			raw = [
				[
					[b.x, b.y],
					[b.x + b.width, b.y],
					[b.x + b.width, b.y + b.height],
					[b.x, b.y + b.height],
					[b.x, b.y],
				],
			]
		}
		return raw.map((pts) => dedupe(pts.map(([x, y]) => apply(m, x, y))))
	}
}

// --- Helpers ----------------------------------------------------------------

function containerType(el: Element): string | null {
	switch (el.tagName) {
		case 'rect':
			return 'rectangle'
		case 'circle':
		case 'ellipse':
			return 'ellipse'
		case 'polygon': {
			const n = (el.getAttribute('points') ?? '').trim().split(/[\s,]+/).length / 2
			return n === 4 && isDiamond(el) ? 'diamond' : null
		}
		default:
			return null
	}
}

function isDiamond(el: Element): boolean {
	const nums = (el.getAttribute('points') ?? '').trim().split(/[\s,]+/).map(parseFloat)
	const xs = [nums[0], nums[2], nums[4], nums[6]]
	const ys = [nums[1], nums[3], nums[5], nums[7]]
	const cx = (Math.min(...xs) + Math.max(...xs)) / 2
	const cy = (Math.min(...ys) + Math.max(...ys)) / 2
	// Each vertex sits on the vertical or horizontal centre line.
	return xs.every((x, i) => Math.abs(x - cx) < 0.5 || Math.abs(ys[i] - cy) < 0.5)
}

function isRounded(el: Element, g: Element | null): boolean {
	if (el.tagName !== 'rect') return false
	const rx = parseFloat(el.getAttribute('rx') ?? '0')
	const styled = /rx\s*:/.test(el.getAttribute('style') ?? '')
	const cls = g?.getAttribute('class') ?? ''
	return rx > 0 || styled || /pele-shape-(rounded|stadium)/.test(cls)
}

/** Background rectangles behind labels (page color, no outline). */
function isMask(el: Element): boolean {
	if (el.tagName !== 'rect') return false
	const cs = getComputedStyle(el)
	return color(cs.fill) === PELE_BG && !color(cs.stroke)
}

function visible(el: Element): boolean {
	const cs = getComputedStyle(el)
	if (cs.display === 'none' || cs.visibility === 'hidden' || parseFloat(cs.opacity) === 0) {
		return false
	}
	if (el.tagName === 'g' || el.tagName === 'text') return true
	return !!color(cs.fill) || !!color(cs.stroke)
}

/** Excalidraw's arrowhead for the markers at one end of an edge. */
function arrowhead(markers: Element[]): string | null {
	const kinds = markers.flatMap(markerKinds)
	if (!kinds.length) return null
	// ER cardinality: a fork is "many", a bar "one"; the circle ("zero")
	// has no Excalidraw equivalent and is dropped.
	if (kinds.includes('crow')) return kinds.includes('bar') ? 'crowfoot_one_or_many' : 'crowfoot_many'
	if (kinds.includes('bar')) return 'crowfoot_one'
	return kinds[0] === 'circle' ? 'circle_outline' : kinds[0]
}

function markerKinds(m: Element): string[] {
	const cs = getComputedStyle(m)
	const fill = color(cs.fill)
	const filled = !!fill && fill !== PELE_BG
	if (m.tagName === 'circle' || m.tagName === 'ellipse') return [filled ? 'dot' : 'circle']
	const polygon = m.tagName === 'polygon' || m.tagName === 'polyline'
	const d = (polygon ? 'M' + m.getAttribute('points') : m.getAttribute('d')) ?? ''
	const subs = parsePath(d)
	const closed = m.tagName === 'polygon' || /[Zz]/.test(d)
	if (closed) {
		const corners = (subs[0]?.length ?? 1) - 1
		if (corners === 4) return [filled ? 'diamond' : 'diamond_outline']
		return [filled ? 'triangle' : 'triangle_outline']
	}
	// Open strokes: a lone segment is a bar, a "<" an arrow (or, on an ER
	// relationship, the crow's foot).
	const er = !!m.closest('[class*="pele-er"]')
	return subs.map((sub) => (sub.length === 2 ? 'bar' : er ? 'crow' : 'arrow'))
}

function textOf(el: Element): string {
	const spans = Array.from(el.querySelectorAll('tspan'))
	if (spans.length > 1) {
		const lines: string[] = []
		let lastY: string | null = null
		for (const s of spans) {
			const y = s.getAttribute('y') ?? s.getAttribute('dy')
			if (lines.length && y === lastY) lines[lines.length - 1] += s.textContent ?? ''
			else lines.push(s.textContent ?? '')
			lastY = y
		}
		return lines.map((l) => l.trim()).join('\n')
	}
	return (el.textContent ?? '').trim()
}

function fontSize(el: Element): number {
	const size = parseFloat(getComputedStyle(el).fontSize)
	return Number.isFinite(size) && size > 0 ? Math.round(size) : 16
}

function textColor(el: Element): string {
	const c = color(getComputedStyle(el).fill)
	return !c || NEUTRAL_STROKES.has(c) ? INK : c
}

/** A computed CSS color as #rrggbb; null for none / fully transparent. */
function color(value: string | null): string | null {
	if (!value || value === 'none') return null
	const m = value.match(/rgba?\(([^)]+)\)/)
	if (!m) return value.startsWith('#') ? value.toLowerCase() : null
	const [r, g, b, a] = m[1].split(/[\s,/]+/).map(parseFloat)
	if (a === 0) return null
	return '#' + [r, g, b].map((v) => Math.round(v).toString(16).padStart(2, '0')).join('')
}

function frame(b: Box) {
	return { id: uid(), x: b.x, y: b.y, width: b.w, height: b.h }
}

function pad(b: Box, by: number): Box {
	return { x: b.x - by, y: b.y - by, w: b.w + 2 * by, h: b.h + 2 * by }
}

function textAnchor(el: Element): string {
	const own = el.closest('[text-anchor]')?.getAttribute('text-anchor')
	return own || getComputedStyle(el).getPropertyValue('text-anchor') || 'start'
}

/** #rrggbb at `alpha` over white. */
function tint(hex: string, alpha: number): string {
	const n = parseInt(hex.slice(1, 7), 16)
	return (
		'#' +
		[n >> 16, (n >> 8) & 255, n & 255]
			.map((v) => Math.round(v * alpha + 255 * (1 - alpha)).toString(16).padStart(2, '0'))
			.join('')
	)
}

function distanceToRoute(p: Point, points: Point[]): number {
	let best = Infinity
	for (let i = 1; i < points.length; i++) {
		const [a, b] = [points[i - 1], points[i]]
		const dx = b[0] - a[0]
		const dy = b[1] - a[1]
		const len = dx * dx + dy * dy
		const t = len ? Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len)) : 0
		best = Math.min(best, dist(p, [a[0] + t * dx, a[1] + t * dy]))
	}
	return best
}

function apply(m: DOMMatrix, x: number, y: number): Point {
	return [m.a * x + m.c * y + m.e, m.b * x + m.d * y + m.f]
}

function center(b: Box): Point {
	return [b.x + b.w / 2, b.y + b.h / 2]
}

function dist(a: Point, b: Point): number {
	return Math.hypot(a[0] - b[0], a[1] - b[1])
}

function inside(inner: Box, outer: Box): boolean {
	const [cx, cy] = center(inner)
	return cx >= outer.x && cx <= outer.x + outer.w && cy >= outer.y && cy <= outer.y + outer.h
}

/** Distance from a point to a box's outline (0 on it, also when inside). */
function edgeDistance(p: Point, b: Box): number {
	const dx = Math.max(b.x - p[0], 0, p[0] - (b.x + b.w))
	const dy = Math.max(b.y - p[1], 0, p[1] - (b.y + b.h))
	if (dx || dy) return Math.hypot(dx, dy)
	return Math.min(p[0] - b.x, b.x + b.w - p[0], p[1] - b.y, b.y + b.h - p[1])
}

function dedupe(points: Point[]): Point[] {
	return points.filter((p, i) => i === 0 || dist(p, points[i - 1]) > 0.01)
}

const CURVE_STEPS = 8

/** SVG path data → subpaths of points, curves and arcs sampled. */
export function parsePath(d: string): Point[][] {
	const tokens = d.match(/[a-zA-Z]|-?(?:\d+\.?\d*|\.\d+)(?:e[-+]?\d+)?/g) ?? []
	const subs: Point[][] = []
	let cur: Point[] = []
	let x = 0
	let y = 0
	let sx = 0
	let sy = 0
	let cmd = ''
	let lastCtrl: Point | null = null
	let i = 0
	const num = () => parseFloat(tokens[i++])
	const push = (p: Point) => {
		cur.push(p)
		x = p[0]
		y = p[1]
	}
	const cubic = (c1: Point, c2: Point, e: Point) => {
		const s: Point = [x, y]
		for (let k = 1; k <= CURVE_STEPS; k++) {
			const t = k / CURVE_STEPS
			const u = 1 - t
			push([
				u * u * u * s[0] + 3 * u * u * t * c1[0] + 3 * u * t * t * c2[0] + t * t * t * e[0],
				u * u * u * s[1] + 3 * u * u * t * c1[1] + 3 * u * t * t * c2[1] + t * t * t * e[1],
			])
		}
		lastCtrl = c2
	}
	while (i < tokens.length) {
		if (/[a-zA-Z]/.test(tokens[i])) cmd = tokens[i++]
		const rel = cmd === cmd.toLowerCase()
		const ox = rel ? x : 0
		const oy = rel ? y : 0
		switch (cmd.toUpperCase()) {
			case 'M': {
				if (cur.length) subs.push(cur)
				cur = []
				const p: Point = [num() + ox, num() + oy]
				push(p)
				sx = p[0]
				sy = p[1]
				cmd = rel ? 'l' : 'L'
				lastCtrl = null
				break
			}
			case 'L':
				push([num() + ox, num() + oy])
				lastCtrl = null
				break
			case 'H':
				push([num() + ox, y])
				lastCtrl = null
				break
			case 'V':
				push([x, num() + oy])
				lastCtrl = null
				break
			case 'C': {
				const c1: Point = [num() + ox, num() + oy]
				const c2: Point = [num() + ox, num() + oy]
				cubic(c1, c2, [num() + ox, num() + oy])
				break
			}
			case 'S': {
				const c1: Point = lastCtrl ? [2 * x - lastCtrl[0], 2 * y - lastCtrl[1]] : [x, y]
				const c2: Point = [num() + ox, num() + oy]
				cubic(c1, c2, [num() + ox, num() + oy])
				break
			}
			case 'Q': {
				const q: Point = [num() + ox, num() + oy]
				const e: Point = [num() + ox, num() + oy]
				cubic([x + (2 / 3) * (q[0] - x), y + (2 / 3) * (q[1] - y)], [e[0] + (2 / 3) * (q[0] - e[0]), e[1] + (2 / 3) * (q[1] - e[1])], e)
				break
			}
			case 'T':
				push([num() + ox, num() + oy])
				break
			case 'A': {
				const [rx, ry, rot, large, sweep] = [num(), num(), num(), num(), num()]
				const e: Point = [num() + ox, num() + oy]
				for (const p of arc([x, y], rx, ry, rot, large, sweep, e)) push(p)
				lastCtrl = null
				break
			}
			case 'Z':
				push([sx, sy])
				lastCtrl = null
				// Numbers can't follow Z; drop any rather than loop on them.
				cmd = ''
				break
			default:
				i++
		}
	}
	if (cur.length) subs.push(cur)
	return subs
}

/** Points along an SVG elliptical arc (endpoint form), ending at `e`. */
function arc(s: Point, rx: number, ry: number, rotDeg: number, large: number, sweep: number, e: Point): Point[] {
	rx = Math.abs(rx)
	ry = Math.abs(ry)
	if (!rx || !ry || dist(s, e) < 1e-9) return [e]
	const phi = (rotDeg * Math.PI) / 180
	const cos = Math.cos(phi)
	const sin = Math.sin(phi)
	const dx = (s[0] - e[0]) / 2
	const dy = (s[1] - e[1]) / 2
	const x1 = cos * dx + sin * dy
	const y1 = -sin * dx + cos * dy
	const scale = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry)
	if (scale > 1) {
		rx *= Math.sqrt(scale)
		ry *= Math.sqrt(scale)
	}
	const sign = large === sweep ? -1 : 1
	const num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1
	const den = rx * rx * y1 * y1 + ry * ry * x1 * x1
	const co = sign * Math.sqrt(Math.max(0, num / den))
	const cx1 = (co * rx * y1) / ry
	const cy1 = (-co * ry * x1) / rx
	const cx = cos * cx1 - sin * cy1 + (s[0] + e[0]) / 2
	const cy = sin * cx1 + cos * cy1 + (s[1] + e[1]) / 2
	const angle = (ux: number, uy: number, vx: number, vy: number) =>
		Math.atan2(ux * vy - uy * vx, ux * vx + uy * vy)
	const t1 = angle(1, 0, (x1 - cx1) / rx, (y1 - cy1) / ry)
	let dt = angle((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry)
	if (!sweep && dt > 0) dt -= 2 * Math.PI
	if (sweep && dt < 0) dt += 2 * Math.PI
	const steps = Math.max(2, Math.ceil((Math.abs(dt) / (Math.PI / 2)) * CURVE_STEPS))
	const out: Point[] = []
	for (let k = 1; k <= steps; k++) {
		const t = t1 + (dt * k) / steps
		const px = rx * Math.cos(t)
		const py = ry * Math.sin(t)
		out.push([cos * px - sin * py + cx, sin * px + cos * py + cy])
	}
	out[out.length - 1] = e
	return out
}
