use base64::Engine as _;
use serde::Serialize;
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

#[derive(Serialize)]
pub struct ImportedAttachment {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub display_name: String,
}

#[derive(Serialize)]
pub struct AttachmentInfo {
    pub display_name: String,
    pub size_bytes: u64,
}

#[tauri::command]
pub fn attachment_info(source_path: String) -> Result<AttachmentInfo, String> {
    let src = Path::new(&source_path);
    if !src.is_file() {
        return Err("source file does not exist".into());
    }
    let meta = fs::metadata(src).map_err(|e| e.to_string())?;
    Ok(AttachmentInfo {
        display_name: src.file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("attachment")
            .to_string(),
        size_bytes: meta.len(),
    })
}
#[tauri::command]
pub fn import_attachment(
    a: tauri::AppHandle,
    source_path: String,
) -> Result<ImportedAttachment, String> {
    let src = Path::new(&source_path);
    if src.is_file() {
        let size = fs::metadata(src).map_err(|e| e.to_string())?.len();
        if size > 100 * 1024 * 1024 {
            return Err("attachment exceeds 100 MB".into());
        }
    }
    if !src.is_file() {
        return Err("source file does not exist".into());
    }
    let name = src
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("attachment")
        .to_string();
    let ext = src
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_lowercase();
    let kind = match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" => "image",
        "mp4" | "mov" | "avi" | "mkv" | "webm" => "video",
        "mp3" | "wav" | "ogg" | "m4a" | "flac" => "audio",
        _ => "file",
    }
    .to_string();
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || ".-_ ".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let id = format!("a{}", stamp);
    let dir = a
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("attachments");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dst = dir.join(format!("{}_{}", id, safe));
    fs::copy(src, &dst).map_err(|e| e.to_string())?;
    Ok(ImportedAttachment {
        id,
        kind,
        path: dst.to_string_lossy().to_string(),
        display_name: name,
    })
}

#[derive(Serialize)]
pub struct AttachmentPreview { pub data_url:String }

#[tauri::command]
pub fn attachment_preview(a:tauri::AppHandle,path:String)->Result<AttachmentPreview,String>{
 use tauri::Manager;
 let root=fs::canonicalize(a.path().app_data_dir().map_err(|e|e.to_string())?.join("attachments")).map_err(|e|e.to_string())?;
 let p=fs::canonicalize(Path::new(&path)).map_err(|e|e.to_string())?;
 if !p.starts_with(&root){return Err("outside attachment storage".into())}
 let ext=p.extension().and_then(|x|x.to_str()).unwrap_or("").to_ascii_lowercase();
 let mime=match ext.as_str(){
  "png"=>"image/png","jpg"|"jpeg"=>"image/jpeg","gif"=>"image/gif",
  "webp"=>"image/webp","bmp"=>"image/bmp",_=>return Err("unsupported image".into())
 };
 if fs::metadata(&p).map_err(|e|e.to_string())?.len()>20*1024*1024{return Err("image too large".into())}
 let b=fs::read(p).map_err(|e|e.to_string())?;
 Ok(AttachmentPreview{data_url:format!("data:{};base64,{}",mime,base64::engine::general_purpose::STANDARD.encode(b))})
}
#[tauri::command]
pub fn discard_attachment(a: tauri::AppHandle, path: String) -> Result<(), String> {
    let root = a
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("attachments");
    let target = Path::new(&path);
    if !target.exists() {
        return Ok(());
    }
    let root_canon = fs::canonicalize(&root).map_err(|e| e.to_string())?;
    let target_canon = fs::canonicalize(target).map_err(|e| e.to_string())?;
    if !target_canon.starts_with(&root_canon) {
        return Err("refusing path outside attachment storage".into());
    }
    fs::remove_file(target_canon).map_err(|e| e.to_string())?;
    Ok(())
}
