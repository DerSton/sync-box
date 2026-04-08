use serde::{Deserialize, Serialize};
use ssh2::Session;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    pub used_bytes: u64,
    pub total_bytes: u64,
    pub home_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadJob {
    pub id: String,
    pub connection_id: String,
    pub connection_name: String,
    pub remote_path: String,
    pub files: Vec<String>,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub status: JobStatus,
    pub speed_bps: f64,
    pub eta_seconds: f64,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub current_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadProgress {
    pub job_id: String,
    pub transferred_bytes: u64,
    pub total_bytes: u64,
    pub speed_bps: f64,
    pub eta_seconds: f64,
    pub status: JobStatus,
    pub current_file: String,
}

struct SftpConnection {
    config: ConnectionConfig,
    #[allow(dead_code)]
    session: Session,
}

struct AppState {
    connections: HashMap<String, SftpConnection>,
    jobs: HashMap<String, UploadJob>,
    saved_configs: Vec<ConnectionConfig>,
    cancel_flags: HashMap<String, Arc<AtomicBool>>,
}

impl AppState {
    fn new() -> Self {
        Self {
            connections: HashMap::new(),
            jobs: HashMap::new(),
            saved_configs: Vec::new(),
            cancel_flags: HashMap::new(),
        }
    }
}

type SharedState = Arc<Mutex<AppState>>;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn get_config_path(app: &AppHandle) -> std::path::PathBuf {
    let data_dir = app.path().app_data_dir().unwrap_or_else(|_| {
        dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("sync-box")
    });
    std::fs::create_dir_all(&data_dir).ok();
    data_dir.join("connections.json")
}

#[tauri::command]
async fn get_saved_connections(
    app: AppHandle,
    state: State<'_, SharedState>,
) -> Result<Vec<ConnectionConfig>, String> {
    let path = get_config_path(&app);
    if path.exists() {
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let configs: Vec<ConnectionConfig> = serde_json::from_str(&content).unwrap_or_default();
        let mut s = state.lock().unwrap();
        s.saved_configs = configs.clone();
        Ok(configs)
    } else {
        Ok(vec![])
    }
}

#[tauri::command]
async fn save_connection(
    app: AppHandle,
    state: State<'_, SharedState>,
    config: ConnectionConfig,
) -> Result<(), String> {
    let path = get_config_path(&app);
    let mut s = state.lock().unwrap();
    let pos = s.saved_configs.iter().position(|c| c.id == config.id);
    if let Some(i) = pos {
        s.saved_configs[i] = config;
    } else {
        s.saved_configs.push(config);
    }
    let json = serde_json::to_string_pretty(&s.saved_configs).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn delete_connection(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
    let path = get_config_path(&app);
    let mut s = state.lock().unwrap();
    s.saved_configs.retain(|c| c.id != id);
    let json = serde_json::to_string_pretty(&s.saved_configs).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

fn create_session(config: &ConnectionConfig) -> Result<Session, String> {
    let host = format!("{}:{}", config.host, config.port);
    let tcp = TcpStream::connect(&host).map_err(|e| format!("TCP connect failed: {}", e))?;
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(60))).ok();
    tcp.set_write_timeout(Some(std::time::Duration::from_secs(60))).ok();
    let mut session = Session::new().map_err(|e| e.to_string())?;
    session.set_tcp_stream(tcp);
    // Compress if latency is high (storage boxes are remote)
    session.set_compress(true);
    session.handshake().map_err(|e| format!("SSH handshake failed: {}", e))?;
    session
        .userauth_password(&config.username, &config.password)
        .map_err(|e| format!("Authentication failed: {}", e))?;
    if !session.authenticated() {
        return Err("Authentication failed".to_string());
    }
    Ok(session)
}

fn exec_command(session: &Session, cmd: &str) -> String {
    let result = (|| -> Result<String, Box<dyn std::error::Error>> {
        let mut channel = session.channel_session()?;
        channel.exec(cmd)?;
        let mut output = String::new();
        channel.read_to_string(&mut output)?;
        channel.wait_close().ok();
        Ok(output)
    })();
    result.unwrap_or_default()
}

fn get_disk_stats(session: &Session) -> StorageStats {
    // Get home dir via sftp realpath(".") — most reliable, no shell needed
    let home_dir = session.sftp().ok()
        .and_then(|sftp| sftp.realpath(Path::new(".")).ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| exec_command(session, "pwd").trim().to_string())
        .trim()
        .to_string();
    let home_dir = if home_dir.is_empty() { ".".to_string() } else { home_dir };

    // Try quota first (Hetzner specific), fall back to df
    let (used_bytes, total_bytes) = try_quota(session)
        .or_else(|| try_df(session))
        .unwrap_or((0, 0));

    StorageStats { used_bytes, total_bytes, home_dir }
}

fn try_quota(session: &Session) -> Option<(u64, u64)> {
    // Hetzner storage boxes expose quota via `quota -s` or just `quota`
    // Output looks like: "Disk quotas for user u123456 (uid 1234):"
    // followed by: "   /dev/...  used  soft  hard  ..."
    // We try quota with raw bytes (-l flag or without -s)
    let output = exec_command(session, "quota 2>/dev/null || quota -l 2>/dev/null");
    parse_quota_output(&output)
}

fn parse_quota_output(output: &str) -> Option<(u64, u64)> {
    // quota output has two formats:
    // Format A (single-line, filesystem as col 0):
    //   /dev/sda1  1234*  2048  4096  -  100  200  400  -
    //   cols:       0      1     2     3
    //   used=1, soft=2, hard=3  (blocks in 1KB units, '*' = over soft limit)
    //
    // Format B (two-line, filesystem on own line):
    //   /dev/sda1
    //              1234   2048   4096   ...
    let mut prev_was_filesystem = false;
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("Disk") || trimmed.starts_with("File") {
            prev_was_filesystem = false;
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();

        // Format B: previous line was just a filesystem path
        let data_parts: &[&str] = if prev_was_filesystem {
            &parts
        } else if parts[0].starts_with('/') && parts.len() >= 4 {
            // Format A: filesystem is first column
            &parts[1..]
        } else {
            prev_was_filesystem = parts.len() == 1 && parts[0].starts_with('/');
            continue;
        };
        prev_was_filesystem = false;

        if data_parts.len() >= 3 {
            let used_kb = data_parts[0].trim_end_matches('*').parse::<u64>().ok()?;
            // prefer hard limit (col 2), fall back to soft (col 1)
            let limit_kb = data_parts[2].parse::<u64>()
                .ok()
                .filter(|&v| v > 0)
                .or_else(|| data_parts[1].parse::<u64>().ok().filter(|&v| v > 0))?;
            return Some((used_kb * 1024, limit_kb * 1024));
        }
    }
    None
}

fn try_df(session: &Session) -> Option<(u64, u64)> {
    let output = exec_command(session, "df -B1 . 2>/dev/null | tail -1");
    let parts: Vec<&str> = output.split_whitespace().collect();
    if parts.len() >= 3 {
        let total = parts[1].parse::<u64>().ok()?;
        let used = parts[2].parse::<u64>().ok()?;
        Some((used, total))
    } else {
        None
    }
}

#[tauri::command]
async fn connect_storage_box(
    state: State<'_, SharedState>,
    config: ConnectionConfig,
) -> Result<StorageStats, String> {
    let session = create_session(&config)?;
    let stats = get_disk_stats(&session);
    let id = config.id.clone();
    let mut s = state.lock().unwrap();
    s.connections.insert(id, SftpConnection { config, session });
    Ok(stats)
}

#[tauri::command]
async fn disconnect_storage_box(
    state: State<'_, SharedState>,
    connection_id: String,
) -> Result<(), String> {
    let mut s = state.lock().unwrap();
    s.connections.remove(&connection_id);
    Ok(())
}

#[tauri::command]
async fn get_storage_stats(
    state: State<'_, SharedState>,
    connection_id: String,
) -> Result<StorageStats, String> {
    let s = state.lock().unwrap();
    let conn = s.connections.get(&connection_id).ok_or("Not connected")?;
    Ok(get_disk_stats(&conn.session))
}

#[tauri::command]
async fn list_directory(
    state: State<'_, SharedState>,
    connection_id: String,
    path: String,
) -> Result<Vec<FileEntry>, String> {
    let s = state.lock().unwrap();
    let conn = s.connections.get(&connection_id).ok_or("Not connected")?;
    let sftp = conn.session.sftp().map_err(|e| e.to_string())?;
    let entries = sftp
        .readdir(Path::new(&path))
        .map_err(|e| format!("readdir failed: {}", e))?;

    let mut result: Vec<FileEntry> = entries
        .into_iter()
        .filter_map(|(pathbuf, stat)| {
            let name = pathbuf.file_name()?.to_string_lossy().to_string();
            if name == "." || name == ".." {
                return None;
            }
            Some(FileEntry {
                name: name.clone(),
                path: format!("{}/{}", path.trim_end_matches('/'), name),
                is_dir: stat.is_dir(),
                size: stat.size.unwrap_or(0),
            })
        })
        .collect();

    result.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(result)
}

#[tauri::command]
async fn get_dir_sizes(
    state: State<'_, SharedState>,
    connection_id: String,
    path: String,
) -> Result<HashMap<String, u64>, String> {
    let s = state.lock().unwrap();
    let conn = s.connections.get(&connection_id).ok_or("Not connected")?;
    // du -sb * gives size in bytes for each entry in the directory
    let cmd = format!("du -sb \"{}\"/* 2>/dev/null || true", path.trim_end_matches('/'));
    let mut channel = conn.session.channel_session().map_err(|e| e.to_string())?;
    channel.exec(&cmd).map_err(|e| e.to_string())?;
    let mut output = String::new();
    channel.read_to_string(&mut output).map_err(|e| e.to_string())?;
    channel.wait_close().ok();

    let mut sizes: HashMap<String, u64> = HashMap::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        if parts.len() == 2 {
            let size = parts[0].trim().parse::<u64>().unwrap_or(0);
            let name = parts[1].trim().split('/').last().unwrap_or("").to_string();
            if !name.is_empty() {
                sizes.insert(name, size);
            }
        }
    }
    Ok(sizes)
}

