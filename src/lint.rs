use regex::Regex;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct LintResult {
    pub diagnostics: Vec<Diagnostic>,
    pub placeholders: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockKind {
    Loop,
    DoubleLoop,
    RunFirst,
}

#[derive(Clone, Copy, Debug)]
struct Block {
    kind: BlockKind,
    pos: usize,
}

pub fn lint_template(text: &str) -> LintResult {
    let mut diagnostics = Vec::new();
    diagnostics.extend(check_balanced_markers(text));
    diagnostics.extend(check_known_issues(text));

    let placeholder_re = Regex::new(r"\{[^{}\r\n]+\}").expect("valid placeholder regex");
    let mut placeholders = BTreeSet::new();
    for m in placeholder_re.find_iter(text) {
        let p = m.as_str().trim();
        if !is_control_marker(p) {
            placeholders.insert(p.to_string());
        }
    }

    LintResult {
        diagnostics,
        placeholders: placeholders.into_iter().collect(),
    }
}

fn check_balanced_markers(text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut stack: Vec<Block> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let rest = &text[i..];

        if rest.starts_with("{:||}") {
            pop_or_report(
                &mut stack,
                BlockKind::DoubleLoop,
                i,
                text,
                "{:||}",
                &mut diagnostics,
            );
            i += 5;
            continue;
        }

        if rest.starts_with("{:|}") {
            pop_or_report(
                &mut stack,
                BlockKind::Loop,
                i,
                text,
                "{:|}",
                &mut diagnostics,
            );
            i += 4;
            continue;
        }

        if rest.starts_with("!}") {
            pop_or_report(
                &mut stack,
                BlockKind::RunFirst,
                i,
                text,
                "!}",
                &mut diagnostics,
            );
            i += 2;
            continue;
        }

        if rest.starts_with("{||:") {
            stack.push(Block {
                kind: BlockKind::DoubleLoop,
                pos: i,
            });
            i += 4;
            continue;
        }

        if rest.starts_with("{|:") {
            stack.push(Block {
                kind: BlockKind::Loop,
                pos: i,
            });
            i += 3;
            continue;
        }

        if rest.starts_with("{!") {
            stack.push(Block {
                kind: BlockKind::RunFirst,
                pos: i,
            });
            i += 2;
            continue;
        }

        i += 1;
    }

    while let Some(block) = stack.pop() {
        let msg = match block.kind {
            BlockKind::Loop => "Unclosed marker `{|:` (expected `{:|}`).",
            BlockKind::DoubleLoop => "Unclosed marker `{||:` (expected `{:||}`).",
            BlockKind::RunFirst => "Unclosed marker `{!` (expected `!}`).",
        };
        diagnostics.push(build_diag(Severity::Error, text, block.pos, msg));
    }

    diagnostics
}

fn pop_or_report(
    stack: &mut Vec<Block>,
    expected: BlockKind,
    pos: usize,
    text: &str,
    token: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match stack.last() {
        Some(top) if top.kind == expected => {
            stack.pop();
        }
        _ => {
            diagnostics.push(build_diag(
                Severity::Error,
                text,
                pos,
                &format!("Unexpected closing marker `{token}`."),
            ));
        }
    }
}

fn check_known_issues(text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let rules = vec![
        (
            Regex::new(r"returnpe\.").expect("valid regex"),
            Severity::Error,
            "Potential typo: `returnpe.` found. Expected `return pe.`",
        ),
        (
            Regex::new(r"return\s+null;\s*!}").expect("valid regex"),
            Severity::Warning,
            "Place `!}` on a new line after `return null;` to avoid parser edge cases.",
        ),
        (
            Regex::new(r"com\.dialog\.dialims\.business\.BiolabAnsprechpartnerController")
                .expect("valid regex"),
            Severity::Error,
            "Legacy class path found. Use `com.dialog.dialims.kunden.BiolabAnsprechpartnerController`.",
        ),
        (
            Regex::new(r"com\.dialog\.dialims\.business\.BiolabPruefberichtController")
                .expect("valid regex"),
            Severity::Error,
            "Legacy class path found. Use `com.dialog.dialims.kunden.BiolabPruefberichtController`.",
        ),
        (
            Regex::new(r"kein passendes Property gefunden").expect("valid regex"),
            Severity::Warning,
            "Debug fallback text found in template output section.",
        ),
    ];

    for (re, severity, message) in rules {
        for m in re.find_iter(text) {
            diagnostics.push(build_diag(severity.clone(), text, m.start(), message));
        }
    }

    diagnostics
}

fn is_control_marker(value: &str) -> bool {
    value.starts_with("{|:")
        || value.starts_with("{||:")
        || value.starts_with("{:")
        || value.starts_with("{!")
        || value.starts_with("{/")
}

fn build_diag(severity: Severity, text: &str, index: usize, message: &str) -> Diagnostic {
    let (line, column) = line_column(text, index);
    Diagnostic {
        severity,
        line,
        column,
        message: message.to_string(),
    }
}

fn line_column(text: &str, index: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut column = 1usize;
    let mut i = 0usize;

    for ch in text.chars() {
        if i >= index {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
        i += ch.len_utf8();
    }

    (line, column)
}
