use super::{Metric, Preferences, gpu::GpuSampler};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, Weak, mpsc},
    time::{Duration, Instant},
};
use sysinfo::{
    DiskRefreshKind, Disks, InterfaceOperationalState, MemoryRefreshKind, Networks, System,
};

#[derive(Clone, Debug)]
pub(super) struct Device {
    pub id: String,
    pub name: String,
    pub rate: Option<(f64, f64)>,
    pub used: Option<u64>,
    pub total: u64,
    pub usage: Option<f64>,
}
#[derive(Clone, Debug)]
pub(super) struct Frame {
    pub at: Instant,
    pub cpu: Option<f64>,
    pub memory: Option<(u64, u64)>,
    pub networks: Vec<Device>,
    pub disks: Vec<Device>,
    pub gpus: Vec<Device>,
}
impl Default for Frame {
    fn default() -> Self {
        Self {
            at: Instant::now(),
            cpu: None,
            memory: None,
            networks: Vec::new(),
            disks: Vec::new(),
            gpus: Vec::new(),
        }
    }
}
#[derive(Default, Clone)]
pub(super) struct Snapshot {
    pub sequence: u64,
    pub unavailable: bool,
    pub frame: Frame,
    pub history: VecDeque<Frame>,
}
impl std::ops::Deref for Snapshot {
    type Target = Frame;
    fn deref(&self) -> &Frame {
        &self.frame
    }
}
struct Request {
    id: u64,
    options: Option<Preferences>,
}
struct Service {
    alive: Arc<std::sync::atomic::AtomicBool>,
    sender: mpsc::Sender<Request>,
    data: Arc<Mutex<Arc<Snapshot>>>,
}
impl Service {
    fn is_alive(&self) -> bool {
        self.alive.load(std::sync::atomic::Ordering::Acquire)
    }
}
static SHARED: Mutex<Weak<Service>> = Mutex::new(Weak::new());
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(super) fn snapshot() -> Arc<Snapshot> {
    let shared = SHARED.lock().unwrap();
    let Some(service) = shared.upgrade() else {
        return Arc::new(Snapshot::default());
    };
    if service.is_alive() {
        service.data.lock().unwrap().clone()
    } else {
        Arc::new(Snapshot { unavailable: true, ..Default::default() })
    }
}
pub(in crate::pane) struct Client {
    id: u64,
    service: Arc<Service>,
    active: bool,
    heartbeat: Instant,
    sequence: u64,
    options: String,
}
impl Client {
    pub(in crate::pane) fn new() -> Result<Self, String> {
        let mut shared = SHARED.lock().unwrap();
        let service = if let Some(service) = shared.upgrade().filter(|s| s.is_alive()) {
            service
        } else {
            let (sender, requests) = mpsc::channel();
            let data = Arc::new(Mutex::new(Arc::new(Snapshot::default())));
            let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
            let worker_alive = alive.clone();
            let service = Arc::new(Service {
                alive,
                sender,
                data: data.clone(),
            });
            std::thread::Builder::new()
                .name("system-monitor".into())
                .spawn(move || {
                    let _exit = WorkerAlive(worker_alive);
                    run(requests, data);
                })
                .map_err(|e| e.to_string())?;
            *shared = Arc::downgrade(&service);
            service
        };
        Ok(Self {
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            service,
            active: false,
            heartbeat: Instant::now() - Duration::from_secs(2),
            sequence: 0,
            options: String::new(),
        })
    }
    pub(in crate::pane) fn tick(&mut self, visible: bool, options: &Preferences) -> bool {
        if !self.service.is_alive() {
            let replacement = SHARED.lock().unwrap().upgrade().filter(|s| s.is_alive());
            if let Some(service) = replacement {
                self.service = service;
                self.options.clear();
            } else {
                let changed = self.sequence != u64::MAX;
                self.sequence = u64::MAX;
                return changed;
            }
        }
        let active = visible && !options.paused;
        let key = serde_json::to_string(options).unwrap_or_default();
        if active != self.active
            || key != self.options
            || (active && self.heartbeat.elapsed() >= Duration::from_secs(1))
        {
            self.active = active;
            self.options = key;
            self.heartbeat = Instant::now();
            if self.service.sender.send(Request {
                id: self.id,
                options: active.then(|| options.clone()),
            }).is_err() {
                self.service.alive.store(false, std::sync::atomic::Ordering::Release);
                self.sequence = u64::MAX;
                return true;
            }
        }
        let sequence = self.service.data.lock().unwrap().sequence;
        if active && sequence != self.sequence {
            self.sequence = sequence;
            true
        } else {
            false
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.service.sender.send(Request {
            id: self.id,
            options: None,
        });
    }
}

#[cfg(test)]
pub(in crate::pane) fn fixture(snapshot: Snapshot) -> impl Drop {
    let (sender, _receiver) = mpsc::channel();
    let service = Arc::new(Service {
        alive: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        sender,
        data: Arc::new(Mutex::new(Arc::new(snapshot))),
    });
    *SHARED.lock().unwrap() = Arc::downgrade(&service);
    service
}

struct WorkerAlive(Arc<std::sync::atomic::AtomicBool>);
impl Drop for WorkerAlive {
    fn drop(&mut self) { self.0.store(false, std::sync::atomic::Ordering::Release); }
}

fn run(requests: mpsc::Receiver<Request>, data: Arc<Mutex<Arc<Snapshot>>>) {
    let mut sampler = Sampler::new();
    let mut clients: HashMap<u64, (Preferences, Instant)> = HashMap::new();
    let mut last: Option<Instant> = None;
    let mut history = VecDeque::new();
    let mut sequence = 0;
    loop {
        let received = if clients.is_empty() {
            requests
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        } else {
            requests.recv_timeout(Duration::from_millis(250))
        };
        match received {
            Ok(request) => {
                apply_request(&mut clients, request);
                for request in requests.try_iter() {
                    apply_request(&mut clients, request);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        clients.retain(|_, (_, at)| at.elapsed() < Duration::from_secs(3));
        if clients.is_empty() {
            last = None;
            continue;
        }
        let interval = clients.values().map(|(p, _)| p.interval).min().unwrap_or(1);
        if last.is_some_and(|at| at.elapsed() < Duration::from_secs(interval)) {
            continue;
        }
        let mut metrics = Vec::new();
        for (prefs, _) in clients.values() {
            for metric in &prefs.metrics {
                if !metrics.contains(metric) {
                    metrics.push(*metric);
                }
            }
        }
        let frame = sampler.sample(&metrics, last.is_none());
        last = Some(frame.at);
        history.push_back(frame.clone());
        while history
            .front()
            .is_some_and(|f: &Frame| frame.at.duration_since(f.at) > Duration::from_secs(60))
        {
            history.pop_front();
        }
        sequence += 1;
        *data.lock().unwrap() = Arc::new(Snapshot {
            sequence,
            unavailable: false,
            frame,
            history: history.clone(),
        });
    }
}
fn apply_request(clients: &mut HashMap<u64, (Preferences, Instant)>, request: Request) {
    if let Some(options) = request.options {
        clients.insert(request.id, (options, Instant::now()));
    } else {
        clients.remove(&request.id);
    }
}

struct Sampler {
    system: System,
    networks: Networks,
    disks: Disks,
    gpu: Option<GpuSampler>,
    gpu_retry: Instant,
    previous: Vec<Metric>,
    network_bytes: HashMap<String, (u64, u64, Instant)>,
    disk_bytes: HashMap<String, (u64, u64, Instant)>,
    inventory: Instant,
}
impl Sampler {
    fn new() -> Self {
        Self {
            system: System::new(),
            networks: Networks::new(),
            disks: Disks::new(),
            gpu: None,
            gpu_retry: Instant::now(),
            previous: Vec::new(),
            network_bytes: HashMap::new(),
            disk_bytes: HashMap::new(),
            inventory: Instant::now() - Duration::from_secs(30),
        }
    }
    fn sample(&mut self, metrics: &[Metric], reset: bool) -> Frame {
        if reset {
            self.previous.clear();
            self.network_bytes.clear();
            self.disk_bytes.clear();
            self.gpu = None;
            self.gpu_retry = Instant::now();
        }
        let mut frame = Frame::default();
        let inventory = self.inventory.elapsed() >= Duration::from_secs(15);
        if metrics.contains(&Metric::Cpu) {
            self.system.refresh_cpu_usage();
            if self.previous.contains(&Metric::Cpu) && !self.system.cpus().is_empty() {
                frame.cpu = Some(f64::from(self.system.global_cpu_usage()).clamp(0.0, 100.0));
            }
        }
        if metrics.contains(&Metric::Memory) {
            self.system
                .refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
            let total = self.system.total_memory();
            if total > 0 {
                frame.memory = Some((self.system.used_memory(), total));
            }
        }
        if metrics.contains(&Metric::Network) {
            if !self.previous.contains(&Metric::Network) {
                self.network_bytes.clear();
            }
            self.networks.refresh(inventory);
            for (name, net) in &self.networks {
                if net.operational_state() != InterfaceOperationalState::Up {
                    continue;
                }
                let current = (net.total_received(), net.total_transmitted(), frame.at);
                let rate = self
                    .network_bytes
                    .insert(name.clone(), current)
                    .and_then(|old| rate(old, current));
                frame.networks.push(Device {
                    id: name.clone(),
                    name: name.clone(),
                    rate,
                    used: None,
                    total: 0,
                    usage: None,
                });
            }
            self.network_bytes
                .retain(|id, _| frame.networks.iter().any(|d| &d.id == id));
            frame.networks.sort_by(|a, b| a.id.cmp(&b.id));
        }
        if metrics.contains(&Metric::Disk) {
            if !self.previous.contains(&Metric::Disk) {
                self.disk_bytes.clear();
            }
            if inventory || self.disks.list().is_empty() {
                self.disks
                    .refresh_specifics(true, DiskRefreshKind::everything());
            } else {
                for disk in self.disks.list_mut() {
                    disk.refresh_specifics(DiskRefreshKind::nothing().with_io_usage());
                }
            }
            for disk in &self.disks {
                let id = disk.mount_point().to_string_lossy().into_owned();
                let usage = disk.usage();
                let current = (usage.total_read_bytes, usage.total_written_bytes, frame.at);
                let rate = self
                    .disk_bytes
                    .insert(id.clone(), current)
                    .and_then(|old| rate(old, current));
                // sysinfo's Windows backend does not report I/O query failures.
                // A nonzero cumulative counter is required before trusting a rate.
                let rate = rate.filter(|_| current.0 > 0 || current.1 > 0);
                frame.disks.push(Device {
                    name: id.clone(),
                    id,
                    rate,
                    used: Some(disk.total_space().saturating_sub(disk.available_space())),
                    total: disk.total_space(),
                    usage: None,
                });
            }
            self.disk_bytes
                .retain(|id, _| frame.disks.iter().any(|d| &d.id == id));
            frame.disks.sort_by(|a, b| a.id.cmp(&b.id));
        }
        if metrics.contains(&Metric::Gpu) {
            if !self.previous.contains(&Metric::Gpu) {
                self.gpu = None;
                self.gpu_retry = Instant::now();
            }
            if (self.gpu.is_none() || inventory) && Instant::now() >= self.gpu_retry {
                if let Some(gpu) = &mut self.gpu {
                    gpu.refresh_devices();
                } else {
                    self.gpu = GpuSampler::new();
                }
                self.gpu_retry = Instant::now() + Duration::from_secs(30);
            }
            if let Some(gpu) = &mut self.gpu {
                frame.gpus = gpu.sample();
            }
        } else {
            self.gpu = None;
        }
        if inventory {
            self.inventory = frame.at;
        }
        self.previous = metrics.to_vec();
        frame
    }
}

fn rate(old: (u64, u64, Instant), now: (u64, u64, Instant)) -> Option<(f64, f64)> {
    let elapsed = now.2.checked_duration_since(old.2)?.as_secs_f64();
    if elapsed <= 0.0 || elapsed > 7.0 {
        return None;
    }
    Some((
        now.0.checked_sub(old.0)? as f64 / elapsed,
        now.1.checked_sub(old.1)? as f64 / elapsed,
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn disconnected_service_is_unavailable_and_not_reused() {
        use super::*;
        let (sender, receiver) = mpsc::channel();
        drop(receiver);
        let service = Arc::new(Service {
            alive: Arc::new(std::sync::atomic::AtomicBool::new(true)), sender,
            data: Arc::new(Mutex::new(Arc::new(Snapshot::default()))),
        });
        *SHARED.lock().unwrap() = Arc::downgrade(&service);
        let mut client = Client::new().unwrap();
        assert!(client.tick(true, &Preferences::default()));
        assert!(snapshot().unavailable);
        let replacement = Client::new().unwrap();
        assert!(!Arc::ptr_eq(&replacement.service, &service));
        assert!(!snapshot().unavailable);
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        drop(WorkerAlive(alive.clone()));
        assert!(!alive.load(std::sync::atomic::Ordering::Acquire));
    }

    use super::*;
    #[test]
    fn rates_use_actual_elapsed_and_reject_counter_resets_and_gaps() {
        let at = Instant::now();
        assert_eq!(
            rate((100, 200, at), (400, 800, at + Duration::from_millis(1500))),
            Some((200.0, 400.0))
        );
        assert_eq!(
            rate((100, 200, at), (50, 800, at + Duration::from_secs(1))),
            None
        );
        assert_eq!(
            rate((100, 200, at), (400, 800, at + Duration::from_secs(20))),
            None
        );
        assert_eq!(rate((100, 200, at), (400, 800, at)), None);
    }
    #[test]
    fn sampling_never_populates_process_inventory_and_resumes_with_empty_baselines() {
        let mut sampler = Sampler::new();
        let first = sampler.sample(&[Metric::Cpu, Metric::Memory, Metric::Network], true);
        assert!(first.cpu.is_none());
        assert!(first.networks.iter().all(|d| d.rate.is_none()));
        assert!(sampler.system.processes().is_empty());
        let resumed = sampler.sample(&[Metric::Cpu, Metric::Network], true);
        assert!(resumed.cpu.is_none());
        assert!(resumed.networks.iter().all(|d| d.rate.is_none()));
    }
    #[test]
    fn native_sampling_produces_real_cpu_memory_and_device_rates() {
        let mut sampler = Sampler::new();
        let metrics = Preferences::default().metrics;
        sampler.sample(&metrics, true);
        std::thread::sleep(Duration::from_millis(1200));
        let frame = sampler.sample(&metrics, false);
        assert!(frame.cpu.is_some_and(|v| (0.0..=100.0).contains(&v)));
        assert!(
            frame
                .memory
                .is_some_and(|(used, total)| total > 0 && used <= total)
        );
        assert!(frame.networks.iter().all(|d| d.rate.is_some()));
        assert!(sampler.system.processes().is_empty());
        eprintln!(
            "CPU={:?}; RAM={:?}; networks={:?}; disks={:?}; GPUs={:?}",
            frame.cpu, frame.memory, frame.networks, frame.disks, frame.gpus
        );
    }
    #[test]
    fn clients_share_a_worker_and_hidden_clients_stop_sampling() {
        let mut first = Client::new().unwrap();
        let mut second = Client::new().unwrap();
        assert!(Arc::ptr_eq(&first.service, &second.service));
        let prefs = Preferences {
            metrics: vec![Metric::Memory],
            ..Default::default()
        };
        first.tick(true, &prefs);
        second.tick(true, &prefs);
        std::thread::sleep(Duration::from_millis(400));
        assert!(snapshot().memory.is_some());
        first.tick(false, &prefs);
        second.tick(false, &prefs);
        std::thread::sleep(Duration::from_millis(400));
        let sequence = snapshot().sequence;
        std::thread::sleep(Duration::from_millis(1100));
        assert_eq!(snapshot().sequence, sequence);
    }
}
