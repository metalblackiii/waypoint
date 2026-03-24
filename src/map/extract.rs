use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

/// Compile a regex from a string literal. Panics on invalid syntax.
#[allow(clippy::unwrap_used)]
fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}

/// Collect the first capture group from regex matches, format as "label: a, b, c" or return fallback.
fn regex_collect(re: &Regex, content: &str, limit: usize, label: &str, fallback: &str) -> String {
    let items: Vec<&str> = re
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
        .take(limit)
        .collect();
    if items.is_empty() {
        fallback.to_string()
    } else if label.is_empty() {
        items.join(", ")
    } else {
        format!("{label}: {}", items.join(", "))
    }
}

/// Extract a one-line description for a file based on its content.
/// Uses tree-sitter for supported languages, regex fallback for others.
#[must_use]
pub fn extract_description(path: &Path, content: &str) -> String {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    if let Some(desc) = known_file_description(filename) {
        return desc;
    }

    if let Some(desc) = tree_sitter_extract(ext, content)
        && !desc.is_empty()
    {
        return desc;
    }

    regex_extract(ext, content, filename)
}

fn known_file_description(filename: &str) -> Option<String> {
    let desc = match filename {
        "package.json" => "npm package manifest",
        "package-lock.json" => "npm dependency lock",
        "tsconfig.json" => "TypeScript compiler configuration",
        "tsconfig.build.json" => "TypeScript build configuration",
        ".eslintrc.json" | ".eslintrc.js" | ".eslintrc.cjs" | "eslint.config.js"
        | "eslint.config.mjs" => "ESLint configuration",
        ".prettierrc" | ".prettierrc.json" | "prettier.config.js" => "Prettier configuration",
        "Cargo.toml" => "Rust package manifest",
        "Cargo.lock" => "Rust dependency lock",
        "Dockerfile" | "Dockerfile.dev" | "Dockerfile.prod" => "Container build instructions",
        "docker-compose.yml" | "docker-compose.yaml" | "compose.yml" | "compose.yaml" => {
            "Docker Compose service definitions"
        }
        "Makefile" => "Make build recipes",
        "Justfile" | "justfile" => "Just command recipes",
        ".gitignore" => "Git ignore patterns",
        ".gitattributes" => "Git attributes",
        ".editorconfig" => "Editor configuration",
        "jest.config.ts" | "jest.config.js" => "Jest test configuration",
        "vitest.config.ts" | "vitest.config.js" => "Vitest configuration",
        "webpack.config.js" | "webpack.config.ts" => "Webpack bundler configuration",
        "vite.config.ts" | "vite.config.js" => "Vite bundler configuration",
        "rollup.config.js" | "rollup.config.ts" => "Rollup bundler configuration",
        "babel.config.js" | ".babelrc" => "Babel transpiler configuration",
        "tailwind.config.js" | "tailwind.config.ts" => "Tailwind CSS configuration",
        "postcss.config.js" => "PostCSS configuration",
        ".env.example" => "Environment variable template",
        "LICENSE" | "LICENSE.md" => "License",
        "CHANGELOG.md" | "CHANGELOG" => "Changelog",
        "README.md" | "README" => "Project documentation",
        "go.mod" => "Go module definition",
        "go.sum" => "Go dependency checksums",
        "requirements.txt" => "Python dependency list",
        "pyproject.toml" => "Python project configuration",
        "setup.py" | "setup.cfg" => "Python package setup",
        "Gemfile" => "Ruby gem dependencies",
        "Brewfile" => "Homebrew bundle dependencies",
        _ => return None,
    };
    Some(desc.to_string())
}

// ---------------------------------------------------------------------------
// Tree-sitter extraction
// ---------------------------------------------------------------------------

const COLLECTION_CAP: usize = 30;
const DISPLAY_BUDGET: usize = 8;

struct Declaration {
    name: String,
    text: String,
    exported: bool,
}

fn tree_sitter_extract(ext: &str, content: &str) -> Option<String> {
    let language = match ext {
        "rs" => tree_sitter_rust::LANGUAGE,
        "ts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX,
        "js" | "jsx" | "mjs" | "cjs" => tree_sitter_javascript::LANGUAGE,
        "py" => tree_sitter_python::LANGUAGE,
        "go" => tree_sitter_go::LANGUAGE,
        _ => return None,
    };

    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language.into()).ok()?;
    let tree = parser.parse(content, None)?;
    let root = tree.root_node();

    let is_js = matches!(ext, "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs");

    // Pass 1: collect export names from `export { ... }` clauses and
    // `module.exports = { ... }` / `module.exports = Identifier` patterns.
    let mut export_names: HashSet<String> = HashSet::new();
    if is_js {
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            collect_export_names(child, content, &mut export_names);
        }
    }

    // Pass 2: collect declarations from top-level nodes.
    let mut declarations: Vec<Declaration> = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if let Some(decl) = extract_declaration(child, ext, content) {
            declarations.push(decl);
            if declarations.len() >= COLLECTION_CAP {
                break;
            }
        }
    }

    // Mark declarations whose names appear in export clauses / module.exports.
    for decl in &mut declarations {
        if !decl.exported && export_names.contains(&decl.name) {
            decl.exported = true;
            decl.text = format!("export {}", decl.text);
        }
    }

    // Stable sort: exports first, preserve source order within each group.
    declarations.sort_by_key(|d| !d.exported);

    if declarations.is_empty() {
        return None;
    }

    let result: Vec<&str> = declarations
        .iter()
        .take(DISPLAY_BUDGET)
        .map(|d| d.text.as_str())
        .collect();

    Some(result.join(", "))
}

