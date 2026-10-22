//! Crossfade background brushes without fading the content visual.
use super::{Acrylic, effects};
use windows::{
    Foundation::{TimeSpan, TypedEventHandler},
    UI::Composition::{
        CompositionBatchCompletedEventArgs, CompositionBatchTypes, CompositionBrush,
        CompositionScopedBatch,
    },
    core::{HSTRING, Result},
};

pub(super) const DURATION: TimeSpan = TimeSpan {
    Duration: 2_000_000,
}; // 200 ms

pub(super) struct Pending {
    batch: CompositionScopedBatch,
    token: i64,
}
impl Drop for Pending {
    fn drop(&mut self) {
        let _ = self.batch.RemoveCompleted(self.token);
    }
}

impl Acrylic {
    pub(super) fn animate_material(&self) -> bool {
        if !self.material_initialized.get() || !self.effects_enabled() || self.disable_backdrop {
            return false;
        }
        #[cfg(test)]
        if let Some(enabled) = self.animations_override.get() {
            return enabled;
        }
        self.ui_settings
            .as_ref()
            .is_some_and(|settings| settings.AnimationsEnabled().unwrap_or(false))
    }

    pub(super) fn replace_background(&self, brush: &CompositionBrush) -> Result<()> {
        let old = self.target_brush.borrow().clone();
        if old.as_ref() == Some(brush) {
            if !self.animate_material() {
                self.clear_transition()?;
            }
            return Ok(());
        }
        self.transition_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.transition.borrow_mut().take();
        let animate = self.animate_material();
        *self.target_brush.borrow_mut() = Some(brush.clone());
        self.material_initialized.set(true);
        if animate && let Some(old) = old {
            // Never nest transition graphs. Animation failure must not hide the material.
            match self.crossfade(&old, brush) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    crate::pane::render_debug::render_trace(format_args!(
                        "material transition unavailable: {error}"
                    ));
                    #[cfg(test)]
                    eprintln!("material transition unavailable: {error:?}");
                }
            }
        }
        self.backdrop.SetBrush(brush)
    }

    fn crossfade(&self, old: &CompositionBrush, next: &CompositionBrush) -> Result<()> {
        let compositor = &self.runtime.compositor;
        let mut factory = self.runtime.transition_factory.borrow_mut();
        if factory.is_none() {
            *factory = Some(effects::transition_factory(compositor)?);
        }
        let fade = factory.as_ref().unwrap().CreateBrush()?;
        fade.SetSourceParameter(&HSTRING::from("From"), old)?;
        fade.SetSourceParameter(&HSTRING::from("To"), next)?;
        let animation = compositor.CreateScalarKeyFrameAnimation()?;
        animation.InsertKeyFrame(0.0, 0.0)?;
        animation.InsertKeyFrame(1.0, 1.0)?;
        animation.SetDuration(DURATION)?;
        let visual = self.backdrop.clone();
        let generation = self.transition_generation.clone();
        let expected = generation.load(std::sync::atomic::Ordering::Relaxed);
        let target = next.clone();
        let batch = compositor.CreateScopedBatch(CompositionBatchTypes::Animation)?;
        let registered = batch.Completed(&TypedEventHandler::<
            windows::core::IInspectable,
            CompositionBatchCompletedEventArgs,
        >::new(move |_, _| {
            if generation.load(std::sync::atomic::Ordering::Relaxed) == expected {
                // Release the outgoing backdrop as soon as the animation finishes.
                visual.SetBrush(&target)?;
            }
            Ok(())
        }));
        let token = match registered {
            Ok(token) => token,
            Err(error) => {
                let _ = batch.End();
                return Err(error);
            }
        };
        let pending = Pending { batch, token };
        let started = self
            .backdrop
            .SetBrush(&fade)
            .and_then(|()| fade.StartAnimation(&HSTRING::from("Fade.Weight"), &animation));
        let ended = pending.batch.End();
        started?;
        ended?;
        *self.transition.borrow_mut() = Some(pending);
        Ok(())
    }

    pub(super) fn clear_transition(&self) -> Result<()> {
        self.transition_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.transition.borrow_mut().take();
        if let Some(brush) = self.target_brush.borrow().as_ref() {
            self.backdrop.SetBrush(brush)?;
        }
        Ok(())
    }
}
