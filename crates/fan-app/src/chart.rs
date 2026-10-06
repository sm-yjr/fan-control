//! Native curve editor chart. Points can be dragged with the mouse; exact
//! keyboard editing remains in the system text fields beside it.
use fan_core::{Curve, CurvePoint};
use objc2::{
    define_class, msg_send, rc::Retained, runtime::AnyObject, DefinedClass, MainThreadOnly, Message,
};
use objc2_app_kit::{NSBezierPath, NSColor, NSEvent, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use std::cell::{Cell, RefCell};

const INSET: f64 = 16.;
const HANDLE: f64 = 5.;
/// A press this close to a point (in points) grabs it.
const GRAB: f64 = 14.;
pub const SPEED_MIN: f64 = -20.;
pub const SPEED_MAX: f64 = 100.;

/// Horizontal axis maximum: 0–100% for thermal demand, 0–120°C otherwise.
pub fn input_maximum(curve: &Curve) -> f64 {
    if curve.sensor_key == fan_core::THERMAL_DEMAND_KEY {
        100.
    } else {
        120.
    }
}

pub fn to_view(point: &CurvePoint, size: NSSize, maximum: f64) -> NSPoint {
    let w = (size.width - INSET * 2.).max(1.);
    let h = (size.height - INSET * 2.).max(1.);
    NSPoint::new(
        INSET + w * (point.temperature / maximum).clamp(0., 1.),
        INSET + h * ((point.speed_percent - SPEED_MIN) / (SPEED_MAX - SPEED_MIN)).clamp(0., 1.),
    )
}

/// Inverse of `to_view`, rounded to whole units for readable values.
pub fn from_view(location: NSPoint, size: NSSize, maximum: f64) -> CurvePoint {
    let w = (size.width - INSET * 2.).max(1.);
    let h = (size.height - INSET * 2.).max(1.);
    CurvePoint {
        temperature: (((location.x - INSET) / w).clamp(0., 1.) * maximum).round(),
        speed_percent: (SPEED_MIN
            + ((location.y - INSET) / h).clamp(0., 1.) * (SPEED_MAX - SPEED_MIN))
            .round(),
    }
}

pub fn nearest(curve: &Curve, location: NSPoint, size: NSSize) -> Option<usize> {
    let maximum = input_maximum(curve);
    curve
        .points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let p = to_view(point, size, maximum);
            (index, (p.x - location.x).hypot(p.y - location.y))
        })
        .filter(|(_, distance)| *distance <= GRAB)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

/// Moves one point while keeping inputs strictly increasing, so dragging can
/// never reorder or stack points on top of each other.
pub fn move_point(curve: &mut Curve, index: usize, to: CurvePoint) {
    curve
        .points
        .sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
    let Some(_) = curve.points.get(index) else {
        return;
    };
    let low = index
        .checked_sub(1)
        .and_then(|i| curve.points.get(i))
        .map_or(0., |p| p.temperature + 1.);
    let high = curve
        .points
        .get(index + 1)
        .map_or(input_maximum(curve), |p| p.temperature - 1.);
    let point = &mut curve.points[index];
    if low <= high {
        point.temperature = to.temperature.clamp(low, high);
    }
    point.speed_percent = to.speed_percent.clamp(SPEED_MIN, SPEED_MAX);
}

#[derive(Default)]
pub(crate) struct Ivars {
    curve: RefCell<Option<Curve>>,
    current: Cell<Option<f64>>,
    dragging: Cell<Option<usize>>,
    target: RefCell<Option<Retained<AnyObject>>>,
}

