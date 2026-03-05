use crate::collab::{self, CollabMessage, Decision, WsEnvelope, WsHandle};
use crate::diff_engine::{self, DiffKind, DiffLine};
use crate::docx;
use crate::git_ops;
use crate::highlight;
use crate::lint::{self, LintResult, Severity};
use crate::render;
use arboard::Clipboard;
use eframe::egui::{self, Color32, RichText, TextBuffer, TextEdit, TextStyle};
use eframe::{App, CreationContext, Frame};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceKind {
    TextFile,
    DocxExtract,
    Scratch,
}

#[derive(Clone, Debug)]
struct TemplateTab {
    id: u64,
    title: String,
    source_kind: SourceKind,
    origin_path: Option<PathBuf>,
    save_path: Option<PathBuf>,
    text: String,
    dirty: bool,
    lint: LintResult,
}

impl TemplateTab {
    fn new(
        id: u64,
        title: String,
        source_kind: SourceKind,
        origin_path: Option<PathBuf>,
        save_path: Option<PathBuf>,
        text: String,
    ) -> Self {
        let lint = lint::lint_template(&text);
        Self {
            id,
            title,
            source_kind,
            origin_path,
            save_path,
            text,
            dirty: false,
            lint,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Snippet {
    id: u64,
    name: String,
    body: String,
    #[serde(default)]
    folder: String,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct SnippetStore {
    snippets: Vec<Snippet>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiffSourceMode {
    Tab,
    File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WsMode {
    Off,
    Host,
    Client,
}

pub struct OlehGroovyEditorApp {
    tabs: Vec<TemplateTab>,
    current_tab: usize,
    next_tab_id: u64,
    next_snippet_id: u64,
    status: String,
    machine_identity: String,
    last_open_dir: Option<PathBuf>,

    search_input: String,
    replace_input: String,

    snippet_store: SnippetStore,
    snippet_name_input: String,
    snippet_body_input: String,
    snippet_folder_input: String,
    snippet_tags_input: String,
    snippet_filter_folder: String,
    snippet_filter_tag: String,

    show_diff_window: bool,
    diff_mode: DiffSourceMode,
    diff_target_tab: usize,
    diff_file_path: String,
    diff_rows: Vec<DiffLine>,

    show_harness_window: bool,
    harness_vars_json: String,
    harness_output: String,
    harness_unresolved: Vec<String>,
    harness_warnings: Vec<String>,
    harness_auto_render: bool,

    show_git_window: bool,
    git_repo_path: String,
    git_commit_message: String,
    git_merge_branch: String,
    git_log: String,

    show_collab_window: bool,
    collab_db_path: String,
    collab_author: String,
    collab_body_input: String,
    collab_quote_input: String,
    collab_reply_to_input: String,
    collab_limit: usize,
    collab_messages: Vec<CollabMessage>,

    ws_mode: WsMode,
    ws_addr: String,
    ws_handle: Option<WsHandle>,
    ws_log: Vec<String>,
}

impl OlehGroovyEditorApp {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let machine_identity = detect_machine_identity();
        let shared_db = cwd
            .join("shared")
            .join("oleh_groovy_editor_messages.sqlite");

        let mut app = Self {
            tabs: Vec::new(),
            current_tab: 0,
            next_tab_id: 1,
            next_snippet_id: 1,
            status: "Ready.".to_string(),
            machine_identity: machine_identity.clone(),
            last_open_dir: None,

            search_input: String::new(),
            replace_input: String::new(),

            snippet_store: SnippetStore::default(),
            snippet_name_input: String::new(),
            snippet_body_input: String::new(),
            snippet_folder_input: String::new(),
            snippet_tags_input: String::new(),
            snippet_filter_folder: String::new(),
            snippet_filter_tag: String::new(),

            show_diff_window: false,
            diff_mode: DiffSourceMode::Tab,
            diff_target_tab: 0,
            diff_file_path: String::new(),
            diff_rows: Vec::new(),

            show_harness_window: false,
            harness_vars_json: "{}".to_string(),
            harness_output: String::new(),
            harness_unresolved: Vec::new(),
            harness_warnings: Vec::new(),
            harness_auto_render: true,

            show_git_window: false,
            git_repo_path: cwd.display().to_string(),
            git_commit_message: "Update templates".to_string(),
            git_merge_branch: "main".to_string(),
            git_log: String::new(),

            show_collab_window: false,
            collab_db_path: shared_db.display().to_string(),
            collab_author: machine_identity,
            collab_body_input: String::new(),
            collab_quote_input: String::new(),
            collab_reply_to_input: String::new(),
            collab_limit: 200,
            collab_messages: Vec::new(),

            ws_mode: WsMode::Off,
            ws_addr: "127.0.0.1:9002".to_string(),
            ws_handle: None,
            ws_log: Vec::new(),
        };

        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        app.load_snippets();
        app.create_empty_tab();
        app.refresh_collab_messages();
        app.run_harness_current();
        app
    }

    fn current_tab(&self) -> Option<&TemplateTab> {
        self.tabs.get(self.current_tab)
    }

    fn current_tab_mut(&mut self) -> Option<&mut TemplateTab> {
        self.tabs.get_mut(self.current_tab)
    }

    fn bump_tab_id(&mut self) -> u64 {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        id
    }

    fn bump_snippet_id(&mut self) -> u64 {
        let id = self.next_snippet_id;
        self.next_snippet_id += 1;
        id
    }

    fn create_empty_tab(&mut self) {
        let id = self.bump_tab_id();
        let title = format!("untitled-{id}.groovy");
        self.tabs.push(TemplateTab::new(
            id,
            title,
            SourceKind::Scratch,
            None,
            None,
            String::new(),
        ));
        self.current_tab = self.tabs.len().saturating_sub(1);
    }

    fn open_file_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("Templates", &["groovy", "txt", "docx"]);
        if let Some(dir) = self.last_open_dir.clone() {
            dialog = dialog.set_directory(dir);
        }

        if let Some(path) = dialog.pick_file() {
            self.open_path(&path);
        }
    }

    fn open_path(&mut self, path: &Path) {
        let ext = path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        let text_result = if ext == "docx" {
            docx::extract_template_text(path)
        } else {
            fs::read_to_string(path).map_err(|e| e.to_string())
        };

        match text_result {
            Ok(text) => {
                let id = self.bump_tab_id();
                let title = path
                    .file_name()
                    .and_then(|v| v.to_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("file-{id}"));
                let source_kind = if ext == "docx" {
                    SourceKind::DocxExtract
                } else {
                    SourceKind::TextFile
                };
                let save_path = if source_kind == SourceKind::DocxExtract {
                    None
                } else {
                    Some(path.to_path_buf())
                };

                self.tabs.push(TemplateTab::new(
                    id,
                    title,
                    source_kind,
                    Some(path.to_path_buf()),
                    save_path,
                    text,
                ));
                self.current_tab = self.tabs.len().saturating_sub(1);
                self.last_open_dir = path.parent().map(Path::to_path_buf);
                self.status = format!("Opened {}", path.display());
                self.run_harness_current();
            }
            Err(err) => {
                self.status = format!("Open failed: {err}");
            }
        }
    }

    fn save_current_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }

        let idx = self.current_tab;
        if let Some(path) = self.tabs[idx].save_path.clone() {
            self.save_tab_to_path(idx, path);
        } else {
            self.save_current_tab_as();
        }
    }

    fn save_current_tab_as(&mut self) {
        if self.tabs.is_empty() {
            return;
        }

        let idx = self.current_tab;
        let suggested = suggest_save_name(&self.tabs[idx]);

        let mut dialog = rfd::FileDialog::new()
            .add_filter("Groovy/Txt", &["groovy", "txt"])
            .set_file_name(suggested.as_str());
        if let Some(dir) = self.last_open_dir.clone() {
            dialog = dialog.set_directory(dir);
        }

        if let Some(path) = dialog.save_file() {
            self.save_tab_to_path(idx, path);
        }
    }

    fn save_tab_to_path(&mut self, tab_index: usize, path: PathBuf) {
        if tab_index >= self.tabs.len() {
            return;
        }
        let text = self.tabs[tab_index].text.clone();
        match fs::write(&path, text) {
            Ok(()) => {
                self.tabs[tab_index].dirty = false;
                self.tabs[tab_index].save_path = Some(path.clone());
                self.tabs[tab_index].title = path
                    .file_name()
                    .and_then(|v| v.to_str())
                    .unwrap_or("template.groovy")
                    .to_string();
                self.last_open_dir = path.parent().map(Path::to_path_buf);
                self.status = format!("Saved {}", path.display());
            }
            Err(err) => {
                self.status = format!("Save failed: {err}");
            }
        }
    }

    fn run_lint_current(&mut self) {
        if let Some(tab) = self.current_tab_mut() {
            tab.lint = lint::lint_template(&tab.text);
            self.status = format!("Lint complete: {} issues.", tab.lint.diagnostics.len());
        }
    }

    fn replace_all_current(&mut self) {
        let find = self.search_input.clone();
        let replace = self.replace_input.clone();

        if find.is_empty() {
            self.status = "Replace skipped: search text is empty.".to_string();
            return;
        }

        let mut replaced = 0usize;
        if let Some(tab) = self.current_tab_mut() {
            replaced = tab.text.matches(find.as_str()).count();
            if replaced > 0 {
                tab.text = tab.text.replace(find.as_str(), replace.as_str());
                tab.dirty = true;
                tab.lint = lint::lint_template(&tab.text);
            }
        }

        self.status = format!("Replace complete: {replaced} matches.");
        if replaced > 0 && self.harness_auto_render {
            self.run_harness_current();
        }
    }

    fn load_snippets(&mut self) {
        let path = snippets_path();
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => return,
        };

        match serde_json::from_str::<SnippetStore>(&text) {
            Ok(store) => {
                self.snippet_store = store;
                let mut max_id = 0u64;
                for s in &self.snippet_store.snippets {
                    if s.id > max_id {
                        max_id = s.id;
                    }
                }
                self.next_snippet_id = max_id.saturating_add(1);
            }
            Err(err) => {
                self.status = format!("Could not parse snippets.json: {err}");
            }
        }
    }

    fn save_snippets(&mut self) {
        let path = snippets_path();
        if let Some(parent) = path.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                self.status = format!("Snippet folder create failed: {err}");
                return;
            }
        }

