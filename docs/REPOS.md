# 多仓一览

| 目录 | 产品定位 | 仓库 |
|------|----------|------|
| `f:\svg3dx` | 格式规范 + samples | [svg3dx](https://github.com/xiaohunqupo/svg3dx) |
| `f:\svg3dx-core` | 引擎（本仓）：解析 / WASM / C ABI | [svg3dx-core](https://github.com/xiaohunqupo/svg3dx-core) |
| `f:\svg3dx-viewer` | **查看器组件**（可嵌入） | [svg3dx-viewer](https://github.com/xiaohunqupo/svg3dx-viewer) |
| `f:\svg3dx-studio` | **svg3dx 编辑器** | [svg3dx-studio](https://github.com/xiaohunqupo/svg3dx-studio) |

构建顺序：`svg3dx-core`（wasm-pack → `pkg/`）→ `svg3dx-viewer` → `svg3dx-studio`。

环境变量：

- `SVG3DX_ROOT`：规范仓根（默认 `../svg3dx`）
- `SVG3DX_CORE_ROOT`：引擎仓根（viewer/studio 开发时默认 `../svg3dx-core`）

> 若仍打开着旧目录 `f:\svg3dxlib`，请改开 `f:\svg3dx-core`（旧目录仅作迁移残留）。
