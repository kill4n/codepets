use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

fn default_true() -> bool { true }
fn default_wander_after() -> u32 { 20 }
fn default_permission_wait() -> u32 { 60 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SpriteSize { Small, #[default] Normal, Large }

impl SpriteSize {
    pub fn factor(self) -> f64 { match self { SpriteSize::Small => 0.5, SpriteSize::Normal => 1.0, SpriteSize::Large => 1.5 } }
    /// Effective integer scale for a pack: `scale × factor`, rounded, never below 1 (keeps pixel art crisp).
    pub fn apply(self, pack_scale: u32) -> u32 { ((pack_scale as f64 * self.factor()).round() as u32).max(1) }
    pub fn parse(s: &str) -> Option<SpriteSize> {
        match s { "small" => Some(SpriteSize::Small), "normal" => Some(SpriteSize::Normal), "large" => Some(SpriteSize::Large), _ => None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub port: u16,
    pub token: String,
    pub pack: String,
    pub bind_all: bool,
    /// On-screen sprite size, applied on top of the pack's own `scale`.
    #[serde(default)]
    pub size: SpriteSize,
    /// Seconds a permission prompt is held for the bubble before the terminal asks (0 = never hold).
    #[serde(default = "default_permission_wait")]
    pub permission_wait_secs: u32,
    /// Show the session name under each sprite.
    #[serde(default = "default_true")]
    pub show_name: bool,
    /// Idle sprites take short strolls and come back home.
    #[serde(default = "default_true")]
    pub wander: bool,
    /// Seconds a sprite must be idle before it starts strolling.
    #[serde(default = "default_wander_after")]
    pub wander_after_secs: u32,
    /// Look for a new release on launch and every few hours.
    #[serde(default = "default_true")]
    pub auto_update: bool,
    /// Absolute path to the `claude` binary; overrides PATH lookup (GUI apps get a minimal PATH).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_path: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self { port: 7433, token: generate_token(), pack: "default".into(), bind_all: false, size: SpriteSize::Normal, permission_wait_secs: 60, show_name: true, wander: true, wander_after_secs: 20, auto_update: true, claude_path: None }
    }
}

impl Config {
    pub fn load(path: &Path) -> Config {
        std::fs::read_to_string(path).ok()
            .and_then(|t| serde_json::from_str::<Config>(&t).ok())
            .filter(|c| c.token.len() == 32)
            .unwrap_or_default()
    }
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(p) = path.parent() { std::fs::create_dir_all(p)?; }
        std::fs::write(path, serde_json::to_string_pretty(self).unwrap())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
    pub fn hook_url(&self) -> String { format!("http://127.0.0.1:{}/hook", self.port) }
    pub fn bind_addr(&self) -> SocketAddr {
        let ip = if self.bind_all { Ipv4Addr::UNSPECIFIED } else { Ipv4Addr::LOCALHOST };
        SocketAddr::new(IpAddr::V4(ip), self.port)
    }
}

pub fn generate_token() -> String {
    let mut b = [0u8; 16];
    rand::rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn config_dir() -> PathBuf {
    directories::ProjectDirs::from("dev", "kill4n", "codepets")
        .map(|p| p.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".codepets"))
}
pub fn config_path() -> PathBuf { config_dir().join("config.json") }
pub fn positions_path() -> PathBuf { config_dir().join("positions.json") }

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn token_is_32_hex() {
        let t = generate_token();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(t, generate_token());
    }
    #[test] fn load_missing_creates_defaults_with_token() {
        let d = tempfile::tempdir().unwrap();
        let c = Config::load(&d.path().join("config.json"));
        assert_eq!(c.port, 7433); assert_eq!(c.pack, "default"); assert!(!c.bind_all);
        assert_eq!(c.token.len(), 32);
    }
    #[test] fn save_then_load_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        let mut c = Config::load(&p); c.port = 8000; c.pack = "cat".into();
        c.save(&p).unwrap();
        assert_eq!(Config::load(&p), c);
    }
    #[test] fn load_invalid_json_returns_defaults() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        std::fs::write(&p, "nope").unwrap();
        assert_eq!(Config::load(&p).port, 7433);
    }
    #[test] fn sprite_size_scales_pack_scale_with_a_floor_of_one() {
        assert_eq!(SpriteSize::Small.apply(2), 1);
        assert_eq!(SpriteSize::Normal.apply(2), 2);
        assert_eq!(SpriteSize::Large.apply(2), 3);
        assert_eq!(SpriteSize::Small.apply(1), 1, "never below 1");
        assert_eq!(SpriteSize::parse("large"), Some(SpriteSize::Large));
        assert_eq!(SpriteSize::parse("huge"), None);
    }
    #[test] fn size_defaults_to_normal_for_old_configs() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        std::fs::write(&p, r#"{"port":7433,"token":"0123456789abcdef0123456789abcdef","pack":"default","bind_all":false}"#).unwrap();
        let mut c = Config::load(&p);
        assert_eq!(c.size, SpriteSize::Normal);
        assert!(c.wander, "wander defaults to on for old configs");
        assert!(c.show_name, "show_name defaults to on");
        assert_eq!(c.permission_wait_secs, 60);
        assert_eq!(c.wander_after_secs, 20);
        c.size = SpriteSize::Small; c.wander = false; c.save(&p).unwrap();
        assert_eq!(Config::load(&p).size, SpriteSize::Small);
        assert!(!Config::load(&p).wander);
    }
    #[test] fn claude_path_is_optional_and_roundtrips() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        std::fs::write(&p, r#"{"port":7433,"token":"0123456789abcdef0123456789abcdef","pack":"default","bind_all":false}"#).unwrap();
        let mut c = Config::load(&p);
        assert_eq!(c.token, "0123456789abcdef0123456789abcdef", "old config without claude_path still loads");
        assert_eq!(c.claude_path, None);
        c.claude_path = Some("/opt/claude".into());
        c.save(&p).unwrap();
        assert_eq!(Config::load(&p).claude_path.as_deref(), Some("/opt/claude"));
    }
    #[test] fn auto_update_defaults_to_on_for_old_configs() {
        let c: Config = serde_json::from_str(r#"{"port":7433,"token":"0123456789abcdef0123456789abcdef","pack":"default","bind_all":false}"#).unwrap();
        assert!(c.auto_update);
        let off = Config { auto_update: false, ..c };
        let d = tempfile::tempdir().unwrap();
        off.save(&d.path().join("c.json")).unwrap();
        assert!(!Config::load(&d.path().join("c.json")).auto_update);
    }

    #[test] fn urls_and_bind() {
        let mut c = Config::load(std::path::Path::new("/nonexistent/x.json"));
        assert_eq!(c.hook_url(), "http://127.0.0.1:7433/hook");
        assert_eq!(c.bind_addr().to_string(), "127.0.0.1:7433");
        c.bind_all = true;
        assert_eq!(c.bind_addr().to_string(), "0.0.0.0:7433");
    }
    #[cfg(unix)]
    #[test] fn save_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        Config::load(&p).save(&p).unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
