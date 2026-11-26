//! Compare Repairs (#2003): a table of the features Repair Geometry would repair (the selected
//! ones, or all), each with what it would make of them under each rule ([`crate::repair::Rule`]),
//! so a rule can be chosen for the layer before it is repaired. Nothing in the layer changes.
use super::check_geometry::features;
use super::repair_geometry::repairs_of;
use super::*;
use crate::repair::{self, Apart, Kept, Outcome, Rule};

const TOOL: &str = "Compare Repairs";

/// The table's fields, in order: the feature's id; what Repair Geometry would write in its
/// repairs field under Shells Minus Holes and under Even-Odd; whether the two rules differ; the
/// ground area each would leave (m²); the area Even-Odd adds to Shells Minus Holes' result, and
/// the area it takes out of it (m²); and the parts and holes of each result.
const FIELDS: [(&str, FieldType); 12] = [
    ("feature", FieldType::Integer),
    ("shells_repairs", FieldType::Text),
    ("even_odd_repairs", FieldType::Text),
    ("differ", FieldType::Boolean),
    ("shells_m2", FieldType::Double),
    ("even_odd_m2", FieldType::Double),
    ("added_m2", FieldType::Double),
    ("removed_m2", FieldType::Double),
    ("shells_parts", FieldType::Integer),
    ("even_odd_parts", FieldType::Integer),
    ("shells_holes", FieldType::Integer),
    ("even_odd_holes", FieldType::Integer),
];

/// Writes one row for each processed feature that isn't valid as it is (see [`FIELDS`]).
pub(super) fn run(inp: &dyn Inputs, p: &Json) -> Result<ToolResult, String> {
    run_with(
        inp,
        p,
        &web_has_room,
        |g, rule| repair::repair_by(g, rule, &web_has_room),
        |a, b, moved| repair::apart(a, b, moved, &web_has_room),
    )
}

/// What one rule would make of a feature, as measured.
#[derive(Clone, Copy, Default)]
struct Side {
    /// Its ground area in m²: 0 written without geometry; `None` when written unchanged, for a
    /// geometry that isn't a polygon, or when it can't be measured.
    m2: Option<f64>,
    parts: Option<usize>,
    holes: Option<usize>,
}

/// What the comparisons found, by feature, for the notices.
#[derive(Default)]
struct Tally {
    processed: usize,
    /// Features that aren't valid as they are: one row each.
    invalid: usize,
    differ: usize,
    /// Features left unchanged under one rule or both, so whether they differ isn't known.
    unknown: usize,
    /// Features whose two results couldn't be compared: too much work, not enough memory, or the
    /// overlay failed.
    uncompared: usize,
    /// Features with a place that can't be given in longitude/latitude, so not measured.
    unplaced: usize,
    /// Sliver pieces left out of the comparison, and from how many features.
    slivers: usize,
    sliver_features: usize,
}

