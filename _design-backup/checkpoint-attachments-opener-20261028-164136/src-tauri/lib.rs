mod attachment;
mod persistence;
use tauri::{Manager,menu::{Menu,MenuItem},tray::{TrayIconBuilder,TrayIconEvent,MouseButton,MouseButtonState}};
use tauri_plugin_sql::{Migration,MigrationKind};

#[tauri::command]
fn app_version(a:tauri::AppHandle)->String{a.package_info().version.to_string()}

#[tauri::command]
fn hide_main_window(a:tauri::AppHandle)->Result<(),String>{
 a.get_webview_window("main").ok_or("missing")?.hide().map_err(|e|e.to_string())
}

#[tauri::command]
fn window_action(a:tauri::AppHandle,action:String)->Result<(),String>{
 let w=a.get_webview_window("main").ok_or("missing")?;
 match action.as_str(){
  "hide"=>w.hide().map_err(|e|e.to_string()),
  "minimize"=>w.minimize().map_err(|e|e.to_string()),
  "fullscreen"=>{let f=w.is_fullscreen().map_err(|e|e.to_string())?;w.set_fullscreen(!f).map_err(|e|e.to_string())},
  _=>Ok(())
 }
}
#[cfg_attr(mobile,tauri::mobile_entry_point)]
pub fn run(){
 let m=vec![Migration{version:1,description:"init",sql:include_str!("../migrations/001_init.sql"),kind:MigrationKind::Up},Migration{version:2,description:"note_fields",sql:include_str!("../migrations/002_note_fields.sql"),kind:MigrationKind::Up}];

 tauri::Builder::default()
 .plugin(tauri_plugin_single_instance::init(|a,_,_|{
  if let Some(w)=a.get_webview_window("main"){let _=w.show();let _=w.unminimize();let _=w.set_focus();}
 }))
 .plugin(tauri_plugin_store::Builder::default().build())
 .plugin(tauri_plugin_process::init())
.plugin(tauri_plugin_dialog::init())
.plugin(tauri_plugin_opener::init())
 .plugin(tauri_plugin_window_state::Builder::default().build())
 .plugin(tauri_plugin_sql::Builder::default().add_migrations("sqlite:mufakkirati.db",m).build())
 .invoke_handler(tauri::generate_handler![app_version,hide_main_window,window_action,persistence::persist_state_atomic,attachment::import_attachment,attachment::discard_attachment])
 .setup(|a|{
  let show=MenuItem::with_id(a,"show","Show",true,None::<&str>)?;
  let quit=MenuItem::with_id(a,"quit","Quit",true,None::<&str>)?;
  let menu=Menu::with_items(a,&[&show,&quit])?;
  TrayIconBuilder::new().menu(&menu).show_menu_on_left_click(false)
   .on_menu_event(|a,e|match e.id.as_ref(){
    "show"=>if let Some(w)=a.get_webview_window("main"){let _=w.show();let _=w.unminimize();let _=w.set_focus();},
    "quit"=>a.exit(0),_=>{}
   })
   .on_tray_icon_event(|t,e|if let TrayIconEvent::Click{button:MouseButton::Left,button_state:MouseButtonState::Up,..}=e{
    if let Some(w)=t.app_handle().get_webview_window("main"){let _=w.show();let _=w.unminimize();let _=w.set_focus();}
   }).build(a)?;
  Ok(())
 })
 .on_window_event(|w,e|if let tauri::WindowEvent::CloseRequested{api,..}=e{if w.label()=="main"{api.prevent_close();let _=w.hide();}})
 .run(tauri::generate_context!()).expect("run failed");
}