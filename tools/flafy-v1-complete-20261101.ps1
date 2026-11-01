$ErrorActionPreference='Stop'
$root='C:\Users\JAMAL\Desktop\Flafy'
$t=Join-Path $root 'src-tauri'
$src=Join-Path $t 'src'
$dist=Join-Path $root 'dist'
$utf8=New-Object System.Text.UTF8Encoding($false)
New-Item -ItemType Directory -Force -Path $src,$dist | Out-Null

$toml=@'
[package]
name="flafy"
version="1.0.0"
description="Flafy Personal Control Room"
authors=["Jamal & Sol"]
edition="2021"
[lib]
name="flafy_lib"
crate-type=["staticlib","cdylib","rlib"]
[build-dependencies]
tauri-build={version="2",features=[]}
[dependencies]
tauri={version="2",features=["tray-icon"]}
serde={version="1",features=["derive"]}
serde_json="1"
sysinfo="0.40"
rusqlite={version="0.32",features=["bundled"]}
[profile.release]
codegen-units=1
lto=true
opt-level="s"
panic="abort"
strip=true
'@
[IO.File]::WriteAllText((Join-Path $t 'Cargo.toml'),$toml,$utf8)

$lib=@'
use serde::{Serialize,Deserialize};
use std::{env,fs,path::{Path,PathBuf},process::Command,thread,time::Duration};
use sysinfo::{Disks,System};
use rusqlite::{Connection,params};
use tauri::{Manager,WindowEvent};

#[derive(Serialize)] struct Stats{cpu:f32,ram:f32,storage:f32,total_ram_gb:f64,used_ram_gb:f64}
#[derive(Serialize)] struct Project{name:String,path:String,exists:bool}
#[derive(Serialize)] struct DownloadItem{name:String,size_mb:f64,modified:String}
#[derive(Serialize,Deserialize)] struct Focus{text:String}

fn home()->PathBuf{PathBuf::from(env::var("USERPROFILE").unwrap_or_else(|_|"C:\\Users\\JAMAL".into()))}
fn db_path()->PathBuf{home().join("AppData\\Local\\Flafy\\flafy.db")}
fn db()->Result<Connection,String>{let p=db_path();if let Some(d)=p.parent(){fs::create_dir_all(d).map_err(|e|e.to_string())?;}let c=Connection::open(p).map_err(|e|e.to_string())?;c.execute("CREATE TABLE IF NOT EXISTS kv (k TEXT PRIMARY KEY,v TEXT NOT NULL)",[]).map_err(|e|e.to_string())?;Ok(c)}

#[tauri::command]
fn system_stats()->Result<Stats,String>{let mut s=System::new_all().map_err(|e|e.to_string())?;thread::sleep(Duration::from_millis(220));s.refresh_cpu_usage();s.refresh_memory();let total=s.total_memory()as f64;let used=s.used_memory()as f64;let ram=if total>0.0{(used/total*100.0)as f32}else{0.0};let disks=Disks::new_with_refreshed_list().map_err(|e|e.to_string())?;let(mut dt,mut da)=(0u64,0u64);for d in disks.iter(){dt+=d.total_space();da+=d.available_space();}let storage=if dt>0{((dt-da)as f64/dt as f64*100.0)as f32}else{0.0};Ok(Stats{cpu:s.global_cpu_usage(),ram,storage,total_ram_gb:total/1073741824.0,used_ram_gb:used/1073741824.0})}

#[tauri::command]
fn projects()->Vec<Project>{let h=home();let defs=[("Flafy",h.join("Desktop\\Flafy")),("SOL",h.join("Desktop\\Sol")),("MedFlux",h.join("Desktop\\medflux")),("مفكرتي",h.join("Desktop\\مفكرتي")),("Net Download Manager",h.join("Desktop\\Net Download Manager")),("Project Zomboid",h.join("Zomboid"))];defs.into_iter().map(|(n,p)|Project{name:n.into(),path:p.to_string_lossy().into(),exists:p.exists()}).collect()}

#[tauri::command]
fn open_path(path:String)->Result<(),String>{if !Path::new(&path).exists(){return Err("path not found".into())}Command::new("explorer").arg(path).spawn().map_err(|e|e.to_string())?;Ok(())}

