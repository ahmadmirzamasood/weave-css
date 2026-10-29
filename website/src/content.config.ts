import { defineCollection } from 'astro:content';
import { getPages } from './data/docs-tree.mjs';

// Documentation pages are cut from ../docs/*.md by src/data/docs-tree.mjs.
const docs = defineCollection({
  loader: {
    name: 'weave-docs',
    async load({ store, renderMarkdown, generateDigest }) {
      store.clear();
      for (const page of getPages()) {
        store.set({
          id: page.id,
          data: { title: page.title, section: page.section, file: page.file, note: page.note ?? '' },
          body: page.body,
          rendered: await renderMarkdown(page.body),
          digest: generateDigest(page.body),
        });
      }
    },
  },
});

export const collections = { docs };
