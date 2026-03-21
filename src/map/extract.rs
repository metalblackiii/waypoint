use std::path::Path;

use regex::Regex;

/// Extract a one-line description for a file based on its content.
/// Uses tree-sitter for supported languages, regex fallback for others.
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

    let mut declarations = Vec::new();
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        if let Some(decl) = extract_declaration(child, ext, content) {
            declarations.push(decl);
            if declarations.len() >= 5 {
                break;
            }
        }
    }

    if declarations.is_empty() {
        return None;
    }

    Some(declarations.join(", "))
}

fn extract_declaration(node: tree_sitter::Node, ext: &str, source: &str) -> Option<String> {
    let kind = node.kind();
    match ext {
        "rs" => extract_rust(node, kind, source),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => extract_js(node, kind, source),
        "py" => extract_python(node, kind, source),
        "go" => extract_go(node, kind, source),
        _ => None,
    }
}

fn extract_rust(node: tree_sitter::Node, kind: &str, source: &str) -> Option<String> {
    match kind {
        "function_item" => {
            let name = child_text(node, "identifier", source)?;
            Some(format!("fn {name}()"))
        }
        "struct_item" => {
            let name = child_text(node, "type_identifier", source)?;
            Some(format!("struct {name}"))
        }
        "enum_item" => {
            let name = child_text(node, "type_identifier", source)?;
            Some(format!("enum {name}"))
        }
        "trait_item" => {
            let name = child_text(node, "type_identifier", source)?;
            Some(format!("trait {name}"))
        }
        "impl_item" => {
            let name = child_text(node, "type_identifier", source)?;
            Some(format!("impl {name}"))
        }
        "mod_item" => {
            let name = child_text(node, "identifier", source)?;
            Some(format!("mod {name}"))
        }
        "const_item" | "static_item" => {
            let name = child_text(node, "identifier", source)?;
            Some(format!("const {name}"))
        }
        _ => None,
    }
}

fn extract_js(node: tree_sitter::Node, kind: &str, source: &str) -> Option<String> {
    match kind {
        "function_declaration" => {
            let name = child_text(node, "identifier", source)?;
            Some(format!("function {name}()"))
        }
        "class_declaration" => {
            let name = child_text(node, "identifier", source)?;
            let text = node_text(node, source);
            if text.contains("HTMLElement")
                || text.contains("LitElement")
                || text.contains("customElement")
            {
                Some(format!("class {name} (web component)"))
            } else {
                Some(format!("class {name}"))
            }
        }
        "export_statement" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if let Some(decl) = extract_js(child, child.kind(), source) {
                    return Some(format!("export {decl}"));
                }
            }
            let text = node_text(node, source);
            if text.starts_with("export default") {
                Some("export default".to_string())
            } else {
                None
            }
        }
        "lexical_declaration" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "variable_declarator"
                    && let Some(name) = child_text(child, "identifier", source)
                {
                    let text = node_text(node, source);
                    if text.contains("=>") || text.contains("function") {
                        return Some(format!("{name}()"));
                    }
                    return Some(name);
                }
            }
            None
        }
        "interface_declaration" | "type_alias_declaration" => {
            let name = child_text(node, "type_identifier", source)
                .or_else(|| child_text(node, "identifier", source))?;
            Some(format!("type {name}"))
        }
        _ => None,
    }
}

fn extract_python(node: tree_sitter::Node, kind: &str, source: &str) -> Option<String> {
    match kind {
        "function_definition" => {
            let name = child_text(node, "identifier", source)?;
            Some(format!("def {name}()"))
        }
        "class_definition" => {
            let name = child_text(node, "identifier", source)?;
            Some(format!("class {name}"))
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

fn extract_go(node: tree_sitter::Node, kind: &str, source: &str) -> Option<String> {
    match kind {
        "function_declaration" => {
            let name = child_text(node, "identifier", source)?;
            Some(format!("func {name}()"))
        }
        "method_declaration" => {
            let name = child_text(node, "field_identifier", source)?;
            Some(format!("func {name}()"))
        }
        "type_declaration" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "type_spec"
                    && let Some(name) = child_text(child, "type_identifier", source)
                {
                    return Some(format!("type {name}"));
                }
            }
            None
        }
        _ => None,
    }
}

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
    let re = Regex::new(r"(?m)^(?:function\s+(\w+)|(\w+)\s*\(\))").unwrap();
    let funcs: Vec<String> = re
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).or(cap.get(2)).map(|m| m.as_str().to_string()))
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
    let title_re = Regex::new(r"<title>([^<]+)</title>").unwrap();
    if let Some(cap) = title_re.captures(content) {
        return cap[1].trim().to_string();
    }
    "HTML document".to_string()
}

