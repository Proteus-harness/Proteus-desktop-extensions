//! The source's viewport owns the raw SVG fence's preferred size. Explicit
//! json-render widgets keep their caller-supplied box independently of this.

pub(super) fn dimensions(source: &str) -> (f64, f64) {
    let Some((width, height)) = intrinsic_size(source) else {
        return (640., 360.);
    };
    let scale = 1_f64.min(640. / width).min(4096. / height);
    // The public Svg schema requires each side to be at least one logical
    // pixel, including tiny drawings and exceptionally narrow viewports.
    ((width * scale).max(1.), (height * scale).max(1.))
}

fn intrinsic_size(source: &str) -> Option<(f64, f64)> {
    // Parsing only inspects this bounded source. It never opens references or
    // external entities. Native SVG validation/rendering remains with the host.
    let document = roxmltree::Document::parse(source).ok()?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" {
        return None;
    }
    let width = root.attribute("width").and_then(physical_length);
    let height = root.attribute("height").and_then(physical_length);
    let size = match (width, height) {
        (Some(width), Some(height)) => (width, height),
        (width, height) => {
            let (view_width, view_height) = view_box(root.attribute("viewBox")?)?;
            match (width, height) {
                (Some(width), None) => (width, width * (view_height / view_width)),
                (None, Some(height)) => (height * (view_width / view_height), height),
                _ => (view_width, view_height),
            }
        }
    };
    [size.0, size.1]
        .into_iter()
        .all(|side| side.is_finite() && side > 0.)
        .then_some(size)
}

fn view_box(value: &str) -> Option<(f64, f64)> {
    let mut values = value
        .split(|ch: char| ch.is_whitespace() || ch == ',')
        .filter(|part| !part.is_empty());
    let mut box_values = [0_f64; 4];
    for value in &mut box_values {
        *value = values.next()?.parse().ok()?;
    }
    (values.next().is_none()
        && box_values.into_iter().all(f64::is_finite)
        && box_values[2] > 0.
        && box_values[3] > 0.)
        .then_some((box_values[2], box_values[3]))
}

fn physical_length(value: &str) -> Option<f64> {
    let value = value.trim();
    let units = [
        ("px", 1.),
        ("in", 96.),
        ("cm", 96. / 2.54),
        ("mm", 96. / 25.4),
        ("pt", 96. / 72.),
        ("pc", 16.),
        ("Q", 96. / 101.6),
    ];
    let (number, factor) = units
        .iter()
        .find_map(|(unit, factor)| value.strip_suffix(unit).map(|number| (number, *factor)))
        .unwrap_or((value, 1.));
    let length = number.trim().parse::<f64>().ok()? * factor;
    (length.is_finite() && length > 0.).then_some(length)
}
