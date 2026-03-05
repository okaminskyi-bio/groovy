mod docx;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tokio::net::TcpListener;
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const DEFAULT_PORT: u16 = 8787;

#[derive(Clone)]
struct AppState {
    workspace_root: PathBuf,
    db_path: PathBuf,
}

#[derive(Serialize)]
struct ApiError {
    error: String,
}

type ApiResult<T> = Result<Json<T>, (StatusCode, Json<ApiError>)>;

#[derive(Serialize)]
struct ConfigResponse {
    workspace_root: String,
    db_path: String,
}

#[derive(Deserialize)]
struct TreeQuery {
    path: Option<String>,
}

#[derive(Serialize)]
struct TreeResponse {
    current_path: String,
    entries: Vec<FsEntry>,
}

#[derive(Serialize)]
struct FsEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
    modified: String,
}

#[derive(Deserialize)]
struct OpenRequest {
    path: String,
    mode: Option<String>,
}

#[derive(Serialize)]
struct OpenResponse {
    session_id: String,
    path: String,
    is_docx: bool,
    mode: String,
    language: String,
    content: String,
}

#[derive(Deserialize)]
struct SaveRequest {
    session_id: String,
    content: String,
    mode: Option<String>,
}

#[derive(Serialize)]
struct SaveResponse {
    ok: bool,
    saved_path: String,
}

#[derive(Deserialize)]
struct SessionQuery {
    session_id: String,
}

#[derive(Serialize)]
struct SessionResponse {
    session_id: String,
    path: String,
    is_docx: bool,
    mode: String,
    language: String,
    content: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct SnapshotRequest {
    session_id: String,
    summary: String,
}

#[derive(Deserialize)]
struct TimelineQuery {
    session_id: String,
}

#[derive(Serialize)]
struct TimelineResponse {
    entries: Vec<TimelineEntry>,
}

#[derive(Serialize)]
struct TimelineEntry {
    id: i64,
    summary: String,
    created_at: String,
}

#[derive(Deserialize)]
struct RevertRequest {
    session_id: String,
    entry_id: i64,
}

#[derive(Serialize)]
struct RevertResponse {
    content: String,
    mode: String,
}

#[derive(Debug)]
struct SessionRow {
    id: String,
    file_path: String,
    is_docx: bool,
    mode: String,
    content: String,
    updated_at: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workspace_root = std::env::current_dir()?;
    let data_dir = workspace_root.join("data");
    fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("oleh_groovy_studio.sqlite");
    ensure_db(&db_path)?;

    let state = AppState {
        workspace_root: workspace_root.clone(),
        db_path,
    };

    let app = Router::new()
        .route("/", get(index_html))
        .route("/styles.css", get(styles_css))
        .route("/app.js", get(app_js))
        .route("/api/config", get(api_config))
        .route("/api/tree", get(api_tree))
        .route("/api/open", post(api_open))
        .route("/api/save", post(api_save))
        .route("/api/session", get(api_session))
        .route(
            "/api/timeline",
            get(api_timeline).post(api_timeline_snapshot),
        )
        .route("/api/timeline/revert", post(api_timeline_revert))
        .with_state(state);

    let listener = TcpListener::bind(("127.0.0.1", DEFAULT_PORT)).await?;
    let addr = listener.local_addr()?;
    let url = format!("http://{addr}");

    println!("Oleh Groovy Studio Web running at {url}");
    let _ = webbrowser::open(url.as_str());
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index_html() -> Html<&'static str> {
    Html(include_str!("../web/index.html"))
}

async fn styles_css() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../web/styles.css"),
    )
}

async fn app_js() -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../web/app.js"),
    )
}

async fn api_config(State(state): State<AppState>) -> ApiResult<ConfigResponse> {
    Ok(Json(ConfigResponse {
        workspace_root: state.workspace_root.display().to_string(),
        db_path: state.db_path.display().to_string(),
    }))
}

