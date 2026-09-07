//! Squarified treemap layout — Bruls/Huizing/van Wijk, ported from
//! `packages/studio-ui/src/treemap.ts` (#3339).
//!
//! Pure geometry: values in, rectangles out, deterministic. Each run of
//! items is laid along the container's SHORT side, taking an item into the
//! run while it improves the run's worst aspect ratio — which is what
//! keeps blocks square-ish instead of slivered, and is the whole reason to
//! prefer this over a row of bars: area is comparable across the view at a
//! glance, and a bar chart of forty knots is not.

/// One laid-out block, in the same units the call passed in.
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    pub key: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Lay `items` (key and value) inside the rectangle `(x, y, w, h)`.
///
/// Zero and negative values are dropped rather than laid out at zero size:
/// a block nobody can see is a block nobody can click, and an empty
/// rectangle in a treemap reads as a gap in the data.
#[must_use]
pub fn squarify(items: &[(String, f64)], x: f32, y: f32, w: f32, h: f32) -> Vec<Tile> {
    let positive: Vec<&(String, f64)> = items.iter().filter(|(_, v)| *v > 0.).collect();
    let total: f64 = positive.iter().map(|(_, v)| *v).sum();
    if total <= 0. || w <= 0. || h <= 0. {
        return Vec::new();
    }
    // Descending, as the algorithm assumes; a stable sort keeps input
    // order for ties, so the same program always draws the same way.
    let mut sorted = positive;
    sorted.sort_by(|a, b| b.1.total_cmp(&a.1));
    let area_per_value = f64::from(w) * f64::from(h) / total;

    let mut out = Vec::with_capacity(sorted.len());
    let (mut rx, mut ry, mut rw, mut rh) = (x, y, w, h);
    let mut row: Vec<(String, f64)> = Vec::new();

    for (key, value) in sorted {
        let next = (key.clone(), value * area_per_value);
        let mut with_next = row.clone();
        with_next.push(next.clone());
        if row.is_empty() || worst(&with_next, rw, rh) <= worst(&row, rw, rh) {
            row.push(next);
        } else {
            layout_row(&row, &mut rx, &mut ry, &mut rw, &mut rh, &mut out);
            row.clear();
            row.push(next);
        }
    }
    if !row.is_empty() {
        layout_row(&row, &mut rx, &mut ry, &mut rw, &mut rh, &mut out);
    }
    out
}

/// The worst aspect ratio in a candidate run.
fn worst(row: &[(String, f64)], rw: f32, rh: f32) -> f64 {
    let side = f64::from(rw.min(rh));
    let sum: f64 = row.iter().map(|(_, a)| *a).sum();
    if sum <= 0. || side <= 0. {
        return f64::INFINITY;
    }
    let thickness = sum / side;
    row.iter().fold(0.0_f64, |max, (_, area)| {
        let length = area / thickness;
        if length <= 0. {
            return f64::INFINITY;
        }
        max.max(thickness / length).max(length / thickness)
    })
}

fn layout_row(
    row: &[(String, f64)],
    rx: &mut f32,
    ry: &mut f32,
    rw: &mut f32,
    rh: &mut f32,
    out: &mut Vec<Tile>,
) {
    let side = f64::from(rw.min(*rh));
    let sum: f64 = row.iter().map(|(_, a)| *a).sum();
    if side <= 0. || sum <= 0. {
        return;
    }
    let thickness = sum / side;
    let mut along = 0.0_f64;
    for (key, area) in row {
        let length = area / thickness;
        let tile = if *rw >= *rh {
            Tile {
                key: key.clone(),
                x: *rx,
                y: *ry + along as f32,
                w: thickness as f32,
                h: length as f32,
            }
        } else {
            Tile {
                key: key.clone(),
                x: *rx + along as f32,
                y: *ry,
                w: length as f32,
                h: thickness as f32,
            }
        };
        out.push(tile);
        along += length;
    }
    if *rw >= *rh {
        *rx += thickness as f32;
        *rw -= thickness as f32;
    } else {
        *ry += thickness as f32;
        *rh -= thickness as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::{Tile, squarify};

    fn items(values: &[(&str, f64)]) -> Vec<(String, f64)> {
        values.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
    }

    fn area(t: &Tile) -> f32 {
        t.w * t.h
    }

    #[test]
    fn every_block_gets_its_share_of_the_area() {
        let tiles = squarify(
            &items(&[("a", 6.), ("b", 3.), ("c", 1.)]),
            0.,
            0.,
            100.,
            100.,
        );
        assert_eq!(tiles.len(), 3);
        let total: f32 = tiles.iter().map(area).sum();
        assert!((total - 10_000.).abs() < 1., "the rect is filled: {total}");
        let by = |key: &str| tiles.iter().find(|t| t.key == key).map(area).unwrap();
        // 6:3:1 in area, within rounding.
        assert!(
            (by("a") / by("b") - 2.).abs() < 0.05,
            "{}",
            by("a") / by("b")
        );
        assert!((by("b") / by("c") - 3.).abs() < 0.05);
    }

    #[test]
    fn blocks_stay_inside_the_rectangle_they_were_given() {
        let tiles = squarify(
            &items(&[("a", 5.), ("b", 4.), ("c", 3.), ("d", 2.), ("e", 1.)]),
            10.,
            20.,
            200.,
            80.,
        );
        for t in &tiles {
            assert!(t.x >= 9.9 && t.y >= 19.9, "{t:?}");
            assert!(t.x + t.w <= 210.1 && t.y + t.h <= 100.1, "{t:?}");
            assert!(t.w > 0. && t.h > 0., "no invisible block: {t:?}");
        }
    }

    #[test]
    fn squarified_beats_a_naive_strip_on_aspect_ratio() {
        // The point of the algorithm: no slivers. A strip layout of these
        // would give the last block a ratio in the hundreds.
        let values: Vec<(String, f64)> =
            (1..=20).map(|i| (format!("k{i}"), f64::from(i))).collect();
        let tiles = squarify(&values, 0., 0., 400., 300.);
        let worst = tiles
            .iter()
            .map(|t| f64::from(t.w / t.h).max(f64::from(t.h / t.w)))
            .fold(0.0_f64, f64::max);
        assert!(worst < 8., "worst aspect ratio was {worst}");
    }

    #[test]
    fn nothing_to_lay_out_lays_nothing_out() {
        assert!(squarify(&[], 0., 0., 100., 100.).is_empty());
        assert!(squarify(&items(&[("a", 0.)]), 0., 0., 100., 100.).is_empty());
        // A zero-valued block is dropped, not drawn at zero size — a block
        // nobody can see is a block nobody can click.
        let tiles = squarify(
            &items(&[("a", 1.), ("b", 0.), ("c", -3.)]),
            0.,
            0.,
            10.,
            10.,
        );
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].key, "a");
        // A rectangle with no room lays nothing out either.
        assert!(squarify(&items(&[("a", 1.)]), 0., 0., 0., 100.).is_empty());
    }

    #[test]
    fn the_same_values_always_draw_the_same_way() {
        let values = items(&[("a", 4.), ("b", 4.), ("c", 4.)]);
        assert_eq!(
            squarify(&values, 0., 0., 90., 60.),
            squarify(&values, 0., 0., 90., 60.),
            "ties keep input order, so a program draws the same twice"
        );
    }
}
