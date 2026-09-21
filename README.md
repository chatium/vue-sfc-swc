# vue-sfc-swc

Rust/SWC port of `@vue/compiler-sfc` (pinned to Vue **3.5.38**), built to replace the Node
`@vue/compiler-sfc` call in `@chatium/ugc-source-compiler`'s `helper.cjs`.

Goal: byte-identical output to the JS compiler for the API surface that package uses:

| JS API | Rust |
| --- | --- |
| `parse(source, { sourceMap, filename })` | `sfc::parse` |
| `compileScript(descriptor, { id, inlineTemplate, templateOptions })` | `sfc::compile_script` |
| `compileTemplate({ source, ast, filename, id, scoped, compilerOptions })` | `sfc::compile_template` |
| `compileStyleAsync({ id, source, filename, scoped, modules, preprocessLang })` | `sfc::compile_style` |
| `rewriteDefault(content, as, expressionPlugins)` | `sfc::rewrite_default` |

## Testing

Conformance is differential: `tools/gen-fixtures.mjs` runs the real `@vue/compiler-sfc@3.5.38`
over a corpus (the official `vuejs/core` compiler test inputs plus our own) and writes expected
output JSON into `tests/fixtures/`. `cargo test` replays the same corpus through the Rust port and
asserts equality.

```bash
npm --prefix tools ci && node tools/gen-fixtures.mjs   # refresh expectations
cargo test
```
