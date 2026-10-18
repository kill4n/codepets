use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const REQUIRED: [&str; 6] = ["working", "idle", "needs_input", "delegating", "waiting_peer", "gone"];

fn default_true() -> bool { true }
fn default_scale() -> u32 { 1 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Animation {
    pub sheet: String,
    pub row: u32,
    pub frames: u32,
    pub fps: u32,
    #[serde(default = "default_true")]
    pub r#loop: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub name: String,
    pub frame_size: [u32; 2],
    #[serde(default = "default_scale")]
    pub scale: u32,
    pub animations: BTreeMap<String, Animation>,
    /// Frame pixels of empty space under the sprite's feet; the name label is pulled up over it so
    /// it sits right under the creature instead of under the frame.
    #[serde(default)]
    pub label_overlap: u32,
    /// Runtime-only: height (px) of the session-name label drawn under the sprite, 0 when hidden.
    /// Never read from pack.json; set by the app when it applies the user's settings.
    #[serde(default, skip_deserializing)]
    pub label_h: u32,
    /// Runtime-only: extra fractional zoom applied on top of `scale` (subagent sprites use 0.5 when
    /// the integer scale cannot be halved). 1.0 for normal sprites.
    #[serde(default = "default_zoom", skip_deserializing)]
    pub zoom: f64,
}

fn default_zoom() -> f64 { 1.0 }

#[derive(Debug)]
pub enum PackError {
    Io(std::io::Error),
    Json(serde_json::Error),
    MissingAnimation(String),
    SheetNotFound(PathBuf),
    BadPng(PathBuf),
    BadDimensions { sheet: String, width: u32, height: u32, frame: [u32; 2] },
    RowOutOfRange { animation: String, row: u32, rows: u32 },
    FramesOutOfRange { animation: String, frames: u32, cols: u32 },
}
impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{self:?}") }
}
impl std::error::Error for PackError {}
impl From<std::io::Error> for PackError { fn from(e: std::io::Error) -> Self { PackError::Io(e) } }
impl From<serde_json::Error> for PackError { fn from(e: serde_json::Error) -> Self { PackError::Json(e) } }

fn png_size(path: &Path) -> Result<(u32, u32), PackError> {
    let f = std::fs::File::open(path)?;
    let dec = png::Decoder::new(std::io::BufReader::new(f));
    let reader = dec.read_info().map_err(|_| PackError::BadPng(path.to_path_buf()))?;
    let info = reader.info();
    Ok((info.width, info.height))
}

pub fn load_pack(dir: &Path) -> Result<Manifest, PackError> {
    let text = std::fs::read_to_string(dir.join("pack.json"))?;
    let m: Manifest = serde_json::from_str(&text)?;
    for req in REQUIRED {
        if !m.animations.contains_key(req) { return Err(PackError::MissingAnimation(req.to_string())); }
    }
    let [fw, fh] = m.frame_size;
    for (name, a) in &m.animations {
        let sheet_path = dir.join(&a.sheet);
        if !sheet_path.is_file() { return Err(PackError::SheetNotFound(sheet_path)); }
        let (w, h) = png_size(&sheet_path)?;
        if fw == 0 || fh == 0 || w % fw != 0 || h % fh != 0 {
            return Err(PackError::BadDimensions { sheet: a.sheet.clone(), width: w, height: h, frame: m.frame_size });
        }
        let rows = h / fh; let cols = w / fw;
        if a.row >= rows { return Err(PackError::RowOutOfRange { animation: name.clone(), row: a.row, rows }); }
        if a.frames == 0 || a.frames > cols {
            return Err(PackError::FramesOutOfRange { animation: name.clone(), frames: a.frames, cols });
        }
    }
    Ok(m)
}

/// A pack directory plus the human name from its manifest (falls back to the directory name).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PackInfo { pub dir: String, pub name: String }

pub fn list_pack_infos(root: &Path) -> Vec<PackInfo> {
    list_packs(root).into_iter().map(|dir| {
        let name = load_pack(&root.join(&dir)).map(|m| m.name).unwrap_or_else(|_| dir.clone());
        PackInfo { dir, name }
    }).collect()
}