fn extract_css(content: &str) -> String {
    let selector_re = Regex::new(r"(?m)^([.#]?[\w-]+)\s*\{").unwrap();
    let selectors: Vec<&str> = selector_re
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
        .take(5)
        .collect();
    if selectors.is_empty() {
        "stylesheet".to_string()
    } else {
        format!("styles: {}", selectors.join(", "))
    }
}

fn extract_sql(content: &str) -> String {
    let table_re =
        Regex::new(r#"(?mi)CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?[`"]?(\w+)"#).unwrap();
    let tables: Vec<&str> = table_re
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
        .take(5)
        .collect();
    if tables.is_empty() {
        "SQL script".to_string()
    } else {
        format!("tables: {}", tables.join(", "))
    }
}

fn extract_yaml(content: &str, filename: &str) -> String {
    let lower = filename.to_lowercase();
    if lower.contains("docker-compose") || lower.contains("compose") {
        let svc_re = Regex::new(r"(?m)^\s{2}(\w[\w-]+):$").unwrap();
        let services: Vec<&str> = svc_re
            .captures_iter(content)
            .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
            .take(5)
            .collect();
        if !services.is_empty() {
            return format!("compose services: {}", services.join(", "));
        }
    }

    if content.contains("on:") && (content.contains("jobs:") || content.contains("workflow")) {
        let name_re = Regex::new(r"(?m)^name:\s*(.+)").unwrap();
        if let Some(cap) = name_re.captures(content) {
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
    let type_re = Regex::new(r"(?m)^type\s+(\w+)").unwrap();
    let types: Vec<&str> = type_re
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
        .take(5)
        .collect();
    if types.is_empty() {
        "GraphQL schema".to_string()
    } else {
        format!("types: {}", types.join(", "))
    }
}

fn extract_terraform(content: &str) -> String {
    let resource_re = Regex::new(r#"(?m)^resource\s+"(\w+)"\s+"(\w+)""#).unwrap();
    let resources: Vec<String> = resource_re
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
    let root_re = Regex::new(r"<(\w+)[\s>]").unwrap();
    if let Some(cap) = root_re.captures(content) {
        let tag = &cap[1];
        if tag != "xml" {
            return format!("XML: <{tag}>");
        }
    }
    "XML document".to_string()
}

fn extract_vue(content: &str) -> String {
    let name_re = Regex::new(r#"(?m)name:\s*['"](\w+)['"]"#).unwrap();
    if let Some(cap) = name_re.captures(content) {
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
    let re = Regex::new(r"(?m)^(?:class|module)\s+(\w+)").unwrap();
    let items: Vec<&str> = re
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
        .take(3)
        .collect();
    if items.is_empty() {
        "Ruby source".to_string()
    } else {
        items.join(", ")
    }
}

fn extract_jvm(content: &str) -> String {
    let re = Regex::new(r"(?m)^(?:public\s+)?(?:class|interface|enum)\s+(\w+)").unwrap();
    let items: Vec<&str> = re
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
        .take(3)
        .collect();
    if items.is_empty() {
        "JVM source".to_string()
    } else {
        items.join(", ")
    }
}

fn extract_c(content: &str) -> String {
    let fn_re = Regex::new(r"(?m)^\w[\w\s*]+\s+(\w+)\s*\(").unwrap();
    let funcs: Vec<&str> = fn_re
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
    let re = Regex::new(r"(?m)^(?:class|struct|enum|protocol)\s+(\w+)").unwrap();
    let items: Vec<&str> = re
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
        .take(3)
        .collect();
    if items.is_empty() {
        "Swift source".to_string()
    } else {
        items.join(", ")
    }
}

fn extract_proto(content: &str) -> String {
    let msg_re = Regex::new(r"(?m)^message\s+(\w+)").unwrap();
    let svc_re = Regex::new(r"(?m)^service\s+(\w+)").unwrap();
    let messages: Vec<&str> = msg_re
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
        .take(3)
        .collect();
    let services: Vec<&str> = svc_re
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
