# glib 0.18.5, patched

Unmodified copy of the `glib` 0.18.5 crate from crates.io, with one fix backported:
gtk-rs/gtk-rs-core#1343 ("glib: fix UB in VariantStrIter::impl_get", released in glib 0.20.0),
which fixes the unsoundness in `VariantStrIter` (GHSA-wrw7-89jp-8q8g).

Why it's vendored: Tauri 2's Linux stack (gtk 0.18, webkit2gtk 2.0) requires glib 0.18, and there is
no patched 0.18.x release. `src-tauri/Cargo.toml` swaps it in with `[patch.crates-io]`.

Remove this directory and the patch entry once Tauri depends on glib >= 0.20.
