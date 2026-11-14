//! Human-friendly color input, separate from strict protocol hex encoding.
use crate::{
    Error, Result,
    protocol::{Bytes, Color},
};

pub const NAMED: &[(&str, &str)] = &[
    ("aqua", "00ffff"),
    ("beige", "f5f5dc"),
    ("black", "000000"),
    ("blue", "0000ff"),
    ("brown", "a52a2a"),
    ("chocolate", "d2691e"),
    ("coral", "ff7f50"),
    ("crimson", "dc143c"),
    ("cyan", "00ffff"),
    ("darkblue", "00008b"),
    ("darkgray", "a9a9a9"),
    ("darkgreen", "006400"),
    ("darkgrey", "a9a9a9"),
    ("darkorange", "ff8c00"),
    ("darkred", "8b0000"),
    ("deeppink", "ff1493"),
    ("deepskyblue", "00bfff"),
    ("forestgreen", "228b22"),
    ("fuchsia", "ff00ff"),
    ("gold", "ffd700"),
    ("gray", "808080"),
    ("green", "008000"),
    ("greenyellow", "adff2f"),
    ("grey", "808080"),
    ("hotpink", "ff69b4"),
    ("indigo", "4b0082"),
    ("ivory", "fffff0"),
    ("khaki", "f0e68c"),
    ("lavender", "e6e6fa"),
    ("lightblue", "add8e6"),
    ("lightgray", "d3d3d3"),
    ("lightgreen", "90ee90"),
    ("lightgrey", "d3d3d3"),
    ("lightpink", "ffb6c1"),
    ("lightskyblue", "87cefa"),
    ("lime", "00ff00"),
    ("limegreen", "32cd32"),
    ("magenta", "ff00ff"),
    ("maroon", "800000"),
    ("mintcream", "f5fffa"),
    ("navy", "000080"),
    ("olive", "808000"),
    ("orange", "ffa500"),
    ("orangered", "ff4500"),
    ("orchid", "da70d6"),
    ("pink", "ffc0cb"),
    ("plum", "dda0dd"),
    ("purple", "800080"),
    ("rebeccapurple", "663399"),
    ("red", "ff0000"),
    ("royalblue", "4169e1"),
    ("salmon", "fa8072"),
    ("seagreen", "2e8b57"),
    ("sienna", "a0522d"),
    ("silver", "c0c0c0"),
    ("skyblue", "87ceeb"),
    ("slateblue", "6a5acd"),
    ("snow", "fffafa"),
    ("steelblue", "4682b4"),
    ("tan", "d2b48c"),
    ("teal", "008080"),
    ("tomato", "ff6347"),
    ("turquoise", "40e0d0"),
    ("violet", "ee82ee"),
    ("white", "ffffff"),
    ("whitesmoke", "f5f5f5"),
    ("yellow", "ffff00"),
    ("yellowgreen", "9acd32"),
];
fn normalize(input: &str) -> String {
    input.chars().filter(|c| !c.is_whitespace() && !matches!(c, '-' | '_')).flat_map(char::to_lowercase).collect()
}
pub fn parse(input: &str) -> Result<Color> {
    let text = input.trim().trim_start_matches('#').to_ascii_lowercase();
    if text.len() == 6 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return text.parse();
    }
    let name = normalize(input);
    NAMED
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, hex)| hex.parse().expect("built-in color"))
        .ok_or(Error::Invalid("unknown color; choose a suggested name or enter six RGB hex digits"))
}
pub fn suggestions(input: &str) -> Vec<&'static str> {
    let prefix = normalize(input);
    NAMED.iter().filter_map(|(name, _)| name.starts_with(&prefix).then_some(*name)).collect()
}
pub fn name(color: Color) -> Option<&'static str> {
    NAMED.iter().find_map(|(name, value)| (parse(value).ok() == Some(color)).then_some(*name))
}
pub fn rgb(color: Color) -> ratatui::style::Color {
    let Bytes([r, g, b]) = color;
    ratatui::style::Color::Rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_aliases_hex_and_prefix_completion() {
        assert_eq!(parse("Light Blue").unwrap(), parse("#ADD8E6").unwrap());
        assert_eq!(parse("light-blue").unwrap(), parse("light_blue").unwrap());
        assert_eq!(parse("aqua").unwrap(), parse("cyan").unwrap());
        assert_eq!(suggestions("re"), vec!["rebeccapurple", "red"]);
        assert!(parse("redder").is_err());
        assert!(parse("#123").is_err());
        for (name, hex) in NAMED {
            assert_eq!(parse(name).unwrap(), parse(hex).unwrap());
        }
    }
}
