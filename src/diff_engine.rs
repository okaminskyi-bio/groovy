use similar::{ChangeTag, DiffTag, TextDiff};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    Equal,
    Added,
    Removed,
    Replaced,
}

#[derive(Clone, Debug)]
pub struct DiffLine {
    pub left: Option<String>,
    pub right: Option<String>,
    pub kind: DiffKind,
}

pub fn side_by_side_diff(left: &str, right: &str) -> Vec<DiffLine> {
    let diff = TextDiff::from_lines(left, right);
    let mut out = Vec::new();

    for op in diff.ops() {
        match op.tag() {
            DiffTag::Equal => {
                for change in diff.iter_changes(op) {
                    let line = trim_trailing_newline(change.to_string());
                    out.push(DiffLine {
                        left: Some(line.clone()),
                        right: Some(line),
                        kind: DiffKind::Equal,
                    });
                }
            }
            DiffTag::Delete => {
                for change in diff.iter_changes(op) {
                    out.push(DiffLine {
                        left: Some(trim_trailing_newline(change.to_string())),
                        right: None,
                        kind: DiffKind::Removed,
                    });
                }
            }
            DiffTag::Insert => {
                for change in diff.iter_changes(op) {
                    out.push(DiffLine {
                        left: None,
                        right: Some(trim_trailing_newline(change.to_string())),
                        kind: DiffKind::Added,
                    });
                }
            }
            DiffTag::Replace => {
                let mut removes = Vec::new();
                let mut adds = Vec::new();
                for change in diff.iter_changes(op) {
                    match change.tag() {
                        ChangeTag::Delete => {
                            removes.push(trim_trailing_newline(change.to_string()))
                        }
                        ChangeTag::Insert => adds.push(trim_trailing_newline(change.to_string())),
                        ChangeTag::Equal => {}
                    }
                }

                let max_len = removes.len().max(adds.len());
                for idx in 0..max_len {
                    out.push(DiffLine {
                        left: removes.get(idx).cloned(),
                        right: adds.get(idx).cloned(),
                        kind: DiffKind::Replaced,
                    });
                }
            }
        }
    }

    // Upgrade adjacent remove+add runs to replaced pairs for cleaner split view.
    collapse_replace_runs(out)
}

fn collapse_replace_runs(lines: Vec<DiffLine>) -> Vec<DiffLine> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        if lines[i].kind == DiffKind::Removed {
            let start = i;
            let mut removes = Vec::new();
            while i < lines.len() && lines[i].kind == DiffKind::Removed {
                removes.push(lines[i].left.clone().unwrap_or_default());
                i += 1;
            }

            if i < lines.len() && lines[i].kind == DiffKind::Added {
                let mut adds = Vec::new();
                while i < lines.len() && lines[i].kind == DiffKind::Added {
                    adds.push(lines[i].right.clone().unwrap_or_default());
                    i += 1;
                }
                let max_len = removes.len().max(adds.len());
                for idx in 0..max_len {
                    out.push(DiffLine {
                        left: removes.get(idx).cloned(),
                        right: adds.get(idx).cloned(),
                        kind: DiffKind::Replaced,
                    });
                }
            } else {
                for r in removes {
                    out.push(DiffLine {
                        left: Some(r),
                        right: None,
                        kind: DiffKind::Removed,
                    });
                }
            }

            if start == i {
                i += 1;
            }
        } else {
            out.push(lines[i].clone());
            i += 1;
        }
    }
    out
}

fn trim_trailing_newline(mut line: String) -> String {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    line
}