#[tauri::command]
async fn create_directory(
    state: State<'_, SharedState>,
    connection_id: String,
    path: String,
) -> Result<(), String> {
    let s = state.lock().unwrap();
    let conn = s.connections.get(&connection_id).ok_or("Not connected")?;
    let sftp = conn.session.sftp().map_err(|e| e.to_string())?;
    sftp.mkdir(Path::new(&path), 0o755)
        .map_err(|e| format!("mkdir failed: {}", e))?;
    Ok(())
}

#[tauri::command]
async fn start_upload_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    connection_id: String,
    local_paths: Vec<String>,
    remote_path: String,
) -> Result<String, String> {
    let job_id = Uuid::new_v4().to_string();

    let mut total_bytes: u64 = 0;
    for p in &local_paths {
        if let Ok(meta) = std::fs::metadata(p) {
            total_bytes += meta.len();
        }
    }

    let (conn_name, config) = {
        let s = state.lock().unwrap();
        let conn = s.connections.get(&connection_id).ok_or("Not connected")?;
        (conn.config.name.clone(), conn.config.clone())
    };

    let job = UploadJob {
        id: job_id.clone(),
        connection_id: connection_id.clone(),
        connection_name: conn_name,
        remote_path: remote_path.clone(),
        files: local_paths.clone(),
        total_bytes,
        transferred_bytes: 0,
        status: JobStatus::Queued,
        speed_bps: 0.0,
        eta_seconds: 0.0,
        started_at: now_secs(),
        finished_at: None,
        current_file: String::new(),
    };

    let cancel_flag = Arc::new(AtomicBool::new(false));

    {
        let mut s = state.lock().unwrap();
        s.jobs.insert(job_id.clone(), job);
        s.cancel_flags.insert(job_id.clone(), cancel_flag.clone());
    }

    let state_clone = state.inner().clone();
    let jid = job_id.clone();
    std::thread::spawn(move || {
        run_upload_job(app, state_clone, jid, config, local_paths, remote_path, cancel_flag);
    });

    Ok(job_id)
}

