// Differential check of the SSR output over a directory tree of real .vue
// files: `compileTemplate({ ssr: true })` and the inline SSR render function.
import fs from 'node:fs'
import path from 'node:path'
import { createRequire } from 'node:module'

const require = createRequire(path.resolve('tools/reference-helper.cjs'))
const vue = require('@vue/compiler-sfc')

const root = process.argv[2]
const out = process.argv[3]
if (!root || !out) {
  console.error('usage: node tools/check-dir-ssr.mjs <dir> <out.json>')
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

const results = []
for (const source of sources) {
  const out1 = { source }
  // compileScript runs first: it is what the real pipeline does, and
  // compileTemplate transforms the descriptor's AST in place
  let d
  try {
    d = vue.parse(source, { filename: '/component.vue', sourceMap: false }).descriptor
  } catch (e) {
    out1.parseError = String(e.message || e)
    results.push(out1)
    continue
  }
  const expressionPlugins =
    d.script?.lang?.startsWith('ts') || d.scriptSetup?.lang?.startsWith('ts')
      ? ['typescript']
      : undefined

  if (d.script || d.scriptSetup) {
    try {
      const r = vue.compileScript(d, {
        id: 'someid',
        inlineTemplate: true,
        sourceMap: false,
        templateOptions: { ssr: true },
      })
      out1.script = r.content
    } catch (e) {
      out1.scriptThrow = String(e.message || e)
    }
  }
  if (d.template && !d.template.src) {
    try {
      const r = vue.compileTemplate({
        source: d.template.content,
        ast: d.template.ast,
        filename: 'component.vue',
        id: 'someid',
        ssr: true,
        ssrCssVars: d.cssVars,
        compilerOptions: { expressionPlugins },
      })
      out1.template = r.code
      out1.templateErrors = r.errors.map(e => (typeof e === 'string' ? e : e.message))
    } catch (e) {
      out1.templateThrow = String(e.message || e)
    }
  }
  results.push(out1)
}
fs.writeFileSync(out, JSON.stringify(results))
console.log(`${files.length} files, ${sources.length} unique -> ${out}`)
