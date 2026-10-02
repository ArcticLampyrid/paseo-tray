//! Pairing offers (`paseo daemon pair --json`) and their QR code.

use crate::cli::Cli;
use anyhow::{Context, Result};
use qrcode::{Color, QrCode};
use serde::Deserialize;
use slint::{Rgba8Pixel, SharedPixelBuffer};

pub const RELAY_DOCS_URL: &str = "https://paseo.sh/docs/security";

#[derive(Debug)]
pub enum Offer {
    Ready {
        url: String,
    },
    /// The daemon will not offer pairing until relay is enabled, which edits its config.
    RelayDisabled,
}

#[derive(Deserialize)]
struct RawOffer {
    url: Option<String>,
}

pub fn fetch(cli: &Cli, enable_relay: bool) -> Result<Offer> {
    let mut args = vec!["daemon", "pair", "--json"];
    if enable_relay {
        args.push("--relay");
    }
    match cli.json::<RawOffer>(&args) {
        Ok(RawOffer { url: Some(url) }) => Ok(Offer::Ready { url }),
        Ok(RawOffer { url: None }) => anyhow::bail!("the daemon returned no pairing link"),
        Err(e) if format!("{e:#}").contains("RELAY_DISABLED") => Ok(Offer::RelayDisabled),
        Err(e) => Err(e),
    }
}

const QUIET_ZONE: usize = 4;
const MODULE_PX: usize = 6;

/// Black-on-white QR code with the standard quiet zone, ready to wrap in an `Image`.
pub fn qr_pixels(text: &str) -> Result<SharedPixelBuffer<Rgba8Pixel>> {
    let code = QrCode::new(text.as_bytes()).context("encoding the pairing link as a QR code")?;
    let modules = code.width();
    let side = (modules + 2 * QUIET_ZONE) * MODULE_PX;
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(side as u32, side as u32);
    let colors = code.to_colors();
    for (i, pixel) in buffer.make_mut_slice().iter_mut().enumerate() {
        let (mx, my) = ((i % side) / MODULE_PX, (i / side) / MODULE_PX);
        let dark = mx
            .checked_sub(QUIET_ZONE)
            .zip(my.checked_sub(QUIET_ZONE))
            .filter(|&(x, y)| x < modules && y < modules)
            .is_some_and(|(x, y)| colors[y * modules + x] == Color::Dark);
        let v = if dark { 0 } else { 255 };
        *pixel = Rgba8Pixel {
            r: v,
            g: v,
            b: v,
            a: 255,
        };
    }
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_is_square_with_quiet_zone_and_dark_modules() {
        let qr = qr_pixels("https://example.com/#offer=abc").unwrap();
        assert_eq!(qr.width(), qr.height());
        assert_eq!((qr.width() as usize) % MODULE_PX, 0);
        let pixels = qr.as_slice();
        assert_eq!(pixels[0].r, 255, "corner is quiet zone");
        assert!(pixels.iter().any(|p| p.r == 0), "has dark modules");
    }
}
