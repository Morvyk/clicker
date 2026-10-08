//! Read desktop appearance settings without linking Qt or GTK.
//! Qt/KDE palettes take priority, followed by GTK settings/CSS, then Breeze.
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use iced::{Color, Font, font};

/// Breeze's frame radius in logical pixels.
pub const RADIUS: f32 = 5.0;

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

/// The KDE color roles this app draws with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    pub dark: bool,
    /// `Colors:Window`: window and toolbar background.
    pub window: Color,
    pub text: Color,
    /// `Colors:View`: the darker background of lists, inputs and grooves.
    pub base: Color,
    /// `Colors:View` `BackgroundAlternate`: zebra rows and cards.
    pub alt_base: Color,
    pub button: Color,
    pub button_text: Color,
    pub selection: Color,
    pub selection_text: Color,
    /// `DecorationFocus`: focus rings, slider fill, pressed controls.
    pub focus: Color,
    /// `DecorationHover`: outline of hovered controls.
    pub hover: Color,
    pub link: Color,
    pub muted: Color,
    pub border: Color,
    pub error: Color,
    pub positive: Color,
    pub neutral: Color,
}

impl Colors {
    /// The stock Breeze Dark / Breeze Light schemes.
    pub fn breeze(dark: bool) -> Self {
        let negative = rgb(218, 68, 83);
        let neutral = rgb(246, 116, 0);
        let positive = rgb(39, 174, 96);
        let accent = rgb(61, 174, 233);
        if dark {
            let (window, text) = (rgb(32, 35, 38), rgb(252, 252, 252));
            Self {
                dark,
                window,
                text,
                base: rgb(20, 22, 24),
                alt_base: rgb(27, 30, 32),
                button: rgb(49, 54, 59),
                button_text: text,
                selection: accent,
                selection_text: rgb(252, 252, 252),
                focus: accent,
                hover: accent,
                link: accent,
                muted: rgb(161, 169, 177),
                border: mix(window, text, 0.22),
                error: negative,
                positive,
                neutral,
            }
        } else {
            let (window, text) = (rgb(239, 240, 241), rgb(35, 38, 39));
            Self {
                dark,
                window,
                text,
                base: rgb(252, 252, 252),
                alt_base: rgb(247, 247, 247),
                button: rgb(252, 252, 252),
                button_text: text,
                selection: accent,
                selection_text: rgb(252, 252, 252),
                focus: accent,
                hover: accent,
                link: rgb(41, 128, 185),
                muted: rgb(112, 125, 138),
                border: mix(window, text, 0.22),
                error: negative,
                positive,
                neutral,
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct FontSpec {
    pub family: String,
    pub size: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Appearance {
    pub source: String,
    /// `None` when the desktop provides no palette; Breeze is used instead.
    pub palette: Option<Colors>,
    pub dark: Option<bool>,
    pub font: Option<FontSpec>,
    pub fixed_font: Option<FontSpec>,
    pub icon_theme: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            source: "Built-in".into(),
            palette: None,
            dark: None,
            font: None,
            fixed_font: None,
            icon_theme: "Adwaita".into(),
        }
    }
}

#[derive(Clone)]
struct Paths {
    config: Vec<PathBuf>,
    data: Vec<PathBuf>,
    home: PathBuf,
    qt_platform: String,
    gtk_theme: Option<String>,
}

impl Paths {
    fn system() -> Self {
        let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let mut config = vec![
            env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config")),
        ];
        config.extend(env::split_paths(
            &env::var_os("XDG_CONFIG_DIRS").unwrap_or_else(|| "/etc/xdg".into()),
        ));
        let mut data = vec![
            env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/share")),
        ];
        data.extend(env::split_paths(
            &env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into()),
        ));
        Self {
            config,
            data,
            home,
            qt_platform: env::var("QT_QPA_PLATFORMTHEME").unwrap_or_default(),
            gtk_theme: env::var("GTK_THEME").ok(),
        }
    }

    fn settings(&self, relative: &str) -> Ini {
        let mut settings = Ini::default();
        // System defaults first; user settings override individual keys.
        for root in self.config.iter().rev() {
            settings.extend(Ini::read(&root.join(relative)));
        }
        settings
    }

    fn data_file(&self, relative: &str) -> Option<PathBuf> {
        self.data
            .iter()
            .map(|root| root.join(relative))
            .find(|path| path.is_file())
    }
}

#[derive(Default)]
struct Ini(BTreeMap<(String, String), String>);

impl Ini {
    fn parse(text: &str) -> Self {
        let mut ini = Self::default();
        let mut group = "";
        for line in text.lines().map(str::trim) {
            if line.starts_with(['#', ';']) {
                continue;
            }
            if let Some(section) = line
                .strip_prefix('[')
                .and_then(|line| line.strip_suffix(']'))
            {
                group = section;
            } else if let Some((key, value)) = line.split_once('=') {
                ini.0.insert(
                    (group.into(), key.trim().trim_end_matches("[$i]").into()),
                    value.trim().trim_matches('"').into(),
                );
            }
        }
        ini
    }

    fn read(path: &Path) -> Self {
        fs::read_to_string(path)
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }
    fn extend(&mut self, other: Self) {
        self.0.extend(other.0);
    }
    fn get(&self, group: &str, key: &str) -> Option<&str> {
        self.0
            .get(&(group.into(), key.into()))
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }
    fn color(&self, group: &str, key: &str) -> Option<Color> {
        color(self.get(group, key)?)
    }
}

fn color(value: &str) -> Option<Color> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        if !hex.is_ascii() {
            return None;
        }
        return match hex.len() {
            3 => Some(rgb(
                u8::from_str_radix(&hex[0..1], 16).ok()? * 17,
                u8::from_str_radix(&hex[1..2], 16).ok()? * 17,
                u8::from_str_radix(&hex[2..3], 16).ok()? * 17,
            )),
            6 | 8 if hex.is_ascii() => {
                // Qt writes #AARRGGBB; exported GTK palette colors are #RRGGBB.
                let digits = &hex[hex.len() - 6..];
                Some(rgb(
                    u8::from_str_radix(&digits[..2], 16).ok()?,
                    u8::from_str_radix(&digits[2..4], 16).ok()?,
                    u8::from_str_radix(&digits[4..], 16).ok()?,
                ))
            }
            _ => None,
        };
    }
    let body = value
        .strip_prefix("rgba(")
        .or_else(|| value.strip_prefix("rgb("))
        .map(|v| v.trim_end_matches(')'))
        .unwrap_or(value);
    let channels = body
        .split(',')
        .take(3)
        .map(|v| v.trim().parse::<u8>())
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (channels.len() == 3).then(|| rgb(channels[0], channels[1], channels[2]))
}

