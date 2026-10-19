//! PDH query owned and closed on the sampling thread in the monitor process.
use super::gpu;
use std::ptr;
use windows_sys::Win32::System::Performance::*;

pub(super) struct Counters {
    query: PDH_HQUERY,
    cpu: Option<PDH_HCOUNTER>,
    engines: Option<PDH_HCOUNTER>,
    memory: Option<PDH_HCOUNTER>,
    requested: (bool, bool),
    primed: bool,
}
impl Drop for Counters {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}
impl Counters {
    pub(super) fn new(cpu: bool, gpu: bool) -> Option<Self> {
        let mut query = ptr::null_mut();
        if unsafe { PdhOpenQueryW(ptr::null(), 0, &mut query) } != 0 {
            return None;
        }
        let mut result = Self {
            query,
            cpu: None,
            engines: None,
            memory: None,
            requested: (cpu, gpu),
            primed: false,
        };
        let add = |path: &str| {
            let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            let mut counter = ptr::null_mut();
            (unsafe { PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) } == 0)
                .then_some(counter)
        };
        if cpu {
            result.cpu = add(r"\Processor(_Total)\% Idle Time");
        }
        if gpu {
            result.engines = add(r"\GPU Engine(*)\Utilization Percentage");
            result.memory = add(r"\GPU Adapter Memory(*)\Dedicated Usage");
        }
        Some(result)
    }
    pub(super) fn complete(&self) -> bool {
        (!self.requested.0 || self.cpu.is_some())
            && (!self.requested.1 || self.engines.is_some() && self.memory.is_some())
    }
    pub(super) fn sample(&mut self, adapters: &mut [gpu::Adapter]) -> Option<f64> {
        for adapter in adapters.iter_mut() {
            adapter.device.usage = None;
            adapter.device.used = None;
        }
        if unsafe { PdhCollectQueryData(self.query) } != 0 {
            self.primed = false;
            return None;
        }
        let cpu = self.cpu.and_then(|counter| {
            let mut value = PDH_FMT_COUNTERVALUE::default();
            if unsafe {
                PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, ptr::null_mut(), &mut value)
            } != 0
                || !valid(value.CStatus)
            {
                return None;
            }
            let idle = unsafe { value.Anonymous.doubleValue };
            (self.primed && idle.is_finite()).then(|| (100.0 - idle).clamp(0.0, 100.0))
        });
        let engines = self.engines.map(rows).unwrap_or_default();
        let memory = self.memory.map(rows).unwrap_or_default();
        for adapter in adapters {
            if self.primed {
                adapter.device.usage = adapter
                    .instances
                    .iter()
                    .filter_map(|id| gpu::engine_usage(id, &engines))
                    .reduce(f64::max);
            }
            adapter.device.used = memory
                .iter()
                .filter(|(id, _)| {
                    adapter
                        .instances
                        .iter()
                        .any(|instance| instance.eq_ignore_ascii_case(id))
                })
                .map(|(_, v)| *v as u64)
                .max();
        }
        self.primed = true;
        cpu
    }
}
fn valid(status: u32) -> bool {
    status == 0 || status == PDH_CSTATUS_NEW_DATA
}
fn rows(counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
    let mut size = 0;
    let mut count = 0;
    unsafe {
        PdhGetFormattedCounterArrayW(
            counter,
            PDH_FMT_DOUBLE | 0x8000,
            &mut size,
            &mut count,
            ptr::null_mut(),
        );
    }
    let mut storage = vec![0u64; (size as usize).div_ceil(8)];
    if unsafe {
        PdhGetFormattedCounterArrayW(
            counter,
            PDH_FMT_DOUBLE | 0x8000,
            &mut size,
            &mut count,
            storage.as_mut_ptr().cast(),
        )
    } != 0
    {
        return Vec::new();
    }
    let items = unsafe {
        std::slice::from_raw_parts(
            storage.as_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>(),
            count as usize,
        )
    };
    items
        .iter()
        .filter_map(|item| {
            if !valid(item.FmtValue.CStatus) {
                return None;
            }
            let mut n = 0;
            unsafe {
                while *item.szName.add(n) != 0 {
                    n += 1;
                }
            }
            let value = unsafe { item.FmtValue.Anonymous.doubleValue };
            if !value.is_finite() || value < 0.0 {
                return None;
            }
            Some((
                String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(item.szName, n) }),
                value,
            ))
        })
        .collect()
}
