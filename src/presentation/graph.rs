use crate::vault::bookmark::Bookmark;
use crate::vault::metadata::MetadataService;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SIMILARITY_EDGE_THRESHOLD: f32 = 0.3;
const CHROME_EPOCH_OFFSET_SECONDS: i64 = 11_644_473_600;
const SEARCH_HISTORY_LOOKBACK_SECONDS: i64 = 30 * 24 * 60 * 60;
const BOOKMARK_SEARCH_CONTEXT_SECONDS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub title: String,
    pub source_type: String,
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub content_preview: Option<String>,
    #[serde(default)]
    pub ocr_preview: Option<String>,
    #[serde(default)]
    pub file_metadata: Option<LocalFileMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalFileMetadata {
    pub file_name: String,
    pub path: String,
    pub extension: String,
    pub size_bytes: u64,
    pub modified_at: Option<i64>,
    pub image_dimensions: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHistoryEntry {
    pub query: String,
    pub engine: String,
    pub searched_at: i64,
}

struct ChromeHistorySnapshot {
    path: PathBuf,
}

impl Drop for ChromeHistorySnapshot {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(sqlite_sidecar_path(&self.path, "-wal"));
        let _ = fs::remove_file(sqlite_sidecar_path(&self.path, "-shm"));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub similarity: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedLink {
    pub source: String,
    pub target_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub unresolved_links: Vec<UnresolvedLink>,
    #[serde(default)]
    pub search_history: Vec<SearchHistoryEntry>,
    #[serde(default)]
    pub search_history_status: String,
}

#[derive(Debug, Default)]
pub struct KnowledgeGraph {
    nodes: HashMap<String, GraphNode>,
    title_to_id: HashMap<String, String>,
    raw_links: Vec<(String, String)>,
    outgoing_edges: HashMap<String, Vec<String>>,
    incoming_edges: HashMap<String, Vec<String>>,
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_bookmark(&mut self, bookmark: &Bookmark) {
        let node = GraphNode {
            id: bookmark.id.clone(),
            title: bookmark.title.clone(),
            source_type: bookmark.source_type.clone(),
            tags: bookmark.tags.clone(),
            created_at: bookmark.created_at,
            updated_at: bookmark.updated_at,
            source_url: bookmark.source_url.clone(),
            summary: bookmark.summary.clone(),
            content_preview: preview_text(bookmark.captured_text.as_deref()),
            ocr_preview: preview_text(bookmark.ocr_text.as_deref()),
            file_metadata: local_file_metadata(bookmark),
        };

        self.title_to_id
            .insert(bookmark.title.to_lowercase(), bookmark.id.clone());
        self.nodes.insert(bookmark.id.clone(), node);

        let content = bookmark.text_for_embedding();
        let targets = MetadataService::extract_wiki_links(&content);
        for target in targets {
            self.raw_links.push((bookmark.id.clone(), target));
        }
    }

    /// Resolves explicit wiki links between bookmarks.
    pub fn build_graph(&mut self) -> GraphData {
        self.outgoing_edges.clear();
        self.incoming_edges.clear();

        let mut resolved_edges = Vec::new();
        let mut unresolved_links = Vec::new();
        let mut seen_edges = HashSet::new();
        let mut seen_unresolved = HashSet::new();

        for (source_id, target_ref) in &self.raw_links {
            let target_lower = target_ref.to_lowercase();
            let resolved_target_id = if self.nodes.contains_key(target_ref) {
                Some(target_ref.clone())
            } else {
                self.title_to_id.get(&target_lower).cloned()
            };

            if let Some(target_id) = resolved_target_id {
                if !seen_edges.insert((source_id.clone(), target_id.clone())) {
                    continue;
                }
                self.outgoing_edges
                    .entry(source_id.clone())
                    .or_default()
                    .push(target_id.clone());
                self.incoming_edges
                    .entry(target_id.clone())
                    .or_default()
                    .push(source_id.clone());
                resolved_edges.push(GraphEdge {
                    source: source_id.clone(),
                    target: target_id,
                    similarity: None,
                });
            } else if seen_unresolved.insert((source_id.clone(), target_ref.clone())) {
                unresolved_links.push(UnresolvedLink {
                    source: source_id.clone(),
                    target_ref: target_ref.clone(),
                });
            }
        }

        let mut nodes: Vec<GraphNode> = self.nodes.values().cloned().collect();
        nodes.sort_by(|left, right| left.title.cmp(&right.title));

        GraphData {
            nodes,
            edges: resolved_edges,
            unresolved_links,
            search_history: Vec::new(),
            search_history_status: "Chrome search history has not been loaded.".into(),
        }
    }

    /// Adds undirected similarity edges for bookmark pairs at or above the threshold.
    pub fn build_graph_with_similarities(
        &mut self,
        embeddings: &HashMap<String, Vec<f32>>,
        threshold: f32,
    ) -> GraphData {
        let mut graph = self.build_graph();
        let node_ids: Vec<String> = graph.nodes.iter().map(|node| node.id.clone()).collect();
        let mut existing_edges: HashSet<(String, String)> = graph
            .edges
            .iter()
            .map(|edge| ordered_pair(&edge.source, &edge.target))
            .collect();

        for left_index in 0..node_ids.len() {
            let left_id = &node_ids[left_index];
            let Some(left_embedding) = embeddings.get(left_id) else {
                continue;
            };

            for right_id in node_ids.iter().skip(left_index + 1) {
                let Some(right_embedding) = embeddings.get(right_id) else {
                    continue;
                };

                let similarity = cosine_similarity(left_embedding, right_embedding);
                if similarity < threshold || !existing_edges.insert(ordered_pair(left_id, right_id))
                {
                    continue;
                }

                graph.edges.push(GraphEdge {
                    source: left_id.clone(),
                    target: right_id.clone(),
                    similarity: Some(similarity),
                });
            }
        }

        graph
    }

    /// Gets explicit wiki-link backlinks for a bookmark ID or title.
    pub fn get_backlinks(&self, id_or_title: &str) -> Vec<String> {
        let id = self
            .title_to_id
            .get(&id_or_title.to_lowercase())
            .map(String::as_str)
            .unwrap_or(id_or_title);

        self.incoming_edges.get(id).cloned().unwrap_or_default()
    }

    /// Gets explicit wiki-link targets for a bookmark ID.
    pub fn get_neighbors(&self, id: &str) -> Vec<String> {
        self.outgoing_edges.get(id).cloned().unwrap_or_default()
    }
}

fn ordered_pair(left: &str, right: &str) -> (String, String) {
    if left <= right {
        (left.to_string(), right.to_string())
    } else {
        (right.to_string(), left.to_string())
    }
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.is_empty() || left.len() != right.len() {
        return 0.0;
    }

    let dot: f32 = left.iter().zip(right).map(|(a, b)| a * b).sum();
    let left_norm: f32 = left.iter().map(|value| value * value).sum::<f32>().sqrt();
    let right_norm: f32 = right.iter().map(|value| value * value).sum::<f32>().sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        0.0
    } else {
        dot / (left_norm * right_norm)
    }
}

fn preview_text(text: Option<&str>) -> Option<String> {
    let text = text?.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return None;
    }

    let preview: String = text.chars().take(500).collect();
    Some(if text.chars().count() > 500 {
        format!("{preview}…")
    } else {
        preview
    })
}