fn dark(color: Color) -> bool {
    color.r * 0.299 + color.g * 0.587 + color.b * 0.114 < 0.5
}

pub fn mix(a: Color, b: Color, amount: f32) -> Color {
    Color::from_rgb(
        a.r + (b.r - a.r) * amount,
        a.g + (b.g - a.g) * amount,
        a.b + (b.b - a.b) * amount,
    )
}

fn kde_palette(ini: &Ini) -> Option<Colors> {
    let window = ini.color("Colors:Window", "BackgroundNormal")?;
    let text = ini.color("Colors:Window", "ForegroundNormal")?;
    let is_dark = dark(window);
    let defaults = Colors::breeze(is_dark);
    let base = ini
        .color("Colors:View", "BackgroundNormal")
        .unwrap_or(window);
    let selection = ini
        .color("Colors:Selection", "BackgroundNormal")
        .unwrap_or(defaults.selection);
    let foreground = |role: &str, fallback: Color| {
        ini.color("Colors:Window", role)
            .or_else(|| ini.color("Colors:View", role))
            .unwrap_or(fallback)
    };
    let decoration = |role: &str| {
        ini.color("Colors:Button", role)
            .or_else(|| ini.color("Colors:View", role))
            .unwrap_or(selection)
    };
    Some(Colors {
        dark: is_dark,
        window,
        text,
        base,
        alt_base: ini
            .color("Colors:View", "BackgroundAlternate")
            .unwrap_or(mix(base, text, 0.04)),
        button: ini
            .color("Colors:Button", "BackgroundNormal")
            .unwrap_or(window),
        button_text: ini
            .color("Colors:Button", "ForegroundNormal")
            .unwrap_or(text),
        selection,
        selection_text: ini
            .color("Colors:Selection", "ForegroundNormal")
            .unwrap_or(defaults.selection_text),
        focus: decoration("DecorationFocus"),
        hover: decoration("DecorationHover"),
        link: foreground("ForegroundLink", defaults.link),
        muted: ini
            .color("Colors:Window", "ForegroundInactive")
            .unwrap_or(mix(window, text, 0.6)),
        border: mix(window, text, 0.22),
        error: foreground("ForegroundNegative", defaults.error),
        positive: foreground("ForegroundPositive", defaults.positive),
        neutral: foreground("ForegroundNeutral", defaults.neutral),
    })
}