/// [`run`], asking `has_room` whether the browser can give the table's memory, repairing each
/// geometry with `repair` and comparing two results with `apart`.
fn run_with(
    inp: &dyn Inputs,
    p: &Json,
    has_room: &dyn Fn(u64) -> bool,
    repair: impl Fn(Option<&Geometry>, Rule) -> Outcome,
    apart: impl Fn(&Geometry, &Geometry, f64) -> Result<Apart, Kept>,
) -> Result<ToolResult, String> {
    let ds = input(inp, p, "input")?;
    if ds.geometry_type == GeometryType::None {
        return Err(format!("{TOOL} needs a layer with geometry: '{}' is a table", ds.name));
    }
    let mut rows = rows(inp, p, "input", ds);
    rows.sort_unstable();
    rows.dedup();
    // Areas are measured on the ground, each result given in longitude/latitude for it, as
    // Calculate Geometry measures; the repairs are made in the layer's own coordinates, as Repair
    // Geometry makes them.
    let to = ds.lonlat_conversion()?;
    let mut out =
        Dataset::new(out_name(p, format!("{}_compare", ds.name)), GeometryType::None, FIELDS.map(|(n, t)| Field::new(n, t)).to_vec());

    let mut tally = Tally { processed: rows.len(), ..Tally::default() };
    let no_memory = |e: TryReserveError| format!("{TOOL}: not enough memory for the comparisons of {}: {e}", features(rows.len()));
    let mut table: Vec<Vec<Value>> = Vec::new();
    crate::control::begin(rows.len());
    for (i, row) in rows.iter().enumerate() {
        crate::control::check()?;
        crate::control::reached(i);
        let Some(f) = ds.features.get(*row) else { continue };
        let g = f.geometry.as_ref();
        let shells = repair(g, Rule::ShellsMinusHoles);
        // Whether a geometry is valid doesn't depend on the rule.
        if shells == Outcome::Valid {
            continue;
        }
        // Each repair can take a while for a large feature: a cancel stops before the next.
        crate::control::check()?;
        let even_odd = repair(g, Rule::EvenOdd);
        crate::control::check()?;
        tally.invalid += 1;
        let mut unplaced = false;
        let (s, e) = (side(&shells, &to, &mut unplaced), side(&even_odd, &to, &mut unplaced));
        // Whether they differ, and the area Even-Odd adds and takes out.
        let (differ, added, removed) = match (&shells, &even_odd) {
            (Outcome::Kept(_), _) | (_, Outcome::Kept(_)) => {
                tally.unknown += 1;
                (None, None, None)
            }
            (Outcome::Repaired(a), Outcome::Repaired(b)) if a.geometry == b.geometry => (Some(false), Some(0.0), Some(0.0)),
            (Outcome::Repaired(a), Outcome::Repaired(b)) => {
                let moved = a.moved.unwrap_or(0.0) + b.moved.unwrap_or(0.0);
                match apart(&a.geometry, &b.geometry, moved) {
                    Ok(Apart { only_b, only_a, slivers }) => {
                        tally.slivers += slivers;
                        tally.sliver_features += usize::from(slivers > 0);
                        let differ = !(only_b.is_empty() && only_a.is_empty());
                        let added = area(&Geometry::Polygon(only_b), &to, &mut unplaced);
                        let removed = area(&Geometry::Polygon(only_a), &to, &mut unplaced);
                        (Some(differ), added, removed)
                    }
                    Err(_) => {
                        tally.uncompared += 1;
                        (None, None, None)
                    }
                }
            }
            // One rule or both leave it without geometry, so no area: they differ when the other
            // leaves some.
            _ => {
                let differ = repairs_of(&shells, Rule::ShellsMinusHoles).map_err(no_memory)?
                    != repairs_of(&even_odd, Rule::ShellsMinusHoles).map_err(no_memory)?;
                let gain = |a: Side, b: Side| Some((b.m2? - a.m2?).max(0.0));
                (Some(differ), gain(s, e), gain(e, s))
            }
        };
        tally.differ += usize::from(differ == Some(true));
        tally.unplaced += usize::from(unplaced);
        let float = |x: Option<f64>| x.filter(|x| x.is_finite()).map_or(Value::Null, Value::Float);
        let int = |n: Option<usize>| n.and_then(|n| i64::try_from(n).ok()).map_or(Value::Null, Value::Int);
        let values = [
            i64::try_from(f.id).map_or(Value::Null, Value::Int),
            Value::Text(repairs_of(&shells, Rule::ShellsMinusHoles).map_err(no_memory)?),
            Value::Text(repairs_of(&even_odd, Rule::EvenOdd).map_err(no_memory)?),
            differ.map_or(Value::Null, Value::Bool),
            float(s.m2),
            float(e.m2),
            float(added),
            float(removed),
            int(s.parts),
            int(e.parts),
            int(s.holes),
            int(e.holes),
        ];
        let mut attrs = Vec::new();
        attrs.try_reserve_exact(values.len()).map_err(no_memory)?;
        attrs.extend(values);
        table.try_reserve(1).map_err(no_memory)?;
        table.push(attrs);
    }

    // The table, once its memory is known to be there: in the browser, whose memory has a ceiling,
    // it is asked for first.
    let need = table
        .iter()
        .map(|attrs| {
            let text: usize = attrs.iter().map(|v| if let Value::Text(t) = v { t.len() } else { 0 }).sum();
            point_bytes(&[], FIELDS.len()).saturating_add(u64::try_from(text).unwrap_or(u64::MAX))
        })
        .fold(0u64, u64::saturating_add);
    if !table.is_empty() && !has_room(need) {
        return Err(format!(
            "{TOOL}: the comparisons of {} would need about {} MiB, more than the browser can give now: select fewer features",
            features(table.len()),
            need.div_ceil(1 << 20)
        ));
    }
    for attrs in table {
        out.push_next(None, attrs);
    }
    Ok(ToolResult { output: Output::Table(out), messages: notices(&tally), losses: Vec::new() })
}

