# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-11-27

Aligned with svg3dx draft 0.2. Draft 0.1 was never released; a 0.1 document is still accepted.

### Added

- `<s3dx:point>` and live `ref(#id)` on point and point-list attributes
- SMIL-shaped animation descriptors on `EvaluationResult.animations` (`<s3dx:animate>`, `<s3dx:animateTransform>`), including an optional element `id`

### Removed

- `<s3dx:exp>` parameter overrides. Expressions are written on primitive attributes (`var(--*)`, `calc(...)`). A leftover `<s3dx:exp>` node is ignored like any other unrecognized node.

[0.2.0]: https://github.com/xiaohunqupo/svg3dx-core/compare/v0.1.2...v0.2.0