fn qt_palette(ini: &Ini) -> Option<Colors> {
    if let Some(palette) = kde_palette(ini) {
        return Some(palette);
    }
    let colors: Vec<_> = ini
        .get("ColorScheme", "active")?
        .split(',')
        .map(color)
        .collect();
    let get = |index| colors.get(index).copied().flatten();
    let is_dark = dark(get(10)?);
    let defaults = Colors::breeze(is_dark);
    Some(Colors {
        dark: is_dark,
        window: get(10)?,
        text: get(0)?,
        base: get(9)?,
        alt_base: mix(get(9)?, get(0)?, 0.04),
        button: get(1)?,
        button_text: get(8)?,
        selection: get(12)?,
        selection_text: get(13)?,
        focus: get(12)?,
        hover: get(12)?,
        link: get(14).unwrap_or(get(12)?),
        muted: mix(get(10)?, get(0)?, 0.6),
        border: get(5)?,
        error: defaults.error,
        positive: defaults.positive,
        neutral: defaults.neutral,
    })
}

fn qt_font(value: &str) -> Option<FontSpec> {
    let mut fields = value.trim_matches('"').split(',');
    let family = fields.next()?.trim().to_owned();
    let size = fields.next()?.trim().parse::<f32>().ok()?;
    (!family.is_empty() && size.is_finite() && size > 0.0).then(|| FontSpec {
        family,
        size: size.clamp(6.0, 32.0) * 4.0 / 3.0,
    })
}

fn gtk_font(value: &str) -> Option<FontSpec> {
    let (family, size) = value.trim().rsplit_once(' ')?;
    let size = size.parse::<f32>().ok()?;
    (size.is_finite() && size > 0.0).then(|| FontSpec {
        family: family.trim_end_matches([',', ' ']).into(),
        size: size.clamp(6.0, 32.0) * 4.0 / 3.0,
    })
}

fn qt(paths: &Paths) -> Option<Appearance> {
    let kde = paths.settings("kdeglobals");
    let qt6 = paths.settings("qt6ct/qt6ct.conf");
    let qt5 = paths.settings("qt5ct/qt5ct.conf");
    let kde_theme = || {
        let mut palette = kde_palette(&kde);
        if palette.is_none()
            && let Some(name) = kde.get("General", "ColorScheme")
            && let Some(path) = paths.data_file(&format!("color-schemes/{name}.colors"))
        {
            palette = kde_palette(&Ini::read(&path));
        }
        let palette = palette?;
        Some(Appearance {
            source: "Qt / KDE".into(),
            dark: Some(dark(palette.window)),
            palette: Some(palette),
            font: kde.get("General", "font").and_then(qt_font),
            fixed_font: kde.get("General", "fixed").and_then(qt_font),
            icon_theme: kde.get("Icons", "Theme").unwrap_or("").into(),
        })
    };
    let ct_theme = |ini: &Ini, name: &str| {
        if ini.get("Appearance", "custom_palette") != Some("true") {
            return None;
        }
        let path = ini.get("Appearance", "color_scheme_path")?;
        let path = if let Some(rest) = path.strip_prefix("~/") {
            paths.home.join(rest)
        } else {
            PathBuf::from(path)
        };
        let palette = qt_palette(&Ini::read(&path))?;
        Some(Appearance {
            source: format!("Qt / {name}"),
            dark: Some(dark(palette.window)),
            palette: Some(palette),
            font: ini.get("Fonts", "general").and_then(qt_font),
            fixed_font: ini.get("Fonts", "fixed").and_then(qt_font),
            icon_theme: ini.get("Appearance", "icon_theme").unwrap_or("").into(),
        })
    };
    let mut result = if paths.qt_platform.contains("qt6ct") {
        ct_theme(&qt6, "qt6ct")
            .or_else(kde_theme)
            .or_else(|| ct_theme(&qt5, "qt5ct"))
    } else if paths.qt_platform.contains("qt5ct") {
        ct_theme(&qt5, "qt5ct")
            .or_else(kde_theme)
            .or_else(|| ct_theme(&qt6, "qt6ct"))
    } else {
        kde_theme()
            .or_else(|| ct_theme(&qt6, "qt6ct"))
            .or_else(|| ct_theme(&qt5, "qt5ct"))
    }?;
    // Individual missing settings can still come from the other Qt configuration.
    result.font = result
        .font
        .or_else(|| kde.get("General", "font").and_then(qt_font))
        .or_else(|| qt6.get("Fonts", "general").and_then(qt_font));
    if result.icon_theme.is_empty() {
        result.icon_theme = kde
            .get("Icons", "Theme")
            .or_else(|| qt6.get("Appearance", "icon_theme"))
            .unwrap_or("")
            .into();
    }
    Some(result)
}

