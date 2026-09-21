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

pub fn preprocess(lang: &str, source: &str) -> Result<String, String> {
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
        .map_err(|e| e.to_string())
}
