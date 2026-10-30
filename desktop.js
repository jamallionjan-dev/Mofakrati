import {I18N,tr} from "./i18n.js";
import { listen } from "@tauri-apps/api/event";
import {invoke} from "@tauri-apps/api/core";
import {check} from "@tauri-apps/plugin-updater";
import {getCurrentWindow,LogicalSize,PhysicalSize,PhysicalPosition} from "@tauri-apps/api/window";
import Database from "@tauri-apps/plugin-sql";
import {load} from "@tauri-apps/plugin-store";
import {open,save} from "@tauri-apps/plugin-dialog";
import {openPath} from "@tauri-apps/plugin-opener";
import {sendNotification,isPermissionGranted,requestPermission,onAction} from "@tauri-apps/plugin-notification";

let db=null,store=null,timer=null;

async function ensure(){
 if(!db) db=await Database.load("sqlite:mufakkirati.db");
 if(!store) store=await load("settings.json",{autoSave:100});
}
function reminderTasksFromState(state){
 const out=[];
 for(const group of Object.values(state?.notes||{})){
  for(const n of group||[]){
   if(n?.id&&n?.reminderAt&&!n?.reminderFired)out.push({id:String(n.id),at:String(n.reminderAt)});
  }
 }
 return out;
}
async function syncReminderTasks(state){
 try{return await invoke("sync_reminder_tasks",{tasks:reminderTasksFromState(state)})}
 catch(e){console.error("reminder-task-sync",e);return null}
}
window.__desktopSyncReminderTasks=syncReminderTasks;

let reminderSyncWanted=null;
let reminderSyncRunning=false;
let lastReminderTaskSignature=null;

function reminderTaskSignature(state){
 return JSON.stringify(
  reminderTasksFromState(state)
   .map(x=>[x.id,x.at])
   .sort((a,b)=>a[0].localeCompare(b[0]))
 );
}

function queueReminderTaskSync(state){
 const snap=structuredClone(state);
 const sig=reminderTaskSignature(snap);
 if(sig===lastReminderTaskSignature)return;
 reminderSyncWanted={snap,sig};
 if(reminderSyncRunning)return;

 reminderSyncRunning=true;
 (async()=>{
  try{
   while(reminderSyncWanted){
    const job=reminderSyncWanted;
    reminderSyncWanted=null;
    const result=await syncReminderTasks(job.snap);
    if(result!==null)lastReminderTaskSignature=job.sig;
   }
  }finally{
   reminderSyncRunning=false;
  }
 })();
}
let persistSerial=Promise.resolve();

function applyCanonicalReminders(source,target){
 if(!source?.notes||!target?.notes)return;
 const map=new Map();
 for(const group of Object.values(source.notes)){
  for(const n of group||[])if(n?.id)map.set(String(n.id),n);
 }
 for(const group of Object.values(target.notes)){
  for(const n of group||[]){
   const c=map.get(String(n?.id||""));
   if(!c)continue;
   n.reminderAt=c.reminderAt||"";
   n.reminderFired=!!c.reminderFired;
  }
 }
}

async function persistStateCore(state){
 await ensure();
 const canonical=await invoke("persist_state_atomic",{state});
 const saved=canonical&&typeof canonical==="object"?canonical:state;

 applyCanonicalReminders(saved,state);
 if(window.__notebookState){
  applyCanonicalReminders(saved,window.__notebookState);
  localStorage.setItem("jamal-notes-prototype-v2",JSON.stringify(window.__notebookState));
 }

 // The SQLite write above is authoritative; settings/statistics are secondary metadata.
 try{
  await store.set("theme",saved.theme||"blue");
  await store.set("notebookName",saved.notebookName||"");
  await store.set("profileImage",saved.profileImage||"");
  await store.set("viewLayout",saved.viewLayout||"columns");
  await store.set("viewSettings",saved.viewSettings||{});

  const cs=await db.select("SELECT COUNT(*) AS n FROM columns");
  const ns=await db.select("SELECT COUNT(*) AS n FROM notes");
  const ts=await db.select("SELECT COUNT(*) AS n FROM note_tags");
  await store.set("dbStats",{columns:Number(cs[0]?.n||0),notes:Number(ns[0]?.n||0),tags:Number(ts[0]?.n||0),checkedAt:new Date().toISOString()});
 }catch(e){console.error("persistMetadata",e)}

 queueReminderTaskSync(saved);
 return saved;
}