fn read_css(path: &Path, definitions: &mut BTreeMap<String, String>, depth: usize) {
    if depth > 5 {
        return;
    }
    let Ok(css) = fs::read_to_string(path) else {
        return;
    };
    let mut clean = String::new();
    let mut rest = css.as_str();
    while let Some((before, comment)) = rest.split_once("/*") {
        clean.push_str(before);
        rest = comment.split_once("*/").map_or("", |(_, after)| after);
    }
    clean.push_str(rest);
    // Process imports and declarations in source order, including KDE's colors.css.
    for statement in clean.split(';') {
        if let Some((_, value)) = statement.rsplit_once("@import") {
            let import = value
                .trim()
                .trim_start_matches("url(")
                .trim_end_matches(')')
                .trim()
                .trim_matches(['\'', '"']);
            if !import.contains("://") {
                read_css(
                    &path.parent().unwrap_or(Path::new(".")).join(import),
                    definitions,
                    depth + 1,
                );
            }
        } else if let Some((_, value)) = statement.rsplit_once("@define-color") {
            if let Some((name, value)) = value.trim().split_once(char::is_whitespace) {
                definitions.insert(name.into(), value.trim().into());
            }
        } else if let Some(value) = statement
            .rsplit(['{', '}'])
            .next()
            .map(str::trim)
            .and_then(|value| value.strip_prefix("--"))
            && let Some((name, value)) = value.split_once(':')
        {
            definitions.insert(name.trim().replace('-', "_"), value.trim().into());
        }
    }
}

fn css_color(definitions: &BTreeMap<String, String>, name: &str, depth: usize) -> Option<Color> {
    if depth > 8 {
        return None;
    }
    let value = definitions.get(name)?;
    if value.is_ascii() && value.starts_with('#') {
        // CSS uses RGBA, whereas Qt palette files use ARGB.
        if value.len() == 9 {
            return color(&value[..7]);
        }
        if value.len() == 5 {
            return color(&value[..4]);
        }
    }
    if let Some(name) = value.strip_prefix('@') {
        return css_color(definitions, name, depth + 1);
    }
    if let Some(name) = value
        .strip_prefix("var(--")
        .and_then(|value| value.strip_suffix(')'))
    {
        return css_color(definitions, &name.replace('-', "_"), depth + 1);
    }
    color(value)
}

