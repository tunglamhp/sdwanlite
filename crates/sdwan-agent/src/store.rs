//! Persistent device store for `sdwan-agent`.
//!
//! Replaces the P0 in-memory store with a SQLite-backed implementation while
//! preserving the existing controller contract:
//!
//! * `DeviceStore::new() -> Arc<Self>`
//! * `insert(&self, rec: DeviceRecord) -> Result<()>`
//! * `get(&self, id: DeviceId) -> Result<DeviceRecord>`
//! * `replace_config(&self, id: DeviceId, new_config: DeviceConfig) -> Result<DeviceRecord>`
//! * `delete(&self, id: DeviceId) -> Result<()>`
//! * `list(&self) -> Result<Vec<DeviceRecord>>`
//!
//! Safety/defaults:
//! * Database file is created with mode `0600` where supported.
//! * WAL mode is enabled at open.
//! * Schema is auto-migrated from `migrations/001_init.sql` on first open.

use std::any::Any;
use std::path::Path;
use std::sync::Arc;

use crate::error::{AgentError, Result as AgentResult};
use sdwan_core::{DeviceConfig, DeviceId, DeviceState, OrgId, SiteId};
use serde::Serialize;
use tokio::sync::{broadcast, Mutex};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database open failed: {0}")]
    Open(String),
    #[error("migration failed: {0}")]
    Migration(String),
    #[error("device not found: {0}")]
    NotFound(DeviceId),
    #[error("duplicate device: {0}")]
    Duplicate(DeviceId),
    #[error("sqlite: {0}")]
    Rusqlite(#[from] rusqlite::Error),
}

impl From<StoreError> for AgentError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::NotFound(_id) => AgentError::NotFound,
            _ => AgentError::Storage(err.to_string()),
        }
    }
}

/// Storage backend abstraction.
pub trait Storage: Send + Sync {
    fn insert_device(&self, rec: &DeviceRecord) -> AgentResult<()>;
    fn get_device(&self, id: DeviceId) -> AgentResult<DeviceRecord>;
    fn replace_config(&self, id: DeviceId, new_config: DeviceConfig) -> AgentResult<DeviceRecord>;
    fn delete_device(&self, id: DeviceId) -> AgentResult<()>;
    fn list_devices(&self) -> AgentResult<Vec<DeviceRecord>>;
    fn insert_telemetry(&self, _frame: &crate::telemetry::TelemetryFrame) -> AgentResult<()> {
        let _ = _frame;
        Ok(())
    }
    fn as_any(&self) -> &dyn Any;
}

/// In-memory store for tests and fallback.
#[derive(Default)]
pub struct MemoryStore {
    inner: std::sync::Mutex<std::collections::HashMap<DeviceId, DeviceRecord>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Storage for MemoryStore {
    fn insert_device(&self, rec: &DeviceRecord) -> AgentResult<()> {
        let mut g = self.inner.lock().unwrap();
        g.insert(rec.current.device_id, rec.clone());
        Ok(())
    }

    fn get_device(&self, id: DeviceId) -> AgentResult<DeviceRecord> {
        self.inner
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(id).into())
    }

    fn replace_config(&self, id: DeviceId, new_config: DeviceConfig) -> AgentResult<DeviceRecord> {
        let mut g = self.inner.lock().unwrap();
        let rec = g.get_mut(&id).ok_or(StoreError::NotFound(id))?;
        rec.current = new_config.clone();
        Ok(rec.clone())
    }

    fn delete_device(&self, id: DeviceId) -> AgentResult<()> {
        let mut g = self.inner.lock().unwrap();
        g.remove(&id)
            .map(|_| ())
            .ok_or_else(|| StoreError::NotFound(id).into())
    }