fn extract_declaration(node: tree_sitter::Node, ext: &str, source: &str) -> Option<Declaration> {
    let kind = node.kind();
    match ext {
        "rs" => extract_rust(node, kind, source),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => extract_js(node, kind, source),
        "py" => extract_python(node, kind, source),
        "go" => extract_go(node, kind, source),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Rust
// ---------------------------------------------------------------------------

fn extract_rust(node: tree_sitter::Node, kind: &str, source: &str) -> Option<Declaration> {
    let is_pub = has_child_kind(node, "visibility_modifier");
    let prefix = if is_pub { "pub " } else { "" };

    match kind {
        "function_item" => {
            let name = child_text(node, "identifier", source)?;
            Some(Declaration {
                name: name.clone(),
                text: format!("{prefix}fn {name}()"),
                exported: is_pub,
            })
        }
        "struct_item" => {
            let name = child_text(node, "type_identifier", source)?;
            Some(Declaration {
                name: name.clone(),
                text: format!("{prefix}struct {name}"),
                exported: is_pub,
            })
        }
        "enum_item" => {
            let name = child_text(node, "type_identifier", source)?;
            Some(Declaration {
                name: name.clone(),
                text: format!("{prefix}enum {name}"),
                exported: is_pub,
            })
        }
        "trait_item" => {
            let name = child_text(node, "type_identifier", source)?;
            Some(Declaration {
                name: name.clone(),
                text: format!("{prefix}trait {name}"),
                exported: is_pub,
            })
        }
        "impl_item" => {
            let name = child_text(node, "type_identifier", source)?;
            Some(Declaration {
                name: name.clone(),
                text: format!("impl {name}"),
                exported: false,
            })
        }
        "mod_item" => {
            let name = child_text(node, "identifier", source)?;
            Some(Declaration {
                name: name.clone(),
                text: format!("{prefix}mod {name}"),
                exported: is_pub,
            })
        }
        "const_item" => {
            let name = child_text(node, "identifier", source)?;
            Some(Declaration {
                name: name.clone(),
                text: format!("{prefix}const {name}"),
                exported: is_pub,
            })
        }
        "static_item" => {
            let name = child_text(node, "identifier", source)?;
            Some(Declaration {
                name: name.clone(),
                text: format!("{prefix}static {name}"),
                exported: is_pub,
            })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// JavaScript / TypeScript
// ---------------------------------------------------------------------------

fn extract_js(node: tree_sitter::Node, kind: &str, source: &str) -> Option<Declaration> {
    match kind {
        "function_declaration" => {
            let name = child_text(node, "identifier", source)?;
            Some(Declaration {
                name: name.clone(),
                text: format!("function {name}()"),
                exported: false,
            })
        }
        "class_declaration" => {
            let name = child_text(node, "identifier", source)?;
            let full_text = node_text(node, source);
            let label = if full_text.contains("HTMLElement")
                || full_text.contains("LitElement")
                || full_text.contains("customElement")
            {
                format!("class {name} (web component)")
            } else {
                format!("class {name}")
            };
            Some(Declaration {
                name: name.clone(),
                text: label,
                exported: false,
            })
        }
        "export_statement" => extract_js_export(node, source),
        "lexical_declaration" | "variable_declaration" => extract_js_lexical(node, source),
        "expression_statement" => extract_js_commonjs(node, source),
        "interface_declaration" | "type_alias_declaration" => {
            let name = child_text(node, "type_identifier", source)
                .or_else(|| child_text(node, "identifier", source))?;
            Some(Declaration {
                name: name.clone(),
                text: format!("type {name}"),
                exported: false,
            })
        }
        "enum_declaration" => {
            let name = child_text(node, "identifier", source)?;
            Some(Declaration {
                name: name.clone(),
                text: format!("enum {name}"),
                exported: false,
            })
        }
        _ => None,
    }
}

/// Handle `export <declaration>`, `export default <expr>`, and
/// `export { ... }` (the last is skipped — names collected in pass 1).
fn extract_js_export(node: tree_sitter::Node, source: &str) -> Option<Declaration> {
    let text = node_text(node, source);
    let is_default = text.starts_with("export default");
    let prefix = if is_default {
        "export default "
    } else {
        "export "
    };

    let mut cursor = node.walk();

    // Try to find a wrapped declaration (export function/class/const/type).
    for child in node.children(&mut cursor) {
        if let Some(mut decl) = extract_js(child, child.kind(), source) {
            decl.text = format!("{prefix}{}", decl.text);
            decl.exported = true;
            return Some(decl);
        }
    }

    // Bare default export without a named declaration child.
    if is_default {
        return Some(describe_default_export(node, source));
    }

    // `export { ... }` handled in pass 1 — skip here.
    None
}

/// Produce a richer description for `export default <expression>`.
fn describe_default_export(export_node: tree_sitter::Node, source: &str) -> Declaration {
    let mut cursor = export_node.walk();
    for child in export_node.children(&mut cursor) {
        match child.kind() {
            "arrow_function" | "function_expression" => {
                let name = child_text(child, "identifier", source).unwrap_or_default();
                return if name.is_empty() {
                    Declaration {
                        name: String::new(),
                        text: "export default function".to_string(),
                        exported: true,
                    }
                } else {
                    Declaration {
                        name: name.clone(),
                        text: format!("export default function {name}()"),
                        exported: true,
                    }
                };
            }
            "class" | "class_declaration" => {
                let name = child_text(child, "identifier", source).unwrap_or_default();
                let full_text = node_text(child, source);
                let wc = full_text.contains("HTMLElement")
                    || full_text.contains("LitElement")
                    || full_text.contains("customElement");
                let suffix = if wc { " (web component)" } else { "" };
                return if name.is_empty() {
                    Declaration {
                        name: String::new(),
                        text: format!("export default class{suffix}"),
                        exported: true,
                    }
                } else {
                    Declaration {
                        name: name.clone(),
                        text: format!("export default class {name}{suffix}"),
                        exported: true,
                    }
                };
            }
            "identifier" => {
                let name = node_text(child, source).to_string();
                return Declaration {
                    name: name.clone(),
                    text: format!("export default {name}"),
                    exported: true,
                };
            }
            "call_expression" => {
                if let Some(func) = child.child_by_field_name("function") {
                    let func_name = node_text(func, source);
                    return Declaration {
                        name: String::new(),
                        text: format!("export default {func_name}(...)"),
                        exported: true,
                    };
                }
            }
            "object" => {
                return Declaration {
                    name: String::new(),
                    text: "export default {...}".to_string(),
                    exported: true,
                };
            }
            _ => {}
        }
    }
    Declaration {
        name: String::new(),
        text: "export default".to_string(),
        exported: true,
    }
}

/// Handle `const`/`let`/`var` declarations.
/// Filters out `require()` imports and uses tree-sitter node types for
/// accurate function detection (no false-positive `()` on objects).
fn extract_js_lexical(node: tree_sitter::Node, source: &str) -> Option<Declaration> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() != "variable_declarator" {
            continue;
        }
        let name = child_text(child, "identifier", source)?;

        if let Some(value) = child.child_by_field_name("value") {
            // Skip require() imports (direct or chained like require('x').Foo).
            if is_require_call(value, source) {
                return None;
            }

            let is_fn = matches!(value.kind(), "arrow_function" | "function_expression");
            let text = if is_fn {
                format!("{name}()")
            } else {
                name.clone()
            };
            return Some(Declaration {
                name,
                text,
                exported: false,
            });
        }

        return Some(Declaration {
            name: name.clone(),
            text: name,
            exported: false,
        });
    }
    None
}

/// Handle `CommonJS` exports:
/// - `module.exports = { a, b }` → handled in pass 1 (names collected), skip here
/// - `module.exports = <identifier>` → handled in pass 1, skip here
/// - `module.exports = function/class/arrow` → create Declaration
/// - `exports.X = ...` → create Declaration
fn extract_js_commonjs(node: tree_sitter::Node, source: &str) -> Option<Declaration> {
    let mut cursor = node.walk();
    let assignment = node
        .children(&mut cursor)
        .find(|c| c.kind() == "assignment_expression")?;

    let left = assignment.child_by_field_name("left")?;
    if left.kind() != "member_expression" {
        return None;
    }

    let obj = left.child_by_field_name("object")?;
    let prop = left.child_by_field_name("property")?;
    let obj_text = node_text(obj, source);
    let prop_text = node_text(prop, source);

    if obj_text == "module" && prop_text == "exports" {
        let right = assignment.child_by_field_name("right")?;
        match right.kind() {
            // Object and identifier cases handled in pass 1 via export_names.
            "object" | "identifier" => None,
            "function_expression" | "arrow_function" => Some(Declaration {
                name: String::new(),
                text: "export default function".to_string(),
                exported: true,
            }),
            "class" => {
                let name = child_text(right, "identifier", source).unwrap_or_default();
                let text = if name.is_empty() {
                    "export default class".to_string()
                } else {
                    format!("export default class {name}")
                };
                Some(Declaration {
                    name,
                    text,
                    exported: true,
                })
            }
            _ => Some(Declaration {
                name: String::new(),
                text: "export default".to_string(),
                exported: true,
            }),
        }
    } else if obj_text == "exports" {
        let right = assignment.child_by_field_name("right")?;
        let is_fn = matches!(right.kind(), "function_expression" | "arrow_function");
        let text = if is_fn {
            format!("export {prop_text}()")
        } else {
            format!("export {prop_text}")
        };
        Some(Declaration {
            name: prop_text.to_string(),
            text,
            exported: true,
        })
    } else {
        None
    }
}

/// Collect export names from `export { a, b }` clauses and `CommonJS`
/// `module.exports = { a, b }` / `module.exports = Identifier`.
fn collect_export_names(node: tree_sitter::Node, source: &str, names: &mut HashSet<String>) {
    match node.kind() {
        "export_statement" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "export_clause" {
                    let mut clause_cursor = child.walk();
                    for spec in child.children(&mut clause_cursor) {
                        if spec.kind() == "export_specifier" {
                            // Use the `name` field (local identifier) for matching.
                            if let Some(name_node) = spec.child_by_field_name("name") {
                                names.insert(node_text(name_node, source).to_string());
                            }
                        }
                    }
                }
            }
        }
        "expression_statement" => {
            collect_cjs_export_names(node, source, names);
        }
        _ => {}
    }
}

/// Extract names from `module.exports = { a, b }` or `module.exports = X`.
fn collect_cjs_export_names(
    expr_stmt: tree_sitter::Node,
    source: &str,
    names: &mut HashSet<String>,
) {
    let mut cursor = expr_stmt.walk();
    let Some(assignment) = expr_stmt
        .children(&mut cursor)
        .find(|c| c.kind() == "assignment_expression")
    else {
        return;
    };

    let Some(left) = assignment.child_by_field_name("left") else {
        return;
    };
    if left.kind() != "member_expression" {
        return;
    }

    let Some(obj) = left.child_by_field_name("object") else {
        return;
    };
    let Some(prop) = left.child_by_field_name("property") else {
        return;
    };
    if node_text(obj, source) != "module" || node_text(prop, source) != "exports" {
        return;
    }

    let Some(right) = assignment.child_by_field_name("right") else {
        return;
    };

    match right.kind() {
        "object" => {
            let mut obj_cursor = right.walk();
            for child in right.children(&mut obj_cursor) {
                match child.kind() {
                    "shorthand_property_identifier" => {
                        names.insert(node_text(child, source).to_string());
                    }
                    "pair" => {
                        // { save: saveImage } → use value (local name) for matching.
                        if let Some(value) = child.child_by_field_name("value")
                            && value.kind() == "identifier"
                        {
                            names.insert(node_text(value, source).to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
        "identifier" => {
            names.insert(node_text(right, source).to_string());
        }
        _ => {}
    }
}

/// Detect `require()` calls, including chained: `require('x').Foo`.
fn is_require_call(node: tree_sitter::Node, source: &str) -> bool {
    match node.kind() {
        "call_expression" => node
            .child_by_field_name("function")
            .is_some_and(|func| node_text(func, source) == "require"),
        "member_expression" => node
            .child_by_field_name("object")
            .is_some_and(|obj| is_require_call(obj, source)),
        "await_expression" => {
            let mut cursor = node.walk();
            node.children(&mut cursor)
                .any(|c| is_require_call(c, source))
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Python
// ---------------------------------------------------------------------------

fn extract_python(node: tree_sitter::Node, kind: &str, source: &str) -> Option<Declaration> {
    match kind {
        "function_definition" => {
            let name = child_text(node, "identifier", source)?;
            let exported = !name.starts_with('_');
            Some(Declaration {
                name: name.clone(),
                text: format!("def {name}()"),
                exported,
            })
        }
        "class_definition" => {
            let name = child_text(node, "identifier", source)?;
            let exported = !name.starts_with('_');
            Some(Declaration {
                name: name.clone(),
                text: format!("class {name}"),
                exported,
            })
        }
        "decorated_definition" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "function_definition" || child.kind() == "class_definition" {
                    return extract_python(child, child.kind(), source);
                }
            }
            None
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Go
// ---------------------------------------------------------------------------

fn extract_go(node: tree_sitter::Node, kind: &str, source: &str) -> Option<Declaration> {
    match kind {
        "function_declaration" => {
            let name = child_text(node, "identifier", source)?;
            let exported = name.starts_with(|c: char| c.is_uppercase());
            Some(Declaration {
                name: name.clone(),
                text: format!("func {name}()"),
                exported,
            })
        }
        "method_declaration" => {
            let name = child_text(node, "field_identifier", source)?;
            let exported = name.starts_with(|c: char| c.is_uppercase());
            Some(Declaration {
                name: name.clone(),
                text: format!("func {name}()"),
                exported,
            })
        }
        "type_declaration" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "type_spec"
                    && let Some(name) = child_text(child, "type_identifier", source)
                {
                    let exported = name.starts_with(|c: char| c.is_uppercase());
                    return Some(Declaration {
                        name: name.clone(),
                        text: format!("type {name}"),
                        exported,
                    });
                }
            }
            None
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn child_text(node: tree_sitter::Node, child_kind: &str, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == child_kind {
            return Some(node_text(child, source).to_string());
        }
    }
    None
}

fn node_text<'a>(node: tree_sitter::Node, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

fn has_child_kind(node: tree_sitter::Node, kind: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|c| c.kind() == kind)
}

// ---------------------------------------------------------------------------
// Regex fallback extraction
// ---------------------------------------------------------------------------

fn regex_extract(ext: &str, content: &str, filename: &str) -> String {
    match ext {
        "sh" | "bash" | "zsh" | "fish" => extract_shell(content),
        "md" | "mdx" => extract_markdown(content),
        "html" | "htm" => extract_html(content),
        "css" | "scss" | "sass" | "less" => extract_css(content),
        "sql" => extract_sql(content),
        "yaml" | "yml" => extract_yaml(content, filename),
        "json" => extract_json(filename),
        "graphql" | "gql" => extract_graphql(content),
        "tf" | "tfvars" | "hcl" => extract_terraform(content),
        "xml" => extract_xml(content),
        "vue" => extract_vue(content),
        "svelte" => extract_svelte(content),
        "rb" => extract_ruby(content),
        "java" | "kt" => extract_jvm(content),
        "c" | "cpp" | "h" | "hpp" => extract_c(content),
        "swift" => extract_swift(content),
        "proto" => extract_proto(content),
        _ => format!("{ext} source file"),
    }
}

fn extract_shell(content: &str) -> String {
    // Shell functions can match in group 1 or group 2, so we can't use regex_collect directly
    static RE: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^(?:function\s+(\w+)|(\w+)\s*\(\))"));
    let funcs: Vec<&str> = RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).or(cap.get(2)).map(|m| m.as_str()))
        .take(5)
        .collect();
    if funcs.is_empty() {
        "shell script".to_string()
    } else {
        format!("shell: {}", funcs.join(", "))
    }
}

fn extract_markdown(content: &str) -> String {
    for line in content.lines() {
        if let Some(heading) = line.strip_prefix("# ") {
            return heading.trim().to_string();
        }
    }
    "markdown document".to_string()
}

fn extract_html(content: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| re(r"<title>([^<]+)</title>"));
    if let Some(cap) = RE.captures(content) {
        return cap[1].trim().to_string();
    }
    "HTML document".to_string()
}

fn extract_css(content: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^([.#]?[\w-]+)\s*\{"));
    regex_collect(&RE, content, 5, "styles", "stylesheet")
}

fn extract_sql(content: &str) -> String {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| re(r#"(?mi)CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?[`"]?(\w+)"#));
    regex_collect(&RE, content, 5, "tables", "SQL script")
}

fn extract_yaml(content: &str, filename: &str) -> String {
    static SVC_RE: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^\s{2}(\w[\w-]+):$"));
    static NAME_RE: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^name:\s*(.+)"));

    let lower = filename.to_lowercase();

    if lower.contains("docker-compose") || lower.contains("compose") {
        let services: Vec<&str> = SVC_RE
            .captures_iter(content)
            .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
            .take(5)
            .collect();
        if !services.is_empty() {
            return format!("compose services: {}", services.join(", "));
        }
    }

    if content.contains("on:") && (content.contains("jobs:") || content.contains("workflow")) {
        if let Some(cap) = NAME_RE.captures(content) {
            return format!("GHA: {}", cap[1].trim());
        }
        return "GitHub Actions workflow".to_string();
    }

    "YAML configuration".to_string()
}

fn extract_json(filename: &str) -> String {
    if filename.ends_with(".schema.json") {
        return "JSON schema".to_string();
    }
    "JSON data".to_string()
}

fn extract_graphql(content: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^type\s+(\w+)"));
    regex_collect(&RE, content, 5, "types", "GraphQL schema")
}

fn extract_terraform(content: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| re(r#"(?m)^resource\s+"(\w+)"\s+"(\w+)""#));
    let resources: Vec<String> = RE
        .captures_iter(content)
        .map(|cap| format!("{}.{}", &cap[1], &cap[2]))
        .take(5)
        .collect();
    if resources.is_empty() {
        "Terraform configuration".to_string()
    } else {
        format!("resources: {}", resources.join(", "))
    }
}

fn extract_xml(content: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| re(r"<(\w+)[\s>]"));
    if let Some(cap) = RE.captures(content) {
        let tag = &cap[1];
        if tag != "xml" {
            return format!("XML: <{tag}>");
        }
    }
    "XML document".to_string()
}

fn extract_vue(content: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| re(r#"(?m)name:\s*['"](\w+)['"]"#));
    if let Some(cap) = RE.captures(content) {
        return format!("Vue: {}", &cap[1]);
    }
    "Vue component".to_string()
}

fn extract_svelte(content: &str) -> String {
    if content.contains("<script") {
        "Svelte component".to_string()
    } else {
        "Svelte template".to_string()
    }
}

fn extract_ruby(content: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^(?:class|module)\s+(\w+)"));
    regex_collect(&RE, content, 3, "", "Ruby source")
}

fn extract_jvm(content: &str) -> String {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| re(r"(?m)^(?:public\s+)?(?:class|interface|enum)\s+(\w+)"));
    regex_collect(&RE, content, 3, "", "JVM source")
}

fn extract_c(content: &str) -> String {
    // Custom filter needed to exclude control-flow keywords that match the function pattern
    static RE: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^\w[\w\s*]+\s+(\w+)\s*\("));
    let funcs: Vec<&str> = RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
        .filter(|n| !matches!(*n, "if" | "for" | "while" | "switch" | "return"))
        .take(5)
        .collect();
    if funcs.is_empty() {
        "C/C++ source".to_string()
    } else {
        funcs.join(", ")
    }
}

fn extract_swift(content: &str) -> String {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| re(r"(?m)^(?:class|struct|enum|protocol)\s+(\w+)"));
    regex_collect(&RE, content, 3, "", "Swift source")
}

fn extract_proto(content: &str) -> String {
    static MSG_RE: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^message\s+(\w+)"));
    static SVC_RE: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^service\s+(\w+)"));
    let messages: Vec<&str> = MSG_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
        .take(3)
        .collect();
    let services: Vec<&str> = SVC_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
        .take(2)
        .collect();

    let mut parts = Vec::new();
    if !services.is_empty() {
        parts.push(format!("services: {}", services.join(", ")));
    }
    if !messages.is_empty() {
        parts.push(format!("messages: {}", messages.join(", ")));
    }
    if parts.is_empty() {
        "Protocol Buffers".to_string()
    } else {
        parts.join("; ")
    }
}

// ---------------------------------------------------------------------------
// Symbol extraction (structured, for sketch and find)
// ---------------------------------------------------------------------------

/// A structured symbol extracted from source code via tree-sitter.
#[derive(Debug, Clone)]
pub struct Symbol {
    /// Relative file path (set by caller, not by extraction).
    pub file_path: String,
    pub name: String,
    pub kind: String,
    /// Declaration signature without body.
    pub signature: String,
    /// 1-based start line.
    pub line_start: i64,
    /// 1-based end line.
    pub line_end: i64,
    pub exported: bool,
}

/// Extract structured symbols from a source file using tree-sitter.
/// Returns an empty vec for unsupported languages or parse failures.
#[must_use]
pub fn extract_symbols(path: &Path, content: &str) -> Vec<Symbol> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    let language = match ext {
        "rs" => tree_sitter_rust::LANGUAGE,
        "ts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX,
        "js" | "jsx" | "mjs" | "cjs" => tree_sitter_javascript::LANGUAGE,
        "py" => tree_sitter_python::LANGUAGE,
        "go" => tree_sitter_go::LANGUAGE,
        _ => return Vec::new(),
    };

    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&language.into()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(content, None) else {
        return Vec::new();
    };

    let mut symbols = Vec::new();
    let root = tree.root_node();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        collect_node_symbol(child, ext, content, &mut symbols);
    }
    symbols
}

fn collect_node_symbol(
    node: tree_sitter::Node,
    ext: &str,
    source: &str,
    symbols: &mut Vec<Symbol>,
) {
    match ext {
        "rs" => collect_rust_symbols(node, source, symbols),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => collect_js_symbols(node, source, symbols),
        "py" => collect_python_symbols(node, source, symbols),
        "go" => collect_go_symbols(node, source, symbols),
        _ => {}
    }
}

/// Extract the declaration signature (everything before the body).
fn symbol_signature(node: tree_sitter::Node, source: &str) -> String {
    let text = node_text(node, source);
    let sig = if let Some(pos) = text.find('{') {
        text[..pos].trim()
    } else if let Some(pos) = text.find(';') {
        text[..pos].trim()
    } else {
        text.lines().next().unwrap_or(text).trim()
    };
    let collapsed: String = sig.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.len() > 200 {
        let mut end = 197;
        while !collapsed.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &collapsed[..end])
    } else {
        collapsed
    }
}

#[allow(clippy::cast_possible_wrap)]
fn build_symbol(
    name: String,
    kind: &str,
    node: tree_sitter::Node,
    source: &str,
    exported: bool,
) -> Symbol {
    Symbol {
        file_path: String::new(),
        name,
        kind: kind.to_string(),
        signature: symbol_signature(node, source),
        line_start: node.start_position().row as i64 + 1,
        line_end: node.end_position().row as i64 + 1,
        exported,
    }
}

fn collect_rust_symbols(node: tree_sitter::Node, source: &str, symbols: &mut Vec<Symbol>) {
    let is_pub = has_child_kind(node, "visibility_modifier");
    match node.kind() {
        "function_item" => {
            if let Some(name) = child_text(node, "identifier", source) {
                symbols.push(build_symbol(name, "fn", node, source, is_pub));
            }
        }
        "struct_item" => {
            if let Some(name) = child_text(node, "type_identifier", source) {
                symbols.push(build_symbol(name, "struct", node, source, is_pub));
            }
        }
        "enum_item" => {
            if let Some(name) = child_text(node, "type_identifier", source) {
                symbols.push(build_symbol(name, "enum", node, source, is_pub));
            }
        }
        "trait_item" => {
            if let Some(name) = child_text(node, "type_identifier", source) {
                symbols.push(build_symbol(name, "trait", node, source, is_pub));
            }
        }
        "impl_item" => {
            // For `impl Type` or `impl Trait for Type`, take the last type child
            // as the concrete type. Handles both simple (`Foo`) and generic (`Foo<T>`)
            // forms — `generic_type` wraps the `type_identifier`.
            let mut cursor = node.walk();
            let impl_type: Option<String> = node
                .children(&mut cursor)
                .filter_map(|c| impl_type_name(c, source))
                .last();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "declaration_list" {
                    collect_impl_methods(child, impl_type.as_deref(), source, symbols);
                }
            }
        }
        "const_item" => {
            if let Some(name) = child_text(node, "identifier", source) {
                symbols.push(build_symbol(name, "const", node, source, is_pub));
            }
        }
        "static_item" => {
            if let Some(name) = child_text(node, "identifier", source) {
                symbols.push(build_symbol(name, "static", node, source, is_pub));
            }
        }
        "mod_item" => {
            if let Some(name) = child_text(node, "identifier", source) {
                symbols.push(build_symbol(name, "mod", node, source, is_pub));
            }
        }
        _ => {}
    }
}

/// Extract the base type name from an impl target node.
/// Handles `Foo`, `Foo<T>`, `path::Foo`, and `path::Foo<T>`.
fn impl_type_name(node: tree_sitter::Node, source: &str) -> Option<String> {
    match node.kind() {
        "type_identifier" => Some(node_text(node, source).to_string()),
        "generic_type" => {
            // `Foo<T>` → direct type_identifier child is `Foo`
            // `path::Foo<T>` → scoped_type_identifier child wraps the path
            child_text(node, "type_identifier", source).or_else(|| {
                let mut cursor = node.walk();
                node.children(&mut cursor)
                    .find(|c| c.kind() == "scoped_type_identifier")
                    .and_then(|c| scoped_type_name(c, source))
            })
        }
        "scoped_type_identifier" => scoped_type_name(node, source),
        _ => None,
    }
}

/// Extract the trailing type name from a scoped type like `path::Foo`.
fn scoped_type_name(node: tree_sitter::Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    // The last type_identifier in the scoped path is the type name
    node.children(&mut cursor)
        .filter(|c| c.kind() == "type_identifier")
        .last()
        .map(|c| node_text(c, source).to_string())
}

fn collect_impl_methods(
    decl_list: tree_sitter::Node,
    impl_type: Option<&str>,
    source: &str,
    symbols: &mut Vec<Symbol>,
) {
    let mut cursor = decl_list.walk();
    for child in decl_list.children(&mut cursor) {
        if child.kind() == "function_item" {
            let is_pub = has_child_kind(child, "visibility_modifier");
            if let Some(method_name) = child_text(child, "identifier", source) {
                let qualified = match impl_type {
                    Some(t) => format!("{t}::{method_name}"),
                    None => method_name,
                };
                symbols.push(build_symbol(qualified, "method", child, source, is_pub));
            }
        }
    }
}

fn collect_js_symbols(node: tree_sitter::Node, source: &str, symbols: &mut Vec<Symbol>) {
    match node.kind() {
        "function_declaration" => {
            if let Some(name) = child_text(node, "identifier", source) {
                symbols.push(build_symbol(name, "fn", node, source, false));
            }
        }
        "class_declaration" => {
            if let Some(name) = child_text(node, "identifier", source) {
                symbols.push(build_symbol(name, "class", node, source, false));
            }
        }
        "export_statement" => {
            collect_js_export_symbols(node, source, symbols);
        }
        "lexical_declaration" | "variable_declaration" => {
            collect_js_var_symbols(node, source, symbols, false);
        }
        "interface_declaration" | "type_alias_declaration" => {
            let name = child_text(node, "type_identifier", source)
                .or_else(|| child_text(node, "identifier", source));
            if let Some(name) = name {
                symbols.push(build_symbol(name, "type", node, source, false));
            }
        }
        "enum_declaration" => {
            if let Some(name) = child_text(node, "identifier", source) {
                symbols.push(build_symbol(name, "enum", node, source, false));
            }
        }
        _ => {}
    }
}

fn collect_js_export_symbols(node: tree_sitter::Node, source: &str, symbols: &mut Vec<Symbol>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_declaration" => {
                if let Some(name) = child_text(child, "identifier", source) {
                    symbols.push(build_symbol(name, "fn", child, source, true));
                }
                return;
            }
            "class_declaration" => {
                if let Some(name) = child_text(child, "identifier", source) {
                    symbols.push(build_symbol(name, "class", child, source, true));
                }
                return;
            }
            "lexical_declaration" | "variable_declaration" => {
                collect_js_var_symbols(child, source, symbols, true);
                return;
            }
            "interface_declaration" | "type_alias_declaration" => {
                let name = child_text(child, "type_identifier", source)
                    .or_else(|| child_text(child, "identifier", source));
                if let Some(name) = name {
                    symbols.push(build_symbol(name, "type", child, source, true));
                }
                return;
            }
            "enum_declaration" => {
                if let Some(name) = child_text(child, "identifier", source) {
                    symbols.push(build_symbol(name, "enum", child, source, true));
                }
                return;
            }
            _ => {}
        }
    }
}

fn collect_js_var_symbols(
    node: tree_sitter::Node,
    source: &str,
    symbols: &mut Vec<Symbol>,
    exported: bool,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() != "variable_declarator" {
            continue;
        }
        let Some(name) = child_text(child, "identifier", source) else {
            continue;
        };
        if let Some(value) = child.child_by_field_name("value") {
            if is_require_call(value, source) {
                continue;
            }
            let kind = if matches!(value.kind(), "arrow_function" | "function_expression") {
                "fn"
            } else {
                "const"
            };
            symbols.push(build_symbol(name, kind, node, source, exported));
        } else {
            symbols.push(build_symbol(name, "const", node, source, exported));
        }
    }
}

fn collect_python_symbols(node: tree_sitter::Node, source: &str, symbols: &mut Vec<Symbol>) {
    match node.kind() {
        "function_definition" => {
            if let Some(name) = child_text(node, "identifier", source) {
                let exported = !name.starts_with('_');
                symbols.push(build_symbol(name, "fn", node, source, exported));
            }
        }
        "class_definition" => {
            if let Some(name) = child_text(node, "identifier", source) {
                let exported = !name.starts_with('_');
                symbols.push(build_symbol(name, "class", node, source, exported));
            }
        }
        "decorated_definition" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if matches!(child.kind(), "function_definition" | "class_definition") {
                    collect_python_symbols(child, source, symbols);
                    return;
                }
            }
        }
        _ => {}
    }
}