fn local_file_metadata(bookmark: &Bookmark) -> Option<LocalFileMetadata> {
    let candidates = [&bookmark.path, bookmark.source_url.as_deref().unwrap_or("")];
    let path = candidates
        .iter()
        .map(|value| Path::new(value))
        .find(|path| {
            !value_is_web_url(&path.to_string_lossy()) && path.exists() && path.is_file()
        })?;

    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            eprintln!(
                "Could not read file metadata for graph bookmark '{}': {error}",
                bookmark.id
            );
            return None;
        }
    };
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_lowercase();
    let image_dimensions = if bookmark.source_type.eq_ignore_ascii_case("image")
        || matches!(
            extension.as_str(),
            "png" | "jpg" | "jpeg" | "bmp" | "webp" | "gif"
        ) {
        match image::image_dimensions(path) {
            Ok(dimensions) => Some(dimensions),
            Err(error) => {
                eprintln!(
                    "Could not read image dimensions for graph bookmark '{}': {error}",
                    bookmark.id
                );
                None
            }
        }
    } else {
        None
    };
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(system_time_to_unix_seconds);

    Some(LocalFileMetadata {
        file_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Unknown file")
            .to_string(),
        path: path.to_string_lossy().to_string(),
        extension,
        size_bytes: metadata.len(),
        modified_at,
        image_dimensions,
    })
}

