//! Visible-only GPU cache and a lazy bounded Shell worker.
use super::*;
use std::collections::{HashMap, HashSet};
use crate::pane::{assets::Pixels, wake::Wake};
const CAPACITY: usize = VISIBLE;
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key { extension: String, folder: bool, size: i32, path: Option<std::path::PathBuf> }
impl Key {
    fn new(entry: &Entry, size: i32) -> Self {
        let extension = entry.path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let specific = entry.folder || matches!(extension.as_str(), "exe" | "lnk" | "url");
        Self { path: specific.then(||entry.path.clone()),
            extension: if entry.folder {String::new()} else {extension},
            folder: entry.folder, size: if size<=16 {16} else {32} }
    }
}
struct Worker {
    wake: Wake,
    stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
    sender: mpsc::SyncSender<Key>,
    receiver: mpsc::Receiver<(Key, Option<Pixels>)>,
}
impl Worker {
    fn new(hwnd: HWND) -> Self {
        let wake=Wake::default();
        let ready=wake.clone();
        let stopped=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancelled=stopped.clone();
        let (sender, requests)=mpsc::sync_channel::<Key>(CAPACITY);
        let (responses, receiver)=mpsc::sync_channel(CAPACITY);
        std::thread::spawn(move || {
            use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
            let initialized=unsafe {CoInitializeEx(None, COINIT_MULTITHREADED)}.is_ok();
            while let Ok(key)=requests.recv() {
                if cancelled.load(std::sync::atomic::Ordering::Acquire) {break;}
                let pixels=if initialized {load(&key).ok()} else {None};
                if cancelled.load(std::sync::atomic::Ordering::Acquire) {break;}
                if responses.send((key,pixels)).is_err() {break;}
                ready.notify();
            }
            if initialized {unsafe {CoUninitialize();}}
        });
        wake.bind(hwnd as isize);
        Self {wake,stopped,sender,receiver}
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stopped.store(true,std::sync::atomic::Ordering::Release);
        self.wake.unbind();
    }
}
pub(super) struct Icons {
    worker: Worker,
    cache: HashMap<Key, Option<Pixels>>,
    pending: HashSet<Key>,
}
impl Icons {
    pub fn new(hwnd: HWND) -> Self {
        Self {worker:Worker::new(hwnd),cache:HashMap::new(),pending:HashSet::new()}
    }
    pub fn refresh(&mut self, entries: &[Entry], size: i32) {
        let worker=&self.worker;
        worker.wake.received();
        let visible: HashSet<_>=entries.iter().take(CAPACITY).map(|e|Key::new(e,size)).collect();
        self.cache.retain(|key,_|visible.contains(key));
        for (key,pixels) in worker.receiver.try_iter() {
            self.pending.remove(&key);
            if visible.contains(&key) {self.cache.insert(key,pixels);}
        }
        for key in visible {
            if !self.cache.contains_key(&key) && !self.pending.contains(&key)
                && self.pending.len()<CAPACITY
                && worker.sender.try_send(key.clone()).is_ok() {self.pending.insert(key);}
        }
    }
    pub fn get(&self, entry: &Entry, size: i32) -> Option<&Pixels> {
        self.cache.get(&Key::new(entry,size)).and_then(Option::as_ref)
    }
}

