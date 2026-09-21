// Runs the reference @vue/compiler-sfc over the corpus and records expectations.
import fs from 'node:fs'
import { createRequire } from 'node:module'
const require = createRequire(import.meta.url)
const core = require('@vue/compiler-core')

const templates = JSON.parse(fs.readFileSync('tests/corpus/templates.json', 'utf8'))

function clean(value) {
  // drop babel ASTs and Sets, which we do not model
  return JSON.parse(
    JSON.stringify(value, (k, v) => {
      if (k === 'ast') return undefined
      if (v instanceof Set) return {}
      return v
    }),
  )
}

const out = []
for (const input of templates) {
  const errors = []
  let ast
  try {
    ast = core.baseParse(input, { onError: e => errors.push(e) })
  } catch (e) {
    continue
  }
  out.push({
    input,
    ast: clean(ast),
    errors: errors.map(e => ({ code: e.code, message: e.message, loc: clean(e.loc) })),
  })
}

fs.mkdirSync('tests/fixtures', { recursive: true })
fs.writeFileSync('tests/fixtures/parse-base.json', JSON.stringify(out))
console.log(`parse-base: ${out.length}`)
