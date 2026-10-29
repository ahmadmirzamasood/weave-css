# Weave v0.1 built-in reference

Status: normative reference for the prelude (LANGUAGE.md §4.3, §18). It completes [TYPES.md §9](TYPES.md#9-core-built-ins), which lists the same set; where they disagree, this document wins and the difference is a specification bug. Adding or changing a built-in is a specification change and needs a decision record.

The reference compiler implements every built-in below.

## Rules shared by all built-ins

- **Deterministic and pure.** A built-in's result depends only on its arguments. No built-in reads files, the network, the clock, random sources or the environment, and none emits CSS. Results are identical on every platform.
- **Calls.** Built-ins follow LANGUAGE.md §5.4: positional arguments first, then named ones using the parameter names shown here. Too many positional arguments is E4004 and an unknown name E4002 (both at the argument); a parameter passed twice is E4003; a missing required argument is E4001 (at the callee). Parameters written `name = value` are optional.
- **Types.** An argument of the wrong type is E4005 at that argument. "Integer" means a unitless integer in the sense of TYPES.md §2.1 (E2009 otherwise). Indices are zero-based.
- **Limits.** Results are bounded by the string limit (1 MiB, E8007) and the collection limit (65,536 elements, E8007), checked before the result is built. Producing a string costs one extra evaluation step per KiB and producing a list or map one per 64 elements; `list.contains` and `==` spend one step per 64 values compared (E8004).
- **Errors** concerning the call as a whole are reported at the callee (`math.min` in `math.min(1px, 1em)`).
- **Shadowing.** A user binding, function or recipe may shadow a prelude name; the built-in is then unreachable in that scope.

## `range`

### `range(start, end, step = 1)`

A list of unitless integers from `start` up to, but excluding, `end`, `step` apart. `step` may be negative. All three must be integers (E2009); a zero `step` is E3003. A range that would exceed the collection limit is E8007 and is never built.

```weave
range(0, 3)        /* [0, 1, 2] */
range(3, 0, -1)    /* [3, 2, 1] */
range(0, 10, 3)    /* [0, 3, 6, 9] */
range(2, 2)        /* [] */
```

## `css`: explicit CSS sequences

### `css.space(items)`

A CSS fragment: the value-context serialization (TYPES.md §5) of each element of the list `items`, joined by one space. An element that is a list or map is E2004.

### `css.comma(items)`

As `css.space`, joined by `, `.

### `css.slash(left, right)`

The fragment `left / right` (each serialized in the value context). A list or map is E2004.

### `css.ident(text)`

The string `text` as an identifier fragment, escaped as the start of an identifier (LANGUAGE.md §7.2): `css.ident("10up")` is `` `\31 0up` ``. An empty string is E2006.

### `css.fragment(text)`

The string `text` as a CSS fragment after validation (TYPES.md §7): balanced `()` and `[]`, no `{`, `}` or top-level `;`, no bad or unterminated strings, URLs or comments, no lone `\` (E2006). Surrounding whitespace is trimmed and line breaks become LF.

## `math`: numbers

Rounding works on the quantized value (TYPES.md §2.1), so it agrees with serialization: `2.4999999` serializes as `2.5` and rounds to `3`. All results keep the magnitude bound (E3002).

### `math.round(x)`

`x` rounded to an integer, ties toward +∞ (as CSS `round()`): `2.5` → `3`, `-2.5` → `-2`. Keeps the unit.

### `math.floor(x)`

The largest integer not above `x`. Keeps the unit.

### `math.ceil(x)`

The smallest integer not below `x`. Keeps the unit.

### `math.abs(x)`

The magnitude of `x`. Keeps the unit.

### `math.min(a, b)`

The smaller of two numbers with compatible units (E3001 otherwise), in `a`'s unit: `math.min(1in, 90px)` is `0.9375in`.

### `math.max(a, b)`

The larger of two numbers with compatible units, in `a`'s unit.

### `math.clamp(low, x, high)`

`x` limited to `low..high`, in `x`'s unit. All three units must be compatible (E3001); `low > high` is E2003.

### `math.unit(x)`

The canonical spelling of `x`'s unit as a string; `""` when unitless: `math.unit(12PT)` is `"pt"`.

### `math.value(x)`

`x` without its unit.

### `math.convert(x, unit)`

`x` converted to the unit named by the string `unit` (TYPES.md §2.3; the name is case-insensitive). An unknown unit, or one that is not convertible from `x`'s unit (including converting a unitless number), is E3005 at `unit`.

```weave
math.convert(1in, "cm")       /* 2.54cm */
math.convert(90deg, "turn")   /* 0.25turn */
math.convert(2s, "ms")        /* 2000ms */
```

## `list`: lists

### `list.len(l)`

The number of elements.

### `list.append(l, value)`

A new list: `l` followed by `value`.

### `list.concat(a, b)`

A new list: the elements of `a`, then those of `b`.

### `list.slice(l, start, end)`

The elements from `start` up to, but excluding, `end`. Requires `0 ≤ start ≤ end ≤ list.len(l)` (E2005).

### `list.contains(l, value)`

Whether an element of `l` is `==` to `value` (TYPES.md §6).

## `map`: maps

Maps keep insertion order. Keys are strings (E4005 for other types).

### `map.len(m)`

The number of entries.

### `map.has(m, key)`

Whether `m` has the key.

### `map.get(m, key, default)`

The value for `key`, or `default` when absent.

### `map.set(m, key, value)`

A new map with `key` set to `value`. An existing key keeps its position; a new key is added last.

### `map.remove(m, key)`

A new map without `key`; `m` itself when the key is absent.

### `map.merge(a, b)`

A new map: `a`'s keys in order, with `b`'s values where `b` has the key, followed by `b`'s other keys in order.

### `map.keys(m)`

A list of the keys, in insertion order.

### `map.values(m)`

A list of the values, in insertion order.

## `string`: strings

Lengths and indices count Unicode scalar values. Case mapping covers ASCII letters only, because Unicode case tables change between toolchain versions and would make output platform-dependent.

### `string.len(s)`

The number of scalar values: `string.len("héllo")` is `5`.

### `string.slice(s, start, end)`

The scalar values from `start` up to, but excluding, `end`. Requires `0 ≤ start ≤ end ≤ string.len(s)` (E2005).

### `string.contains(s, needle)`

Whether `needle` occurs in `s`. Every string contains `""`.

### `string.upper(s)`

`s` with ASCII letters in upper case: `string.upper("abc-é")` is `"ABC-é"`.

### `string.lower(s)`

`s` with ASCII letters in lower case.

### `string.join(items, separator)`

The strings of the list `items` joined by `separator`. An element that is not a string is E4005 at `items`.

## `color`: 8-bit sRGB colors

Colors are four 8-bit channels (TYPES.md §4). These built-ins compute on them exactly, with integer arithmetic on the six-decimal quantized argument values (TYPES.md §2.1); no floating-point rounding is involved. Rounding is always to the nearest integer with **ties toward +∞**. Wide-gamut and functional colors are CSS fragments, not color values, and cannot be passed here (E4005).

A *unitless-or-percentage* argument is a number with no unit, or a percentage; any other unit is E3001 at that argument.

### `color.rgb(r, g, b, alpha = 1)`

A color from channels. `r`, `g` and `b` are unitless numbers 0–255 or percentages 0%–100% (100% is 255); `alpha` is a unitless number 0–1 or a percentage 0%–100% (100% is 1). Values outside the range are clamped to it, then scaled to 0–255 and rounded (`alpha` becomes `round(alpha × 255)`). Channels may be fractional: `127.5` rounds to `128`.

```weave
color.rgb(37, 99, 235)            /* #2563eb */
color.rgb(100%, 0%, 50%)          /* #ff0080 */
color.rgb(300, -5, 127.5, 0.5)    /* #ff008080: clamped, 127.5 rounds up */
color.rgb(0, 0, 0, 50%)           /* #00000080 */
```

### `color.mix(a, b, weight = 50%)`

The color `weight` of the way from `b` to `a`: each channel, and alpha, is `round(a × w + b × (1 − w))` with `w` the weight as a fraction. `weight` is a unitless number 0–1 or a percentage 0%–100%; a value outside that range is E2003 at the argument (it is not clamped), a unit other than `%` is E3001. Alpha is mixed like a channel (no premultiplication). `weight` 1 gives `a` and 0 gives `b`.

```weave
color.mix(#000000, #ffffff)               /* #808080: 127.5 rounds up */
color.mix(#ff0000, #0000ff, 25%)          /* #4000bf */
color.mix(#ff000080, #0000ff, 0.5)        /* #800080c0 */
```

### `color.red(c)`

The red channel of `c` as a unitless integer 0–255.

### `color.green(c)`

The green channel of `c` as a unitless integer 0–255.

### `color.blue(c)`

The blue channel of `c` as a unitless integer 0–255.

### `color.alpha(c)`

The alpha as a unitless number: `a / 255`, quantized to six decimals when serialized (`#2563eb80` gives `0.501961`, an opaque color gives `1`).

## `meta`: values

### `meta.type(x)`

The type of `x` as a string: `"number"`, `"string"`, `"color"`, `"bool"`, `"list"`, `"map"` or `"css"`.
