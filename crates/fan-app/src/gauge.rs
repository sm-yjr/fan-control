//! Small drawn indicators: a ring for one headline percentage and a bar for
//! comparing many values. Text stays in real labels so VoiceOver reads it.
use objc2::{define_class, msg_send, rc::Retained, DefinedClass, MainThreadOnly, Message};
use objc2_app_kit::{NSBezierPath, NSColor, NSView};
use objc2_foundation::{MainThreadMarker, NSObjectProtocol, NSPoint, NSRect};
use std::cell::{Cell, RefCell};

#[derive(Default)]
pub(crate) struct Ivars {
    /// 0…1, or None when the value is unknown (only the track is drawn).
    fraction: Cell<Option<f64>>,
    color: RefCell<Option<Retained<NSColor>>>,
    thickness: Cell<f64>,
}

impl Ivars {
    fn set(&self, view: &NSView, fraction: Option<f64>, color: &NSColor) -> bool {
        let fraction = fraction.filter(|f| f.is_finite()).map(|f| f.clamp(0., 1.));
        let changed = self.fraction.get() != fraction
            || self
                .color
                .borrow()
                .as_ref()
                .is_none_or(|current| !current.isEqual(Some(color)));
        if changed {
            self.fraction.set(fraction);
            self.color.replace(Some(color.retain()));
            view.setNeedsDisplay(true);
        }
        changed
    }
}

define_class!(
    /// Circular progress ring; 0 starts at the bottom left and fills clockwise.
    #[unsafe(super=NSView)]
    #[thread_kind=MainThreadOnly]
    #[ivars=Ivars]
    pub(crate) struct Ring;
    impl Ring {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let thickness = self.ivars().thickness.get();
            let radius = (bounds.size.width.min(bounds.size.height) - thickness) / 2.;
            let center = NSPoint::new(bounds.size.width / 2., bounds.size.height / 2.);
            // A 270° arc open at the bottom, like a speedometer.
            let start = 225.;
            let sweep = 270.;
            let arc = |to: f64| {
                let path = NSBezierPath::bezierPath();
                path.appendBezierPathWithArcWithCenter_radius_startAngle_endAngle_clockwise(
                    center, radius, start, start - sweep * to, true,
                );
                path.setLineWidth(thickness);
                path.setLineCapStyle(objc2_app_kit::NSLineCapStyle::Round);
                path
            };
            NSColor::quaternaryLabelColor().setStroke();
            arc(1.).stroke();
            if let (Some(fraction), Some(color)) =
                (self.ivars().fraction.get(), self.ivars().color.borrow().as_ref())
            {
                if fraction > 0.005 {
                    color.setStroke();
                    arc(fraction).stroke();
                }
            }
        }
    }
);

impl Ring {
    pub fn new(mtm: MainThreadMarker, size: f64, thickness: f64) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            thickness: Cell::new(thickness),
            ..Ivars::default()
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.widthAnchor()
            .constraintEqualToConstant(size)
            .setActive(true);
        this.heightAnchor()
            .constraintEqualToConstant(size)
            .setActive(true);
        this
    }
    pub fn set(&self, fraction: Option<f64>, color: &NSColor) {
        self.ivars().set(self, fraction, color);
    }
}

define_class!(
    /// Rounded horizontal bar with a colored fill.
    #[unsafe(super=NSView)]
    #[thread_kind=MainThreadOnly]
    #[ivars=Ivars]
    pub(crate) struct Bar;
    impl Bar {
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let height = self.ivars().thickness.get().min(bounds.size.height);
            let y = (bounds.size.height - height) / 2.;
            let radius = height / 2.;
            let track = NSRect::new(NSPoint::new(0., y), objc2_foundation::NSSize::new(bounds.size.width, height));
            NSColor::quaternaryLabelColor().setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(track, radius, radius).fill();
            if let (Some(fraction), Some(color)) =
                (self.ivars().fraction.get(), self.ivars().color.borrow().as_ref())
            {
                if fraction > 0. {
                    let mut fill = track;
                    fill.size.width = (bounds.size.width * fraction).max(height);
                    color.setFill();
                    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(fill, radius, radius).fill();
                }
            }
        }
    }
);

impl Bar {
    pub fn new(mtm: MainThreadMarker, width: f64, thickness: f64) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            thickness: Cell::new(thickness),
            ..Ivars::default()
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.widthAnchor()
            .constraintEqualToConstant(width)
            .setActive(true);
        this.heightAnchor()
            .constraintEqualToConstant(thickness.max(8.))
            .setActive(true);
        this
    }
    pub fn set(&self, fraction: Option<f64>, color: &NSColor) {
        self.ivars().set(self, fraction, color);
    }
}

/// Color for a component temperature: calm, warm, hot, critical.
pub(crate) fn temperature_color(celsius: f64) -> Retained<NSColor> {
    match crate::presenter::temperature_level(celsius) {
        crate::presenter::Tone::Good => NSColor::systemGreenColor(),
        crate::presenter::Tone::Info => NSColor::systemYellowColor(),
        crate::presenter::Tone::Warning => NSColor::systemOrangeColor(),
        crate::presenter::Tone::Danger => NSColor::systemRedColor(),
    }
}

// A separate 180-degree tachometer; existing overview rings retain their 270-degree shape.
define_class!(
    #[unsafe(super=NSView)] #[thread_kind=MainThreadOnly] #[ivars=Ivars]
    pub(crate) struct Arc;
    impl Arc {
        #[unsafe(method(drawRect:))]
        fn draw(&self,_dirty:NSRect){
            let b=self.bounds();let t=self.ivars().thickness.get();
            let radius=(b.size.width-t)/2.;let center=NSPoint::new(b.size.width/2.,t/2.+10.);
            let path=|fraction:f64|{let p=NSBezierPath::bezierPath();p.appendBezierPathWithArcWithCenter_radius_startAngle_endAngle_clockwise(center,radius,180.,180.-180.*fraction,true);p.setLineWidth(t);p.setLineCapStyle(objc2_app_kit::NSLineCapStyle::Round);p};
            NSColor::quaternaryLabelColor().setStroke();path(1.).stroke();
            if let (Some(f),Some(c))=(self.ivars().fraction.get(),self.ivars().color.borrow().as_ref()){if f>0.005{c.setStroke();path(f).stroke();}}
        }
    }
);
impl Arc {
    pub fn new(mtm: MainThreadMarker, width: f64, thickness: f64) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            thickness: Cell::new(thickness),
            ..Ivars::default()
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.widthAnchor()
            .constraintEqualToConstant(width)
            .setActive(true);
        this.heightAnchor()
            .constraintEqualToConstant(width / 2. + 24.)
            .setActive(true);
        this
    }
    pub fn set(&self, fraction: Option<f64>, color: &NSColor) {
        self.ivars().set(self, fraction, color);
    }
}
