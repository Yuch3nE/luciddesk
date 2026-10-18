//! PDH counters are keyed by DXGI adapter LUID, independent of vendor and locale.
use super::sampling::Device;
use std::collections::HashMap;
use windows_sys::Win32::System::Performance::*;

pub(super) struct GpuSampler {
    query: PDH_HQUERY,
    engines: Option<PDH_HCOUNTER>,
    memory: Option<PDH_HCOUNTER>,
    adapters: Vec<Device>,
    primed: bool,
    healthy: bool,
}
impl GpuSampler {
    pub(super) fn new() -> Option<Self> {
        let adapters = adapters();
        if adapters.is_empty() {
            return None;
        }
        let mut query = std::ptr::null_mut();
        if unsafe { PdhOpenQueryW(std::ptr::null(), 0, &raw mut query) } != 0 {
            return None;
        }
        let counter = |path: &str| {
            let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            let mut counter = std::ptr::null_mut();
            (unsafe { PdhAddEnglishCounterW(query, wide.as_ptr(), 0, &raw mut counter) } == 0)
                .then_some(counter)
        };
        Some(Self {
            query,
            adapters,
            engines: counter(r"\GPU Engine(*)\Utilization Percentage"),
            memory: counter(r"\GPU Adapter Memory(*)\Dedicated Usage"),
            primed: false,
            healthy: false,
        })
    }
    pub(super) fn sample(&mut self) -> Vec<Device> {
        let success = unsafe { PdhCollectQueryData(self.query) } == 0;
        let mut adapters = self.adapters.clone();
        if success {
            let engines = self.engines.map(values).unwrap_or_default();
            let memory = self.memory.map(values).unwrap_or_default();
            self.healthy = !engines.is_empty() || !memory.is_empty();
            for adapter in &mut adapters {
                if self.primed {
                    adapter.usage = engine_usage(&adapter.id, &engines);
                }
                adapter.used = memory
                    .iter()
                    .find(|(name, _)| name == &adapter.id)
                    .map(|(_, bytes)| *bytes as u64);
            }
        }
        if !success {
            self.healthy = false;
        }
        self.primed = success;
        adapters
    }
    pub(super) fn refresh_devices(&mut self) {
        if self.healthy {
            self.adapters = adapters();
        } else if let Some(replacement) = Self::new() {
            *self = replacement;
        }
    }
}
impl Drop for GpuSampler {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

fn adapters() -> Vec<Device> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
    };
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for index in 0..32 {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
            break;
        };
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let length = desc
            .Description
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(desc.Description.len());
        result.push(Device {
            id: format!(
                "luid_0x{:08x}_0x{:08x}_phys_0",
                desc.AdapterLuid.HighPart as u32, desc.AdapterLuid.LowPart
            ),
            name: String::from_utf16_lossy(&desc.Description[..length]),
            total: desc.DedicatedVideoMemory as u64,
            rate: None,
            used: None,
            usage: None,
        });
    }
    result
}

fn values(counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
    unsafe {
        let mut bytes = 0;
        let mut count = 0;
        // PDH_FMT_NOCAP100 (0x8000) is a header macro omitted from windows-sys.
        let format = PDH_FMT_DOUBLE | 0x8000;
        if PdhGetFormattedCounterArrayW(
            counter,
            format,
            &raw mut bytes,
            &raw mut count,
            std::ptr::null_mut(),
        ) != PDH_MORE_DATA
        {
            return Vec::new();
        }
        // PDH may change the wildcard instance set between calls. Retry boundedly,
        // using u64 storage to satisfy the returned structures' alignment.
        for _ in 0..3 {
            if bytes == 0 || bytes > 32 * 1024 * 1024 {
                return Vec::new();
            }
            let mut buffer = vec![0u64; (bytes as usize).div_ceil(8)];
            let items = buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let status = PdhGetFormattedCounterArrayW(
                counter,
                format,
                &raw mut bytes,
                &raw mut count,
                items,
            );
            if status == PDH_MORE_DATA {
                continue;
            }
            if status != 0
                || count as usize > buffer.len() * 8 / size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>()
            {
                return Vec::new();
            }
            return std::slice::from_raw_parts(items, count as usize)
                .iter()
                .filter_map(|item| {
                    if ![PDH_CSTATUS_VALID_DATA, PDH_CSTATUS_NEW_DATA]
                        .contains(&item.FmtValue.CStatus)
                        || item.szName.is_null()
                    {
                        return None;
                    }
                    let value = item.FmtValue.Anonymous.doubleValue;
                    if !value.is_finite() || value < 0.0 {
                        return None;
                    }
                    let mut len = 0;
                    while *item.szName.add(len) != 0 {
                        len += 1;
                    }
                    Some((
                        String::from_utf16_lossy(std::slice::from_raw_parts(item.szName, len)),
                        value,
                    ))
                })
                .collect();
        }
        Vec::new()
    }
}

fn engine_usage(adapter: &str, values: &[(String, f64)]) -> Option<f64> {
    let mut engines: HashMap<&str, f64> = HashMap::new();
    for (name, value) in values {
        let Some((_, suffix)) = name.split_once(adapter) else {
            continue;
        };
        let Some(engine) = suffix.strip_prefix("_eng_") else {
            continue;
        };
        // Sum process contributions within one engine, then pick the busiest engine.
        // Summing engines would exceed 100%; averaging would understate busy GPUs.
        *engines.entry(engine).or_default() += value;
    }
    engines
        .values()
        .copied()
        .reduce(f64::max)
        .map(|v| v.clamp(0.0, 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gpu_usage_sums_processes_per_engine_and_selects_busiest_for_one_adapter() {
        let id = "luid_0x00000000_0x00000001_phys_0";
        let rows = vec![
            (format!("pid_1_{id}_eng_0_engtype_3D"), 40.0),
            (format!("pid_2_{id}_eng_0_engtype_3D"), 30.0),
            (format!("pid_2_{id}_eng_1_engtype_Copy"), 90.0),
            (
                "pid_2_luid_0x00000000_0x00000002_phys_0_eng_0_engtype_3D".into(),
                100.0,
            ),
        ];
        assert_eq!(engine_usage(id, &rows), Some(90.0));
        assert_eq!(engine_usage("missing", &rows), None);
        assert_eq!(
            engine_usage(id, &[(format!("pid_3_{id}_eng_0_engtype_3D"), 101.0)]),
            Some(100.0)
        );
    }
    #[test]
    fn native_gpu_probe_tolerates_missing_counters() {
        if let Some(mut sampler) = GpuSampler::new() {
            for device in sampler.sample() {
                assert!(device.usage.is_none_or(|v| (0.0..=100.0).contains(&v)));
            }
        }
    }
}
