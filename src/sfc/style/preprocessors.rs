//! Style preprocessors.
//!
//! Sass/SCSS go through `grass`, a dart-sass compatible implementation.
//! Filesystem imports are denied, matching the SFC pipeline we target
//! (`helper.cjs` installs importers that refuse to touch the file system).

use std::path::Path;

use grass::{Fs, InputSyntax, Options};

#[derive(Debug)]
struct DenyFs;

impl Fs for DenyFs {
    fn is_dir(&self, _path: &Path) -> bool {
        false
    }
    fn is_file(&self, _path: &Path) -> bool {
        false
    }
    fn read(&self, _path: &Path) -> std::io::Result<Vec<u8>> {
        Err(std::io::Error::other("Sass filesystem imports are disabled"))
    }
}

/// Reformats a grass error to dart-sass's shape. The wording of Sass errors
/// still differs between the two implementations — see README.
fn format_sass_error(e: &grass::Error, filename: &str) -> String {
    use grass::ErrorKind as PublicSassErrorKind;
    match e.clone().kind() {
        PublicSassErrorKind::ParseError { message, loc, .. } => {
            // the consumer denies filesystem imports with this message
            let message = if message == "Can't find stylesheet to import." {
                "Sass filesystem imports are disabled".to_string()
            } else {
                message
            };
            let line = loc.begin.line + 1;
            let col = loc.begin.column + 1;
            let padding = " ".repeat(line.to_string().len() + 1);
            let carets = loc
                .end
                .column
                .max(loc.begin.column)
                .saturating_sub(loc.begin.column.min(loc.end.column))
                .max(1);
            format!(
                "{message}\n{padding}╷\n{line} │ {}\n{padding}│ {}{}\n{padding}╵\n  {filename} {line}:{col}  root stylesheet",
                loc.file.source_line(loc.begin.line),
                " ".repeat(loc.begin.column),
                "^".repeat(carets),
            )
        }
        other => format!("{other:?}"),
    }
}

pub fn preprocess_with_filename(
    lang: &str,
    source: &str,
    filename: &str,
) -> Result<String, String> {
    let syntax = match lang {
        // `lang="sass"` also parses as SCSS: Vue passes the legacy
        // `indentedSyntax` flag to sass's modern `compileString`, which
        // ignores it, so indented sources are parsed as SCSS.
        "scss" | "sass" => InputSyntax::Scss,
        other => {
            return Err(format!(
                "[@vue/compiler-sfc] unsupported style preprocessor: {other}"
            ));
        }
    };
    let fs = DenyFs;
    let options = Options::default().input_syntax(syntax).fs(&fs);
    grass::from_string(source.to_string(), &options)
        // dart-sass does not emit a trailing newline; grass does
        .map(|css| match css.strip_suffix('\n') {
            Some(s) => s.to_string(),
            None => css,
        })
        .map(|css| as_dart_sass_output(&css))
        .map_err(|e| format_sass_error(&e, filename))
}

pub fn preprocess(lang: &str, source: &str) -> Result<String, String> {
    preprocess_with_filename(lang, source, "input.scss")
}

/// Where grass's output differs from dart-sass's, rewritten after the fact:
///
/// - dart-sass does not emit a 4- or 8-digit hex color as written, as it does
///   other hex colors: it serializes it as `rgba(r, g, b, a)`, or by name or as
///   `#rrggbb` when it is opaque. Custom properties, strings and `url()` are
///   not SassScript there and stay as they are.
/// - an `hsl()`/`hsla()` color's hue is unitless in dart-sass and `Ndeg` in
///   grass.
/// - a selector list kept on several lines continues at the rule's own
///   indentation inside an at-rule; grass starts those lines at column 0.
// ponytail: text from `unquote()`/interpolation that happens to look like such
// a color is a string in dart-sass and would be rewritten here
fn as_dart_sass_output(css: &str) -> String {
    use super::postcss::node::CssKind;
    if !css.contains('#') && !css.contains(",\n") && !css.contains("deg, ") {
        return css.to_string();
    }
    let Ok(mut tree) = super::postcss::parse::parse(css) else {
        return css.to_string();
    };
    let mut changed = false;
    for node in tree.nodes.iter_mut() {
        match node.kind {
            CssKind::Decl if !node.prop.starts_with("--") => {
                if let Some(v) = rewrite_colors(&node.value) {
                    node.value = v;
                    node.raws.value = None;
                    changed = true;
                }
            }
            CssKind::Rule if node.selector.contains('\n') => {
                let indent = match node.raws.before.as_deref().and_then(|b| b.rsplit_once('\n')) {
                    Some((_, indent)) if !indent.is_empty() => indent.to_string(),
                    _ => continue,
                };
                let mut lines = node.selector.split('\n');
                let mut selector = lines.next().unwrap_or_default().to_string();
                for line in lines {
                    selector.push('\n');
                    if !line.starts_with([' ', '\t']) {
                        selector.push_str(&indent);
                    }
                    selector.push_str(line);
                }
                if selector != node.selector {
                    node.selector = selector;
                    node.raws.selector = None;
                    changed = true;
                }
            }
            _ => {}
        }
    }
    if changed {
        super::postcss::stringify::stringify(&tree)
    } else {
        css.to_string()
    }
}

