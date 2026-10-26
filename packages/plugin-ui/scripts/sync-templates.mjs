import { mkdir, readFile, writeFile } from 'node:fs/promises'
const check = process.argv.slice(2)
if (check.some((arg) => arg !== '--check') || check.length > 1) {
  console.error('Usage: node packages/plugin-ui/scripts/sync-templates.mjs [--check]')
  process.exitCode = 1
} else {
  const source = new URL('../src/', import.meta.url)
  const target = new URL('../../../examples/public-plugins/com.uipilot.templates/package/dist/ui/', import.meta.url)
  for (const file of ['ui.js', 'ui.css', 'form.js', 'action-panel.js']) {
    const bytes = await readFile(new URL(file, source))
    if (check.length) {
      const copy = await readFile(new URL(file, target)).catch((error) => {
        if (error.code !== 'ENOENT') throw error
        return null
      })
      if (!copy?.equals(bytes)) {
        console.error('UI copy differs: templates/package/dist/ui/' + file + '; run sync-templates.mjs')
        process.exitCode = 1
      }
    } else {
      await mkdir(target, { recursive: true })
      await writeFile(new URL(file, target), bytes)
    }
  }
}
