//! Thread-local compositor and effect factory, released before the dispatcher queue.
use std::{cell::RefCell, rc::Rc};
use windows::{
    System::DispatcherQueueController,
    UI::Composition::Compositor,
    Win32::System::WinRT::{
        CreateDispatcherQueueController, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT,
        DispatcherQueueOptions,
    },
    core::Result,
};

pub(super) struct Runtime {
    // Release effects before their compositor and dispatcher queue.
    pub(super) material_factory:
        RefCell<Option<windows::UI::Composition::CompositionEffectFactory>>,
    pub(super) transition_factory:
        RefCell<Option<windows::UI::Composition::CompositionEffectFactory>>,
    pub(super) compositor: Compositor,
    _queue: DispatcherQueueController,
}

thread_local! {
    static RUNTIME: RefCell<Option<Rc<Runtime>>> = const { RefCell::new(None) };
}

pub(super) fn clear_thread_cache() {
    let runtime = RUNTIME.with(|slot| slot.borrow_mut().take());
    drop(runtime);
}

impl Runtime {
    pub(super) fn shared() -> Result<Rc<Self>> {
        RUNTIME.with(|slot| -> Result<Rc<Runtime>> {
            let mut slot = slot.borrow_mut();
            if let Some(runtime) = slot.as_ref() {
                return Ok(Rc::clone(runtime));
            }
            let queue = unsafe {
                CreateDispatcherQueueController(DispatcherQueueOptions {
                    dwSize: u32::try_from(size_of::<DispatcherQueueOptions>()).unwrap(),
                    threadType: DQTYPE_THREAD_CURRENT,
                    apartmentType: DQTAT_COM_NONE,
                })?
            };
            let runtime = Rc::new(Runtime {
                compositor: Compositor::new()?,
                _queue: queue,
                material_factory: RefCell::new(None),
                transition_factory: RefCell::new(None),
            });
            *slot = Some(Rc::clone(&runtime));
            Ok(runtime)
        })
    }
}