function persistState(state,useLive=true){
 const run=()=>{
  const payload=useLive&&window.__notebookState
   ?structuredClone(window.__notebookState)
   :structuredClone(state);
  return persistStateCore(payload);
 };
 const p=persistSerial.then(run,run);
 persistSerial=p.catch(()=>{});
 return p;
}
window.__desktopSave=state=>{
 clearTimeout(timer);
 timer=setTimeout(()=>persistState(structuredClone(window.__notebookState||state)).catch(e=>console.error("persistState",e)),180);
};
window.__desktopPersistNow=state=>{
 clearTimeout(timer);
 return persistState(structuredClone(state),false);
};

window.__desktopPickAttachment=async()=>{
 try{
  const p=await open({multiple:false,directory:false});
  if(!p)return null;

  const info=await invoke("attachment_info",{sourcePath:p});
  const mb=Number(info?.size_bytes||0)/(1024*1024);

  if(mb>100){
   const m=currentLanguage==="ar"
    ?"الملف أكبر من 100 ميغابايت ولا يمكن إرفاقه."
    :currentLanguage==="de"
    ?"Die Datei ist größer als 100 MB und kann nicht angehängt werden."
    :"The file is larger than 100 MB and cannot be attached.";
   window.showToast?.(m);
   return null;
  }

  if(mb>20){
   const m=currentLanguage==="ar"
    ?`حجم الملف ${mb.toFixed(1)} MB. هل تريد إرفاقه؟`
    :currentLanguage==="de"
    ?`Die Datei ist ${mb.toFixed(1)} MB groß. Trotzdem anhängen?`
    :`This file is ${mb.toFixed(1)} MB. Attach it anyway?`;
   if(!window.confirm(m))return null;
  }

  return await invoke("import_attachment",{sourcePath:p});
 }catch(e){
  console.error("attachment-picker",e);
  return null;
 }
};
window.__desktopDiscardAttachment=async path=>{if(!path)return;try{await invoke("discard_attachment",{path})}catch(e){console.error("attachment-discard",e)}};
window.__desktopOpenAttachment=async path=>{if(!path)return;try{await openPath(path)}catch(e){console.error("attachment-open",e)}};
window.__desktopAttachmentPreview=async path=>{
 if(!path)return null;
 try{return await invoke("attachment_preview",{path})}
 catch(e){console.error("attachment-preview",e);return null}
};
queueMicrotask(()=>window.__hydrateAttachmentPreviews?.());
window.__desktopCloneAttachment=async path=>{if(!path)return null;try{return await invoke("import_attachment",{sourcePath:path})}catch(e){console.error("attachment-clone",e);return null}};
window.__desktopCreateBackup=async()=>{
 try{
  await ensure();
  const state=structuredClone(window.__notebookState||{});
  await persistState(state);
  const d=new Date().toISOString().slice(0,10);
  let p=await save({
   defaultPath:`Mufakkirati-backup-${d}.zip`,
   filters:[{name:"Mufakkirati Backup",extensions:["zip"]}]
  });
  if(!p)return null;
  if(!p.toLowerCase().endsWith(".zip"))p+=".zip";
  const out=await invoke("create_backup",{destinationPath:p,state});
  window.showToast?.(`${tr(currentLanguage,"backup")} ✓`);
  return out;
 }catch(e){
  console.error("backup-create",e);
  window.showToast?.(`${tr(currentLanguage,"backup")} ✕`);
  return null;
 }
};

