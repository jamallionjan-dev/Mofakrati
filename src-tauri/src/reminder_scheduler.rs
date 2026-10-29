use serde::Deserialize;
use serde_json::{json,Value};
use sqlx::{Connection,Row};
use std::{io::Write,path::PathBuf,process::{Command,Stdio}};
#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Deserialize)]
pub struct ReminderTask{pub id:String,pub at:String}

fn reminder_arg()->Option<String>{
 let mut a=std::env::args().skip(1);
 while let Some(x)=a.next(){
  if x=="--reminder-id"{return a.next().filter(|v|!v.is_empty())}
  if let Some(v)=x.strip_prefix("--reminder-id="){
   if !v.is_empty(){return Some(v.to_string())}
  }
 }
 None
}

fn uri_decode(v:&str)->Option<String>{
 let b=v.as_bytes();let mut o=Vec::new();let mut i=0;
 while i<b.len(){
  if b[i]==b'%'{
   if i+2>=b.len(){return None}
   let h=|c:u8|match c{b'0'..=b'9'=>Some(c-b'0'),b'a'..=b'f'=>Some(c-b'a'+10),b'A'..=b'F'=>Some(c-b'A'+10),_=>None};
   o.push(h(b[i+1])?*16+h(b[i+2])?);i+=3;
  }else{o.push(b[i]);i+=1}
 }
 String::from_utf8(o).ok()
}

