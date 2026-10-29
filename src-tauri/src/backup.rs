use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

fn stamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn psq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let source = entry.path();
        let target = dst.join(entry.file_name());
        if source.is_dir() {
            copy_dir(&source, &target)?;
        } else {
            fs::copy(&source, &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn run_ps(script: String) -> Result<(), String> {
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).to_string())
    }
}

#[tauri::command]
pub fn create_backup(
    a: tauri::AppHandle,
    destination_path: String,
    state: Value,
) -> Result<String, String> {
    let tmp = std::env::temp_dir().join(format!("mufakkirati-backup-{}", stamp()));
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;

    let state_bytes = serde_json::to_vec_pretty(&state).map_err(|e| e.to_string())?;
    fs::write(tmp.join("state.json"), state_bytes).map_err(|e| e.to_string())?;

    let settings = a
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("settings.json");
    if settings.is_file() {
        fs::copy(settings, tmp.join("settings.json")).map_err(|e| e.to_string())?;
    }

    let attachments = a
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("attachments");
    if attachments.is_dir() {
        copy_dir(&attachments, &tmp.join("attachments"))?;
    }

    let dst = PathBuf::from(&destination_path);
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if dst.exists() {
        fs::remove_file(&dst).map_err(|e| e.to_string())?;
    }

    let script = format!(
        "Compress-Archive -Path {} -DestinationPath {} -Force",
        psq(&format!("{}\\*", tmp.display())),
        psq(&destination_path)
    );
    let result = run_ps(script);
    let _ = fs::remove_dir_all(&tmp);
    result?;
    Ok(destination_path)
}
#[tauri::command]
pub fn restore_backup(
    a: tauri::AppHandle,
    source_path: String,
    current_state: Value,
) -> Result<Value, String> {
    let safety_dir = a.path().app_data_dir()
        .map_err(|e| e.to_string())?
        .join("backups");
    fs::create_dir_all(&safety_dir).map_err(|e| e.to_string())?;
    let safety = safety_dir.join(format!("pre-restore-{}.zip", stamp()));

    create_backup(
        a.clone(),
        safety.to_string_lossy().to_string(),
        current_state,
    )?;

    let tmp = std::env::temp_dir()
        .join(format!("mufakkirati-restore-{}", stamp()));
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;

    run_ps(format!(
        "Expand-Archive -LiteralPath {} -DestinationPath {} -Force",
        psq(&source_path),
        psq(&tmp.to_string_lossy())
    ))?;

    let state_file = tmp.join("state.json");
    if !state_file.is_file() {
        let _ = fs::remove_dir_all(&tmp);
        return Err("invalid backup: state.json missing".into());
    }

    let mut state: Value = serde_json::from_slice(
        &fs::read(&state_file).map_err(|e| e.to_string())?
    ).map_err(|e| e.to_string())?;

    let dst_root = a.path().app_data_dir()
        .map_err(|e| e.to_string())?
        .join("attachments");
    fs::create_dir_all(&dst_root).map_err(|e| e.to_string())?;

    if let Some(notes) = state.get_mut("notes").and_then(|v| v.as_object_mut()) {
        for arr in notes.values_mut().filter_map(|v| v.as_array_mut()) {
            for note in arr {
                if let Some(atts) = note.get_mut("attachments").and_then(|v| v.as_array_mut()) {
                    for att in atts {
                        let old = att.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        if let Some(name) = Path::new(old).file_name() {
                            let src = tmp.join("attachments").join(name);
                            let dst = dst_root.join(name);
                            if src.is_file() {
                                fs::copy(&src, &dst).map_err(|e| e.to_string())?;
                                att["path"] = Value::String(dst.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    let settings = fs::read(tmp.join("settings.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .unwrap_or(Value::Null);

    let result = serde_json::json!({
        "state": state,
        "settings": settings,
        "safetyPath": safety.to_string_lossy()
    });

    let _ = fs::remove_dir_all(&tmp);
    Ok(result)
}