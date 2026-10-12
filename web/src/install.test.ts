import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

/*
 * What a phone reads to put austeris on its home screen. Nothing renders the
 * manifest in a test, so it is held here: a start page the app does not
 * answer, an icon that is not in the build, or a status-bar colour that
 * parted from the page's are all found only on a phone otherwise.
 */

// Paths from this file's directory rather than `new URL(..., import.meta.url)`,
// which Vite rewrites into an address on its server for anything in public/.
const SRC = dirname(fileURLToPath(import.meta.url))
const PUBLIC = join(SRC, '..', 'public')
const read = (...path: string[]) => readFileSync(join(SRC, ...path), 'utf8')
const manifest = JSON.parse(read('..', 'public', 'manifest.webmanifest')) as {
  start_url: string
  theme_color: string
  background_color: string
  icons: { src: string; purpose: string }[]
}
const html = read('..', 'index.html')

describe('installing on a phone', () => {
  it('opens onto a screen the app answers', () => {
    expect(read('App.tsx')).toContain(`path="${manifest.start_url}"`)
  })

  it('has every icon it names, and one a launcher may cut its own shape from', () => {
    for (const icon of manifest.icons) {
      expect(existsSync(join(PUBLIC, icon.src)), icon.src).toBe(true)
    }
    expect(manifest.icons.some((icon) => icon.purpose === 'maskable')).toBe(true)
    expect(html).toContain('href="/manifest.webmanifest"')
    expect(existsSync(join(PUBLIC, 'apple-touch-icon.png'))).toBe(true)
  })

  it('colours the status bar as the page is coloured in the dark', () => {
    const dark = html.match(/media="\(prefers-color-scheme: dark\)" content="(#[0-9a-f]{6})"/)?.[1]
    expect(dark).toBe(manifest.theme_color)
    expect(manifest.background_color).toBe(manifest.theme_color)
  })
})
