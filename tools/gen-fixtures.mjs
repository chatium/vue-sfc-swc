// Runs the reference @vue/compiler-sfc over the corpus and records expectations.
import fs from 'node:fs'
import { createRequire } from 'node:module'
const require = createRequire(import.meta.url)
const core = require('@vue/compiler-core')

const templates = [
  ...JSON.parse(fs.readFileSync('tests/corpus/templates.json', 'utf8')),
  ...JSON.parse(fs.readFileSync('tests/corpus/extra.json', 'utf8')),
]

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
const sfcSources = [
  ...JSON.parse(fs.readFileSync('tests/corpus/sfc.json', 'utf8')),
  ...JSON.parse(fs.readFileSync('tests/corpus/sfc-extra.json', 'utf8')),
]
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

// --- compiler-dom compile (client render function) ---------------------------
const dom = require('@vue/compiler-dom')
const compileOut = []
for (const input of templates) {
  const errors = []
  let code = null
  try {
    const r = dom.compile(input, {
      mode: 'module',
      prefixIdentifiers: true,
      hoistStatic: true,
      cacheHandlers: true,
      sourceMap: false,
      filename: 'template.vue.html',
      onError: e => errors.push(e),
      onWarn: () => {},
    })
    code = r.code
  } catch (e) {
    continue
  }
  compileOut.push({
    input,
    code,
    errors: errors.map(e => ({ code: e.code ?? null, message: e.message })),
  })
}
fs.writeFileSync('tests/fixtures/compile-dom.json', JSON.stringify(compileOut))
console.log(`compile-dom: ${compileOut.length}`)

// --- compileTemplate (asset-url transforms on by default) --------------------
const tmplOut = []
for (const input of templates) {
  for (const scoped of [false, true]) {
    let r
    try {
      r = sfcApi.compileTemplate({
        source: input,
        filename: 'anonymous.vue',
        id: 'someid',
        scoped,
      })
    } catch (e) {
      continue
    }
    tmplOut.push({
      input,
      scoped,
      code: r.code,
      errors: r.errors.map(e => (typeof e === 'string' ? e : e.message)),
    })
  }
}
fs.writeFileSync('tests/fixtures/compile-template.json', JSON.stringify(tmplOut))
console.log(`compile-template: ${tmplOut.length}`)

// --- rewriteDefault ---------------------------------------------------------
const rdCases = JSON.parse(fs.readFileSync('tests/corpus/rewrite-default.json', 'utf8'))
const rdOut = []
for (const input of rdCases) {
  for (const ts of [false, true]) {
    let out
    try {
      out = sfcApi.rewriteDefault(input, '__sfc__', ts ? ['typescript'] : undefined)
    } catch (e) {
      continue
    }
    rdOut.push({ input, ts, output: out })
  }
}
fs.writeFileSync('tests/fixtures/rewrite-default.json', JSON.stringify(rdOut))
console.log(`rewrite-default: ${rdOut.length}`)

// --- compileScript ----------------------------------------------------------
const scriptOut = []
for (const source of sfcSources) {
  let descriptor
  try {
    descriptor = sfcApi.parse(source, { filename: 'anonymous.vue', sourceMap: false }).descriptor
  } catch {
    continue
  }
  if (!descriptor.script && !descriptor.scriptSetup) continue
  for (const inlineTemplate of [false, true]) {
    let out
    try {
      const r = sfcApi.compileScript(descriptor, {
        id: 'xxxxxxxx',
        inlineTemplate,
        sourceMap: false,
      })
      out = { content: r.content, bindings: r.bindings ? { ...r.bindings } : null }
    } catch (e) {
      out = { error: String(e.message || e) }
    }
    scriptOut.push({ input: source, inlineTemplate, ...out })
  }
}
fs.writeFileSync('tests/fixtures/compile-script.json', JSON.stringify(scriptOut))
console.log(`compile-script: ${scriptOut.length}`)

// --- postcss round-trip -----------------------------------------------------
const postcss = require('postcss')
const cssCases = JSON.parse(fs.readFileSync('tests/corpus/css.json', 'utf8'))
const cssOut = []
for (const input of cssCases) {
  try {
    const root = postcss.parse(input)
    cssOut.push({ input, output: root.toString() })
  } catch (e) {
    cssOut.push({ input, error: String(e.message) })
  }
}
fs.writeFileSync('tests/fixtures/postcss-roundtrip.json', JSON.stringify(cssOut))
console.log(`postcss-roundtrip: ${cssOut.length}`)

