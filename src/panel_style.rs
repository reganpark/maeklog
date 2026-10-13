//! Presentation-only roles. Text stays ANSI-free for wrapping, search and CLI output.
use crossterm::style::{Attribute, Color};
use std::{borrow::Borrow, fmt, ops::Deref};

pub const BODY: Color = Color::Rgb {
    r: 235,
    g: 219,
    b: 178,
}; // Gruvbox fg1
pub const MUTED: Color = BODY; // Secondary information keeps the same readable foreground.
pub const AQUA: Color = Color::Rgb {
    r: 142,
    g: 192,
    b: 124,
};
pub const YELLOW: Color = Color::Rgb {
    r: 250,
    g: 189,
    b: 47,
};
pub const ORANGE: Color = Color::Rgb {
    r: 254,
    g: 128,
    b: 25,
};
pub const RED: Color = Color::Rgb {
    r: 251,
    g: 101,
    b: 84,
}; // Gruvbox red lifted for 4.5:1 small-text contrast on bg0.
pub const SURFACE: Color = Color::Rgb {
    r: 60,
    g: 56,
    b: 54,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Role {
    #[default]
    Body,
    Muted,
    Title,
    Heading,
    Action,
    Warning,
    Error,
    Selected,
}
impl Role {
    pub fn color(self) -> Color {
        match self {
            Self::Body | Self::Title => BODY,
            Self::Muted => MUTED,
            Self::Heading => AQUA,
            Self::Action | Self::Selected => YELLOW,
            Self::Warning => ORANGE,
            Self::Error => RED,
        }
    }
    pub fn attribute(self) -> Attribute {
        match self {
            Self::Body | Self::Muted => Attribute::NormalIntensity,
            _ => Attribute::Bold,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Line {
    pub text: String,
    pub role: Role,
}
impl Deref for Line {
    type Target = str;
    fn deref(&self) -> &str {
        &self.text
    }
}
impl Borrow<str> for Line {
    fn borrow(&self) -> &str {
        &self.text
    }
}
impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.text.fmt(f)
    }
}
pub fn wrap(lines: &mut Vec<Line>, text: &str, width: usize, role: Role) {
    let mut plain = Vec::new();
    crate::panel::wrapped_bullet(&mut plain, text, width, "");
    lines.extend(plain.into_iter().map(|text| Line { text, role }));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gruvbox_text_roles_pass_contrast_on_dark_surfaces() {
        fn luminance(color: Color) -> f64 {
            let Color::Rgb { r, g, b } = color else {
                panic!("RGB required")
            };
            let linear = |v: u8| {
                let v = f64::from(v) / 255.;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
        }
        for background in [
            Color::Rgb {
                r: 40,
                g: 40,
                b: 40,
            },
            SURFACE,
        ] {
            for role in [
                Role::Body,
                Role::Muted,
                Role::Title,
                Role::Heading,
                Role::Action,
                Role::Warning,
                Role::Error,
                Role::Selected,
            ] {
                // Menus use only body, aqua title and yellow selection on bg1.
                if background == SURFACE
                    && !matches!(role, Role::Body | Role::Heading | Role::Selected)
                {
                    continue;
                }
                let contrast = (luminance(role.color()) + 0.05) / (luminance(background) + 0.05);
                assert!(contrast >= 4.5, "{role:?}: {contrast}");
            }
        }
    }
    #[test]
    fn roles_survive_hangul_wrapping_without_ansi_in_text() {
        let mut lines = Vec::new();
        wrap(&mut lines, "결정 — 현재 유효한 결정", 10, Role::Heading);
        assert!(lines.len() > 1);
        assert!(
            lines
                .iter()
                .all(|l| l.role == Role::Heading && !l.contains('\x1b'))
        );
        // Model/body text is never classified by keywords or made to look like a heading.
        wrap(&mut lines, "막힘", 10, Role::Body);
        assert_eq!(lines.last().unwrap().role, Role::Body);
    }
}
