// Harvests template / SFC sources from the official vuejs/core test suite.
// Usage: node tools/extract-corpus.mjs <path-to-vuejs-core-checkout>
import fs from 'node:fs'
import path from 'node:path'

const root = process.argv[2]
if (!root) {
  console.error('usage: node tools/extract-corpus.mjs <vuejs/core checkout>')
  process.exit(1)
}

const dirs = [
  'packages/compiler-core/__tests__',
  'packages/compiler-dom/__tests__',
  'packages/compiler-sfc/__tests__',
]

function walk(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name)
    if (e.isDirectory()) walk(p, out)
    else if (/\.(spec\.ts|vue)$/.test(e.name)) out.push(p)
  }
  return out
}

/** Pull out every string literal (template/single/double) that looks like markup. */
function literals(src) {
  const out = []
  for (let i = 0; i < src.length; i++) {
    const q = src[i]
    if (q !== '`' && q !== "'" && q !== '"') continue
    // skip comments crudely: not worth it, false positives are filtered later
    let j = i + 1
    let raw = ''
    let interpolated = false
    let closed = false
    while (j < src.length) {
      const c = src[j]
      if (c === '\\') {
        raw += src[j] + src[j + 1]
        j += 2
        continue
      }
      if (q === '`' && c === '$' && src[j + 1] === '{') interpolated = true
      if (q !== '`' && (c === '\n' || c === '\r')) break
      if (c === q) {
        closed = true
        break
      }
      raw += c
      j++
    }
    if (!closed) continue
    i = j
    if (interpolated) continue
    out.push({ quote: q, raw })
  }
  return out
}

function unescape(quote, raw) {
  try {
    return JSON.parse(
      quote === '"' ? `"${raw}"` : `"${raw.replace(/"/g, '\\"').replace(/\\'/g, "'").replace(/\\`/g, '`')}"`,
    )
  } catch {
    return null
  }
}

const templates = new Set()
const sfcs = new Set()

for (const dir of dirs) {
  const abs = path.join(root, dir)
  if (!fs.existsSync(abs)) continue
  for (const file of walk(abs)) {
    const src = fs.readFileSync(file, 'utf8')
    if (file.endsWith('.vue')) {
      sfcs.add(src)
      continue
    }
    for (const { quote, raw } of literals(src)) {
      const s = unescape(quote, raw)
      if (s == null) continue
      if (!/[<]|\{\{/.test(s)) continue
      if (s.length > 4000) continue
      if (/^\s*(https?:|\/\/)/.test(s)) continue
      if (/<(template|script|style)[\s>]/.test(s)) sfcs.add(s)
      else templates.add(s)
    }
  }
}

fs.mkdirSync('tests/corpus', { recursive: true })
fs.writeFileSync('tests/corpus/templates.json', JSON.stringify([...templates], null, 0))
fs.writeFileSync('tests/corpus/sfc.json', JSON.stringify([...sfcs], null, 0))
console.log(`templates: ${templates.size}, sfc: ${sfcs.size}`)
