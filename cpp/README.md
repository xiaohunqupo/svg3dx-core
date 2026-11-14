# C / C++ bindings

## Headers

[`include/svg3dx.h`](include/svg3dx.h) — C ABI for `svg3dx_core.dll` / `libsvg3dx_core.so`.

**Do not edit by hand.** Regenerate from `svg3dx-ffi-c` with cbindgen:

```powershell
.\scripts\generate-c-header.ps1
.\scripts\generate-c-header.ps1 -Check   # CI / drift check
```

```bash
./scripts/generate-c-header.sh
./scripts/generate-c-header.sh --check
```

`build-release.ps1` and GitHub Actions CI run generation / drift check before building `dump_json`.

## dump_json

Command-line host: read an `.svg` / `.svg3dx` → print geometry-instruction JSON.

Samples live in the official **svg3dx** format repo (sibling `../svg3dx` or `$env:SVG3DX_ROOT`).

```powershell
.\cpp\build\Debug\dump_json.exe ..\svg3dx\samples\draft-0.1-example.svg3dx `
  --dll .\target\release\svg3dx_core.dll

.\cpp\build\Debug\dump_json.exe ..\svg3dx\samples\draft-0.1-example.svg3dx `
  --dll .\target\release\svg3dx_core.dll `
  --model wireframe-view `
  --params '{\"R\":40}'
```

The example loads the DLL dynamically (`LoadLibrary` / `dlopen`) so an MSVC-built Rust DLL works with MinGW.

## OCCT / STEP (next)

Map `GeometryInstruction` JSON to Open CASCADE:

| Instruction | OCCT |
|-------------|------|
| `box` | `BRepPrimAPI_MakeBox` (+ orientation from lengthVec/widthVec) |
| `cylinder` | `BRepPrimAPI_MakeCylinder` |
| `sphere` | `BRepPrimAPI_MakeSphere` |
| `bool_op.union` | `BRepAlgoAPI_Fuse` |
| `bool_op.subtract` | `BRepAlgoAPI_Cut` |
| `bool_op.intersect` | `BRepAlgoAPI_Common` |

Export with `STEPControl_Writer`. Not bundled here — install OCCT separately and link against your own host.