window.__desktopRestoreBackup=async()=>{
 try{
  await ensure();
  const p=await open({
   multiple:false,
   directory:false,
   filters:[{name:"Mufakkirati Backup",extensions:["zip"]}]
  });
  if(!p)return null;
  const currentState=structuredClone(window.__notebookState||{});
  await persistState(currentState);
  const res=await invoke("restore_backup",{sourcePath:p,currentState});
  if(!res?.state)throw new Error("backup contains no state");
  if(res.settings&&typeof res.settings==="object"&&!Array.isArray(res.settings)){
   for(const [k,v] of Object.entries(res.settings))await store.set(k,v);
  }
  await persistState(res.state,false);
  localStorage.setItem("jamal-notes-prototype-v2",JSON.stringify(res.state));
  sessionStorage.removeItem("db-hydrated-normalized");
  location.reload();
  return res;
 }catch(e){
  console.error("backup-restore",e);
  window.showToast?.(`${tr(currentLanguage,"restore")} ✕`);
  return null;
 }
};

window.__desktopHydrationReady=false;
function finishHydration(){
 window.__desktopHydrationReady=true;
 const board=document.getElementById("board");
 if(board){board.inert=false;board.removeAttribute("aria-busy")}
 window.renderBoard?.();
}
async function hydrate(){
 window.__desktopHydrationReady=false;
 try{
  await ensure();
  const persisted=await invoke("load_persisted_state");
  if(persisted&&typeof persisted==="object"){
   const h=structuredClone(persisted);
   h.theme=(await store.get("theme"))||h.theme||"blue";
   h.notebookName=(await store.get("notebookName"))||h.notebookName||"";
   h.profileImage=(await store.get("profileImage"))||h.profileImage||"";
   h.viewLayout=(await store.get("viewLayout"))||h.viewLayout||"columns";
   h.viewSettings=(await store.get("viewSettings"))||h.viewSettings||{};
   const j=JSON.stringify(h);
   const l=localStorage.getItem("jamal-notes-prototype-v2");
   if(j!==l){
    localStorage.setItem("jamal-notes-prototype-v2",j);
    location.reload();
    return;
   }
  }
  if(window.__notebookState)await persistState(window.__notebookState);
  finishHydration();
 }catch(e){
  console.error("hydrate",e);
  finishHydration();
 }
}

async function ensureReminderPermission(){
 try{
  let ok=await isPermissionGranted();
  if(!ok)ok=(await requestPermission())==="granted";
  return ok;
 }catch(e){console.error("notification-permission",e);return false}
}
window.__desktopEnableNotifications=ensureReminderPermission;

async function checkReminders(){
 const st=window.__notebookState;
 if(!st?.notes)return;
 const now=Date.now();

 for(const col of Object.values(st.notes)){
  for(const note of (col||[])){
   if(!note?.reminderAt||note.reminderFired)continue;

   const due=new Date(note.reminderAt).getTime();
   if(!Number.isFinite(due)||due>now)continue;
   if(!(await ensureReminderPermission()))continue;

   let owned=false;
   try{
    owned=!!(await invoke("claim_reminder",{id:String(note.id)}));
   }catch(e){
    console.error("reminder-claim",e);
    continue;
   }

   // Whether this process won or another headless process won,
   // reconcile the in-memory state so a later save cannot revive it.
   note.reminderFired=true;
   window.__desktopSave?.(structuredClone(st));
   window.renderBoard?.();

   if(!owned)continue;

   try{
    sendNotification({
     title:`\u23F0 ${note.title||"Reminder"}`,
     body:(note.text||"Reminder due").slice(0,220),
     extra:{noteId:note.id}
    });
   }catch(e){
    console.error("reminder-fire",e);
   }
  }
 }
}
window.__desktopCheckReminders=checkReminders;
setInterval(checkReminders,10000);
setTimeout(checkReminders,1800);
onAction(async e=>{
 if(!e?.notification?.extra?.noteId)return;
 try{await nativeWindow.show();await nativeWindow.unminimize();await nativeWindow.setFocus()}catch(err){console.error("reminder-focus",err)}
}).catch(e=>console.error("notification-action",e));
function nativeButton(id,action){
 const el=document.getElementById(id);
 if(!el)return;
 el.addEventListener("click",async e=>{
  e.preventDefault();
  e.stopImmediatePropagation();
  try{await invoke("window_action",{action})}catch(err){console.error(action,err)}
 },true);
}

