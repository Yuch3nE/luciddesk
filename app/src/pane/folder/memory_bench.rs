//! Manual memory measurement using real folder sources and composition windows.
use super::*;
use std::time::{Duration, Instant};
use windows::Win32::Graphics::Dxgi::{
    DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL,
    DXGI_QUERY_VIDEO_MEMORY_INFO, IDXGIAdapter3, IDXGIDevice,
};
use windows::core::Interface as _;
use windows_sys::Win32::{
    System::{ProcessStatus::*, Threading::GetCurrentProcess},
    UI::WindowsAndMessaging::*,
};

const FILES: usize = 512;

fn thumbnail(index: usize) -> Vec<u8> {
    const SIDE: usize = 128;
    let data = SIDE * SIDE * 3;
    let mut bmp = vec![0; 54 + data];
    bmp[..2].copy_from_slice(b"BM");
    for (at, value) in [(2, 54 + data), (10, 54), (14, 40), (18, SIDE), (22, SIDE), (34, data)] {
        bmp[at..at + 4].copy_from_slice(&(value as u32).to_le_bytes());
    }
    bmp[26] = 1;
    bmp[28] = 24;
    for (pixel, rgb) in bmp[54..].chunks_exact_mut(3).enumerate() {
        rgb.copy_from_slice(&[index as u8, (index >> 8) as u8 * 31, (pixel / SIDE + pixel % SIDE) as u8]);
    }
    bmp
}

fn pump(state: &Rc<RefCell<PaneApp>>, done: impl Fn(&PaneApp) -> bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&raw mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message != WM_QUIT {
                    TranslateMessage(&raw const msg);
                    DispatchMessageW(&raw const msg);
                }
            }
        }
        let mut app = state.borrow_mut();
        poll(&mut app);
        if done(&app) { return true; }
        drop(app);
        if Instant::now() >= deadline { return false; }
        std::thread::sleep(Duration::from_millis(8));
    }
}

fn sample(phase: &str, state: &PaneApp, adapter: Option<&IDXGIAdapter3>) {
    let mut ram = PROCESS_MEMORY_COUNTERS_EX::default();
    ram.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    assert_ne!(unsafe { GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut ram).cast(), ram.cb) }, 0);
    let gpu = |segment| {
        let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        adapter.and_then(|adapter| unsafe { adapter.QueryVideoMemoryInfo(0, segment, &raw mut info).ok() })
            .map_or("unavailable".into(), |()| (info.CurrentUsage / 1024).to_string())
    };
    let mut unique = HashMap::new();
    for source in state.folders.values() {
        for image in source.items.iter().filter_map(|item| item.image.as_ref()) {
            unique.insert(Arc::as_ptr(image), image.data.capacity());
        }
    }
    println!("memory_fixture phase={phase} private_kib={} working_kib={} gpu_local_kib={} gpu_nonlocal_kib={} snapshot_pixel_kib={} snapshot_images={}",
        ram.PrivateUsage / 1024, ram.WorkingSetSize / 1024, gpu(DXGI_MEMORY_SEGMENT_GROUP_LOCAL),
        gpu(DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL), unique.values().sum::<usize>() / 1024, unique.len());
}

#[test]
#[ignore = "Manual benchmark using real windows and generated large folders; run alone"]
fn large_folder_panel_memory() {
    let count: usize = std::env::var("LUCIDDESK_MEMORY_PANELS").unwrap_or_else(|_| "1".into()).parse().unwrap();
    assert!(matches!(count, 1 | 4));
    let _sta = crate::pane::test_support::apartment();
    let _graphics = super::super::native_graphics::GraphicsLifetime;
    let device = super::super::native_graphics::gpu_device().unwrap();
    let dxgi: IDXGIDevice = super::super::native_graphics::native_interface(device.d3d_device()).unwrap();
    let adapter = unsafe { dxgi.GetAdapter() }.ok().and_then(|adapter| adapter.cast::<IDXGIAdapter3>().ok());
    let root = tempfile::tempdir().unwrap();
    let state = Rc::new(RefCell::new(super::super::tests::test_state()));
    sample("idle", &state.borrow(), adapter.as_ref());

    for index in 0..count {
        let dir = root.path().join(format!("panel-{index}"));
        std::fs::create_dir(&dir).unwrap();
        for row in 0..FILES {
            std::fs::write(dir.join(format!("photo-{row:04}.bmp")), thumbnail(index * FILES + row)).unwrap();
        }
        let id = PanelId::new(10 + index as u64);
        let mut panel = Panel::new(id, format!("Memory fixture {index}"), RectDip::from_bounds(30.0, 30.0, 600.0, 450.0));
        panel.set_folder(Some(dir));
        state.borrow_mut().workspace.add_panel(panel).unwrap();
        create_view(&state, id).unwrap();
    }
    let windows: Vec<_> = state.borrow().views.iter().map(|view| view.window.hwnd().cast()).collect();
    for (index, &hwnd) in windows.iter().enumerate() {
        unsafe {
            assert_ne!(SetWindowPos(hwnd, std::ptr::null_mut(), 30 + 35 * index as i32, 30 + 24 * index as i32,
                600, 450, SWP_NOZORDER | SWP_NOACTIVATE), 0);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            assert_ne!(IsWindowVisible(hwnd), 0);
        }
    }
    assert!(pump(&state, |app| app.folders.values().all(|source| {
        source.items.len() == FILES && source.items.iter().any(|item| item.image.is_some())
            && !source.request.images_pending.load(std::sync::atomic::Ordering::Acquire)
    }), Duration::from_secs(45)), "large folders did not settle");
    pump(&state, |_| false, Duration::from_millis(900));
    sample("expanded", &state.borrow(), adapter.as_ref());

    let resize = |collapsed: bool| {
        for view in &state.borrow().views {
            view.model.borrow_mut().collapsed = collapsed;
        }
        for &hwnd in &windows {
            unsafe {
                assert_ne!(SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 600,
                    if collapsed { super::super::layout::HEADER as i32 } else { 450 },
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE), 0);
            }
        }
    };
    resize(true);
    pump(&state, |_| false, Duration::from_millis(900));
    sample("collapsed", &state.borrow(), adapter.as_ref());
    resize(false);
    pump(&state, |_| false, Duration::from_millis(900));
    for &hwnd in &windows { unsafe { ShowWindow(hwnd, SW_HIDE); } }
    assert!(windows.iter().all(|&hwnd| unsafe { SendMessageW(hwnd, window::TEST_SURFACE_OPACITY, 0, 0) >= 0 }),
        "a brief hide should preserve the surface");
    pump(&state, |_| false, Duration::from_secs(7));
    sample("hidden", &state.borrow(), adapter.as_ref());
    assert!(windows.iter().all(|&hwnd| unsafe { SendMessageW(hwnd, window::TEST_SURFACE_OPACITY, 0, 0) == -1 }),
        "persistent hide should retire all folder surfaces");

    for &hwnd in &windows { unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE); } }
    assert!(pump(&state, |app| app.views.iter().all(|view| view.model.borrow().items.iter().any(|item| item.image.is_some())),
        Duration::from_secs(10)), "restored windows did not regain icons");
    pump(&state, |_| false, Duration::from_millis(700));
    sample("restored", &state.borrow(), adapter.as_ref());
    assert!(windows.iter().all(|&hwnd| unsafe { SendMessageW(hwnd, window::TEST_SURFACE_OPACITY, 0, 0) == 1000 }),
        "restored panes should have fully visible surfaces");
}
