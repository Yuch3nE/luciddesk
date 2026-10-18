//! Independent read-only Explorer view probe for desktop CLI integration tests.
fn main() -> Result<(), String> {
    let _sta = luciddesk_shell::ShellApartment::initialize_sta().map_err(|e|e.to_string())?;
    let snapshot = luciddesk_shell::native_desktop_snapshot()?;
    if !snapshot.is_complete() {return Err("Explorer snapshot is incomplete".into());}
    let items:Vec<_>=snapshot.items.iter().map(|(entry,x,y)|serde_json::json!({
        "key":entry.identity.persistent_key(),"path":entry.identity.file_system_path(),"x":x,"y":y
    })).collect();
    println!("{}",serde_json::json!({"items":items,"icons_hidden":luciddesk_shell::desktop_icons_hidden()}));
    Ok(())
}