fn reminder_protocol_arg()->Option<(String,String,u32)>{
 for x in std::env::args().skip(1){
  let Some(r)=x.strip_prefix("mufakkirati://reminder/") else{continue};
  if let Some(id)=r.strip_prefix("done/"){
   return Some(("done".into(),uri_decode(id)?,0))
  }
  if let Some(v)=r.strip_prefix("snooze/"){
   let mut p=v.splitn(2,'/');
   let m=p.next()?.parse::<u32>().ok()?;
   let id=uri_decode(p.next()?)?;
   if (1..=1440).contains(&m){return Some(("snooze".into(),id,m))}
  }
 }
 None
}
async fn apply_protocol_action(a:&str,id:&str,m:u32)->Result<Option<String>,String>{
 let base=std::env::var_os("APPDATA").ok_or("APPDATA missing")?;
 #[cfg(debug_assertions)]
 let db=std::env::var_os("MUFAKKIRATI_TEST_DB_PATH").map(PathBuf::from)
  .unwrap_or_else(||PathBuf::from(&base).join("com.jamal.mufakkirati").join("mufakkirati.db"));
 #[cfg(not(debug_assertions))]
 let db=PathBuf::from(base).join("com.jamal.mufakkirati").join("mufakkirati.db");
 if !db.exists(){return Err("database missing".into())}
 let o=sqlx::sqlite::SqliteConnectOptions::new().filename(db).create_if_missing(false);
 let mut c=sqlx::SqliteConnection::connect_with(&o).await.map_err(|e|e.to_string())?;
 let mut tx=c.begin().await.map_err(|e|e.to_string())?;
 let n=if a=="snooze"{
  sqlx::query("UPDATE notes SET reminder_at=strftime('%Y-%m-%dT%H:%M:%SZ','now',?),reminder_fired=0,updated_at=CURRENT_TIMESTAMP WHERE id=?")
   .bind(format!("+{m} minutes")).bind(id).execute(&mut *tx).await.map_err(|e|e.to_string())?.rows_affected()
 }else{
  sqlx::query("UPDATE notes SET reminder_at=NULL,reminder_fired=1,updated_at=CURRENT_TIMESTAMP WHERE id=?")
   .bind(id).execute(&mut *tx).await.map_err(|e|e.to_string())?.rows_affected()
 };
 // REMINDER_PROTOCOL_STATE_SYNC
 let mut scheduled_at=None;
 if n>0{
  let q=sqlx::query("SELECT reminder_at FROM notes WHERE id=?").bind(id)
   .fetch_one(&mut *tx).await.map_err(|e|e.to_string())?;
  let at:Option<String>=q.try_get("reminder_at").map_err(|e|e.to_string())?;
  scheduled_at=at.clone();
  if let Some(r)=sqlx::query("SELECT value FROM app_meta WHERE key='state' LIMIT 1")
   .fetch_optional(&mut *tx).await.map_err(|e|e.to_string())?{
   let raw:String=r.try_get("value").map_err(|e|e.to_string())?;
   let mut st:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
   if let Some(gs)=st.get_mut("notes").and_then(Value::as_object_mut){
    for g in gs.values_mut(){if let Some(arr)=g.as_array_mut(){for note in arr{
     if note.get("id").and_then(Value::as_str)==Some(id){
      if let Some(o)=note.as_object_mut(){
       o.insert("reminderAt".into(),at.clone().map(Value::String).unwrap_or(Value::Null));
       o.insert("reminderFired".into(),Value::Bool(a!="snooze"));
      }
     }
    }}}
   }
   let out=serde_json::to_string(&st).map_err(|e|e.to_string())?;
   sqlx::query("UPDATE app_meta SET value=?,updated_at=CURRENT_TIMESTAMP WHERE key='state'")
    .bind(out).execute(&mut *tx).await.map_err(|e|e.to_string())?;
  }
 }
  if n>0{
   let marker=json!({"id":id,"reminderAt":scheduled_at.clone(),"reminderFired":a!="snooze"}).to_string();
   sqlx::query("INSERT INTO app_meta(key,value,updated_at) VALUES(?,?,CURRENT_TIMESTAMP) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=CURRENT_TIMESTAMP")
    .bind(format!("reminder_action:{id}")).bind(marker)
    .execute(&mut *tx).await.map_err(|e|e.to_string())?;
  }
  #[cfg(windows)]
  let scheduled_task=if a=="snooze"{
   if let Some(ref at)=scheduled_at{
    if let Err(e)=schedule_one_task(id,at){
     remove_task(id);
     return Err(e)
    }
    true
   }else{false}
  }else{false};

  if let Err(e)=tx.commit().await{
   #[cfg(windows)]
   if scheduled_task{remove_task(id);}
   return Err(e.to_string())
  }
  Ok(if n>0{scheduled_at}else{None})
}
async fn claim_due(id:&str,notify:bool)->Result<Option<(String,String)>,String>{
 let base=std::env::var_os("APPDATA").ok_or("APPDATA missing")?;
 #[cfg(debug_assertions)]
 let db=std::env::var_os("MUFAKKIRATI_TEST_DB_PATH")
  .map(PathBuf::from)
  .unwrap_or_else(||PathBuf::from(&base).join("com.jamal.mufakkirati").join("mufakkirati.db"));
 #[cfg(not(debug_assertions))]
 let db=PathBuf::from(base).join("com.jamal.mufakkirati").join("mufakkirati.db");
 if !db.exists(){return Err(format!("database missing: {}",db.display()))}
 let opt=sqlx::sqlite::SqliteConnectOptions::new()
  .filename(&db).create_if_missing(false);
 let mut c=sqlx::SqliteConnection::connect_with(&opt)
  .await.map_err(|e|e.to_string())?;
 let mut tx=c.begin().await.map_err(|e|e.to_string())?;
 let row=sqlx::query(
  "UPDATE notes SET reminder_fired=1,updated_at=CURRENT_TIMESTAMP WHERE id=? AND reminder_fired=0 AND reminder_at IS NOT NULL AND datetime(reminder_at)<=datetime('now','+45 seconds') RETURNING title,body"
 ).bind(id).fetch_optional(&mut *tx).await.map_err(|e|e.to_string())?;

 let Some(row)=row else{
  tx.commit().await.map_err(|e|e.to_string())?;
  return Ok(None)
 };

 let title:String=row.try_get("title").unwrap_or_default();
 let body:String=row.try_get("body").unwrap_or_default();

 if let Some(m)=sqlx::query("SELECT value FROM app_meta WHERE key='state' LIMIT 1")
  .fetch_optional(&mut *tx).await.map_err(|e|e.to_string())?{
  let raw:String=m.try_get("value").map_err(|e|e.to_string())?;
  let mut state:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
  let mut changed=false;
  if let Some(groups)=state.get_mut("notes").and_then(Value::as_object_mut){
   for group in groups.values_mut(){
    if let Some(arr)=group.as_array_mut(){
     for note in arr{
      if note.get("id").and_then(Value::as_str)==Some(id){
       if let Some(o)=note.as_object_mut(){
        o.insert("reminderFired".into(),Value::Bool(true));
        changed=true;
       }
      }
     }
    }
   }
  }
  if changed{
   let out=serde_json::to_string(&state).map_err(|e|e.to_string())?;
   sqlx::query("UPDATE app_meta SET value=?,updated_at=CURRENT_TIMESTAMP WHERE key='state'")
    .bind(out).execute(&mut *tx).await.map_err(|e|e.to_string())?;
  }
 }

 #[cfg(windows)]
{
    #[cfg(debug_assertions)]
    let suppress_toast =
        std::env::var_os("MUFAKKIRATI_HEADLESS_TEST_NO_TOAST").is_some();

    #[cfg(not(debug_assertions))]
    let suppress_toast = false;

    if notify && !suppress_toast {
        toast(id, &title, &body)?;
    }
}
 #[cfg(not(windows))]
 let _=notify;

 tx.commit().await.map_err(|e|e.to_string())?;
 Ok(Some((title,body)))
}
#[tauri::command]
pub async fn claim_reminder(id:String)->Result<bool,String>{
 Ok(claim_due(&id,false).await?.is_some())
}

