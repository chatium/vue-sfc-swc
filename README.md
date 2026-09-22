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

## Using it

```toml
[dependencies]
vue-sfc = { git = "ssh://git@github.com/chatium/vue-sfc-swc.git" }
```

```rust
let r = vue_sfc::ugc::compile_vue(source, path)?;   // { logic, template, code }
```

It pins the same `swc_core` (`=80.0.0`) and toolchain (1.98.1) as
`chatium-ugc-compiler`, so it drops into that build.

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

Every bug this port has had is pinned by a minimal case in
`tests/corpus/regressions.json`, which feeds the `ugc-vue` fixture. Reverting
any one of the fixes turns `cargo test` red.

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
| `parse-base` | 674 |
| `sfc-parse` | 238 |
| `compile-dom` | 673 |
| `compile-template` | 1346 |
| `compile-ssr` | 672 |
| `compile-script` | 204 |
| `compile-script-ssr` | 102 |
| `ssr-css-vars` | 656 |
| `compile-style` | 448 |
| `compile-template-opts` / `compile-script-opts` / `compile-style-opts` | 476 |
| `postcss-roundtrip` / `selector-scoped` / `css-modules` / `sass` / `rewrite-default` | 158 |
| `ugc-vue` (end to end) | 288 |

The `*-opts` suites vary the *options* rather than the input — the same sources compiled with
`isProd`, `scoped`, `slotted`, `ssr`, `inlineTemplate`, `genDefaultAs`, `customElement` and CSS
`modules`, in the combinations the consumer and the official tests use.

`tools/check-dir.mjs <dir>` runs the same end-to-end check over a tree of real `.vue` files without
vendoring them — it writes `tests/fixtures/ugc-vue-local.json` (gitignored), which `ugc_vue_local`
picks up and otherwise skips. Against 3,746 distinct `.vue` files found under `~/github`: every
successful compile is byte-identical, and the 17 remaining cases are invalid input where only the
diagnostic's wording differs (that corpus is not fixed, so `ugc_vue_local` reports those rather
than failing on them).

Sixteen public Vue codebases, checked the same way — 12964 of 12966 distinct SFCs:

| repo | files | result |
| --- | --- | --- |
| [unovue/shadcn-vue](https://github.com/unovue/shadcn-vue) | 4380 | 4380 |
| [primefaces/primevue](https://github.com/primefaces/primevue) | 2517 | 2517 |
| [vuetifyjs/vuetify](https://github.com/vuetifyjs/vuetify) | 1264 | 1263 |
| [Tencent/tdesign-vue-next](https://github.com/Tencent/tdesign-vue-next) | 845 | 845 |
| [element-plus/element-plus](https://github.com/element-plus/element-plus) | 816 | 816 |
| [nuxt/ui](https://github.com/nuxt/ui) | 790 | 790 |
| [vueComponent/ant-design-vue](https://github.com/vueComponent/ant-design-vue) | 731 | 731 |
| [vbenjs/vue-vben-admin](https://github.com/vbenjs/vue-vben-admin) | 574 | 574 |
| [elk-zone/elk](https://github.com/elk-zone/elk) | 264 | 264 |
| [varletjs/varlet](https://github.com/varletjs/varlet) | 220 | 220 |
| [PanJiaChen/vue-element-admin](https://github.com/PanJiaChen/vue-element-admin) | 131 | 130 |
| [youzan/vant](https://github.com/youzan/vant) | 128 | 128 |
| [slidevjs/slidev](https://github.com/slidevjs/slidev) | 125 | 125 |
| [epicmaxco/vuestic-admin](https://github.com/epicmaxco/vuestic-admin) | 99 | 99 |
| [vuejs/vitepress](https://github.com/vuejs/vitepress) | 71 | 71 |
| [vuejs/core](https://github.com/vuejs/core) | 11 | 11 |

```bash
git clone --depth 1 https://github.com/elk-zone/elk /tmp/elk
node tools/check-dir.mjs /tmp/elk            # writes tests/fixtures/ugc-vue-local.json
cargo test --test conformance ugc_vue_local  # or: cargo run --example triage -- <corpus.json>
```

The SSR output is checked over the same trees, by `tools/check-dir-ssr.mjs` and
`examples/triage-ssr.rs`, which compare `compileTemplate({ ssr: true })` and
the inline SSR render function. All 10871 SFCs whose reference compiles match;
the other 2095 fail identically on both sides (a `defineProps` type that would
have to come from an import).

```bash
node tools/check-dir-ssr.mjs /tmp/elk /tmp/elk-ssr.json
cargo run --release --example triage-ssr -- /tmp/elk-ssr.json
```

`examples/triage.rs` replays such a corpus and groups the divergences by kind,
which is the quicker way in when a fresh tree turns some up.

Both misses are `lang="sass"`/`scss` and are `grass` disagreeing with
dart-sass, not with this port: dart-sass drops blank lines inside an unknown
at-rule's prelude, and re-indents a multi-line comment's continuation lines.

## Known divergences

Six `ugc-vue` cases differ, all of them the *wording* of a diagnostic for invalid input; the stage,
position and code frame match. They are listed in `UGC_DIAGNOSTIC_DIVERGENCE` in
`tests/conformance.rs`:

- JS syntax errors read as swc phrases them, not Babel (`Expression expected` vs
  `Unexpected reserved word 'enum'.`). Matching would mean porting Babel's parser.
- Sass errors read as `grass` phrases them, and its caret can sit one column left of dart-sass's.

`@value name from './other.css'` in a CSS module is reported as an error: resolving it means
reading another file, and this compiler has no filesystem. So does a `defineProps<T>()` whose `T`
comes from an import, which is what `@vue/compiler-sfc` itself does without an `fs` option.

Source maps are not produced. `compileTemplate` generates one in JS, but the SFC pipeline this
targets never reads it.