        match serde_json::to_string_pretty(&self.snippet_store) {
            Ok(json) => {
                if let Err(err) = fs::write(&path, json) {
                    self.status = format!("Snippet save failed: {err}");
                }
            }
            Err(err) => {
                self.status = format!("Snippet serialization failed: {err}");
            }
        }
    }

    fn add_snippet_from_inputs(&mut self) {
        let name = self.snippet_name_input.trim().to_string();
        let body = self.snippet_body_input.clone();
        let folder = self.snippet_folder_input.trim().to_string();
        let tags = parse_tags(self.snippet_tags_input.as_str());

        if name.is_empty() {
            self.status = "Snippet name is required.".to_string();
            return;
        }
        if body.trim().is_empty() {
            self.status = "Snippet body is empty.".to_string();
            return;
        }

        let snippet = Snippet {
            id: self.bump_snippet_id(),
            name,
            body,
            folder,
            tags,
        };
        self.snippet_store.snippets.push(snippet);
        self.snippet_store.snippets.sort_by(|a, b| {
            a.folder
                .to_ascii_lowercase()
                .cmp(&b.folder.to_ascii_lowercase())
                .then(
                    a.name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase()),
                )
        });
        self.save_snippets();
        self.status = "Snippet added.".to_string();
    }

    fn remove_snippet(&mut self, id: u64) {
        self.snippet_store.snippets.retain(|s| s.id != id);
        self.save_snippets();
        self.status = format!("Snippet {id} removed.");
    }

    fn copy_text_to_clipboard(&mut self, text: &str) {
        match Clipboard::new().and_then(|mut cb| cb.set_text(text.to_string())) {
            Ok(()) => {
                self.status = "Copied to clipboard.".to_string();
            }
            Err(err) => {
                self.status = format!("Clipboard error: {err}");
            }
        }
    }

    fn capture_clipboard_to_snippet(&mut self) {
        match Clipboard::new().and_then(|mut cb| cb.get_text()) {
            Ok(text) => {
                self.snippet_body_input = text;
                if self.snippet_name_input.trim().is_empty() {
                    self.snippet_name_input = format!("clipboard-{}", timestamp_id());
                }
                if self.snippet_folder_input.trim().is_empty() {
                    self.snippet_folder_input = "clipboard".to_string();
                }
                self.status = "Clipboard captured into snippet draft.".to_string();
            }
            Err(err) => {
                self.status = format!("Cannot read clipboard: {err}");
            }
        }
    }

    fn insert_snippet_body(&mut self, body: &str) {
        if let Some(tab) = self.current_tab_mut() {
            if !tab.text.is_empty() && !tab.text.ends_with('\n') {
                tab.text.push('\n');
            }
            tab.text.push_str(body);
            tab.dirty = true;
            tab.lint = lint::lint_template(&tab.text);
            self.status = "Snippet inserted.".to_string();
            if self.harness_auto_render {
                self.run_harness_current();
            }
        }
    }

    fn filtered_snippets(&self) -> Vec<Snippet> {
        let folder_filter = self.snippet_filter_folder.trim().to_ascii_lowercase();
        let tag_filter = self.snippet_filter_tag.trim().to_ascii_lowercase();

        self.snippet_store
            .snippets
            .iter()
            .filter(|s| {
                let folder_ok = folder_filter.is_empty()
                    || s.folder
                        .to_ascii_lowercase()
                        .contains(folder_filter.as_str());
                let tag_ok = tag_filter.is_empty()
                    || s.tags
                        .iter()
                        .any(|t| t.to_ascii_lowercase().contains(tag_filter.as_str()));
                folder_ok && tag_ok
            })
            .cloned()
            .collect()
    }

    fn recompute_diff(&mut self) {
        if self.tabs.is_empty() {
            self.diff_rows.clear();
            return;
        }

        let left = self.tabs[self.current_tab].text.clone();
        let right = match self.diff_mode {
            DiffSourceMode::Tab => {
                if self.diff_target_tab >= self.tabs.len() {
                    self.status = "Diff target tab out of range.".to_string();
                    return;
                }
                self.tabs[self.diff_target_tab].text.clone()
            }
            DiffSourceMode::File => {
                let path = PathBuf::from(self.diff_file_path.trim());
                if path.as_os_str().is_empty() {
                    self.status = "Diff file path is empty.".to_string();
                    return;
                }
                match fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(err) => {
                        self.status = format!("Diff file read error: {err}");
                        return;
                    }
                }
            }
        };

        self.diff_rows = diff_engine::side_by_side_diff(&left, &right);
        self.status = format!("Diff ready: {} rows.", self.diff_rows.len());
    }

    fn run_harness_current(&mut self) {
        let text = match self.current_tab() {
            Some(tab) => tab.text.clone(),
            None => return,
        };

        let result = render::render_template_preview(&text, &self.harness_vars_json);
        self.harness_output = result.output;
        self.harness_unresolved = result.unresolved;
        self.harness_warnings = result.warnings;
    }

    fn load_sample_vars_from_template(&mut self) {
        let text = match self.current_tab() {
            Some(tab) => tab.text.clone(),
            None => return,
        };
        self.harness_vars_json = render::placeholders_as_sample_json(&text);
        self.status = "Generated sample JSON from placeholders.".to_string();
        if self.harness_auto_render {
            self.run_harness_current();
        }
    }

    fn repo_path(&self) -> PathBuf {
        PathBuf::from(self.git_repo_path.trim())
    }

    fn append_git_log(&mut self, label: &str, result: Result<String, String>) {
        let mut line = String::new();
        line.push_str("== ");
        line.push_str(label);
        line.push_str(" ==\n");
        match result {
            Ok(text) => {
                line.push_str(text.trim());
                self.status = format!("{label} succeeded.");
            }
            Err(err) => {
                line.push_str(err.trim());
                self.status = format!("{label} failed.");
            }
        }
        line.push_str("\n\n");
        self.git_log.push_str(&line);
    }

    fn run_git_status(&mut self) {
        let path = self.repo_path();
        self.append_git_log("git status", git_ops::status(&path));
    }

    fn run_git_fetch(&mut self) {
        let path = self.repo_path();
        self.append_git_log("git fetch --all --prune", git_ops::fetch(&path));
    }

    fn run_git_pull_rebase(&mut self) {
        let path = self.repo_path();
        self.append_git_log("git pull --rebase", git_ops::pull_rebase(&path));
    }

    fn run_git_push(&mut self) {
        let path = self.repo_path();
        self.append_git_log("git push", git_ops::push(&path));
    }

    fn run_git_commit_push(&mut self) {
        let path = self.repo_path();
        let msg = self.git_commit_message.clone();
        self.append_git_log("git commit + push", git_ops::commit_and_push(&path, &msg));
    }

    fn run_git_merge(&mut self) {
        let path = self.repo_path();
        let branch = self.git_merge_branch.clone();
        self.append_git_log(
            &format!("git merge --no-edit {branch}"),
            git_ops::merge(&path, branch.as_str()),
        );
    }

    fn collab_db_path_buf(&self) -> PathBuf {
        PathBuf::from(self.collab_db_path.trim())
    }

    fn refresh_collab_messages(&mut self) {
        let path = self.collab_db_path_buf();
        match collab::list_messages(&path, self.collab_limit) {
            Ok(messages) => {
                self.collab_messages = messages;
            }
            Err(err) => {
                self.status = format!("Collab load failed: {err}");
            }
        }
    }

    fn post_collab_message(&mut self) {
        let path = self.collab_db_path_buf();
        let body = self.collab_body_input.trim().to_string();
        if body.is_empty() {
            self.status = "Message is empty.".to_string();
            return;
        }

        let author = if self.collab_author.trim().is_empty() {
            self.machine_identity.clone()
        } else {
            self.collab_author.trim().to_string()
        };

        let parent_id = self.collab_reply_to_input.trim().parse::<i64>().ok();
        match collab::add_message(
            &path,
            author.as_str(),
            body.as_str(),
            self.collab_quote_input.as_str(),
            parent_id,
        ) {
            Ok(id) => {
                self.collab_body_input.clear();
                self.collab_reply_to_input.clear();
                self.refresh_collab_messages();
                self.send_ws_event("collab:new", format!("message:{id}"));
                self.status = "Message posted.".to_string();
            }
            Err(err) => {
                self.status = format!("Post failed: {err}");
            }
        }
    }

    fn update_collab_decision(&mut self, id: i64, decision: Decision) {
        let path = self.collab_db_path_buf();
        match collab::set_decision(&path, id, decision) {
            Ok(()) => {
                self.refresh_collab_messages();
                self.send_ws_event("collab:update", format!("decision:{id}"));
                self.status = "Decision updated.".to_string();
            }
            Err(err) => {
                self.status = format!("Decision update failed: {err}");
            }
        }
    }

    fn start_ws_host(&mut self) {
        self.stop_ws();
        match collab::start_server(self.ws_addr.trim()) {
            Ok(handle) => {
                self.ws_mode = WsMode::Host;
                self.ws_handle = Some(handle);
                self.ws_log.push(format!("Hosting at {}", self.ws_addr));
                self.status = "WebSocket host started.".to_string();
            }
            Err(err) => {
                self.status = format!("WS host failed: {err}");
            }
        }
    }

    fn start_ws_client(&mut self) {
        self.stop_ws();
        let url = if self.ws_addr.starts_with("ws://") || self.ws_addr.starts_with("wss://") {
            self.ws_addr.clone()
        } else {
            format!("ws://{}", self.ws_addr)
        };

        match collab::start_client(url.as_str()) {
            Ok(handle) => {
                self.ws_mode = WsMode::Client;
                self.ws_handle = Some(handle);
                self.ws_log.push(format!("Client connecting to {url}"));
                self.status = "WebSocket client started.".to_string();
            }
            Err(err) => {
                self.status = format!("WS client failed: {err}");
            }
        }
    }

    fn stop_ws(&mut self) {
        if let Some(handle) = self.ws_handle.take() {
            handle.stop();
        }
        self.ws_mode = WsMode::Off;
    }

    fn send_ws_event(&mut self, event: &str, payload: String) {
        let text = match serde_json::to_string(&WsEnvelope {
            event: event.to_string(),
            payload,
        }) {
            Ok(v) => v,
            Err(err) => {
                self.status = format!("WS event encode failed: {err}");
                return;
            }
        };

        if let Some(handle) = &self.ws_handle {
            handle.send(text);
        }
    }

    fn poll_ws_events(&mut self) {
        let mut events = Vec::new();
        if let Some(handle) = &self.ws_handle {
            while let Some(msg) = handle.try_recv() {
                events.push(msg);
            }
        }

        for event in events {
            self.ws_log.push(event.clone());
            if self.ws_log.len() > 300 {
                let drain_len = self.ws_log.len() - 300;
                self.ws_log.drain(0..drain_len);
            }

            if let Ok(env) = serde_json::from_str::<WsEnvelope>(&event) {
                if env.event.starts_with("collab:") {
                    self.refresh_collab_messages();
                }
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let new_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::N);
        let open_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::O);
        let save_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::S);
        let save_as_shortcut = egui::KeyboardShortcut::new(
            egui::Modifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            },
            egui::Key::S,
        );
        let lint_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::F7);

        if ctx.input_mut(|i| i.consume_shortcut(&new_shortcut)) {
            self.create_empty_tab();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&open_shortcut)) {
            self.open_file_dialog();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&save_as_shortcut)) {
            self.save_current_tab_as();
        } else if ctx.input_mut(|i| i.consume_shortcut(&save_shortcut)) {
            self.save_current_tab();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&lint_shortcut)) {
            self.run_lint_current();
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped {
            if let Some(path) = f.path {
                self.open_path(&path);
            }
        }
    }

    fn ui_top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("New").clicked() {
                self.create_empty_tab();
            }
            if ui.button("Open").clicked() {
                self.open_file_dialog();
            }
            if ui.button("Save").clicked() {
                self.save_current_tab();
            }
            if ui.button("Save As").clicked() {
                self.save_current_tab_as();
            }
            if ui.button("Lint (F7)").clicked() {
                self.run_lint_current();
            }

            ui.separator();
            if ui.button("Split Diff").clicked() {
                self.show_diff_window = true;
                self.recompute_diff();
            }
            if ui.button("Template Harness").clicked() {
                self.show_harness_window = true;
                self.run_harness_current();
            }
            if ui.button("Git").clicked() {
                self.show_git_window = true;
            }
            if ui.button("Collab").clicked() {
                self.show_collab_window = true;
            }

            ui.separator();
            ui.label("Find:");
            ui.add(egui::TextEdit::singleline(&mut self.search_input).desired_width(160.0));
            ui.label("Replace:");
            ui.add(egui::TextEdit::singleline(&mut self.replace_input).desired_width(160.0));
            if ui.button("Replace All").clicked() {
                self.replace_all_current();
            }
        });
    }

    fn ui_tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for idx in 0..self.tabs.len() {
                let tab = &self.tabs[idx];
                let mut title = tab.title.clone();
                if tab.dirty {
                    title.push('*');
                }
                if ui
                    .selectable_label(self.current_tab == idx, title)
                    .clicked()
                {
                    self.current_tab = idx;
                    if self.harness_auto_render {
                        self.run_harness_current();
                    }
                }
            }
        });
    }

    fn ui_editor(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        if let Some(tab) = self.current_tab_mut() {
            let mut layouter = |ui: &egui::Ui, text: &dyn TextBuffer, wrap_width: f32| {
                let mut job = highlight::groovy_layout(text.as_str(), ui.visuals().dark_mode);
                job.wrap.max_width = wrap_width;
                ui.fonts_mut(|f| f.layout_job(job))
            };

            let response = ui.add(
                TextEdit::multiline(&mut tab.text)
                    .font(TextStyle::Monospace)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(40)
                    .layouter(&mut layouter),
            );

            if response.changed() {
                tab.dirty = true;
                tab.lint = lint::lint_template(&tab.text);
                changed = true;
            }
        }

        if changed && self.harness_auto_render {
            self.run_harness_current();
        }
    }

    fn ui_diagnostics(&mut self, ui: &mut egui::Ui) {
        let Some(tab) = self.current_tab() else {
            ui.label("No active tab.");
            return;
        };

        ui.heading("Diagnostics");
        ui.label(format!(
            "Source: {}",
            match tab.source_kind {
                SourceKind::TextFile => "Text file",
                SourceKind::DocxExtract => "DOCX extracted",
                SourceKind::Scratch => "Scratch",
            }
        ));
        if let Some(origin) = &tab.origin_path {
            ui.label(format!("Origin: {}", origin.display()));
        }
        if let Some(save) = &tab.save_path {
            ui.label(format!("Save target: {}", save.display()));
        }
        ui.label(format!("Tab ID: {}", tab.id));
        ui.separator();

        if tab.lint.diagnostics.is_empty() {
            ui.label(RichText::new("No lint issues.").color(Color32::LIGHT_GREEN));
        } else {
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    for d in &tab.lint.diagnostics {
                        let color = match d.severity {
                            Severity::Error => Color32::from_rgb(250, 95, 90),
                            Severity::Warning => Color32::from_rgb(242, 194, 72),
                            Severity::Info => Color32::from_rgb(110, 200, 255),
                        };
                        ui.label(
                            RichText::new(format!(
                                "[{:?}] {}:{} {}",
                                d.severity, d.line, d.column, d.message
                            ))
                            .color(color),
                        );
                    }
                });
        }

        ui.separator();
        ui.heading("Placeholders");
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .show(ui, |ui| {
                for p in &tab.lint.placeholders {
                    ui.monospace(p);
                }
            });
    }

    fn ui_snippets(&mut self, ui: &mut egui::Ui) {
        ui.heading("Snippet Tray");
        ui.label("Folders + tags for reusable Groovy blocks.");

        ui.horizontal(|ui| {
            ui.label("Folder");
            ui.add(
                egui::TextEdit::singleline(&mut self.snippet_filter_folder).desired_width(120.0),
            );
            ui.label("Tag");
            ui.add(egui::TextEdit::singleline(&mut self.snippet_filter_tag).desired_width(120.0));
        });

        ui.separator();
        ui.label("New snippet");
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut self.snippet_name_input).desired_width(160.0));
        });
        ui.horizontal(|ui| {
            ui.label("Folder");
            ui.add(egui::TextEdit::singleline(&mut self.snippet_folder_input).desired_width(120.0));
            ui.label("Tags");
            ui.add(egui::TextEdit::singleline(&mut self.snippet_tags_input).desired_width(180.0));
        });
        ui.add(
            TextEdit::multiline(&mut self.snippet_body_input)
                .font(TextStyle::Monospace)
                .desired_rows(5)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            if ui.button("Capture Clipboard").clicked() {
                self.capture_clipboard_to_snippet();
            }
            if ui.button("Add Snippet").clicked() {
                self.add_snippet_from_inputs();
            }
            if ui.button("Clear Draft").clicked() {
                self.snippet_name_input.clear();
                self.snippet_body_input.clear();
                self.snippet_folder_input.clear();
                self.snippet_tags_input.clear();
            }
        });

        ui.separator();
        ui.heading("Saved snippets");
        let snippets = self.filtered_snippets();
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut to_delete = None::<u64>;
            let mut to_insert = None::<String>;
            let mut to_copy = None::<String>;

            for s in snippets {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(s.name.clone()).strong());
                        if !s.folder.is_empty() {
                            ui.label(RichText::new(format!("[{}]", s.folder)).italics());
                        }
                        if !s.tags.is_empty() {
                            ui.label(format!("#{}", s.tags.join(" #")));
                        }
                    });

                    let mut preview = s.body.clone();
                    ui.add(
                        TextEdit::multiline(&mut preview)
                            .font(TextStyle::Monospace)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY)
                            .interactive(false),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Insert").clicked() {
                            to_insert = Some(s.body.clone());
                        }
                        if ui.button("Copy").clicked() {
                            to_copy = Some(s.body.clone());
                        }
                        if ui.button("Delete").clicked() {
                            to_delete = Some(s.id);
                        }
                    });
                });
                ui.add_space(4.0);
            }

            if let Some(body) = to_insert {
                self.insert_snippet_body(body.as_str());
            }
            if let Some(body) = to_copy {
                self.copy_text_to_clipboard(body.as_str());
            }
            if let Some(id) = to_delete {
                self.remove_snippet(id);
            }
        });
    }

    fn ui_diff_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_diff_window;
        egui::Window::new("Split-View Diff")
            .open(&mut open)
            .default_width(1220.0)
            .default_height(700.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.diff_mode, DiffSourceMode::Tab, "Against tab");
                    ui.radio_value(&mut self.diff_mode, DiffSourceMode::File, "Against file");
                });

                match self.diff_mode {
                    DiffSourceMode::Tab => {
                        egui::ComboBox::from_label("Target tab")
                            .selected_text(
                                self.tabs
                                    .get(self.diff_target_tab)
                                    .map(|t| t.title.clone())
                                    .unwrap_or_else(|| "Select tab".to_string()),
                            )
                            .show_ui(ui, |ui| {
                                for idx in 0..self.tabs.len() {
                                    ui.selectable_value(
                                        &mut self.diff_target_tab,
                                        idx,
                                        self.tabs[idx].title.clone(),
                                    );
                                }
                            });
                    }
                    DiffSourceMode::File => {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.diff_file_path)
                                    .desired_width(500.0),
                            );
                            if ui.button("Browse").clicked() {
                                if let Some(path) = rfd::FileDialog::new().pick_file() {
                                    self.diff_file_path = path.display().to_string();
                                }
                            }
                        });
                    }
                }

                ui.horizontal(|ui| {
                    if ui.button("Recompute Diff").clicked() {
                        self.recompute_diff();
                    }
                    ui.label(format!("rows: {}", self.diff_rows.len()));
                });

                ui.separator();
                egui::ScrollArea::both().show(ui, |ui| {
                    egui::Grid::new("split_diff_grid")
                        .num_columns(2)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Current tab").strong());
                            ui.label(RichText::new("Target").strong());
                            ui.end_row();

                            for row in &self.diff_rows {
                                let color = match row.kind {
                                    DiffKind::Equal => Color32::GRAY,
                                    DiffKind::Added => Color32::from_rgb(100, 220, 130),
                                    DiffKind::Removed => Color32::from_rgb(255, 120, 120),
                                    DiffKind::Replaced => Color32::from_rgb(255, 198, 103),
                                };

                                ui.label(
                                    RichText::new(row.left.as_deref().unwrap_or(""))
                                        .color(color)
                                        .monospace(),
                                );
                                ui.label(
                                    RichText::new(row.right.as_deref().unwrap_or(""))
                                        .color(color)
                                        .monospace(),
                                );
                                ui.end_row();
                            }
                        });
                });
            });
        self.show_diff_window = open;
    }

    fn ui_harness_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_harness_window;
        egui::Window::new("Quick Template Test Harness")
            .open(&mut open)
            .default_width(1260.0)
            .default_height(760.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.harness_auto_render, "Auto render");
                    if ui.button("Render now").clicked() {
                        self.run_harness_current();
                    }
                    if ui.button("Generate sample vars").clicked() {
                        self.load_sample_vars_from_template();
                    }
                });

                ui.columns(2, |cols| {
                    cols[0].label(RichText::new("Variables JSON").strong());
                    let vars_changed = cols[0]
                        .add(
                            TextEdit::multiline(&mut self.harness_vars_json)
                                .font(TextStyle::Monospace)
                                .desired_rows(26)
                                .desired_width(f32::INFINITY),
                        )
                        .changed();
                    if vars_changed && self.harness_auto_render {
                        self.run_harness_current();
                    }

                    cols[0].separator();
                    cols[0].label(RichText::new("Warnings").strong());
                    if self.harness_warnings.is_empty() {
                        cols[0].label("None");
                    } else {
                        for w in &self.harness_warnings {
                            cols[0].label(RichText::new(w).color(Color32::from_rgb(242, 194, 72)));
                        }
                    }

                    cols[0].separator();
                    cols[0].label(RichText::new("Unresolved placeholders").strong());
                    if self.harness_unresolved.is_empty() {
                        cols[0].label(RichText::new("None").color(Color32::LIGHT_GREEN));
                    } else {
                        for p in &self.harness_unresolved {
                            cols[0].monospace(p);
                        }
                    }

                    cols[1].label(RichText::new("Rendered Output").strong());
                    cols[1].add(
                        TextEdit::multiline(&mut self.harness_output)
                            .font(TextStyle::Monospace)
                            .desired_rows(20)
                            .desired_width(f32::INFINITY)
                            .interactive(false),
                    );

                    cols[1].separator();
                    cols[1].label(RichText::new("DOCX-style Preview").strong());
                    cols[1].group(|ui| {
                        ui.visuals_mut().override_text_color = Some(Color32::BLACK);
                        egui::ScrollArea::vertical()
                            .max_height(280.0)
                            .show(ui, |ui| {
                                for line in self.harness_output.lines() {
                                    if line.trim().is_empty() {
                                        ui.add_space(6.0);
                                    } else {
                                        ui.label(RichText::new(line).color(Color32::BLACK));
                                    }
                                }
                            });
                    });
                });
            });
        self.show_harness_window = open;
    }

    fn ui_git_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_git_window;
        egui::Window::new("Git Panel")
            .open(&mut open)
            .default_width(980.0)
            .default_height(620.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Repo:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.git_repo_path).desired_width(600.0),
                    );
                    if ui.button("Browse").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.git_repo_path = path.display().to_string();
                        }
                    }
                });

                ui.horizontal_wrapped(|ui| {
                    if ui.button("Status").clicked() {
                        self.run_git_status();
                    }
                    if ui.button("Fetch").clicked() {
                        self.run_git_fetch();
                    }
                    if ui.button("Pull --rebase").clicked() {
                        self.run_git_pull_rebase();
                    }
                    if ui.button("Push").clicked() {
                        self.run_git_push();
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Commit:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.git_commit_message)
                            .desired_width(420.0),
                    );
                    if ui.button("Commit + Push").clicked() {
                        self.run_git_commit_push();
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Merge branch:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.git_merge_branch).desired_width(240.0),
                    );
                    if ui.button("Merge").clicked() {
                        self.run_git_merge();
                    }
                });

                ui.separator();
                ui.label(RichText::new("Git log").strong());
                ui.add(
                    TextEdit::multiline(&mut self.git_log)
                        .font(TextStyle::Monospace)
                        .desired_rows(24)
                        .desired_width(f32::INFINITY)
                        .interactive(false),
                );
            });
        self.show_git_window = open;
    }

    fn ui_collab_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_collab_window;
        egui::Window::new("Collaboration (SQLite + WebSocket)")
            .open(&mut open)
            .default_width(1180.0)
            .default_height(760.0)
            .show(ctx, |ui| {
                ui.label(format!("Machine identity: {}", self.machine_identity));

                ui.horizontal(|ui| {
                    ui.label("DB path:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.collab_db_path).desired_width(560.0),
                    );
                    if ui.button("Browse").clicked() {
                        if let Some(path) = rfd::FileDialog::new().save_file() {
                            self.collab_db_path = path.display().to_string();
                        }
                    }
                    if ui.button("Refresh").clicked() {
                        self.refresh_collab_messages();
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Author:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.collab_author).desired_width(280.0),
                    );
                    ui.label("Reply-to id:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.collab_reply_to_input)
                            .desired_width(80.0),
                    );
                    ui.label("Limit:");
                    ui.add(egui::DragValue::new(&mut self.collab_limit).range(10..=1000));
                });

                ui.label("Message");
                ui.add(
                    TextEdit::multiline(&mut self.collab_body_input)
                        .font(TextStyle::Monospace)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
                ui.label("Quoted code");
                ui.add(
                    TextEdit::multiline(&mut self.collab_quote_input)
                        .font(TextStyle::Monospace)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );

                ui.horizontal(|ui| {
                    if ui.button("Post message").clicked() {
                        self.post_collab_message();
                    }
                    if ui.button("Clear input").clicked() {
                        self.collab_body_input.clear();
                        self.collab_quote_input.clear();
                        self.collab_reply_to_input.clear();
                    }
                });

                ui.separator();
                ui.heading("Realtime network");
                ui.horizontal(|ui| {
                    ui.label("WS addr/url:");
                    ui.add(egui::TextEdit::singleline(&mut self.ws_addr).desired_width(300.0));
                    if ui.button("Host").clicked() {
                        self.start_ws_host();
                    }
                    if ui.button("Client").clicked() {
                        self.start_ws_client();
                    }
                    if ui.button("Stop").clicked() {
                        self.stop_ws();
                    }
                    ui.label(format!("Mode: {:?}", self.ws_mode));
                });

                egui::CollapsingHeader::new("WebSocket log")
                    .default_open(false)
                    .show(ui, |ui| {
                        let mut log_text = self.ws_log.join("\n");
                        ui.add(
                            TextEdit::multiline(&mut log_text)
                                .font(TextStyle::Monospace)
                                .desired_rows(5)
                                .desired_width(f32::INFINITY)
                                .interactive(false),
                        );
                    });

                ui.separator();
                ui.heading("Messages");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut action: Option<(i64, Decision)> = None;
                    let mut draft_quote: Option<(String, String)> = None;
                    for m in &self.collab_messages {
                        ui.group(|ui| {
                            let decision_text = match m.decision {
                                Decision::Pending => ("PENDING", Color32::from_rgb(242, 194, 72)),
                                Decision::Approved => {
                                    ("APPROVED", Color32::from_rgb(100, 220, 130))
                                }
                                Decision::Denied => ("DENIED", Color32::from_rgb(255, 120, 120)),
                            };
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "#{} {} @ {}",
                                        m.id, m.author, m.created_at
                                    ))
                                    .strong(),
                                );
                                ui.label(RichText::new(decision_text.0).color(decision_text.1));
                                if let Some(parent) = m.parent_id {
                                    ui.label(format!("reply-to #{parent}"));
                                }
                            });
                            ui.label(m.body.clone());
                            if !m.code_quote.trim().is_empty() {
                                let mut quote = m.code_quote.clone();
                                ui.add(
                                    TextEdit::multiline(&mut quote)
                                        .font(TextStyle::Monospace)
                                        .desired_rows(3)
                                        .desired_width(f32::INFINITY)
                                        .interactive(false),
                                );
                            }
                            ui.horizontal(|ui| {
                                if ui.button("Approve").clicked() {
                                    action = Some((m.id, Decision::Approved));
                                }
                                if ui.button("Deny").clicked() {
                                    action = Some((m.id, Decision::Denied));
                                }
                                if ui.button("Reset").clicked() {
                                    action = Some((m.id, Decision::Pending));
                                }
                                if ui.button("Quote to draft").clicked() {
                                    draft_quote = Some((m.body.clone(), m.id.to_string()));
                                }
                            });
                        });
                        ui.add_space(6.0);
                    }

                    if let Some((id, decision)) = action {
                        self.update_collab_decision(id, decision);
                    }
                    if let Some((quote, reply_id)) = draft_quote {
                        self.collab_quote_input = quote;
                        self.collab_reply_to_input = reply_id;
                    }
                });
            });
        self.show_collab_window = open;
    }
}