#[cfg(windows)]
fn uri_id(v:&str)->String{
 use std::fmt::Write;
 let mut o=String::new();
 for &b in v.as_bytes(){
  if b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_'|b'.'|b'~'){o.push(b as char)}
  else{let _=write!(&mut o,"%{b:02X}");}
 }
 o
}
#[cfg(windows)]
fn xe(v:&str)->String{
 v.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;")
}
#[cfg(windows)]
fn toast(id:&str,title:&str,body:&str)->Result<(),String>{
 use windows::{core::HSTRING,Data::Xml::Dom::XmlDocument,UI::Notifications::{ToastNotification,ToastNotificationManager}};
 let t=if title.trim().is_empty(){"Reminder"}else{title};
 let b=if body.trim().is_empty(){"Reminder due"}else{body};
 let i=uri_id(id);
 let o=format!("mufakkirati://reminder/open/{i}");
 let d=format!("mufakkirati://reminder/done/{i}");
 let z=format!("mufakkirati://reminder/snooze/10/{i}");
 let x=format!(r#"<toast launch="{o}" activationType="protocol"><visual><binding template="ToastGeneric"><text>Mufakkirati</text><text>{t}</text><text>{b}</text></binding></visual><actions><action content="Open" arguments="{o}" activationType="protocol"/><action content="Done" arguments="{d}" activationType="protocol"/><action content="Snooze 10m" arguments="{z}" activationType="protocol"/></actions></toast>"#,o=o,d=d,z=z,t=xe(t),b=xe(b));
 let doc=XmlDocument::new().map_err(|e|e.to_string())?;
 doc.LoadXml(&HSTRING::from(x)).map_err(|e|e.to_string())?;
 let n=ToastNotification::CreateToastNotification(&doc).map_err(|e|e.to_string())?;
 let q=ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from("com.jamal.mufakkirati")).map_err(|e|e.to_string())?;
 q.Show(&n).map_err(|e|e.to_string())?;
 std::thread::sleep(std::time::Duration::from_millis(10));
 Ok(())
}