// --- selector round-trip ----------------------------------------------------
const selectorParser = require('@vue/compiler-sfc/dist/compiler-sfc.cjs.js') && null
const selCases = JSON.parse(fs.readFileSync('tests/corpus/selectors.json', 'utf8'))
const selOut = []
for (const input of selCases) {
  // round-trip through the same selector parser Vue uses, via a no-op scoped run
  let out
  try {
    out = sfcApi.compileStyle({
      source: `${input} { color: red }`,
      filename: 'a.vue',
      id: 'data-v-xxxxxxxx',
      scoped: true,
      trim: false,
    }).code
  } catch (e) {
    out = `ERROR: ${e.message}`
  }
  selOut.push({ input, scoped: out })
}
fs.writeFileSync('tests/fixtures/selector-scoped.json', JSON.stringify(selOut))
console.log(`selector-scoped: ${selOut.length}`)

// --- compileStyle -----------------------------------------------------------
const styleOut = []
const styleCases = [
  ...cssCases,
  ...selCases.map(s => `${s} { color: red }`),
  '.a { color: v-bind(color) }',
  '.a { color: v-bind("a.b") }',
  '.a { color: v-bind(\'x\') }',
  '@keyframes spin { from { transform: rotate(0) } }\n.a { animation: spin 1s }',
  '@keyframes spin { }\n.a { animation-name: spin }',
  '@-webkit-keyframes spin { }\n.a { -webkit-animation-name: spin }',
  '.a {\n  color: red;\n  .b { color: blue }\n}',
  '.a { &:hover { color: red } }',
  '@media (min-width: 1px) { .a { color: red } }',
  '.a :deep(.b) { color: red }',
  '.a:deep(.b) .c { color: red }',
  ':is(.a, .b :deep(.c)) .d { color: red }',
  ':not(.a, .b :deep(.c)) .d { color: red }',
  '.a ::v-deep .b { color: red }',
  '::v-slotted(.a) { color: red }',
  '.a { color: red }\n\n\n.b { color: blue }\n',
]
for (const source of styleCases) {
  for (const scoped of [false, true]) {
    for (const trim of [true, false]) {
      let out
      try {
        const r = sfcApi.compileStyle({
          source,
          filename: 'a.vue',
          id: 'data-v-xxxxxxxx',
          scoped,
          trim,
        })
        out = r.errors.length ? { error: String(r.errors[0].message || r.errors[0]) } : { code: r.code }
      } catch (e) {
        out = { error: String(e.message || e) }
      }
      styleOut.push({ source, scoped, trim, ...out })
    }
  }
}
fs.writeFileSync('tests/fixtures/compile-style.json', JSON.stringify(styleOut))
console.log(`compile-style: ${styleOut.length}`)

// --- sass / scss preprocessing ---------------------------------------------
const sassCases = [
  ['scss', '$c: red;\n.a { color: $c; }'],
  ['scss', '.a { .b { color: red } }'],
  ['scss', '.a { &:hover { color: red } }'],
  ['scss', '@mixin m { color: red }\n.a { @include m; }'],
  ['scss', '$m: (a: 1, b: 2);\n.a { width: map-get($m, a) * 1px }'],
  ['scss', '.a { width: 1px + 2px }'],
  ['scss', '@for $i from 1 through 3 { .c-#{$i} { width: $i * 1px } }'],
  ['scss', '.a { color: rgba(0,0,0,.5) }'],
  ['scss', '// line comment\n.a { color: red }'],
  ['scss', '/* block */\n.a { color: red }'],
  ['scss', '.a { @media (min-width: 1px) { color: red } }'],
  ['scss', '%p { color: red }\n.a { @extend %p; }'],
  ['sass', '.a\n  color: red'],
  ['sass', '$c: blue\n.a\n  color: $c'],
  ['sass', '.a\n  .b\n    color: red'],
  ['scss', '.a { color: v-bind(color) }'],
]
const sassOut = []
for (const [lang, source] of sassCases) {
  let out
  try {
    const r = sfcApi.compileStyle({
      source,
      filename: 'a.vue',
      id: 'data-v-xxxxxxxx',
      scoped: false,
      preprocessLang: lang,
      preprocessCustomRequire: id => require(id),
    })
    out = r.errors.length ? { error: String(r.errors[0].message || r.errors[0]) } : { code: r.code }
  } catch (e) {
    out = { error: String(e.message || e) }
  }
  sassOut.push({ lang, source, ...out })
}
fs.writeFileSync('tests/fixtures/sass.json', JSON.stringify(sassOut))
console.log(`sass: ${sassOut.length}`)

