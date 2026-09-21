//! Port of `shared/src/patchFlags.ts`.

pub const TEXT: i32 = 1;
pub const CLASS: i32 = 1 << 1;
pub const STYLE: i32 = 1 << 2;
pub const PROPS: i32 = 1 << 3;
pub const FULL_PROPS: i32 = 1 << 4;
pub const NEED_HYDRATION: i32 = 1 << 5;
pub const STABLE_FRAGMENT: i32 = 1 << 6;
pub const KEYED_FRAGMENT: i32 = 1 << 7;
pub const UNKEYED_FRAGMENT: i32 = 1 << 8;
pub const NEED_PATCH: i32 = 1 << 9;
pub const DYNAMIC_SLOTS: i32 = 1 << 10;
pub const DEV_ROOT_FRAGMENT: i32 = 1 << 11;
pub const CACHED: i32 = -1;
pub const BAIL: i32 = -2;

pub fn patch_flag_name(flag: i32) -> &'static str {
    match flag {
        TEXT => "TEXT",
        CLASS => "CLASS",
        STYLE => "STYLE",
        PROPS => "PROPS",
        FULL_PROPS => "FULL_PROPS",
        NEED_HYDRATION => "NEED_HYDRATION",
        STABLE_FRAGMENT => "STABLE_FRAGMENT",
        KEYED_FRAGMENT => "KEYED_FRAGMENT",
        UNKEYED_FRAGMENT => "UNKEYED_FRAGMENT",
        NEED_PATCH => "NEED_PATCH",
        DYNAMIC_SLOTS => "DYNAMIC_SLOTS",
        DEV_ROOT_FRAGMENT => "DEV_ROOT_FRAGMENT",
        CACHED => "CACHED",
        BAIL => "BAIL",
        _ => "",
    }
}
