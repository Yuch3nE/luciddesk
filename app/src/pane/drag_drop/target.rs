//! OLE link-style collection drops. A successful drop never requests a source file move.
use luciddesk_core::ShellIdentity;
use std::{cell::RefCell, rc::Rc};
use windows::{
    Win32::{
        Foundation::{HWND, POINT, POINTL},
        System::{
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IDataObject},
            Ole::*,
            SystemServices::MODIFIERKEYS_FLAGS,
        },
        UI::Shell::{CLSID_DragDropHelper, IDropTargetHelper},
    },
    core::{Ref, Result, implement},
};

use super::PaneDrag;

type Accept = Rc<dyn Fn(&[ShellIdentity], Option<&PaneDrag>, Option<POINTL>) -> bool>;

enum DraggedItems {
    Shell(Vec<ShellIdentity>),
    Pane(Rc<PaneDrag>),
}

impl DraggedItems {
    fn accepted(&self, accept: &Accept, drop_point: Option<POINTL>) -> bool {
        match self {
            Self::Shell(items) => !items.is_empty() && accept(items, None, drop_point),
            Self::Pane(drag) => accept(&drag.items, Some(drag), drop_point),
        }
    }
}
#[implement(IDropTarget)]
struct Target {
    effect: DROPEFFECT,
    hwnd: HWND,
    helper: Option<IDropTargetHelper>,
    accept: Accept,
    items: RefCell<Option<Rc<DraggedItems>>>,
    description: RefCell<Option<Rc<super::description::QuietDescription>>>,
}
impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let screen = POINT {
            x: point.x,
            y: point.y,
        };
        // Explorer has already left its old target. Hand off the existing image
        // before decoding Shell objects, which may synchronously call Explorer.
        // This provisional effect is visual only; update_effect below decides
        // what we return to the source, and Drop validates again before commit.
        let preview_effect = unsafe { *effect & self.effect };
        if let (Some(helper), Some(data)) = (&self.helper, data.as_ref()) {
            unsafe {
                let _ = helper.DragEnter(self.hwnd, data, &raw const screen, preview_effect);
            }
        }
        drop(self.description.take());
        let items = data.as_ref().and_then(|data| {
            if let Some(drag) = super::pane_drag(data) {
                Some(DraggedItems::Pane(drag))
            } else {
                luciddesk_shell::drag_shell_identities(data).ok().map(DraggedItems::Shell)
            }
        });
        *self.items.borrow_mut() = items.map(Rc::new);
        self.update_effect(effect);
        if self.effect == DROPEFFECT_LINK && unsafe { *effect != DROPEFFECT_NONE } {
            *self.description.borrow_mut() = data
                .as_ref()
                .map(|data| Rc::new(super::description::QuietDescription::new(data)));
        }
        let description = self.description.borrow().clone();
        if let Some(description) = description {
            description.suppress();
        }
        // Correct rejected/offered effects and apply the quiet caption without
        // tearing down and recreating the helper's image.
        if let Some(helper) = &self.helper {
            unsafe {
                let _ = helper.DragOver(&raw const screen, *effect);
            }
        }
        let description = self.description.borrow().clone();
        if let Some(description) = description {
            description.suppress();
        }
        Ok(())
    }
    fn DragOver(
        &self,
        _: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        self.update_effect(effect);
        let description = self.description.borrow().clone();
        if let Some(description) = description {
            description.suppress();
        }
        if let Some(helper) = &self.helper {
            let point = POINT {
                x: point.x,
                y: point.y,
            };
            unsafe {
                let _ = helper.DragOver(&raw const point, *effect);
            }
        }
        let description = self.description.borrow().clone();
        if let Some(description) = description {
            description.suppress();
        }
        Ok(())
    }
    fn DragLeave(&self) -> Result<()> {
        let _ = self.items.take();
        if let Some(helper) = &self.helper {
            unsafe {
                let _ = helper.DragLeave();
            }
        }
        drop(self.description.take());
        Ok(())
    }
    fn Drop(
        &self,
        data: Ref<IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let items = self.items.take();
        unsafe {
            *effect = if (*effect & self.effect) != DROPEFFECT_NONE
                && items.as_ref().is_some_and(|items| items.accepted(&self.accept, None))
            {
                self.effect
            } else {
                DROPEFFECT_NONE
            };
        }
        if let (Some(helper), Some(data)) = (&self.helper, data.as_ref()) {
            let point = POINT {
                x: point.x,
                y: point.y,
            };
            unsafe {
                let _ = helper.Drop(data, &raw const point, *effect);
            }
        }
        drop(self.description.take());
        // Helper::Drop and restoring IDataObject descriptions can pump messages.
        // Queue the membership change only after both have finished, otherwise
        // our posted action can run while Explorer is still waiting for Drop.
        // Do not make any more outgoing COM calls after committing.
        unsafe {
            if *effect != DROPEFFECT_NONE
                && !items.as_ref().is_some_and(|items| items.accepted(&self.accept, Some(*point)))
            {
                *effect = DROPEFFECT_NONE;
            }
        }
        Ok(())
    }
}
impl Target_Impl {
    fn update_effect(&self, effect: *mut DROPEFFECT) {
        let items = self.items.borrow().clone();
        unsafe {
            *effect = if (*effect & self.effect) != DROPEFFECT_NONE
                && items.as_ref().is_some_and(|items| items.accepted(&self.accept, None))
            {
                self.effect
            } else {
                DROPEFFECT_NONE
            };
        }
    }
}
pub(in crate::pane) struct Registration {
    hwnd: HWND,
    _target: IDropTarget,
}
impl Registration {
    pub fn window(&self) -> HWND {
        self.hwnd
    }
    pub fn new(
        hwnd: HWND,
        effect: DROPEFFECT,
        accept: impl Fn(&[ShellIdentity], Option<&PaneDrag>, Option<POINTL>) -> bool + 'static,
    ) -> Result<Self> {
        let target: IDropTarget = Target {
            effect,
            hwnd,
            helper: unsafe {
                CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER).ok()
            },
            accept: Rc::new(accept),
            items: RefCell::new(None),
            description: RefCell::new(None),
        }
        .into();
        unsafe {
            RegisterDragDrop(hwnd, &target)?;
        }
        Ok(Self {
            hwnd,
            _target: target,
        })
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        let _ = unsafe { RevokeDragDrop(self.hwnd) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Shell::{IDropTargetHelper_Impl, SHCreateDataObject};

    #[implement(IDropTargetHelper)]
    struct Helper(Rc<RefCell<Vec<(&'static str, i32, i32)>>>);
    impl IDropTargetHelper_Impl for Helper_Impl {
        fn DragEnter(
            &self,
            _: HWND,
            _: Ref<IDataObject>,
            p: *const POINT,
            _: DROPEFFECT,
        ) -> Result<()> {
            let p = unsafe { *p };
            self.0.borrow_mut().push(("enter", p.x, p.y));
            Ok(())
        }
        fn DragOver(&self, p: *const POINT, _: DROPEFFECT) -> Result<()> {
            let p = unsafe { *p };
            self.0.borrow_mut().push(("over", p.x, p.y));
            Ok(())
        }
        fn DragLeave(&self) -> Result<()> {
            self.0.borrow_mut().push(("leave", 0, 0));
            Ok(())
        }
        fn Drop(&self, _: Ref<IDataObject>, p: *const POINT, _: DROPEFFECT) -> Result<()> {
            let p = unsafe { *p };
            self.0.borrow_mut().push(("drop", p.x, p.y));
            Ok(())
        }
        fn Show(&self, _: windows::core::BOOL) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn drag_returning_from_an_external_window_uses_the_pane_drop_point() {
        use crate::pane::{create_view, hybrid, items_for, tests::test_state};
        use luciddesk_core::{DesktopItem, DesktopPlacement, GridPosition, PanelId, PointDip};
        use windows_sys::Win32::{
            Foundation::RECT,
            Graphics::Gdi::ClientToScreen,
            UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::*},
        };

        let _apartment = crate::pane::test_support::apartment();
        let directory = tempfile::tempdir().unwrap();
        let identities = ["A", "B", "C"].map(|name| {
            let path = directory.path().join(format!("{name} test.txt"));
            std::fs::write(&path, []).unwrap();
            ShellIdentity::FileSystem { path, volume_id: None, file_id: None }
        });
        let source = PanelId::new(1);
        let mut app = test_state();
        let inventory = identities.iter().zip(["A", "B", "C"]).enumerate()
            .map(|(index, (identity, name))| {
                let mut item = DesktopItem::new(identity.clone(), name);
                item.set_placement(DesktopPlacement::Pane {
                    pane_id: source,
                    position: GridPosition::new(index as u32, 0),
                });
                item
            });
        app.workspace.reconcile_desktop_items(inventory);
        let state = Rc::new(RefCell::new(app));
        create_view(&state, source).unwrap();
        hybrid::register_drop(&state, source).unwrap();
        let (target, hwnd, mut first_cell) = {
            let app = state.borrow();
            let view = &app.views[0];
            let hwnd = view.window.hwnd().cast();
            let mut bounds = RECT::default();
            assert_ne!(unsafe { GetClientRect(hwnd, &raw mut bounds) }, 0);
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
            let model = view.model.borrow();
            let grid = model.grid(bounds.right as f32 / dpi, bounds.bottom as f32 / dpi);
            let (x, y) = model.cell(grid, 0);
            let point = windows_sys::Win32::Foundation::POINT {
                x: ((x + 8.0) * dpi).round() as i32,
                y: ((y + 8.0) * dpi).round() as i32,
            };
            (app.drops[0]._target.clone(), hwnd, point)
        };
        assert_ne!(unsafe { ClientToScreen(hwnd, &raw mut first_cell) }, 0);
        let point = POINTL { x: first_cell.x, y: first_cell.y };
        let grabbed = identities[2].clone();
        let file_drag = luciddesk_shell::FileDrag::new(std::slice::from_ref(&grabbed)).unwrap();
        let drag = PaneDrag {
            source, items: vec![grabbed.clone()], grabbed, offset: PointDip::new(8.0, 8.0),
        };
        super::super::with_pane_drag(file_drag.data(), drag, || {
            unsafe {
                let mut effect = DROPEFFECT_LINK;
                target.DragEnter(file_drag.data(), MODIFIERKEYS_FLAGS(0), point, &raw mut effect)?;
                // OLE sends DragLeave when the pointer crosses into another window.
                target.DragLeave()?;
                effect = DROPEFFECT_LINK;
                target.DragEnter(file_drag.data(), MODIFIERKEYS_FLAGS(0), point, &raw mut effect)?;
                assert_eq!(effect, DROPEFFECT_LINK);
                target.Drop(file_drag.data(), MODIFIERKEYS_FLAGS(0), point, &raw mut effect)?;
                assert_eq!(effect, DROPEFFECT_LINK);
            }
            Ok(())
        }).unwrap();

        let mut message = MSG::default();
        unsafe {
            while PeekMessageW(&raw mut message, hwnd, 0, 0, PM_REMOVE) != 0 {
                DispatchMessageW(&message);
            }
        }
        let app = state.borrow();
        let order: Vec<_> = items_for(&app, source).into_iter().map(|item| item.label).collect();
        assert_eq!(order, ["C", "A", "B"]);
        assert_eq!(app.store.load_workspace().unwrap(), app.workspace);
        assert!(super::super::pane_drag(file_drag.data()).is_none());
    }

    #[test]
    fn drop_commits_only_after_shell_helper_cleanup() {
        let _apartment = crate::pane::test_support::apartment();
        for accepted in [true, false] {
            let events = Rc::new(RefCell::new(Vec::new()));
            let observed = events.clone();
            let target: IDropTarget = Target {
                effect: DROPEFFECT_LINK,
                hwnd: HWND::default(),
                helper: Some(Helper(events.clone()).into()),
                accept: Rc::new(move |_, _, drop_point| {
                    if let Some(point) = drop_point {
                        observed.borrow_mut().push(("commit", point.x, point.y));
                        accepted
                    } else { true }
                }),
                items: RefCell::new(Some(Rc::new(DraggedItems::Shell(vec![ShellIdentity::Namespace {
                    parsing_name: "test:dragged".into(),
                }])))),
                description: RefCell::new(None),
            }.into();
            unsafe {
                let data: IDataObject = SHCreateDataObject(None, None, None).unwrap();
                let mut effect = DROPEFFECT_LINK;
                target.Drop(&data, MODIFIERKEYS_FLAGS(0), POINTL { x: 10, y: 20 }, &raw mut effect).unwrap();
                assert_eq!(effect, if accepted { DROPEFFECT_LINK } else { DROPEFFECT_NONE });
            }
            assert_eq!(*events.borrow(), [("drop", 10, 20), ("commit", 10, 20)]);
        }
    }

    #[test]
    fn drag_image_helper_receives_screen_coordinates_and_full_lifecycle() {
        let _apartment = crate::pane::test_support::apartment();
        let events = Rc::new(RefCell::new(Vec::new()));
        let target: IDropTarget = Target {
            effect: DROPEFFECT_LINK,
            hwnd: HWND::default(),
            helper: Some(Helper(events.clone()).into()),
            accept: Rc::new(|_, _, _| false),
            items: RefCell::new(None),
            description: RefCell::new(None),
        }
        .into();
        unsafe {
            let data: IDataObject = SHCreateDataObject(None, None, None).unwrap();
            let mut effect = DROPEFFECT_LINK;
            let point = POINTL { x: -640, y: 230 };
            target
                .DragEnter(&data, MODIFIERKEYS_FLAGS(0), point, &raw mut effect)
                .unwrap();
            target
                .DragOver(
                    MODIFIERKEYS_FLAGS(0),
                    POINTL { x: -620, y: 240 },
                    &raw mut effect,
                )
                .unwrap();
            target.DragLeave().unwrap();
            target
                .DragEnter(&data, MODIFIERKEYS_FLAGS(0), point, &raw mut effect)
                .unwrap();
            target
                .Drop(&data, MODIFIERKEYS_FLAGS(0), point, &raw mut effect)
                .unwrap();
        }
        assert_eq!(
            *events.borrow(),
            [
                ("enter", -640, 230),
                ("over", -640, 230),
                ("over", -620, 240),
                ("leave", 0, 0),
                ("enter", -640, 230),
                ("over", -640, 230),
                ("drop", -640, 230)
            ]
        );
    }
}
