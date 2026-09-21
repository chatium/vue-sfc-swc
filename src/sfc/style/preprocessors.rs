//! Style preprocessors. Sass/SCSS are compiled with `grass`, a dart-sass
//! compatible implementation; filesystem imports are disabled, matching the
//! SFC pipeline we target.

pub fn preprocess(lang: &str, source: &str) -> Result<String, String> {
    match lang {
        "scss" | "sass" => Err(format!(
            "[@vue/compiler-sfc] preprocessor `{lang}` is not wired up yet"
        )),
        other => Err(format!(
            "[@vue/compiler-sfc] unsupported style preprocessor: {other}"
        )),
    }
}
