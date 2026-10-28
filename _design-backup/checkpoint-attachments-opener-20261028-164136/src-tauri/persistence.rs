use std::collections::HashSet;
use sqlx::{Connection,SqliteConnection,sqlite::SqliteConnectOptions};
use tauri::Manager;
#[tauri::command]
pub async fn persist_state_atomic(a:tauri::AppHandle,state:serde_json::Value)->Result<(),String>{
 let p=a.path().app_config_dir().map_err(|e|e.to_string())?.join("mufakkirati.db");
 let o=SqliteConnectOptions::new().filename(&p).create_if_missing(true);
 let mut c=SqliteConnection::connect_with(&o).await.map_err(|e|e.to_string())?;
 sqlx::query("PRAGMA foreign_keys=ON").execute(&mut c).await.map_err(|e|e.to_string())?;
 let mut tx=c.begin().await.map_err(|e|e.to_string())?;
 let json=serde_json::to_string(&state).map_err(|e|e.to_string())?;
 sqlx::query("INSERT INTO app_meta(key,value,updated_at) VALUES('state',?,CURRENT_TIMESTAMP) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=CURRENT_TIMESTAMP").bind(json).execute(&mut *tx).await.map_err(|e|e.to_string())?;
 let cols=state.get("columns").and_then(|v|v.as_array()).cloned().unwrap_or_default();
 let notes=state.get("notes").and_then(|v|v.as_object());
 let mut keep_cols=HashSet::new();
 let mut keep_notes=HashSet::new();
 for (ci,col) in cols.iter().enumerate(){
  let id=match col.get("id").and_then(|v|v.as_str()){Some(v)=>v,None=>continue};
  keep_cols.insert(id.to_string());
  let title=col.get("label").and_then(|v|v.as_str()).unwrap_or("");
  sqlx::query("INSERT INTO columns(id,title,position,created_at,updated_at) VALUES(?,?,?,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP) ON CONFLICT(id) DO UPDATE SET title=excluded.title,position=excluded.position,updated_at=CURRENT_TIMESTAMP").bind(id).bind(title).bind(ci as i64).execute(&mut *tx).await.map_err(|e|e.to_string())?;
  if let Some(arr)=notes.and_then(|m|m.get(id)).and_then(|v|v.as_array()){
   for (ni,n) in arr.iter().enumerate(){
    let nid=match n.get("id").and_then(|v|v.as_str()){Some(v)=>v,None=>continue};
    keep_notes.insert(nid.to_string());
    let title=n.get("title").and_then(|v|v.as_str()).unwrap_or("");
    let body=n.get("text").and_then(|v|v.as_str()).unwrap_or("");
    let rating=n.get("rating").and_then(|v|v.as_i64()).unwrap_or(0);
    let emoji=n.get("emoji").and_then(|v|v.as_str()).unwrap_or("");
    let date=n.get("date").and_then(|v|v.as_str());
    sqlx::query("INSERT INTO notes(id,column_id,title,body,rating,position,created_at,updated_at,emoji,note_date) VALUES(?,?,?,?,?,?,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,?,?) ON CONFLICT(id) DO UPDATE SET column_id=excluded.column_id,title=excluded.title,body=excluded.body,rating=excluded.rating,position=excluded.position,updated_at=CURRENT_TIMESTAMP,emoji=excluded.emoji,note_date=excluded.note_date")
      .bind(nid).bind(id).bind(title).bind(body).bind(rating).bind(ni as i64).bind(emoji).bind(date)
      .execute(&mut *tx).await.map_err(|e|e.to_string())?;
    sqlx::query("DELETE FROM note_tags WHERE note_id=?").bind(nid).execute(&mut *tx).await.map_err(|e|e.to_string())?;
    if let Some(tags)=n.get("chips").and_then(|v|v.as_array()){
     for t in tags{
      if let Some(tag)=t.as_str(){
       sqlx::query("INSERT OR IGNORE INTO note_tags(note_id,tag) VALUES(?,?)").bind(nid).bind(tag).execute(&mut *tx).await.map_err(|e|e.to_string())?;
      }
     }
    }
     sqlx::query("DELETE FROM attachments WHERE note_id=?").bind(nid).execute(&mut *tx).await.map_err(|e|e.to_string())?;
     if let Some(atts)=n.get("attachments").and_then(|v|v.as_array()){
      for a in atts{
       let aid=match a.get("id").and_then(|v|v.as_str()){Some(v)=>v,None=>continue};
       let kind=a.get("kind").and_then(|v|v.as_str()).unwrap_or("file");
       let path=a.get("path").and_then(|v|v.as_str()).unwrap_or("");
       let name=a.get("displayName").or_else(||a.get("display_name")).and_then(|v|v.as_str()).unwrap_or("");
       sqlx::query("INSERT INTO attachments(id,note_id,kind,path,display_name,created_at) VALUES(?,?,?,?,?,CURRENT_TIMESTAMP)").bind(aid).bind(nid).bind(kind).bind(path).bind(name).execute(&mut *tx).await.map_err(|e|e.to_string())?;
      }
     }
   }
  }
 }
 let old_notes:Vec<(String,)>=sqlx::query_as("SELECT id FROM notes").fetch_all(&mut *tx).await.map_err(|e|e.to_string())?;
 for (id,) in old_notes{if !keep_notes.contains(&id){sqlx::query("DELETE FROM notes WHERE id=?").bind(id).execute(&mut *tx).await.map_err(|e|e.to_string())?;}}
 let old_cols:Vec<(String,)>=sqlx::query_as("SELECT id FROM columns").fetch_all(&mut *tx).await.map_err(|e|e.to_string())?;
 for (id,) in old_cols{if !keep_cols.contains(&id){sqlx::query("DELETE FROM columns WHERE id=?").bind(id).execute(&mut *tx).await.map_err(|e|e.to_string())?;}}
 tx.commit().await.map_err(|e|e.to_string())?;
 Ok(())
}
