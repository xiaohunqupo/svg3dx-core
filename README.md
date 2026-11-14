<p align="center">
  <img src="assets/title.png" alt="svg 3dx core" width="360"/>
</p>

# svg3dx-core

svg3dx 官方**引擎库**：在 Rust 中解析与求值，经 WASM / C ABI 供给查看器组件、编辑器与桌面宿主。

## 关联库

| 仓库 | 职责 |
|------|------|
| [svg3dx](https://github.com/xiaohunqupo/svg3dx) | 格式规范与 samples（`SVG3DX_ROOT`） |
| **svg3dx-core**（本仓） | 解析求值 + WASM + C ABI |
| [svg3dx-viewer](https://github.com/xiaohunqupo/svg3dx-viewer) | 可嵌入的**查看器组件** |
| [svg3dx-studio](https://github.com/xiaohunqupo/svg3dx-studio) | **svg3dx 编辑器** |

## 架构

```
svg3dx-core (Rust)  →  几何指令序列 (JSON)
        │
   ┌────┴────┐
 WASM FFI   C ABI
```

不做网格、布尔实体运算或渲染——那是宿主（viewer / OCCT 等）的职责。

**会话 API**：`DocumentSession`（WASM：`Svg3dxSession`，C：`svg3dx_session_*`）支持解析一次后改参重算，返回带稳定 `node_id` 的 `UpdateResult`（全量结果 + 节点 diff），供 viewer / studio 局部更新场景。

## 仓库结构

```
svg3dx-core/
├── assets/                # Logo 与标题图
├── crates/
│   ├── svg3dx-core/       # 解析与求值
│   ├── svg3dx-ffi-wasm/   # wasm-bindgen
│   └── svg3dx-ffi-c/      # C ABI → svg3dx_core.dll
├── cpp/                   # 头文件 + dump_json
├── pkg/                   # wasm-pack 产出（构建后）
├── scripts/build-release.ps1
└── docs/
```

## 构建

```powershell
.\scripts\build-release.ps1
```

或手动：

```bash
cargo test -p svg3dx-core
cargo build -p svg3dx-ffi-c --release
wasm-pack build crates/svg3dx-ffi-wasm --target bundler --out-dir pkg --out-name svg3dx_ffi_wasm
cmake -S cpp -B cpp/build && cmake --build cpp/build
```

```powershell
.\cpp\build\Debug\dump_json.exe ..\svg3dx\samples\draft-0.1-example.svg3dx `
  --dll .\target\release\svg3dx_core.dll
```

## 多仓关系

详见 [docs/REPOS.md](docs/REPOS.md)。

## 许可证

MIT OR MulanPSL-2.0（与规范仓一致）。
