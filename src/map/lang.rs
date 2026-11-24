use std::path::Path;

/// A source language with structured extraction (symbols, imports, calls).
///
/// The single list of extracted file extensions: every per-language branch in
/// scanning, extraction, and import resolution matches on this enum, so adding
/// a variant makes the compiler list each site the new language must handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Go,
    CSharp,
    /// Line-based extraction in `vb6.rs`; there is no usable tree-sitter grammar.
    Vb6,
}

impl Lang {
    #[must_use]
    pub fn from_ext(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Self::Rust),
            "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "py" => Some(Self::Python),
            "go" => Some(Self::Go),
            "cs" => Some(Self::CSharp),
            "frm" | "cls" | "bas" | "dob" | "ctl" => Some(Self::Vb6),
            _ => None,
        }
    }

    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|e| e.to_str())
            .and_then(Self::from_ext)
    }

    #[must_use]
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::TypeScript | Self::Tsx => "TypeScript",
            Self::JavaScript => "JavaScript",
            Self::Python => "Python",
            Self::Go => "Go",
            Self::CSharp => "C#",
            Self::Vb6 => "VB6",
        }
    }

    pub(crate) fn is_js_family(self) -> bool {
        matches!(self, Self::TypeScript | Self::Tsx | Self::JavaScript)
    }

    /// Parse `content` with this language's tree-sitter grammar. `None` for VB6,
    /// which has no grammar, so the tree-sitter extraction paths yield nothing.
    pub(crate) fn parse(self, content: &str) -> Option<tree_sitter::Tree> {
        let grammar = match self {
            Self::Rust => tree_sitter_rust::LANGUAGE,
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX,
            Self::JavaScript => tree_sitter_javascript::LANGUAGE,
            Self::Python => tree_sitter_python::LANGUAGE,
            Self::Go => tree_sitter_go::LANGUAGE,
            Self::CSharp => tree_sitter_c_sharp::LANGUAGE,
            Self::Vb6 => return None,
        };
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&grammar.into()).ok()?;
        parser.parse(content, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_family_extensions_share_one_language() {
        for ext in ["js", "jsx", "mjs", "cjs"] {
            assert_eq!(Lang::from_ext(ext), Some(Lang::JavaScript));
        }
        assert_eq!(Lang::from_ext("tsx"), Some(Lang::Tsx));
    }

    #[test]
    fn unextracted_extension_has_no_language() {
        assert_eq!(Lang::from_ext("rb"), None);
        assert_eq!(Lang::from_path(Path::new("Makefile")), None);
    }

    #[test]
    fn every_language_parses_empty_source() {
        for lang in [
            Lang::Rust,
            Lang::TypeScript,
            Lang::Tsx,
            Lang::JavaScript,
            Lang::Python,
            Lang::Go,
            Lang::CSharp,
        ] {
            assert!(lang.parse("").is_some(), "{lang:?}");
        }
    }
}