fn gtk(paths: &Paths) -> Option<Appearance> {
    let mut settings = paths.settings("gtk-3.0/settings.ini");
    settings.extend(paths.settings("gtk-4.0/settings.ini"));
    let name = paths
        .gtk_theme
        .as_deref()
        .or_else(|| settings.get("Settings", "gtk-theme-name"));
    let prefers_dark = name.is_some_and(|name| name.to_lowercase().contains("dark"))
        || settings.get("Settings", "gtk-application-prefer-dark-theme") == Some("true")
        || settings.get("Settings", "gtk-application-prefer-dark-theme") == Some("1");
    let mut definitions = BTreeMap::new();
    if let Some(name) = name {
        let name = name.split(':').next().unwrap_or(name);
        let mut roots = paths
            .data
            .iter()
            .map(|root| root.join("themes"))
            .collect::<Vec<_>>();
        roots.insert(0, paths.home.join(".themes"));
        if let Some(root) = roots
            .iter()
            .map(|root| root.join(name))
            .find(|root| root.is_dir())
        {
            for version in ["gtk-3.0", "gtk-4.0"] {
                let dark_css = root.join(version).join("gtk-dark.css");
                read_css(
                    &if prefers_dark && dark_css.is_file() {
                        dark_css
                    } else {
                        root.join(version).join("gtk.css")
                    },
                    &mut definitions,
                    0,
                );
            }
        }
    }
    for root in paths.config.iter().rev() {
        for version in ["gtk-3.0", "gtk-4.0"] {
            read_css(&root.join(version).join("gtk.css"), &mut definitions, 0);
        }
    }
    if settings.0.is_empty() && name.is_none() && definitions.is_empty() {
        return None;
    }
    let find = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| css_color(&definitions, name, 0))
    };
    let window = find(&["window_bg_color", "theme_bg_color", "theme_bg_color_breeze"]);
    let is_dark = window.map_or(prefers_dark, dark);
    let mut palette = Colors::breeze(is_dark);
    if let Some(value) = window {
        palette.window = value;
    }
    if let Some(value) = find(&["window_fg_color", "theme_fg_color", "theme_fg_color_breeze"]) {
        palette.text = value;
    }
    if let Some(value) = find(&[
        "view_bg_color",
        "theme_base_color",
        "theme_base_color_breeze",
    ]) {
        palette.base = value;
    }
    palette.alt_base = mix(palette.base, palette.text, 0.04);
    if let Some(value) = find(&[
        "theme_button_background_normal_breeze",
        "headerbar_bg_color",
    ]) {
        palette.button = value;
    }
    palette.button_text = find(&["theme_button_foreground_normal_breeze"]).unwrap_or(palette.text);
    if let Some(value) = find(&[
        "accent_bg_color",
        "theme_selected_bg_color",
        "theme_selected_bg_color_breeze",
    ]) {
        palette.selection = value;
    }
    if let Some(value) = find(&[
        "accent_fg_color",
        "theme_selected_fg_color",
        "theme_selected_fg_color_breeze",
    ]) {
        palette.selection_text = value;
    }
    palette.focus = palette.selection;
    palette.hover = palette.selection;
    if let Some(value) = find(&["link_color", "theme_link_color", "link_color_breeze"]) {
        palette.link = value;
    }
    palette.border =
        find(&["borders", "borders_breeze"]).unwrap_or(mix(palette.window, palette.text, 0.22));
    palette.muted = mix(palette.window, palette.text, 0.6);
    if let Some(value) = find(&["error_color", "error_color_breeze"]) {
        palette.error = value;
    }
    if let Some(value) = find(&["success_color", "success_color_breeze"]) {
        palette.positive = value;
    }
    if let Some(value) = find(&["warning_color", "warning_color_breeze"]) {
        palette.neutral = value;
    }
    Some(Appearance {
        source: format!("GTK / {}", name.unwrap_or("system")),
        palette: Some(palette),
        dark: Some(is_dark),
        font: settings.get("Settings", "gtk-font-name").and_then(gtk_font),
        fixed_font: None,
        icon_theme: settings
            .get("Settings", "gtk-icon-theme-name")
            .unwrap_or("Adwaita")
            .into(),
    })
}

fn detect(paths: &Paths) -> Appearance {
    let gtk = gtk(paths);
    if let Some(mut qt) = qt(paths) {
        if let Some(gtk) = gtk {
            qt.font = qt.font.or(gtk.font);
            if qt.icon_theme.is_empty() {
                qt.icon_theme = gtk.icon_theme;
            }
        }
        if qt.icon_theme.is_empty() {
            qt.icon_theme = "hicolor".into();
        }
        qt
    } else {
        gtk.unwrap_or_default()
    }
}

/// Everything the views need to draw: colors, fonts and the base text size.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub c: Colors,
    pub font: Font,
    pub mono: Font,
    pub size: f32,
}

impl Look {
    pub fn new(appearance: &Appearance, system_dark: bool) -> Self {
        let c = appearance
            .palette
            .unwrap_or_else(|| Colors::breeze(appearance.dark.unwrap_or(system_dark)));
        Self {
            c,
            font: appearance.font.as_ref().map_or(Font::DEFAULT, font_for),
            mono: appearance
                .fixed_font
                .as_ref()
                .map_or(Font::MONOSPACE, font_for),
            size: appearance.font.as_ref().map_or(13.0, |font| font.size),
        }
    }
}

/// `Font::with_name` needs a `'static` name. Fonts change rarely, so intern them.
fn font_for(spec: &FontSpec) -> Font {
    static NAMES: OnceLock<Mutex<Vec<&'static str>>> = OnceLock::new();
    let mut names = NAMES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let name = match names.iter().find(|name| **name == spec.family) {
        Some(name) => name,
        None => {
            let name: &'static str = Box::leak(spec.family.clone().into_boxed_str());
            names.push(name);
            name
        }
    };
    Font {
        family: font::Family::Name(name),
        ..Font::DEFAULT
    }
}

