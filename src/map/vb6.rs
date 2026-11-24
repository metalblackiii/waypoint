//! Line-based VB6 extraction.
//!
//! WHY not tree-sitter: both published VB6 grammars (`tree-sitter-vb6` 0.0.2,
//! `wicked-estate-tree-sitter-vb6` 0.1.1) produced parse errors on 1,119 of
//! 1,134 real `.frm`/`.cls`/`.bas`/`.dob` files and recovered 3,662 of 21,015
//! procedures. Procedure headers are regular enough to match line by line.

use std::sync::LazyLock;

use regex::Regex;

use super::extract::{Symbol, re};

/// Description budget, matching the tree-sitter description path.
const DISPLAY_BUDGET: usize = 8;
const SIGNATURE_MAX: usize = 200;

/// Whether `bytes` is a VB6 source file, judged by its first line.
///
/// WHY: `.cls` is also a LaTeX class file and `.bas` is shared with other
/// BASICs. The VB6 IDE always writes `VERSION …` first in forms, classes,
/// user documents, and user controls, and `Attribute VB_Name` first in modules.
#[must_use]
pub fn is_source(bytes: &[u8]) -> bool {
    bytes.starts_with(b"VERSION ") || bytes.starts_with(b"Attribute VB_Name")
}

/// Decode a VB6 source file. The VB6 IDE writes the ANSI code page, which is
/// Windows-1252 on US-English installs, and never UTF-8.
#[must_use]
pub fn decode(bytes: &[u8]) -> String {
    let (text, _had_errors) = encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes);
    text.into_owned()
}

/// Extract `Sub`, `Function`, and `Property Get/Let/Set` procedures, qualified
/// by the module's `VB_Name` (`frmMain::cmdSave_Click`). Returns nothing for
/// content that is not VB6 source.
#[must_use]
pub fn symbols(content: &str) -> Vec<Symbol> {
    if !is_source(content.as_bytes()) {
        return Vec::new();
    }
    // Modules (`.bas`) hold free procedures; every other VB6 file is a class-like
    // object (form, class, user document, user control) whose procedures are methods.
    let procedure_kind = if content.starts_with("Attribute VB_Name") {
        "fn"
    } else {
        "method"
    };
    let module = module_name(content);

    let lines = logical_lines(content);
    let mut symbols = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(header) = HEADER.captures(&line.text) else {
            continue;
        };
        let keyword = header["keyword"].to_ascii_lowercase();
        let kind = if keyword.starts_with("property") {
            "property"
        } else {
            procedure_kind
        };
        let name = &header["name"];
        let exported = !header
            .name("access")
            .is_some_and(|a| a.as_str().eq_ignore_ascii_case("private"));
        let line_end = if INLINE_END.is_match(&line.text) {
            line.last_physical
        } else {
            lines[index + 1..]
                .iter()
                .find(|l| END.is_match(&l.text))
                .map_or(line.last_physical, |l| l.first_physical)
        };
        symbols.push(Symbol {
            file_path: String::new(),
            name: match module {
                Some(module) => format!("{module}::{name}"),
                None => name.to_string(),
            },
            kind: kind.to_string(),
            signature: signature(&line.text),
            line_start: line.first_physical,
            line_end,
            exported,
        });
    }
    symbols
}

/// One-line description: the module name, then up to eight procedure names
/// with public ones first. `None` for content that is not VB6 source.
#[must_use]
pub fn description(content: &str) -> Option<String> {
    let mut symbols = symbols(content);
    if symbols.is_empty() {
        return None;
    }
    symbols.sort_by_key(|s| !s.exported);
    let mut names: Vec<&str> = Vec::new();
    for symbol in &symbols {
        let bare = symbol.name.rsplit("::").next().unwrap_or(&symbol.name);
        // Property Get/Let/Set share one name; list it once.
        if !names.contains(&bare) {
            names.push(bare);
        }
        if names.len() == DISPLAY_BUDGET {
            break;
        }
    }
    let listed = names.join(", ");
    Some(match module_name(content) {
        Some(module) => format!("{module}: {listed}"),
        None => listed,
    })
}

