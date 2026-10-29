// Brotli-compresses the bundle in place: every file except the (already
// compressed) fonts becomes `<name>.br`, which the app decodes when the
// webview asks for `<name>` (see `serve` in crates/app/src/canvas.rs).
// 9 MB embedded raw becomes under 3 MB. Run by build.mjs; on its own:
// `node compress.mjs`.
import { brotliCompressSync, constants } from 'node:zlib'
import { readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'

export function compress(dir) {
	for (const entry of readdirSync(dir, { recursive: true, withFileTypes: true })) {
		const name = entry.name
		if (!entry.isFile() || name.endsWith('.woff2') || name.endsWith('.br')) continue
		const path = `${entry.parentPath}/${name}`
		const raw = readFileSync(path)
		const packed = brotliCompressSync(raw, {
			params: {
				[constants.BROTLI_PARAM_MODE]: constants.BROTLI_MODE_TEXT,
				[constants.BROTLI_PARAM_QUALITY]: constants.BROTLI_MAX_QUALITY,
				[constants.BROTLI_PARAM_LGWIN]: constants.BROTLI_MAX_WINDOW_BITS,
				[constants.BROTLI_PARAM_SIZE_HINT]: raw.length,
			},
		})
		writeFileSync(path + '.br', packed)
		rmSync(path)
	}
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
	compress(new URL('../../crates/app/canvas/', import.meta.url).pathname)
}