/// Reads the desktop configuration. Does file IO, so call it off the UI thread.
pub fn system_appearance() -> Appearance {
    detect(&Paths::system())
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = env::temp_dir().join(format!(
                "clicker-theme-{}-{:?}",
                std::process::id(),
                thread::current().id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn write(&self, name: &str, text: &str) {
            let path = self.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        fn paths(&self) -> Paths {
            Paths {
                config: vec![self.0.clone()],
                data: vec![self.0.join("data")],
                home: self.0.clone(),
                qt_platform: String::new(),
                gtk_theme: None,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn qt_then_gtk_then_builtin_and_invalid_qt_falls_back() {
        let f = Fixture::new();
        assert_eq!(detect(&f.paths()).source, "Built-in");
        f.write("gtk-3.0/settings.ini", "[Settings]\ngtk-theme-name=Test\ngtk-icon-theme-name=test-icons\ngtk-font-name=Sans 11");
        f.write("gtk-3.0/gtk.css", "@import 'colors.css';");
        f.write("gtk-3.0/colors.css", "@define-color bg #eee; @define-color theme_bg_color @bg; @define-color theme_fg_color #111;");
        let gtk = detect(&f.paths());
        assert!(gtk.source.starts_with("GTK"));
        assert_eq!(
            gtk.palette.unwrap().window,
            rgb(238, 238, 238)
        );
        f.write(
            "kdeglobals",
            "[Colors:Window]\nBackgroundNormal=32,35,38\nForegroundNormal=252,252,252",
        );
        assert_eq!(detect(&f.paths()).source, "Qt / KDE");
        f.write("kdeglobals", "[Colors:Window]\nBackgroundNormal=broken");
        assert!(detect(&f.paths()).source.starts_with("GTK"));
    }

    #[test]
    fn active_qt6ct_palette_overrides_kde_and_maps_qt_roles() {
        let f = Fixture::new();
        f.write(
            "kdeglobals",
            "[Colors:Window]\nBackgroundNormal=32,35,38\nForegroundNormal=252,252,252",
        );
        let colors = "#111111,#eeeeee,#ffffff,#dddddd,#333333,#888888,#111111,#ffffff,#222222,#ffffff,#eeeeee,#000000,#123456,#ffffff";
        f.write("palette.conf", &format!("[ColorScheme]\nactive={colors}"));
        f.write("qt6ct/qt6ct.conf", &format!("[Appearance]\ncustom_palette=true\ncolor_scheme_path={}\nicon_theme=test\n[Fonts]\ngeneral=Sans,10,-1,5,50", f.0.join("palette.conf").display()));
        let mut paths = f.paths();
        paths.qt_platform = "qt6ct".into();
        let theme = detect(&paths);
        assert_eq!(theme.source, "Qt / qt6ct");
        assert_eq!(
            theme.palette.unwrap().selection,
            rgb(18, 52, 86)
        );
        assert_eq!(theme.dark, Some(false));
        assert_eq!(theme.icon_theme, "test");
    }

    #[test]
    fn css_reference_cycles_and_malformed_colors_are_ignored() {
        let definitions = BTreeMap::from([("a".into(), "@b".into()), ("b".into(), "@a".into())]);
        assert_eq!(css_color(&definitions, "a", 0), None);
        assert_eq!(color("#nonsense"), None);
        assert_eq!(color("#€"), None);
        assert_eq!(color("#ff123456"), Some(rgb(18, 52, 86)));
        assert_eq!(gtk_font("Noto Sans,  10").unwrap().family, "Noto Sans");
    }

    #[test]
    fn gtk_css_variables_resolve_and_builtin_is_breeze() {
        let f = Fixture::new();
        f.write("gtk-4.0/gtk.css", ":root { --base-color: #123456; --window-bg-color: var(--base-color); --window-fg-color: #ffffff; }");
        let appearance = detect(&f.paths());
        assert_eq!(appearance.palette.as_ref().unwrap().window, rgb(18, 52, 86));
        let builtin = Appearance::default();
        assert_eq!(Look::new(&builtin, true).c, Colors::breeze(true));
        assert_eq!(Look::new(&builtin, false).c, Colors::breeze(false));
    }

    #[test]
    fn kde_focus_hover_and_link_colors_are_read() {
        let f = Fixture::new();
        f.write(
            "kdeglobals",
            "[Colors:Window]\nBackgroundNormal=32,35,38\nForegroundNormal=252,252,252\nForegroundLink=41,128,185\n[Colors:Button]\nDecorationFocus=61,174,233\nDecorationHover=1,2,3",
        );
        let p = detect(&f.paths()).palette.unwrap();
        assert_eq!(p.focus, rgb(61, 174, 233));
        assert_eq!(p.hover, rgb(1, 2, 3));
        assert_eq!(p.link, rgb(41, 128, 185));
    }
}