fn run_upload_job(
    app: AppHandle,
    state: SharedState,
    job_id: String,
    config: ConnectionConfig,
    local_paths: Vec<String>,
    remote_path: String,
    cancel_flag: Arc<AtomicBool>,
) {
    let session = match create_session(&config) {
        Ok(s) => s,
        Err(e) => {
            let mut s = state.lock().unwrap();
            if let Some(job) = s.jobs.get_mut(&job_id) {
                job.status = JobStatus::Failed(e);
                job.finished_at = Some(now_secs());
            }
            return;
        }
    };

    let sftp = match session.sftp() {
        Ok(s) => s,
        Err(e) => {
            let mut s = state.lock().unwrap();
            if let Some(job) = s.jobs.get_mut(&job_id) {
                job.status = JobStatus::Failed(e.to_string());
                job.finished_at = Some(now_secs());
            }
            return;
        }
    };

    {
        let mut s = state.lock().unwrap();
        if let Some(job) = s.jobs.get_mut(&job_id) {
            job.status = JobStatus::Running;
        }
    }

    let total_bytes = {
        let s = state.lock().unwrap();
        s.jobs.get(&job_id).map(|j| j.total_bytes).unwrap_or(0)
    };

    let start = Instant::now();
    let mut transferred_total: u64 = 0;

    for local_path in &local_paths {
        let file_name = Path::new(local_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let remote_file_path = format!("{}/{}", remote_path.trim_end_matches('/'), file_name);

        {
            let mut s = state.lock().unwrap();
            if let Some(job) = s.jobs.get_mut(&job_id) {
                job.current_file = file_name.clone();
            }
        }

        let mut local_file = match std::fs::File::open(local_path) {
            Ok(f) => f,
            Err(e) => {
                let mut s = state.lock().unwrap();
                if let Some(job) = s.jobs.get_mut(&job_id) {
                    job.status = JobStatus::Failed(format!("Open error: {}", e));
                    job.finished_at = Some(now_secs());
                }
                return;
            }
        };

        let mut remote_file = match sftp.create(Path::new(&remote_file_path)) {
            Ok(f) => f,
            Err(e) => {
                let mut s = state.lock().unwrap();
                if let Some(job) = s.jobs.get_mut(&job_id) {
                    job.status = JobStatus::Failed(format!("Create remote file error: {}", e));
                    job.finished_at = Some(now_secs());
                }
                return;
            }
        };

        // 256 KB chunks — sweet spot for SSH throughput
        const CHUNK: usize = 262144;
        let mut buf = vec![0u8; CHUNK];
        let mut last_emit = Instant::now();

        loop {
            let n = match local_file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    let mut s = state.lock().unwrap();
                    if let Some(job) = s.jobs.get_mut(&job_id) {
                        job.status = JobStatus::Failed(format!("Read error: {}", e));
                        job.finished_at = Some(now_secs());
                    }
                    return;
                }
            };

            if let Err(e) = remote_file.write_all(&buf[..n]) {
                let mut s = state.lock().unwrap();
                if let Some(job) = s.jobs.get_mut(&job_id) {
                    job.status = JobStatus::Failed(format!("Write error: {}", e));
                    job.finished_at = Some(now_secs());
                }
                return;
            }

            transferred_total += n as u64;

            // Check for cancellation
            if cancel_flag.load(Ordering::Relaxed) {
                let mut s = state.lock().unwrap();
                if let Some(job) = s.jobs.get_mut(&job_id) {
                    job.status = JobStatus::Failed("Cancelled".to_string());
                    job.finished_at = Some(now_secs());
                }
                app.emit("upload-progress", &UploadProgress {
                    job_id: job_id.clone(),
                    transferred_bytes: transferred_total,
                    total_bytes,
                    speed_bps: 0.0,
                    eta_seconds: 0.0,
                    status: JobStatus::Failed("Cancelled".to_string()),
                    current_file: String::new(),
                }).ok();
                return;
            }

            // Emit progress at most every 300ms to avoid overhead
            if last_emit.elapsed().as_millis() >= 300 {
                let elapsed = start.elapsed().as_secs_f64().max(0.001);
                let speed = transferred_total as f64 / elapsed;
                let remaining = total_bytes.saturating_sub(transferred_total);
                let eta = if speed > 0.0 { remaining as f64 / speed } else { 0.0 };

                {
                    let mut s = state.lock().unwrap();
                    if let Some(job) = s.jobs.get_mut(&job_id) {
                        job.transferred_bytes = transferred_total;
                        job.speed_bps = speed;
                        job.eta_seconds = eta;
                    }
                }

                app.emit("upload-progress", &UploadProgress {
                    job_id: job_id.clone(),
                    transferred_bytes: transferred_total,
                    total_bytes,
                    speed_bps: speed,
                    eta_seconds: eta,
                    status: JobStatus::Running,
                    current_file: file_name.clone(),
                }).ok();

                last_emit = Instant::now();
            }
        }
    }

    {
        let mut s = state.lock().unwrap();
        if let Some(job) = s.jobs.get_mut(&job_id) {
            job.status = JobStatus::Completed;
            job.transferred_bytes = total_bytes;
            job.eta_seconds = 0.0;
            job.finished_at = Some(now_secs());
        }
    }

    app.emit(
        "upload-progress",
        &UploadProgress {
            job_id,
            transferred_bytes: total_bytes,
            total_bytes,
            speed_bps: 0.0,
            eta_seconds: 0.0,
            status: JobStatus::Completed,
            current_file: String::new(),
        },
    )
    .ok();
}

#[tauri::command]
async fn get_all_jobs(state: State<'_, SharedState>) -> Result<Vec<UploadJob>, String> {
    let s = state.lock().unwrap();
    let mut jobs: Vec<UploadJob> = s.jobs.values().cloned().collect();
    jobs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(jobs)
}

#[tauri::command]
async fn cancel_job(state: State<'_, SharedState>, job_id: String) -> Result<(), String> {
    let mut s = state.lock().unwrap();
    if let Some(flag) = s.cancel_flags.get(&job_id) {
        flag.store(true, Ordering::Relaxed);
    }
    if let Some(job) = s.jobs.get_mut(&job_id) {
        if job.status == JobStatus::Queued {
            job.status = JobStatus::Failed("Cancelled".to_string());
            job.finished_at = Some(now_secs());
        }
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state: SharedState = Arc::new(Mutex::new(AppState::new()));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_saved_connections,
            save_connection,
            delete_connection,
            connect_storage_box,
            disconnect_storage_box,
            list_directory,
            get_dir_sizes,
            create_directory,
            start_upload_job,
            get_all_jobs,
            cancel_job,
            get_storage_stats,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
