//! Port of `compiler-core/src/errors.ts` and `compiler-dom/src/errors.ts`.

use super::ast::SourceLocation;

/// Core error codes. Values match the TypeScript enum ordinals exactly, because
/// `compileTemplate` surfaces them to callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
#[allow(non_camel_case_types, dead_code)]
pub enum ErrorCode {
    ABRUPT_CLOSING_OF_EMPTY_COMMENT = 0,
    CDATA_IN_HTML_CONTENT,
    DUPLICATE_ATTRIBUTE,
    END_TAG_WITH_ATTRIBUTES,
    END_TAG_WITH_TRAILING_SOLIDUS,
    EOF_BEFORE_TAG_NAME,
    EOF_IN_CDATA,
    EOF_IN_COMMENT,
    EOF_IN_SCRIPT_HTML_COMMENT_LIKE_TEXT,
    EOF_IN_TAG,
    INCORRECTLY_CLOSED_COMMENT,
    INCORRECTLY_OPENED_COMMENT,
    INVALID_FIRST_CHARACTER_OF_TAG_NAME,
    MISSING_ATTRIBUTE_VALUE,
    MISSING_END_TAG_NAME,
    MISSING_WHITESPACE_BETWEEN_ATTRIBUTES,
    NESTED_COMMENT,
    UNEXPECTED_CHARACTER_IN_ATTRIBUTE_NAME,
    UNEXPECTED_CHARACTER_IN_UNQUOTED_ATTRIBUTE_VALUE,
    UNEXPECTED_EQUALS_SIGN_BEFORE_ATTRIBUTE_NAME,
    UNEXPECTED_NULL_CHARACTER,
    UNEXPECTED_QUESTION_MARK_INSTEAD_OF_TAG_NAME,
    UNEXPECTED_SOLIDUS_IN_TAG,

    X_INVALID_END_TAG,
    X_MISSING_END_TAG,
    X_MISSING_INTERPOLATION_END,
    X_MISSING_DIRECTIVE_NAME,
    X_MISSING_DYNAMIC_DIRECTIVE_ARGUMENT_END,

    X_V_IF_NO_EXPRESSION,
    X_V_IF_SAME_KEY,
    X_V_ELSE_NO_ADJACENT_IF,
    X_V_FOR_NO_EXPRESSION,
    X_V_FOR_MALFORMED_EXPRESSION,
    X_V_FOR_TEMPLATE_KEY_PLACEMENT,
    X_V_BIND_NO_EXPRESSION,
    X_V_ON_NO_EXPRESSION,
    X_V_SLOT_UNEXPECTED_DIRECTIVE_ON_SLOT_OUTLET,
    X_V_SLOT_MIXED_SLOT_USAGE,
    X_V_SLOT_DUPLICATE_SLOT_NAMES,
    X_V_SLOT_EXTRANEOUS_DEFAULT_SLOT_CHILDREN,
    X_V_SLOT_MISPLACED,
    X_V_MODEL_NO_EXPRESSION,
    X_V_MODEL_MALFORMED_EXPRESSION,
    X_V_MODEL_ON_SCOPE_VARIABLE,
    X_V_MODEL_ON_PROPS,
    X_V_MODEL_ON_CONST,
    X_INVALID_EXPRESSION,
    X_KEEP_ALIVE_INVALID_CHILDREN,

    X_PREFIX_ID_NOT_SUPPORTED,
    X_MODULE_MODE_NOT_SUPPORTED,
    X_CACHE_HANDLER_NOT_SUPPORTED,
    X_SCOPE_ID_NOT_SUPPORTED,
    X_VNODE_HOOKS,

    X_V_BIND_INVALID_SAME_NAME_ARGUMENT,

    EXTEND_POINT,
}

/// DOM error codes continue where the core codes stop (54).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
#[allow(non_camel_case_types, dead_code)]
pub enum DomErrorCode {
    X_V_HTML_NO_EXPRESSION = 54,
    X_V_HTML_WITH_CHILDREN,
    X_V_TEXT_NO_EXPRESSION,
    X_V_TEXT_WITH_CHILDREN,
    X_V_MODEL_ON_INVALID_ELEMENT,
    X_V_MODEL_ARG_ON_ELEMENT,
    X_V_MODEL_ON_FILE_INPUT_ELEMENT,
    X_V_MODEL_UNNECESSARY_VALUE,
    X_V_SHOW_NO_EXPRESSION,
    X_TRANSITION_INVALID_CHILDREN,
    X_IGNORED_SIDE_EFFECT_TAG,
}

