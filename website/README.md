# Weave website

An [Astro](https://astro.build) site with a landing page and the project documentation.

The documentation is not copied: pages are rendered straight from `../docs/*.md`, so the repository docs stay the single source of truth. `src/data/docs.mjs` chooses which files are published, their order and grouping. Relative links in the docs are rewritten to site routes or to the GitHub repository (`src/plugins/rehype-doc-links.mjs`). `.weave` code blocks are highlighted with the grammar from `../editors/vscode`.

The site's own stylesheet is written in Weave: `styles/*.weave` is compiled by `weavec` into `src/styles/global.css` (generated, git-ignored) before every `dev` and `build`, and rebuilt on save in dev. Build `weavec` first (`cargo build --release -p weave-css` at the repository root) or set `WEAVEC` to its path. The Playground page also uses `weavec` through the dev server.

```bash
npm install
npm run dev      # http://localhost:4321
npm run build    # static output in dist/
npm run preview
```

Requires Node 22.23.4 or newer. Run commands from this directory (the build resolves `../docs` and `../editors`).
