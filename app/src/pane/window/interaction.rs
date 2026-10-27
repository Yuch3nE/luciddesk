//! Item drag state shared by pointer messages, tab changes and cancellation.
use super::GroupModel;
use crate::pane::drag_drop::image::DragImage;
use luciddesk_core::{PointDip, ShellIdentity};
use std::cell::RefCell;
use windows_sys::Win32::Foundation::POINT;

#[derive(Default)]
pub(super) struct ItemDrag {
    pub press: Option<(usize, POINT, bool)>,
    pub offset: PointDip,
    pub identity: Option<ShellIdentity>,
    pub image: Option<DragImage>,
}

impl ItemDrag {
    /// Release the preview before releasing capture. Capture loss must retain
    /// the identity until the mouse-up handler resolves the current item index.
    pub fn finish_preview(&mut self) -> Option<(usize, POINT, bool)> {
        let press = self.press.take();
        self.image = None;
        press
    }

    /// Explicit cancellation and OLE handoff also discard the target identity.
    pub fn cancel(&mut self) {
        self.press = None;
        self.identity = None;
        self.image = None;
    }

    pub fn take_target(&mut self, model: &RefCell<GroupModel>) -> Option<usize> {
        let identity = self.identity.take()?;
        model
            .borrow()
            .items
            .iter()
            .position(|item| item.identity == identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_loss_preserves_mouse_up_target_across_inventory_reorder() {
        let state = crate::pane::tests::test_state();
        let model = RefCell::new(
            crate::pane::create_model(&state, luciddesk_core::PanelId::new(1)).unwrap(),
        );
        let mut drag = ItemDrag {
            press: Some((0, POINT::default(), true)),
            identity: Some(model.borrow().items[0].identity.clone()),
            ..Default::default()
        };
        let press = drag.finish_preview();
        assert!(matches!(press, Some((0, _, true))));
        // ReleaseCapture can synchronously dispatch another cancellation message.
        assert!(drag.finish_preview().is_none());
        model.borrow_mut().items.swap(0, 1);
        assert_eq!(drag.take_target(&model), Some(1));
        assert_eq!(drag.take_target(&model), None);
    }

    #[test]
    fn explicit_cancel_and_removed_targets_cannot_produce_a_drop() {
        let state = crate::pane::tests::test_state();
        let model = RefCell::new(
            crate::pane::create_model(&state, luciddesk_core::PanelId::new(1)).unwrap(),
        );
        let mut drag = ItemDrag {
            press: Some((0, POINT::default(), true)),
            identity: Some(model.borrow().items[0].identity.clone()),
            ..Default::default()
        };
        drag.cancel();
        assert!(drag.press.is_none());
        {
            // No identity means no model borrow, even during reentrant cleanup.
            let _borrow = model.borrow_mut();
            assert_eq!(drag.take_target(&model), None);
        }
        drag.identity = Some(model.borrow().items[0].identity.clone());
        model.borrow_mut().items.remove(0);
        assert_eq!(drag.take_target(&model), None);
    }
}