fn value_is_web_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn system_time_to_unix_seconds(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
}

/// Reads recent search queries from Chrome profile databases without modifying them.
pub fn load_chrome_search_history() -> (Vec<SearchHistoryEntry>, String) {
    let profile_root = match chrome_profile_root() {
        Some(path) => path,
        None => {
            return (
                Vec::new(),
                "Chrome profile location could not be determined.".into(),
            );
        }
    };
    if !profile_root.is_dir() {
        return (Vec::new(), "Chrome profile directory was not found.".into());
    }

    let profiles = match fs::read_dir(&profile_root) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!(
                "Could not list Chrome profiles at '{}': {error}",
                profile_root.display()
            );
            return (
                Vec::new(),
                "Chrome profile directory could not be read.".into(),
            );
        }
    };
    let mut profile_dirs = Vec::new();
    let mut profile_listing_failures = 0usize;
    for entry in profiles {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                eprintln!("Could not read a Chrome profile entry: {error}");
                profile_listing_failures += 1;
                continue;
            }
        };
        let path = entry.path();
        let profile_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if path.is_dir() && (profile_name == "Default" || profile_name.starts_with("Profile ")) {
            profile_dirs.push(path);
        }
    }
    profile_dirs.sort();

    let cutoff = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
        - SEARCH_HISTORY_LOOKBACK_SECONDS;
    let chrome_cutoff = (cutoff + CHROME_EPOCH_OFFSET_SECONDS) * 1_000_000;
    let mut entries = Vec::new();
    let mut readable_profiles = 0usize;
    let mut unavailable_profiles = profile_listing_failures;
    let mut seen = HashSet::new();

    for profile in profile_dirs {
        let history_path = profile.join("History");
        if !history_path.is_file() {
            continue;
        }
        let snapshot = match snapshot_chrome_history(&history_path) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                eprintln!(
                    "Could not snapshot Chrome history from '{}': {error}",
                    history_path.display()
                );
                unavailable_profiles += 1;
                continue;
            }
        };
        let connection = match rusqlite::Connection::open_with_flags(
            &snapshot.path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            Ok(connection) => connection,
            Err(error) => {
                eprintln!(
                    "Could not read Chrome history from '{}': {error}",
                    history_path.display()
                );
                unavailable_profiles += 1;
                continue;
            }
        };
        let mut statement = match connection.prepare(
            "SELECT u.url, v.visit_time FROM visits v JOIN urls u ON u.id = v.url WHERE v.visit_time >= ?1 ORDER BY v.visit_time DESC LIMIT 10000",
        ) {
            Ok(statement) => statement,
            Err(error) => {
                eprintln!(
                    "Could not query Chrome history in '{}': {error}",
                    history_path.display()
                );
                unavailable_profiles += 1;
                continue;
            }
        };
        let rows = match statement.query_map([chrome_cutoff], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        }) {
            Ok(rows) => rows,
            Err(error) => {
                eprintln!(
                    "Could not read Chrome history rows in '{}': {error}",
                    history_path.display()
                );
                unavailable_profiles += 1;
                continue;
            }
        };

        readable_profiles += 1;
        for row in rows {
            let (visited_url, chrome_time) = match row {
                Ok(row) => row,
                Err(error) => {
                    eprintln!("Could not decode a Chrome history entry: {error}");
                    continue;
                }
            };
            let Some((engine, query)) = parse_search_url(&visited_url) else {
                continue;
            };
            let searched_at = chrome_timestamp_to_unix_seconds(chrome_time);
            if searched_at <= 0 || !seen.insert((engine.clone(), query.clone(), searched_at)) {
                continue;
            }
            entries.push(SearchHistoryEntry {
                query,
                engine,
                searched_at,
            });
        }
    }

    entries.sort_by(|left, right| right.searched_at.cmp(&left.searched_at));
    entries.truncate(500);
    let status = if readable_profiles == 0 {
        if unavailable_profiles > 0 {
            "Chrome history files were found but could not be read; see the Moneta log for details."
                .into()
        } else {
            "No readable Chrome history databases were found.".into()
        }
    } else if unavailable_profiles > 0 {
        format!(
            "Loaded {} recent searches from Chrome; {unavailable_profiles} profile(s) could not be read.",
            entries.len()
        )
    } else {
        format!("Loaded {} recent searches from Chrome.", entries.len())
    };
    (entries, status)
}

