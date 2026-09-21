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
        .map_err(|e| format_sass_error(&e, filename))
}

pub fn preprocess(lang: &str, source: &str) -> Result<String, String> {
    preprocess_with_filename(lang, source, "input.scss")
}