fn collect_go_symbols(node: tree_sitter::Node, source: &str, symbols: &mut Vec<Symbol>) {
    match node.kind() {
        "function_declaration" => {
            if let Some(name) = child_text(node, "identifier", source) {
                let exported = name.starts_with(|c: char| c.is_uppercase());
                symbols.push(build_symbol(name, "fn", node, source, exported));
            }
        }
        "method_declaration" => {
            if let Some(name) = child_text(node, "field_identifier", source) {
                let exported = name.starts_with(|c: char| c.is_uppercase());
                symbols.push(build_symbol(name, "method", node, source, exported));
            }
        }
        "type_declaration" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "type_spec"
                    && let Some(name) = child_text(child, "type_identifier", source)
                {
                    let exported = name.starts_with(|c: char| c.is_uppercase());
                    symbols.push(build_symbol(name, "type", node, source, exported));
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // -- Existing tests (preserved) --

    #[test]
    fn known_files() {
        assert_eq!(
            extract_description(Path::new("package.json"), "{}"),
            "npm package manifest"
        );
        assert_eq!(
            extract_description(Path::new("Cargo.toml"), "[package]"),
            "Rust package manifest"
        );
    }

    #[test]
    fn rust_extraction() {
        let src = r#"
pub fn main() {}
pub struct Config {}
enum State {}
"#;
        let desc = extract_description(Path::new("lib.rs"), src);
        assert!(desc.contains("fn main()"), "got: {desc}");
        assert!(desc.contains("struct Config"), "got: {desc}");
    }

    #[test]
    fn markdown_heading() {
        let src = "# My Awesome Project\n\nSome text.";
        let desc = extract_description(Path::new("README.md"), src);
        assert_eq!(desc, "Project documentation");
    }

    #[test]
    fn shell_functions() {
        let src = "#!/bin/bash\nsetup() {\n  echo hi\n}\nfunction teardown {\n  echo bye\n}";
        let desc = extract_description(Path::new("test.sh"), src);
        assert!(desc.contains("setup"), "got: {desc}");
        assert!(desc.contains("teardown"), "got: {desc}");
    }

    #[test]
    fn github_actions_yaml() {
        let src = "name: CI\non: push\njobs:\n  build:\n    runs-on: ubuntu-latest";
        let desc = extract_description(Path::new("ci.yml"), src);
        assert!(desc.contains("GHA: CI"), "got: {desc}");
    }

    // -- New tests for extraction heuristic fixes --

    #[test]
    fn js_export_priority() {
        let src = r#"
const INTERNAL_A = 1;
const INTERNAL_B = 2;
const INTERNAL_C = 3;
export function publicFn() {}
export class PublicClass {}
"#;
        let desc = extract_description(Path::new("test.js"), src);
        // Exports must appear before internals.
        let pos_fn = desc.find("export function publicFn()");
        let pos_cls = desc.find("export class PublicClass");
        let pos_int = desc.find("INTERNAL_A");
        assert!(pos_fn.is_some(), "missing publicFn in: {desc}");
        assert!(pos_cls.is_some(), "missing PublicClass in: {desc}");
        assert!(pos_int.is_some(), "missing INTERNAL_A in: {desc}");
        assert!(
            pos_fn.unwrap() < pos_int.unwrap(),
            "export should precede internal in: {desc}"
        );
    }

    #[test]
    fn js_require_filtered() {
        let src = r#"
const fs = require('fs');
const path = require('path');
const Model = require('sequelize').Model;
function doWork() {}
"#;
        let desc = extract_description(Path::new("test.js"), src);
        assert!(
            !desc.contains(" fs"),
            "require('fs') should be filtered, got: {desc}"
        );
        assert!(
            !desc.contains("path"),
            "require('path') should be filtered, got: {desc}"
        );
        assert!(
            !desc.contains("Model"),
            "chained require should be filtered, got: {desc}"
        );
        assert!(
            desc.contains("doWork"),
            "function should remain, got: {desc}"
        );
    }

    #[test]
    fn js_no_false_positive_parens() {
        let src = r#"
const CONFIG = {
    handler: function() {},
    name: 'test'
};
"#;
        let desc = extract_description(Path::new("test.js"), src);
        assert!(
            !desc.contains("CONFIG()"),
            "object should not get () suffix, got: {desc}"
        );
        assert!(
            desc.contains("CONFIG"),
            "CONFIG should be present, got: {desc}"
        );
    }

    #[test]
    fn js_arrow_function_gets_parens() {
        let src = "const handler = (req, res) => {};\n";
        let desc = extract_description(Path::new("test.js"), src);
        assert!(
            desc.contains("handler()"),
            "arrow fn should get (), got: {desc}"
        );
    }

    #[test]
    fn js_export_default_class() {
        let src = "export default class Foo extends LitElement {}\n";
        let desc = extract_description(Path::new("test.js"), src);
        assert!(desc.contains("export default class Foo"), "got: {desc}");
        assert!(
            desc.contains("(web component)"),
            "LitElement should be detected, got: {desc}"
        );
    }

    #[test]
    fn js_export_default_function() {
        let src = "export default function createApp() {}\n";
        let desc = extract_description(Path::new("test.js"), src);
        assert!(desc.contains("export"), "got: {desc}");
        assert!(desc.contains("createApp"), "got: {desc}");
    }

    #[test]
    fn js_export_default_identifier() {
        let src = r#"
class Router {}
export default Router;
"#;
        let desc = extract_description(Path::new("test.js"), src);
        assert!(desc.contains("export default Router"), "got: {desc}");
    }

    #[test]
    fn js_export_default_object() {
        let src = "export default { a: 1, b: 2 };\n";
        let desc = extract_description(Path::new("test.js"), src);
        assert!(desc.contains("export default {...}"), "got: {desc}");
    }

    #[test]
    fn js_commonjs_module_exports_object() {
        let src = r#"
function saveImage() {}
function deleteImage() {}
function internal() {}
module.exports = { saveImage, deleteImage };
"#;
        let desc = extract_description(Path::new("test.js"), src);
        assert!(desc.contains("export function saveImage()"), "got: {desc}");
        assert!(
            desc.contains("export function deleteImage()"),
            "got: {desc}"
        );
        // Exports should appear before internals.
        let pos_save = desc.find("saveImage").unwrap();
        let pos_int = desc.find("internal").unwrap();
        assert!(
            pos_save < pos_int,
            "export should precede internal in: {desc}"
        );
    }

    #[test]
    fn js_commonjs_module_exports_function() {
        let src = "module.exports = function handler() {};\n";
        let desc = extract_description(Path::new("test.js"), src);
        assert!(desc.contains("export default function"), "got: {desc}");
    }

    #[test]
    fn js_commonjs_exports_dot() {
        let src = r#"
exports.saveImage = async function saveImage() {};
exports.deleteImage = async function deleteImage() {};
"#;
        let desc = extract_description(Path::new("test.js"), src);
        assert!(desc.contains("export saveImage()"), "got: {desc}");
        assert!(desc.contains("export deleteImage()"), "got: {desc}");
    }

    #[test]
    fn js_export_clause_resolution() {
        let src = r#"
function saveImage() {}
function deleteImage() {}
function internalHelper() {}
export { saveImage, deleteImage };
"#;
        let desc = extract_description(Path::new("test.js"), src);
        assert!(desc.contains("export function saveImage()"), "got: {desc}");
        assert!(
            desc.contains("export function deleteImage()"),
            "got: {desc}"
        );
        // Exports should appear before internal.
        let pos_save = desc.find("saveImage").unwrap();
        let pos_int = desc.find("internalHelper").unwrap();
        assert!(pos_save < pos_int, "exports first in: {desc}");
    }

    #[test]
    fn js_budget_8() {
        let src = r#"
export function a() {}
export function b() {}
export function c() {}
export function d() {}
export function e() {}
export function f() {}
export function g() {}
export function h() {}
export function i() {}
export function j() {}
"#;
        let desc = extract_description(Path::new("test.js"), src);
        let count = desc.split(", ").count();
        assert_eq!(count, 8, "budget should cap at 8, got {count}: {desc}");
    }

    #[test]
    fn rust_pub_priority() {
        let src = r#"
fn private_fn() {}
struct PrivateStruct {}
pub fn public_fn() {}
pub struct PublicStruct {}
"#;
        let desc = extract_description(Path::new("lib.rs"), src);
        let pos_pub = desc.find("pub fn public_fn()").unwrap();
        let pos_priv = desc.find("fn private_fn()").unwrap();
        assert!(
            pos_pub < pos_priv,
            "pub items should precede private in: {desc}"
        );
    }

    #[test]
    fn python_public_priority() {
        let src = r#"
def _private_helper():
    pass

class _InternalParser:
    pass

def public_api():
    pass

class PublicService:
    pass
"#;
        let desc = extract_description(Path::new("main.py"), src);
        let pos_pub = desc.find("def public_api()").unwrap();
        let pos_priv = desc.find("def _private_helper()").unwrap();
        assert!(pos_pub < pos_priv, "public items first in: {desc}");
    }

    #[test]
    fn go_exported_priority() {
        let src = r#"
package main

func helper() {}
func Handler() {}
type config struct {}
type Service struct {}
"#;
        let desc = extract_description(Path::new("main.go"), src);
        let pos_handler = desc.find("func Handler()").unwrap();
        let pos_helper = desc.find("func helper()").unwrap();
        assert!(pos_handler < pos_helper, "exported items first in: {desc}");
    }

    #[test]
    fn ts_enum_declaration() {
        let src = r#"
export enum Status {
    Active,
    Inactive,
}
"#;
        let desc = extract_description(Path::new("types.ts"), src);
        assert!(desc.contains("export enum Status"), "got: {desc}");
    }

    #[test]
    fn js_commonjs_module_exports_identifier() {
        let src = r#"
class Router {}
module.exports = Router;
"#;
        let desc = extract_description(Path::new("test.js"), src);
        assert!(desc.contains("export class Router"), "got: {desc}");
    }

    // -- Symbol extraction tests --

    #[test]
    fn extract_symbols_rust_basic() {
        let src = "pub fn main() {}\npub struct Config {}\nenum State {}\n";
        let symbols = extract_symbols(Path::new("lib.rs"), src);
        assert_eq!(symbols.len(), 3);
        assert_eq!(symbols[0].name, "main");
        assert_eq!(symbols[0].kind, "fn");
        assert!(symbols[0].exported);
        assert_eq!(symbols[1].name, "Config");
        assert_eq!(symbols[1].kind, "struct");
        assert_eq!(symbols[2].name, "State");
        assert_eq!(symbols[2].kind, "enum");
        assert!(!symbols[2].exported);
    }

    #[test]
    fn extract_symbols_rust_impl_methods() {
        let src = "pub struct Foo {}\n\nimpl Foo {\n    pub fn new() -> Self { Foo {} }\n    fn helper(&self) {}\n}\n";
        let symbols = extract_symbols(Path::new("foo.rs"), src);
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Foo" && s.kind == "struct")
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Foo::new" && s.kind == "method" && s.exported)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Foo::helper" && s.kind == "method" && !s.exported)
        );
    }

    #[test]
    fn extract_symbols_rust_trait_impl_uses_concrete_type() {
        let src = "trait Builder {\n    fn build(&self);\n}\nstruct Car {}\nimpl Builder for Car {\n    fn build(&self) {}\n}\n";
        let symbols = extract_symbols(Path::new("car.rs"), src);
        assert!(
            symbols.iter().any(|s| s.name == "Car::build"),
            "trait impl method should be qualified with concrete type, got: {symbols:?}"
        );
        assert!(
            !symbols.iter().any(|s| s.name == "Builder::build"),
            "should not use trait name for qualification"
        );
    }

    #[test]
    fn extract_symbols_rust_generic_impl() {
        let src = "struct Foo<T> { val: T }\nimpl<T> Foo<T> {\n    pub fn new(val: T) -> Self { Foo { val } }\n}\n";
        let symbols = extract_symbols(Path::new("foo.rs"), src);
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Foo::new" && s.kind == "method"),
            "generic impl should qualify as Foo::new, got: {symbols:?}"
        );
    }

    #[test]
    fn extract_symbols_rust_generic_trait_impl() {
        let src = "trait Builder<T> {\n    fn build(&self) -> T;\n}\nstruct Car {}\nimpl<T> Builder<T> for Car {\n    fn build(&self) -> T { todo!() }\n}\n";
        let symbols = extract_symbols(Path::new("car.rs"), src);
        assert!(
            symbols.iter().any(|s| s.name == "Car::build"),
            "generic trait impl should qualify with concrete type, got: {symbols:?}"
        );
    }

    #[test]
    fn extract_symbols_rust_scoped_impl() {
        let src =
            "mod inner { pub struct Car {} }\nimpl inner::Car {\n    pub fn drive(&self) {}\n}\n";
        let symbols = extract_symbols(Path::new("car.rs"), src);
        assert!(
            symbols.iter().any(|s| s.name == "Car::drive"),
            "scoped impl should qualify with final type name, got: {symbols:?}"
        );
    }

    #[test]
    fn extract_symbols_rust_scoped_generic_impl() {
        let src = "mod inner { pub struct Car<T> { val: T } }\nimpl<T> inner::Car<T> {\n    pub fn new(val: T) -> Self { inner::Car { val } }\n}\n";
        let symbols = extract_symbols(Path::new("car.rs"), src);
        assert!(
            symbols.iter().any(|s| s.name == "Car::new"),
            "scoped generic impl should qualify with final type name, got: {symbols:?}"
        );
    }

    #[test]
    fn extract_symbols_js_exports() {
        let src =
            "export function doWork() {}\nexport class Service {}\nconst helper = () => {};\n";
        let symbols = extract_symbols(Path::new("test.js"), src);
        assert!(symbols.iter().any(|s| s.name == "doWork" && s.exported));
        assert!(symbols.iter().any(|s| s.name == "Service" && s.exported));
        assert!(symbols.iter().any(|s| s.name == "helper" && !s.exported));
    }

    #[test]
    fn extract_symbols_python() {
        let src =
            "def public_fn():\n    pass\n\nclass MyClass:\n    pass\n\ndef _private():\n    pass\n";
        let symbols = extract_symbols(Path::new("main.py"), src);
        assert!(symbols.iter().any(|s| s.name == "public_fn" && s.exported));
        assert!(symbols.iter().any(|s| s.name == "MyClass" && s.exported));
        assert!(symbols.iter().any(|s| s.name == "_private" && !s.exported));
    }

    #[test]
    fn extract_symbols_unsupported_ext() {
        assert!(extract_symbols(Path::new("styles.css"), ".foo { color: red; }").is_empty());
    }

    #[test]
    fn symbol_signature_strips_body() {
        let src = "pub fn foo(x: i32) -> String {\n    x.to_string()\n}\n";
        let symbols = extract_symbols(Path::new("test.rs"), src);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].signature, "pub fn foo(x: i32) -> String");
    }

    #[test]
    fn symbol_line_numbers() {
        let src = "\npub fn alpha() {}\n\npub fn beta() {}\n";
        let symbols = extract_symbols(Path::new("test.rs"), src);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "alpha");
        assert_eq!(symbols[0].line_start, 2);
        assert_eq!(symbols[1].name, "beta");
        assert_eq!(symbols[1].line_start, 4);
    }
}
