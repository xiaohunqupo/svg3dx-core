# svg3dx-core

Kernel-agnostic parser and evaluator for the [svg3dx](https://github.com/xiaohunqupo/svg3dx) format.

Format specification and samples live in the separate **svg3dx** format repository (`SVG3DX_ROOT`, default sibling `../svg3dx`). This crate does not vendor them.

Related: [svg3dx-core](https://github.com/xiaohunqupo/svg3dx-core) · [svg3dx-viewer](https://github.com/xiaohunqupo/svg3dx-viewer) · [svg3dx-studio](https://github.com/xiaohunqupo/svg3dx-studio) · [docs.rs](https://docs.rs/svg3dx-core)

```rust
use svg3dx_core::{parse_and_evaluate, DocumentSession, Value};
use std::collections::HashMap;

// One-shot
let result = parse_and_evaluate(svg_text)?;
println!("{}", svg3dx_core::to_json(&result)?);

// Session: parse once, patch params, get node-level diffs
let (mut session, update) = DocumentSession::open(svg_text)?;
let mut patch = HashMap::new();
patch.insert("D".into(), Value::Number(180.0));
let update = session.patch_params(patch)?;
for change in &update.changes {
    println!("{} {:?}", change.node_id, change.kind);
}
```

Output is a tree of `GeometryInstruction` values (`Primitive` / `BoolOp` / `Group` / `Instance`) with stable `node_id`s, plus `<s3dx:model>` descriptors. `UpdateResult.changes` lists Added / Removed / Updated nodes for local scene patching in viewer / studio.
