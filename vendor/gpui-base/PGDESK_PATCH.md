# gpui-base 0.8.1 — PG Desk patch

Unmodified copy of `gpui-base` 0.8.1 from crates.io (Apache-2.0), used via
`[patch.crates-io]` in the workspace `Cargo.toml`. Only change, in
`src/input/base/element.rs` (search for "PG Desk patch"):

- line numbers are left-aligned (`{:<width$}` instead of `{:>width$}`);
- the gutter reserves 2 digits minimum instead of 3.

Upstream has no option for either. To drop the patch, delete this folder and
the `[patch.crates-io]` entry.
