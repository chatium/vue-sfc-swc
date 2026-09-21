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

// --- SFC descriptor parse ---------------------------------------------------
const sfcSources = JSON.parse(fs.readFileSync('tests/corpus/sfc.json', 'utf8'))
const sfcApi = require('@vue/compiler-sfc')

function block(b) {
  if (!b) return null
  return {
    type: b.type,
    content: b.content,
    attrs: b.attrs,
    lang: b.lang ?? null,
    src: b.src ?? null,
    scoped: b.scoped ?? false,
    module: b.module ?? null,
    setup: b.setup ?? null,
    loc: clean(b.loc),
  }
}

const sfcOut = []
for (const source of sfcSources) {
  let r
  try {
    r = sfcApi.parse(source, { filename: 'anonymous.vue', sourceMap: false })
  } catch (e) {
    continue
  }
  const d = r.descriptor
  sfcOut.push({
    input: source,
    descriptor: {
      template: block(d.template),
      script: block(d.script),
      scriptSetup: block(d.scriptSetup),
      styles: d.styles.map(block),
      customBlocks: d.customBlocks.map(block),
      cssVars: d.cssVars,
      slotted: d.slotted,
    },
    errors: r.errors.map(e => ({
      code: e.code ?? null,
      message: e.message,
      loc: e.loc ? clean(e.loc) : null,
    })),
  })
}
fs.writeFileSync('tests/fixtures/sfc-parse.json', JSON.stringify(sfcOut))
console.log(`sfc-parse: ${sfcOut.length}`)
