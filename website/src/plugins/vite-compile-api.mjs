import { execFile } from 'node:child_process';
import { watch } from 'node:fs';
import { buildCss, findWeavec } from './build-css.mjs';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const MAX_BYTES = 100_000;

/**
 * Dev-server only. Exposes the local `weavec` binary to the Playground page:
 * GET /api/compile reports availability, POST /api/compile compiles the posted
 * source. Modules cannot load files: the root is an empty temporary directory.
 */
export function compileApi() {
  return {
    name: 'weave-compile-api',
    apply: 'serve',
    configureServer(server) {
      const weavec = findWeavec();
      watchStyles(server);
      const root = mkdtempSync(join(tmpdir(), 'weave-playground-'));
      server.middlewares.use('/api/compile', (req, res) => {
        res.setHeader('content-type', 'application/json');
        res.setHeader('cache-control', 'no-store');
        if (req.method === 'GET') {
          res.end(JSON.stringify({ available: weavec !== null }));
          return;
        }
        if (req.method !== 'POST' || !weavec) {
          res.statusCode = weavec ? 405 : 503;
          res.end(JSON.stringify({ ok: false, error: weavec ? 'Method not allowed' : 'weavec binary not found' }));
          return;
        }
        let body = '';
        let size = 0;
        req.on('data', (chunk) => {
          size += chunk.length;
          if (size > MAX_BYTES) req.destroy();
          else body += chunk;
        });
        req.on('end', () => {
          let source = '';
          try {
            source = String(JSON.parse(body).source ?? '');
          } catch {
            res.statusCode = 400;
            res.end(JSON.stringify({ ok: false, error: 'Invalid request' }));
            return;
          }
          const child = execFile(
            weavec,
            ['build', '-', '--root', root, '--diagnostics', 'human'],
            { timeout: 5000, maxBuffer: 2_000_000 },
            (err, stdout, stderr) => {
              res.end(JSON.stringify({ ok: !err, css: stdout, diagnostics: stderr }));
            },
          );
          child.stdin.end(source);
        });
      });
    },
  };
}

// Rebuild src/styles/global.css whenever a styles/*.weave file changes.
function watchStyles(server) {
  let timer;
  try {
    watch(resolve(process.cwd(), 'styles'), () => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        try {
          buildCss();
          server.config.logger.info('[weave] rebuilt global.css');
        } catch (err) {
          server.config.logger.error(String(err.stderr || err.message));
        }
      }, 100);
    });
  } catch {
    /* styles/ missing: nothing to watch */
  }
}
