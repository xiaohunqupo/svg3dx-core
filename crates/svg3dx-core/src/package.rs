//! `.svg3dxz` package codec (ZIP, in-memory).
//!
//! Entry file is always `main.svg3dx` at the package root. Sidecar assets use
//! relative paths without `..`. Hosts obtain/persist bytes; this module does
//! not touch the filesystem.

use std::io::{Cursor, Read, Write};

use indexmap::IndexMap;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::{Error, Result};

/// Canonical entry path inside a `.svg3dxz` package.
pub const MAIN_ENTRY: &str = "main.svg3dx";

/// In-memory svg3dx document package (single file or unpacked `.svg3dxz`).
#[derive(Debug, Clone, Default)]
pub struct Svg3dxPackage {
    /// UTF-8 contents of `main.svg3dx` (or a standalone `.svg3dx` document).
    pub main_xml: String,
    /// Relative path → file bytes (does not include `main.svg3dx`).
    pub assets: IndexMap<String, Vec<u8>>,
}

impl Svg3dxPackage {
    pub fn new(main_xml: impl Into<String>) -> Self {
        Self {
            main_xml: main_xml.into(),
            assets: IndexMap::new(),
        }
    }

    pub fn set_asset(&mut self, path: impl Into<String>, bytes: Vec<u8>) -> Result<()> {
        let path = normalize_package_path(&path.into())?;
        if path == MAIN_ENTRY {
            return Err(Error::Package(format!(
                "asset path must not be {MAIN_ENTRY}"
            )));
        }
        self.assets.insert(path, bytes);
        Ok(())
    }
}

/// Open a `.svg3dxz` ZIP from memory. Requires root entry [`MAIN_ENTRY`].
pub fn open_svg3dxz(bytes: &[u8]) -> Result<Svg3dxPackage> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| Error::Package(format!("invalid zip: {e}")))?;

    let mut main_xml: Option<String> = None;
    let mut assets = IndexMap::new();

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| Error::Package(format!("zip entry: {e}")))?;
        let raw_name = file.name().to_string();
        if raw_name.ends_with('/') {
            continue;
        }
        let path = normalize_package_path(&raw_name)?;
        let mut buf = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut buf)
            .map_err(|e| Error::Package(format!("read {path}: {e}")))?;

        if path == MAIN_ENTRY {
            let text = String::from_utf8(buf)
                .map_err(|e| Error::Package(format!("{MAIN_ENTRY} is not UTF-8: {e}")))?;
            main_xml = Some(text);
        } else {
            assets.insert(path, buf);
        }
    }

    let main_xml = main_xml.ok_or_else(|| {
        Error::Package(format!("missing package entry `{MAIN_ENTRY}`"))
    })?;

    Ok(Svg3dxPackage { main_xml, assets })
}

/// Write a `.svg3dxz` ZIP to memory. Always emits [`MAIN_ENTRY`] plus assets.
pub fn write_svg3dxz(package: &Svg3dxPackage) -> Result<Vec<u8>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let file_opts = || SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file(MAIN_ENTRY, file_opts())
        .map_err(|e| Error::Package(format!("write {MAIN_ENTRY}: {e}")))?;
    zip.write_all(package.main_xml.as_bytes())
        .map_err(|e| Error::Package(format!("write {MAIN_ENTRY} body: {e}")))?;

    for (path, data) in &package.assets {
        let path = normalize_package_path(path)?;
        if path == MAIN_ENTRY {
            return Err(Error::Package(format!(
                "asset path must not be {MAIN_ENTRY}"
            )));
        }
        zip.start_file(&path, file_opts())
            .map_err(|e| Error::Package(format!("write {path}: {e}")))?;
        zip.write_all(data)
            .map_err(|e| Error::Package(format!("write {path} body: {e}")))?;
    }

    let cursor = zip
        .finish()
        .map_err(|e| Error::Package(format!("finish zip: {e}")))?;
    Ok(cursor.into_inner())
}

/// Open either a standalone UTF-8 `.svg3dx` document or a `.svg3dxz` package.
///
/// `hint` is typically a file name / extension (e.g. `foo.svg3dxz`). When hint
/// is absent, ZIP local-file magic (`PK\\x03\\x04` / `PK\\x05\\x06`) selects package mode.
pub fn open_document_bytes(bytes: &[u8], hint: Option<&str>) -> Result<Svg3dxPackage> {
    if looks_like_svg3dxz(bytes, hint) {
        open_svg3dxz(bytes)
    } else {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| Error::Package(format!("document is not UTF-8: {e}")))?
            .to_string();
        Ok(Svg3dxPackage::new(text))
    }
}

fn looks_like_svg3dxz(bytes: &[u8], hint: Option<&str>) -> bool {
    if let Some(h) = hint {
        let lower = h.to_ascii_lowercase();
        if lower.ends_with(".svg3dxz") {
            return true;
        }
        if lower.ends_with(".svg3dx") || lower.ends_with(".svg") {
            return false;
        }
    }
    matches!(
        bytes,
        [0x50, 0x4B, 0x03, 0x04, ..]
            | [0x50, 0x4B, 0x05, 0x06, ..]
            | [0x50, 0x4B, 0x07, 0x08, ..]
    )
}

/// Normalize a package-relative path: `/` separators, no `..`, no absolute form.
pub fn normalize_package_path(path: &str) -> Result<String> {
    let path = path.replace('\\', "/");
    let path = path.trim_start_matches("./");
    if path.is_empty() {
        return Err(Error::Package("empty package path".into()));
    }
    if path.starts_with('/') || path.contains(':') {
        return Err(Error::Package(format!(
            "absolute or drive paths are not allowed: {path}"
        )));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(Error::Package(format!(
                "parent path segments are not allowed: {path}"
            )));
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(Error::Package("empty package path".into()));
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_package() {
        let mut pkg = Svg3dxPackage::new("<svg svg3d=\"true\"/>");
        pkg.set_asset("assets/box.glb", vec![1, 2, 3, 4]).unwrap();
        let bytes = write_svg3dxz(&pkg).unwrap();
        assert!(bytes.starts_with(&[0x50, 0x4B]));
        let opened = open_svg3dxz(&bytes).unwrap();
        assert_eq!(opened.main_xml, "<svg svg3d=\"true\"/>");
        assert_eq!(opened.assets.get("assets/box.glb").unwrap(), &[1, 2, 3, 4]);
    }

    #[test]
    fn missing_main_errors() {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            let opts =
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zip.start_file("assets/x.glb", opts).unwrap();
            zip.write_all(&[9]).unwrap();
            zip.finish().unwrap();
        }
        let err = open_svg3dxz(&cursor.into_inner()).unwrap_err();
        assert!(err.to_string().contains(MAIN_ENTRY), "{err}");
    }

    #[test]
    fn open_document_bytes_plain_xml() {
        let xml = b"<svg svg3d=\"true\"/>";
        let pkg = open_document_bytes(xml, Some("a.svg3dx")).unwrap();
        assert_eq!(pkg.main_xml, "<svg svg3d=\"true\"/>");
        assert!(pkg.assets.is_empty());
    }

    #[test]
    fn reject_dotdot() {
        assert!(normalize_package_path("../x").is_err());
        assert!(normalize_package_path("a/../b").is_err());
    }
}
