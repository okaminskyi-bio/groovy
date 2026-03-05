use eframe::egui::{self, Color32, FontId, TextFormat};
use regex::Regex;
use std::sync::OnceLock;

pub fn groovy_layout(text: &str, dark_mode: bool) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let token_re = token_regex();

    let base = text_format(color(dark_mode, [224, 227, 231], [16, 18, 21]));
    let comment = text_format(color(dark_mode, [126, 162, 104], [69, 126, 50]));
    let string = text_format(color(dark_mode, [242, 190, 111], [160, 90, 0]));
    let keyword = text_format(color(dark_mode, [115, 179, 255], [0, 86, 170]));
    let marker = text_format(color(dark_mode, [255, 114, 147], [177, 33, 76]));
    let number = text_format(color(dark_mode, [172, 216, 140], [36, 130, 58]));

    let mut cursor = 0usize;
    for m in token_re.find_iter(text) {
        if m.start() > cursor {
            job.append(&text[cursor..m.start()], 0.0, base.clone());
        }

        let token = m.as_str();
        let fmt = if token.starts_with("//") || token.starts_with("/*") {
            comment.clone()
        } else if token.starts_with('"') || token.starts_with('\'') {
            string.clone()
        } else if is_template_marker(token) {
            marker.clone()
        } else if token.chars().all(|c| c.is_ascii_digit() || c == '.') {
            number.clone()
        } else if is_groovy_keyword(token) {
            keyword.clone()
        } else {
            base.clone()
        };

        job.append(token, 0.0, fmt);
        cursor = m.end();
    }

    if cursor < text.len() {
        job.append(&text[cursor..], 0.0, base);
    }

    job
}

fn text_format(color: Color32) -> TextFormat {
    TextFormat {
        font_id: FontId::monospace(15.0),
        color,
        ..Default::default()
    }
}

fn color(dark_mode: bool, dark: [u8; 3], light: [u8; 3]) -> Color32 {
    let c = if dark_mode { dark } else { light };
    Color32::from_rgb(c[0], c[1], c[2])
}

fn token_regex() -> &'static Regex {
    static TOKEN_RE: OnceLock<Regex> = OnceLock::new();
    TOKEN_RE.get_or_init(|| {
        Regex::new(
            r#"(?s)/\*.*?\*/|//[^\n]*|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|\{:\|\|\}|\{:\|\}|\{\|\|\:|\{\|\:|\{\!|!\}|\{/[^\n]*?\}|[A-Za-z_][A-Za-z0-9_]*|\d+(?:\.\d+)?"#,
        )
        .expect("valid token regex")
    })
}

fn is_template_marker(token: &str) -> bool {
    matches!(
        token,
        "{:||}" | "{:|}" | "{||:" | "{|:" | "{!" | "!}" | "{/Printer"
    ) || token.starts_with("{/")
}

fn is_groovy_keyword(token: &str) -> bool {
    matches!(
        token,
        "as" | "assert"
            | "break"
            | "case"
            | "catch"
            | "class"
            | "continue"
            | "def"
            | "default"
            | "do"
            | "else"
            | "enum"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "if"
            | "implements"
            | "import"
            | "in"
            | "instanceof"
            | "interface"
            | "new"
            | "null"
            | "package"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "throws"
            | "trait"
            | "true"
            | "try"
            | "while"
    )
}
