//! QR codes, drawn on the server as inline SVG: no script, and nothing the
//! pages' CSP has to allow. Black on white in either colour scheme, which
//! every scanner reads.

use qrcode::{EcLevel, QrCode, render::svg};

const XML_DECLARATION: &str = r#"<?xml version="1.0" standalone="yes"?>"#;

/// An `<svg>` element encoding `text`, one unit per module, quiet zone
/// included; CSS decides how big it shows.
pub fn svg(text: &str) -> String {
    let code = QrCode::with_error_correction_level(text, EcLevel::M)
        .expect("links are far shorter than a QR code holds");
    let image = code
        .render::<svg::Color>()
        .module_dimensions(1, 1)
        .quiet_zone(true)
        .build();
    match image.strip_prefix(XML_DECLARATION) {
        Some(element) => element.to_owned(),
        None => image,
    }
}

#[cfg(test)]
mod tests {
    use regex::Regex;

    use super::*;

    const LINK: &str = "https://members.weblounge.org/invite/q7Hx2mZp9vLcT4wRfBa-_w";
    /// Pixels per module when rasterizing for the decoder.
    const SCALE: usize = 4;

    /// Rasterizes the SVG the renderer draws, a white square under one path
    /// of dark rectangles, and decodes it the way a phone's camera would.
    fn decode(svg: &str) -> String {
        let size: usize = Regex::new(r#"viewBox="0 0 (\d+) \d+""#)
            .unwrap()
            .captures(svg)
            .unwrap()[1]
            .parse()
            .unwrap();
        let mut dark = vec![false; size * size];
        let rect = Regex::new(r"M(\d+) (\d+)h(\d+)v(\d+)H\d+V\d+").unwrap();
        let path = Regex::new(r##"<path fill="#000" d="([^"]*)"/>"##)
            .unwrap()
            .captures(svg)
            .unwrap()[1]
            .to_owned();
        assert_eq!(rect.replace_all(&path, ""), "", "only rectangles");
        for r in rect.captures_iter(&path) {
            let [x, y, w, h] = [1, 2, 3, 4].map(|i| r[i].parse::<usize>().unwrap());
            for row in y..y + h {
                for column in x..x + w {
                    dark[row * size + column] = true;
                }
            }
        }
        let pixels = size * SCALE;
        let mut image = rqrr::PreparedImage::prepare_from_greyscale(pixels, pixels, |x, y| {
            if dark[(y / SCALE) * size + x / SCALE] {
                0
            } else {
                255
            }
        });
        let grids = image.detect_grids();
        assert_eq!(grids.len(), 1);
        grids[0].decode().unwrap().1
    }

    #[test]
    fn scans_back_to_the_text() {
        assert_eq!(decode(&svg(LINK)), LINK);
    }

    #[test]
    fn is_an_svg_element_with_no_styles_or_scripts() {
        let svg = svg(LINK);
        assert!(
            svg.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg""#),
            "{svg}"
        );
        assert!(svg.ends_with("</svg>"), "{svg}");
        for forbidden in ["<?xml", "style", "script", "href"] {
            assert!(!svg.contains(forbidden), "{forbidden}: {svg}");
        }
        assert!(svg.contains(r##"fill="#fff""##) && svg.contains(r##"fill="#000""##));
    }
}
