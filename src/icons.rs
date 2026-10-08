use std::{collections::BTreeMap, fmt, fs, path::Path};

use iced::{
    Color, Element, Length,
    widget::{
        self,
        image::{FilterMethod, Handle},
    },
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Icon {
    Start,
    Stop,
    Record,
    Clear,
    Add,
    Edit,
    Delete,
    Up,
    Down,
    Reset,
    Mouse,
    Apply,
    Cancel,
    Keyboard,
    Sequence,
    Settings,
    Test,
    Error,
    Warning,
    Info,
    Close,
}

const ALL: [Icon; 21] = [
    Icon::Start,
    Icon::Stop,
    Icon::Record,
    Icon::Clear,
    Icon::Add,
    Icon::Edit,
    Icon::Delete,
    Icon::Up,
    Icon::Down,
    Icon::Reset,
    Icon::Mouse,
    Icon::Apply,
    Icon::Cancel,
    Icon::Keyboard,
    Icon::Sequence,
    Icon::Settings,
    Icon::Test,
    Icon::Error,
    Icon::Warning,
    Icon::Info,
    Icon::Close,
];

impl Icon {
    /// Freedesktop names in order of preference.
    fn names(self) -> &'static [&'static str] {
        match self {
            Self::Start => &["media-playback-start"],
            Self::Stop => &["media-playback-stop"],
            Self::Record => &["media-record"],
            Self::Clear => &["edit-clear-all", "edit-clear"],
            Self::Add => &["list-add"],
            Self::Edit => &["document-edit", "edit-entry"],
            Self::Delete => &["edit-delete", "user-trash"],
            Self::Up => &["go-up"],
            Self::Down => &["go-down"],
            Self::Reset => &["view-refresh"],
            Self::Mouse => &["input-mouse"],
            Self::Apply => &["dialog-ok-apply", "dialog-ok"],
            Self::Cancel => &["dialog-cancel", "dialog-close"],
            Self::Keyboard => &["input-keyboard"],
            Self::Sequence => &["view-list-details", "view-list-text", "format-list-unordered"],
            Self::Settings => &["configure", "preferences-system"],
            Self::Test => &["speedometer", "games-config-board", "input-mouse"],
            Self::Error => &["dialog-error"],
            Self::Warning => &["dialog-warning"],
            Self::Info => &["dialog-information"],
            Self::Close => &["window-close", "dialog-close"],
        }
    }
}

/// A set of icons rendered in one foreground color.
#[derive(Default, Clone)]
struct Tint(BTreeMap<Icon, Handle>);

/// Icons rendered once in the text color and once in the selection-text color,
/// so the same icon stays legible on a highlighted sidebar row.
#[derive(Default, Clone)]
pub struct Icons {
    normal: Tint,
    on_accent: Tint,
}

impl fmt::Debug for Icons {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Icons({} loaded)", self.normal.0.len())
    }
}

impl Icons {
    pub fn load(theme: &str, text: Color, on_accent: Color) -> Self {
        let mut normal = BTreeMap::new();
        let mut accent = BTreeMap::new();
        for icon in ALL {
            let Some(path) = find(theme, icon) else {
                continue;
            };
            if let Some(handle) = decode(&path, text) {
                normal.insert(icon, handle);
            }
            if let Some(handle) = decode(&path, on_accent) {
                accent.insert(icon, handle);
            }
        }
        Self {
            normal: Tint(normal),
            on_accent: Tint(accent),
        }
    }

    /// The icon at `size` logical pixels, or nothing when the theme lacks it.
    pub fn view<'a, M: 'a>(&self, icon: Icon, size: f32) -> Option<Element<'a, M>> {
        self.draw(&self.normal, icon, size, 1.0)
    }

    pub fn view_dim<'a, M: 'a>(&self, icon: Icon, size: f32, opacity: f32) -> Option<Element<'a, M>> {
        self.draw(&self.normal, icon, size, opacity)
    }

    pub fn view_on_accent<'a, M: 'a>(&self, icon: Icon, size: f32) -> Option<Element<'a, M>> {
        self.draw(&self.on_accent, icon, size, 1.0)
    }

    fn draw<'a, M: 'a>(
        &self,
        tint: &Tint,
        icon: Icon,
        size: f32,
        opacity: f32,
    ) -> Option<Element<'a, M>> {
        let handle = tint.0.get(&icon)?.clone();
        Some(
            widget::image(handle)
                .width(Length::Fixed(size))
                .height(Length::Fixed(size))
                .filter_method(FilterMethod::Linear)
                .opacity(opacity)
                .into(),
        )
    }
}

fn find(theme: &str, icon: Icon) -> Option<std::path::PathBuf> {
    for name in icon.names() {
        let symbolic = format!("{name}-symbolic");
        // Lookup follows index.theme inheritance and the hicolor fallback.
        for theme in [theme, "breeze", "Adwaita", "hicolor"] {
            for name in [name.to_string(), symbolic.clone()] {
                if let Some(path) = freedesktop_icons::lookup(&name)
                    .with_theme(theme)
                    .with_size(22)
                    .force_svg()
                    .find()
                {
                    return Some(path);
                }
            }
        }
    }
    None
}

const RENDER_SIZE: u32 = 96;

fn decode(path: &Path, foreground: Color) -> Option<Handle> {
    let [r, g, b, _] = foreground.into_rgba8();
    let bytes = fs::read(path).ok()?;
    let symbolic = path.file_name()?.to_string_lossy().contains("-symbolic");
    if path.extension()?.eq_ignore_ascii_case("svg") {
        let css = format!(
            "svg, .ColorScheme-Text {{ color: #{r:02x}{g:02x}{b:02x} !important; }}"
        );
        let options = resvg::usvg::Options {
            resources_dir: path.parent().map(Path::to_path_buf),
            style_sheet: Some(css),
            ..Default::default()
        };
        let tree = resvg::usvg::Tree::from_data(&bytes, &options).ok()?;
        let mut pixels = resvg::tiny_skia::Pixmap::new(RENDER_SIZE, RENDER_SIZE)?;
        let scale = RENDER_SIZE as f32 / tree.size().width().max(tree.size().height());
        let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
        resvg::render(&tree, transform, &mut pixels.as_mut());
        let mut rgba = Vec::with_capacity(pixels.data().len());
        for pixel in pixels.pixels() {
            if symbolic {
                rgba.extend([r, g, b, pixel.alpha()]);
            } else {
                let c = pixel.demultiply();
                rgba.extend([c.red(), c.green(), c.blue(), c.alpha()]);
            }
        }
        Some(Handle::from_rgba(RENDER_SIZE, RENDER_SIZE, rgba))
    } else if path.extension()?.eq_ignore_ascii_case("png") {
        let mut pixels = ::image::load_from_memory_with_format(&bytes, ::image::ImageFormat::Png)
            .ok()?
            .into_rgba8();
        if symbolic {
            for pixel in pixels.pixels_mut() {
                pixel.0[..3].copy_from_slice(&[r, g, b]);
            }
        }
        Some(Handle::from_rgba(
            pixels.width(),
            pixels.height(),
            pixels.into_raw(),
        ))
    } else {
        None
    }
}
