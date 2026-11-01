$ErrorActionPreference='Stop'
$root='C:\Users\JAMAL\Desktop\Flafy'
$t=Join-Path $root 'src-tauri'
$lib=Join-Path $t 'src\lib.rs'
$utf8=New-Object System.Text.UTF8Encoding($false)
$code=@'
use serde::Serialize;
use std::{env,fs,path::PathBuf,process::Command,thread,time::Duration};
use sysinfo::{Disks,System};

#[derive(Serialize)]
struct Stats{cpu:f32,ram:f32,storage:f32,total_ram_gb:f64,used_ram_gb:f64}

#[tauri::command]
fn system_stats()->Result<Stats,String>{
    let mut s=System::new_all().map_err(|e|e.to_string())?;
    thread::sleep(Duration::from_millis(220));
    s.refresh_cpu_usage();
    s.refresh_memory();
    let total=s.total_memory() as f64;
    let used=s.used_memory() as f64;
    let ram=if total>0.0{(used/total*100.0) as f32}else{0.0};
    let disks=Disks::new_with_refreshed_list().map_err(|e|e.to_string())?;
    let(mut dt,mut da)=(0u64,0u64);
    for d in disks.list(){dt+=d.total_space();da+=d.available_space();}
    let storage=if dt>0{((dt-da) as f64/dt as f64*100.0) as f32}else{0.0};
    Ok(Stats{cpu:s.global_cpu_usage(),ram,storage,total_ram_gb:total/1073741824.0,used_ram_gb:used/1073741824.0})
}

fn home()->PathBuf{PathBuf::from(env::var("USERPROFILE").unwrap_or_else(|_|"C:\\Users\\JAMAL".into()))}

#[tauri::command]
fn open_location(kind:String)->Result<(),String>{
    let h=home();
    let t=match kind.as_str(){
        "flafy"=>h.join("Desktop\\Flafy"),
        "downloads"=>h.join("Downloads"),
        "desktop"=>h.join("Desktop"),
        "mofakrati"=>h.join("Desktop\\مفكرتي"),
        _=>return Err("unknown target".into())
    };
    Command::new("explorer").arg(t).spawn().map_err(|e|e.to_string())?;
    Ok(())
}

#[tauri::command]
fn project_status()->serde_json::Value{
    let h=home();
    serde_json::json!({
        "flafy":h.join("Desktop\\Flafy").exists(),
        "mofakrati":h.join("Desktop\\مفكرتي").exists(),
        "downloads":h.join("Downloads").exists()
    })
}

#[cfg_attr(mobile,tauri::mobile_entry_point)]
pub fn run(){
    tauri::Builder::default()
        .setup(|_|{
            let _=fs::write(env::temp_dir().join("Flafy_runtime_ok.txt"),"runtime-ok");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![system_stats,open_location,project_status])
        .run(tauri::generate_context!())
        .expect("Flafy failed");
}
'@
[IO.File]::WriteAllText($lib,$code,$utf8)
Remove-Item "$env:TEMP\Flafy_runtime_ok.txt" -Force -ErrorAction SilentlyContinue
Remove-Item "$root\FLAFY_COMPLETE.txt","$root\FLAFY_FAILED.txt" -Force -ErrorAction SilentlyContinue
Set-Location $root
npm run tauri build
if($LASTEXITCODE -ne 0){'BUILD_FAILED'|Set-Content "$root\FLAFY_FAILED.txt";exit 1}
$exe="$t\target\release\flafy.exe"
if(!(Test-Path $exe)){'EXE_MISSING'|Set-Content "$root\FLAFY_FAILED.txt";exit 1}
$p=Start-Process $exe -PassThru
Start-Sleep 12
$alive=Get-Process -Id $p.Id -ErrorAction SilentlyContinue
$marker=Test-Path "$env:TEMP\Flafy_runtime_ok.txt"
$conf=Get-Content "$t\tauri.conf.json" -Raw|ConvertFrom-Json
$uiok=(Test-Path "$root\dist\index.html") -and (Test-Path "$root\dist\styles.css") -and (Test-Path "$root\dist\app.js")
$modeok=($conf.app.windows[0].fullscreen -eq $true -and $conf.app.windows[0].decorations -eq $false)
if($alive -and $marker -and $uiok -and $modeok){
    "BUILD_OK`r`nRUNTIME_OK`r`nBORDERLESS_FULLSCREEN_OK`r`nUI_FILES_OK`r`nEXE=$exe"|Set-Content "$root\FLAFY_COMPLETE.txt"
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    Write-Output 'FLAFY_FULL_RUNTIME_PASS'
    exit 0
}
"RUNTIME_FAILED alive=$([bool]$alive) marker=$marker ui=$uiok fullscreen=$modeok"|Set-Content "$root\FLAFY_FAILED.txt"
exit 1
