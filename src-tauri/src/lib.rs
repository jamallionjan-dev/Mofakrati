mod attachment;
mod backup;
mod persistence;
mod reminder_scheduler;

pub fn run_headless_reminder_from_args()->Option<i32>{
    reminder_scheduler::run_headless_from_args()
}
use tauri::{
    Emitter,
    Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_sql::{Migration, MigrationKind};
use tauri_plugin_deep_link::DeepLinkExt;
struct PendingOpenNote(std::sync::Mutex<Option<String>>);

#[tauri::command]
fn peek_pending_open_note(p:tauri::State<PendingOpenNote>)->Option<String>{
    p.0.lock().ok()?.clone()
}

#[tauri::command]
fn ack_pending_open_note(id:String,p:tauri::State<PendingOpenNote>)->bool{
    let Ok(mut slot)=p.0.lock() else{return false};
    if slot.as_deref()==Some(id.as_str()){
        *slot=None;
        true
    }else{
        false
    }
}

fn route_reminder_open_url(a:&tauri::AppHandle,raw:&str){
    const P:&str="mufakkirati://reminder/open/";
    let Some(rest)=raw.strip_prefix(P) else{return};
    let id=rest.split(['?','#']).next().unwrap_or("");
    if id.is_empty(){return}
    if let Some(p)=a.try_state::<PendingOpenNote>(){
        if let Ok(mut slot)=p.0.lock(){*slot=Some(id.to_string());}
    }
    let _=a.emit("mufakkirati-open-note",id.to_string());
    if let Some(w)=a.get_webview_window("main"){
        let _=w.show();
        let _=w.unminimize();
        let _=w.set_focus();
    }
}
#[tauri::command]
fn app_version(a: tauri::AppHandle) -> String {
    a.package_info().version.to_string()
}

#[tauri::command]
fn hide_main_window(a: tauri::AppHandle) -> Result<(), String> {
    a.get_webview_window("main")
        .ok_or("missing")?
        .hide()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn window_action(a: tauri::AppHandle, action: String) -> Result<(), String> {
    let w = a.get_webview_window("main").ok_or("missing")?;
    match action.as_str() {
        "hide" => w.hide().map_err(|e| e.to_string()),
        "minimize" => w.minimize().map_err(|e| e.to_string()),
        "fullscreen" => {
            let f = w.is_fullscreen().map_err(|e| e.to_string())?;
            w.set_fullscreen(!f).map_err(|e| e.to_string())
        }
        _ => Ok(()),
    }
}

#[tauri::command]
fn copy_text_to_clipboard(text: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::{ffi::c_void, mem::size_of, ptr::copy_nonoverlapping, thread, time::Duration};
        use windows::Win32::{
            Foundation::HANDLE,
            System::{
                DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
                Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
            },
        };

        let mut last_error = String::from("Windows clipboard is busy");
        for _ in 0..5 {
            unsafe {
                let opened = OpenClipboard(None);
                if opened.is_ok() {
                    let result = (|| -> Result<(), String> {
                        EmptyClipboard().map_err(|e| e.to_string())?;
                        let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                        let bytes = utf16.len() * size_of::<u16>();
                        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes).map_err(|e| e.to_string())?;
                        let destination = GlobalLock(memory);
                        if destination.is_null() {
                            return Err("Could not allocate clipboard memory".into());
                        }
                        copy_nonoverlapping(utf16.as_ptr(), destination.cast::<u16>(), utf16.len());
                        let _ = GlobalUnlock(memory);
                        SetClipboardData(13, Some(HANDLE(memory.0 as *mut c_void))).map_err(|e| e.to_string())?;
                        Ok(())
                    })();
                    let _ = CloseClipboard();
                    return result;
                }
                last_error = opened.err().map(|e| e.to_string()).unwrap_or(last_error);
            }
            thread::sleep(Duration::from_millis(30));
        }
        return Err(last_error);
    }
    #[cfg(not(windows))]
    {
        let _ = text;
        Err("System clipboard copy is not configured for this platform".into())
    }
}