/// What `outcome`, one rule's repair, would write, measured (see [`Side`]).
fn side(outcome: &Outcome, to: &rustgis_data::raster::Reprojection, unplaced: &mut bool) -> Side {
    match outcome {
        Outcome::Repaired(r) => match &r.geometry {
            Geometry::Polygon(polys) => Side {
                m2: area(&r.geometry, to, unplaced),
                parts: Some(polys.len()),
                holes: Some(polys.iter().map(|rings| rings.len().saturating_sub(1)).sum()),
            },
            Geometry::Line(parts) => Side { m2: None, parts: Some(parts.len()), holes: None },
            Geometry::MultiPoint(points) => Side { m2: None, parts: Some(points.len()), holes: None },
            Geometry::Point(_) => Side { m2: None, parts: Some(1), holes: None },
        },
        Outcome::Gone(_) | Outcome::GoneAfterDuplicates { .. } => Side { m2: Some(0.0), parts: Some(0), holes: Some(0) },
        Outcome::Valid | Outcome::Kept(_) => Side::default(),
    }
}

/// The ground area (m²) of the polygon geometry `g`, given in longitude/latitude by `to`: `None`,
/// noted in `unplaced`, when one of its places can't be given there or lies beyond the poles, and
/// `None` when a ring's winding is ambiguous.
fn area(g: &Geometry, to: &rustgis_data::raster::Reprojection, unplaced: &mut bool) -> Option<f64> {
    let lonlat = Dataset::converted_to_lonlat(to, g).filter(|g| {
        let mut placed = true;
        g.for_each_coord(|c| placed &= !(c.is_finite() && c.y.abs() > 90.0));
        placed
    });
    let Some(lonlat) = lonlat else {
        *unplaced = true;
        return None;
    };
    Some(crate::join::area_m2(&lonlat).abs()).filter(|a| a.is_finite())
}

