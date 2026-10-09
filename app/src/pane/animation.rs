use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    time::{Duration, Instant},
};

pub const MENU_DURATION: Duration = Duration::from_millis(120);
pub const HOVER_DURATION: Duration = Duration::from_millis(90);
pub const TOGGLE_DURATION: Duration = Duration::from_millis(140);
pub const FOLD_DURATION: Duration = Duration::from_millis(200);
pub const SETTINGS_DURATION: Duration = Duration::from_millis(160);
pub const PANE_SHOW_DURATION: Duration = Duration::from_millis(160);

// Weak ownership keeps COM objects on their UI thread without retaining them
// beyond the last animated window (including short-lived test apartments).
thread_local! { static ENGINE: RefCell<Weak<Engine>> = RefCell::new(Weak::new()); }
struct Engine {
    manager: windows_animation::Manager,
    library: windows_animation::TransitionLibrary,
    epoch: Instant,
    updated: Cell<Option<Instant>>,
}
impl Engine {
    fn shared(now: Instant) -> canvas_core::Result<Rc<Self>> {
        ENGINE.with(|slot| {
            if let Some(engine) = slot.borrow().upgrade() {
                return Ok(engine);
            }
            let engine = Rc::new(Self {
                manager: windows_animation::Manager::new()?,
                library: windows_animation::TransitionLibrary::new()?,
                epoch: now,
                updated: Cell::new(None),
            });
            *slot.borrow_mut() = Rc::downgrade(&engine);
            Ok(engine)
        })
    }
    fn time(&self, now: Instant) -> f64 {
        now.saturating_duration_since(self.epoch).as_secs_f64()
    }
    fn update(&self, now: Instant) -> canvas_core::Result<()> {
        if self.updated.get().is_none_or(|previous| now > previous) {
            self.manager.update(self.time(now))?;
            self.updated.set(Some(now));
        }
        Ok(())
    }
}

pub struct Motion {
    track: Option<(Rc<Engine>, windows_animation::Variable)>,
    pub to: f32,
    started: Instant,
    duration: Duration,
}
impl Motion {
    pub fn settled(value: f32, now: Instant) -> Self {
        Self {
            track: None,
            to: value,
            started: now,
            duration: Duration::ZERO,
        }
    }
    fn transition(
        from: f32,
        to: f32,
        now: Instant,
        duration: Duration,
    ) -> canvas_core::Result<Self> {
        if duration.is_zero() || from == to {
            return Ok(Self::settled(to, now));
        }
        let engine = Engine::shared(now)?;
        engine.update(now)?;
        let variable = engine.manager.create_variable(f64::from(from))?;
        let transition = engine.library.accelerate_decelerate(
            duration.as_secs_f64(),
            f64::from(to),
            0.0,
            1.0,
        )?;
        engine
            .manager
            .schedule_transition(&variable, &transition, engine.time(now))?;
        Ok(Self {
            track: Some((engine, variable)),
            to,
            started: now,
            duration,
        })
    }
    pub fn sample(&self, now: Instant) -> canvas_core::Result<f32> {
        let Some((engine, value)) = &self.track else {
            return Ok(self.to);
        };
        engine.update(now)?;
        if now.saturating_duration_since(self.started) >= self.duration {
            return Ok(self.to);
        }
        Ok(value.value()? as f32)
    }
    pub fn retarget(&mut self, to: f32, now: Instant, animate: bool) -> f32 {
        self.retarget_with_duration(to, now, animate, TOGGLE_DURATION)
    }
    pub fn retarget_with_duration(
        &mut self,
        to: f32,
        now: Instant,
        animate: bool,
        duration: Duration,
    ) -> f32 {
        let result = (|| -> canvas_core::Result<f32> {
            if !animate {
                *self = Self::settled(to, now);
            } else if self.to != to {
                let from = self.sample(now)?;
                *self = Self::transition(from, to, now, duration)?;
                return Ok(from);
            }
            let value = self.sample(now)?;
            if now.saturating_duration_since(self.started) >= self.duration {
                *self = Self::settled(to, now);
            }
            Ok(value)
        })();
        match result {
            Ok(value) => value,
            Err(error) => {
                luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Warn, "pane.animation", &format!("Animation unavailable: {error}"));
                *self = Self::settled(to, now);
                to
            }
        }
    }
}

