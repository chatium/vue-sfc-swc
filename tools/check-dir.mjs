// Differential check over a directory tree of real .vue files, without
// vendoring them: runs the reference helper, writes a throwaway fixture file.
import fs from 'node:fs'
import path from 'node:path'
import { spawn } from 'node:child_process'

const root = process.argv[2]
const out = process.argv[3] || 'tests/fixtures/ugc-vue-local.json'
if (!root) {
  console.error('usage: node tools/check-dir.mjs <dir> [out.json]')
  process.exit(1)
}

const files = []
;(function walk(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    if (e.name === 'node_modules' || e.name.startsWith('.')) continue
    const p = path.join(dir, e.name)
    if (e.isDirectory()) walk(p)
    else if (e.name.endsWith('.vue')) files.push(p)
  }
})(root)

const seen = new Set()
const sources = []
for (const f of files) {
  const source = fs.readFileSync(f, 'utf8')
  if (seen.has(source)) continue
  seen.add(source)
  sources.push(source)
}
console.log(`${files.length} files, ${sources.length} unique`)

const helper = spawn(process.execPath, [path.resolve('tools/reference-helper.cjs')], {
  stdio: ['pipe', 'pipe', 'inherit'],
  cwd: path.resolve('tools'),
})
let resolveLine
let buf = ''
helper.stdout.setEncoding('utf8')
helper.stdout.on('data', chunk => {
  buf += chunk
  let i
  while ((i = buf.indexOf('\n')) >= 0) {
    const line = buf.slice(0, i)
    buf = buf.slice(i + 1)
    resolveLine(JSON.parse(line))
  }
})
const run = (source, filePath) =>
  new Promise(resolve => {
    resolveLine = resolve
    helper.stdin.write(JSON.stringify({ kind: 'vue', source, path: filePath }) + '\n')
  })

const results = []
for (const source of sources) {
  results.push({ source, path: 'component.vue', out: await run(source, 'component.vue') })
}
helper.stdin.end()
fs.writeFileSync(out, JSON.stringify(results))
console.log(`wrote ${results.length} cases to ${out}`)
