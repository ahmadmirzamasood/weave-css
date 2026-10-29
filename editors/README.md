# Editor support

Basic syntax highlighting for `.weave` files. This is a TextMate grammar and
a language configuration (comments, brackets, auto-closing pairs). It is
**not** a language server: there is no completion, go-to-definition or live
diagnostics. Nothing here is published to an extension marketplace.

| File | Purpose |
|---|---|
| [vscode/syntaxes/weave.tmLanguage.json](vscode/syntaxes/weave.tmLanguage.json) | TextMate grammar, scope `source.weave` |
| [vscode/language-configuration.json](vscode/language-configuration.json) | Comment, bracket and indentation rules for VS Code |
| [vscode/package.json](vscode/package.json) | Local VS Code extension manifest (`private`, not published) |

`python3 scripts/check_editor.py` validates these files (JSON, regular
expressions, includes, scope names) and runs a small highlighting smoke test
over `examples/`; CI runs it.

## Highlighted constructs

Statement keywords (`load … as`, `export`, `bind`, `fn`, `recipe`, `rule`,
`when`/`otherwise`, `each … in`, `apply`, `slot`, `yield`, `fail`, `warn`,
`opaque`), `rule` selectors up to their block, `&`, at-rules, declarations
and `!important`, `/* */` and preserved `/*! */` comments, strings, CSS
fragments in backticks, `#{…}` interpolation (including inside strings) and
the literal marker `\#{`, numbers with units, hex colors, booleans, type
annotations, built-in namespaces (`css.`, `math.`, `list.`, `map.`,
`string.`, `color.`, `meta.`) and `range`.

The grammar is line-based, like every TextMate grammar: it does not
reproduce the compiler's item classification exactly (for example a
selector split across lines after `rule`, or a declaration whose property
name comes from interpolation), so treat colours as a reading aid, and
`weavec check` as the authority.

## VS Code: install the extension package

A release attaches the ready-made package `weave-syntax-<version>.vsix`. From a checkout you can build it yourself with the Python standard library only (no Node, no network access):

```bash
# macOS and Linux
python3 scripts/package_vscode.py --out weave-syntax.vsix
```

```powershell
# Windows (PowerShell): the command is `python` (or `py`), not `python3`
python scripts\package_vscode.py --out weave-syntax.vsix
```

Install the `.vsix` in **Visual Studio Code**, by either way:

- **From the editor (works everywhere):** open the Extensions view (`Ctrl+Shift+X`), choose `...` -> *Install from VSIX...* and pick the file.
- **From a terminal:** `code --install-extension weave-syntax.vsix`, **if `code` is VS Code's command.** Other editors built on VS Code (Cursor, VSCodium, Windsurf, ...) install a `code`-style command too, and whichever comes first on your `PATH` wins, so the extension can silently go into the wrong editor. Check with `code --version` (VS Code prints a version such as `1.9x.x` and a commit hash) or `where code` (Windows) / `which -a code`. If it is the wrong one, call VS Code's own command by its full path:

  ```powershell
  # Windows, per-user install
  & "$env:LOCALAPPDATA\Programs\Microsoft VS Code\bin\code.cmd" --install-extension weave-syntax.vsix
  ```

  ```bash
  # macOS
  "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code" --install-extension weave-syntax.vsix
  ```

  On macOS and Linux you can also run *Shell Command: Install 'code' command in PATH* from the VS Code command palette.

Then reload the window or restart the editor (`Ctrl+Shift+P` -> *Developer: Reload Window*). Files ending in `.weave` get the language **Weave** (shown at the right of the status bar), and fenced blocks tagged `weave` in Markdown are highlighted too. To remove it: uninstall *Weave* in the Extensions view, or `code --uninstall-extension weave-local.weave-syntax`. The same package installs in the other VS Code-based editors through their *Install from VSIX* command.

The package is for local use (publisher `weave-local`, `"private": true`); publishing to a marketplace needs a publisher account and is not done by this repository.

## VS Code: install from the folder

Copy or link the `vscode` folder into your extensions directory and reload
the window:

```bash
# macOS and Linux
ln -s "$PWD/editors/vscode" ~/.vscode/extensions/weave-syntax
```

```powershell
# Windows (PowerShell)
Copy-Item -Recurse editors\vscode "$env:USERPROFILE\.vscode\extensions\weave-syntax"
```

## Configuration examples

VS Code `settings.json`, for example in `.vscode/settings.json` of a project:

```json
{
  "files.associations": { "*.weave": "weave" },
  "[weave]": {
    "editor.tabSize": 4,
    "editor.insertSpaces": true,
    "files.eol": "\n",
    "files.insertFinalNewline": true
  },
  "files.watcherExclude": { "**/dist/**": true }
}
```

A VS Code task that runs the watcher (`.vscode/tasks.json`). The JSON
diagnostics format writes one object per build on stderr, which other tools
can parse; the human format is easier to read in a terminal:

```json
{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "weave: watch",
      "type": "shell",
      "command": "weavec watch src/main.weave -o dist/site.css --root src --source-map",
      "isBackground": true,
      "problemMatcher": {
        "owner": "weave",
        "fileLocation": ["relative", "${workspaceFolder}/src"],
        "pattern": [
          { "regexp": "^(error|warning)\\[(\\w+)\\]: (.*)$", "severity": 1, "code": 2, "message": 3 },
          { "regexp": "^  --> (.+):(\\d+):(\\d+)$", "file": 1, "line": 2, "column": 3 }
        ],
        "background": {
          "activesBegin": "^\\[weavec\\] (wrote|build failed)",
          "beginsPattern": "^(error|warning)\\[",
          "endsPattern": "^\\[weavec\\] (wrote|build failed)"
        }
      }
    }
  ]
}
```

Module names in diagnostics are relative to the module root, hence
`fileLocation` relative to `src` when `--root src` is used.

[EditorConfig](https://editorconfig.org) (`.editorconfig`):

```ini
[*.weave]
charset = utf-8
end_of_line = lf
indent_style = space
indent_size = 4
insert_final_newline = true
trim_trailing_whitespace = true
```

Other editors that read TextMate grammars (for example Sublime Text, or
editors based on `vscode-textmate`) can load `weave.tmLanguage.json`
directly; associate it with the `.weave` extension.