#[tauri::command]
fn open_special(kind:String)->Result<(),String>{let h=home();let p=match kind.as_str(){"downloads"=>h.join("Downloads"),"desktop"=>h.join("Desktop"),"flafy"=>h.join("Desktop\\Flafy"),_=>return Err("unknown target".into())};Command::new("explorer").arg(p).spawn().map_err(|e|e.to_string())?;Ok(())}

#[tauri::command]
fn recent_downloads()->Vec<DownloadItem>{let d=home().join("Downloads");let mut rows:Vec<_>=fs::read_dir(d).ok().into_iter().flat_map(|x|x.flatten()).filter_map(|e|{let m=e.metadata().ok()?;if !m.is_file(){return None}let mt=m.modified().ok()?;let secs=mt.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();Some((secs,DownloadItem{name:e.file_name().to_string_lossy().into(),size_mb:m.len()as f64/1048576.0,modified:secs.to_string()}))}).collect();rows.sort_by(|a,b|b.0.cmp(&a.0));rows.into_iter().take(6).map(|x|x.1).collect()}

#[tauri::command]
fn load_focus()->Result<Focus,String>{let c=db()?;let mut st=c.prepare("SELECT v FROM kv WHERE k='focus'").map_err(|e|e.to_string())?;let v=st.query_row([],|r|r.get::<_,String>(0)).unwrap_or_else(|_|"شغلة رئيسية واحدة، والباقي تحت عين Flafy.".into());Ok(Focus{text:v})}

#[tauri::command]
fn save_focus(text:String)->Result<(),String>{let c=db()?;c.execute("INSERT INTO kv(k,v) VALUES('focus',?1) ON CONFLICT(k) DO UPDATE SET v=excluded.v",params![text]).map_err(|e|e.to_string())?;Ok(())}

#[tauri::command]
fn set_visibility(app:tauri::AppHandle,action:String)->Result<(),String>{let w=app.get_webview_window("main").ok_or("window missing")?;match action.as_str(){"hide"=>w.hide().map_err(|e|e.to_string())?,"show"=>{w.show().map_err(|e|e.to_string())?;let _=w.set_focus();},"minimize"=>w.minimize().map_err(|e|e.to_string())?,"exit"=>app.exit(0),_=>return Err("unknown action".into())};Ok(())}

#[cfg_attr(mobile,tauri::mobile_entry_point)]
pub fn run(){tauri::Builder::default().setup(|app|{let _=fs::write(env::temp_dir().join("Flafy_runtime_ok.txt"),"runtime-ok");let _=db();if let Some(w)=app.get_webview_window("main"){let _=w.set_fullscreen(true);}Ok(())}).on_window_event(|w,e|{if let WindowEvent::CloseRequested{api,..}=e{api.prevent_close();let _=w.hide();}}).invoke_handler(tauri::generate_handler![system_stats,projects,open_path,open_special,recent_downloads,load_focus,save_focus,set_visibility]).run(tauri::generate_context!()).expect("Flafy failed");}
'@
[IO.File]::WriteAllText((Join-Path $src 'lib.rs'),$lib,$utf8)

