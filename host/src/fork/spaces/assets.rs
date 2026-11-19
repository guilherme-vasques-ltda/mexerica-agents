//! Assets passivos, limitados e autocontidos. Nenhum caminho/URL externo é resolvido.
use super::{Appearance, OwnerPresentation, Tile};
use anyhow::{Result, anyhow, ensure};
use base64::Engine as _;
use std::collections::HashSet;
pub const ICONS: &[&str] = &[
    "briefcase",
    "building",
    "chart",
    "target",
    "rocket",
    "scale",
    "contract",
    "globe",
    "bag",
    "cart",
    "store",
    "boxes",
    "tag",
    "truck",
    "card",
    "megaphone",
    "code",
    "chip",
    "cloud",
    "zap",
    "database",
    "bot",
    "user",
    "home",
    "leaf",
    "heart",
    "book",
    "coffee",
    "paw",
    "dumbbell",
];
const MAX_ASSET: usize = 256 * 1024;
pub fn tile(tile: &Tile) -> Result<()> {
    ensure!(
        ["violeta", "cobre", "musgo", "oceano", "ameixa", "grafite"].contains(&tile.color.as_str()),
        "invalid_tile_color"
    );
    ensure!(["filled", "soft"].contains(&tile.style.as_str()), "invalid_tile_style");
    match tile.symbol.kind.as_str() {
        "icon" => ensure!(ICONS.contains(&tile.symbol.value.as_str()), "invalid_tile_icon"),
        "letters" => letters(&tile.symbol.value)?,
        "logo" => image(&tile.symbol.value, true)?,
        _ => return Err(anyhow!("invalid_tile_symbol")),
    }
    Ok(())
}
fn letters(value: &str) -> Result<()> {
    ensure!(
        (1..=2).contains(&value.chars().count()) && value.chars().all(char::is_alphanumeric),
        "invalid_tile_letters"
    );
    Ok(())
}
pub fn owner(owner: &OwnerPresentation) -> Result<()> {
    ensure!(
        !owner.name.trim().is_empty() && owner.name.chars().count() <= 60 && !owner.name.chars().any(char::is_control),
        "invalid_owner_presentation"
    );
    letters(&owner.initials)?;
    if let Some(photo) = &owner.photo {
        image(photo, false)?;
    }
    Ok(())
}
pub fn appearance(value: &Appearance) -> Result<()> {
    ensure!(
        ["none", "serra", "duna", "mare", "noite", "cidade", "risca", "mogno"].contains(&value.wallpaper.as_str()),
        "invalid_wallpaper"
    );
    for (n, max) in [(value.blur, 40.0), (value.transparency, 100.0), (value.veil_light, 0.6), (value.veil_dark, 0.6)] {
        ensure!(n.is_finite() && (0.0..=max).contains(&n), "invalid_appearance_range");
    }
    Ok(())
}
fn image(value: &str, svg_allowed: bool) -> Result<()> {
    ensure!(value.len() <= MAX_ASSET * 4 / 3 + 128, "asset_too_large");
    if let Some(raw) = value.strip_prefix("data:image/png;base64,") {
        let bytes = base64::engine::general_purpose::STANDARD.decode(raw)?;
        ensure!(bytes.len() <= MAX_ASSET, "asset_too_large");
        return png(&bytes);
    }
    if svg_allowed && let Some(raw) = value.strip_prefix("data:image/svg+xml;base64,") {
        let bytes = base64::engine::general_purpose::STANDARD.decode(raw)?;
        ensure!(bytes.len() <= MAX_ASSET, "asset_too_large");
        return svg(std::str::from_utf8(&bytes)?);
    }
    Err(anyhow!("asset_requires_inline_png_or_passive_svg"))
}
fn crc(bytes: &[u8]) -> u32 {
    let mut value = u32::MAX;
    for byte in bytes {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            value = (value >> 1) ^ (0xedb8_8320_u32.wrapping_mul(value & 1));
        }
    }
    !value
}
fn png(bytes: &[u8]) -> Result<()> {
    ensure!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "invalid_png");
    let mut offset = 8;
    let mut header = false;
    let mut data = false;
    let mut ended = false;
    while offset < bytes.len() {
        ensure!(offset + 12 <= bytes.len() && !ended, "invalid_png_chunk");
        let size = usize::try_from(u32::from_be_bytes(bytes[offset..offset + 4].try_into()?))?;
        let end =
            offset.checked_add(12).and_then(|v| v.checked_add(size)).ok_or_else(|| anyhow!("invalid_png_chunk"))?;
        ensure!(end <= bytes.len(), "invalid_png_chunk");
        let kind = &bytes[offset + 4..offset + 8];
        let payload = &bytes[offset + 8..end - 4];
        ensure!(
            crc(&bytes[offset + 4..end - 4]) == u32::from_be_bytes(bytes[end - 4..end].try_into()?),
            "invalid_png_crc"
        );
        if !header {
            ensure!(kind == b"IHDR" && size == 13, "invalid_png_header");
            let w = u32::from_be_bytes(payload[0..4].try_into()?);
            let h = u32::from_be_bytes(payload[4..8].try_into()?);
            ensure!((1..=1024).contains(&w) && (1..=1024).contains(&h), "invalid_png_dimensions");
            ensure!(
                matches!(payload[8], 1 | 2 | 4 | 8 | 16)
                    && matches!(payload[9], 0 | 2 | 3 | 4 | 6)
                    && payload[10] == 0
                    && payload[11] == 0
                    && payload[12] <= 1,
                "invalid_png_header"
            );
            header = true;
        } else if kind == b"IDAT" {
            ensure!(size > 0, "empty_png_data");
            data = true;
        } else if kind == b"IEND" {
            ensure!(size == 0 && data, "invalid_png_end");
            ended = true;
        } else {
            ensure!(
                [b"PLTE", b"tRNS", b"gAMA", b"cHRM", b"sRGB", b"pHYs", b"sBIT", b"bKGD"].contains(&kind.try_into()?),
                "unsupported_png_chunk"
            );
        }
        offset = end;
    }
    ensure!(header && data && ended, "incomplete_png");
    Ok(())
}
fn svg(text: &str) -> Result<()> {
    ensure!(!text.contains(['&', '\0']) && !text.to_ascii_lowercase().contains("url"), "active_svg_refused");
    let tags = ["svg", "g", "path", "rect", "circle", "ellipse", "line", "polyline", "polygon"];
    let attrs = [
        "xmlns",
        "viewBox",
        "width",
        "height",
        "x",
        "y",
        "cx",
        "cy",
        "rx",
        "ry",
        "r",
        "x1",
        "x2",
        "y1",
        "y2",
        "d",
        "points",
        "fill",
        "stroke",
        "stroke-width",
        "stroke-linecap",
        "stroke-linejoin",
        "fill-rule",
        "opacity",
        "fill-opacity",
        "stroke-opacity",
        "transform",
    ];
    let mut rest = text.trim();
    let mut stack = Vec::new();
    let mut root = false;
    let mut nodes = 0;
    while !rest.is_empty() {
        ensure!(rest.starts_with('<'), "svg_text_refused");
        let end = rest.find('>').ok_or_else(|| anyhow!("invalid_svg"))?;
        let token = rest[1..end].trim();
        rest = rest[end + 1..].trim_start();
        if let Some(close) = token.strip_prefix('/') {
            ensure!(stack.pop() == Some(close.trim()), "invalid_svg_nesting");
            continue;
        }
        let self_closed = token.ends_with('/');
        let token = token.trim_end_matches('/').trim_end();
        let split = token.find(char::is_whitespace).unwrap_or(token.len());
        let name = &token[..split];
        ensure!(tags.contains(&name), "active_svg_element");
        if root {
            ensure!(!stack.is_empty() && name != "svg", "invalid_svg_root");
        } else {
            ensure!(name == "svg", "invalid_svg_root");
            root = true;
        }
        nodes += 1;
        ensure!(nodes <= 4096, "svg_too_complex");
        let mut attributes = token[split..].trim();
        let mut seen = HashSet::new();
        let (mut width, mut height, mut view_box) = (None, None, None);
        while !attributes.is_empty() {
            let equals = attributes.find('=').ok_or_else(|| anyhow!("invalid_svg_attribute"))?;
            let key = attributes[..equals].trim();
            ensure!(attrs.contains(&key) && seen.insert(key), "active_svg_attribute");
            let value = attributes[equals + 1..].trim_start();
            let quote = value.chars().next().ok_or_else(|| anyhow!("invalid_svg_attribute"))?;
            ensure!(quote == '\'' || quote == '"', "invalid_svg_attribute");
            let value = &value[1..];
            let finish = value.find(quote).ok_or_else(|| anyhow!("invalid_svg_attribute"))?;
            let content = &value[..finish];
            if key == "xmlns" {
                ensure!(name == "svg" && content == "http://www.w3.org/2000/svg", "external_svg_namespace");
            } else {
                ensure!(
                    content.len() <= 65536
                        && content
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || ".,-+#()%".contains(c)),
                    "active_svg_value"
                );
            }
            match key {
                "width" => width = Some(svg_length(content)?),
                "height" => height = Some(svg_length(content)?),
                "viewBox" => {
                    ensure!(name == "svg", "invalid_svg_viewbox");
                    ensure!(content.split(',').all(|part| !part.trim().is_empty()), "invalid_svg_viewbox");
                    let values = content
                        .split(|c: char| c.is_ascii_whitespace() || c == ',')
                        .filter(|s| !s.is_empty())
                        .map(svg_number)
                        .collect::<Result<Vec<_>>>()?;
                    ensure!(values.len() == 4, "invalid_svg_viewbox");
                    let (x, y, w, h) = (values[0], values[1], values[2], values[3]);
                    ensure!(
                        (1.0..=1024.0).contains(&w)
                            && (1.0..=1024.0).contains(&h)
                            && [x, y, x + w, y + h].iter().all(|v| (-1024.0..=1024.0).contains(v)),
                        "invalid_svg_viewbox"
                    );
                    view_box = Some((w, h));
                }
                _ => {}
            }
            attributes = value[finish + 1..].trim_start();
        }
        if name == "svg" {
            ensure!(
                width.or(view_box.map(|v| v.0)).is_some() && height.or(view_box.map(|v| v.1)).is_some(),
                "svg_dimensions_required"
            );
        }
        if !self_closed {
            stack.push(name);
        }
    }
    ensure!(root && stack.is_empty(), "incomplete_svg");
    Ok(())
}

/// Subconjunto decimal explícito: não aceita NaN, expoentes/overflow ou unidades implícitas.
fn svg_number(text: &str) -> Result<f64> {
    let raw = text.trim();
    let digits = raw.strip_prefix(['-', '+']).unwrap_or(raw);
    ensure!(
        !digits.is_empty()
            && digits.bytes().any(|b| b.is_ascii_digit())
            && digits.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            && digits.bytes().filter(|b| *b == b'.').count() <= 1,
        "invalid_svg_number"
    );
    let n: f64 = raw.parse()?;
    ensure!(n.is_finite(), "invalid_svg_number");
    Ok(n)
}
fn svg_length(text: &str) -> Result<f64> {
    let raw = text.trim();
    let n = svg_number(raw.strip_suffix("px").unwrap_or(raw))?;
    ensure!((1.0..=1024.0).contains(&n), "invalid_svg_dimensions");
    Ok(n)
}
