//! Adapter identity and aggregation shared by native performance counters.
use super::sampling::Device;
use std::collections::HashMap;

pub(super) fn adapters() -> Vec<Device> {
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

pub(super) fn engine_usage(adapter: &str, values: &[(String, f64)]) -> Option<f64> {
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
}