fn rewrite_colors(value: &str) -> Option<String> {
    let b = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let (mut i, mut copied, mut changed) = (0, 0, false);
    while i < b.len() {
        match b[i] {
            q @ (b'"' | b'\'') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b'(' if value[..i].to_ascii_lowercase().ends_with("url") => {
                i = value[i..].find(')').map_or(b.len(), |j| i + j + 1);
            }
            b'#' if i == 0 || !is_word(b[i - 1]) => {
                let len = b[i + 1..].iter().take_while(|c| is_word(**c)).count();
                let digits = &value[i + 1..i + 1 + len];
                if (len == 4 || len == 8) && digits.bytes().all(|c| c.is_ascii_hexdigit()) {
                    out.push_str(&value[copied..i]);
                    out.push_str(&dart_sass_color(digits));
                    copied = i + 1 + len;
                    changed = true;
                }
                i += 1 + len;
            }
            b'h' if (i == 0 || !is_word(b[i - 1])) && value[i..].starts_with("hsl") => {
                // grass's own serialization of an hsl color, nothing looser
                static HSL: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
                    regex::Regex::new(r"^hsla?\(-?[0-9.]+(deg), [0-9.]+%, [0-9.]+%(, [0-9.]+)?\)")
                        .unwrap()
                });
                match HSL.captures(&value[i..]) {
                    Some(m) => {
                        let deg = m.get(1).unwrap();
                        out.push_str(&value[copied..i + deg.start()]);
                        copied = i + deg.end();
                        changed = true;
                        i += m.get(0).unwrap().end();
                    }
                    None => i += 1,
                }
            }
            _ => i += 1,
        }
    }
    changed.then(|| out + &value[copied..])
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'_'
}

/// `#rgba` / `#rrggbbaa` the way dart-sass's serializer writes the color
fn dart_sass_color(digits: &str) -> String {
    let channel = |i: usize| -> u32 {
        if digits.len() == 4 {
            u32::from_str_radix(&digits[i..i + 1], 16).unwrap() * 17
        } else {
            u32::from_str_radix(&digits[i * 2..i * 2 + 2], 16).unwrap()
        }
    };
    let (r, g, b, a) = (channel(0), channel(1), channel(2), channel(3));
    if a == 255 {
        let rgb = (r << 16) | (g << 8) | b;
        return match DART_SASS_COLOR_NAMES.binary_search_by_key(&rgb, |(c, _)| *c) {
            Ok(i) => DART_SASS_COLOR_NAMES[i].1.to_string(),
            Err(_) => format!("#{rgb:06x}"),
        };
    }
    // dart-sass prints at most ten decimal places
    let alpha = format!("{:.10}", a as f64 / 255.0);
    let alpha = alpha.trim_end_matches('0').trim_end_matches('.');
    format!("rgba({r}, {g}, {b}, {alpha})")
}