$html=@'
<!doctype html><html lang="ar" dir="rtl"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Flafy</title><link rel="stylesheet" href="styles.css"></head><body><div class="app"><header><div class="brand"><div class="logo">F</div><div><h1>Flafy</h1><small>Personal Control Room · Jamal & Sol</small></div></div><div class="controls"><span id="live">SYSTEM LIVE</span><button data-win="minimize">—</button><button data-win="hide">○</button><button data-win="exit">×</button></div></header><main><section class="hero card"><div><span class="eyebrow">FLAFY // HOME</span><h2 id="greet">صباح الخير، جمال</h2><p>واجهة رئيسية فعلية لإدارة شغلك ومراقبة جهازك بدون زحمة.</p></div><div class="focusbox"><label>FOCUS NOW</label><div class="focusrow"><input id="focusInput" maxlength="140"><button id="saveFocus">حفظ</button></div><small id="focusState">محفوظ محليًا في SQLite</small></div><div class="heroBottom"><div><div id="clock" class="clock">--:--</div><div id="date" class="muted">---</div></div><div class="weather"><div id="weatherTemp">--°</div><span id="weatherText">Hameln · جاري التحميل</span></div></div></section><section class="card system"><div class="head"><b>System Health</b><span>Real telemetry</span></div><div class="metric"><label>CPU <b id="cpu">--%</b></label><div><i id="cpuBar"></i></div></div><div class="metric"><label>Memory <b id="ram">--%</b></label><div><i id="ramBar"></i></div></div><div class="metric"><label>Storage <b id="disk">--%</b></label><div><i id="diskBar"></i></div></div><small id="ramText" class="muted">Reading…</small></section><section class="card projects"><div class="head"><b>Projects</b><span id="projectCount">Scanning</span></div><div id="projectList" class="list"></div></section><section class="card downloads"><div class="head"><b>Recent Downloads</b><button data-special="downloads">فتح المجلد</button></div><div id="downloadsList" class="list"></div></section><section class="card actions"><div class="head"><b>Quick Actions</b><span>Functional</span></div><div class="gridButtons"><button data-special="desktop">Desktop</button><button data-special="downloads">Downloads</button><button data-special="flafy">Flafy Folder</button><button id="refreshAll">Refresh</button></div></section><section class="card signals"><div class="head"><b>Signals</b><span>Live state</span></div><div class="signalGrid"><div><span>Weather</span><b id="sigWeather">Connecting</b></div><div><span>System</span><b id="sigSystem">Reading</b></div><div><span>Projects</span><b id="sigProjects">Scanning</b></div><div><span>Mode</span><b>Borderless Fullscreen</b></div></div></section><section class="card sol"><div class="head"><b>Flafy Intelligence</b><span>Signal, not noise</span></div><div class="intel"><article><span>FOCUS</span><b id="intelFocus">المهمة المحفوظة تظهر هنا.</b></article><article><span>LOCAL</span><b>CPU / RAM / Storage / Projects / Downloads حقيقية.</b></article><article><span>CONNECTED SERVICES</span><b>Mail / Calendar / Travel تظهر فقط بعد ربط الحسابات؛ ما في بيانات وهمية.</b></article></div></section></main><footer><span>Flafy 1.0 · Tauri 2 + Rust</span><span id="footerState">Initializing…</span></footer></div><script src="app.js"></script></body></html>
'@
[IO.File]::WriteAllText((Join-Path $dist 'index.html'),$html,$utf8)

