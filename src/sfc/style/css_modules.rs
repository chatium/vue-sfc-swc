//! `postcss-modules` equivalent (pending).

use super::postcss::node::CssTree;

pub fn apply(_tree: &mut CssTree, _filename: &str) -> Vec<(String, String)> {
    Vec::new()
}
