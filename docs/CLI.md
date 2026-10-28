# The `weave` command line

Reference for the `weave` binary of this repository (crate `weave-cli`). Run
`weave --help` for the summary and the resource-limit defaults of your build.

## Commands

| Command | Effect |
|---|---|
| `weave build <input> [options]` | Compile and write CSS: to stdout by default, or to `--output <file>` |
| `weave check <input> [options]` | Compile and report diagnostics; write no CSS (`--deps` is allowed) |
| `weave watch <input> -o <file> [options]` | Build, then rebuild whenever an input changes ([Watch](#watch)) |
| `weave help`, `-h`, `--help` | Print help |
| `weave version`, `-V`, `--version` | Print the compiler version, the language specification version and the tool features |

`<input>` is a `.weave` file, or `-` to read the entry module from stdin
(`build` and `check` only). Diagnostics name stdin `<stdin>.weave`.

Exit status: `0` success; `1` the build failed (diagnostics) or an output
could not be written; `2` invalid command line.

## Options

| Option | Meaning |
|---|---|
| `-o`, `--output <file\|->` | CSS destination. `-` is stdout (the default for `build`). Files are replaced atomically. |
| `--root <dir>` | Module root. Every module, the input included, must resolve inside it, symbolic links included (E7006). Default: the input file's directory; for stdin, the current directory. Module names in diagnostics and dependency reports are relative to the root. |
| `--source-map[=file\|inline]` | Write a source map: `file` (default) writes `<output>.map` and requires `--output <file>`; `inline` appends it to the CSS as a `data:` URL. |
| `--source-map-content` | Embed source text (`sourcesContent`) in the map. Off by default. |
| `--diagnostics <human\|json>` | Format of the report on stderr ([Diagnostics](#diagnostics)). |
| `--deps <file\|->` | Write the modules of the build, one path per line, relative to the current directory, entry first, then in discovery order. Written only after a successful build. `-` is stdout; not allowed when CSS also goes to stdout, or with `watch`. |
| `--limit <name>=<n>` | Override one resource limit (LANGUAGE.md §17); repeatable, each limit once. Zero and values above the limit's maximum are rejected. Names: `file-bytes`, `total-input-bytes`, `modules`, `nesting-depth`, `call-depth`, `evaluation-steps`, `emitted-selectors`, `output-bytes`, `collection-elements`, `string-bytes`, `diagnostics`. |
| `--poll-interval <ms>` | How often `watch` checks its inputs: 10 to 60000, default 200. |
| `--` | Ends options; a later argument starting with `-` is the input. |

Options take their value as the next argument or after `=` (`--output=out.css`).

## Module files and the root

Each module is resolved lexically (`load` paths cannot contain `..` beyond
the root), then canonicalized, following symbolic links and Windows
junctions; a canonical path outside the root is E7006. Only regular files
are read: a directory, FIFO or device named like a module is reported as
unreadable (E7001) instead of being opened. Reads stop one byte past the
file limit.

These checks use ordinary path operations. A process that can modify the
module tree while a build runs can replace a checked directory or file with
a link between the check and the read (time-of-check to time-of-use), so the
root confines honest mistakes and untrusted *content*, not untrusted
*concurrent writers*. Weave is not a sandbox; to compile trees that others
can write to during the build, copy them first or use operating-system
isolation.

## Outputs

- Outputs are written only after a successful build. Each file is written
  to a temporary file in the same directory (`.<name>.<pid>-<n>.weave-tmp`),
  flushed to disk, and renamed over the target, so readers see the old file
  or the new one, never a partial write. On failure the temporary file is
  removed and the old output is untouched. With several outputs the source
  map is written first, then the CSS, then the dependency list; each
  replacement is atomic, the group is not.
- **Collisions.** Output paths with the `.weave` extension (any letter case)
  are refused before compiling. After compiling, every output's identity
  (the canonical path of an existing file, else its canonical directory and
  name) is compared with every module of the build and with the other
  outputs; on Windows and macOS the comparison ignores ASCII case. A
  collision fails the build and writes nothing.
- With `--source-map=file`, the CSS ends with
  `/*# sourceMappingURL=<name>.css.map */` (the name percent-encoded). With
  `inline`, the comment holds
  `data:application/json;charset=utf-8;base64,…`.

## Source maps

Maps follow ECMA-426 (1st edition, December 2024): `version` 3, `file` (the
CSS file name, when writing to a file), `sources`, optional
`sourcesContent`, an empty `names` and `mappings`. Lines and columns are
0-based; columns count UTF-16 code units in both the CSS and the sources.
A leading byte-order mark is not a column, and embedded content omits it.
CR, LF, CRLF and FF each end a source line, as in diagnostics.

- **Paths.** `sources` holds one entry per module of the build, entry first,
  in discovery order: the path relative to the directory of the CSS file
  (stdout: relative to the current directory), with `/` separators. If no
  relative path exists (for example another Windows drive) the module's
  root-relative name is used. Absolute paths are never written.
- **Content.** Source text is embedded only with `--source-map-content`.
- **Provenance policy.** Each written rule, declaration, at-rule, keyframe
  block, comment and `opaque` block has one mapping, from the first
  character of its text in the CSS (after indentation) to its *definition
  location*: the start of the source item that produced it.
  - Items expanded by `apply` map to their position inside the recipe body
    or content block, in the module that contains it. The chain of `apply`
    sites (innermost first) is available to library users as
    `Mapping::expansion`; ECMA-426 has no standard field for it, so the JSON
    does not include it.
  - Values computed by functions belong to the declaration that called the
    function; the declaration maps to where it is written.
  - Rules lowered from nesting map to the nested `rule` item that wrote
    their selector; the segments of a rule split by nested rules map to the
    parent `rule`. Rules produced by `each` map to the `rule` item in the
    loop body, once per iteration.
  - Items of loaded modules map into those modules' sources.
  - The `sourceMappingURL` comment and closing braces are not mapped.

## Diagnostics

`human` (default) writes the rendering of LANGUAGE.md §16:

```text
error[E1002]: unresolved name `nope`
  --> main.weave:3:8
  = note: …
```

`json` writes exactly one JSON object per build, on one line of stderr:

```json
{"version":1,"command":"check","success":false,"output":null,
 "dependencies":["main.weave"],
 "diagnostics":[{"severity":"error","code":"E1002","message":"unresolved name `nope`",
   "location":{"file":"main.weave","line":3,"column":8,"end_line":3,"end_column":12},
   "labels":[],"notes":["…"]}]}
```

(Shown wrapped; the real object has no line breaks.) `output` is the CSS
destination as given (`-` for stdout) or `null` for `check`.
`dependencies` lists the root-relative module names reached, also after a
failure. Lines and columns are 1-based and count Unicode scalar values, like
the human format. Problems outside the compiler (an input that cannot be
read, an output that cannot be written, a collision) have `"location":null`,
and `"code":null` unless they have a diagnostic code (a missing input is
E7001). Warnings use `"severity":"warning"`. Messages are JSON-escaped; the
human format (including `watch` status lines) instead writes C0/C1 control
characters, DEL, U+2028, U+2029 and the bidirectional formatting characters
as `\u{…}`.

## Watch

`weave watch <input> -o <file>` builds once, then polls the files that can
change the result and rebuilds when one changes. It runs until interrupted.

- **What is watched.** After every build (successful or not): the input,
  every module the build loaded, and the paths of `load` statements that
  were tried, including ones that did not exist. Creating a missing module,
  deleting or renaming one, and changing `load` statements therefore cause
  the right rebuilds, and the watched set follows the new module graph.
- **Failures keep the last good output.** A failed build writes nothing:
  CSS, map and dependency list stay as the last successful build wrote them.
  The report says so (human: `[weave] build failed; out.css keeps the last
  successful output`; JSON: `"success":false`).
- **No loops.** Output files are never watched, and outputs cannot name a
  `.weave` file or an input module, so writing output cannot trigger a
  build.
- **Reports.** Human format: diagnostics followed by one status line per
  build (`[weave] wrote out.css (… bytes from … modules)`), and once
  `[weave] watching for changes every … ms (Ctrl-C to stop)`. JSON format:
  one object per build with `"command":"watch"`, and nothing else.
- **Implementation and platforms.** Polling of file metadata (existence,
  length, modification time) with the standard library only; no native
  file-notification APIs, so behaviour is the same on Linux, macOS and
  Windows. Parsed modules whose text is unchanged are reused between builds
  (`ModuleCache`); evaluation always runs in full. The watch tests
  (`crates/weave-cli/tests/watch.rs`) run in CI on Linux, macOS and Windows.
- **Limitations.** A change that keeps a file's length within the file
  system's timestamp granularity (for example two edits in the same second on
  a file system with one-second timestamps) is noticed only with the next
  change. A change to a file made while a build reads it is detected if the
  file was already watched before that build. Polling cost grows with the
  number of watched files. Editors that save by replacing files are
  supported (a replacement changes the modification time).