nativeButton("hideBtn","hide");
nativeButton("minBtn","minimize");
nativeButton("fullBtn","fullscreen");
hydrate();
let currentLanguage="ar";
window.__t=k=>tr(currentLanguage,k);
function tx(sel,key){const e=document.querySelector(sel);if(e)e.textContent=tr(currentLanguage,key)}
function ph(sel,key){const e=document.querySelector(sel);if(e)e.placeholder=tr(currentLanguage,key)}
function txc(sel,key){const e=document.querySelector(sel);if(e?.firstElementChild)e.firstElementChild.textContent=tr(currentLanguage,key)}
function ttl(sel,key){const e=document.querySelector(sel);if(e){e.title=tr(currentLanguage,key);e.setAttribute("aria-label",tr(currentLanguage,key))}}
function themeTx(name,key){const e=document.querySelector(`.theme-choice[data-theme="${name}"]`);if(!e)return;const d=e.querySelector(".theme-dot");e.textContent="";if(d)e.appendChild(d);e.append(" "+tr(currentLanguage,key))}
function applyTranslations(){
 document.documentElement.lang=currentLanguage;
 document.documentElement.dir="rtl";
 document.body?.setAttribute("data-language",currentLanguage);
 tx("#languageSettingsLabel","lang");tx("#languageCurrent","language");tx("#backupSettingsLabel","backup");tx("#restoreBackupSettingsLabel","restore");tx("#updateSettingsLabel","updates");tx("#aboutSettingsLabel","about");
 tx("#profileSettingsBtn","profile");tx("#settingsBtn","settings");tx("#customizeBtn","customize");tx("#closeSettings","close");
 /* FULL_I18N_BINDINGS_A */
 tx("#viewLayoutTitle","layout");tx("#viewLayoutDesc","layoutDesc");
 tx("#layoutColumnsLabel","columnsLayout");tx("#layoutCompactLabel","compactLayout");tx("#layoutListLabel","listLayout");tx("#layoutAdaptiveLabel","adaptiveLayout");tx("#layoutTodayLabel","today");tx("#layoutTimelineLabel","timeline");tx("#closeViewLayout","close");
 tx("#viewCustomizeTitle","appearance");tx("#cardSizeTitle","cardSize");
 tx('[data-card-size-choice="small"]',"cardSmall");tx('[data-card-size-choice="medium"]',"cardMedium");tx('[data-card-size-choice="large"]',"cardLarge");
 tx("#spacingTitle","spacing");tx('[data-spacing-choice="tight"]',"spacingTight");tx('[data-spacing-choice="normal"]',"spacingNormal");tx('[data-spacing-choice="airy"]',"spacingAiry");
 tx("#detailsTitle","details");txc('[data-toggle-view="showDate"]',"showDate");txc('[data-toggle-view="showRating"]',"showRating");txc('[data-toggle-view="showChips"]',"showChips");txc('[data-toggle-view="showIcons"]',"showIcons");
 tx("#resetViewSettings","reset");tx("#closeViewCustomize","close");
 tx("#profileModalTitle","profile");tx("#profileDesc","profileDesc");tx("#notebookNameLabel","notebook");tx("#profileImageLabel","image");
 tx("#settingsTitle","settings");tx("#manageNotesLabel","notes");tx("#advancedSettingsLabel","advancedSettings");tx("#comingSoonLabel","comingSoon");
 /* FULL_I18N_BINDINGS_B */
 tx("#themeTitle","theme");tx("#themeDesc","themeDesc");
 themeTx("auto","themeAuto");themeTx("black","themeBlack");themeTx("white","themeWhite");themeTx("yellow","themeYellow");themeTx("red","themeRed");
 themeTx("pink","themePink");themeTx("purple","themePurple");themeTx("green","themeGreen");themeTx("orange","themeOrange");themeTx("blue","themeBlue");tx("#closeTheme","close");
 tx("#manageColumnsTitle","columns");tx("#colLabel1","firstSection");tx("#colLabel2","secondSection");tx("#colLabel3","thirdSection");
 tx("#manageColumnsTip","manageColumnsTip");tx("#saveColumns","saveColumns");tx("#cancelManage","cancel");
 ttl("#layoutBtn","layout");ttl("#filterBtn","filters");ttl("#smartSortBtn",window.__notebookState?.sortMode==="smart"?"smartSortOn":"smartSortOff");ttl("#quickCaptureBtn","quickCapture");
 tx("#quickCaptureTitle","quickCapture");tx("#quickCaptureTitleLabel","quickCaptureTitleLabel");tx("#quickCaptureBodyLabel","quickCaptureBodyLabel");tx("#quickCaptureColumnLabel","section");tx("#quickCaptureSave","save");tx("#quickCaptureCancel","cancel");
 ph("#quickCaptureName","quickCaptureTitlePlaceholder");ph("#quickCaptureBody","quickCaptureBodyPlaceholder");
 tx("#saveNote","save");tx("#cancelNote","cancel");tx("#clearReminderBtn","clear");tx("#saveProfile","saveProfile");tx("#removeProfileImage","removePhoto");tx("#cancelProfile","cancel");
 ph("#searchInput","search");ph("#noteTitle","title");ph("#noteText","content");ph("#notebookNameInput","notebook");
 const add=document.querySelector("#addBtn");if(add)add.innerHTML='<span style="font-size:24px">+</span> '+tr(currentLanguage,"add");
 document.querySelectorAll(".priority-badge").forEach(e=>e.textContent=tr(currentLanguage,e.dataset.priority||"weak"));
 document.querySelectorAll("input,textarea").forEach(e=>e.style.direction="auto");
}
async function applyLanguage(code){currentLanguage=I18N[code]?code:"ar";applyTranslations();await ensure();await store.set("language",currentLanguage)}
window.__applyLanguage=applyLanguage;
function translateNoteModal(){
 tx("#noteModalTitle",document.querySelector("#deleteNote")?.style.display==="none"?"add":"editNote");
 tx("#lblNoteTitle","title");
 tx("#lblNoteText","content");
 tx("#lblNoteNextStep","nextStep");tx("#lblNoteLinks","noteLinks");
 ph("#noteNextStep","nextStepPlaceholder");ph("#noteLinkSearch","linkSearchPlaceholder");
 tx("#lblNoteColumn","section");
 const r=document.querySelector("#lblNoteReminder");if(r)r.textContent="\u23F0 "+tr(currentLanguage,"reminder");
 tx("#lblNoteAttachments","attachments");
 tx("#lblNoteEmoji","emoji");tx("#lblNoteRating","rating");tx("#lblNoteTags","tags");tx("#deleteNote","deleteNote");
 const at=document.getElementById("addAttachmentBtn");if(at)at.textContent="📎 "+tr(currentLanguage,"attach");
 document.querySelectorAll(".note-copy").forEach(b=>{b.title=tr(currentLanguage,"copy");b.setAttribute("aria-label",tr(currentLanguage,"copy"))});document.querySelectorAll(".note-delete").forEach(b=>{b.title=tr(currentLanguage,"delete");b.setAttribute("aria-label",tr(currentLanguage,"delete"))});
 document.querySelectorAll(".note-delete-confirm-text").forEach(e=>e.textContent=tr(currentLanguage,"confirmDeleteNote"));
 document.querySelectorAll(".note-delete-confirm-action").forEach(e=>e.textContent=tr(currentLanguage,"delete"));
 document.querySelectorAll(".note-delete-cancel").forEach(e=>e.textContent=tr(currentLanguage,"cancel"));
 document.getElementById("orbQuickCapture")?.setAttribute("title",tr(currentLanguage,"quickCapture"));
}function openLanguagePicker(){
 let m=document.getElementById("languagePickerModal");
 if(!m){
  m=document.createElement("div");m.id="languagePickerModal";m.className="modal-backdrop";
  m.innerHTML='<div class="modal" style="max-width:390px"><h3 id="languagePickerTitle"></h3><div class="settings-list"><button class="settings-row" data-lang-choice="ar"><strong>العربية</strong><span>AR</span></button><button class="settings-row" data-lang-choice="en"><strong>English</strong><span>EN</span></button><button class="settings-row" data-lang-choice="de"><strong>Deutsch</strong><span>DE</span></button></div><div class="modal-actions"><button class="secondary" id="closeLanguagePicker"></button></div></div>';
  document.body.appendChild(m);
  m.querySelectorAll("[data-lang-choice]").forEach(b=>b.onclick=async()=>{await applyLanguage(b.dataset.langChoice);translateNoteModal();m.classList.remove("open")});
  m.querySelector("#closeLanguagePicker").onclick=()=>m.classList.remove("open");
  m.onclick=e=>{if(e.target===m)m.classList.remove("open")};
 }
 m.querySelector("#languagePickerTitle").textContent=tr(currentLanguage,"lang");
 m.querySelector("#closeLanguagePicker").textContent=tr(currentLanguage,"close");
 m.classList.add("open");
}async function initLanguage(){await ensure();await applyLanguage((await store.get("language"))||"ar");const b=document.getElementById("languageSettingsBtn");if(b)b.onclick=openLanguagePicker;translateNoteModal();const a=document.getElementById("aboutSettingsBtn");if(a)a.onclick=async()=>alert(`Mufakkirati\nVersion ${await invoke("app_version")}\nTauri Desktop`)}
initLanguage();
/* MUFAKKIRATI_NATIVE_RESIZE */
const nativeWindow=getCurrentWindow();
let normalWindowBounds=null;