#[cfg(all(test, windows))]
mod clipboard_tests {
    use super::copy_text_to_clipboard;
    use windows::Win32::System::{
        DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard},
    };
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    #[test]
    fn copies_arabic_multiline_note_text_to_windows_clipboard() {
        let expected = "خطة الدراسة\n\nمراجعة الفيزياء والكيمياء\n\nالخطوة التالية: حل 20 سؤالًا";
        copy_text_to_clipboard(expected.to_string()).expect("clipboard write should succeed");
        unsafe {
            OpenClipboard(None).expect("clipboard should reopen");
            let result = (|| {
                let data = GetClipboardData(13).expect("CF_UNICODETEXT should exist");
                let memory = HGLOBAL(data.0);
                let ptr = GlobalLock(memory);
                assert!(!ptr.is_null());
                let mut length = 0;
                while *ptr.cast::<u16>().add(length) != 0 {
                    length += 1;
                }
                let actual = String::from_utf16(std::slice::from_raw_parts(ptr.cast::<u16>(), length))
                    .expect("clipboard should contain valid UTF-16");
                let _ = GlobalUnlock(memory);
                assert_eq!(actual, expected);
            })();
            let _ = CloseClipboard();
            result;
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let m = vec![
        Migration {
            version: 1,
            description: "init",
            sql: include_str!("../migrations/001_init.sql"),
            kind: MigrationKind::Up,
        },
        Migration {
            version: 2,
            description: "note_fields",
            sql: include_str!("../migrations/002_note_fields.sql"),
            kind: MigrationKind::Up,
        },
        Migration {
            version: 3,
            description: "note_priority",
            sql: include_str!("../migrations/003_note_priority.sql"),
            kind: MigrationKind::Up,
        },
        Migration {
            version: 4,
            description: "note_reminder",
            sql: include_str!("../migrations/004_note_reminder.sql"),
            kind: MigrationKind::Up,
        },
    ];

    let reminder_boot = std::env::args()
        .any(|x| x == "--reminder-id" || x.starts_with("--reminder-id="));

    let mut window_state = tauri_plugin_window_state::Builder::default()
        .with_state_flags(
            tauri_plugin_window_state::StateFlags::SIZE
                | tauri_plugin_window_state::StateFlags::POSITION
                | tauri_plugin_window_state::StateFlags::MAXIMIZED
                | tauri_plugin_window_state::StateFlags::FULLSCREEN,
        );
    if reminder_boot {
        window_state = window_state.skip_initial_state("main");
    }

    let app = tauri::Builder::default()
        .manage(PendingOpenNote(std::sync::Mutex::new(None)))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_single_instance::init(|a, args, _| {
            let reminder_launch = args.iter()
                .any(|x| x == "--reminder-id" || x.starts_with("--reminder-id="));
            if !reminder_launch {
                if let Some(w) = a.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }
        }))
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(window_state.build())
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:mufakkirati.db", m)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            app_version,
            peek_pending_open_note,
            ack_pending_open_note,
            hide_main_window,
            window_action,
            copy_text_to_clipboard,
            persistence::load_persisted_state,
            persistence::persist_state_atomic,
            reminder_scheduler::sync_reminder_tasks,
            reminder_scheduler::claim_reminder,
            attachment::attachment_info,
            attachment::attachment_preview,
            attachment::import_attachment,
            attachment::discard_attachment,
            backup::create_backup,
            backup::restore_backup
        ])
                .setup(|a| {
            // REMINDER_OPEN_DEEPLINK_ROUTE
            a.deep_link().register_all()?;
            let open_handle=a.handle().clone();
            if let Ok(Some(urls))=a.deep_link().get_current(){
                for u in urls{
                    route_reminder_open_url(&open_handle,&u.to_string());
                }
            }
            let live_handle=open_handle.clone();
            a.deep_link().on_open_url(move |ev|{
                for u in ev.urls(){
                    route_reminder_open_url(&live_handle,&u.to_string());
                }
            });
            let show = MenuItem::with_id(a, "show", "Show", true, None::<&str>)?;
            let quit = MenuItem::with_id(a, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(a, &[&show, &quit])?;
            TrayIconBuilder::new()
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|a, e| match e.id.as_ref() {
                    "show" => {
                        if let Some(w) = a.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => a.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|t, e| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = e
                    {
                        if let Some(w) = t.app_handle().get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                })
                .build(a)?;
            Ok(())
        })
        .on_window_event(|w, e| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = e {
                if w.label() == "main" {
                    api.prevent_close();
                    let _ = w.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("build failed");

    app.run(move |app_handle, event| {
        if let tauri::RunEvent::Ready = event {
            if let Some(w) = app_handle.get_webview_window("main") {
                if reminder_boot {
                    let _ = w.hide();
                } else {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }
        }
    });
}