/// The run's notices: the result in one line, then one per kind of feature it couldn't compare
/// fully.
fn notices(t: &Tally) -> Vec<String> {
    let mut messages = vec![if t.processed == 0 {
        format!("{TOOL}: no features to compare")
    } else if t.invalid == 0 {
        format!("{TOOL}: no repairs needed in {}", features(t.processed))
    } else {
        format!(
            "{TOOL}: {} of {} need{} repairs, and the two rules differ on {}",
            t.invalid,
            features(t.processed),
            if t.invalid == 1 { "s" } else { "" },
            t.differ
        )
    }];
    let was = |n: usize| if n == 1 { "was" } else { "were" };
    if t.unknown > 0 {
        messages.push(format!(
            "{TOOL}: {} {} left unchanged under one rule or both (too_complex or repair_failed in shells_repairs or even_odd_repairs), so whether the rules differ on {} isn't known: differ is empty",
            features(t.unknown),
            was(t.unknown),
            if t.unknown == 1 { "it" } else { "them" }
        ));
    }
    if t.uncompared > 0 {
        messages.push(format!(
            "{TOOL}: the two results of {} couldn't be compared, because comparing them needed more work or memory than the tool spends on one feature or the overlay failed: differ, added_m2 and removed_m2 are empty",
            features(t.uncompared)
        ));
    }
    if t.unplaced > 0 {
        messages.push(format!(
            "{TOOL}: {} {} a place that can't be given in longitude/latitude, so {} areas aren't measured and are empty",
            features(t.unplaced),
            if t.unplaced == 1 { "has" } else { "have" },
            if t.unplaced == 1 { "its" } else { "their" }
        ));
    }
    if t.slivers > 0 {
        messages.push(format!(
            "{TOOL}: {} {} where the two results differ by no more than the overlay's rounding {} left out of added_m2 and removed_m2 in {}, as the same edge rounded twice",
            t.slivers,
            if t.slivers == 1 { "sliver" } else { "slivers" },
            was(t.slivers),
            features(t.sliver_features)
        ));
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repair::Repaired;

    struct One(Dataset);
    impl Inputs for One {
        fn dataset(&self, name: &str) -> Option<&Dataset> {
            (name == "input").then_some(&self.0)
        }
        fn selection(&self, _: &str) -> &[usize] {
            &[]
        }
    }

    /// A layer of `n` bowties, each invalid.
    fn bowties(n: usize) -> One {
        let mut ds = Dataset::new("input", GeometryType::Polygon, vec![]);
        for _ in 0..n {
            let ring = [(0.0, 0.0), (1.0, 1.0), (1.0, 0.0), (0.0, 1.0), (0.0, 0.0)].map(|(x, y)| Coord::new(x * 0.001, y * 0.001));
            ds.push_next(Some(Geometry::Polygon(vec![vec![ring.to_vec()]])), vec![]);
        }
        One(ds)
    }
    fn p() -> Json {
        serde_json::json!({"input": "input"})
    }
    fn table(r: &ToolResult) -> &Dataset {
        let Output::Table(ds) = &r.output else { panic!("a table") };
        ds
    }
    fn real(g: Option<&Geometry>, rule: Rule) -> Outcome {
        repair::repair_by(g, rule, &|_| true)
    }
    fn real_apart(a: &Geometry, b: &Geometry, moved: f64) -> Result<Apart, Kept> {
        repair::apart(a, b, moved, &|_| true)
    }

    /// A feature left unchanged under one rule: whether the rules differ isn't known, and the run
    /// says so; the other rule's result is still measured.
    #[test]
    fn a_feature_kept_under_one_rule_is_said() {
        let repair = |g: Option<&Geometry>, rule: Rule| if rule == Rule::EvenOdd { Outcome::Kept(Kept::Work) } else { real(g, rule) };
        let r = run_with(&bowties(2), &p(), &|_| true, repair, real_apart).unwrap();
        let t = table(&r);
        assert_eq!(t.value(0, 3), &Value::Null, "differ");
        assert_eq!(t.value(0, 2), &Value::Text("too_complex".into()));
        assert!(matches!(t.value(0, 4), Value::Float(a) if *a > 0.0), "shells_m2 {:?}", t.value(0, 4));
        assert_eq!(t.value(0, 5), &Value::Null, "even_odd_m2");
        assert_eq!(
            r.messages,
            [
                "Compare Repairs: 2 of 2 features need repairs, and the two rules differ on 0",
                "Compare Repairs: 2 features were left unchanged under one rule or both (too_complex or repair_failed in shells_repairs or even_odd_repairs), so whether the rules differ on them isn't known: differ is empty",
            ]
        );
    }

    /// Two results that can't be compared leave differ and the areas between them empty, said once.
    #[test]
    fn results_that_cant_be_compared_are_said() {
        // Results that differ, so that they are compared.
        let repair = |g: Option<&Geometry>, rule: Rule| match real(g, rule) {
            Outcome::Repaired(mut r) if rule == Rule::EvenOdd => {
                if let Geometry::Polygon(polys) = &mut r.geometry {
                    polys.truncate(1);
                }
                Outcome::Repaired(r)
            }
            o => o,
        };
        let r = run_with(&bowties(1), &p(), &|_| true, repair, |_, _, _| Err(Kept::Memory)).unwrap();
        let t = table(&r);
        assert_eq!((t.value(0, 3), t.value(0, 6), t.value(0, 7)), (&Value::Null, &Value::Null, &Value::Null));
        assert_eq!(
            r.messages[1],
            "Compare Repairs: the two results of 1 feature couldn't be compared, because comparing them needed more work or memory than the tool spends on one feature or the overlay failed: differ, added_m2 and removed_m2 are empty"
        );
        // Compared, the dropped triangle is what Even-Odd takes out.
        let r = run_with(&bowties(1), &p(), &|_| true, repair, real_apart).unwrap();
        let t = table(&r);
        assert_eq!(t.value(0, 3), &Value::Bool(true));
        let (Value::Float(shells), Value::Float(even_odd), Value::Float(removed)) = (t.value(0, 4), t.value(0, 5), t.value(0, 7)) else {
            panic!("areas")
        };
        assert!((shells - even_odd - removed).abs() <= shells * 1e-9, "{shells} - {even_odd} != {removed}");
        assert_eq!(r.messages, ["Compare Repairs: 1 of 1 feature needs repairs, and the two rules differ on 1"]);
    }

    /// Without the browser's memory for the table, the run stops with a message.
    #[test]
    fn the_tables_memory_is_asked_for() {
        let e = run_with(&bowties(3), &p(), &|_| false, real, real_apart).unwrap_err();
        assert!(e.starts_with("Compare Repairs: the comparisons of 3 features would need about "), "{e}");
        // Nothing to write: nothing asked.
        assert!(run_with(&bowties(0), &p(), &|_| false, real, real_apart).is_ok());
    }

    /// Two results whose edges differ by no more than their rounding are the same: the sliver is
    /// left out and said; a real difference is kept.
    #[test]
    fn slivers_within_the_rounding_are_the_same_edge() {
        let square = |w: f64| vec![vec![[(0.0, 0.0), (w, 0.0), (w, 1.0), (0.0, 1.0), (0.0, 0.0)].map(|(x, y)| Coord::new(x, y)).to_vec()]];
        let (a, b) = (Geometry::Polygon(square(1.0)), Geometry::Polygon(square(1.0 + 1e-7)));
        let apart = repair::apart(&a, &b, 1e-6, &|_| true).unwrap();
        assert_eq!((apart.only_b.len(), apart.only_a.len(), apart.slivers), (0, 0, 1));
        let apart = repair::apart(&a, &b, 1e-8, &|_| true).unwrap();
        assert_eq!((apart.only_b.len(), apart.only_a.len(), apart.slivers), (1, 0, 0));
        // Said by the tool.
        let repair = |g: Option<&Geometry>, rule: Rule| match real(g, rule) {
            Outcome::Repaired(r) => {
                Outcome::Repaired(Repaired { geometry: if rule == Rule::EvenOdd { b.clone() } else { a.clone() }, moved: Some(1e-6), ..r })
            }
            o => o,
        };
        let r = run_with(&bowties(1), &p(), &|_| true, repair, real_apart).unwrap();
        assert_eq!(table(&r).value(0, 3), &Value::Bool(false));
        assert_eq!(
            r.messages[1],
            "Compare Repairs: 1 sliver where the two results differ by no more than the overlay's rounding was left out of added_m2 and removed_m2 in 1 feature, as the same edge rounded twice"
        );
    }

    /// Geometries that aren't both polygons, or a move that isn't a distance, aren't compared.
    #[test]
    fn only_polygons_are_compared() {
        let line = Geometry::Line(vec![vec![Coord::new(0.0, 0.0), Coord::new(1.0, 1.0)]]);
        assert_eq!(repair::apart(&line, &line, 0.0, &|_| true), Err(Kept::Failed));
        let square =
            Geometry::Polygon(vec![vec![vec![Coord::new(0.0, 0.0), Coord::new(1.0, 0.0), Coord::new(1.0, 1.0), Coord::new(0.0, 0.0)]]]);
        assert_eq!(repair::apart(&square, &square, f64::NAN, &|_| true), Err(Kept::Failed));
        assert_eq!(repair::apart(&square, &square, 0.0, &|_| false), Err(Kept::Memory));
        assert_eq!(repair::apart_with(&square, &square, 0.0, 10, &|_| true, &|_| true), Err(Kept::Work));
        let same = repair::apart(&square, &square, 0.0, &|_| true).unwrap();
        assert_eq!((same.only_a.len(), same.only_b.len()), (0, 0));
    }
}
