use super::{Metric, Preferences, counters::Counters, gpu};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, Weak, mpsc},
    time::{Duration, Instant},
};
use sysinfo::{
    DiskRefreshKind, Disks, InterfaceOperationalState, MemoryRefreshKind, Networks, System,
};

// Short scheduling delays are still valid measurement intervals. Longer gaps
// restart difference counters and begin a new history segment.
const MAX_SAMPLE_GAP: Duration = Duration::from_secs(7);

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
    pub break_before: bool,
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
            break_before: false,
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
    pub frame: Arc<Frame>,
    pub history: VecDeque<Arc<Frame>>,
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
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    alive: Arc<std::sync::atomic::AtomicBool>,
    sender: mpsc::Sender<Request>,
    data: Arc<Mutex<Arc<Snapshot>>>,
}
impl Service {
    fn is_alive(&self) -> bool {
        self.alive.load(std::sync::atomic::Ordering::Acquire)
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.sender.send(Request {
            id: 0,
            options: None,
        });
        if let Some(worker) = self.worker.get_mut().unwrap().take() {
            let _ = worker.join();
        }
    }
}
static SHARED: Mutex<Weak<Service>> = Mutex::new(Weak::new());
#[cfg(feature = "test-diagnostics")]
pub(crate) fn service_alive() -> bool {
    SHARED.lock().unwrap().upgrade().is_some()
}
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(super) fn snapshot() -> Arc<Snapshot> {
    let shared = SHARED.lock().unwrap();
    let Some(service) = shared.upgrade() else {
        return Arc::new(Snapshot::default());
    };
    if service.is_alive() {
        service.data.lock().unwrap().clone()
    } else {
        Arc::new(Snapshot {
            unavailable: true,
            ..Default::default()
        })
    }
}
pub(crate) struct Client {
    id: u64,
    service: Arc<Service>,
    active: bool,
    sequence: u64,
    options: Option<Preferences>,
}
impl Client {
    pub(crate) fn new() -> Result<Self, String> {
        let mut shared = SHARED.lock().unwrap();
        let service = if let Some(service) = shared.upgrade().filter(|s| s.is_alive()) {
            service
        } else {
            let (sender, requests) = mpsc::channel();
            let data = Arc::new(Mutex::new(Arc::new(Snapshot::default())));
            let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
            let worker_alive = alive.clone();
            let service = Arc::new(Service {
                worker: Mutex::new(None),
                alive,
                sender,
                data: data.clone(),
            });
            let worker = std::thread::Builder::new()
                .name("system-monitor".into())
                .spawn(move || {
                    let _exit = WorkerAlive(worker_alive);
                    run(requests, data);
                })
                .map_err(|e| e.to_string())?;
            *service.worker.lock().unwrap() = Some(worker);
            *shared = Arc::downgrade(&service);
            service
        };
        Ok(Self {
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            service,
            active: false,
            sequence: 0,
            options: None,
        })
    }
    pub(crate) fn tick(&mut self, visible: bool, options: &Preferences) -> bool {
        if !self.service.is_alive() {
            let replacement = SHARED.lock().unwrap().upgrade().filter(|s| s.is_alive());
            if let Some(service) = replacement {
                self.service = service;
                self.options = None;
            } else {
                let changed = self.sequence != u64::MAX;
                self.sequence = u64::MAX;
                return changed;
            }
        }
        let active = visible && options.enabled && !options.paused && !options.metrics.is_empty();
        if active != self.active || self.options.as_ref() != Some(options) {
            self.active = active;
            if self.options.as_ref() != Some(options) {
                self.options = Some(options.clone());
            }
            if self
                .service
                .sender
                .send(Request {
                    id: self.id,
                    options: active.then(|| options.clone()),
                })
                .is_err()
            {
                self.service
                    .alive
                    .store(false, std::sync::atomic::Ordering::Release);
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

struct WorkerAlive(Arc<std::sync::atomic::AtomicBool>);
impl Drop for WorkerAlive {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
}

fn run(requests: mpsc::Receiver<Request>, data: Arc<Mutex<Arc<Snapshot>>>) {
    // WorkerAlive marks the service unavailable if native initialization fails.
    let Ok(mut sampler) = Sampler::new() else { return };
    let mut clients: HashMap<u64, Preferences> = HashMap::new();
    let mut last: Option<Instant> = None;
    let mut history = VecDeque::new();
    let mut sequence = 0;
    loop {
        let received = if clients.is_empty() {
            requests
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        } else {
            let wait = last.map_or(Duration::ZERO, |at| {
                sampling_interval(&clients).saturating_sub(at.elapsed())
            });
            requests.recv_timeout(wait)
        };
        match received {
            Ok(request) => {
                if request.id == 0 {
                    break;
                }
                apply_request(&mut clients, &mut last, request);
                for request in requests.try_iter() {
                    if request.id == 0 {
                        return;
                    }
                    apply_request(&mut clients, &mut last, request);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if clients.is_empty() {
            last = None;
            continue;
        }
        if last.is_some_and(|at| at.elapsed() < sampling_interval(&clients)) {
            continue;
        }
        let mut metrics = Vec::new();
        for prefs in clients.values() {
            for metric in &prefs.metrics {
                if !metrics.contains(metric) {
                    metrics.push(*metric);
                }
            }
        }
        let reset = needs_baseline(last, Instant::now());
        let mut frame = sampler.sample(&metrics, reset);
        frame.break_before = reset;
        let frame = Arc::new(frame);
        last = Some(frame.at);
        history.push_back(frame.clone());
        while history
            .front()
            .is_some_and(|f: &Arc<Frame>| frame.at.duration_since(f.at) > Duration::from_secs(60))
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
fn sampling_interval(clients: &HashMap<u64, Preferences>) -> Duration {
    Duration::from_secs(
        clients
            .values()
            .map(|p| p.interval)
            .min()
            .unwrap_or(1)
            .max(1),
    )
}
fn needs_baseline(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|at| now.saturating_duration_since(at) > MAX_SAMPLE_GAP)
}
fn apply_request(
    clients: &mut HashMap<u64, Preferences>,
    last: &mut Option<Instant>,
    request: Request,
) {
    if let Some(options) = request.options {
        clients.insert(request.id, options);
    } else {
        clients.remove(&request.id);
    }
    // Preserve even a pause/resume pair received in one queue drain.
    if clients.is_empty() {
        *last = None;
    }
}

struct Sampler {
    system: System,
    networks: Networks,
    disks: Disks,
    counters: Option<Counters>,
    counter_retry: Instant,
    adapters: Vec<gpu::Adapter>,
    previous: Vec<Metric>,
    network_bytes: HashMap<String, (u64, u64, Instant)>,
    disk_bytes: HashMap<String, (u64, u64, Instant)>,
    inventory: Instant,
}
impl Sampler {
    fn new() -> Result<Self, sysinfo::Error> {
        Ok(Self {
            system: System::new()?,
            networks: Networks::new()?,
            disks: Disks::new()?,
            counters: None,
            counter_retry: Instant::now(),
            adapters: Vec::new(),
            previous: Vec::new(),
            network_bytes: HashMap::new(),
            disk_bytes: HashMap::new(),
            inventory: Instant::now() - Duration::from_secs(30),
        })
    }
    fn sample(&mut self, metrics: &[Metric], reset: bool) -> Frame {
        if reset {
            self.previous.clear();
            self.network_bytes.clear();
            self.disk_bytes.clear();
            self.counters = None;
            self.counter_retry = Instant::now();
        }
        let mut frame = Frame::default();
        let inventory = self.inventory.elapsed() >= Duration::from_secs(15);
        let enabled = (
            metrics.contains(&Metric::Cpu),
            metrics.contains(&Metric::Gpu),
        );
        let changed = enabled
            != (
                self.previous.contains(&Metric::Cpu),
                self.previous.contains(&Metric::Gpu),
            );
        if !enabled.0 && !enabled.1 {
            self.counters = None;
            self.adapters.clear();
        } else {
            if changed
                || (Instant::now() >= self.counter_retry
                    && self.counters.as_ref().is_none_or(|c| !c.complete()))
            {
                self.counters = Counters::new(enabled.0, enabled.1);
                self.counter_retry = Instant::now() + Duration::from_secs(30);
            }
            if !enabled.1 {
                self.adapters.clear();
            }
            if enabled.1 && (changed || inventory) {
                self.adapters = gpu::adapters();
            }
            if let Some(counters) = &mut self.counters {
                frame.cpu = counters.sample(&mut self.adapters);
            } else {
                for adapter in &mut self.adapters {
                    adapter.device.usage = None;
                    adapter.device.used = None;
                }
            }
            if enabled.1 {
                frame.gpus = self.adapters.iter().map(|a| a.device.clone()).collect();
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
                let name = name.to_string_lossy().into_owned();
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
            if inventory || self.disks.is_empty() {
                self.disks
                    .refresh_specifics(true, DiskRefreshKind::everything());
            } else {
                for disk in &mut self.disks {
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
        if inventory {
            self.inventory = frame.at;
        }
        self.previous = metrics.to_vec();
        frame
    }
}

fn rate(old: (u64, u64, Instant), now: (u64, u64, Instant)) -> Option<(f64, f64)> {
    let elapsed = now.2.checked_duration_since(old.2)?.as_secs_f64();
    if elapsed <= 0.0 || elapsed > MAX_SAMPLE_GAP.as_secs_f64() {
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
    fn active_subscription_survives_ui_stalls_and_pause_starts_a_new_segment() {
        use super::*;
        let (sender, receiver) = mpsc::channel();
        let data = Arc::new(Mutex::new(Arc::new(Snapshot::default())));
        let output = data.clone();
        let worker = std::thread::spawn(move || run(receiver, output));
        let options = Preferences {
            metrics: vec![Metric::Memory],
            ..Default::default()
        };
        let subscribe = || {
            sender
                .send(Request {
                    id: 1,
                    options: Some(options.clone()),
                })
                .unwrap()
        };
        let wait_for = |sequence| {
            let deadline = Instant::now() + Duration::from_secs(8);
            loop {
                let value = data.lock().unwrap().clone();
                if value.sequence >= sequence {
                    break value;
                }
                assert!(
                    Instant::now() < deadline,
                    "sampling stopped without an explicit unsubscribe"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        subscribe();
        // No UI ticks/heartbeats for over four seconds. The old three-second
        // lease stopped this subscription before reaching sequence five.
        let before_pause = wait_for(5);
        assert!(
            before_pause
                .frame
                .at
                .duration_since(before_pause.history[0].at)
                >= Duration::from_secs(4)
        );
        assert!(before_pause.history[0].break_before);
        assert!(before_pause.history.iter().skip(1).all(|f| !f.break_before));
        assert!(before_pause.history.iter().all(|f| f.memory.is_some()));
        sender
            .send(Request {
                id: 1,
                options: None,
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let paused = data.lock().unwrap().sequence;
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(data.lock().unwrap().sequence, paused);
        subscribe();
        let resumed = wait_for(paused + 1);
        assert!(
            resumed.break_before,
            "even a short explicit pause must retain a history boundary"
        );
        assert!(resumed.history.len() > before_pause.history.len());
        sender
            .send(Request {
                id: 0,
                options: None,
            })
            .unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn short_delays_keep_baselines_but_long_gaps_and_queued_pauses_reset_them() {
        use super::*;
        let at = Instant::now();
        assert!(!needs_baseline(Some(at), at + Duration::from_millis(2500)));
        assert!(!needs_baseline(Some(at), at + MAX_SAMPLE_GAP));
        assert!(needs_baseline(
            Some(at),
            at + MAX_SAMPLE_GAP + Duration::from_millis(1)
        ));
        assert!(needs_baseline(None, at));
        let mut clients = HashMap::new();
        let mut last = Some(at);
        apply_request(
            &mut clients,
            &mut last,
            Request {
                id: 1,
                options: Some(Preferences::default()),
            },
        );
        assert_eq!(last, Some(at));
        apply_request(
            &mut clients,
            &mut last,
            Request {
                id: 1,
                options: None,
            },
        );
        apply_request(
            &mut clients,
            &mut last,
            Request {
                id: 1,
                options: Some(Preferences::default()),
            },
        );
        assert_eq!(last, None);
    }

    #[test]
    #[ignore = "Samples real devices for 35 seconds; run on the interactive machine"]
    fn native_sampling_cadence_reports_delays_and_missing_metrics() {
        use super::*;
        let mut client = Client::new().unwrap();
        let mut options = Preferences {
            interval: 5,
            ..Default::default()
        };
        client.tick(true, &options);
        let started = Instant::now();
        let mut previous = None;
        let mut sequence = 0;
        while started.elapsed() < Duration::from_secs(35) {
            let interval = if started.elapsed().as_secs() < 12 {
                5
            } else if started.elapsed().as_secs() < 22 {
                2
            } else {
                1
            };
            if options.interval != interval {
                options.interval = interval;
                client.tick(true, &options);
            }
            // Reading a snapshot does not tick/renew the subscription. This
            // also exercises twelve seconds without any UI message delivery.
            let frame = snapshot();
            if frame.sequence == sequence {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            sequence = frame.sequence;
            let gap = previous.map(|at| frame.at.duration_since(at).as_secs_f64());
            println!(
                "sequence={sequence} t={:.3}s interval={interval}s gap={gap:?} break={} publication_lag={:.3}s cpu={:?} gpu={:?} network={:?} disk={:?}",
                frame.at.saturating_duration_since(started).as_secs_f64(),
                frame.break_before,
                frame.at.elapsed().as_secs_f64(),
                frame.cpu,
                frame
                    .gpus
                    .iter()
                    .map(|d| (&d.name, d.usage))
                    .collect::<Vec<_>>(),
                frame
                    .networks
                    .iter()
                    .map(|d| d.rate.is_some())
                    .collect::<Vec<_>>(),
                frame
                    .disks
                    .iter()
                    .map(|d| d.rate.is_some())
                    .collect::<Vec<_>>(),
            );
            if previous.is_some() {
                assert!(
                    !frame.break_before,
                    "UI inactivity or an interval change split history"
                );
                assert!(frame.cpu.is_some(), "CPU baseline unexpectedly reset");
                if let Ok(name) = std::env::var("LUCIDDESK_TEST_GPU") {
                    assert!(
                        frame
                            .gpus
                            .iter()
                            .any(|d| d.name.contains(&name) && d.usage.is_some())
                    );
                }
            }
            previous = Some(frame.at);
        }
        assert!(sequence >= 20, "sampler stopped during UI inactivity");
    }

    #[test]
    fn published_and_paused_snapshots_share_frames_and_release_them_on_drop() {
        use super::*;
        let (sender, receiver) = mpsc::channel();
        let data = Arc::new(Mutex::new(Arc::new(Snapshot::default())));
        let output = data.clone();
        let worker = std::thread::spawn(move || run(receiver, output));
        sender
            .send(Request {
                id: 1,
                options: Some(Preferences {
                    metrics: vec![Metric::Memory],
                    interval: 0,
                    ..Default::default()
                }),
            })
            .unwrap();
        let wait_for = |sequence| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let snapshot = data.lock().unwrap().clone();
                if snapshot.sequence >= sequence {
                    break snapshot;
                }
                assert!(Instant::now() < deadline, "sampler did not publish");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let paused = wait_for(1);
        let current = wait_for(paused.sequence + 1);
        assert!(Arc::ptr_eq(&current.frame, current.history.back().unwrap()));
        assert!(
            current
                .history
                .iter()
                .any(|frame| Arc::ptr_eq(frame, &paused.frame))
        );
        assert!(paused.memory.is_some());
        let retained = Arc::downgrade(&paused.frame);
        sender
            .send(Request {
                id: 0,
                options: None,
            })
            .unwrap();
        worker.join().unwrap();
        drop(current);
        drop(data);
        assert!(retained.upgrade().is_some());
        drop(paused);
        assert!(retained.upgrade().is_none());
    }

    #[test]
    fn disabled_panels_and_empty_metrics_unsubscribe_from_sampling() {
        use super::*;
        let (sender, receiver) = mpsc::channel();
        let service = Arc::new(Service {
            worker: Mutex::new(None),
            alive: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            sender,
            data: Arc::new(Mutex::new(Arc::new(Snapshot::default()))),
        });
        *SHARED.lock().unwrap() = Arc::downgrade(&service);
        let mut client = Client::new().unwrap();
        client.tick(true, &Preferences::default());
        assert!(receiver.try_recv().unwrap().options.is_some());
        for options in [
            Preferences {
                enabled: false,
                ..Default::default()
            },
            Preferences {
                metrics: Vec::new(),
                ..Default::default()
            },
        ] {
            client.tick(true, &options);
            assert!(receiver.try_recv().unwrap().options.is_none());
            client.tick(true, &options);
            assert!(receiver.try_recv().is_err());
        }
    }

    #[test]
    fn disconnected_service_is_unavailable_and_not_reused() {
        use super::*;
        let (sender, receiver) = mpsc::channel();
        drop(receiver);
        let service = Arc::new(Service {
            worker: Mutex::new(None),
            alive: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            sender,
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
        let mut sampler = Sampler::new().unwrap();
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
        let mut sampler = Sampler::new().unwrap();
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
    fn native_metric_switch_closes_query_and_restarts_cpu_baseline() {
        let mut sampler = Sampler::new().unwrap();
        sampler.sample(&[Metric::Cpu], true);
        assert!(sampler.counters.as_ref().unwrap().complete());
        std::thread::sleep(Duration::from_millis(1100));
        assert!(sampler.sample(&[Metric::Cpu], false).cpu.is_some());
        let memory = sampler.sample(&[Metric::Memory], false);
        assert!(memory.cpu.is_none());
        assert!(sampler.counters.is_none());
        assert!(sampler.adapters.is_empty());
        assert!(sampler.sample(&[Metric::Cpu], false).cpu.is_none());
        std::thread::sleep(Duration::from_millis(1100));
        assert!(sampler.sample(&[Metric::Cpu], false).cpu.is_some());
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
