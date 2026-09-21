// Private compiler pipe: never load UGC code or backend services in this process.
const { createInterface } = require('node:readline')
// Vue path only: the Solid path needs babel, which this harness does not use.
async function compile(request) {
  if (request.kind === 'vue') return compileVue(request)
  throw new Error(`Unknown compiler helper operation: ${request.kind}`)
}

async function main() {
  for await (const line of createInterface({ input: process.stdin, crlfDelay: Infinity })) {
    try {
      process.stdout.write(JSON.stringify(await compile(JSON.parse(line))) + '\n')
    } catch (error) {
      process.stdout.write(
        JSON.stringify({
          error: error.buildError || error.message,
          logic: error.logic,
          stage: error.stage || (error.logic ? 'style' : 'script'),
        }) + '\n',
      )
    }
  }
}
main().catch(error => {
  console.error(error)
  process.exitCode = 1
})

function vueErrors(path, errors) {
  if (!errors.length) return
  throw {
    buildError: {
      type: 'UgcBuildMultiError',
      filePath: path,
      msg: `Found ${errors.length} errors in file ${path}`,
      errors: errors.map(error => {
        const loc = error.loc?.start
        const position = loc
          ? { line: loc.line - 1, character: loc.column - 1 }
          : typeof error.line === 'number'
          ? { line: error.line - 1, character: error.column - 1 }
          : undefined
        return position
          ? { type: 'UgcBuildError', msg: error.message, filePath: path, position }
          : typeof error === 'string'
          ? { type: 'UgcBuildError', msg: error }
          : error.message
      }),
    },
  }
}

// Both the entrypoint and subsequent loads must stop before Sass's filesystem fallback.
const denySassImports = {
  canonicalize() {
    throw new Error('Sass filesystem imports are disabled')
  },
  load() {
    throw new Error('Sass filesystem imports are disabled')
  },
}

async function compileVue({ source, path }) {
  const vue = require('@vue/compiler-sfc')
  const { descriptor: d, errors } = vue.parse(source, { sourceMap: true, filename: '/' + path })
  try {
    // Empty SFC fails in the legacy client stage, which uses a leading slash.
    vueErrors(source.trim() ? path : '/' + path, errors)
  } catch (error) {
    error.stage = 'parse'
    throw error
  }
  const expressionPlugins =
    d.script?.lang?.startsWith('ts') || d.scriptSetup?.lang?.startsWith('ts') ? ['typescript'] : undefined
  const logic = '// @shared\n' + (d.script || d.scriptSetup ? vue.compileScript(d, { id: 'someid' }).content : '')
  const allErrors = []
  let template = ''
  if (d.template) {
    const result = vue.compileTemplate({
      source: d.template.content,
      ast: d.template.ast,
      filename: path,
      id: 'someid',
      compilerOptions: { expressionPlugins },
    })
    allErrors.push(...result.errors)
    template = result.code
  }
  const hash = require('node:crypto').createHash('sha1').update(source).digest('hex')
  const id = hash.slice(0, 4) + hash.slice(-4)
  const scoped = d.styles.some(style => style.scoped)
  const templateFailed = allErrors.length > 0
  let styles = ''
  const modules = {}
  for (const style of d.styles) {
    if (!style.content) continue
    const result = await vue.compileStyleAsync({
      id,
      source: style.content,
      filename: path,
      scoped: style.scoped,
      modules: !!style.module,
      preprocessLang: ['sass', 'scss'].includes(style.lang) ? style.lang : undefined,
      preprocessCustomRequire: require,
      preprocessOptions: { importer: denySassImports, importers: [denySassImports], loadPaths: [] },
    })
    allErrors.push(...result.errors)
    if (style.module && result.modules)
      modules[typeof style.module === 'string' ? style.module : '$style'] = result.modules
    styles += `\n;(function() { const style = document.createElement('style'); style.innerHTML = ${JSON.stringify(
      result.code,
    )}; document.head.appendChild(style); })();`
  }
  try {
    vueErrors(path, allErrors)
  } catch (error) {
    error.logic = logic
    error.stage = templateFailed ? 'template' : 'style'
    throw error
  }
  let code = 'const __sfc__ = {};'
  let bindings
  if (d.script || d.scriptSetup) {
    const script = vue.compileScript(d, {
      id,
      inlineTemplate: true,
      templateOptions: { scoped, compilerOptions: { expressionPlugins } },
    })
    bindings = script.bindings
    code = vue.rewriteDefault(script.content, '__sfc__', expressionPlugins) + ';'
  }
  if (d.template && !d.scriptSetup) {
    const compiled = vue.compileTemplate({
      id,
      source: d.template.content,
      ast: d.template.ast,
      filename: path,
      scoped,
      compilerOptions: { bindingMetadata: bindings, expressionPlugins },
    })
    vueErrors(path, compiled.errors)
    code +=
      '\n' +
      compiled.code.replace(/\nexport (function|const) render/, '$1 __sfc__render') +
      '\n__sfc__.render = __sfc__render;'
  }
  code += styles
  if (scoped) code += `\n(__sfc__.__vccOpts || __sfc__).__scopeId = ${JSON.stringify('data-v-' + id)};`
  if (Object.keys(modules).length) code += `\n(__sfc__.__vccOpts || __sfc__).__cssModules = ${JSON.stringify(modules)};`
  return { logic, template, code: '// @shared\n' + code + '\nexport default __sfc__;' }
}