impl App for OlehGroovyEditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        self.handle_shortcuts(ctx);
        self.handle_dropped_files(ctx);
        self.poll_ws_events();

        egui::TopBottomPanel::top("toolbar_top").show(ctx, |ui| {
            self.ui_top_bar(ui);
            ui.separator();
            self.ui_tab_bar(ui);
        });

        egui::SidePanel::left("snippet_panel")
            .resizable(true)
            .default_width(380.0)
            .show(ctx, |ui| {
                self.ui_snippets(ui);
            });

        egui::SidePanel::right("diagnostics_panel")
            .resizable(true)
            .default_width(370.0)
            .show(ctx, |ui| {
                self.ui_diagnostics(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.ui_editor(ui);
        });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(self.status.clone()).color(Color32::from_rgb(130, 210, 255)),
                );
            });
        });

        if self.show_diff_window {
            self.ui_diff_window(ctx);
        }
        if self.show_harness_window {
            self.ui_harness_window(ctx);
        }
        if self.show_git_window {
            self.ui_git_window(ctx);
        }
        if self.show_collab_window {
            self.ui_collab_window(ctx);
        }
    }
}

impl Drop for OlehGroovyEditorApp {
    fn drop(&mut self) {
        self.save_snippets();
        self.stop_ws();
    }
}