impl ErrorCode {
    pub fn message(self) -> &'static str {
        use ErrorCode::*;
        match self {
            ABRUPT_CLOSING_OF_EMPTY_COMMENT => "Illegal comment.",
            CDATA_IN_HTML_CONTENT => "CDATA section is allowed only in XML context.",
            DUPLICATE_ATTRIBUTE => "Duplicate attribute.",
            END_TAG_WITH_ATTRIBUTES => "End tag cannot have attributes.",
            END_TAG_WITH_TRAILING_SOLIDUS => "Illegal '/' in tags.",
            EOF_BEFORE_TAG_NAME => "Unexpected EOF in tag.",
            EOF_IN_CDATA => "Unexpected EOF in CDATA section.",
            EOF_IN_COMMENT => "Unexpected EOF in comment.",
            EOF_IN_SCRIPT_HTML_COMMENT_LIKE_TEXT => "Unexpected EOF in script.",
            EOF_IN_TAG => "Unexpected EOF in tag.",
            INCORRECTLY_CLOSED_COMMENT => "Incorrectly closed comment.",
            INCORRECTLY_OPENED_COMMENT => "Incorrectly opened comment.",
            INVALID_FIRST_CHARACTER_OF_TAG_NAME => "Illegal tag name. Use '&lt;' to print '<'.",
            MISSING_ATTRIBUTE_VALUE => "Attribute value was expected.",
            MISSING_END_TAG_NAME => "End tag name was expected.",
            MISSING_WHITESPACE_BETWEEN_ATTRIBUTES => "Whitespace was expected.",
            NESTED_COMMENT => "Unexpected '<!--' in comment.",
            UNEXPECTED_CHARACTER_IN_ATTRIBUTE_NAME => {
                "Attribute name cannot contain U+0022 (\"), U+0027 ('), and U+003C (<)."
            }
            UNEXPECTED_CHARACTER_IN_UNQUOTED_ATTRIBUTE_VALUE => {
                "Unquoted attribute value cannot contain U+0022 (\"), U+0027 ('), U+003C (<), U+003D (=), and U+0060 (`)."
            }
            UNEXPECTED_EQUALS_SIGN_BEFORE_ATTRIBUTE_NAME => "Attribute name cannot start with '='.",
            UNEXPECTED_QUESTION_MARK_INSTEAD_OF_TAG_NAME => "'<?' is allowed only in XML context.",
            UNEXPECTED_NULL_CHARACTER => "Unexpected null character.",
            UNEXPECTED_SOLIDUS_IN_TAG => "Illegal '/' in tags.",

            X_INVALID_END_TAG => "Invalid end tag.",
            X_MISSING_END_TAG => "Element is missing end tag.",
            X_MISSING_INTERPOLATION_END => "Interpolation end sign was not found.",
            X_MISSING_DYNAMIC_DIRECTIVE_ARGUMENT_END => {
                "End bracket for dynamic directive argument was not found. Note that dynamic directive argument cannot contain spaces."
            }
            X_MISSING_DIRECTIVE_NAME => "Legal directive name was expected.",

            X_V_IF_NO_EXPRESSION => "v-if/v-else-if is missing expression.",
            X_V_IF_SAME_KEY => "v-if/else branches must use unique keys.",
            X_V_ELSE_NO_ADJACENT_IF => "v-else/v-else-if has no adjacent v-if or v-else-if.",
            X_V_FOR_NO_EXPRESSION => "v-for is missing expression.",
            X_V_FOR_MALFORMED_EXPRESSION => "v-for has invalid expression.",
            X_V_FOR_TEMPLATE_KEY_PLACEMENT => {
                "<template v-for> key should be placed on the <template> tag."
            }
            X_V_BIND_NO_EXPRESSION => "v-bind is missing expression.",
            X_V_BIND_INVALID_SAME_NAME_ARGUMENT => {
                "v-bind with same-name shorthand only allows static argument."
            }
            X_V_ON_NO_EXPRESSION => "v-on is missing expression.",
            X_V_SLOT_UNEXPECTED_DIRECTIVE_ON_SLOT_OUTLET => {
                "Unexpected custom directive on <slot> outlet."
            }
            X_V_SLOT_MIXED_SLOT_USAGE => {
                "Mixed v-slot usage on both the component and nested <template>. When there are multiple named slots, all slots should use <template> syntax to avoid scope ambiguity."
            }
            X_V_SLOT_DUPLICATE_SLOT_NAMES => "Duplicate slot names found. ",
            X_V_SLOT_EXTRANEOUS_DEFAULT_SLOT_CHILDREN => {
                "Extraneous children found when component already has explicitly named default slot. These children will be ignored."
            }
            X_V_SLOT_MISPLACED => "v-slot can only be used on components or <template> tags.",
            X_V_MODEL_NO_EXPRESSION => "v-model is missing expression.",
            X_V_MODEL_MALFORMED_EXPRESSION => {
                "v-model value must be a valid JavaScript member expression."
            }
            X_V_MODEL_ON_SCOPE_VARIABLE => {
                "v-model cannot be used on v-for or v-slot scope variables because they are not writable."
            }
            X_V_MODEL_ON_PROPS => {
                "v-model cannot be used on a prop, because local prop bindings are not writable.\nUse a v-bind binding combined with a v-on listener that emits update:x event instead."
            }
            X_V_MODEL_ON_CONST => "v-model cannot be used on a const binding because it is not writable.",
            X_INVALID_EXPRESSION => "Error parsing JavaScript expression: ",
            X_KEEP_ALIVE_INVALID_CHILDREN => "<KeepAlive> expects exactly one child component.",

            X_PREFIX_ID_NOT_SUPPORTED => {
                "\"prefixIdentifiers\" option is not supported in this build of compiler."
            }
            X_MODULE_MODE_NOT_SUPPORTED => {
                "ES module mode is not supported in this build of compiler."
            }
            X_CACHE_HANDLER_NOT_SUPPORTED => {
                "\"cacheHandlers\" option is only supported when the \"prefixIdentifiers\" option is enabled."
            }
            X_SCOPE_ID_NOT_SUPPORTED => "\"scopeId\" option is only supported in module mode.",
            X_VNODE_HOOKS => {
                "@vnode-* hooks in templates are no longer supported. Use the vue: prefix instead. For example, @vnode-mounted should be changed to @vue:mounted. @vnode-* hooks support has been removed in 3.4."
            }
            EXTEND_POINT => "",
        }
    }
}

