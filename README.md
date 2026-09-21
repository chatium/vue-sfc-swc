# vue-sfc-swc

Rust/SWC port of `@vue/compiler-sfc` and `@vue/compiler-ssr` (pinned to Vue **3.5.38**), built to
replace the Node framework-compiler call in `@chatium/ugc-source-compiler`'s `helper.cjs`.

Goal: byte-identical output to the JS compiler for the API surface that package uses.

| JS API | Rust |
| --- | --- |
| `parse(source, { sourceMap, filename })` | `sfc::parse` |
| `compileScript(descriptor, { id, inlineTemplate, templateOptions })` | `sfc::compile_script` |
| `compileTemplate({ source, ast, filename, id, scoped, ssr, compilerOptions })` | `sfc::compile_template` |
| `compileStyleAsync({ id, source, filename, scoped, modules, preprocessLang })` | `sfc::compile_style` |
| `rewriteDefault(content, as, expressionPlugins)` | `sfc::rewrite_default` |
| `@vue/compiler-dom`'s `compile` | `dom::compile` |
| `@vue/compiler-ssr`'s `compile` | `ssr::compile` |

`ugc::compile_vue(source, path)` is the whole `helper.cjs` `.vue` pipeline in one call, returning
the same `{ logic, template, code }` (or the same build error) the Node helper produces.

## Server-side rendering

`compileTemplate` with `ssr: true` goes through the `ssr` module — the SSR transform preset, the
string-buffer codegen pass and `vue/server-renderer` imports — instead of the vnode pipeline.
`compileScript` with `inline_template` + `template_ssr` emits `__ssrInlineRender: true` and the
inline `(_ctx, _push, _parent, _attrs) => {}` render function, with `<style>`'s `v-bind()` injected
as `_cssVars`. Nothing in `ugc::compile_vue` uses it: the consumer has no SSR path today, so SSR is
an additive capability on the library API.

## Shape of the port

It is a transliteration, not a redesign — the JS control flow, helper registration order and even
its quirks are reproduced, because the output has to match byte for byte.

- **Arena AST.** `core::ast::Arena` holds every node behind a `NodeId`. Vue's codegen graph aliases
  template nodes and their children arrays (a `VNodeCall`'s children *is* the element's children
  array), and `cacheStatic`/`stringifyStatic` mutate through those aliases; `Node::ChildrenRef`
  models a shared array.
- **Offsets.** UTF-16 code units wherever JS counts them (tokenizer, postcss, code frames), byte
  offsets for swc spans and MagicString.
- **JS parsing** is swc with spans rebased to 0 and `ParenExpr` stripped, to match what Babel hands
  the compiler.
- **CSS** is a port of postcss's tokenizer/parser/stringifier plus the slice of
  postcss-selector-parser that `scoped` needs; Sass goes through `grass`.

## Testing

Conformance is differential: `tools/gen-fixtures.mjs` runs the real `@vue/compiler-sfc@3.5.38` over
a corpus (the official `vuejs/core` compiler test inputs plus our own) and writes expected output
into `tests/fixtures/`. `cargo test` replays the same corpus through the Rust port and asserts
equality.

```bash
npm --prefix tools ci && node tools/gen-fixtures.mjs   # refresh expectations
cargo test
```

Current state — every case byte-identical:

| suite | cases |
| --- | --- |
| `parse-base` | 620 |
| `sfc-parse` | 180 |
| `compile-dom` | 656 |
| `compile-template` | 1312 |
| `compile-ssr` | 655 |
| `compile-script` | 200 |
| `compile-script-ssr` | 100 |
| `ssr-css-vars` | 652 |
| `compile-style` | 448 |
| `postcss-roundtrip` / `selector-*` | 208 |
| `ugc-vue` (end to end) | 250 of 256 |

## Known divergences

Six `ugc-vue` cases differ, all of them the *wording* of a diagnostic for invalid input; the stage,
position and code frame match. They are listed in `UGC_DIAGNOSTIC_DIVERGENCE` in
`tests/conformance.rs`:

- JS syntax errors read as swc phrases them, not Babel (`Expression expected` vs
  `Unexpected reserved word 'enum'.`). Matching would mean porting Babel's parser.
- Sass errors read as `grass` phrases them, and its caret can sit one column left of dart-sass's.

Source maps are not produced. `compileTemplate` generates one in JS, but the SFC pipeline this
targets never reads it.
