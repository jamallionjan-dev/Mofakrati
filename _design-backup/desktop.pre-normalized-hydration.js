import {invoke} from "@tauri-apps/api/core";
import {getCurrentWindow} from "@tauri-apps/api/window";
import Database from "@tauri-apps/plugin-sql";
import {load} from "@tauri-apps/plugin-store";

let db=null,store=null,timer=null;

async function ensure(){
 if(!db) db=await Database.load("sqlite:mufakkirati.db");
 if(!store) store=await load("settings.json",{autoSave:100});
}
async function persistState(state){
 try{
  await ensure();
  await invoke("persist_state_atomic",{state});
  await store.set("theme",state.theme||"blue");
  await store.set("notebookName",state.notebookName||"");
  await store.set("profileImage",state.profileImage||"");
  await store.set("viewLayout",state.viewLayout||"columns");
  await store.set("viewSettings",state.viewSettings||{});
  const cs=await db.select("SELECT COUNT(*) AS n FROM columns");
  const ns=await db.select("SELECT COUNT(*) AS n FROM notes");
  const ts=await db.select("SELECT COUNT(*) AS n FROM note_tags");
  await store.set("dbStats",{columns:Number(cs[0]?.n||0),notes:Number(ns[0]?.n||0),tags:Number(ts[0]?.n||0),checkedAt:new Date().toISOString()});
 }catch(e){console.error("persistState",e)}
}

window.__desktopSave=state=>{
 clearTimeout(timer);
 timer=setTimeout(()=>persistState(state),180);
};

async function hydrate(){
 try{
  await ensure();
  const rows=await db.select("SELECT value FROM app_meta WHERE key=$1 LIMIT 1",["state"]);
  const local=localStorage.getItem("jamal-notes-prototype-v2");
  if(rows.length&&rows[0].value&&rows[0].value!==local&&!sessionStorage.getItem("db-hydrated")){
   sessionStorage.setItem("db-hydrated","1");
   localStorage.setItem("jamal-notes-prototype-v2",rows[0].value);
   location.reload();
   return;
  }
  if(window.__notebookState) await persistState(window.__notebookState);
 }catch(e){console.error("hydrate",e)}
}

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
const I18N={
 ar:{lang:"العربية",language:"اللغة",updates:"التحقق من التحديثات",about:"حول مفكرتي",dir:"rtl"},
 en:{lang:"English",language:"Language",updates:"Check for updates",about:"About Mufakkirati",dir:"ltr"},
 de:{lang:"Deutsch",language:"Sprache",updates:"Nach Updates suchen",about:"Über Mufakkirati",dir:"ltr"}
};

async function applyLanguage(code){
 const t=I18N[code]||I18N.ar;
 document.documentElement.lang=code;
 document.documentElement.dir=t.dir;
 document.getElementById("languageSettingsLabel")?.replaceChildren(t.language);
 document.getElementById("languageCurrent")?.replaceChildren(t.lang);
 document.getElementById("updateSettingsLabel")?.replaceChildren(t.updates);
 document.getElementById("aboutSettingsLabel")?.replaceChildren(t.about);
 await ensure();
 await store.set("language",code);
}

async function initLanguage(){
 await ensure();
 const code=(await store.get("language"))||"ar";
 await applyLanguage(code);
 const b=document.getElementById("languageSettingsBtn");
 if(b)b.onclick=async()=>{
  const cur=document.documentElement.lang||"ar";
  const next=cur==="ar"?"en":cur==="en"?"de":"ar";
  await applyLanguage(next);
 };
 const a=document.getElementById("aboutSettingsBtn");
 if(a)a.onclick=async()=>{
  const v=await invoke("app_version");
  alert(`Mufakkirati\nVersion ${v}\nTauri Desktop`);
 };
 const up=document.getElementById("updateSettingsBtn");
 if(up)up.onclick=()=>alert("Updater channel is not configured yet.");
}
initLanguage();
/* MUFAKKIRATI_NATIVE_RESIZE */
const nativeWindow=getCurrentWindow();
document.querySelectorAll(".native-resize-handle").forEach(h=>h.addEventListener("pointerdown",async e=>{
 if(e.button!==0)return;e.preventDefault();e.stopPropagation();
 try{await nativeWindow.startResizeDragging(h.dataset.resizeDirection)}catch(err){console.error("resize",err)}
}));
const dragTopbar=document.querySelector(".topbar");
if(dragTopbar)dragTopbar.addEventListener("pointerdown",async e=>{
 if(e.button!==0||e.target.closest("button,input,textarea,select,a,.profile,.search-wrap,.window-controls,[contenteditable='true']"))return;
 e.preventDefault();try{await nativeWindow.startDragging()}catch(err){console.error("drag",err)}
});

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