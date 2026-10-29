// Bundles the canvas into crates/app/canvas (embedded by the app with
// include_dir). Run `npm run build` here after changing src/ or bumping
// Excalidraw. Excalidraw is MIT-licensed: no key, no watermark.
import { build } from 'esbuild'
import { cpSync, mkdirSync, readdirSync, rmSync } from 'node:fs'
import { compress } from './compress.mjs'

const out = new URL('../../crates/app/canvas/', import.meta.url).pathname

// The canvas is English only (main.tsx passes langCode="en" and shows no
// language picker), so Excalidraw's other 54 translations would never load:
// each becomes an empty module instead of a 10–75 KB chunk.
const englishOnly = {
	name: 'english-only',
	setup(b) {
		b.onResolve({ filter: /^\.\/locales\/[^/]+\.js$/ }, (args) => {
			const keep = /\/(en|percentages)-[^/]+$/.test(args.path)
			if (keep || !args.importer.includes('@excalidraw/excalidraw')) return undefined
			return { path: args.path, namespace: 'no-locale' }
		})
		b.onLoad({ filter: /.*/, namespace: 'no-locale' }, () => ({ contents: 'export default {}', loader: 'js' }))
	},
}
rmSync(out, { recursive: true, force: true })
mkdirSync(out, { recursive: true })

await build({
	entryPoints: { canvas: 'src/main.tsx' },
	bundle: true,
	format: 'esm',
	target: 'safari16',
	minify: true,
	// Lazy features (export, Mermaid import, …) load only when used.
	splitting: true,
	chunkNames: 'chunks/[name]-[hash]',
	assetNames: 'assets/[name]-[hash]',
	jsx: 'automatic',
	outdir: out,
	legalComments: 'none',
	define: { 'process.env.NODE_ENV': '"production"', 'process.env.IS_PREACT': '"false"' },
	loader: { '.woff2': 'file', '.ttf': 'file' },
	conditions: ['production'],
	// Mermaid → shapes with Pele (src/pele.ts) instead of mermaid.js.
	alias: { '@excalidraw/mermaid-to-excalidraw': './src/pele.ts' },
	plugins: [englishOnly],
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
compress(out)
