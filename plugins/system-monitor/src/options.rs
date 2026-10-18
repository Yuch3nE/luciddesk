use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Metric {
    Cpu,
    Memory,
    Gpu,
    Network,
    Disk,
}
impl Metric {
    pub(crate) fn label(self) -> &'static str {
        crate::i18n::text(match self {
            Self::Cpu => "monitor-cpu",
            Self::Memory => "monitor-memory",
            Self::Gpu => "monitor-gpu",
            Self::Network => "monitor-network",
            Self::Disk => "monitor-disk",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Preferences {
    pub enabled: bool,
    pub metrics: Vec<Metric>,
    pub compact: bool,
    pub paused: bool,
    pub interval: u64,
    pub network: Option<String>,
    pub disk: Option<String>,
    pub gpu: Option<String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            enabled: true,
            metrics: vec![
                Metric::Cpu,
                Metric::Memory,
                Metric::Gpu,
                Metric::Network,
                Metric::Disk,
            ],
            compact: false,
            paused: false,
            interval: 1,
            network: None,
            disk: None,
            gpu: None,
        }
    }
}
