//! 仅由隔离 worker 查询实际图像设备的类型，避免在宿主热路径触发驱动调用。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use windows::core::{s, w, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{FreeLibrary, HMODULE, LUID};
use windows::Win32::Graphics::DXCore::{IDXCoreAdapter, IDXCoreAdapterFactory, IsIntegrated};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
};

use super::native_protocol::NativeAdapterKind;

pub(super) fn classify(luid_high: i32, luid_low: u32) -> NativeAdapterKind {
    static CACHE: OnceLock<Mutex<HashMap<(i32, u32), NativeAdapterKind>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    *cache.entry((luid_high, luid_low)).or_insert_with(|| {
        probe(LUID {
            HighPart: luid_high,
            LowPart: luid_low,
        })
        .unwrap_or_default()
    })
}

struct DxCoreLibrary(HMODULE);

impl Drop for DxCoreLibrary {
    fn drop(&mut self) {
        // COM 对象先于此 guard 释放，库的虚表代码始终有效。
        let _ = unsafe { FreeLibrary(self.0) };
    }
}

fn probe(luid: LUID) -> Option<NativeAdapterKind> {
    // DXCore 并非所有受支持的 Windows 都有；仅从系统目录加载，缺失保留未知。
    let library = DxCoreLibrary(unsafe {
        LoadLibraryExW(w!("dxcore.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?
    });
    type CreateFactory =
        unsafe extern "system" fn(*const GUID, *mut *mut std::ffi::c_void) -> HRESULT;
    let address = unsafe { GetProcAddress(library.0, s!("DXCoreCreateAdapterFactory"))? };
    // 导出签名来自 dxcore.h；库 guard 覆盖全部调用与 COM 对象的生命周期。
    let create: CreateFactory = unsafe { std::mem::transmute(address) };
    let mut raw = std::ptr::null_mut();
    unsafe { create(&IDXCoreAdapterFactory::IID, &mut raw).ok().ok()? };
    let factory = unsafe { IDXCoreAdapterFactory::from_raw(raw) };
    let adapter: IDXCoreAdapter = unsafe { factory.GetAdapterByLuid(&luid).ok()? };
    let mut integrated = false;
    unsafe {
        adapter
            .GetProperty(
                IsIntegrated,
                std::mem::size_of_val(&integrated),
                std::ptr::from_mut(&mut integrated).cast(),
            )
            .ok()?;
    }
    Some(if integrated {
        NativeAdapterKind::Integrated
    } else {
        NativeAdapterKind::Discrete
    })
}