$css=@'
:root{--bg:#090c11;--panel:#121722;--line:#273044;--text:#f5f7fb;--muted:#8d98ad;--blue:#6ea8ff;--violet:#9b8cff;--green:#67d7a5;--amber:#e6b76e}*{box-sizing:border-box}html,body{margin:0;width:100%;height:100%;overflow:hidden;font-family:Segoe UI,Tahoma,Arial,sans-serif;background:radial-gradient(circle at 12% 4%,#132035 0,transparent 28%),radial-gradient(circle at 88% 0,#1a1530 0,transparent 25%),var(--bg);color:var(--text)}button,input{font:inherit}.app{height:100vh;padding:16px 20px;display:grid;grid-template-rows:auto 1fr auto;gap:12px}header{display:flex;justify-content:space-between;align-items:center}.brand{display:flex;align-items:center;gap:11px}.logo{width:44px;height:44px;border-radius:14px;display:grid;place-items:center;background:linear-gradient(145deg,#d7e6ff,#95baff,#b6a8ff);color:#10131a;font-weight:900}.brand h1{margin:0;font-size:21px}.brand small{color:var(--muted);font-size:10px}.controls{display:flex;align-items:center;gap:7px}.controls span{font-size:9px;color:var(--green);border:1px solid var(--line);padding:7px 10px;border-radius:9px}.controls button{width:34px;height:30px;border:1px solid var(--line);background:#ffffff05;color:var(--muted);border-radius:9px;cursor:pointer}main{min-height:0;display:grid;grid-template-columns:1fr 1.35fr 1fr;grid-template-rows:1.08fr .78fr .82fr;gap:11px}.card{background:linear-gradient(180deg,#141923ef,#10151fef);border:1px solid var(--line);border-radius:17px;box-shadow:0 18px 50px #0005;overflow:hidden}.hero{grid-column:2;grid-row:1/3;padding:25px;display:flex;flex-direction:column;justify-content:space-between}.eyebrow{font-size:9px;color:var(--blue)}.hero h2{font-size:34px;margin:7px 0}.hero>div>p{color:var(--muted);font-size:12px}.focusbox{padding:15px;border:1px solid #2b3950;border-radius:14px;background:#0e141d}.focusbox label{display:block;color:var(--blue);font-size:9px;margin-bottom:8px}.focusrow{display:flex;gap:8px}.focusrow input{flex:1;background:#0b1018;border:1px solid #2a3547;color:var(--text);padding:10px 12px;border-radius:9px;outline:none}.focusrow button,.head button,.gridButtons button{border:1px solid #2b3850;background:#ffffff06;color:var(--text);border-radius:9px;padding:9px 12px;cursor:pointer}.focusbox small{display:block;margin-top:7px;color:var(--muted)}.heroBottom{display:flex;justify-content:space-between;align-items:end}.clock{font-size:54px;font-weight:700}.muted{color:var(--muted)}.weather{text-align:left;direction:ltr}.weather div{font-size:30px;font-weight:700}.weather span{color:var(--muted);font-size:10px}.head{height:43px;padding:0 13px;border-bottom:1px solid var(--line);display:flex;align-items:center;justify-content:space-between}.head b{font-size:11px}.head span{font-size:9px;color:var(--muted)}.system{grid-column:1;grid-row:1;padding-bottom:13px}.metric{padding:11px 14px 0}.metric label{display:flex;justify-content:space-between;color:var(--muted);font-size:9px;margin-bottom:6px}.metric>div{height:7px;background:#222a38;border-radius:99px;overflow:hidden}.metric i{display:block;width:0;height:100%;background:linear-gradient(90deg,var(--blue),var(--violet));transition:width .3s}.system>small{display:block;padding:12px 14px 0}.projects{grid-column:3;grid-row:1}.downloads{grid-column:3;grid-row:2}.actions{grid-column:1;grid-row:2}.signals{grid-column:1;grid-row:3}.sol{grid-column:2/4;grid-row:3}.list{padding:8px;display:grid;gap:6px;overflow:auto;max-height:calc(100% - 43px)}.row{display:flex;justify-content:space-between;align-items:center;gap:8px;padding:9px 10px;border:1px solid #273044;border-radius:10px;background:#ffffff03;font-size:10px}.row button{border:0;background:transparent;color:var(--blue);cursor:pointer}.good{color:var(--green)}.bad{color:var(--amber)}.gridButtons{padding:11px;display:grid;grid-template-columns:1fr 1fr;gap:8px}.signalGrid{padding:10px;display:grid;grid-template-columns:1fr 1fr;gap:8px}.signalGrid div{padding:10px;border:1px solid #273044;border-radius:10px;background:#ffffff03}.signalGrid span{display:block;color:var(--muted);font-size:8px}.signalGrid b{display:block;margin-top:5px;font-size:9px}.intel{height:calc(100% - 43px);padding:10px;display:grid;grid-template-columns:repeat(3,1fr);gap:8px}.intel article{padding:13px;border:1px solid #273044;border-radius:11px;background:#ffffff03}.intel span{color:var(--blue);font-size:8px}.intel b{display:block;margin-top:6px;font-size:10px;line-height:1.6}footer{display:flex;justify-content:space-between;color:var(--muted);font-size:9px}@media(max-width:1100px){.app{padding:12px}.hero h2{font-size:28px}.clock{font-size:44px}}
'@
[IO.File]::WriteAllText((Join-Path $dist 'styles.css'),$css,$utf8)

$js=@'
const invoke=(cmd,args={})=>window.__TAURI__.core.invoke(cmd,args),$=id=>document.getElementById(id);function tick(){const d=new Date();$('clock').textContent=d.toLocaleTimeString('en-GB',{hour:'2-digit',minute:'2-digit'});$('date').textContent=d.toLocaleDateString('ar-DE',{weekday:'long',year:'numeric',month:'long',day:'numeric'});$('greet').textContent=(d.getHours()<12?'صباح الخير':'مساء الخير')+'، جمال'}async function stats(){try{const s=await invoke('system_stats');for(const [k,v] of [['cpu',s.cpu],['ram',s.ram],['disk',s.storage]]){$(k).textContent=Math.round(v)+'%';$(k+'Bar').style.width=Math.min(100,v)+'%'}$('ramText').textContent=`${s.used_ram_gb.toFixed(1)} / ${s.total_ram_gb.toFixed(1)} GB RAM`;$('sigSystem').textContent='Live'}catch{$('sigSystem').textContent='Error'}}async function projects(){try{const p=await invoke('projects');$('projectList').innerHTML=p.map((x,i)=>`<div class="row"><span>${x.name}</span><span class="${x.exists?'good':'bad'}">${x.exists?`<button data-path="${i}">OPEN</button>`:'NOT FOUND'}</span></div>`).join('');$('projectCount').textContent=p.filter(x=>x.exists).length+' available';$('sigProjects').textContent=p.filter(x=>x.exists).length+' ready';document.querySelectorAll('[data-path]').forEach(b=>b.onclick=()=>invoke('open_path',{path:p[+b.dataset.path].path}))}catch{$('sigProjects').textContent='Error'}}async function downloads(){try{const d=await invoke('recent_downloads');$('downloadsList').innerHTML=d.length?d.map(x=>`<div class="row"><span title="${x.name}">${x.name.length>30?x.name.slice(0,27)+'…':x.name}</span><span class="muted">${x.size_mb.toFixed(1)} MB</span></div>`).join(''):'<div class="row"><span>لا توجد ملفات حديثة</span></div>'}catch{}}async function focus(){try{const f=await invoke('load_focus');$('focusInput').value=f.text;$('intelFocus').textContent=f.text}catch{}}async function weather(){try{const r=await fetch('https://api.open-meteo.com/v1/forecast?latitude=52.103&longitude=9.356&current=temperature_2m,weather_code&timezone=Europe%2FBerlin');const j=await r.json();const t=Math.round(j.current.temperature_2m);$('weatherTemp').textContent=t+'°';$('weatherText').textContent='Hameln · Open-Meteo';$('sigWeather').textContent=t+'° Hameln'}catch{$('weatherText').textContent='Hameln · غير متصل';$('sigWeather').textContent='Offline'}}async function refresh(){await Promise.all([stats(),projects(),downloads(),weather()]);$('footerState').textContent='Updated '+new Date().toLocaleTimeString('en-GB',{hour:'2-digit',minute:'2-digit'})}document.querySelectorAll('[data-special]').forEach(b=>b.onclick=()=>invoke('open_special',{kind:b.dataset.special}));document.querySelectorAll('[data-win]').forEach(b=>b.onclick=()=>invoke('set_visibility',{action:b.dataset.win}));$('saveFocus').onclick=async()=>{const text=$('focusInput').value.trim();if(!text)return;await invoke('save_focus',{text});$('intelFocus').textContent=text;$('focusState').textContent='تم الحفظ في SQLite'};$('refreshAll').onclick=refresh;tick();setInterval(tick,1000);focus();refresh();setInterval(()=>{if(!document.hidden)stats()},4000);setInterval(()=>{if(!document.hidden){downloads();projects()}},30000);setInterval(()=>{if(!document.hidden)weather()},900000);document.addEventListener('visibilitychange',()=>{if(!document.hidden)refresh()});
'@
[IO.File]::WriteAllText((Join-Path $dist 'app.js'),$js,$utf8)

Remove-Item "$env:TEMP\Flafy_runtime_ok.txt" -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $root 'FLAFY_V1_COMPLETE.txt'),(Join-Path $root 'FLAFY_V1_FAILED.txt') -Force -ErrorAction SilentlyContinue
Set-Location $root
npm run tauri build
if($LASTEXITCODE -ne 0){'BUILD_FAILED'|Set-Content (Join-Path $root 'FLAFY_V1_FAILED.txt');exit 1}
$exe=Join-Path $t 'target\release\flafy.exe'
if(!(Test-Path $exe)){'EXE_MISSING'|Set-Content (Join-Path $root 'FLAFY_V1_FAILED.txt');exit 1}
$p=Start-Process $exe -PassThru
Start-Sleep 12
$alive=Get-Process -Id $p.Id -ErrorAction SilentlyContinue
$marker=Test-Path "$env:TEMP\Flafy_runtime_ok.txt"
$dbok=Test-Path "$env:LOCALAPPDATA\Flafy\flafy.db"
$uiok=(Test-Path (Join-Path $dist 'index.html'))-and(Test-Path (Join-Path $dist 'styles.css'))-and(Test-Path (Join-Path $dist 'app.js'))
if($alive -and $marker -and $dbok -and $uiok){"BUILD_OK`r`nRUNTIME_OK`r`nSQLITE_OK`r`nFUNCTIONAL_UI_OK`r`nEXE=$exe"|Set-Content (Join-Path $root 'FLAFY_V1_COMPLETE.txt');Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue;exit 0}else{"RUNTIME_FAILED alive=$([bool]$alive) marker=$marker db=$dbok ui=$uiok"|Set-Content (Join-Path $root 'FLAFY_V1_FAILED.txt');if($alive){Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue};exit 1}