// --- CSS modules ------------------------------------------------------------
const moduleCases = [
  '.a { color: red }',
  '.a .b { color: red }',
  '.a, .b { color: red }',
  ':global(.g) .a { color: red }',
  '.a:hover { color: red }',
  '#id { color: red }',
  '@keyframes spin { from {} }\n.a{animation:spin 1s}',
  '@media (min-width: 1px) { .a { color: red } }',
  '.a { color: red }\n.a { color: blue }',
  ':global .g { color: red }',
  '.a :global(.g) { color: red }',
  'div.a > .b + .c { color: red }',
]
const modOut = []
for (const source of moduleCases) {
  let out
  try {
    const r = await sfcApi.compileStyleAsync({
      source,
      filename: '/foo/bar.vue',
      id: 'data-v-xxxxxxxx',
      modules: true,
    })
    out = r.errors.length
      ? { error: String(r.errors[0].message || r.errors[0]) }
      : { code: r.code, modules: r.modules }
  } catch (e) {
    out = { error: String(e.message || e) }
  }
  modOut.push({ source, ...out })
}
fs.writeFileSync('tests/fixtures/css-modules.json', JSON.stringify(modOut))
console.log(`css-modules: ${modOut.length}`)

// --- compileTemplate({ ssr: true }) -----------------------------------------
const ssrOut = []
for (const input of templates) {
  let r
  try {
    r = sfcApi.compileTemplate({
      source: input,
      filename: 'anonymous.vue',
      id: 'someid',
      ssr: true,
    })
  } catch (e) {
    continue
  }
  ssrOut.push({
    input,
    code: r.code,
    errors: r.errors.map(e => (typeof e === 'string' ? e : e.message)),
  })
}
fs.writeFileSync('tests/fixtures/compile-ssr.json', JSON.stringify(ssrOut))
console.log(`compile-ssr: ${ssrOut.length}`)

// --- compileScript({ templateOptions: { ssr: true } }) ----------------------
const scriptSsrOut = []
for (const source of sfcSources) {
  let descriptor
  try {
    descriptor = sfcApi.parse(source, { filename: 'anonymous.vue', sourceMap: false }).descriptor
  } catch {
    continue
  }
  if (!descriptor.script && !descriptor.scriptSetup) continue
  let out
  try {
    const r = sfcApi.compileScript(descriptor, {
      id: 'xxxxxxxx',
      inlineTemplate: true,
      sourceMap: false,
      templateOptions: { ssr: true },
    })
    out = { content: r.content, bindings: r.bindings ? { ...r.bindings } : null }
  } catch (e) {
    out = { error: String(e.message || e) }
  }
  scriptSsrOut.push({ input: source, ...out })
}
fs.writeFileSync('tests/fixtures/compile-script-ssr.json', JSON.stringify(scriptSsrOut))
console.log(`compile-script-ssr: ${scriptSsrOut.length}`)

// --- ssrCssVars: <style> v-bind() with an SSR inline template ---------------
const ssrCssVarOut = []
for (const tpl of templates) {
  if (tpl.includes('</template>') || tpl.includes('<script')) continue
  const source =
    `<template>${tpl}</template>\n` +
    `<script setup>const color = 'red'</script>\n` +
    `<style>div { color: v-bind(color) }</style>`
  let out
  try {
    const descriptor = sfcApi.parse(source, {
      filename: 'anonymous.vue',
      sourceMap: false,
    }).descriptor
    const r = sfcApi.compileScript(descriptor, {
      id: 'xxxxxxxx',
      inlineTemplate: true,
      sourceMap: false,
      templateOptions: { ssr: true },
    })
    out = { content: r.content }
  } catch (e) {
    out = { error: String(e.message || e) }
  }
  ssrCssVarOut.push({ input: source, ...out })
}
fs.writeFileSync('tests/fixtures/ssr-css-vars.json', JSON.stringify(ssrCssVarOut))
console.log(`ssr-css-vars: ${ssrCssVarOut.length}`)
