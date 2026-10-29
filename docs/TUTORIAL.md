# Weave tutorial

This tutorial builds a small stylesheet step by step. Every `weave` block with a `css` block after it is compiled by the test suite (`crates/weave-compiler/tests/docs.rs`), so the output shown is what the compiler produces. The language rules are in the [language reference](LANGUAGE.md); this is the guided tour.

Weave is a **release candidate** (0.1.0-rc.2). Two things in this tour are limited today: the `color` built-ins are not implemented (see [Limitations](#limitations)), and there is no editor integration beyond syntax highlighting.

## 1. Install and build the first file

Follow the [installation guide](INSTALL.md), then check the command works:

```bash
weavec --version
```

Create `styles.weave`:

```weave
rule .button {
    color: white;
    padding: 8px 16px;
}
```

```css
.button {
  color: white;
  padding: 8px 16px;
}
```

Build it:

```bash
weavec build styles.weave            # CSS to stdout
weavec build styles.weave -o site.css
```

Two things to notice. Style rules always start with the keyword `rule`; Weave never guesses between a selector and a declaration. And everything else, such as `color: white;`, is ordinary CSS and is emitted as written.

## 2. Bindings and interpolation

`bind` gives a name to a value. `#{…}` inserts a computed value into CSS.

```weave
bind brand = #0b5fff;
bind radius = 6px;

rule .button {
    background: #{brand};
    border-radius: #{radius};
    color: brand;
}
```

```css
.button {
  background: #0b5fff;
  border-radius: 6px;
  color: brand;
}
```

The last line shows the main design rule: a bare word in a CSS value is CSS, never a variable. Only `#{…}` reaches Weave values. Bindings are immutable and scoped to the block they are written in. There is no assignment.

## 3. Numbers, units and expressions

Numbers keep their units, and incompatible units are errors rather than guesses.

```weave
bind gap = 4px;

rule .stack {
    gap: #{gap * 3};
    margin: #{gap * 2} 0;
    width: #{100% - 25%};
}
```

```css
.stack {
  gap: 12px;
  margin: 8px 0;
  width: 75%;
}
```

```weave
bind width = 1px + 1s;
```

```text
error[E3001]: incompatible units: cannot add `px` and `s`
```

Weave does not turn `calc()` into arithmetic. `calc(100% - 4px)` is CSS and stays CSS; use `#{…}` only for values Weave computes. The full numeric model is in [TYPES.md](TYPES.md).

## 4. Lists, maps and loops

```weave
bind palette = {primary: #0b5fff, danger: #d92d20};

each name, value in palette {
    rule .text-#{name} {
        color: #{value};
    }
}
```

```css
.text-primary {
  color: #0b5fff;
}

.text-danger {
  color: #d92d20;
}
```

Maps keep insertion order. `each i, item in [..]` also gives the index, and `range(0, 3)` makes the list `[0, 1, 2]`.

## 5. Conditions

Conditions must be booleans; there is no implicit truthiness.

```weave
bind dense = true;

rule .row {
    when dense {
        padding: 4px;
    } otherwise {
        padding: 12px;
    }
}
```

```css
.row {
  padding: 4px;
}
```

## 6. Functions

Functions compute values. They are pure: they cannot emit CSS and they only see their own module.

```weave
bind unit = 4px;

fn space(steps: number) -> length {
    yield unit * steps;
}

rule .card {
    padding: #{space(4)};
    margin-bottom: #{space(2)};
}
```

```css
.card {
  padding: 16px;
  margin-bottom: 8px;
}
```

Parameters can have defaults and be passed by name: `space(steps: 2)`. The built-in functions (`math`, `list`, `map`, `string`, `css`, `meta`) are listed in [BUILTINS.md](BUILTINS.md).

## 7. Recipes: reusable rules

A recipe is a reusable piece of a rule body. `apply` expands it.

```weave
recipe focus-ring(ring: string = "#0b5fff") {
    outline: 2px solid #{css.fragment(ring)};
    outline-offset: 2px;
}

rule .link:focus-visible {
    apply focus-ring();
}

rule .field:focus-visible {
    apply focus-ring("#d92d20");
}
```

```css
.link:focus-visible {
  outline: 2px solid #0b5fff;
  outline-offset: 2px;
}

.field:focus-visible {
  outline: 2px solid #d92d20;
  outline-offset: 2px;
}
```

Strings are data and are always escaped for where they land, so `css.fragment(…)` is how you say "these are CSS tokens" explicitly. A recipe can also take a block, which it places with `slot;`:

```weave
recipe on-hover {
    rule &:hover {
        slot;
    }
}

rule .tile {
    color: #333333;
    apply on-hover {
        color: #000000;
    }
}
```

```css
.tile {
  color: #333333;
}

.tile:hover {
  color: #000000;
}
```

The block sees the caller's names; the recipe body sees only its own module and parameters.

## 8. Nesting

Nested rules are lowered to flat CSS with the specificity CSS Nesting defines. `&` is the parent; a nested selector without `&` is a descendant, or relative if it starts with a combinator.

```weave
rule .nav {
    display: flex;
    rule a { color: inherit; }
    rule > li { list-style: none; }
    rule &:hover { opacity: 0.9; }
    @media (width >= 768px) {
        gap: 24px;
    }
}
```

```css
.nav {
  display: flex;
}

.nav a {
  color: inherit;
}

.nav > li {
  list-style: none;
}

.nav:hover {
  opacity: 0.9;
}

@media (width >= 768px) {
  .nav {
    gap: 24px;
  }
}
```

When a parent has several selectors, Weave writes the parent as `:is(…)` instead of multiplying selectors, which keeps output small and specificity correct. [CSS-COMPATIBILITY.md §4](CSS-COMPATIBILITY.md#4-nesting-lowering-and-specificity) has the details.

## 9. At-rules and verbatim CSS

Known at-rules keep their CSS syntax. Unknown at-rules and `opaque { … }` blocks are preserved exactly, so CSS that Weave does not know yet still works.

```weave
@layer base, components;

@future-feature demo {
    keep:   this  exactly;
}

opaque {
    .legacy > .item::part(label) { color: red; }
}
```

```css
@layer base, components;

@future-feature demo {
    keep:   this  exactly;
}

.legacy > .item::part(label) { color: red; }
```

## 10. Modules

Split a project into files with `load`, and mark what other files may use with `export`:

```weave
/* tokens.weave */
export bind primary = #2563eb;
export fn step(n: number) -> length { yield 4px * n; }

/* main.weave */
load "./tokens.weave" as tokens;

rule .button {
    color: #{tokens.primary};
    padding: #{tokens.step(2)};
}
```

Paths start with `./` or `../` and are resolved from the importing file; they cannot leave the project root. Each module is built once, and its CSS comes before the CSS of the modules that load it. [examples/design-system](../examples/design-system/main.weave) is a complete multi-file project:

```bash
weavec build examples/design-system/main.weave -o site.css --source-map
```

## 11. Tooling

```bash
weavec check styles.weave                       # report errors, write nothing
weavec build styles.weave -o site.css --source-map
weavec watch styles.weave -o site.css           # rebuild on change
weavec build styles.weave --diagnostics json    # machine-readable errors
```

Errors name a stable code, the file, line and column, and often a note:

```weave
rule .a {
    color: #{missing};
}
```

```text
error[E1002]: unresolved name `missing`
```

Every option is in the [command-line reference](CLI.md). Syntax highlighting for editors is in [editors/](../editors/README.md).

## Limitations

- The `color` built-ins (`color.rgb`, `color.mix`, …) report error E5001, "not implemented by this build". Colors are values (hex literals) that you can store, pass and insert, but not compute on. Use `` `color-mix(in srgb, …)` `` CSS fragments for runtime mixing.
- Very large plain-CSS inputs may exceed the default evaluation budget; see [the limits in the command-line reference](CLI.md#resource-limits).
- The language and library are pre-1.0. See the [compatibility policy](COMPATIBILITY-POLICY.md) for what may change.