/// The name dart-sass prints for an opaque color, generated from its own
/// output (`#rrggbbff` for every CSS color name), sorted by value
const DART_SASS_COLOR_NAMES: &[(u32, &str)] = &[
    (0x000000, "black"),
    (0x000080, "navy"),
    (0x00008b, "darkblue"),
    (0x0000cd, "mediumblue"),
    (0x0000ff, "blue"),
    (0x006400, "darkgreen"),
    (0x008000, "green"),
    (0x008080, "teal"),
    (0x008b8b, "darkcyan"),
    (0x00bfff, "deepskyblue"),
    (0x00ced1, "darkturquoise"),
    (0x00fa9a, "mediumspringgreen"),
    (0x00ff00, "lime"),
    (0x00ff7f, "springgreen"),
    (0x00ffff, "aqua"),
    (0x191970, "midnightblue"),
    (0x1e90ff, "dodgerblue"),
    (0x20b2aa, "lightseagreen"),
    (0x228b22, "forestgreen"),
    (0x2e8b57, "seagreen"),
    (0x2f4f4f, "darkslategray"),
    (0x32cd32, "limegreen"),
    (0x3cb371, "mediumseagreen"),
    (0x40e0d0, "turquoise"),
    (0x4169e1, "royalblue"),
    (0x4682b4, "steelblue"),
    (0x483d8b, "darkslateblue"),
    (0x48d1cc, "mediumturquoise"),
    (0x4b0082, "indigo"),
    (0x556b2f, "darkolivegreen"),
    (0x5f9ea0, "cadetblue"),
    (0x6495ed, "cornflowerblue"),
    (0x663399, "rebeccapurple"),
    (0x66cdaa, "mediumaquamarine"),
    (0x696969, "dimgray"),
    (0x6a5acd, "slateblue"),
    (0x6b8e23, "olivedrab"),
    (0x708090, "slategray"),
    (0x778899, "lightslategray"),
    (0x7b68ee, "mediumslateblue"),
    (0x7cfc00, "lawngreen"),
    (0x7fff00, "chartreuse"),
    (0x7fffd4, "aquamarine"),
    (0x800000, "maroon"),
    (0x800080, "purple"),
    (0x808000, "olive"),
    (0x808080, "gray"),
    (0x87ceeb, "skyblue"),
    (0x87cefa, "lightskyblue"),
    (0x8a2be2, "blueviolet"),
    (0x8b0000, "darkred"),
    (0x8b008b, "darkmagenta"),
    (0x8b4513, "saddlebrown"),
    (0x8fbc8f, "darkseagreen"),
    (0x90ee90, "lightgreen"),
    (0x9370db, "mediumpurple"),
    (0x9400d3, "darkviolet"),
    (0x98fb98, "palegreen"),
    (0x9932cc, "darkorchid"),
    (0x9acd32, "yellowgreen"),
    (0xa0522d, "sienna"),
    (0xa52a2a, "brown"),
    (0xa9a9a9, "darkgray"),
    (0xadd8e6, "lightblue"),
    (0xadff2f, "greenyellow"),
    (0xafeeee, "paleturquoise"),
    (0xb0c4de, "lightsteelblue"),
    (0xb0e0e6, "powderblue"),
    (0xb22222, "firebrick"),
    (0xb8860b, "darkgoldenrod"),
    (0xba55d3, "mediumorchid"),
    (0xbc8f8f, "rosybrown"),
    (0xbdb76b, "darkkhaki"),
    (0xc0c0c0, "silver"),
    (0xc71585, "mediumvioletred"),
    (0xcd5c5c, "indianred"),
    (0xcd853f, "peru"),
    (0xd2691e, "chocolate"),
    (0xd2b48c, "tan"),
    (0xd3d3d3, "lightgray"),
    (0xd8bfd8, "thistle"),
    (0xda70d6, "orchid"),
    (0xdaa520, "goldenrod"),
    (0xdb7093, "palevioletred"),
    (0xdc143c, "crimson"),
    (0xdcdcdc, "gainsboro"),
    (0xdda0dd, "plum"),
    (0xdeb887, "burlywood"),
    (0xe0ffff, "lightcyan"),
    (0xe6e6fa, "lavender"),
    (0xe9967a, "darksalmon"),
    (0xee82ee, "violet"),
    (0xeee8aa, "palegoldenrod"),
    (0xf08080, "lightcoral"),
    (0xf0e68c, "khaki"),
    (0xf0f8ff, "aliceblue"),
    (0xf0fff0, "honeydew"),
    (0xf0ffff, "azure"),
    (0xf4a460, "sandybrown"),
    (0xf5deb3, "wheat"),
    (0xf5f5dc, "beige"),
    (0xf5f5f5, "whitesmoke"),
    (0xf5fffa, "mintcream"),
    (0xf8f8ff, "ghostwhite"),
    (0xfa8072, "salmon"),
    (0xfaebd7, "antiquewhite"),
    (0xfaf0e6, "linen"),
    (0xfafad2, "lightgoldenrodyellow"),
    (0xfdf5e6, "oldlace"),
    (0xff0000, "red"),
    (0xff00ff, "fuchsia"),
    (0xff1493, "deeppink"),
    (0xff4500, "orangered"),
    (0xff6347, "tomato"),
    (0xff69b4, "hotpink"),
    (0xff7f50, "coral"),
    (0xff8c00, "darkorange"),
    (0xffa07a, "lightsalmon"),
    (0xffa500, "orange"),
    (0xffb6c1, "lightpink"),
    (0xffc0cb, "pink"),
    (0xffd700, "gold"),
    (0xffdab9, "peachpuff"),
    (0xffdead, "navajowhite"),
    (0xffe4b5, "moccasin"),
    (0xffe4c4, "bisque"),
    (0xffe4e1, "mistyrose"),
    (0xffebcd, "blanchedalmond"),
    (0xffefd5, "papayawhip"),
    (0xfff0f5, "lavenderblush"),
    (0xfff5ee, "seashell"),
    (0xfff8dc, "cornsilk"),
    (0xfffacd, "lemonchiffon"),
    (0xfffaf0, "floralwhite"),
    (0xfffafa, "snow"),
    (0xffff00, "yellow"),
    (0xffffe0, "lightyellow"),
    (0xfffff0, "ivory"),
    (0xffffff, "white"),
];
