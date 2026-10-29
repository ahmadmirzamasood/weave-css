import { defineConfig } from 'astro/config';
import { weaveLang } from './src/data/weave-lang.mjs';
import { rehypeSignatures } from './src/plugins/rehype-signatures.mjs';
import { compileApi } from './src/plugins/vite-compile-api.mjs';

export default defineConfig({
  server: { port: 4321, host: 'localhost' },
  vite: { plugins: [compileApi()] },
  markdown: {
    rehypePlugins: [rehypeSignatures],
    shikiConfig: {
      theme: 'github-light',
      langs: [weaveLang],
      wrap: false,
    },
  },
});
