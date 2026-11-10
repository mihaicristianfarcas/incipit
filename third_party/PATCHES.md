# third_party

## mitex (0.2.4, MIT OR Apache-2.0)

The Typst side of [mitex](https://github.com/mitex-rs/mitex): the definitions its converter's
output is evaluated with (`specs/`). From `https://packages.typst.org/preview/mitex-0.2.4.tar.gz`.
Compiled into `incipit-math`; `mitex.wasm` and `mitex.typ` are not used (the conversion is the
`mitex` crate's, in Rust).

Changed for Typst 0.15, in `specs/latex/standard.typ`:

- `xarrow` is defined in the file itself, not imported from `@preview/xarrow:0.2.0` (which uses
  `style`, gone from Typst).
- `\KaTeX` is written with a K: the symbol `kai` is gone.

Names the converter itself writes that Typst 0.15 no longer has (`plus.circle`, `angle.l`, `sect`,
`diff`…) are replaced with their characters in `incipit-math` (`RENAMED`).
