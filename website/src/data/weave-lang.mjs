import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

// The editor extension ships a TextMate grammar for `.weave`; reuse it so the
// site highlights Weave exactly as the editors do. Paths resolve from the
// website/ directory because bundling moves this module at build time.
const grammar = JSON.parse(
  readFileSync(resolve(process.cwd(), '../editors/vscode/syntaxes/weave.tmLanguage.json'), 'utf8'),
);

export const weaveLang = { ...grammar, name: 'weave' };
