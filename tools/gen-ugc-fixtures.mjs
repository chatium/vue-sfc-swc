// Runs the real helper.cjs Vue pipeline over a corpus of .vue sources.
import fs from 'node:fs'
import { spawn } from 'node:child_process'
import path from 'node:path'

const sources = [
  ...JSON.parse(fs.readFileSync('tests/corpus/sfc.json', 'utf8')),
  ...JSON.parse(fs.readFileSync('tests/corpus/sfc-extra.json', 'utf8')),
  ...JSON.parse(fs.readFileSync('tests/corpus/ugc-vue.json', 'utf8')),
]

const helper = spawn(process.execPath, [path.resolve('tools/reference-helper.cjs')], {
  stdio: ['pipe', 'pipe', 'inherit'],
  cwd: path.resolve('tools'),
})

const results = []
let resolveLine
helper.stdout.setEncoding('utf8')
let buf = ''
helper.stdout.on('data', chunk => {
  buf += chunk
  let i
  while ((i = buf.indexOf('\n')) >= 0) {
    const line = buf.slice(0, i)
    buf = buf.slice(i + 1)
    resolveLine(JSON.parse(line))
  }
})

async function run(source, filePath) {
  return new Promise(resolve => {
    resolveLine = resolve
    helper.stdin.write(JSON.stringify({ kind: 'vue', source, path: filePath }) + '\n')
  })
}

async function main() {
  for (const source of sources) {
    const out = await run(source, 'component.vue')
    results.push({ source, path: 'component.vue', out })
  }
  helper.stdin.end()
  fs.writeFileSync('tests/fixtures/ugc-vue.json', JSON.stringify(results))
  console.log(`ugc-vue: ${results.length}`)
}
main()