async function enterFloatingMode(){
 await ensure();
 const wasMaximized=await nativeWindow.isMaximized();
 if(wasMaximized)await nativeWindow.toggleMaximize();
 const pos=await nativeWindow.outerPosition(),size=await nativeWindow.innerSize();
 normalWindowBounds={x:pos.x,y:pos.y,width:size.width,height:size.height,wasMaximized};
 await store.set("normalWindowBounds",normalWindowBounds);
 await store.set("floatingMode",true);
 await nativeWindow.setMinSize(null);
 await nativeWindow.setMaxSize(null);
 await nativeWindow.setResizable(false);
 await nativeWindow.setAlwaysOnTop(true);
 document.body.classList.add("floating-orb");
 await nativeWindow.setSize(new LogicalSize(96,96));
 await nativeWindow.setPosition(new PhysicalPosition(pos.x+Math.max(0,size.width-112),pos.y+18));
}

document.querySelectorAll(".native-resize-handle").forEach(h=>h.addEventListener("pointerdown",async e=>{
 if(e.button!==0||document.body.classList.contains("card-drag-active"))return;e.preventDefault();e.stopPropagation();
 try{await nativeWindow.startResizeDragging(h.dataset.resizeDirection)}catch(err){console.error("resize",err)}
}));
const windowDragZone=document.getElementById("windowDragZone");
if(windowDragZone){
 windowDragZone.addEventListener("pointerdown",async e=>{
  if(e.button!==0||e.detail>1)return;
  e.preventDefault();
  try{await nativeWindow.startDragging()}catch(err){console.error("drag-zone",err)}
 });
 windowDragZone.addEventListener("dblclick",async e=>{
  e.preventDefault();
  e.stopPropagation();
  try{await nativeWindow.toggleMaximize()}catch(err){console.error("drag-zone-maximize",err)}
 });
}
async function restoreFloatingMode(){
 await ensure();
 const b=normalWindowBounds||await store.get("normalWindowBounds");
 document.body.classList.remove("floating-orb");
 await nativeWindow.setAlwaysOnTop(false);
 await nativeWindow.setResizable(true);
 await nativeWindow.setMaxSize(null);
 await nativeWindow.setMinSize(new LogicalSize(900,600));
 if(b?.width&&b?.height){
  await nativeWindow.setSize(new PhysicalSize(b.width,b.height));
  await nativeWindow.setPosition(new PhysicalPosition(b.x,b.y));
  if(b.wasMaximized&&!await nativeWindow.isMaximized())await nativeWindow.toggleMaximize();
 }
 await store.set("floatingMode",false);
}