impl DomErrorCode {
    pub fn message(self) -> &'static str {
        use DomErrorCode::*;
        match self {
            X_V_HTML_NO_EXPRESSION => "v-html is missing expression.",
            X_V_HTML_WITH_CHILDREN => "v-html will override element children.",
            X_V_TEXT_NO_EXPRESSION => "v-text is missing expression.",
            X_V_TEXT_WITH_CHILDREN => "v-text will override element children.",
            X_V_MODEL_ON_INVALID_ELEMENT => {
                "v-model can only be used on <input>, <textarea> and <select> elements."
            }
            X_V_MODEL_ARG_ON_ELEMENT => "v-model argument is not supported on plain elements.",
            X_V_MODEL_ON_FILE_INPUT_ELEMENT => {
                "v-model cannot be used on file inputs since they are read-only. Use a v-on:change listener instead."
            }
            X_V_MODEL_UNNECESSARY_VALUE => {
                "Unnecessary value binding used alongside v-model. It will interfere with v-model's behavior."
            }
            X_V_SHOW_NO_EXPRESSION => "v-show is missing expression.",
            X_TRANSITION_INVALID_CHILDREN => {
                "<Transition> expects exactly one child element or component."
            }
            X_IGNORED_SIDE_EFFECT_TAG => {
                "Tags with side effect (<script> and <style>) are ignored in client component templates."
            }
        }
    }
}

/// Mirrors the `SyntaxError` instances the JS compiler throws / collects.
#[derive(Debug, Clone)]
pub struct CompilerError {
    pub code: i32,
    pub message: String,
    pub loc: Option<SourceLocation>,
}

pub fn create_compiler_error(
    code: ErrorCode,
    loc: Option<SourceLocation>,
    additional: Option<&str>,
) -> CompilerError {
    CompilerError {
        code: code as i32,
        message: format!("{}{}", code.message(), additional.unwrap_or("")),
        loc,
    }
}

pub fn create_dom_compiler_error(code: DomErrorCode, loc: Option<SourceLocation>) -> CompilerError {
    CompilerError {
        code: code as i32,
        message: code.message().to_string(),
        loc,
    }
}