define_class!(
    #[unsafe(super=NSView)]
    #[thread_kind=MainThreadOnly]
    #[ivars=Ivars]
    pub(crate) struct CurvePreview;
    impl CurvePreview {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let location = self.convertPoint_fromView(event.locationInWindow(), None);
            let index = self
                .ivars()
                .curve
                .borrow()
                .as_ref()
                .and_then(|curve| nearest(curve, location, self.bounds().size));
            self.ivars().dragging.set(index);
        }
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let Some(index) = self.ivars().dragging.get() else {
                return;
            };
            let location = self.convertPoint_fromView(event.locationInWindow(), None);
            {
                let mut borrowed = self.ivars().curve.borrow_mut();
                let Some(curve) = borrowed.as_mut() else {
                    return;
                };
                let to = from_view(location, self.bounds().size, input_maximum(curve));
                move_point(curve, index, to);
            }
            self.setNeedsDisplay(true);
            if let Some(target) = self.ivars().target.borrow().as_ref() {
                let _: () = unsafe { msg_send![&**target, curveDragged: self] };
            }
        }
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            self.ivars().dragging.set(None);
        }
        #[unsafe(method(drawRect:))]
        fn draw(&self, _dirty: NSRect) {
            let size = self.bounds().size;
            let w = (size.width - INSET * 2.).max(1.);
            let h = (size.height - INSET * 2.).max(1.);
            NSColor::separatorColor().setStroke();
            let grid = NSBezierPath::bezierPath();
            grid.setLineWidth(0.5);
            for i in 0..=4 {
                let y = INSET + h * i as f64 / 4.;
                grid.moveToPoint(NSPoint::new(INSET, y));
                grid.lineToPoint(NSPoint::new(INSET + w, y));
            }
            grid.stroke();
            let borrowed = self.ivars().curve.borrow();
            let Some(curve) = borrowed.as_ref() else {
                return;
            };
            let maximum = input_maximum(curve);
            // Stop zone: below 0% the fan is handed back to the system.
            let zero = to_view(&CurvePoint { temperature: 0., speed_percent: 0. }, size, maximum).y;
            NSColor::quaternarySystemFillColor().setFill();
            NSBezierPath::fillRect(NSRect::new(NSPoint::new(INSET, INSET), NSSize::new(w, zero - INSET)));
            if let Some(current) = self.ivars().current.get() {
                let x = INSET + w * (current / maximum).clamp(0., 1.);
                let marker = NSBezierPath::bezierPath();
                marker.setLineWidth(1.);
                marker.moveToPoint(NSPoint::new(x, INSET));
                marker.lineToPoint(NSPoint::new(x, INSET + h));
                NSColor::systemOrangeColor().setStroke();
                marker.stroke();
            }
            let mut points = curve.points.clone();
            points.sort_by(|a, b| a.temperature.total_cmp(&b.temperature));
            let line = NSBezierPath::bezierPath();
            line.setLineWidth(2.);
            for (i, point) in points.iter().enumerate() {
                let p = to_view(point, size, maximum);
                if i == 0 { line.moveToPoint(p); } else { line.lineToPoint(p); }
            }
            NSColor::controlAccentColor().setStroke();
            line.stroke();
            NSColor::controlAccentColor().setFill();
            for point in &points {
                let p = to_view(point, size, maximum);
                NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                    NSPoint::new(p.x - HANDLE, p.y - HANDLE),
                    NSSize::new(HANDLE * 2., HANDLE * 2.),
                ))
                .fill();
            }
        }
    }
);

impl CurvePreview {
    pub fn new(mtm: MainThreadMarker, frame: NSRect, curve: Curve) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            curve: RefCell::new(Some(curve)),
            ..Ivars::default()
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.setToolTip(Some(&objc2_foundation::NSString::from_str(
            &crate::i18n::translate(
                "拖动圆点调整曲线；横轴为控制输入，纵轴为风扇速度。灰色区域表示交还系统。",
            ),
        )));
        this
    }
    /// Receives `curveDragged:` after each drag step.
    pub fn set_target(&self, target: &AnyObject) {
        self.ivars().target.replace(Some(target.retain()));
    }
    pub fn curve(&self) -> Option<Curve> {
        self.ivars().curve.borrow().clone()
    }
    pub fn is_dragging(&self) -> bool {
        self.ivars().dragging.get().is_some()
    }
    pub fn set_current(&self, value: Option<f64>) {
        if self.ivars().current.get() != value {
            self.ivars().current.set(value);
            self.setNeedsDisplay(true);
        }
    }
    pub fn set_curve(&self, curve: Curve) {
        if self.is_dragging() {
            return;
        }
        if self.ivars().curve.borrow().as_ref() != Some(&curve) {
            self.ivars().curve.replace(Some(curve));
            self.setNeedsDisplay(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve() -> Curve {
        Curve::balanced("test", fan_core::THERMAL_DEMAND_KEY)
    }

    #[test]
    fn view_mapping_round_trips_to_whole_units() {
        let size = NSSize::new(432., 152.);
        let point = CurvePoint {
            temperature: 45.,
            speed_percent: 12.,
        };
        assert_eq!(from_view(to_view(&point, size, 100.), size, 100.), point);
        let corner = from_view(NSPoint::new(-50., 999.), size, 100.);
        assert_eq!((corner.temperature, corner.speed_percent), (0., SPEED_MAX));
    }

    #[test]
    fn dragging_keeps_points_ordered_and_speeds_bounded() {
        let mut curve = curve();
        let before = curve.points.len();
        let next = curve.points[2].temperature;
        move_point(
            &mut curve,
            1,
            CurvePoint {
                temperature: 999.,
                speed_percent: 500.,
            },
        );
        assert_eq!(curve.points.len(), before);
        assert_eq!(curve.points[1].temperature, next - 1.);
        assert_eq!(curve.points[1].speed_percent, SPEED_MAX);
        move_point(
            &mut curve,
            1,
            CurvePoint {
                temperature: -5.,
                speed_percent: -90.,
            },
        );
        assert_eq!(
            curve.points[1].temperature,
            curve.points[0].temperature + 1.
        );
        assert_eq!(curve.points[1].speed_percent, SPEED_MIN);
        assert!(curve
            .points
            .windows(2)
            .all(|pair| pair[0].temperature < pair[1].temperature));
    }

    #[test]
    fn grabbing_needs_a_nearby_point() {
        let curve = curve();
        let size = NSSize::new(432., 152.);
        let target = to_view(&curve.points[3], size, 100.);
        assert_eq!(
            nearest(&curve, NSPoint::new(target.x + 3., target.y - 3.), size),
            Some(3)
        );
        assert_eq!(nearest(&curve, NSPoint::new(-200., -200.), size), None);
    }
}