window.__desktopEnterFloatingMode=enterFloatingMode;
window.__desktopRestoreFloatingMode=restoreFloatingMode;

document.getElementById("floatBtn")?.addEventListener("click",e=>{
 e.stopPropagation();enterFloatingMode().catch(console.error);
});
document.getElementById("orbRestore")?.addEventListener("click",e=>{
 e.stopPropagation();restoreFloatingMode().catch(console.error);
});
document.getElementById("orbDragHandle")?.addEventListener("pointerdown",async e=>{
 if(e.button!==0)return;e.preventDefault();e.stopPropagation();
 try{await nativeWindow.startDragging()}catch(err){console.error("orb-drag",err)}
});
ensure().then(async()=>{
 if(await store.get("floatingMode"))await restoreFloatingMode();
}).catch(console.error);
async function checkForUpdates(checker=check){
 const b=document.getElementById("updateSettingsBtn");
 const l=document.getElementById("updateSettingsLabel");
 if(!b)return null;
 b.disabled=true;if(l)l.textContent=tr(currentLanguage,"checkingUpdates");
 let u=null;
 try{
  u=await checker({timeout:15000});
  if(!u){window.showToast?.(tr(currentLanguage,"noUpdates"));return null}
  const ok=window.confirm(`${tr(currentLanguage,"updateAvailable")} ${u.version}. ${tr(currentLanguage,"installUpdate")}`);
  if(!ok){await u.close();return u.version}
  let total=0,done=0;
  await u.downloadAndInstall(e=>{
   if(e.event==="Started")total=Number(e.data?.contentLength||0);
   if(e.event==="Progress"){done+=Number(e.data?.chunkLength||0);if(l)l.textContent=total?`${tr(currentLanguage,"downloadingUpdate")} ${Math.min(100,Math.round(done*100/total))}%`:tr(currentLanguage,"downloadingUpdate")}
   if(e.event==="Finished"&&l)l.textContent=tr(currentLanguage,"installingUpdate");
  });
  return u.version;
 }catch(e){
  console.error("updater",e);
  window.showToast?.(tr(currentLanguage,"updateCheckFailed"));
  try{await u?.close?.()}catch{}
  return null;
 }finally{
  b.disabled=false;
  if(l)l.textContent=tr(currentLanguage,"updates");
 }
}
window.__desktopCheckForUpdates=checkForUpdates;
document.getElementById("updateSettingsBtn")?.addEventListener("click",()=>checkForUpdates());
document.getElementById("backupSettingsBtn")?.addEventListener("click",async()=>{
 await window.__desktopCreateBackup?.();
});
document.getElementById("restoreBackupSettingsBtn")?.addEventListener("click",async()=>{
 await window.__desktopRestoreBackup?.();
});
/* REMINDER_OPEN_UI_ROUTE */
async function openReminderNoteInUi(rawId){
 const id=String(rawId||"").trim();
 if(!id)return false;
 for(let i=0;i<100;i++){
  try{
   if(window.__desktopHydrationReady===true&&window.__openNoteById?.(id))return true;
  }catch(e){console.error("reminder-open-note",e)}
  await new Promise(r=>setTimeout(r,100));
 }
 console.warn("reminder-open-note-not-found",id);
 return false;
}

async function finishReminderOpen(rawId){
 const id=String(rawId||"").trim();
 if(!id)return false;
 const opened=await openReminderNoteInUi(id);
 if(opened){
  try{await invoke("ack_pending_open_note",{id})}
  catch(err){console.error("reminder-open-ack",err)}
 }
 return opened;
}

listen("mufakkirati-open-note",e=>{
 finishReminderOpen(e.payload).catch(err=>console.error("reminder-open-event",err));
}).catch(err=>console.error("reminder-open-listen",err));

invoke("peek_pending_open_note")
 .then(id=>{if(id)return finishReminderOpen(id)})
 .catch(err=>console.error("reminder-open-pending",err));