pub struct Fade {
    motion: Motion,
}
impl Fade {
    pub fn new(duration: Duration) -> canvas_core::Result<Self> {
        Ok(Self {
            motion: Motion::transition(0.0, 1.0, Instant::now(), duration)?,
        })
    }
    pub fn sample(&self, elapsed: Duration) -> canvas_core::Result<f32> {
        self.motion
            .sample(self.motion.started + elapsed)
            .map(|v| v.clamp(0.0, 1.0))
    }
}

pub struct Fold {
    pub to: f32,
    pub to_reveal: f32,
    from: f32,
    from_reveal: f32,
    progress: Motion,
}
impl Fold {
    pub fn new(
        from: f32,
        to: f32,
        from_reveal: f32,
        to_reveal: f32,
        started: Instant,
        duration: Duration,
    ) -> Self {
        let progress = Motion::transition(0.0, 1.0, started, duration).unwrap_or_else(|error| {
            luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Warn, "pane.animation", &format!("Fold animation unavailable: {error}"));
            Motion::settled(1.0, started)
        });
        Self {
            from,
            to,
            from_reveal,
            to_reveal,
            progress,
        }
    }
    pub fn sample(&self, now: Instant) -> (f32, f32, bool) {
        let eased = self.progress.sample(now).unwrap_or(1.0).clamp(0.0, 1.0);
        (
            self.from + (self.to - self.from) * eased,
            self.from_reveal + (self.to_reveal - self.from_reveal) * eased,
            eased >= 1.0,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn motions_share_clock_and_release_engine_when_settled() {
        let _sta = crate::pane::test_support::apartment();
        let now = Instant::now();
        let mut a = Motion::settled(0.0, now);
        let mut b = Motion::settled(1.0, now);
        a.retarget(1.0, now, true);
        b.retarget(0.0, now, true);
        assert!(Rc::ptr_eq(
            &a.track.as_ref().unwrap().0,
            &b.track.as_ref().unwrap().0
        ));
        let midpoint = now + Duration::from_millis(70);
        let av = a.sample(midpoint).unwrap();
        let bv = b.sample(midpoint).unwrap();
        assert!((av + bv - 1.0).abs() < 0.001);
        let late = now + Duration::from_secs(1);
        assert_eq!(a.retarget(1.0, late, true), 1.0);
        assert_eq!(b.retarget(0.0, late, true), 0.0);
        assert!(a.track.is_none() && b.track.is_none());
        ENGINE.with(|engine| assert!(engine.borrow().upgrade().is_none()));
    }

    #[test]
    fn animation_manager_fade_handles_midpoint_delays_and_disabled_animation() {
        let _sta = crate::pane::test_support::apartment();
        let fade = Fade::new(Duration::from_millis(120)).unwrap();
        assert_eq!(fade.sample(Duration::ZERO).unwrap(), 0.0);
        assert!(fade.sample(Duration::from_millis(60)).unwrap() > 0.5);
        assert_eq!(fade.sample(Duration::from_secs(1)).unwrap(), 1.0);
        let instant = Fade::new(Duration::ZERO).unwrap();
        assert_eq!(instant.sample(Duration::ZERO).unwrap(), 1.0);
    }
    #[test]
    fn interrupted_fold_reverses_without_a_position_jump() {
        let _sta = crate::pane::test_support::apartment();
        let started = Instant::now();
        let duration = Duration::from_millis(200);
        let fold = Fold::new(400.0, 38.0, 1.0, 0.0, started, duration);
        let now = started + Duration::from_millis(80);
        let (height, reveal, done) = fold.sample(now);
        assert!(!done);
        assert!(height > 38.0 && height < 400.0);
        let reverse = Fold::new(height, 400.0, reveal, 1.0, now, duration);
        assert_eq!(reverse.sample(now), (height, reveal, false));
        assert_eq!(reverse.sample(now + duration), (400.0, 1.0, true));
        assert_eq!(fold.sample(started + duration), (38.0, 0.0, true));
    }
}
