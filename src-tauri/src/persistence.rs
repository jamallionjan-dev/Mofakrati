use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{collections::HashSet,path::PathBuf};
use tauri::Manager;
fn state_db_path(a:&tauri::AppHandle)->Result<PathBuf,String>{
 #[cfg(debug_assertions)]
 if let Some(p)=std::env::var_os("MUFAKKIRATI_TEST_DB_PATH"){
  return Ok(PathBuf::from(p));
 }
 a.path().app_config_dir()
  .map_err(|e|e.to_string())
  .map(|p|p.join("mufakkirati.db"))
}

#[tauri::command]
pub async fn load_persisted_state(a:tauri::AppHandle)->Result<Option<serde_json::Value>,String>{
 let p=state_db_path(&a)?;
 if !p.exists(){return Ok(None)}
 let o=SqliteConnectOptions::new().filename(&p).create_if_missing(false);
 let mut c=SqliteConnection::connect_with(&o).await.map_err(|e|e.to_string())?;
 let row:Option<(String,)>=sqlx::query_as(
  "SELECT value FROM app_meta WHERE key='state' LIMIT 1"
 ).fetch_optional(&mut c).await.map_err(|e|e.to_string())?;
 match row{
  Some((raw,))=>serde_json::from_str(&raw).map(Some).map_err(|e|e.to_string()),
  None=>Ok(None)
 }
}
#[tauri::command]
pub async fn persist_state_atomic(
    a: tauri::AppHandle,
    mut state: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let p = state_db_path(&a)?;

    let o = SqliteConnectOptions::new()
        .filename(&p)
        .create_if_missing(true);
    let mut c = SqliteConnection::connect_with(&o)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("PRAGMA foreign_keys=ON")
        .execute(&mut c)
        .await
        .map_err(|e| e.to_string())?;
    let attachment_root = a
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("attachments");
    let mut tx = c.begin().await.map_err(|e| e.to_string())?;

    let fired_pairs: HashSet<(String, Option<String>)> =
        sqlx::query_as("SELECT id,reminder_at FROM notes WHERE reminder_fired=1")
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .collect();

    if let Some(groups) = state.get_mut("notes").and_then(|v| v.as_object_mut()) {
        for group in groups.values_mut() {
            if let Some(arr) = group.as_array_mut() {
                for n in arr {
                    let id = n.get("id").and_then(|v| v.as_str()).map(str::to_string);
                    let at = n.get("reminderAt").and_then(|v| v.as_str())
                        .filter(|v| !v.is_empty()).map(str::to_string);
                    let keep = id.map(|id| fired_pairs.contains(&(id, at))).unwrap_or(false);
                    if keep {
                        if let Some(o) = n.as_object_mut() {
                            o.insert("reminderFired".into(), serde_json::Value::Bool(true));
                        }
                    }
                }
            }
        }
    }
    let actions: Vec<(String,String)> =
        sqlx::query_as("SELECT key,value FROM app_meta WHERE key LIKE 'reminder_action:%'")
            .fetch_all(&mut *tx).await.map_err(|e|e.to_string())?;

    for (key,raw) in actions {
        if let Ok(action)=serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(id)=action.get("id").and_then(|v|v.as_str()) {
                let at=action.get("reminderAt").cloned().unwrap_or(serde_json::Value::Null);
                let fired=action.get("reminderFired").and_then(|v|v.as_bool()).unwrap_or(false);
                if let Some(groups)=state.get_mut("notes").and_then(|v|v.as_object_mut()) {
                    for group in groups.values_mut() {
                        if let Some(arr)=group.as_array_mut() {
                            for note in arr {
                                if note.get("id").and_then(|v|v.as_str())==Some(id) {
                                    if let Some(o)=note.as_object_mut() {
                                        o.insert("reminderAt".into(),at.clone());
                                        o.insert("reminderFired".into(),serde_json::Value::Bool(fired));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        sqlx::query("DELETE FROM app_meta WHERE key=?").bind(key)
            .execute(&mut *tx).await.map_err(|e|e.to_string())?;
    }

    let json = serde_json::to_string(&state).map_err(|e| e.to_string())?;
    sqlx::query("INSERT INTO app_meta(key,value,updated_at) VALUES('state',?,CURRENT_TIMESTAMP) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=CURRENT_TIMESTAMP").bind(json).execute(&mut *tx).await.map_err(|e|e.to_string())?;
    let cols = state
        .get("columns")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let notes = state.get("notes").and_then(|v| v.as_object());
    let mut keep_cols = HashSet::new();
    let mut keep_notes = HashSet::new();
    let mut keep_attachments = HashSet::new();
    for (ci, col) in cols.iter().enumerate() {
        let id = match col.get("id").and_then(|v| v.as_str()) {
            Some(v) => v,
            None => continue,
        };
        keep_cols.insert(id.to_string());
        let title = col.get("label").and_then(|v| v.as_str()).unwrap_or("");
        sqlx::query("INSERT INTO columns(id,title,position,created_at,updated_at) VALUES(?,?,?,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP) ON CONFLICT(id) DO UPDATE SET title=excluded.title,position=excluded.position,updated_at=CURRENT_TIMESTAMP").bind(id).bind(title).bind(ci as i64).execute(&mut *tx).await.map_err(|e|e.to_string())?;
        if let Some(arr) = notes.and_then(|m| m.get(id)).and_then(|v| v.as_array()) {
            for (ni, n) in arr.iter().enumerate() {
                let nid = match n.get("id").and_then(|v| v.as_str()) {
                    Some(v) => v,
                    None => continue,
                };
                keep_notes.insert(nid.to_string());
                let title = n.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let body = n.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let rating = n.get("rating").and_then(|v| v.as_i64()).unwrap_or(0);
                let emoji = n.get("emoji").and_then(|v| v.as_str()).unwrap_or("");
                let date = n.get("date").and_then(|v| v.as_str());
                let priority = n.get("priority").and_then(|v| v.as_str()).unwrap_or("weak");
                let reminder_at = n
                    .get("reminderAt")
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.is_empty());
                let reminder_fired = n
                    .get("reminderFired")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                sqlx::query("INSERT INTO notes(id,column_id,title,body,rating,position,created_at,updated_at,emoji,note_date,priority,reminder_at,reminder_fired) VALUES(?,?,?,?,?,?,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET column_id=excluded.column_id,title=excluded.title,body=excluded.body,rating=excluded.rating,position=excluded.position,updated_at=CURRENT_TIMESTAMP,emoji=excluded.emoji,note_date=excluded.note_date,priority=excluded.priority,reminder_at=excluded.reminder_at,reminder_fired=excluded.reminder_fired")
       .bind(nid).bind(id).bind(title).bind(body).bind(rating).bind(ni as i64).bind(emoji).bind(date).bind(priority).bind(reminder_at).bind(if reminder_fired { 1_i64 } else { 0_i64 })
      .execute(&mut *tx).await.map_err(|e|e.to_string())?;
                sqlx::query("DELETE FROM note_tags WHERE note_id=?")
                    .bind(nid)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| e.to_string())?;
                if let Some(tags) = n.get("chips").and_then(|v| v.as_array()) {
                    for t in tags {
                        if let Some(tag) = t.as_str() {
                            sqlx::query("INSERT OR IGNORE INTO note_tags(note_id,tag) VALUES(?,?)")
                                .bind(nid)
                                .bind(tag)
                                .execute(&mut *tx)
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                    }
                }
                if let Some(atts) = n.get("attachments").and_then(|v| v.as_array()) {
                    for a in atts {
                        let aid = match a.get("id").and_then(|v| v.as_str()) {
                            Some(v) => v,
                            None => continue,
                        };
                        keep_attachments.insert(aid.to_string());
                        let kind = a.get("kind").and_then(|v| v.as_str()).unwrap_or("file");
                        let path = a.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        let name = a
                            .get("displayName")
                            .or_else(|| a.get("display_name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        sqlx::query("INSERT INTO attachments(id,note_id,kind,path,display_name,created_at) VALUES(?,?,?,?,?,CURRENT_TIMESTAMP) ON CONFLICT(id) DO UPDATE SET note_id=excluded.note_id,kind=excluded.kind,path=excluded.path,display_name=excluded.display_name").bind(aid).bind(nid).bind(kind).bind(path).bind(name).execute(&mut *tx).await.map_err(|e|e.to_string())?;
                    }
                }
            }
        }
    }
    let old_atts: Vec<(String, String)> = sqlx::query_as("SELECT id,path FROM attachments")
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    let mut stale_attachment_paths = Vec::new();
    for (aid, path) in old_atts {
        if !keep_attachments.contains(&aid) {
            stale_attachment_paths.push(path);
            sqlx::query("DELETE FROM attachments WHERE id=?")
                .bind(aid)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    let old_notes: Vec<(String,)> = sqlx::query_as("SELECT id FROM notes")
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    for (id,) in old_notes {
        if !keep_notes.contains(&id) {
            sqlx::query("DELETE FROM notes WHERE id=?")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    let old_cols: Vec<(String,)> = sqlx::query_as("SELECT id FROM columns")
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    for (id,) in old_cols {
        if !keep_cols.contains(&id) {
            sqlx::query("DELETE FROM columns WHERE id=?")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    if let Ok(root_canon) = std::fs::canonicalize(&attachment_root) {
        for path in stale_attachment_paths {
            if let Ok(target_canon) = std::fs::canonicalize(&path) {
                if target_canon.starts_with(&root_canon) {
                    let _ = std::fs::remove_file(target_canon);
                }
            }
        }
    }
    Ok(state)
}