fn load(key: &Key) -> windows::core::Result<Pixels> {
    if key.path.is_some() {
        if let Ok(pixels)=query_icon(key,true) {return Ok(pixels);}
    }
    query_icon(key,false)
}
fn query_icon(key: &Key, specific: bool) -> windows::core::Result<Pixels> {
    use windows::Win32::{Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY,FILE_ATTRIBUTE_NORMAL},
        UI::{Shell::{SHGetFileInfoW,SHFILEINFOW,SHGFI_ICON,SHGFI_USEFILEATTRIBUTES,SHGFI_SMALLICON,SHGFI_LARGEICON}, WindowsAndMessaging::DestroyIcon}};
    use std::os::windows::ffi::OsStrExt;
    let generic=format!("luciddesk.{}",key.extension);
    let path=if specific {key.path.as_deref().unwrap().as_os_str()} else {std::ffi::OsStr::new(&generic)};
    let name:Vec<u16>=path.encode_wide().chain(Some(0)).collect();
    let mut info=SHFILEINFOW::default();
    unsafe {
        let found=SHGetFileInfoW(windows::core::PCWSTR(name.as_ptr()),
            if key.folder {FILE_ATTRIBUTE_DIRECTORY} else {FILE_ATTRIBUTE_NORMAL},
            Some(&raw mut info),size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON|if specific {windows::Win32::UI::Shell::SHGFI_FLAGS(0)} else {SHGFI_USEFILEATTRIBUTES}|if key.size<=16 {SHGFI_SMALLICON} else {SHGFI_LARGEICON});
        let pixels=if found==0 {Err(windows::core::Error::from_thread())}
            else {crate::pane::assets::icon_pixels(info.hIcon)};
        if !info.hIcon.is_invalid() {let _=DestroyIcon(info.hIcon);}
        pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(path:&str,folder:bool)->Entry {Entry {path:path.into(),folder}}
    #[test]
    fn specific_icons_use_real_paths_and_fall_back_for_missing_files() {
        let _sta=crate::pane::test_support::apartment();
        let root=tempfile::tempdir().unwrap();
        let system=std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap());
        let exe=system.join("regedit.exe");
        let key=Key::new(&Entry{path:exe.clone(),folder:false},32);
        assert_eq!(key.path.as_ref(),Some(&exe));
        let pixels=query_icon(&key,true).unwrap();
        assert!(pixels.data.chunks_exact(4).any(|p|p[3]!=0));
        assert_ne!(key,Key::new(&entry("C:/different.exe",false),32));
        let missing=Key::new(&Entry{path:root.path().join("missing.exe"),folder:false},32);
        assert!(load(&missing).is_ok());

        use windows::Win32::{System::Com::{CoCreateInstance,IPersistFile,CLSCTX_INPROC_SERVER}, UI::Shell::{ShellLink,IShellLinkW}};
        use windows::core::{Interface,PCWSTR};
        use std::os::windows::ffi::OsStrExt;
        let wide=|p:&std::path::Path|p.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
        let library=system.join("System32/shell32.dll");
        let shortcut=root.path().join("custom.lnk");
        unsafe {
            let link:IShellLinkW=CoCreateInstance(&ShellLink,None,CLSCTX_INPROC_SERVER).unwrap();
            link.SetPath(PCWSTR(wide(&exe).as_ptr())).unwrap();
            link.SetIconLocation(PCWSTR(wide(&library).as_ptr()),13).unwrap();
            let persist:IPersistFile=link.cast().unwrap();
            persist.Save(PCWSTR(wide(&shortcut).as_ptr()),true).unwrap();
        }
        let shortcut_key=Key::new(&Entry{path:shortcut,folder:false},32);
        let shortcut_pixels=query_icon(&shortcut_key,true).unwrap();
        assert_ne!(shortcut_pixels.data,query_icon(&shortcut_key,false).unwrap().data);

        let folder=root.path().join("custom folder");
        std::fs::create_dir(&folder).unwrap();
        let ini=folder.join("desktop.ini");
        let text=format!("[.ShellClassInfo]\r\nIconResource={},13\r\n",library.display());
        let bytes:Vec<u8>=std::iter::once(0xfeffu16).chain(text.encode_utf16()).flat_map(u16::to_le_bytes).collect();
        std::fs::write(&ini,bytes).unwrap();
        unsafe {
            use windows_sys::Win32::Storage::FileSystem::*;
            assert_ne!(SetFileAttributesW(wide(&ini).as_ptr(),FILE_ATTRIBUTE_HIDDEN|FILE_ATTRIBUTE_SYSTEM),0);
            assert_ne!(SetFileAttributesW(wide(&folder).as_ptr(),FILE_ATTRIBUTE_READONLY),0);
        }
        let folder_key=Key::new(&Entry{path:folder.clone(),folder:true},32);
        let custom=query_icon(&folder_key,true).unwrap();
        let generic=query_icon(&folder_key,false).unwrap();
        unsafe {
            windows_sys::Win32::Storage::FileSystem::SetFileAttributesW(wide(&folder).as_ptr(),windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL);
        }
        assert_ne!(custom.data,generic.data,"custom desktop.ini icon must be preserved");
    }

    #[test]
    fn shell_folder_icon_contains_color_and_valid_alpha_pixels() {
        let _sta=crate::pane::test_support::apartment();
        let pixels=load(&Key {extension:String::new(),folder:true,size:32,path:None}).unwrap();
        assert_eq!(pixels.data.len(),(pixels.width*pixels.height*4) as usize);
        assert!(pixels.data.chunks_exact(4).any(|p|p[3]>0 && p[0]!=p[2]));
    }
}
