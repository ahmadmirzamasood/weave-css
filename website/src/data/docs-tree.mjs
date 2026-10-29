// The documentation tree. Source of truth is ../docs/*.md; this module splits
// those files into reference pages, groups them into sections (the "Contents"
// sidebar), and rewrites repository-relative links to site routes.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import GithubSlugger from 'github-slugger';

export const REPO = 'https://github.com/ahmadmirzamasood/weave-css';

const DOCS_DIR = resolve(process.cwd(), '../docs');

// ---------------------------------------------------------------------------
// Section layout. `part` items are one `## ` heading of a file (matched by
// regex on its text); `intro` is the text before the first `## `; `whole`
// publishes a file as a single page; `auto` makes an intro page plus one page
// per `## ` heading.
// ---------------------------------------------------------------------------
const layout = [
  {
    id: 'start',
    title: 'Get started',
    items: [
      { whole: 'INSTALL.md', id: 'install', title: 'Installation' },
      { whole: 'TUTORIAL.md', id: 'tutorial', title: 'Tutorial' },
    ],
  },
  {
    id: 'syntax',
    title: 'Syntax',
    items: [
      { file: 'LANGUAGE.md', intro: true, id: 'language', title: 'Overview' },
      { file: 'LANGUAGE.md', part: /^1\./, id: 'language/design-principles' },
      { file: 'LANGUAGE.md', part: /^2\./, id: 'language/source-text' },
      { file: 'LANGUAGE.md', part: /^3\./, id: 'language/file-structure' },
      { file: 'LANGUAGE.md', part: /^13\./, id: 'language/comments-and-opaque-css' },
    ],
  },
  {
    id: 'style-rules',
    title: 'Style Rules',
    items: [
      { file: 'LANGUAGE.md', part: /^11\./, id: 'language/style-rules-and-nesting' },
      { file: 'LANGUAGE.md', part: /^12\./, id: 'language/css-at-rules' },
    ],
  },
  {
    id: 'bindings',
    title: 'Bindings and Interpolation',
    items: [
      { file: 'LANGUAGE.md', part: /^4\./, id: 'language/bindings-and-scope' },
      { file: 'LANGUAGE.md', part: /^7\./, id: 'language/interpolation' },
    ],
  },
  {
    id: 'values',
    title: 'Values and Expressions',
    items: [
      { file: 'LANGUAGE.md', part: /^5\./, id: 'language/expressions' },
      { file: 'LANGUAGE.md', part: /^6\./, id: 'language/values' },
      { file: 'TYPES.md', intro: true, id: 'types', title: 'Types overview' },
      { file: 'TYPES.md', part: /^1\./, id: 'types/types' },
      { file: 'TYPES.md', part: /^2\./, id: 'types/numbers' },
      { file: 'TYPES.md', part: /^3\./, id: 'types/strings' },
      { file: 'TYPES.md', part: /^4\./, id: 'types/colors' },
      { file: 'TYPES.md', part: /^5\./, id: 'types/serialization' },
      { file: 'TYPES.md', part: /^6\./, id: 'types/lists-and-maps' },
      { file: 'TYPES.md', part: /^7\./, id: 'types/css-fragments' },
      { file: 'TYPES.md', part: /^8\./, id: 'types/type-annotations' },
      { file: 'TYPES.md', part: /^9\./, id: 'types/core-built-ins', title: 'Core built-ins summary' },
    ],
  },
  {
    id: 'functions',
    title: 'Functions and Recipes',
    items: [
      { file: 'LANGUAGE.md', part: /^8\./, id: 'language/functions' },
      { file: 'LANGUAGE.md', part: /^9\./, id: 'language/recipes' },
    ],
  },
  {
    id: 'control-flow',
    title: 'Control Flow',
    items: [{ file: 'LANGUAGE.md', part: /^10\./, id: 'language/control-flow', title: 'Conditions and iteration' }],
  },
  {
    id: 'modules',
    title: 'Modules',
    items: [{ file: 'LANGUAGE.md', part: /^14\./, id: 'language/modules' }],
  },
  {
    id: 'output',
    title: 'Output and Diagnostics',
    items: [
      { file: 'LANGUAGE.md', part: /^15\./, id: 'language/output-format' },
      { file: 'LANGUAGE.md', part: /^16\./, id: 'language/diagnostics' },
      { file: 'LANGUAGE.md', part: /^17\./, id: 'language/resource-limits' },
      { file: 'LANGUAGE.md', part: /^19\./, id: 'language/not-in-v0-1' },
      { file: 'LANGUAGE.md', part: /^20\./, id: 'language/complete-example' },
    ],
  },
  {
    id: 'builtins',
    title: 'Built-In Library',
    items: [
      { file: 'BUILTINS.md', intro: true, id: 'builtins', title: 'Overview' },
      { file: 'LANGUAGE.md', part: /^18\./, id: 'language/core-built-ins', title: 'Specification (§18)' },
      { file: 'BUILTINS.md', part: /^Rules shared/, id: 'builtins/rules', title: 'Rules shared by all built-ins' },
      { file: 'BUILTINS.md', part: /^`range`/, id: 'builtins/range', title: 'range' },
      { file: 'BUILTINS.md', part: /^`css`/, id: 'builtins/css', title: 'css', sub: 'explicit CSS sequences' },
      { file: 'BUILTINS.md', part: /^`math`/, id: 'builtins/math', title: 'math', sub: 'numbers' },
      { file: 'BUILTINS.md', part: /^`list`/, id: 'builtins/list', title: 'list', sub: 'lists' },
      { file: 'BUILTINS.md', part: /^`map`/, id: 'builtins/map', title: 'map', sub: 'maps' },
      { file: 'BUILTINS.md', part: /^`string`/, id: 'builtins/string', title: 'string', sub: 'strings' },
      {
        file: 'BUILTINS.md',
        part: /^`color`/,
        id: 'builtins/color',
        title: 'color',
        sub: '8-bit sRGB colors',
        note: 'Compatibility: weavec 0.1.0-rc.3 and later. The color built-ins were added in that release candidate; earlier builds reported them as E5001.',
      },
      { file: 'BUILTINS.md', part: /^`meta`/, id: 'builtins/meta', title: 'meta', sub: 'values' },
    ],
  },
  {
    id: 'css',
    title: 'CSS Compatibility',
    items: [{ auto: 'CSS-COMPATIBILITY.md', id: 'css-compatibility' }],
  },
  {
    id: 'cli',
    title: 'Command Line',
    items: [{ auto: 'CLI.md', id: 'cli' }],
  },
  {
    id: 'library',
    title: 'Rust Library API',
    items: [{ auto: 'LIBRARY.md', id: 'library' }],
  },
  {
    id: 'project',
    title: 'Project',
    items: [
      { whole: 'COMPATIBILITY-POLICY.md', id: 'compatibility-policy', title: 'Compatibility policy' },
      { whole: 'BENCHMARKS.md', id: 'benchmarks', title: 'Benchmarks' },
      { whole: 'RELEASE-NOTES-0.1.0-rc.3.md', id: 'release-notes', title: 'Release notes 0.1.0-rc.3' },
      { whole: 'RELEASING.md', id: 'releasing', title: 'Releasing' },
    ],
  },
];