    fn list_devices(&self) -> AgentResult<Vec<DeviceRecord>> {
        let g = self.inner.lock().unwrap();
        Ok(g.values().cloned().collect())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// SQLite-backed persistent store.
pub struct SqliteStore {
    conn: std::sync::Mutex<rusqlite::Connection>,
}

impl SqliteStore {
    /// Open `db_path` and apply required pragmas + migrations.
    pub fn open<P: AsRef<Path>>(db_path: P) -> AgentResult<Self> {
        let path = db_path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StoreError::Open(e.to_string()))?;
        }
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .map_err(|e| StoreError::Open(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| StoreError::Open(e.to_string()))?;
        }
        let conn = rusqlite::Connection::open(path).map_err(|e| StoreError::Open(e.to_string()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=5000;",
        )
        .map_err(|e| StoreError::Open(e.to_string()))?;

        let store = Self {
            conn: std::sync::Mutex::new(conn),
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> AgentResult<()> {
        let sql = include_str!("../migrations/001_init.sql");
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(sql)
            .map_err(|e: rusqlite::Error| StoreError::Migration(e.to_string()))?;
        Ok(())
    }
}

impl Storage for SqliteStore {
    fn insert_device(&self, rec: &DeviceRecord) -> AgentResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO devices(device_id, org_id, site_id, hostname, status) VALUES(?1, ?2, ?3, ?4, ?5)",
            [
                &rec.current.device_id.to_string(),
                &rec.org_id.to_string(),
                &rec.site_id.to_string(),
                &rec.hostname,
                &String::from("provisioned"),
            ],
        )
        .map_err(|e: rusqlite::Error| StoreError::Migration(e.to_string()))?;
        Ok(())
    }

    fn get_device(&self, id: DeviceId) -> AgentResult<DeviceRecord> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT device_id, org_id, site_id, hostname FROM devices WHERE device_id = ?1",
            )
            .map_err(|e: rusqlite::Error| StoreError::Open(e.to_string()))?;
        let rec = stmt
            .query_row([id.to_string()], map_row)
            .map_err(|_| StoreError::NotFound(id))?;
        Ok(rec)
    }

    fn replace_config(&self, id: DeviceId, new_config: DeviceConfig) -> AgentResult<DeviceRecord> {
        let conn = self.conn.lock().unwrap();
        let version = new_config.version;
        let payload =
            serde_json::to_string(&new_config).map_err(|e| StoreError::Open(e.to_string()))?;
        conn.execute(
            "INSERT INTO configs(device_id, version, config_json) VALUES(?1, ?2, ?3)",
            [&id.to_string(), &version.to_string(), &payload],
        )
        .map_err(|e: rusqlite::Error| StoreError::Migration(e.to_string()))?;
        self.get_device(id)
    }

    fn delete_device(&self, id: DeviceId) -> AgentResult<()> {
        let conn = self.conn.lock().unwrap();
        let rows = conn
            .execute("DELETE FROM devices WHERE device_id = ?1", [id.to_string()])
            .map_err(|e: rusqlite::Error| StoreError::Migration(e.to_string()))?;
        if rows == 0 {
            return Err(StoreError::NotFound(id).into());
        }
        Ok(())
    }

    fn list_devices(&self) -> AgentResult<Vec<DeviceRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT device_id, org_id, site_id, hostname FROM devices")
            .map_err(|e: rusqlite::Error| StoreError::Open(e.to_string()))?;
        let rows = stmt
            .query_map([], map_row)
            .map_err(|e: rusqlite::Error| StoreError::Open(e.to_string()))?;
        let mut out: Vec<DeviceRecord> = Vec::new();
        for r in rows {
            out.push(r.map_err(|e: rusqlite::Error| StoreError::Open(e.to_string()))?);
        }
        Ok(out)
    }

    fn insert_telemetry(&self, frame: &crate::telemetry::TelemetryFrame) -> AgentResult<()> {
        let conn = self.conn.lock().unwrap();
        let payload = serde_json::to_string(frame).map_err(|e| StoreError::Open(e.to_string()))?;
        conn.execute(
            "INSERT INTO telemetry(device_id, received_at, payload) VALUES(?1, strftime('%Y-%m-%dT%H:%M:%fZ','now'), ?2)",
            [&frame.device_id.to_string(), &payload],
        )
        .map_err(|e: rusqlite::Error| StoreError::Migration(e.to_string()))?;
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DeviceRecord> {
    let device_id_str: String = row.get(0)?;
    let org_id_str: String = row.get(1)?;
    let site_id_str: String = row.get(2)?;
    let hostname: String = row.get(3)?;
    Ok(DeviceRecord {
        org_id: org_id_str.parse::<OrgId>().expect("invalid org_id in db"),
        device_id: device_id_str
            .parse::<DeviceId>()
            .expect("invalid device_id in db"),
        site_id: site_id_str
            .parse::<SiteId>()
            .expect("invalid site_id in db"),
        hostname: hostname.clone(),
        state: DeviceState::Connected,
        current: DeviceConfig {
            device_id: device_id_str
                .parse::<DeviceId>()
                .expect("invalid device_id in db"),
            org_id: org_id_str.parse::<OrgId>().expect("invalid org_id in db"),
            site_id: site_id_str
                .parse::<SiteId>()
                .expect("invalid site_id in db"),
            hostname: hostname.clone(),
            interfaces: Vec::new(),
            tunnels: Vec::new(),
            routes: Vec::new(),
            firewall: Default::default(),
            qos: Default::default(),
            path_labels: Vec::new(),
            version: sdwan_core::ConfigVersion::new(0),
        },
        tx: std::sync::Arc::new(tokio::sync::broadcast::channel::<DeviceConfig>(64).0),
    })
}

/// Device record stored in the controller.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct DeviceRecord {
    pub org_id: OrgId,
    pub device_id: DeviceId,
    pub site_id: SiteId,
    pub hostname: String,
    pub state: DeviceState,
    pub current: DeviceConfig,
    #[serde(skip)]
    #[schemars(skip)]
    pub tx: std::sync::Arc<tokio::sync::broadcast::Sender<DeviceConfig>>,
}