pub fn list_packs(root: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(root) else { return vec![] };
    let mut v: Vec<String> = rd.filter_map(|e| e.ok())
        .filter(|e| e.path().join("pack.json").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_png(path: &std::path::Path, w: u32, h: u32) {
        let f = fs::File::create(path).unwrap();
        let mut enc = png::Encoder::new(f, w, h);
        enc.set_color(png::ColorType::Rgba); enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().unwrap();
        wr.write_image_data(&vec![0u8; (w * h * 4) as usize]).unwrap();
    }
    fn full_json(sheet: &str) -> String {
        let anim = |row: u32| format!(r#"{{"sheet":"{sheet}","row":{row},"frames":4,"fps":4}}"#);
        format!(r#"{{"name":"t","frameSize":[16,16],"animations":{{
          "working":{},"idle":{},"needs_input":{},"delegating":{},"waiting_peer":{},"gone":{}}}}}"#,
          anim(0), anim(1), anim(2), anim(3), anim(4), anim(5))
    }

    #[test] fn valid_pack_loads_with_defaults() {
        let d = tempfile::tempdir().unwrap();
        write_png(&d.path().join("s.png"), 64, 96);
        fs::write(d.path().join("pack.json"), full_json("s.png")).unwrap();
        let m = load_pack(d.path()).unwrap();
        assert_eq!(m.scale, 1);
        assert!(m.animations["working"].r#loop);
        assert_eq!(m.frame_size, [16, 16]);
    }

    #[test] fn missing_animation_is_error() {
        let d = tempfile::tempdir().unwrap();
        write_png(&d.path().join("s.png"), 64, 96);
        let j = full_json("s.png").replace(r#""gone":"#, r#""ghost":"#);
        fs::write(d.path().join("pack.json"), j).unwrap();
        assert!(matches!(load_pack(d.path()), Err(PackError::MissingAnimation(a)) if a == "gone"));
    }

    #[test] fn missing_sheet_file_is_error() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("pack.json"), full_json("nope.png")).unwrap();
        assert!(matches!(load_pack(d.path()), Err(PackError::SheetNotFound(_))));
    }

    #[test] fn sheet_not_multiple_of_frame_size_is_error() {
        let d = tempfile::tempdir().unwrap();
        write_png(&d.path().join("s.png"), 50, 96);
        fs::write(d.path().join("pack.json"), full_json("s.png")).unwrap();
        assert!(matches!(load_pack(d.path()), Err(PackError::BadDimensions { .. })));
    }

    #[test] fn row_out_of_range_is_error() {
        let d = tempfile::tempdir().unwrap();
        write_png(&d.path().join("s.png"), 64, 32); // only 2 rows
        fs::write(d.path().join("pack.json"), full_json("s.png")).unwrap();
        assert!(matches!(load_pack(d.path()), Err(PackError::RowOutOfRange { .. })));
    }

    #[test] fn list_packs_returns_dirs_with_manifest() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("a")).unwrap();
        fs::write(root.path().join("a/pack.json"), "{}").unwrap();
        fs::create_dir(root.path().join("b")).unwrap();
        assert_eq!(list_packs(root.path()), vec!["a".to_string()]);
    }

    #[test] fn bundled_packs_are_valid() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/packs");
        let m = load_pack(&root.join("default")).expect("default pack must validate");
        assert_eq!(m.name, "Slime");
        let m = load_pack(&root.join("fox")).expect("fox pack must validate");
        assert_eq!(m.name, "Fox");
        assert_eq!(list_packs(&root), vec!["default".to_string(), "fox".to_string()]);
    }

    #[test] fn list_pack_infos_returns_dir_and_manifest_name() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("a")).unwrap();
        write_png(&root.path().join("a/s.png"), 64, 96);
        fs::write(root.path().join("a/pack.json"), full_json("s.png").replace(r#""name":"t""#, r#""name":"Pretty""#)).unwrap();
        fs::create_dir(root.path().join("broken")).unwrap();
        fs::write(root.path().join("broken/pack.json"), "{}").unwrap();
        let infos = list_pack_infos(root.path());
        assert_eq!(infos.len(), 2);
        assert_eq!((infos[0].dir.as_str(), infos[0].name.as_str()), ("a", "Pretty"));
        assert_eq!((infos[1].dir.as_str(), infos[1].name.as_str()), ("broken", "broken"), "invalid manifest falls back to dir name");
    }
}