#[cfg(windows)]
fn remove_task(id:&str){
 let safe:String=id.chars()
  .map(|c|if c.is_ascii_alphanumeric()||c=='_'||c=='-'{c}else{'_'})
  .collect();
 let name=format!("Mufakkirati_Reminder_{safe}");
 let mut c=Command::new("schtasks.exe");
 c.args(["/Delete","/TN",&name,"/F"])
  .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
 c.creation_flags(0x08000000);
 let _=c.status();
}
#[cfg(windows)]
fn schedule_one_task(id:&str,at:&str)->Result<(),String>{
 let exe=std::env::current_exe().map_err(|e|e.to_string())?;
 let j=json!({"exe":exe.to_string_lossy(),"id":id,"at":at}).to_string();
 let ps=r#"$j=[Console]::In.ReadToEnd()|ConvertFrom-Json;$id=($j.id-replace'[^A-Za-z0-9_-]','_');$n='Mufakkirati_Reminder_'+$id;$at=[DateTimeOffset]::Parse($j.at).LocalDateTime;$a=New-ScheduledTaskAction -Execute $j.exe -Argument ('--reminder-id "'+$j.id+'"');$t=New-ScheduledTaskTrigger -Once -At $at;$s=New-ScheduledTaskSettingsSet -StartWhenAvailable;Register-ScheduledTask -TaskName $n -Action $a -Trigger $t -Settings $s -Description 'Mufakkirati reminder' -Force|Out-Null"#;
 let mut c=Command::new("powershell.exe");
 c.args(["-NoProfile","-NonInteractive","-ExecutionPolicy","Bypass","-Command",ps])
  .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
 c.creation_flags(0x08000000);
 let mut ch=c.spawn().map_err(|e|e.to_string())?;
 ch.stdin.as_mut().ok_or("stdin")?.write_all(j.as_bytes()).map_err(|e|e.to_string())?;
 let o=ch.wait_with_output().map_err(|e|e.to_string())?;
 if !o.status.success(){return Err(String::from_utf8_lossy(&o.stderr).trim().to_string())}
 Ok(())
}
pub fn run_headless_from_args()->Option<i32>{
 if let Some((action,id,minutes))=reminder_protocol_arg(){
  let result=tauri::async_runtime::block_on(async{
   let at=apply_protocol_action(&action,&id,minutes).await?;
   Ok::<(),String>(())
  });
  return Some(match result{
   Ok(_)=>0,
   Err(e)=>{
    eprintln!("headless reminder action: {e}");
    1
   }
  })
 }
 let id=reminder_arg()?;
 let result=tauri::async_runtime::block_on(async{
  claim_due(&id,true).await.map(|_|())
 });
 Some(match result{
  Ok(_)=>{
   #[cfg(windows)]
   remove_task(&id);
   0
  },
  Err(e)=>{
   eprintln!("headless reminder: {e}");
   1
  }
 })
}
#[tauri::command]
pub fn sync_reminder_tasks(tasks:Vec<ReminderTask>)->Result<u32,String>{
 #[cfg(not(windows))]
 {let _=tasks;return Ok(0)}

 #[cfg(windows)]
 {
  let exe=std::env::current_exe().map_err(|e|e.to_string())?;
  let payload=json!({"exe":exe.to_string_lossy(),"tasks":tasks.iter().map(|x|json!({"id":x.id,"at":x.at})).collect::<Vec<_>>()}).to_string();
  let ps=r#"
$j=[Console]::In.ReadToEnd()|ConvertFrom-Json
$p='Mufakkirati_Reminder_';$keep=@{}
foreach($n in $j.tasks){
 $at=[DateTimeOffset]::Parse($n.at).LocalDateTime
 if($at-le(Get-Date).AddSeconds(-5)){continue}
 $id=($n.id-replace'[^A-Za-z0-9_-]','_');$name=$p+$id;$keep[$name]=$true
 $a=New-ScheduledTaskAction -Execute $j.exe -Argument ('--reminder-id "'+$n.id+'"')
 $t=New-ScheduledTaskTrigger -Once -At $at
 $s=New-ScheduledTaskSettingsSet -StartWhenAvailable
 Register-ScheduledTask -TaskName $name -Action $a -Trigger $t -Settings $s -Description 'Mufakkirati reminder' -Force|Out-Null
}
Get-ScheduledTask -TaskName ($p+'*') -EA SilentlyContinue|?{-not $keep.ContainsKey($_.TaskName)}|Unregister-ScheduledTask -Confirm:$false
"#;
  let mut c=Command::new("powershell.exe");
  c.args(["-NoProfile","-NonInteractive","-ExecutionPolicy","Bypass","-Command",ps]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
  c.creation_flags(0x08000000);
  let mut ch=c.spawn().map_err(|e|e.to_string())?;
  ch.stdin.as_mut().ok_or("stdin")?.write_all(payload.as_bytes()).map_err(|e|e.to_string())?;
  let o=ch.wait_with_output().map_err(|e|e.to_string())?;
  if !o.status.success(){return Err(String::from_utf8_lossy(&o.stderr).trim().to_string())}
  Ok(tasks.len() as u32)
 }
}