fn parse_tags(input: &str) -> Vec<String> {
    let mut out = Vec::<String>::new();
    for raw in input.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let t = raw.trim();
        if t.is_empty() {
            continue;
        }
        if !out.iter().any(|x| x.eq_ignore_ascii_case(t)) {
            out.push(t.to_string());
        }
    }
    out
}

fn suggest_save_name(tab: &TemplateTab) -> String {
    if let Some(path) = &tab.origin_path {
        if tab.source_kind == SourceKind::DocxExtract {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("template");
            return format!("{stem}.groovy");
        }
    }

    let title = tab.title.trim();
    if title.is_empty() {
        "template.groovy".to_string()
    } else if title.ends_with(".groovy") || title.ends_with(".txt") {
        title.to_string()
    } else {
        format!("{title}.groovy")
    }
}

fn snippets_path() -> PathBuf {
    if let Some(base) = dirs::data_local_dir() {
        return base.join("OlehGroovyEditor").join("snippets.json");
    }
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("snippets.json")
}

fn timestamp_id() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn detect_machine_identity() -> String {
    let host = env::var("COMPUTERNAME")
        .or_else(|_| env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown-host".to_string());
    let user = env::var("USERNAME")
        .or_else(|_| env::var("USER"))
        .unwrap_or_else(|_| "unknown-user".to_string());
    let ip = local_ipv4().unwrap_or_else(|| "no-ip".to_string());
    format!("{host}\\{user} ({ip})")
}

fn local_ipv4() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    if socket.connect("8.8.8.8:80").is_err() {
        return None;
    }
    let addr = socket.local_addr().ok()?;
    Some(addr.ip().to_string())
}
