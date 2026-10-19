//! Adapter identity and aggregation shared by native performance counters.
use super::sampling::Device;
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
};

#[derive(Clone, Debug)]
pub(super) struct Adapter {
    pub device: Device,
    pub instances: Vec<String>,
}
static ALIASES: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
pub(super) fn aliases() -> HashMap<String, String> {
    ALIASES.lock().unwrap().clone()
}
pub(super) fn set_aliases(aliases: HashMap<String, String>) {
    *ALIASES.lock().unwrap() = aliases;
}
pub(super) fn resolve(id: &str) -> String {
    ALIASES
        .lock()
        .unwrap()
        .get(&id.to_ascii_lowercase())
        .cloned()
        .unwrap_or_else(|| id.to_owned())
}

pub(super) fn adapters() -> Vec<Adapter> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
    };
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return Vec::new();
    };
    let mut result = Vec::<Adapter>::new();
    let mut aliases = HashMap::new();
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
        let instance = format!(
            "luid_0x{:08x}_0x{:08x}_phys_0",
            desc.AdapterLuid.HighPart as u32, desc.AdapterLuid.LowPart
        );
        let id = physical_id(desc.AdapterLuid).unwrap_or_else(|| instance.clone());
        aliases.insert(instance.clone(), id.clone());
        if let Some(existing) = result.iter_mut().find(|a| a.device.id == id) {
            if !existing.instances.contains(&instance) {
                existing.instances.push(instance);
            }
            continue;
        }
        result.push(Adapter {
            instances: vec![instance],
            device: Device {
                id,
                name: String::from_utf16_lossy(&desc.Description[..length]),
                total: desc.DedicatedVideoMemory as u64,
                rate: None,
                used: None,
                usage: None,
            },
        });
    }
    set_aliases(aliases);
    result
}

// Query the PnP instance, rather than grouping by model name: two identical
// physical cards must remain separate while virtual aliases of one card merge.
fn physical_id(luid: windows::Win32::Foundation::LUID) -> Option<String> {
    use windows::Wdk::Graphics::Direct3D::*;
    struct Handle(u32);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                let _ = D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: self.0 });
            }
        }
    }
    let mut opened = D3DKMT_OPENADAPTERFROMLUID {
        AdapterLuid: luid,
        ..Default::default()
    };
    if unsafe { D3DKMTOpenAdapterFromLuid(&raw mut opened) }.0 < 0 {
        return None;
    }
    let handle = Handle(opened.hAdapter);
    let mut buffer = [0_u16; 2048];
    let mut count = buffer.len() as u32;
    let mut key = D3DKMT_QUERY_PHYSICAL_ADAPTER_PNP_KEY {
        PhysicalAdapterIndex: 0,
        PnPKeyType: D3DKMT_PNP_KEY_HARDWARE,
        pDest: windows::core::PWSTR(buffer.as_mut_ptr()),
        pCchDest: &raw mut count,
    };
    let mut query = D3DKMT_QUERYADAPTERINFO {
        hAdapter: handle.0,
        Type: KMTQAITYPE_PHYSICALADAPTERPNPKEY,
        pPrivateDriverData: (&raw mut key).cast(),
        PrivateDriverDataSize: size_of_val(&key) as u32,
    };
    if unsafe { D3DKMTQueryAdapterInfo(&raw mut query) }.0 < 0 {
        return None;
    }
    let length = buffer.iter().position(|c| *c == 0)?;
    pnp_id(&String::from_utf16_lossy(&buffer[..length]))
}
fn pnp_id(path: &str) -> Option<String> {
    let path = path.to_ascii_lowercase();
    let (_, instance) = path.split_once("\\enum\\")?;
    let instance = instance
        .strip_suffix("\\device parameters")
        .unwrap_or(instance);
    (!instance.is_empty()).then(|| format!("pnp:{instance}"))
}

pub(super) fn engine_usage(adapter: &str, values: &[(String, f64)]) -> Option<f64> {
    let mut engines: HashMap<&str, f64> = HashMap::new();
    for (name, value) in values {
        let Some(start) = name.find("luid_").or_else(|| name.find("LUID_")) else {
            continue;
        };
        let Some(identity) = name.get(start..start + adapter.len()) else {
            continue;
        };
        if !identity.eq_ignore_ascii_case(adapter) {
            continue;
        }
        let suffix = &name[start + adapter.len()..];
        if !suffix
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("_eng_"))
        {
            continue;
        }
        let engine = &suffix[5..];
        // Sum process contributions per engine and select the busiest engine.
        // Compare the LUID without allocating a lowercase copy of every counter row.
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
    fn gpu_counter_luid_matching_ignores_hex_letter_case() {
        let id = "luid_0x00000000_0x00015f88_phys_0";
        assert_eq!(
            engine_usage(
                id,
                &[(
                    "pid_42_luid_0x00000000_0x00015F88_phys_0_eng_0_engtype_3D".into(),
                    17.0
                )]
            ),
            Some(17.0)
        );
    }
    #[test]
    fn physical_identity_survives_control_set_changes_and_keeps_identical_cards_separate() {
        let a = pnp_id(r"\REGISTRY\MACHINE\SYSTEM\ControlSet001\Enum\PCI\VEN_10DE&DEV_2702\CARD1\Device Parameters").unwrap();
        assert_eq!(
            Some(a.clone()),
            pnp_id(
                r"\registry\machine\system\controlset002\enum\pci\ven_10de&dev_2702\card1\device parameters"
            )
        );
        assert_ne!(
            Some(a.clone()),
            pnp_id(
                r"\REGISTRY\MACHINE\SYSTEM\ControlSet001\Enum\PCI\VEN_10DE&DEV_2702\CARD2\Device Parameters"
            )
        );
        set_aliases(HashMap::from([
            ("luid_old".into(), a.clone()),
            ("luid_new".into(), a.clone()),
        ]));
        assert_eq!(resolve("LUID_OLD"), a);
        assert_eq!(resolve("luid_new"), a);
        assert_eq!(resolve("missing"), "missing");
    }

    #[test]
    #[ignore = "Reads native GPU counters; run outside the restricted sandbox"]
    fn native_physical_gpu_inventory_and_readings() {
        let mut adapters = adapters();
        let mut counters = crate::counters::Counters::new(false, true).unwrap();
        counters.sample(&mut adapters);
        std::thread::sleep(std::time::Duration::from_millis(1200));
        counters.sample(&mut adapters);
        for adapter in &adapters {
            println!(
                "{} id={} aliases={} usage={:?} used={:?}",
                adapter.device.name,
                adapter.device.id,
                adapter.instances.len(),
                adapter.device.usage,
                adapter.device.used
            );
        }
        if let Ok(name) = std::env::var("LUCIDDESK_TEST_GPU") {
            let matched: Vec<_> = adapters
                .iter()
                .filter(|a| a.device.name.contains(&name))
                .collect();
            assert_eq!(
                matched.len(),
                1,
                "one physical GPU must have one settings entry"
            );
            assert!(
                matched[0].device.usage.is_some(),
                "expected GPU usage counter"
            );
            assert!(
                matched[0].device.used.is_some(),
                "expected GPU memory counter"
            );
        }
    }
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
