# Weave v0.1 types, numbers and serialization

Status: normative companion to [LANGUAGE.md](LANGUAGE.md). Rationale: [ADR 0003](decisions/0003-numeric-model.md), [ADR 0004](decisions/0004-value-types-and-fragments.md), [ADR 0005](decisions/0005-interpolation-contexts.md).

## 1. Types

| Type | Values | Literal | Ordering (`<` …) | Interpolable alone |
|---|---|---|---|---|
| `number` | finite f64 with an optional single unit | `12`, `1.5rem`, `50%` | compatible units only | yes |
| `string` | sequence of Unicode scalar values | `"a"`, `'a'` | lexicographic by scalar value | yes |
| `color` | 8-bit sRGB channels r, g, b, a | `#2563eb`, `#0003` | no (E2001) | yes |
| `bool` | `true`, `false` | `true` | no (E2001) | yes |
| `list` | ordered, heterogeneous, immutable | `[a, b]` | no (E2001) | no (E2004) |
| `map` | insertion-ordered, string keys, immutable | `{k: v}` | no (E2001) | no (E2004) |
| `css` | validated CSS component-value text (a *fragment*) | `` `1px solid` `` | no (E2001) | yes |

There is no null, no undefined and no implicit truthiness. All values are immutable; operations return new values.

### 1.1 Coercions

Weave performs **no implicit coercion between types**. The only implicit conversions are:

