# Weave for VS Code

Syntax highlighting for [Weave](https://github.com/ahmadmirzamasood/weave-css) stylesheets (`.weave`): statement keywords, `rule` selectors, CSS declarations and at-rules, `#{…}` interpolation, strings, CSS fragments in backticks, numbers with units, hex colors, built-in namespaces (`css.`, `math.`, `color.`, …) and comments. It also sets comment toggling, bracket matching and auto-closing pairs.

This is a TextMate grammar only, not a language server: there is no completion, go-to-definition or live diagnostics. Run `weavec check` for errors.

Fenced blocks tagged `weave` in Markdown are highlighted too.

Licensed under the MIT license.