fn snapshot_chrome_history(source: &Path) -> std::io::Result<ChromeHistorySnapshot> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(std::io::Error::other)?
        .as_nanos();
    let snapshot_path = std::env::temp_dir().join(format!(
        "moneta-chrome-history-{}-{unique}.db",
        std::process::id()
    ));
    fs::copy(source, &snapshot_path)?;
    for suffix in ["-wal", "-shm"] {
        let source_sidecar = sqlite_sidecar_path(source, suffix);
        if source_sidecar.exists() {
            if let Err(error) =
                fs::copy(&source_sidecar, sqlite_sidecar_path(&snapshot_path, suffix))
            {
                let _ = fs::remove_file(&snapshot_path);
                return Err(error);
            }
        }
    }
    Ok(ChromeHistorySnapshot {
        path: snapshot_path,
    })
}

fn sqlite_sidecar_path(database: &Path, suffix: &str) -> PathBuf {
    let mut path = database.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
}

fn chrome_profile_root() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("Google").join("Chrome").join("User Data"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(PathBuf::from).map(|path| {
            path.join("Library")
                .join("Application Support")
                .join("Google")
                .join("Chrome")
        })
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|path| path.join(".config").join("google-chrome"))
    }
    #[cfg(not(any(windows, unix)))]
    {
        None
    }
}

fn parse_search_url(value: &str) -> Option<(String, String)> {
    let parsed = url::Url::parse(value).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let path = parsed.path();
    let (engine, parameter) =
        if (host == "google.com" || host.ends_with(".google.com") || host.starts_with("google."))
            && path == "/search"
        {
            ("Google", "q")
        } else if host == "bing.com" || host.ends_with(".bing.com") {
            ("Bing", "q")
        } else if host == "duckduckgo.com" || host.ends_with(".duckduckgo.com") {
            ("DuckDuckGo", "q")
        } else if host == "search.yahoo.com" {
            ("Yahoo", "p")
        } else if host == "search.brave.com" {
            ("Brave", "q")
        } else if host == "ecosia.org" || host.ends_with(".ecosia.org") {
            ("Ecosia", "q")
        } else if host.starts_with("yandex.") || host.ends_with(".yandex.com") {
            ("Yandex", "text")
        } else if host == "youtube.com" || host.ends_with(".youtube.com") {
            ("YouTube", "search_query")
        } else {
            return None;
        };
    let expected_path = match engine {
        "Bing" | "Yahoo" | "Brave" | "Ecosia" | "Yandex" => path.contains("/search"),
        "DuckDuckGo" => path == "/" || path == "/html/",
        "YouTube" => path == "/results",
        _ => true,
    };
    if !expected_path {
        return None;
    }
    let query = parsed
        .query_pairs()
        .find(|(key, _)| key == parameter)
        .map(|(_, value)| value.trim().to_string())?;
    if query.is_empty() {
        return None;
    }
    Some((engine.into(), query))
}

fn chrome_timestamp_to_unix_seconds(chrome_time: i64) -> i64 {
    chrome_time / 1_000_000 - CHROME_EPOCH_OFFSET_SECONDS
}

