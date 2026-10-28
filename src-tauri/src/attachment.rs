use serde::Serialize;
use std::{fs,path::Path,time::{SystemTime,UNIX_EPOCH}};
use tauri::Manager;

#[derive(Serialize)]
pub struct ImportedAttachment{
 pub id:String,
 pub kind:String,
 pub path:String,
 pub display_name:String
}

#[tauri::command]
pub fn import_attachment(a:tauri::AppHandle,source_path:String)->Result<ImportedAttachment,String>{
 let src=Path::new(&source_path);
 if !src.is_file(){return Err("source file does not exist".into());}
 let name=src.file_name().and_then(|v|v.to_str()).unwrap_or("attachment").to_string();
 let ext=src.extension().and_then(|v|v.to_str()).unwrap_or("").to_lowercase();
 let kind=match ext.as_str(){
  "png"|"jpg"|"jpeg"|"gif"|"webp"|"bmp"|"svg"=>"image",
  "mp4"|"mov"|"avi"|"mkv"|"webm"=>"video",
  "mp3"|"wav"|"ogg"|"m4a"|"flac"=>"audio",
  _=>"file"
 }.to_string();
 let safe:String=name.chars().map(|c|if c.is_ascii_alphanumeric()||".-_ ".contains(c){c}else{'_'}).collect();
 let stamp=SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos();
 let id=format!("a{}",stamp);
 let dir=a.path().app_data_dir().map_err(|e|e.to_string())?.join("attachments");
 fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
 let dst=dir.join(format!("{}_{}",id,safe));
 fs::copy(src,&dst).map_err(|e|e.to_string())?;
 Ok(ImportedAttachment{id,kind,path:dst.to_string_lossy().to_string(),display_name:name})
}
#[tauri::command]
pub fn discard_attachment(a:tauri::AppHandle,path:String)->Result<(),String>{
 let root=a.path().app_data_dir().map_err(|e|e.to_string())?.join("attachments");
 let target=Path::new(&path);
 if !target.exists(){return Ok(());}
 let root_canon=fs::canonicalize(&root).map_err(|e|e.to_string())?;
 let target_canon=fs::canonicalize(target).map_err(|e|e.to_string())?;
 if !target_canon.starts_with(&root_canon){return Err("refusing path outside attachment storage".into());}
 fs::remove_file(target_canon).map_err(|e|e.to_string())?;
 Ok(())
}