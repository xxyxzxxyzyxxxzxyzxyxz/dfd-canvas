//! Diagram coordinates are independent of browser/client coordinates.
use crate::model::{MAX_COORDINATE, MAX_ZOOM, MIN_ZOOM, NodeLayout, Port, Viewport};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

pub fn screen_to_world(point: Point, view: Viewport) -> Point {
    Point {
        x: (point.x - view.pan_x) / view.zoom,
        y: (point.y - view.pan_y) / view.zoom,
    }
}

pub fn world_to_screen(point: Point, view: Viewport) -> Point {
    Point {
        x: point.x * view.zoom + view.pan_x,
        y: point.y * view.zoom + view.pan_y,
    }
}

pub fn port_position(rect: NodeLayout, port: Port) -> Point {
    match port {
        Port::Top => Point {
            x: rect.x + rect.width / 2.0,
            y: rect.y,
        },
        Port::Right => Point {
            x: rect.x + rect.width,
            y: rect.y + rect.height / 2.0,
        },
        Port::Bottom => Point {
            x: rect.x + rect.width / 2.0,
            y: rect.y + rect.height,
        },
        Port::Left => Point {
            x: rect.x,
            y: rect.y + rect.height / 2.0,
        },
    }
}

fn outward(port: Port) -> Point {
    match port {
        Port::Top => Point { x: 0.0, y: -1.0 },
        Port::Right => Point { x: 1.0, y: 0.0 },
        Port::Bottom => Point { x: 0.0, y: 1.0 },
        Port::Left => Point { x: -1.0, y: 0.0 },
    }
}

/// Parallel links use a perpendicular control-point offset and share fixed ports.
pub fn bezier_path(
    source: Point,
    source_port: Port,
    target: Point,
    target_port: Port,
    offset: f64,
) -> (String, Point) {
    let (c1, c2, label) = curve_points(source, source_port, target, target_port, offset);
    (
        format!(
            "M {} {} C {} {}, {} {}, {} {}",
            source.x, source.y, c1.x, c1.y, c2.x, c2.y, target.x, target.y
        ),
        label,
    )
}

/// Conservative bounds: a cubic stays inside its control hull, and its label
/// occupies a 140 × 26 rectangle centered at the curve midpoint.
pub fn bezier_bounds(
    source: Point,
    source_port: Port,
    target: Point,
    target_port: Port,
    offset: f64,
) -> NodeLayout {
    let (c1, c2, label) = curve_points(source, source_port, target, target_port, offset);
    let left = source
        .x
        .min(target.x)
        .min(c1.x)
        .min(c2.x)
        .min(label.x - 70.0);
    let right = source
        .x
        .max(target.x)
        .max(c1.x)
        .max(c2.x)
        .max(label.x + 70.0);
    let top = source
        .y
        .min(target.y)
        .min(c1.y)
        .min(c2.y)
        .min(label.y - 13.0);
    let bottom = source
        .y
        .max(target.y)
        .max(c1.y)
        .max(c2.y)
        .max(label.y + 13.0);
    NodeLayout {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

/// Return the two controls and the midpoint used by both drawing and bounds.
fn curve_points(
    source: Point,
    source_port: Port,
    target: Point,
    target_port: Port,
    offset: f64,
) -> (Point, Point, Point) {
    let dx = target.x - source.x;
    let dy = target.y - source.y;
    let distance = dx.hypot(dy);
    let reach = (distance * 0.4).clamp(45.0, 240.0);
    let normal = if distance > 0.001 {
        Point {
            x: -dy / distance,
            y: dx / distance,
        }
    } else {
        Point { x: 0.0, y: 1.0 }
    };
    let start_dir = outward(source_port);
    let end_dir = outward(target_port);
    let c1 = Point {
        x: source.x + start_dir.x * reach + normal.x * offset,
        y: source.y + start_dir.y * reach + normal.y * offset,
    };
    let c2 = Point {
        x: target.x + end_dir.x * reach + normal.x * offset,
        y: target.y + end_dir.y * reach + normal.y * offset,
    };
    let label = Point {
        x: (source.x + 3.0 * c1.x + 3.0 * c2.x + target.x) / 8.0,
        y: (source.y + 3.0 * c1.y + 3.0 * c2.y + target.y) / 8.0,
    };
    (c1, c2, label)
}

pub fn fit_view(rects: impl IntoIterator<Item = NodeLayout>, width: f64, height: f64) -> Viewport {
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    for rect in rects {
        bounds = Some(match bounds {
            None => (rect.x, rect.y, rect.x + rect.width, rect.y + rect.height),
            Some((left, top, right, bottom)) => (
                left.min(rect.x),
                top.min(rect.y),
                right.max(rect.x + rect.width),
                bottom.max(rect.y + rect.height),
            ),
        });
    }
    let Some((left, top, right, bottom)) = bounds else {
        return Viewport::default();
    };
    let center_x = (left + right) / 2.0;
    let center_y = (top + bottom) / 2.0;
    // A small diagram far from the origin must remain centerable within the
    // document's pan range; clamping pan alone can leave the diagram offscreen.
    let pan_zoom_x = (MAX_COORDINATE - width / 2.0).max(1.0) / center_x.abs().max(1.0);
    let pan_zoom_y = (MAX_COORDINATE - height / 2.0).max(1.0) / center_y.abs().max(1.0);
    let zoom = (((width - 96.0).max(1.0) / (right - left).max(1.0))
        .min((height - 96.0).max(1.0) / (bottom - top).max(1.0)))
    .min(pan_zoom_x)
    .min(pan_zoom_y)
    .clamp(MIN_ZOOM, MAX_ZOOM);
    Viewport {
        pan_x: (width / 2.0 - center_x * zoom).clamp(-MAX_COORDINATE, MAX_COORDINATE),
        pan_y: (height / 2.0 - center_y * zoom).clamp(-MAX_COORDINATE, MAX_COORDINATE),
        zoom,
    }
}