async fn api_tree(
    State(state): State<AppState>,
    Query(query): Query<TreeQuery>,
) -> ApiResult<TreeResponse> {
    let root_canon = canonicalize_path(state.workspace_root.as_path())
        .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let current = if let Some(raw) = query.path {
        resolve_existing_path(&root_canon, raw.as_str())
            .map_err(|e| api_err(StatusCode::BAD_REQUEST, e))?
    } else {
        root_canon.clone()
    };

    let metadata = fs::metadata(&current).map_err(internal_error)?;
    if !metadata.is_dir() {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "Path is not a directory.".to_string(),
        ));
    }

    let mut entries = Vec::<FsEntry>::new();
    let read_dir = fs::read_dir(&current).map_err(internal_error)?;
    for item in read_dir {
        let item = item.map_err(internal_error)?;
        let path = item.path();
        let meta = item.metadata().map_err(internal_error)?;
        let name = item.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && name != ".github" {
            continue;
        }

        let rel = relative_path_string(&root_canon, &path);
        let modified = meta
            .modified()
            .ok()
            .and_then(systemtime_to_rfc3339)
            .unwrap_or_else(|| "-".to_string());
        entries.push(FsEntry {
            name,
            path: rel,
            is_dir: meta.is_dir(),
            size: if meta.is_file() { meta.len() } else { 0 },
            modified,
        });
    }

    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a
            .name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase()),
    });

    Ok(Json(TreeResponse {
        current_path: relative_path_string(&root_canon, &current),
        entries,
    }))
}

async fn api_open(
    State(state): State<AppState>,
    Json(req): Json<OpenRequest>,
) -> ApiResult<OpenResponse> {
    let root_canon = canonicalize_path(state.workspace_root.as_path())
        .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let file_path = resolve_existing_path(&root_canon, req.path.as_str())
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, e))?;
    let metadata = fs::metadata(&file_path).map_err(internal_error)?;
    if !metadata.is_file() {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "Path is not a file.".to_string(),
        ));
    }

    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_docx = ext == "docx";
    let content = if is_docx {
        docx::extract_template_text(file_path.as_path())
            .map_err(|e| api_err(StatusCode::BAD_REQUEST, e))?
    } else {
        fs::read_to_string(&file_path).map_err(internal_error)?
    };

    let mode = normalize_mode(req.mode.as_deref(), is_docx, &ext);
    let language = mode_to_language(mode.as_str()).to_string();
    let now = Utc::now().to_rfc3339();
    let session_id = Uuid::new_v4().to_string();

    with_db(&state.db_path, |conn| {
        conn.execute(
            "INSERT INTO sessions (id, file_path, is_docx, mode, content, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &session_id,
                file_path.display().to_string(),
                if is_docx { 1 } else { 0 },
                mode.as_str(),
                &content,
                &now
            ],
        )?;
        conn.execute(
            "INSERT INTO timeline (session_id, summary, content, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![&session_id, "Opened file", &content, &now],
        )?;
        Ok(())
    })
    .map_err(internal_error)?;

    Ok(Json(OpenResponse {
        session_id,
        path: relative_path_string(&root_canon, &file_path),
        is_docx,
        mode,
        language,
        content,
    }))
}

async fn api_save(
    State(state): State<AppState>,
    Json(req): Json<SaveRequest>,
) -> ApiResult<SaveResponse> {
    let session = get_session_row(state.db_path.as_path(), req.session_id.as_str())
        .map_err(internal_error)?;
    let mode = normalize_mode(req.mode.as_deref(), session.is_docx, "");
    let write_path = PathBuf::from(session.file_path.clone());

    if session.is_docx {
        write_docx_text(write_path.as_path(), req.content.as_str()).map_err(internal_error)?;
    } else {
        fs::write(write_path.as_path(), req.content.as_str()).map_err(internal_error)?;
    }

    let now = Utc::now().to_rfc3339();
    let summary = format!("Saved ({} chars)", req.content.chars().count());
    with_db(&state.db_path, |conn| {
        conn.execute(
            "UPDATE sessions SET mode = ?1, content = ?2, updated_at = ?3 WHERE id = ?4",
            params![
                mode.as_str(),
                req.content.as_str(),
                &now,
                req.session_id.as_str()
            ],
        )?;
        conn.execute(
            "INSERT INTO timeline (session_id, summary, content, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                req.session_id.as_str(),
                summary.as_str(),
                req.content.as_str(),
                &now
            ],
        )?;
        Ok(())
    })
    .map_err(internal_error)?;

    Ok(Json(SaveResponse {
        ok: true,
        saved_path: write_path.display().to_string(),
    }))
}

async fn api_session(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
) -> ApiResult<SessionResponse> {
    let root_canon = canonicalize_path(state.workspace_root.as_path())
        .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let session = get_session_row(state.db_path.as_path(), query.session_id.as_str())
        .map_err(internal_error)?;
    let path = PathBuf::from(session.file_path.clone());
    let rel_path = relative_path_string(&root_canon, &path);
    let language = mode_to_language(session.mode.as_str()).to_string();

    Ok(Json(SessionResponse {
        session_id: session.id,
        path: rel_path,
        is_docx: session.is_docx,
        mode: session.mode,
        language,
        content: session.content,
        updated_at: session.updated_at,
    }))
}