impl GraphData {
    /// Produces a dependency-free, interactive graph view that can open locally.
    pub fn to_interactive_html(&self) -> Result<String, serde_json::Error> {
        let mut graph_json = serde_json::to_string(self)?;
        graph_json = graph_json
            .replace('<', "\\u003c")
            .replace('>', "\\u003e")
            .replace('&', "\\u0026");

        const TEMPLATE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Moneta Bookmark Graph</title>
<style>
*{box-sizing:border-box}body{margin:0;background:#101319;color:#e6eaf2;font:14px system-ui,sans-serif;overflow:hidden}
header{position:absolute;z-index:2;top:16px;left:20px;display:flex;align-items:center;gap:14px}
h1{font-size:18px;margin:0}#stats{color:#a7b0c0;font-size:12px}
#search{background:#1c222d;border:1px solid #394253;border-radius:7px;color:#e6eaf2;padding:8px 10px;width:230px;outline:none}
#graph{width:100vw;height:100vh;display:block}
#detail{position:absolute;right:18px;top:18px;width:min(390px,calc(100vw - 36px));max-height:calc(100vh - 36px);overflow:auto;padding:15px;background:#1a202b;border:1px solid #394253;border-radius:10px;display:none;box-shadow:0 8px 30px #0006}
#detail h2{font-size:15px;margin:0 0 8px}#detail h3{font-size:12px;margin:12px 0 5px;color:#e6eaf2}#detail p,#detail li{color:#a7b0c0;font-size:12px;line-height:1.5;margin:6px 0;overflow-wrap:anywhere}#detail ul{padding-left:18px;margin:4px 0}
#detail .file{white-space:pre-wrap}
#legend{position:absolute;left:20px;bottom:18px;color:#a7b0c0;font-size:12px;display:flex;gap:16px}
.dot{display:inline-block;width:9px;height:9px;border-radius:50%;margin-right:6px}.similar{background:#61c5ff}.wiki{background:#c792ea}
</style>
</head>
<body>
<header><h1>Bookmark Graph</h1><span id="stats"></span><input id="search" placeholder="Filter bookmarks…" autocomplete="off"></header>
<canvas id="graph"></canvas>
<aside id="detail"><h2></h2><p class="type"></p><p class="time"></p><p class="source"></p><p class="tags"></p><p class="links"></p><section class="file-section" hidden><h3>File metadata</h3><p class="file"></p></section><section class="context-section" hidden><h3>Context</h3><p class="summary"></p><p class="content"></p><p class="ocr"></p></section><section class="history-section"><h3>Chrome searches before capture</h3><ul class="searches"></ul><p class="history-status"></p></section></aside>
<div id="legend"><span><i class="dot similar"></i>Similar bookmark</span><span><i class="dot wiki"></i>Wiki link</span></div>
<script id="graph-data" type="application/json">__GRAPH_DATA__</script>
<script>
const data=JSON.parse(document.getElementById('graph-data').textContent),canvas=document.getElementById('graph'),ctx=canvas.getContext('2d');
const SEARCH_CONTEXT_SECONDS=__SEARCH_CONTEXT_SECONDS__;
const nodes=data.nodes.map((n,i)=>({...n,x:Math.cos(i*2.399)*Math.min(innerWidth,innerHeight)*.32,y:Math.sin(i*2.399)*Math.min(innerWidth,innerHeight)*.32,vx:0,vy:0}));
const byId=new Map(nodes.map(n=>[n.id,n])),edges=data.edges.filter(e=>byId.has(e.source)&&byId.has(e.target));
const neighbors=new Map(nodes.map(n=>[n.id,[]]));
for(const e of edges){neighbors.get(e.source).push({node:byId.get(e.target),edge:e});neighbors.get(e.target).push({node:byId.get(e.source),edge:e})}
const detail=document.getElementById('detail'),query=document.getElementById('search');let scale=1,offsetX=innerWidth/2,offsetY=innerHeight/2,drag=null,pan=null,hover=null;
document.getElementById('stats').textContent=`${nodes.length} bookmarks · ${edges.length} connections`;
function resize(){const dpr=devicePixelRatio||1;canvas.width=innerWidth*dpr;canvas.height=innerHeight*dpr;canvas.style.width=innerWidth+'px';canvas.style.height=innerHeight+'px';ctx.setTransform(dpr,0,0,dpr,0,0)}
function visible(n){const q=query.value.toLowerCase();return !q||n.title.toLowerCase().includes(q)||n.tags.some(t=>t.toLowerCase().includes(q))||(n.content_preview||'').toLowerCase().includes(q)||(n.ocr_preview||'').toLowerCase().includes(q)}
function frame(){
 const visibleIds=new Set(nodes.filter(visible).map(n=>n.id));
 for(let i=0;i<nodes.length;i++){const a=nodes[i];if(a===drag)continue;let fx=0,fy=0;
  for(let j=i+1;j<nodes.length;j++){const b=nodes[j],dx=b.x-a.x,dy=b.y-a.y,d2=dx*dx+dy*dy+1,force=900/d2;fx-=dx*force;fy-=dy*force;b.vx+=dx*force*.0008;b.vy+=dy*force*.0008}
  for(const {node:b,edge:e} of neighbors.get(a.id)){const dx=b.x-a.x,dy=b.y-a.y,d=e.similarity||.6,k=e.similarity===null?.0008:.00035+d*.0004;fx+=dx*k;fy+=dy*k}
  const degree=Math.max(1,neighbors.get(a.id).length);
  a.vx=(a.vx+fx/degree)*.88;a.vy=(a.vy+fy/degree)*.88;
  const speed=Math.hypot(a.vx,a.vy);if(speed>2){a.vx*=2/speed;a.vy*=2/speed}
  let nextX=a.x+a.vx,nextY=a.y+a.vy;const radius=Math.hypot(nextX,nextY),limit=Math.max(120,Math.min(innerWidth,innerHeight)*.42);
  if(radius>limit){nextX*=limit/radius;nextY*=limit/radius;const outward=(a.vx*nextX+a.vy*nextY)/(limit*limit);if(outward>0){a.vx-=outward*nextX;a.vy-=outward*nextY}}
  a.x=nextX;a.y=nextY;
 }
 ctx.clearRect(0,0,innerWidth,innerHeight);ctx.save();ctx.translate(offsetX,offsetY);ctx.scale(scale,scale);
 for(const e of edges){const a=byId.get(e.source),b=byId.get(e.target);if(!visibleIds.has(a.id)&&!visibleIds.has(b.id))continue;ctx.beginPath();ctx.moveTo(a.x,a.y);ctx.lineTo(b.x,b.y);ctx.strokeStyle=e.similarity===null?'#c792ea88':'#61c5ff66';ctx.lineWidth=e.similarity===null?1.4:Math.max(.7,(e.similarity||0)*2);ctx.stroke()}
 for(const n of nodes){const show=visibleIds.has(n.id);if(!show)continue;
  const r=n===hover?8:6;ctx.beginPath();ctx.arc(n.x,n.y,r,0,Math.PI*2);ctx.fillStyle=n===hover?'#fff':n.source_type==='image'?'#f6bd60':n.source_type==='web'?'#61c5ff':'#84d98a';ctx.fill();
  if(show&&scale>.65){ctx.font=`${n===hover?13:11}px system-ui`;ctx.fillStyle='#e6eaf2';ctx.fillText(n.title.slice(0,48),n.x+10,n.y+4)}
 }ctx.restore();requestAnimationFrame(frame)
}
function point(ev){const r=canvas.getBoundingClientRect();return{x:(ev.clientX-r.left-offsetX)/scale,y:(ev.clientY-r.top-offsetY)/scale}}
function hit(p){for(let i=nodes.length-1;i>=0;i--){const n=nodes[i];if(visible(n)&&Math.hypot(n.x-p.x,n.y-p.y)<12/scale)return n}return null}
canvas.addEventListener('pointerdown',e=>{canvas.setPointerCapture(e.pointerId);const p=point(e);drag=hit(p);if(!drag)pan={x:e.clientX-offsetX,y:e.clientY-offsetY}});
canvas.addEventListener('pointermove',e=>{const p=point(e);hover=hit(p);if(drag){drag.x=p.x;drag.y=p.y;drag.vx=drag.vy=0}else if(pan){offsetX=e.clientX-pan.x;offsetY=e.clientY-pan.y}});
function showDetails(node){
 detail.style.display='block';
 detail.querySelector('h2').textContent=node.title;
 detail.querySelector('.type').textContent='Type: '+node.source_type;
 detail.querySelector('.time').textContent='Captured: '+formatTime(node.created_at)+' · Updated: '+formatTime(node.updated_at);
 detail.querySelector('.source').textContent=node.source_url?'Source: '+node.source_url:'';
 detail.querySelector('.tags').textContent=node.tags.length?'Tags: '+node.tags.join(', '):'No tags';
 detail.querySelector('.links').textContent='Connections: '+edges.filter(x=>x.source===node.id||x.target===node.id).length;
 const fileSection=detail.querySelector('.file-section'),fileMetadata=node.file_metadata;
 fileSection.hidden=!fileMetadata;
 if(fileMetadata){const lines=['Name: '+fileMetadata.file_name,'Path: '+fileMetadata.path,'Type: '+(fileMetadata.extension||'unknown'),'Size: '+formatSize(fileMetadata.size_bytes),'Modified: '+formatTime(fileMetadata.modified_at)];if(fileMetadata.image_dimensions)lines.push('Dimensions: '+fileMetadata.image_dimensions[0]+' × '+fileMetadata.image_dimensions[1]+' px');detail.querySelector('.file').textContent=lines.join('\n')}
 const contextSection=detail.querySelector('.context-section'),summary=detail.querySelector('.summary'),content=detail.querySelector('.content'),ocr=detail.querySelector('.ocr');
 summary.textContent=node.summary?'Summary: '+node.summary:'';content.textContent=node.content_preview?'Extracted text: '+node.content_preview:'';ocr.textContent=node.ocr_preview?'OCR text: '+node.ocr_preview:'';
 contextSection.hidden=!node.summary&&!node.content_preview&&!node.ocr_preview;
 const relatedSearches=data.search_history.filter(item=>item.searched_at<=node.created_at&&node.created_at-item.searched_at<=SEARCH_CONTEXT_SECONDS).slice(0,5),searches=relatedSearches.length?relatedSearches:data.search_history.slice(0,5),list=detail.querySelector('.searches');
 list.replaceChildren();for(const item of searches){const li=document.createElement('li');li.textContent=formatTime(item.searched_at)+' · '+item.engine+': '+item.query;list.appendChild(li)}
 detail.querySelector('.history-section h3').textContent=relatedSearches.length?'Chrome searches before capture':'Recent Chrome searches';
 detail.querySelector('.history-status').textContent=relatedSearches.length?'':searches.length?'No searches were found within 24 hours before this capture; showing the latest Chrome searches instead.':(data.search_history_status||'No Chrome search history was found.');
}
function formatTime(seconds){return seconds?new Date(seconds*1000).toLocaleString():'Not available'}
function formatSize(bytes){if(bytes<1024)return bytes+' B';const units=['KB','MB','GB','TB'];let value=bytes/1024,index=0;while(value>=1024&&index<units.length-1){value/=1024;index++}return value.toFixed(1)+' '+units[index]}
canvas.addEventListener('pointerup',()=>{const clicked=drag;drag=null;pan=null;if(clicked)showDetails(clicked)});
canvas.addEventListener('wheel',e=>{e.preventDefault();const old=scale;scale=Math.max(.2,Math.min(3,scale*(e.deltaY<0?1.1:.9)));offsetX=e.clientX-(e.clientX-offsetX)*(scale/old);offsetY=e.clientY-(e.clientY-offsetY)*(scale/old)},{passive:false});
query.addEventListener('input',()=>{const n=nodes.find(visible);if(n){offsetX=innerWidth/2-n.x*scale;offsetY=innerHeight/2-n.y*scale}});
addEventListener('resize',resize);resize();frame();
</script>
</body>
</html>"#;

        Ok(TEMPLATE.replace("__GRAPH_DATA__", &graph_json).replace(
            "__SEARCH_CONTEXT_SECONDS__",
            &BOOKMARK_SEARCH_CONTEXT_SECONDS.to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bookmark(id: &str, title: &str, content: &str) -> Bookmark {
        let mut bookmark = Bookmark::new(
            id.to_string(),
            String::new(),
            title.to_string(),
            "web".to_string(),
        );
        bookmark.captured_text = Some(content.to_string());
        bookmark.tags = vec!["test".to_string()];
        bookmark
    }

    #[test]
    fn resolves_wiki_links_and_backlinks() {
        let mut graph = KnowledgeGraph::new();
        graph.add_bookmark(&bookmark("1", "First", "See [[Second]]."));
        graph.add_bookmark(&bookmark("2", "Second", "A page."));

        let data = graph.build_graph();
        assert_eq!(data.edges.len(), 1);
        assert_eq!(data.edges[0].source, "1");
        assert_eq!(data.edges[0].target, "2");
        assert_eq!(graph.get_backlinks("Second"), vec!["1"]);
        assert_eq!(graph.get_neighbors("1"), vec!["2"]);
    }

    #[test]
    fn adds_similarity_edges_at_threshold_without_replacing_wiki_edges() {
        let mut graph = KnowledgeGraph::new();
        graph.add_bookmark(&bookmark("1", "First", "See [[Second]]."));
        graph.add_bookmark(&bookmark("2", "Second", "A page."));
        graph.add_bookmark(&bookmark("3", "Third", "Another page."));
        let embeddings = HashMap::from([
            ("1".to_string(), vec![1.0, 0.0]),
            ("2".to_string(), vec![0.8, 0.6]),
            ("3".to_string(), vec![0.0, 1.0]),
        ]);

        let data = graph.build_graph_with_similarities(&embeddings, 0.3);
        assert_eq!(data.edges.len(), 2);
        assert!(data.edges.iter().any(|edge| edge.similarity.is_none()));
        assert!(data.edges.iter().any(|edge| {
            edge.similarity
                .is_some_and(|score| (score - 0.6).abs() < 0.0001)
        }));
        assert_eq!(graph.get_backlinks("Second"), vec!["1"]);
    }

    #[test]
    fn graph_nodes_include_bookmark_timestamps_and_text_previews() {
        let mut bookmark = bookmark("1", "Research note", "A   compact\ntext preview.");
        bookmark.created_at = 1_700_000_000;
        bookmark.updated_at = 1_700_000_100;
        bookmark.summary = Some("A summary".into());

        let mut graph = KnowledgeGraph::new();
        graph.add_bookmark(&bookmark);
        let node = &graph.build_graph().nodes[0];

        assert_eq!(node.created_at, bookmark.created_at);
        assert_eq!(node.updated_at, bookmark.updated_at);
        assert_eq!(
            node.content_preview.as_deref(),
            Some("A compact text preview.")
        );
        assert_eq!(node.summary.as_deref(), Some("A summary"));
    }

    #[test]
    fn reads_basic_metadata_and_dimensions_for_local_image_bookmarks() {
        let path =
            std::env::temp_dir().join(format!("moneta-graph-image-{}.png", std::process::id()));
        image::RgbImage::new(7, 5).save(&path).unwrap();
        let bookmark = Bookmark::new(
            "image-1".into(),
            path.to_string_lossy().into_owned(),
            "Test image".into(),
            "image".into(),
        );

        let metadata = local_file_metadata(&bookmark).unwrap();
        assert_eq!(
            metadata.file_name,
            path.file_name().unwrap().to_string_lossy()
        );
        assert_eq!(metadata.image_dimensions, Some((7, 5)));
        assert!(metadata.size_bytes > 0);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn parses_search_queries_without_collecting_regular_page_visits() {
        assert_eq!(
            parse_search_url("https://www.google.com/search?q=rust+graph"),
            Some(("Google".into(), "rust graph".into()))
        );
        assert_eq!(
            parse_search_url("https://www.youtube.com/results?search_query=rust"),
            Some(("YouTube".into(), "rust".into()))
        );
        assert_eq!(parse_search_url("https://www.google.com/"), None);
        assert_eq!(
            parse_search_url("https://example.com/search?q=secret"),
            None
        );
    }

    #[test]
    fn converts_chrome_visit_time_to_unix_seconds() {
        let unix_seconds = 1_700_000_000i64;
        let chrome_time = (unix_seconds + CHROME_EPOCH_OFFSET_SECONDS) * 1_000_000;
        assert_eq!(chrome_timestamp_to_unix_seconds(chrome_time), unix_seconds);
    }

    #[test]
    fn snapshots_a_locked_style_history_database_without_changing_the_source() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let source = std::env::temp_dir().join(format!(
            "moneta-chrome-history-test-{}-{unique}.db",
            std::process::id()
        ));
        let connection = rusqlite::Connection::open(&source).unwrap();
        connection
            .execute("CREATE TABLE history_probe (value TEXT)", [])
            .unwrap();
        connection
            .execute("INSERT INTO history_probe VALUES ('preserved')", [])
            .unwrap();
        drop(connection);

        let snapshot = snapshot_chrome_history(&source).unwrap();
        let snapshot_connection = rusqlite::Connection::open_with_flags(
            &snapshot.path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let value: String = snapshot_connection
            .query_row("SELECT value FROM history_probe", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "preserved");
        drop(snapshot_connection);
        let snapshot_path = snapshot.path.clone();
        drop(snapshot);

        assert!(source.is_file());
        assert!(!snapshot_path.exists());
        fs::remove_file(source).unwrap();
    }

    #[test]
    fn escapes_script_markup_in_interactive_html() {
        let data = GraphData {
            nodes: vec![GraphNode {
                id: "1".to_string(),
                title: "</script><script>alert(1)</script>".to_string(),
                source_type: "web".to_string(),
                tags: vec![],
                created_at: 1,
                updated_at: 1,
                source_url: None,
                summary: None,
                content_preview: None,
                ocr_preview: None,
                file_metadata: None,
            }],
            edges: vec![],
            unresolved_links: vec![],
            search_history: vec![],
            search_history_status: String::new(),
        };

        let html = data.to_interactive_html().unwrap();
        assert!(!html.contains("</script><script>alert(1)</script>"));
        assert!(html.contains("\\u003c/script\\u003e"));
        assert!(html.contains("JSON.parse"));
        assert!(html.contains("Chrome searches before capture"));
        assert!(html.contains("SEARCH_CONTEXT_SECONDS=86400"));
    }
}
