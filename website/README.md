# gilfoyle.evrard.online

The marketing site for **Gilfoyle** — a static [Astro](https://astro.build) build with
[Tailwind CSS v4](https://tailwindcss.com). Bilingual (English at `/`, French at `/fr/`),
one landing page plus a docs section.

The site is deliberately **self-contained**: it is a Node project that lives beside the Rust
workspace and does not touch the crate build or `make check`.

## Requirements

- **Node.js 22.23.4 or newer** (Astro 7's floor). The pinned toolchain is not used here.

## Commands

```sh
npm ci            # install exactly from package-lock.json
npm run dev       # dev server with HMR on http://localhost:4321
npm run build     # generate the static site into dist/
npm run preview   # serve dist/ locally
npm run og        # rasterise public/og-*.svg to PNG (also runs on build)
```

From the repository root there are matching targets: `make website-install`,
`make website-dev`, `make website-build`, `make website-preview`.

## Deploying (manual)

The output is plain static files in `website/dist/`, served at the domain root.

```sh
cd website
npm ci
npm run build
# then copy dist/ to the web root, e.g.
rsync -av --delete dist/ deploy@gilfoyle.evrard.online:/var/www/gilfoyle/
```

Any static host works; nothing here needs a Node runtime in production.

## Editing the content

- **Prose and UI strings** live in `src/i18n/en.ts` and `src/i18n/fr.ts`. The two files share
  one TypeScript shape, so a missing French key is a build error.
- **Docs pages** are Markdown in `src/content/docs/en/` and `src/content/docs/fr/`, one
  collection per locale. Frontmatter: `title`, `description`, `order`, `lang`.
- **Domain, repository and release links** are centralised in `src/site.ts`.
- **Social cards** are hand-written SVG in `public/og-{en,fr}.svg`, rasterised to PNG at build.

## Structure

```
src/
├── site.ts                 # domain, repo links, locale helpers
├── i18n/                   # en.ts / fr.ts — all copy
├── content/docs/{en,fr}/   # Markdown docs
├── styles/global.css       # Tailwind theme tokens + signature effects
├── components/             # Nav, Footer, Terminal, Section, Landing, Cross
├── layouts/                # BaseLayout (head/SEO), DocsLayout (sidebar + TOC)
├── pages/                  # index, fr/index, docs/, fr/docs/, 404
└── scripts/main.js         # typewriter + scroll reveal
public/                     # favicon, og-*.svg
scripts/                    # astro launcher, OG rasteriser, WASM shim
```

## Two environment quirks

Both are handled automatically; they are documented so they are not mistaken for cruft.

- **`overrides.rolldown: "1.3.1"`** — Vite 8 pins `rolldown@~1.2.18`, whose native binding
  raises `SIGILL` on CPUs without AVX2/BMI2 (e.g. a QEMU VM). `1.3.1` runs everywhere.
- **`scripts/run-astro.mjs`** — on such CPUs Astro's native Rust compiler also crashes. The
  launcher detects a missing `avx2` flag, fetches the WebAssembly compiler fallback once
  (`@astrojs/compiler-binding-wasm32-wasi`, which npm will not install on x64 because of its
  `cpu` field), and redirects only that binding to it via `scripts/astro-compiler-wasm.cjs`.
  On a normal machine the native compiler is used and none of this runs.

Override detection with `GILFOYLE_FORCE_WASM=1` or `GILFOYLE_NATIVE_COMPILER=1`.