/// Shared handle used by controllers.
#[derive(Clone)]
pub struct DeviceStore {
    storage: Arc<dyn Storage>,
    broadcasts: Arc<Mutex<std::collections::HashMap<DeviceId, broadcast::Sender<DeviceConfig>>>>,
    telemetry: Arc<Mutex<std::collections::HashMap<DeviceId, crate::telemetry::TelemetryFrame>>>,
    alerts: Arc<Mutex<Vec<AlertEvent>>>,
    alerted_flags: Arc<Mutex<std::collections::HashMap<DeviceId, std::collections::HashSet<String>>>>,
}
/// One controller alert (link down, degraded, ...). Newest last.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct AlertEvent {
    pub id: u64,
    pub kind: String,
    pub title: String,
    pub detail: Option<String>,
    pub at: String,
}

impl DeviceStore {
    /// Construct an empty in-memory store wrapped in an `Arc` ready to share across handlers.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            storage: Arc::new(MemoryStore::new()),
            broadcasts: Arc::new(Mutex::new(std::collections::HashMap::new())),
            telemetry: Arc::new(Mutex::new(std::collections::HashMap::new())),
            alerts: Arc::new(Mutex::new(Vec::new())),
            alerted_flags: Arc::new(Mutex::new(std::collections::HashMap::new())),
        })
    }

    /// Open a SQLite-backed store.
    pub fn sqlite<P: AsRef<Path>>(db_path: P) -> AgentResult<Arc<Self>> {
        let backend = SqliteStore::open(db_path)?;
        Ok(Arc::new(Self {
            storage: Arc::new(backend),
            broadcasts: Arc::new(Mutex::new(std::collections::HashMap::new())),
            telemetry: Arc::new(Mutex::new(std::collections::HashMap::new())),
            alerts: Arc::new(Mutex::new(Vec::new())),
            alerted_flags: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }))
    }

    /// Insert a new device.
    pub async fn insert(&self, rec: DeviceRecord) -> AgentResult<()> {
        self.storage.insert_device(&rec)?;
        let (tx, _rx) = broadcast::channel::<DeviceConfig>(64);
        self.broadcasts
            .lock()
            .await
            .insert(rec.current.device_id, tx);
        Ok(())
    }

    /// Fetch a device record (clone).
    pub async fn get(&self, id: DeviceId) -> AgentResult<DeviceRecord> {
        let mut rec = self.storage.get_device(id)?;
        if let Some(tx) = self.broadcasts.lock().await.get(&id) {
            rec.tx = Arc::new(tx.clone());
        }
        Ok(rec)
    }

    /// Replace a device's current config (used by `/apply`).
    pub async fn replace_config(
        &self,
        id: DeviceId,
        new_config: DeviceConfig,
    ) -> AgentResult<DeviceRecord> {
        let rec = self.storage.replace_config(id, new_config.clone())?;
        if let Some(tx) = self.broadcasts.lock().await.get(&rec.current.device_id) {
            let _ = tx.send(new_config);
        }
        Ok(rec)
    }

    /// Delete a device.
    pub async fn delete(&self, id: DeviceId) -> AgentResult<()> {
        self.storage.delete_device(id)?;
        self.broadcasts.lock().await.remove(&id);
        self.telemetry.lock().await.remove(&id);
        self.alerted_flags.lock().await.remove(&id);
        Ok(())
    }
    /// Update device metadata (only fields provided).
    pub async fn update_meta(
        &self,
        id: DeviceId,
        org_id: Option<OrgId>,
        site_id: Option<SiteId>,
        hostname: Option<String>,
    ) -> AgentResult<DeviceRecord> {
        let mut rec = self.storage.get_device(id)?;
        if let Some(org) = org_id {
            rec.org_id = org;
        }
        if let Some(site) = site_id {
            rec.site_id = site;
        }
        if let Some(name) = hostname {
            rec.hostname = name.clone();
            rec.current.hostname = name;
        }
        self.storage.insert_device(&rec)?;
        Ok(rec)
    }

    /// Flag keys that appeared since the previous telemetry frame (alert on
    /// state transitions only, so a repeating flag does not spam alerts).
    pub async fn new_alert_flags(
        &self,
        device_id: DeviceId,
        keys: Vec<String>,
    ) -> Vec<String> {
        let mut guard = self.alerted_flags.lock().await;
        let prev = guard.entry(device_id).or_default();
        let fresh: Vec<String> = keys.iter().filter(|k| !prev.contains(*k)).cloned().collect();
        prev.clear();
        prev.extend(keys);
        fresh
    }

    /// List all devices.
    pub async fn list(&self) -> AgentResult<Vec<DeviceRecord>> {
        self.storage.list_devices()
    }

    /// Record telemetry.
    pub async fn insert_telemetry(
        &self,
        frame: &crate::telemetry::TelemetryFrame,
    ) -> AgentResult<()> {
        self.storage.insert_telemetry(frame)?;
        self.telemetry
            .lock()
            .await
            .insert(frame.device_id.clone(), frame.clone());
        Ok(())
    }
    /// Latest telemetry frame per device (in-memory; P1 adds persistence reads).
    pub async fn latest_telemetry(&self) -> Vec<crate::telemetry::TelemetryFrame> {
        let guard = self.telemetry.lock().await;
        guard.values().cloned().collect()
    }
    /// Append an alert to the ring buffer (keeps the newest 100).
    pub async fn push_alert(&self, kind: &str, title: String, detail: Option<String>) {
        let mut guard = self.alerts.lock().await;
        let id = guard.last().map(|a| a.id + 1).unwrap_or(1);
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default();
        guard.push(AlertEvent {
            id,
            kind: kind.to_string(),
            title,
            detail,
            at,
        });
        if guard.len() > 100 {
            let excess = guard.len() - 100;
            guard.drain(0..excess);
        }
    }

    /// All alerts, oldest first.

    pub async fn alerts(&self) -> Vec<AlertEvent> {
        self.alerts.lock().await.clone()
    }

    /// Subscribe to config pushes for a device.
    pub async fn subscribe(
        &self,
        device_id: DeviceId,
    ) -> AgentResult<tokio::sync::broadcast::Receiver<DeviceConfig>> {
        let mut guard = self.broadcasts.lock().await;
        if let Some(tx) = guard.get(&device_id) {
            return Ok(tx.subscribe());
        }
        let (tx, rx) = broadcast::channel::<DeviceConfig>(64);
        guard.insert(device_id, tx);
        Ok(rx)
    }
}
