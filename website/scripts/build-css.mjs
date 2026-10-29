import { buildCss } from '../src/plugins/build-css.mjs';

try {
  console.log(`[weave] wrote ${buildCss()}`);
} catch (err) {
  console.error(String(err.stderr || err.message || err));
  process.exit(1);
}
