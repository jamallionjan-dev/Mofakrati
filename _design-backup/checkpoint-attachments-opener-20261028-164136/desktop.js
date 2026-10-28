import {invoke} from "@tauri-apps/api/core";
import {getCurrentWindow} from "@tauri-apps/api/window";
import Database from "@tauri-apps/plugin-sql";
import {load} from "@tauri-apps/plugin-store";
import {open} from "@tauri-apps/plugin-dialog";
import {openPath} from "@tauri-apps/plugin-opener";

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

window.__desktopPickAttachment=async()=>{try{const p=await open({multiple:false,directory:false});if(!p)return null;return await invoke("import_attachment",{sourcePath:p})}catch(e){console.error("attachment-picker",e);return null}};
window.__desktopDiscardAttachment=async path=>{if(!path)return;try{await invoke("discard_attachment",{path})}catch(e){console.error("attachment-discard",e)}};
window.__desktopOpenAttachment=async path=>{if(!path)return;try{await openPath(path)}catch(e){console.error("attachment-open",e)}};

async function hydrate(){try{await ensure();const c=await db.select("SELECT id,title,position FROM columns ORDER BY position,id");if(c.length){const n=await db.select("SELECT id,column_id,title,body,rating,position,emoji,note_date FROM notes ORDER BY column_id,position,id"),t=await db.select("SELECT note_id,tag FROM note_tags ORDER BY note_id,tag"),a=await db.select("SELECT id,note_id,kind,path,display_name FROM attachments ORDER BY created_at,id"),m=await db.select("SELECT value FROM app_meta WHERE key=$1 LIMIT 1",["state"]);let r={};try{if(m[0]?.value)r=JSON.parse(m[0].value)||{}}catch{}const oc=new Map((r.columns||[]).map(x=>[x.id,x])),tm=new Map;for(const x of t){if(!tm.has(x.note_id))tm.set(x.note_id,[]);tm.get(x.note_id).push(x.tag)}const h={...r,theme:(await store.get("theme"))||r.theme||"blue",notebookName:(await store.get("notebookName"))||r.notebookName||"",profileImage:(await store.get("profileImage"))||r.profileImage||"",viewLayout:(await store.get("viewLayout"))||r.viewLayout||"columns",viewSettings:(await store.get("viewSettings"))||r.viewSettings||{},columns:c.map(x=>{const o=oc.get(x.id)||{};return{id:x.id,label:x.title||"",icon:o.icon||"",className:o.className||x.id}}),notes:{}};for(const x of h.columns)h.notes[x.id]=n.filter(q=>q.column_id===x.id).map(q=>({id:q.id,emoji:q.emoji||"",title:q.title||"",text:q.body||"",date:q.note_date||"",rating:Number(q.rating||0),chips:tm.get(q.id)||[],attachments:a.filter(z=>z.note_id===q.id).map(z=>({id:z.id,kind:z.kind,path:z.path,displayName:z.display_name}))}));const j=JSON.stringify(h),l=localStorage.getItem("jamal-notes-prototype-v2");if(j!==l&&!sessionStorage.getItem("db-hydrated-normalized")){sessionStorage.setItem("db-hydrated-normalized","1");localStorage.setItem("jamal-notes-prototype-v2",j);location.reload();return}}if(window.__notebookState)await persistState(window.__notebookState)}catch(e){console.error("hydrate",e)}}

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