async fn api_timeline(
    State(state): State<AppState>,
    Query(query): Query<TimelineQuery>,
) -> ApiResult<TimelineResponse> {
    let entries = with_db(&state.db_path, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, summary, created_at
             FROM timeline
             WHERE session_id = ?1
             ORDER BY id DESC
             LIMIT 300",
        )?;
        let rows = stmt.query_map(params![query.session_id.as_str()], |row| {
            Ok(TimelineEntry {
                id: row.get(0)?,
                summary: row.get(1)?,
                created_at: row.get(2)?,
            })
        })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    })
    .map_err(internal_error)?;

    Ok(Json(TimelineResponse { entries }))
}

async fn api_timeline_snapshot(
    State(state): State<AppState>,
    Json(req): Json<SnapshotRequest>,
) -> ApiResult<TimelineResponse> {
    let session = get_session_row(state.db_path.as_path(), req.session_id.as_str())
        .map_err(internal_error)?;
    let summary = if req.summary.trim().is_empty() {
        "Manual snapshot".to_string()
    } else {
        req.summary.trim().to_string()
    };
    let now = Utc::now().to_rfc3339();

    with_db(&state.db_path, |conn| {
        conn.execute(
            "INSERT INTO timeline (session_id, summary, content, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                req.session_id.as_str(),
                summary.as_str(),
                session.content.as_str(),
                &now
            ],
        )?;
        Ok(())
    })
    .map_err(internal_error)?;

    api_timeline(
        State(state),
        Query(TimelineQuery {
            session_id: req.session_id,
        }),
    )
    .await
}

async fn api_timeline_revert(
    State(state): State<AppState>,
    Json(req): Json<RevertRequest>,
) -> ApiResult<RevertResponse> {
    let (content, mode) = with_db(&state.db_path, |conn| {
        let content: String = conn.query_row(
            "SELECT content FROM timeline WHERE id = ?1 AND session_id = ?2",
            params![req.entry_id, req.session_id.as_str()],
            |row| row.get(0),
        )?;
        let mode: String = conn.query_row(
            "SELECT mode FROM sessions WHERE id = ?1",
            params![req.session_id.as_str()],
            |row| row.get(0),
        )?;
        Ok((content, mode))
    })
    .map_err(internal_error)?;

    let now = Utc::now().to_rfc3339();
    with_db(&state.db_path, |conn| {
        conn.execute(
            "UPDATE sessions SET content = ?1, updated_at = ?2 WHERE id = ?3",
            params![content.as_str(), &now, req.session_id.as_str()],
        )?;
        conn.execute(
            "INSERT INTO timeline (session_id, summary, content, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                req.session_id.as_str(),
                format!("Reverted to entry #{}", req.entry_id),
                content.as_str(),
                &now
            ],
        )?;
        Ok(())
    })
    .map_err(internal_error)?;

    Ok(Json(RevertResponse { content, mode }))
}

