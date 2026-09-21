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

// The helper can die outright (Sass load paths reaching the filesystem), so
// it is restarted and that one case dropped.
let helper, resolveLine, buf
function start() {
  buf = ''
  helper = spawn(process.execPath, [path.resolve('tools/reference-helper.cjs')], {
    stdio: ['pipe', 'pipe', 'ignore'],
    cwd: path.resolve('tools'),
  })
  helper.stdout.setEncoding('utf8')
  helper.stdout.on('data', chunk => {
    buf += chunk
    let i
    while ((i = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, i)
      buf = buf.slice(i + 1)
      resolveLine({ ok: JSON.parse(line) })
    }
  })
  helper.on('exit', () => resolveLine && resolveLine({ crashed: true }))
}
start()
const run = source =>
  new Promise(resolve => {
    resolveLine = resolve
    helper.stdin.write(JSON.stringify({ kind: 'vue', source, path: 'component.vue' }) + '\n')
  })

const results = []
let crashed = 0
for (const source of sources) {
  const r = await run(source)
  if (r.crashed) {
    crashed++
    start()
    continue
  }
  results.push({ source, path: 'component.vue', out: r.ok })
}
resolveLine = null
helper.stdin.end()
if (crashed) console.log(`${crashed} cases crashed the reference helper, dropped`)
fs.writeFileSync(out, JSON.stringify(results))
console.log(`wrote ${results.length} cases to ${out}`)
