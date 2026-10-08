//! Windows' light/dark mode and accent colour, as the user set them in Settings, turned into
//! the Windows 11 (Fluent) colours the stylesheet uses.

type Rgb = (u8, u8, u8);

pub struct Palette {
    pub dark: bool,
    light_accent: Rgb, // the accent's lighter shade, used in dark mode
    dark_accent: Rgb,  // and its darker one, used in light mode
}

impl Palette {
    /// The accent as buttons and selections show it in this mode.
    pub fn accent(&self) -> Rgb {
        if self.dark { self.light_accent } else { self.dark_accent }
    }

    /// `@define-color`s for the stylesheet.
    pub fn colors(&self) -> String {
        let hex = |(r, g, b): Rgb| format!("#{r:02x}{g:02x}{b:02x}");
        let accent = hex(self.accent());
        // Fluent's colour tokens over the Mica window background, light and dark
        let named: [(&str, &str, &str); 19] = [
            ("window", "#f3f3f3", "#202020"),
            ("layer", "#f9f9f9", "#282828"),       // the content area, a layer above the window
            ("layer_border", "#e5e5e5", "#1c1c1c"),
            ("text", "#1b1b1b", "#ffffff"),
            ("text_secondary", "#5d5d5d", "#cfcfcf"),
            ("text_disabled", "#a0a0a0", "#787878"),
            ("control", "#fdfdfd", "#2d2d2d"),
            ("control_hover", "#f8f8f8", "#323232"),
            ("control_pressed", "#f5f5f5", "#272727"),
            ("control_border", "#e5e5e5", "#353535"),
            ("control_border_bottom", "#cccccc", "#2b2b2b"),
            ("strong_border", "#8a8a8a", "#9a9a9a"),
            ("input", "#fdfdfd", "#2d2d2d"),
            ("input_focus", "#ffffff", "#1f1f1f"),
            ("subtle_hover", "rgba(0,0,0,0.037)", "rgba(255,255,255,0.06)"),
            ("subtle_pressed", "rgba(0,0,0,0.024)", "rgba(255,255,255,0.04)"),
            ("flyout", "#f9f9f9", "#2c2c2c"),
            ("flyout_border", "rgba(0,0,0,0.1)", "rgba(0,0,0,0.3)"),
            ("divider", "#e5e5e5", "#353535"),
        ];
        let mut css = String::new();
        for (name, light, dark) in named {
            css += &format!("@define-color fl_{name} {};\n", if self.dark { dark } else { light });
        }
        css += &format!("@define-color fl_accent {accent};\n");
        css += &format!("@define-color fl_on_accent {};\n", if self.dark { "#000000" } else { "#ffffff" });
        // GTK's own names, for anything the stylesheet doesn't restyle
        for (name, value) in [("accent_bg_color", accent.as_str()), ("accent_color", accent.as_str()),
                              ("theme_selected_bg_color", accent.as_str())] {
            css += &format!("@define-color {name} {value};\n");
        }
        css
    }
}

/// Windows 11's default blue, when the system doesn't say.
const DEFAULT: Palette = Palette { dark: false, light_accent: (0x4c, 0xc2, 0xff), dark_accent: (0x00, 0x5f, 0xb8) };

#[cfg(windows)]
pub fn palette() -> Palette {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let user = RegKey::predef(HKEY_CURRENT_USER);
    let dark = user.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|k| k.get_value::<u32, _>("AppsUseLightTheme")).is_ok_and(|light| light == 0);
    // eight RGBA shades of the accent, lightest first; the accent itself is the fourth
    let shades = user.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent")
        .and_then(|k| k.get_raw_value("AccentPalette")).map(|v| v.bytes.to_vec()).unwrap_or_default();
    let shade = |i: usize| shades.get(i * 4..i * 4 + 3).map(|c| (c[0], c[1], c[2]));
    match (shade(1), shade(4)) {
        (Some(light_accent), Some(dark_accent)) => Palette { dark, light_accent, dark_accent },
        _ => Palette { dark, ..DEFAULT },
    }
}

/// Elsewhere (testing the Windows look): TURBODM_LOOK=windows-dark for dark mode.
#[cfg(not(windows))]
pub fn palette() -> Palette {
    Palette { dark: std::env::var("TURBODM_LOOK").is_ok_and(|l| l.ends_with("dark")), ..DEFAULT }
}
