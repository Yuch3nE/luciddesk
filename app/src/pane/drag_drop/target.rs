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

type Accept = Rc<dyn Fn(&[ShellIdentity], bool) -> bool>;
#[implement(IDropTarget)]
struct Target {
    effect: DROPEFFECT,
    hwnd: HWND,
    helper: Option<IDropTargetHelper>,
    accept: Accept,
    items: RefCell<Vec<ShellIdentity>>,
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
        let items = data
            .as_ref()
            .and_then(|d| luciddesk_shell::drag_shell_identities(d).ok())
            .unwrap_or_default();
        *self.items.borrow_mut() = items;
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
        self.items.borrow_mut().clear();
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
            *effect = if !items.is_empty()
                && (*effect & self.effect) != DROPEFFECT_NONE
                && (self.accept)(&items, false)
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
            if *effect != DROPEFFECT_NONE && !(self.accept)(&items, true) {
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
            *effect = if !items.is_empty()
                && (*effect & self.effect) != DROPEFFECT_NONE
                && (self.accept)(&items, false)
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
        accept: impl Fn(&[ShellIdentity], bool) -> bool + 'static,
    ) -> Result<Self> {
        let target: IDropTarget = Target {
            effect,
            hwnd,
            helper: unsafe {
                CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER).ok()
            },
            accept: Rc::new(accept),
            items: RefCell::new(Vec::new()),
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
    fn drop_commits_only_after_shell_helper_cleanup() {
        let _apartment = crate::pane::test_support::apartment();
        for accepted in [true, false] {
            let events = Rc::new(RefCell::new(Vec::new()));
            let observed = events.clone();
            let target: IDropTarget = Target {
                effect: DROPEFFECT_LINK,
                hwnd: HWND::default(),
                helper: Some(Helper(events.clone()).into()),
                accept: Rc::new(move |_, commit| {
                    if commit {
                        observed.borrow_mut().push(("commit", 0, 0));
                        accepted
                    } else { true }
                }),
                items: RefCell::new(vec![ShellIdentity::Namespace {
                    parsing_name: "test:dragged".into(),
                }]),
                description: RefCell::new(None),
            }.into();
            unsafe {
                let data: IDataObject = SHCreateDataObject(None, None, None).unwrap();
                let mut effect = DROPEFFECT_LINK;
                target.Drop(&data, MODIFIERKEYS_FLAGS(0), POINTL { x: 10, y: 20 }, &raw mut effect).unwrap();
                assert_eq!(effect, if accepted { DROPEFFECT_LINK } else { DROPEFFECT_NONE });
            }
            assert_eq!(*events.borrow(), [("drop", 10, 20), ("commit", 0, 0)]);
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
            accept: Rc::new(|_, _| false),
            items: RefCell::new(Vec::new()),
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
