windows_core::link!("dcomp.dll" "system" fn DCompositionCreateDevice(dxgidevice : *mut core::ffi::c_void, iid : *const windows_core::GUID, dcompositiondevice : *mut *mut core::ffi::c_void) -> windows_core::HRESULT);
pub type HWND = *mut core::ffi::c_void;
windows_core::imp::define_interface!(
    IDCompositionAnimation,
    IDCompositionAnimation_Vtbl,
    0xcbfd91d9_51b2_45e4_b3de_d19ccfb863c5
);
windows_core::imp::interface_hierarchy!(IDCompositionAnimation, windows_core::IUnknown);
#[repr(C)]
pub struct IDCompositionAnimation_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    Reset: usize,
    SetAbsoluteBeginTime: usize,
    AddCubic: usize,
    AddSinusoidal: usize,
    AddRepeat: usize,
    End: usize,
}
impl windows_core::RuntimeName for IDCompositionAnimation {}
windows_core::imp::define_interface!(
    IDCompositionDevice,
    IDCompositionDevice_Vtbl,
    0xc37ea93a_e7aa_450d_b16f_9746cb0407f3
);
windows_core::imp::interface_hierarchy!(IDCompositionDevice, windows_core::IUnknown);
impl IDCompositionDevice {
    pub unsafe fn Commit(&self) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).Commit)(windows_core::Interface::as_raw(self))
        }
    }
    pub unsafe fn CreateTargetForHwnd(
        &self,
        hwnd: HWND,
        topmost: bool,
    ) -> windows_core::Result<IDCompositionTarget> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CreateTargetForHwnd)(
                windows_core::Interface::as_raw(self),
                hwnd,
                topmost.into(),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub unsafe fn CreateVisual(&self) -> windows_core::Result<IDCompositionVisual> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CreateVisual)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
    pub unsafe fn CreateEffectGroup(&self) -> windows_core::Result<IDCompositionEffectGroup> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CreateEffectGroup)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IDCompositionDevice_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    pub Commit: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    WaitForCommitCompletion: usize,
    GetFrameStatistics: usize,
    pub CreateTargetForHwnd: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        HWND,
        windows_core::BOOL,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateVisual: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    CreateSurface: usize,
    CreateVirtualSurface: usize,
    CreateSurfaceFromHandle: usize,
    CreateSurfaceFromHwnd: usize,
    CreateTranslateTransform: usize,
    CreateScaleTransform: usize,
    CreateRotateTransform: usize,
    CreateSkewTransform: usize,
    CreateMatrixTransform: usize,
    CreateTransformGroup: usize,
    CreateTranslateTransform3D: usize,
    CreateScaleTransform3D: usize,
    CreateRotateTransform3D: usize,
    CreateMatrixTransform3D: usize,
    CreateTransform3DGroup: usize,
    pub CreateEffectGroup: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    CreateRectangleClip: usize,
    CreateAnimation: usize,
    CheckDeviceState: usize,
}
impl windows_core::RuntimeName for IDCompositionDevice {}
windows_core::imp::define_interface!(
    IDCompositionEffect,
    IDCompositionEffect_Vtbl,
    0xec81b08f_bfcb_4e8d_b193_a915587999e8
);
windows_core::imp::interface_hierarchy!(IDCompositionEffect, windows_core::IUnknown);
#[repr(C)]
pub struct IDCompositionEffect_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
}
pub trait IDCompositionEffect_Impl: windows_core::IUnknownImpl {}
impl IDCompositionEffect_Vtbl {
    pub const fn new<Identity: IDCompositionEffect_Impl, const OFFSET: isize>() -> Self {
        Self {
            base__: windows_core::IUnknown_Vtbl::new::<Identity, OFFSET>(),
        }
    }
    pub fn matches(iid: &windows_core::GUID) -> bool {
        iid == &<IDCompositionEffect as windows_core::Interface>::IID
    }
}
impl windows_core::RuntimeName for IDCompositionEffect {}
windows_core::imp::define_interface!(
    IDCompositionEffectGroup,
    IDCompositionEffectGroup_Vtbl,
    0xa7929a74_e6b2_4bd6_8b95_4040119ca34d
);
impl core::ops::Deref for IDCompositionEffectGroup {
    type Target = IDCompositionEffect;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    IDCompositionEffectGroup,
    windows_core::IUnknown,
    IDCompositionEffect
);
impl IDCompositionEffectGroup {
    pub unsafe fn SetOpacity<P0>(&self, animation: P0) -> windows_core::HRESULT
    where
        P0: windows_core::Param<IDCompositionAnimation>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetOpacity)(
                windows_core::Interface::as_raw(self),
                animation.param().abi(),
            )
        }
    }
    pub unsafe fn SetOpacity2(&self, opacity: f32) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).SetOpacity2)(
                windows_core::Interface::as_raw(self),
                opacity,
            )
        }
    }
}
#[repr(C)]
pub struct IDCompositionEffectGroup_Vtbl {
    pub base__: IDCompositionEffect_Vtbl,
    pub SetOpacity: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetOpacity2:
        unsafe extern "system" fn(*mut core::ffi::c_void, f32) -> windows_core::HRESULT,
    SetTransform3D: usize,
}
impl windows_core::RuntimeName for IDCompositionEffectGroup {}
windows_core::imp::define_interface!(
    IDCompositionTarget,
    IDCompositionTarget_Vtbl,
    0xeacdd04c_117e_4e17_88f4_d1b12b0e3d89
);
windows_core::imp::interface_hierarchy!(IDCompositionTarget, windows_core::IUnknown);
impl IDCompositionTarget {
    pub unsafe fn SetRoot<P0>(&self, visual: P0) -> windows_core::HRESULT
    where
        P0: windows_core::Param<IDCompositionVisual>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetRoot)(
                windows_core::Interface::as_raw(self),
                visual.param().abi(),
            )
        }
    }
}
#[repr(C)]
pub struct IDCompositionTarget_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    pub SetRoot: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
pub trait IDCompositionTarget_Impl: windows_core::IUnknownImpl {
    fn SetRoot(&self, visual: windows_core::Ref<IDCompositionVisual>) -> windows_core::Result<()>;
}
impl IDCompositionTarget_Vtbl {
    pub const fn new<Identity: IDCompositionTarget_Impl, const OFFSET: isize>() -> Self {
        unsafe extern "system" fn SetRoot<
            Identity: IDCompositionTarget_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            visual: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                IDCompositionTarget_Impl::SetRoot(this, core::mem::transmute_copy(&visual)).into()
            }
        }
        Self {
            base__: windows_core::IUnknown_Vtbl::new::<Identity, OFFSET>(),
            SetRoot: SetRoot::<Identity, OFFSET>,
        }
    }
    pub fn matches(iid: &windows_core::GUID) -> bool {
        iid == &<IDCompositionTarget as windows_core::Interface>::IID
    }
}
impl windows_core::RuntimeName for IDCompositionTarget {}
windows_core::imp::define_interface!(
    IDCompositionVisual,
    IDCompositionVisual_Vtbl,
    0x4d93059d_097b_4651_9a60_f0f25116e2f3
);
windows_core::imp::interface_hierarchy!(IDCompositionVisual, windows_core::IUnknown);
impl IDCompositionVisual {
    pub unsafe fn SetEffect<P0>(&self, effect: P0) -> windows_core::HRESULT
    where
        P0: windows_core::Param<IDCompositionEffect>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetEffect)(
                windows_core::Interface::as_raw(self),
                effect.param().abi(),
            )
        }
    }
    pub unsafe fn SetContent<P0>(&self, content: P0) -> windows_core::HRESULT
    where
        P0: windows_core::Param<windows_core::IUnknown>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetContent)(
                windows_core::Interface::as_raw(self),
                content.param().abi(),
            )
        }
    }
}
#[repr(C)]
pub struct IDCompositionVisual_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    SetOffsetX: usize,
    SetOffsetX2: usize,
    SetOffsetY: usize,
    SetOffsetY2: usize,
    SetTransform: usize,
    SetTransform2: usize,
    SetTransformParent: usize,
    pub SetEffect: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    SetBitmapInterpolationMode: usize,
    SetBorderMode: usize,
    SetClip: usize,
    SetClip2: usize,
    pub SetContent: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    AddVisual: usize,
    RemoveVisual: usize,
    RemoveAllVisuals: usize,
    SetCompositeMode: usize,
}
impl windows_core::RuntimeName for IDCompositionVisual {}
windows_core::imp::define_interface!(
    IDXGIDevice,
    IDXGIDevice_Vtbl,
    0x54ec77fa_1377_44e6_8c32_88fd5f44c84c
);
impl core::ops::Deref for IDXGIDevice {
    type Target = IDXGIObject;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(IDXGIDevice, windows_core::IUnknown, IDXGIObject);
#[repr(C)]
pub struct IDXGIDevice_Vtbl {
    pub base__: IDXGIObject_Vtbl,
    GetAdapter: usize,
    CreateSurface: usize,
    QueryResourceResidency: usize,
    SetGPUThreadPriority: usize,
    GetGPUThreadPriority: usize,
}
impl windows_core::RuntimeName for IDXGIDevice {}
windows_core::imp::define_interface!(
    IDXGIObject,
    IDXGIObject_Vtbl,
    0xaec22fb8_76f3_4639_9be0_28eb43a67a2e
);
windows_core::imp::interface_hierarchy!(IDXGIObject, windows_core::IUnknown);
#[repr(C)]
pub struct IDXGIObject_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    SetPrivateData: usize,
    SetPrivateDataInterface: usize,
    GetPrivateData: usize,
    GetParent: usize,
}
impl windows_core::RuntimeName for IDXGIObject {}
