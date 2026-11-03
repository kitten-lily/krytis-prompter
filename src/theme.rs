//! The dialog's look, taken from noctalia.
//!
//! Colours come from noctalia's `gtk4` template, which writes libadwaita-style
//! `@define-color`s to `~/.config/gtk-4.0/noctalia.css` and imports it from
//! `gtk.css`. GTK loads that file itself at user priority, above the fallback
//! palette defined here, so a palette change applies to the next dialog with no
//! code here. The font and corner radius come from noctalia's `[shell]`
//! settings. docs/skills/ui.md § Theming.

use std::path::{Path, PathBuf};

/// What we read from noctalia's settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub font_family: Option<String>,
    /// noctalia's `shell.corner_radius_scale`, clamped to its 0–2 range.
    pub corner_radius_scale: f64,
}

impl Default for Look {
    fn default() -> Self {
        Self { font_family: None, corner_radius_scale: 1.0 }
    }
}

impl Look {
    /// Reads the user's noctalia settings: every `*.toml` in
    /// `$XDG_CONFIG_HOME/noctalia` in name order, then the settings panel's
    /// `$XDG_STATE_HOME/noctalia/settings.toml`, later files winning, as
    /// noctalia merges them. Missing or unreadable files are skipped.
    pub fn load() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let config = xdg_dir("XDG_CONFIG_HOME", &home, ".config").join("noctalia");
        let state = xdg_dir("XDG_STATE_HOME", &home, ".local/state").join("noctalia/settings.toml");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&config)
            .map(|dir| {
                dir.filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "toml"))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        files.push(state);
        Self::from_files(&files)
    }

    fn from_files(files: &[PathBuf]) -> Self {
        let mut look = Self::default();
        for file in files {
            if let Ok(text) = std::fs::read_to_string(file) {
                look.apply(&text);
            }
        }
        look
    }

    fn apply(&mut self, toml_text: &str) {
        let Ok(table) = toml_text.parse::<toml::Table>() else { return };
        let Some(shell) = table.get("shell").and_then(|v| v.as_table()) else { return };
        if let Some(font) = shell.get("font_family").and_then(|v| v.as_str()) {
            self.font_family = Some(font.to_owned()).filter(|f| !f.trim().is_empty());
        }
        let scale =
            shell.get("corner_radius_scale").and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)));
        if let Some(scale) = scale.filter(|s| s.is_finite()) {
            self.corner_radius_scale = scale.clamp(0.0, 2.0);
        }
    }

    /// The application stylesheet. noctalia's radii are 6 px for controls and
    /// 12 px for cards, times the scale (`Style::radiusMd`/`radiusXl`).
    pub fn css(&self) -> String {
        let card = 12.0 * self.corner_radius_scale;
        let control = 6.0 * self.corner_radius_scale;
        let font =
            self.font_family.as_deref().and_then(css_string).map(|f| format!("font-family: {f};")).unwrap_or_default();
        format!(
            "{FALLBACK_PALETTE}
window.krytis-prompter {{ background: transparent; {font} }}
window.krytis-prompter.toplevel {{ background: @window_bg_color; }}
.krytis-prompter .card {{
  background: @window_bg_color; color: @window_fg_color;
  border-radius: {card}px; border: 1px solid alpha(@window_fg_color, 0.12);
  padding: 20px; margin: 12px;
  box-shadow: 0 6px 24px alpha(black, 0.35);
}}
.krytis-prompter.toplevel .card {{ margin: 0; box-shadow: none; border: none; }}
.krytis-prompter .heading {{ font-size: 16px; font-weight: bold; }}
.krytis-prompter .body {{ font-size: 14px; }}
.krytis-prompter .warning {{ color: @error_bg_color; font-size: 14px; }}
.krytis-prompter entry, .krytis-prompter button {{ border-radius: {control}px; }}
.krytis-prompter entry {{ background: @view_bg_color; color: @view_fg_color; }}
.krytis-prompter button.suggested-action {{ background: @accent_bg_color; color: @accent_fg_color; }}
.krytis-prompter levelbar block.filled {{ background: @accent_bg_color; border-radius: {control}px; }}
"
        )
    }
}

/// Used when noctalia's `gtk4` template is off or the file is missing.
/// libadwaita's dark defaults; noctalia's file, loaded by GTK at user priority,
/// overrides every name it defines.
const FALLBACK_PALETTE: &str = "@define-color window_bg_color #242424;
@define-color window_fg_color #ffffff;
@define-color view_bg_color #1e1e1e;
@define-color view_fg_color #ffffff;
@define-color accent_bg_color #3584e4;
@define-color accent_fg_color #ffffff;
@define-color error_bg_color #c01c28;";

fn xdg_dir(var: &str, home: &Path, fallback: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(fallback))
}

/// A CSS string literal for a font family name, or `None` if the name holds
/// characters that could escape the declaration.
fn css_string(name: &str) -> Option<String> {
    name.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.')).then(|| format!("\"{name}\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn later_files_win_and_scale_is_clamped() {
        let dir = std::env::temp_dir().join(format!("krytis-prompter-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = write(&dir, "config.toml", "[shell]\nfont_family = \"Inter\"\ncorner_radius_scale = 0.5\n");
        let settings = write(&dir, "settings.toml", "[shell]\ncorner_radius_scale = 5\n");
        let look = Look::from_files(&[config, settings, dir.join("missing.toml")]);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(look, Look { font_family: Some("Inter".into()), corner_radius_scale: 2.0 });
    }

    #[test]
    fn font_names_cannot_inject_css() {
        let look = Look { font_family: Some("x; } * { color: red".into()), corner_radius_scale: 1.0 };
        assert!(!look.css().contains("color: red"));
        let look = Look { font_family: Some("Adwaita Sans".into()), corner_radius_scale: 1.0 };
        assert!(look.css().contains("font-family: \"Adwaita Sans\";"));
    }
}