fn ensure_db(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            file_path TEXT NOT NULL,
            is_docx INTEGER NOT NULL,
            mode TEXT NOT NULL,
            content TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS timeline (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            summary TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_timeline_session_id ON timeline(session_id);
        "#,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn with_db<T, F>(db_path: &Path, f: F) -> Result<T, String>
where
    F: FnOnce(&Connection) -> rusqlite::Result<T>,
{
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    f(&conn).map_err(|e| e.to_string())
}

fn get_session_row(db_path: &Path, session_id: &str) -> Result<SessionRow, String> {
    with_db(db_path, |conn| {
        conn.query_row(
            "SELECT id, file_path, is_docx, mode, content, updated_at FROM sessions WHERE id = ?1",
            params![session_id],
            |row| {
                let is_docx: i64 = row.get(2)?;
                Ok(SessionRow {
                    id: row.get(0)?,
                    file_path: row.get(1)?,
                    is_docx: is_docx != 0,
                    mode: row.get(3)?,
                    content: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        )
        .optional()?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)
    })
}

fn normalize_mode(mode: Option<&str>, is_docx: bool, ext: &str) -> String {
    match mode.unwrap_or("").to_ascii_lowercase().as_str() {
        "text" | "plaintext" => "text".to_string(),
        "groovy" => "groovy".to_string(),
        _ => {
            if is_docx || ext.eq_ignore_ascii_case("groovy") {
                "groovy".to_string()
            } else {
                "text".to_string()
            }
        }
    }
}

fn mode_to_language(mode: &str) -> &'static str {
    if mode.eq_ignore_ascii_case("groovy") {
        "groovy"
    } else {
        "plaintext"
    }
}

fn canonicalize_path(path: &Path) -> Result<PathBuf, String> {
    path.canonicalize().map_err(|e| e.to_string())
}

fn resolve_existing_path(root_canon: &Path, raw: &str) -> Result<PathBuf, String> {
    let clean = clean_relative_path(raw);
    let candidate = root_canon.join(clean);
    let canon = candidate
        .canonicalize()
        .map_err(|e| format!("Cannot resolve path: {e}"))?;
    if !canon.starts_with(root_canon) {
        return Err("Path is outside workspace root.".to_string());
    }
    Ok(canon)
}

fn clean_relative_path(raw: &str) -> String {
    raw.trim()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string()
}

fn relative_path_string(root: &Path, path: &Path) -> String {
    if let Ok(rel) = path.strip_prefix(root) {
        rel.to_string_lossy().replace('\\', "/")
    } else {
        path.to_string_lossy().replace('\\', "/")
    }
}

fn systemtime_to_rfc3339(value: std::time::SystemTime) -> Option<String> {
    let dt: chrono::DateTime<Utc> = value.into();
    Some(dt.to_rfc3339())
}

fn internal_error<E: std::fmt::Display>(err: E) -> (StatusCode, Json<ApiError>) {
    api_err(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

fn api_err(status: StatusCode, message: String) -> (StatusCode, Json<ApiError>) {
    (status, Json(ApiError { error: message }))
}

fn write_docx_text(path: &Path, text: &str) -> Result<(), String> {
    let src = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip_in = ZipArchive::new(src).map_err(|e| e.to_string())?;

    let mut entries = Vec::<(String, CompressionMethod, Vec<u8>, bool)>::new();
    let mut old_doc_xml = None::<String>;

    for i in 0..zip_in.len() {
        let mut file = zip_in.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().to_string();
        let method = file.compression();
        let is_dir = file.is_dir();

        if is_dir {
            entries.push((name, method, Vec::new(), true));
            continue;
        }

        let mut data = Vec::new();
        file.read_to_end(&mut data).map_err(|e| e.to_string())?;
        if name == "word/document.xml" {
            old_doc_xml = Some(String::from_utf8_lossy(data.as_slice()).to_string());
        }
        entries.push((name, method, data, false));
    }

    let sect_pr = old_doc_xml.as_ref().and_then(|xml| extract_sect_pr(xml));
    let new_document_xml = build_document_xml(text, sect_pr.as_deref());

    let temp_path = path.with_extension("tmp.docx");
    let dst = fs::File::create(&temp_path).map_err(|e| e.to_string())?;
    let mut zip_out = ZipWriter::new(dst);

    for (name, method, data, is_dir) in entries {
        let options = SimpleFileOptions::default().compression_method(method);
        if is_dir || name.ends_with('/') {
            zip_out
                .add_directory(name, options)
                .map_err(|e| e.to_string())?;
            continue;
        }

        zip_out
            .start_file(name.as_str(), options)
            .map_err(|e| e.to_string())?;
        if name == "word/document.xml" {
            zip_out
                .write_all(new_document_xml.as_bytes())
                .map_err(|e| e.to_string())?;
        } else {
            zip_out
                .write_all(data.as_slice())
                .map_err(|e| e.to_string())?;
        }
    }

    zip_out.finish().map_err(|e| e.to_string())?;
    fs::rename(&temp_path, path).map_err(|e| e.to_string())?;
    Ok(())
}

fn extract_sect_pr(doc_xml: &str) -> Option<String> {
    let re = Regex::new(r"(?s)(<w:sectPr\b.*?</w:sectPr>)").ok()?;
    re.captures_iter(doc_xml)
        .last()
        .and_then(|cap| cap.get(1).map(|m| m.as_str().to_string()))
}

fn build_document_xml(text: &str, sect_pr: Option<&str>) -> String {
    let mut body = String::new();
    for line in text.lines() {
        body.push_str("<w:p><w:r><w:t xml:space=\"preserve\">");
        body.push_str(xml_escape(line).as_str());
        body.push_str("</w:t></w:r></w:p>");
    }

    let sect = sect_pr.unwrap_or("<w:sectPr/>");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>{body}{sect}</w:body>
</w:document>"#
    )
}

fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
