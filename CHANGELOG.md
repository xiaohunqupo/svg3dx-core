# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.2] - 2026-11-23

### Added

- `BoolOp` geometry instructions carry an optional `transform`
- `EvalOptions` / `DocumentSession` `extra_defs` for evaluating unused named groups on demand
- `retain_inactive` evaluation flag to keep skipped `if` / `if-not` branches in the result
- `transform` support on `group`, `if`, and `if-not` nodes
- `.svg3dxz` package open/write helpers (`zip` dependency)
- `s3dx:transform` TRS function parsing

### Changed

- Corner style attributes: `corner` / `cornerRadius` → `outerCorner` / `innerCorner` / `outerCornerRadius` / `innerCornerRadius` / `miterLimit`

## [0.1.1] - 2026-11-16

### Added

- Initial crates.io / npm (`svg3dx-ffi-wasm`) release line for the 0.1 series

[0.1.2]: https://github.com/xiaohunqupo/svg3dx-core/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/xiaohunqupo/svg3dx-core/releases/tag/v0.1.1
