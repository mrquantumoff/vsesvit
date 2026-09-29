//! Motion: how long things move and how, and whether they move at all.
//!
//! Elements move through implicit transitions declared in their markup ([`implicit`]): code sets
//! where an element should be and XAML animates it there. Sizes, which implicit transitions do
//! not cover, go through a [`Tween`]. With the Windows "Animation effects" setting off, nothing
//! here animates and every change is instant; the theme transitions XAML runs on its own
//! (dialogs, flyouts, the top strip's tabs) follow that setting by themselves.

use std::time::Duration;

use windows_core::{Interface, Result};

use crate::bindings::*;

/// A tab row fading and sliding in, or fading and folding away.
pub(crate) const ROW: Duration = Duration::from_millis(170);
/// The vertical tab pane collapsing or expanding.
pub(crate) const PANE: Duration = Duration::from_millis(200);
/// A page of a multi-page dialog coming in.
pub(crate) const PAGE: Duration = Duration::from_millis(200);
/// How far content that comes in slides, in view pixels.
pub(crate) const SLIDE: f32 = 24.0;
/// Every tween decelerates into place.
const EASING: &str = r#"<CubicEase EasingMode="EaseOut"/>"#;

/// Whether the user lets apps animate (Settings > Accessibility > Visual effects).
pub(crate) fn enabled() -> bool {
    let mut on = windows_core::BOOL(1);
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION as u32,
            0,
            (&raw mut on).cast(),
            0,
        )
    };
    !read.as_bool() || on.as_bool()
}

fn timespan(duration: Duration) -> String {
    format!("0:0:{:.3}", duration.as_secs_f64())
}

/// Property elements for the element `tag` that animate changes of its opacity and translation
/// over `duration`.
pub(crate) fn implicit(tag: &str, duration: Duration) -> String {
    let t = timespan(duration);
    format!(
        r#"<{tag}.OpacityTransition><ScalarTransition Duration="{t}"/></{tag}.OpacityTransition>
<{tag}.TranslationTransition><Vector3Transition Duration="{t}"/></{tag}.TranslationTransition>"#
    )
}

/// Each time `element` (with [`implicit`] transitions) enters the tree, it moves to where it
/// rests, from wherever [`prepare_entrance`] put it.
pub(crate) fn rest_on_load(element: &FrameworkElement) -> Result<()> {
    let target = element.cast::<UIElement>()?;
    element
        .Loaded(move |_, _| {
            let _ = target.SetOpacity(1.0);
            let _ = target.SetTranslation(Vector3::default());
        })?
        .forget();
    Ok(())
}

/// Before `element` enters the tree: faded out and `dx` to the side, so it comes in from there.
pub(crate) fn prepare_entrance(element: &UIElement, dx: f32) -> Result<()> {
    if !enabled() {
        return Ok(());
    }
    element.SetOpacity(0.0)?;
    element.SetTranslation(Vector3 {
        x: dx,
        y: 0.0,
        z: 0.0,
    })
}

/// One `Double` property of one element, animated from code. Its storyboard is declared by
/// [`Tween::markup`] in the resources of the markup that holds the element.
pub(crate) struct Tween {
    board: Storyboard,
    animation: DoubleAnimation,
}

impl Tween {
    /// A storyboard named `name` that animates `property` of the element named `target`.
    pub fn markup(name: &str, target: &str, property: &str, duration: Duration) -> String {
        format!(
            r#"<Storyboard x:Name="{name}" FillBehavior="Stop">
  <DoubleAnimation x:Name="{name}Animation" Storyboard.TargetName="{target}"
                   Storyboard.TargetProperty="{property}" EnableDependentAnimation="True"
                   Duration="{t}"><DoubleAnimation.EasingFunction>{EASING}</DoubleAnimation.EasingFunction></DoubleAnimation>
</Storyboard>"#,
            t = timespan(duration)
        )
    }

    pub fn find(scope: &FrameworkElement, name: &str) -> Result<Self> {
        Ok(Self {
            board: crate::xaml::find(scope, name)?,
            animation: crate::xaml::find(scope, &format!("{name}Animation"))?,
        })
    }

    /// Animates from `from` to `to`. The caller has already set the property to `to`, which
    /// holds once the storyboard ends (or at once, with animations off).
    pub fn run(&self, from: f64, to: f64) -> Result<()> {
        self.board.Stop()?;
        if !enabled() || (from - to).abs() < 0.5 {
            return Ok(());
        }
        self.animation.SetFrom(Some(from))?;
        self.animation.SetTo(Some(to))?;
        self.board.Begin()
    }

    pub fn stop(&self) {
        let _ = self.board.Stop();
    }
}
