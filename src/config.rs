// Configuration file loader.
//
// Reads ~/.config/r-clite/config.toml at startup.
// Missing file or missing keys silently use defaults.
// Invalid values produce a warning message that the editor displays on launch.

use std::fs;

/// Editor configuration loaded from `~/.config/r-clite/config.toml`.
pub struct Config {
    /// Number of spaces inserted per Tab key press.
    pub tab_width: usize,
    /// Whether line numbers are shown on startup.
    pub line_numbers: bool,
    /// Colour theme: `"dark"` or `"light"`.
    pub theme: String,
    /// Whether soft word wrap is enabled on startup.
    pub word_wrap: bool,
    /// Column width used by the ReflowParagraph command.
    pub wrap_column: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tab_width: 4,
            line_numbers: true,
            theme: "dark".to_string(),
            word_wrap: true,
            wrap_column: 80,
        }
    }
}

impl Config {
    /// Load the config file.
    ///
    /// Returns `(Config, Option<warning_message>)`.  A missing file is silently
    /// ignored.  Parse errors produce a warning but the returned config is the
    /// valid subset of what was parsed, falling back to defaults for bad keys.
    pub fn load() -> (Self, Option<String>) {
        let mut cfg = Self::default();

        let Some(home) = dirs_home() else {
            return (cfg, None);
        };
        let path = home.join(".config").join("r-clite").join("config.toml");
        let content = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return (cfg, None), // Missing file is fine.
        };

        let mut warnings: Vec<String> = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            // Skip comments and blank lines.
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, val)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let val = val.trim().trim_matches('"');

            match key {
                "tab_width" => match val.parse::<usize>() {
                    Ok(n) if n > 0 => cfg.tab_width = n,
                    _ => warnings.push(format!("config: invalid tab_width '{}'", val)),
                },
                "line_numbers" => match val {
                    "true" => cfg.line_numbers = true,
                    "false" => cfg.line_numbers = false,
                    _ => warnings.push(format!("config: invalid line_numbers '{}'", val)),
                },
                "theme" => match val {
                    "dark" | "light" => cfg.theme = val.to_string(),
                    _ => warnings.push(format!("config: unknown theme '{}', using 'dark'", val)),
                },
                "word_wrap" => match val {
                    "true" => cfg.word_wrap = true,
                    "false" => cfg.word_wrap = false,
                    _ => warnings.push(format!("config: invalid word_wrap '{}'", val)),
                },
                "wrap_column" => match val.parse::<usize>() {
                    Ok(n) if n > 0 => cfg.wrap_column = n,
                    _ => warnings.push(format!("config: invalid wrap_column '{}'", val)),
                },
                _ => {} // Ignore unknown keys silently.
            }
        }

        let warning = if warnings.is_empty() {
            None
        } else {
            Some(warnings.join("; "))
        };

        (cfg, warning)
    }
}

/// Returns the user's home directory, or `None` if it cannot be determined.
fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}
