# ADR 0003: Numeric model and units

Status: accepted (phase 01, 2026-10-15).

## Context

Numbers must be deterministic across platforms, unsurprising in comparisons (`0.1 + 0.2`), and serialize without exponent notation. Decimal libraries add dependencies and still need a rounding policy; arbitrary precision invites unbounded growth.

## Decision

- binary64 values with an optional single unit; magnitude bounded by 1e12; non-finite results rejected (E3002).
- A six-decimal **quantized value** `roundTiesEven(x × 1e6)` drives serialization, equality, ordering and integer tests. It is transitive and consistent with what is printed.
- Only IEEE-exact operations (`+ - * / %`, rounding, min/max/clamp/abs). No transcendental functions in v0.1, so no dependence on platform math libraries.
- Strict units: no unitless/unit mixing in `+ - %` or ordering; no compound units; `==` never errors and is `false` across incompatible units.
- Exact rational conversion factors for absolute lengths, time, frequency and resolution; `rad` uses the binary64 value of 180/π. The right operand converts to the left operand's unit.
- `math.round` ties toward +∞ to match CSS `round()`; quantization uses ties-to-even.

## Consequences

`gap + 2` must be written `gap + 2px`; `100% - 4px` must be a CSS `calc()` fragment. Six decimals exceed what browsers honor for layout. Values beyond 1e12 are rejected rather than silently losing precision.
