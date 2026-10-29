// Sawit's design tokens, sent by the app (crates/app/src/canvas.rs reads the
// active GPUI theme), mapped onto Excalidraw's CSS variables in canvas.css.
// The app re-sends them whenever its theme changes.

export type SawitTheme = {
	mode: 'dark' | 'light'
	font: string
	monoFont: string
	radius: number
	radiusLg: number
	shadow: boolean
	colors: Record<string, string>
}

// One Dark, used if the app sent nothing (e.g. the page opened on its own).
export const FALLBACK: SawitTheme = {
	mode: 'dark',
	font: '.SystemUIFont',
	monoFont: 'Menlo',
	radius: 4,
	radiusLg: 6,
	shadow: false,
	colors: {
		background: '#282c33',
		foreground: '#dce0e5',
		border: '#363c46',
		muted: '#2e343e',
		mutedForeground: '#a9afbc',
		popover: '#2f343e',
		popoverForeground: '#dce0e5',
		primary: '#74ade8',
		primaryForeground: '#282c33',
		primaryHover: '#85c1ff',
		primaryActive: '#5f9ad6',
		secondary: '#2e343e',
		secondaryHover: '#363c46',
		secondaryActive: '#454a56',
		input: '#464b57',
		ring: '#47679e',
		selection: '#74ade83d',
		link: '#74ade8',
		danger: '#d07277',
		dangerForeground: '#282c33',
		scrollbarThumb: '#3f4550',
		scrollbarThumbHover: '#4d5360',
	},
}

/** GPUI's family names → a CSS font stack. */
function fontStack(family: string, fallback: string): string {
	if (!family || family.startsWith('.') || family === 'system-ui') return fallback
	return `"${family.replace(/"/g, '')}", ${fallback}`
}

const SYSTEM = '-apple-system, BlinkMacSystemFont, system-ui, sans-serif'
const MONO = 'ui-monospace, Menlo, monospace'

export function applyTheme(theme: SawitTheme) {
	const root = document.documentElement.style
	for (const [name, value] of Object.entries(theme.colors)) {
		root.setProperty('--sawit-' + name.replace(/[A-Z]/g, (c) => '-' + c.toLowerCase()), value)
	}
	root.setProperty('--sawit-font', fontStack(theme.font, SYSTEM))
	root.setProperty('--sawit-mono-font', fontStack(theme.monoFont, MONO))
	root.setProperty('--sawit-radius', theme.radius + 'px')
	root.setProperty('--sawit-radius-lg', theme.radiusLg + 'px')
	document.documentElement.dataset.sawitShadow = String(theme.shadow)
	document.documentElement.dataset.sawitMode = theme.mode
}

/**
 * The canvas background to store so that what shows on screen is Sawit's
 * background. In dark mode Excalidraw draws the canvas through
 * `invert(93%) hue-rotate(180deg)` (so light-mode drawings read well on
 * dark); this undoes that filter.
 */
export function canvasBackground(theme: SawitTheme): string {
	const bg = theme.colors.background ?? FALLBACK.colors.background
	if (theme.mode !== 'dark') return bg
	const target = hexToRgb(bg)
	// hue-rotate(180deg) as the CSS matrix, applied after invert(p).
	const m = [
		[-0.574, 1.43, 0.144],
		[0.426, 0.43, 0.144],
		[0.426, 1.43, -0.856],
	]
	const p = 0.93
	const inv = invert3(m)
	const afterInvert = inv.map((row) => row[0] * target[0] + row[1] * target[1] + row[2] * target[2])
	const source = afterInvert.map((v) => clamp01((v - p) / (1 - 2 * p)))
	return rgbToHex(source)
}

function hexToRgb(hex: string): number[] {
	const h = hex.replace('#', '')
	return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255)
}

function rgbToHex(rgb: number[]): string {
	return '#' + rgb.map((v) => Math.round(v * 255).toString(16).padStart(2, '0')).join('')
}

function clamp01(v: number) {
	return Math.min(1, Math.max(0, v))
}

function invert3(m: number[][]): number[][] {
	const [[a, b, c], [d, e, f], [g, h, i]] = m
	const A = e * i - f * h
	const B = -(d * i - f * g)
	const C = d * h - e * g
	const det = a * A + b * B + c * C
	return [
		[A / det, -(b * i - c * h) / det, (b * f - c * e) / det],
		[B / det, (a * i - c * g) / det, -(a * f - c * d) / det],
		[C / det, -(a * h - b * g) / det, (a * e - b * d) / det],
	]
}