// ---------------------------------------------------------------------------

const fileCache = new Map();

function readDoc(name) {
  if (!fileCache.has(name)) fileCache.set(name, readFileSync(resolve(DOCS_DIR, name), 'utf8').replace(/\r\n/g, '\n'));
  return fileCache.get(name);
}

const FENCE = /^\s*(```|~~~)/;

/** Run `fn` on each line outside fenced code blocks; other lines pass through. */
function mapOutsideFences(text, fn) {
  let fenced = false;
  return text
    .split('\n')
    .map((line) => {
      if (FENCE.test(line)) {
        fenced = !fenced;
        return line;
      }
      return fenced ? line : fn(line);
    })
    .join('\n');
}

function cleanHeading(text) {
  return text.replace(/`/g, '').trim();
}

/** Split a markdown file into its intro and its `## ` sections. */
function parse(name) {
  const lines = readDoc(name).split('\n');
  const sections = [];
  let intro = [];
  let current = null;
  let fenced = false;
  for (const line of lines) {
    if (FENCE.test(line)) fenced = !fenced;
    const m = !fenced && /^##\s+(.*)$/.exec(line);
    if (m) {
      current = { heading: m[1].trim(), lines: [] };
      sections.push(current);
    } else if (current) {
      current.lines.push(line);
    } else {
      intro.push(line);
    }
  }
  const title = (intro.find((l) => /^#\s/.test(l)) ?? '# ').replace(/^#\s+/, '');
  return { title, intro: intro.join('\n'), sections: sections.map((s) => ({ heading: s.heading, body: s.lines.join('\n') })) };
}

function titleOf(heading) {
  return cleanHeading(heading).replace(/^\d+\.\s+/, '').replace(/^./, (c) => c.toUpperCase());
}

function slugOf(text) {
  return new GithubSlugger().slug(cleanHeading(text));
}

const files = new Set(
  layout.flatMap((s) => s.items.map((i) => i.file ?? i.whole ?? i.auto)),
);

// ---- build pages ----------------------------------------------------------

function shiftHeadings(body) {
  return mapOutsideFences(body, (l) => (/^#{3,6}\s/.test(l) ? l.slice(1) : l));
}

let built = null;

function build() {
  if (built) return built;
  const pages = [];
  const sections = layout.map((s) => ({ id: s.id, title: s.title, pages: [] }));
  const parsed = new Map();
  const get = (f) => {
    if (!parsed.has(f)) parsed.set(f, parse(f));
    return parsed.get(f);
  };

  layout.forEach((s, si) => {
    const add = (page) => {
      pages.push({ ...page, section: s.title, sectionId: s.id });
      sections[si].pages.push(pages[pages.length - 1]);
    };
    for (const item of s.items) {
      if (item.whole) {
        const doc = get(item.whole);
        add({ id: item.id, title: item.title ?? doc.title, file: item.whole, kind: 'whole', raw: readDoc(item.whole), headingKey: null });
      } else if (item.auto) {
        const doc = get(item.auto);
        add({ id: item.id, title: item.title ?? doc.title.replace(/`/g, ''), navTitle: 'Overview', file: item.auto, kind: 'intro', raw: doc.intro });
        for (const sec of doc.sections) {
          const slug = slugOf(sec.heading);
          add({ id: `${item.id}/${slug}`, title: titleOf(sec.heading), file: item.auto, kind: 'part', heading: sec.heading, raw: sec.body });
        }
      } else if (item.intro) {
        const doc = get(item.file);
        add({ id: item.id, title: item.title ?? doc.title, navTitle: item.title ?? 'Overview', file: item.file, kind: 'intro', raw: doc.intro, note: item.note });
      } else {
        const doc = get(item.file);
        const sec = doc.sections.find((x) => item.part.test(x.heading.trim()));
        if (!sec) throw new Error(`docs-tree: no heading in ${item.file} matches ${item.part}`);
        add({
          id: item.id,
          title: item.title ?? titleOf(sec.heading),
          sub: item.sub,
          file: item.file,
          kind: 'part',
          heading: sec.heading,
          raw: sec.body,
          note: item.note,
        });
      }
    }
  });

  // Map every heading of every published file to the page that now holds it.
  const anchors = new Map(); // `${file}#${slug}` -> page id
  const overview = new Map(); // file -> page id
  for (const page of pages) {
    if (!overview.has(page.file) || page.kind === 'intro' || page.kind === 'whole') {
      if (!overview.has(page.file) || page.kind !== 'part') overview.set(page.file, page.id);
    }
  }
  for (const f of files) {
    const doc = get(f);
    const slugger = new GithubSlugger();
    const walk = (text, pageId) => {
      mapOutsideFences(text, (l) => {
        const m = /^(#{1,6})\s+(.*)$/.exec(l);
        if (m) anchors.set(`${f}#${slugger.slug(cleanHeading(m[2]))}`, pageId);
        return l;
      });
    };
    // Same order as the file so duplicate-heading suffixes match GitHub's.
    const owners = pages.filter((p) => p.file === f);
    if (owners.some((p) => p.kind === 'whole')) {
      walk(readDoc(f), owners[0].id);
      continue;
    }
    const introPage = owners.find((p) => p.kind === 'intro');
    walk(doc.intro, introPage ? introPage.id : overview.get(f));
    for (const sec of doc.sections) {
      const owner = owners.find((p) => p.kind === 'part' && p.heading === sec.heading);
      const target = owner ? owner.id : overview.get(f);
      walk(`## ${sec.heading}\n${sec.body}`, target);
    }
  }

  const rewrite = (text, file) =>
    mapOutsideFences(text, (line) =>
      line.replace(/\]\(([^)\s]+)\)/g, (all, href) => `](${resolveLink(href, file)})`),
    );

  function resolveLink(href, file) {
    if (/^([a-z][a-z0-9+.-]*:|\/)/i.test(href)) return href;
    const [path, hash] = href.split('#');
    const target = path === '' ? file : path.replace(/^\.\//, '');
    if (files.has(target)) {
      const id = hash ? anchors.get(`${target}#${hash}`) ?? overview.get(target) : overview.get(target);
      if (id) return `/docs/${id}/${hash ? `#${hash}` : ''}`;
    }
    if (path === '') return href;
    // Anything else is a repository file: resolve relative to docs/.
    const out = [];
    for (const p of ['docs', ...target.split('/')]) {
      if (p === '..') out.pop();
      else if (p !== '.') out.push(p);
    }
    const joined = out.join('/');
    const kind = path.endsWith('/') || !/\.[a-z0-9]+$/i.test(joined) ? 'tree' : 'blob';
    return `${REPO}/${kind}/main/${joined}${hash ? `#${hash}` : ''}`;
  }

  for (const page of pages) {
    if (page.kind === 'part') {
      const num = `<a id="${slugOf(page.heading)}"></a>\n\n`;
      const subtitle = page.sub ? `<p class="sub-title">${page.sub}</p>\n\n` : '';
      page.body = `${num}# ${page.title}\n\n${subtitle}${rewrite(shiftHeadings(page.raw), page.file)}`;
    } else if (page.kind === 'intro') {
      const text = page.raw.replace(/^#\s+.*\n?/m, '');
      page.body = `# ${page.title}\n\n${rewrite(text, page.file)}`;
    } else {
      page.body = rewrite(page.raw, page.file);
    }
    page.url = `/docs/${page.id}/`;
    page.edit = `${REPO}/blob/main/docs/${page.file}`;
  }

  built = { sections, pages };
  return built;
}

export function getTree() {
  return build();
}

export function getPages() {
  return build().pages;
}

/** Previous/next page in sidebar order. */
export function neighbours(id) {
  const { pages } = build();
  const i = pages.findIndex((p) => p.id === id);
  return { prev: pages[i - 1] ?? null, next: pages[i + 1] ?? null };
}