static HEADER: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)^\s*(?:(?P<access>public|private|friend|global)\s+)?(?:static\s+)?(?P<keyword>sub|function|property\s+(?:get|let|set))\s+(?P<name>[a-z_][a-z0-9_]*)",
    )
});
static END: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^\s*end\s+(?:sub|function|property)\b"));
/// A whole procedure on one line, `Sub A(): End Sub`: the `:`-separated last
/// statement is the End, optionally followed by a `'` or `Rem` comment.
static INLINE_END: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i):\s*end\s+(?:sub|function|property)\s*(?:(?:'|rem\b).*)?$"));
static VB_NAME: LazyLock<Regex> = LazyLock::new(|| re(r#"(?m)^Attribute VB_Name = "([^"]+)""#));

fn module_name(content: &str) -> Option<&str> {
    VB_NAME
        .captures(content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
}

/// A statement after joining ` _` line continuations, with its 1-based
/// physical line span.
struct LogicalLine {
    text: String,
    first_physical: i64,
    last_physical: i64,
}

/// Whether the physical line carries a `'` or `Rem` comment.
///
/// WHY: VB6 never continues a comment onto the next line, so a trailing ` _`
/// in a comment is plain text. Joining it would hide the next line's procedure
/// header.
fn has_comment(physical: &str) -> bool {
    let trimmed = physical.trim_start();
    let starts_with_rem = trimmed
        .get(..3)
        .is_some_and(|word| word.eq_ignore_ascii_case("rem"))
        && trimmed[3..].chars().next().is_none_or(char::is_whitespace);
    if starts_with_rem {
        return true;
    }
    let mut in_string = false;
    for c in physical.chars() {
        match c {
            '"' => in_string = !in_string,
            '\'' if !in_string => return true,
            _ => {}
        }
    }
    false
}

fn logical_lines(content: &str) -> Vec<LogicalLine> {
    let mut lines = Vec::new();
    let mut pending: Option<LogicalLine> = None;
    for (number, raw) in (1_i64..).zip(content.lines()) {
        let physical = raw.trim_end_matches('\r');
        let (part, continues) = match physical.strip_suffix(" _") {
            Some(head) if !has_comment(physical) => (head, true),
            _ => (physical, false),
        };
        let line = match pending.take() {
            Some(mut open) => {
                open.text.push(' ');
                open.text.push_str(part.trim_start());
                open.last_physical = number;
                open
            }
            None => LogicalLine {
                text: part.to_string(),
                first_physical: number,
                last_physical: number,
            },
        };
        if continues {
            pending = Some(line);
        } else {
            lines.push(line);
        }
    }
    lines.extend(pending);
    lines
}

fn signature(header: &str) -> String {
    let collapsed = header.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.len() <= SIGNATURE_MAX {
        return collapsed;
    }
    let mut end = SIGNATURE_MAX - 3;
    while !collapsed.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &collapsed[..end])
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const FORM: &str = "VERSION 5.00\r\n\
Begin VB.Form frmMain \r\n\
   Caption         =   \"Main\"\r\n\
   Begin VB.CommandButton cmdSave \r\n\
      Caption         =   \"Save\"\r\n\
   End\r\n\
End\r\n\
Attribute VB_Name = \"frmMain\"\r\n\
Option Explicit\r\n\
Private Declare Function GetTickCount Lib \"kernel32\" () As Long\r\n\
\r\n\
Private Sub cmdSave_Click()\r\n\
    SaveRecord 1\r\n\
End Sub\r\n\
\r\n\
Public Function SaveRecord(ByVal id As Long, _\r\n\
                           Optional ByVal force As Boolean = False) As Boolean\r\n\
    SaveRecord = True\r\n\
End Function\r\n\
\r\n\
Property Get Caption() As String\r\n\
End Property\r\n\
Property Let Caption(ByVal value As String)\r\n\
End Property\r\n";

    const MODULE: &str = "Attribute VB_Name = \"modUtil\"\r\n\
Friend Static Function Counter() As Long\r\n\
End Function\r\n\
Sub Main()\r\n\
End Sub\r\n";

    fn names(symbols: &[Symbol]) -> Vec<&str> {
        symbols.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn form_procedures_are_qualified_methods() {
        let symbols = symbols(FORM);
        assert_eq!(
            names(&symbols),
            [
                "frmMain::cmdSave_Click",
                "frmMain::SaveRecord",
                "frmMain::Caption",
                "frmMain::Caption",
            ]
        );
        assert_eq!(symbols[0].kind, "method");
        assert_eq!(symbols[2].kind, "property");
    }

    #[test]
    fn designer_controls_and_api_declares_are_not_procedures() {
        let symbols = symbols(FORM);
        assert!(!names(&symbols).iter().any(|n| n.contains("cmdSave ")));
        assert!(!names(&symbols).iter().any(|n| n.contains("GetTickCount")));
    }

    #[test]
    fn line_span_runs_from_header_to_end_statement() {
        let symbols = symbols(FORM);
        assert_eq!((symbols[0].line_start, symbols[0].line_end), (12, 14));
        assert_eq!((symbols[1].line_start, symbols[1].line_end), (16, 19));
    }

    #[test]
    fn continued_header_is_joined_into_one_signature() {
        let symbols = symbols(FORM);
        assert_eq!(
            symbols[1].signature,
            "Public Function SaveRecord(ByVal id As Long, Optional ByVal force As Boolean = False) As Boolean"
        );
    }

    #[test]
    fn comment_ending_in_continuation_marker_does_not_hide_next_procedure() {
        let module = "Attribute VB_Name = \"modA\"\r\n\
' note _\r\n\
Sub First()\r\n\
End Sub\r\n\
Rem remark _\r\n\
Sub Second() ' trailing _\r\n\
End Sub\r\n\
Sub Third(ByVal s As String = \"it's\", _\r\n\
          ByVal n As Long)\r\n\
End Sub\r\n";
        assert_eq!(
            names(&symbols(module)),
            ["modA::First", "modA::Second", "modA::Third"]
        );
    }

    #[test]
    fn only_private_procedures_are_unexported() {
        let form = symbols(FORM);
        assert!(!form[0].exported, "Private Sub");
        assert!(form[1].exported, "Public Function");
        assert!(form[2].exported, "no modifier defaults to Public");
        let module = symbols(MODULE);
        assert!(module[0].exported, "Friend is project-wide");
    }

    #[test]
    fn single_line_procedure_ends_on_its_own_line() {
        let module = "Attribute VB_Name = \"modA\"\r\n\
Sub Noop(): End Sub\r\n\
Function One() As Long: One = 1: End Function ' done\r\n\
Property Get Two() As Long: Two = 2: End Property Rem done\r\n\
Sub Wrapped(ByVal a As Long, _\r\n\
            ByVal b As Long): End Sub\r\n\
Sub Later()\r\n\
End Sub\r\n";
        let symbols = symbols(module);
        let spans: Vec<(i64, i64)> = symbols.iter().map(|s| (s.line_start, s.line_end)).collect();
        assert_eq!(spans, [(2, 2), (3, 3), (4, 4), (5, 6), (7, 8)]);
    }

    #[test]
    fn module_procedures_are_free_functions() {
        let symbols = symbols(MODULE);
        assert_eq!(names(&symbols), ["modUtil::Counter", "modUtil::Main"]);
        assert!(symbols.iter().all(|s| s.kind == "fn"));
    }

    #[test]
    fn non_vb6_cls_yields_nothing() {
        let latex = "\\NeedsTeXFormat{LaTeX2e}\n\\ProvidesClass{report}\nSub not_vb\n";
        assert!(symbols(latex).is_empty());
        assert!(!is_source(latex.as_bytes()));
    }

    #[test]
    fn windows_1252_bytes_decode() {
        // 0xE9 is `é` in Windows-1252 and an invalid lone byte in UTF-8.
        let bytes = b"Attribute VB_Name = \"modA\"\r\n' caf\xe9\r\nSub Go()\r\nEnd Sub\r\n";
        let text = decode(bytes);
        assert!(text.contains("caf\u{e9}"));
        assert_eq!(names(&symbols(&text)), ["modA::Go"]);
    }

    #[test]
    fn description_lists_each_name_once_public_first() {
        assert_eq!(
            description(FORM).unwrap(),
            "frmMain: SaveRecord, Caption, cmdSave_Click"
        );
    }
}