| From | To | When |
|---|---|---|
| number in a convertible unit | the other operand's unit | arithmetic and comparison (§2.4) |
| any interpolable value | text | interpolation, by context ([LANGUAGE §7](LANGUAGE.md#7-interpolation-and-escaping)) |

Explicit conversions:

| Conversion | How |
|---|---|
| any interpolable value → string | `"#{value}"` |
| string → identifier fragment | `css.ident(s)` |
| string → fragment | `css.fragment(s)` (validated) |
| list → fragment | `css.space(list)`, `css.comma(list)` |
| two values → slash fragment | `css.slash(a, b)` |
| number → unitless | `math.value(n)` |
| unitless → unit | `n * 1px` |
| number → other unit | `math.convert(n, "cm")` |
| unit of a number → string | `math.unit(n)` |

## 2. Numbers

### 2.1 Representation (normative)

- A number is an IEEE 754 binary64 value plus an optional unit. Literals are parsed to the nearest binary64 value (ties to even).
- Every literal and every arithmetic result MUST be finite and satisfy `|x| ≤ 1e12`; otherwise E3002. The check runs after each operation.
- The **quantized value** is `q(x) = roundTiesEven(x × 1e6)` computed in binary64 and converted to a signed 64-bit integer (exact, since `|x × 1e6| ≤ 1e18`). Serialization, equality and ordering use `q`, so the language behaves as if numbers had six decimal places of precision: `0.1 + 0.2 == 0.3` is `true`.
- A number is an **integer** when `q(x) mod 1 000 000 = 0`.
- Negative zero has `q = 0`; it compares equal to zero and serializes as `0`.
- Only `+ - * / %`, comparisons and the rounding/min/max/clamp/abs built-ins are provided, all exactly specified by IEEE 754 binary64, so results are identical on every platform. No transcendental functions exist in v0.1.

### 2.2 Units

A unit is the identifier immediately after a numeric literal (`[A-Za-z]+`) or `%`. Units compare ASCII case-insensitively. Unknown units are allowed (future CSS) and are compatible only with themselves.

| Category | Units (canonical spelling) | Convertible |
|---|---|---|
| absolute length | `px`, `cm`, `mm`, `Q`, `in`, `pt`, `pc` | yes, within category |
| angle | `deg`, `grad`, `rad`, `turn` | yes |
| time | `s`, `ms` | yes |
| frequency | `Hz`, `kHz` | yes |
| resolution | `dppx`, `x`, `dpi`, `dpcm` | yes |
| relative length | `em rem ex rex cap rcap ch rch ic ric lh rlh vw vh vi vb vmin vmax svw svh svi svb svmin svmax lvw lvh lvi lvb lvmin lvmax dvw dvh dvi dvb dvmin dvmax cqw cqh cqi cqb cqmin cqmax` | no — identical unit only |
| percentage | `%` | no |
| flex | `fr` | no |
| other | any other identifier | no — identical unit only |

Serialized units use the canonical spelling shown; unknown units are serialized in ASCII lowercase. Relative units are never resolved: Weave has no viewport, font or containing block.

### 2.3 Conversion factors

Each convertible unit has a factor `F = n / d` relative to the category's canonical unit (first listed). All `n` and `d` are integers exactly representable in binary64, except `rad`.

| Unit | n | d | Meaning |
|---|---|---|---|
| px | 1 | 1 | |
| in | 96 | 1 | 1in = 96px |
| cm | 4800 | 127 | 96 / 2.54 |
| mm | 480 | 127 | 96 / 25.4 |
| Q | 120 | 127 | quarter millimetre |
| pt | 4 | 3 | 1in = 72pt |
| pc | 16 | 1 | 1in = 6pc |
| deg | 1 | 1 | |
| grad | 9 | 10 | |
| turn | 360 | 1 | |
| rad | 57.29577951308232 | 1 | binary64 nearest to 180/π |
| ms | 1 | 1 | |
| s | 1000 | 1 | |
| Hz | 1 | 1 | |
| kHz | 1000 | 1 | |
| dppx, x | 1 | 1 | |
| dpi | 1 | 96 | |
| dpcm | 127 | 4800 | |

Converting `v` from unit A to unit B (same category): `v' = (v × (nA × dB)) / (dA × nB)`, evaluated in binary64 in exactly that order (the two products are exact). If A and B are the same unit (case-insensitively), `v' = v` with no arithmetic.

### 2.4 Arithmetic and comparison rules

“Compatible” means: both unitless, the same unit, or two units of the same convertible category. Mixed compatible operands convert the **right** operand to the **left** operand's unit; the result carries the left unit.

| Operation | both unitless | unit, unitless | unitless, unit | unit, unit |
|---|---|---|---|---|
| `a + b`, `a - b`, `a % b` | unitless | E3001 | E3001 | compatible → left unit; else E3001 |
| `a * b` | unitless | left unit | right unit | E3004 |
| `a / b` | unitless | left unit | E3004 | compatible → unitless; else E3004 |
| `<`, `<=`, `>`, `>=` | compare `q` | E3001 | E3001 | compatible → compare `q` after conversion; else E3001 |
| `==`, `!=` | compare `q` | `false` | `false` | compatible → compare `q` after conversion; else `false` |

- `/` and `%` with a right operand exactly `0` (or `-0`) are E3003. `%` is binary64 `fmod` (result has the sign of the dividend), matching Rust `f64::rem`.
- Unary `-` negates and keeps the unit.
- `0 == 0px` is `false`: unit mismatches are never silently ignored.
- Examples: `1in + 1cm` → `1.393701in`; `1cm + 1in` → `3.54cm`; `1turn / 1deg` → `360`; `1 / 3` → `0.333333`; `10px * 2` → `20px`; `10px * 2px` → E3004; `1px + 1em` → E3001; `100% - 4px` → E3001 (use a fragment: `` `calc(100% - 4px)` ``).

## 3. Strings

- A string is a sequence of Unicode scalar values, at most 1 MiB when UTF-8 encoded (E8007).
- Literals use `"` or `'`. Escapes follow CSS: `\` + 1–6 hex digits + one optional whitespace → that code point (zero, surrogates and values above U+10FFFF become U+FFFD); `\` + line break → nothing; `\` + any other scalar → that scalar. An unescaped line break is E0002. `#{…}` interpolates in the string context; `\#{` is a literal `#{`.
- `+` concatenates two strings; string + non-string is E2001.
- Ordering compares scalar values lexicographically; equality is exact scalar equality (no Unicode normalization, no case folding).

## 4. Colors

- A color is four 8-bit channels (r, g, b, alpha). `#rgb` and `#rgba` duplicate each digit; missing alpha is `ff`. Hex digits are case-insensitive.
- Colors have no arithmetic operators; use `color.*` built-ins.
- Equality is channel-wise. Colors are not ordered.
- Wide-gamut and functional colors (`oklch(…)`, `color-mix(…)`, named colors) are written as CSS fragments and are not color values.

## 5. Serialization (value context)

| Type | Serialization |
|---|---|
| number | Let `q = q(x)`. If `q = 0` write `0`. Otherwise write `-` if `q < 0`, then the integer part `|q| div 10^6` in decimal, then if `|q| mod 10^6 ≠ 0`, `.` followed by that remainder as six digits with trailing zeros removed. In both cases append the unit in canonical spelling (`-0px` → `0px`). Never exponent notation; leading zero kept (`0.5`). Examples: `16px`, `0.333333`, `-2.5rem`, `0px`, `1000000`. |
| string | `"` + escaped contents + `"`. Escaping: U+0000 → U+FFFD; U+0001–U+001F and U+007F → `\` + lowercase hex + one space; `"` → `\"`; `\` → `\\`; everything else literal (including non-ASCII). Always double quotes. |
| color | `#rrggbb` when alpha is `ff`, else `#rrggbbaa`; lowercase hex; never shortened. |
| bool | `true` or `false`. |
| css | its text, verbatim. |
| list, map | not serializable (E2004). |

Other contexts (string, url, selector, declaration name) are defined in [LANGUAGE §7](LANGUAGE.md#71-contexts); they start from the *text* of a value: string contents unquoted, otherwise the value-context serialization.

## 6. Lists and maps

- Lists preserve order and may contain any values, including lists and maps. Indexing is zero-based (LANGUAGE §5.3). Equality is element-wise.
- Maps preserve insertion order for iteration, `map.keys` and `map.values`. Keys are strings (identifier keys in literals are strings). Duplicate keys in a literal are E2008. Equality ignores order: same key set and equal values.
- Both are bounded to 65,536 entries (E8007).
- Neither serializes implicitly. Use:
  - `css.space(list)` → items in value-context serialization joined by one space.
  - `css.comma(list)` → joined by `, `.
  - `css.slash(a, b)` → `a / b`.
  - Nested lists inside these constructors are E2004; build inner fragments first.

## 7. CSS fragments

A fragment is text that is guaranteed to be a well-formed sequence of CSS component values.

- **Literal:** `` `…` ``. The scanner respects CSS strings, comments, escapes and `url(` tokens, so a backtick inside a CSS string does not end the fragment; a backtick elsewhere ends it (write `\60 ` for a literal one). Unterminated: E0007. Interpolation uses the value context. Line breaks become LF; leading and trailing whitespace is trimmed; all other text is kept verbatim.
- **Validation** (literal after interpolation, and `css.fragment`): brackets `()` `[]` balanced; no `{` or `}`; no `;` outside strings; no bad-string or bad-url token; no unterminated comment; size ≤ 1 MiB. Violation: E2006 (or E0005 for a literal with unbalanced brackets).
- Equality compares text exactly.

## 8. Type annotations

Parameter and return annotations: `any`, `number`, `unitless`, `length` (absolute or relative length unit), `angle`, `time`, `percent`, `string`, `color`, `bool`, `list`, `map`, `css`. A unitless `0` is not a `length`. Mismatch: E4005 (argument) or E4006 (yield).

## 9. Core built-ins

All built-ins are deterministic and follow the call rules of LANGUAGE §5.4. Parameter names below are the names usable as named arguments. Phase 05 writes the full reference in `docs/BUILTINS.md`; additions or changes need an ADR.

| Built-in | Result | Notes |
|---|---|---|
| `range(start, end, step = 1)` | list | unitless integers, half-open; `step ≠ 0` (E2009 for non-integers, E3003 for zero step) |
| `css.space(items)`, `css.comma(items)` | css | see §6 |
| `css.slash(left, right)` | css | `left / right` |
| `css.ident(text)` | css | text escaped as an identifier start (LANGUAGE §7.2); empty string E2006 |
| `css.fragment(text)` | css | validated (§7) |
| `math.round(x)`, `math.floor(x)`, `math.ceil(x)` | number | keep unit; `round` ties toward +∞ (as CSS `round()`) |
| `math.abs(x)` | number | keeps unit |
| `math.min(a, b)`, `math.max(a, b)` | number | compatible units; result in `a`'s unit |
| `math.clamp(low, x, high)` | number | compatible units; result in `x`'s unit; `low > high` is E2003 |
| `math.unit(x)` | string | canonical unit spelling, `""` if unitless |
| `math.value(x)` | number | unitless |
| `math.convert(x, unit)` | number | E3005 if `unit` is unknown or incompatible |
| `list.len(l)` | number | |
| `list.append(l, value)`, `list.concat(a, b)` | list | |
| `list.slice(l, start, end)` | list | half-open, zero-based, bounds checked (E2005) |
| `list.contains(l, value)` | bool | uses `==` |
| `map.len(m)`, `map.has(m, key)` | number, bool | |
| `map.get(m, key, default)` | any | `default` returned when absent |
| `map.set(m, key, value)` | map | existing key keeps its position |
| `map.remove(m, key)` | map | |
| `map.merge(a, b)` | map | `a`'s keys in order with values from `b` when present, then `b`'s new keys in order |
| `map.keys(m)`, `map.values(m)` | list | insertion order |
| `string.len(s)` | number | scalar values |
| `string.slice(s, start, end)` | string | half-open scalar indices, bounds checked |
| `string.contains(s, needle)` | bool | |
| `string.upper(s)`, `string.lower(s)` | string | ASCII letters only (Unicode tables would vary with toolchains) |
| `string.join(items, separator)` | string | items must be strings |
| `color.rgb(r, g, b, alpha = 1)` | color | channels unitless 0–255 or `%`; alpha unitless 0–1 or `%`; values clamped, then `round(v)` ties toward +∞ (alpha `round(a × 255)`) |
| `color.mix(a, b, weight = 50%)` | color | per channel and alpha: `round(a × w + b × (1 − w))`, ties toward +∞; `w` unitless 0–1 or `%` |
| `color.red(c)`, `color.green(c)`, `color.blue(c)` | number | 0–255 |
| `color.alpha(c)` | number | `a / 255`, unitless |
| `meta.type(x)` | string | `"number"`, `"string"`, `"color"`, `"bool"`, `"list"`, `"map"`, `"css"` |
