// Bundles the canvas into crates/app/canvas (embedded by the app with
// include_dir). Run `npm run build` here after changing src/ or bumping
// Excalidraw. Excalidraw is MIT-licensed: no key, no watermark.
import { build } from 'esbuild'
import { cpSync, mkdirSync, readdirSync, rmSync } from 'node:fs'

const out = new URL('../../crates/app/canvas/', import.meta.url).pathname
rmSync(out, { recursive: true, force: true })
mkdirSync(out, { recursive: true })

await build({
	entryPoints: { canvas: 'src/main.tsx' },
	bundle: true,
	format: 'esm',
	target: 'safari16',
	minify: true,
	// Lazy features (Mermaid, export, …) load only when used.
	splitting: true,
	chunkNames: 'chunks/[name]-[hash]',
	assetNames: 'assets/[name]-[hash]',
	jsx: 'automatic',
	outdir: out,
	legalComments: 'none',
	define: { 'process.env.NODE_ENV': '"production"', 'process.env.IS_PREACT': '"false"' },
	loader: { '.woff2': 'file', '.ttf': 'file' },
	conditions: ['production'],
	logLevel: 'info',
})

cpSync('src/index.html', out + 'index.html')
// Every font except Xiaolai, the 12 MB CJK fallback: Chinese, Japanese and
// Korean text uses the system's fonts instead.
const fonts = 'node_modules/@excalidraw/excalidraw/dist/prod/fonts/'
for (const family of readdirSync(fonts)) {
	if (family !== 'Xiaolai') {
		cpSync(fonts + family, out + 'fonts/' + family, { recursive: true })
	}
}
