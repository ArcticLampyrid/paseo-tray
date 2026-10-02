//! Tray icons are generated, so there are no image assets to ship.
//! Hue carries the status; shape (disc vs. ring) backs it up for colour-blind users.

use crate::daemon::Status;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Idle,
    Busy,
    Error,
}

impl Tone {
    pub fn of(status: Status) -> Tone {
        match status {
            Status::Running => Tone::Ok,
            Status::Stopped => Tone::Idle,
            Status::Unavailable => Tone::Error,
            Status::Checking
            | Status::Starting
            | Status::Stopping
            | Status::Restarting
            | Status::Reloading => Tone::Busy,
        }
    }

    /// A colour-circle glyph for places that only take text (menu labels).
    pub fn dot(self) -> &'static str {
        match self {
            Tone::Ok => "🟢",
            Tone::Idle => "⚪",
            Tone::Busy => "🟡",
            Tone::Error => "🔴",
        }
    }

    fn rgb(self) -> [u8; 3] {
        match self {
            Tone::Ok => [0x2e, 0xb8, 0x5c],
            Tone::Idle => [0x8a, 0x8f, 0x98],
            Tone::Busy => [0xf0, 0xa2, 0x0c],
            Tone::Error => [0xe5, 0x48, 0x4d],
        }
    }
}

/// Rendered at 2x the usual tray size; the desktop scales it down.
const SIZE: u32 = 64;
const LOGO_SVG: &str = include_str!("../assets/paseo-logo.svg");
/// Dark enough to read on every status colour.
const LOGO_INK: &str = "#101114";

/// The `d` attribute of the logo's single path.
fn logo_path() -> &'static str {
    LOGO_SVG
        .split_once(" d=\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(d, _)| d)
        .expect("assets/paseo-logo.svg has a path")
}

/// The Paseo logo on a rounded square in the status colour (as in paseo's own
/// favicon). A stopped daemon gets an outlined square with a tinted logo, so
/// "stopped" does not rely on hue alone.
fn compose_svg(tone: Tone) -> String {
    let [r, g, b] = tone.rgb();
    let colour = format!("#{r:02x}{g:02x}{b:02x}");
    // Same placement as paseo's favicon: logo centred and scaled up slightly.
    let logo = |fill: &str| {
        format!(
            r#"<path transform="translate(350,350) scale(1.05) translate(-350,-350)" d="{}" fill="{fill}"/>"#,
            logo_path()
        )
    };
    let body = if tone == Tone::Idle {
        format!(
            r#"<rect x="24" y="24" width="652" height="652" rx="132" fill="none" stroke="{colour}" stroke-width="48"/>{}"#,
            logo(&colour)
        )
    } else {
        format!(
            r#"<rect width="700" height="700" rx="156" fill="{colour}"/>{}"#,
            logo(LOGO_INK)
        )
    };
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 700 700">{body}</svg>"#)
}

pub fn render(tone: Tone) -> Image {
    let tree = resvg::usvg::Tree::from_str(&compose_svg(tone), &resvg::usvg::Options::default())
        .expect("generated icon SVG is valid");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(SIZE, SIZE).expect("non-zero icon size");
    let scale = SIZE as f32 / tree.size().width();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(SIZE, SIZE);
    for (out, px) in buffer.make_mut_slice().iter_mut().zip(pixmap.pixels()) {
        let c = px.demultiply();
        *out = Rgba8Pixel {
            r: c.red(),
            g: c.green(),
            b: c.blue(),
            a: c.alpha(),
        };
    }
    Image::from_rgba8(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_logo_path() {
        assert!(logo_path().starts_with("M291.495"));
    }

    fn pixels(tone: Tone) -> Vec<Rgba8Pixel> {
        render(tone)
            .to_rgba8()
            .expect("rgba image")
            .as_slice()
            .to_vec()
    }

    #[test]
    fn every_tone_is_a_rounded_square_in_its_colour() {
        for tone in [Tone::Ok, Tone::Idle, Tone::Busy, Tone::Error] {
            let px = pixels(tone);
            let [r, g, b] = tone.rgb();
            assert_eq!(px.len(), (SIZE * SIZE) as usize);
            assert_eq!(px[0].a, 0, "{tone:?}: corner is transparent");
            assert!(
                px.iter().any(|p| (p.r, p.g, p.b, p.a) == (r, g, b, 255)),
                "{tone:?}: status colour present"
            );
        }
    }

    #[test]
    fn filled_tones_carry_the_dark_logo_and_idle_does_not() {
        let ink = |px: &[Rgba8Pixel]| {
            px.iter()
                .any(|p| (p.r, p.g, p.b, p.a) == (0x10, 0x11, 0x14, 255))
        };
        assert!(ink(&pixels(Tone::Ok)));
        assert!(!ink(&pixels(Tone::Idle)));
    }
}
