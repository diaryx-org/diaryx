use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Base directory for diary entries
    pub base_dir: PathBuf,

    /// Preferred editor (falls back to $EDITOR if not set)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,

    /// Default template to use when creating entries
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_template: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        let default_base = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("diaryx");

        Self {
            base_dir: default_base,
            editor: None,
            default_template: None,
        }
    }
}

impl Config {
    /// Get the config file path (~/.config/diaryx/config.toml)
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("diaryx").join("config.toml"))
    }

    /// Load config from file, or return default if file doesn't exist
    pub fn load() -> Result<Self, ConfigError> {
        if let Some(path) = Self::config_path() {
            if path.exists() {
                let contents = fs::read_to_string(&path)
                    .map_err(|e| ConfigError::Io(e))?;
                let config: Config = toml::from_str(&contents)
                    .map_err(|e| ConfigError::Parse(e))?;
                return Ok(config);
            }
        }

        // Return default config if file doesn't exist
        Ok(Config::default())
    }

    /// Save config to file
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::config_path()
            .ok_or(ConfigError::NoConfigDir)?;

        // Create config directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| ConfigError::Io(e))?;
        }

        let contents = toml::to_string_pretty(self)
            .map_err(|e| ConfigError::Serialize(e))?;

        fs::write(&path, contents)
            .map_err(|e| ConfigError::Io(e))?;

        Ok(())
    }

    /// Initialize config with user-provided values
    pub fn init(base_dir: PathBuf) -> Result<Self, ConfigError> {
        let config = Config {
            base_dir,
            editor: None,
            default_template: None,
        };

        config.save()?;
        Ok(config)
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
    NoConfigDir,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "IO error: {}", e),
            ConfigError::Parse(e) => write!(f, "Config parse error: {}", e),
            ConfigError::Serialize(e) => write!(f, "Config serialize error: {}", e),
            ConfigError::NoConfigDir => write!(f, "Could not determine config directory"),
        }
    }
}

impl std::error::Error for ConfigError {}
