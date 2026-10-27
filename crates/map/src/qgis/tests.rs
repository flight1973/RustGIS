//! A small QGIS project and style, written by hand in the shapes QGIS writes them.

use super::*;
use crate::layout::TextAlign;
use crate::symbology::{Glyph, LabelPlacement, LineCap, LineMark, Marker, Placements};

const PROJECT: &str = r#"<!DOCTYPE qgis PUBLIC 'http://mrcc.com/qgis.dtd' 'SYSTEM'>
<qgis projectname="Towns" version="3.42.0-Münster">
  <title>Towns</title>
  <projectCrs><spatialrefsys><authid>EPSG:2056</authid></spatialrefsys></projectCrs>
  <layer-tree-group>
    <layer-tree-layer id="towns_1" checked="Qt::Checked" name="Towns"/>
    <layer-tree-group name="Base" checked="Qt::Unchecked">
      <layer-tree-layer id="roads_1" checked="Qt::Checked" name="Roads"/>
    </layer-tree-group>
    <layer-tree-layer id="land_1" checked="Qt::Checked" name="Land"/>
  </layer-tree-group>
  <mapcanvas name="theMapCanvas">
    <extent><xmin>2690000</xmin><ymin>1250000</ymin><xmax>2700000</xmax><ymax>1260000</ymax></extent>
  </mapcanvas>
  <projectlayers>
    <maplayer type="vector" hasScaleBasedVisibilityFlag="1" minScale="100000" maxScale="1000" labelsEnabled="1">
      <id>land_1</id>
      <datasource>./data/base.gpkg|layername=land|subset="kind" &lt;&gt; 'sea'</datasource>
      <layername>Land</layername>
      <srs><spatialrefsys><authid>EPSG:2056</authid></spatialrefsys></srs>
      <provider encoding="UTF-8">ogr</provider>
      <renderer-v2 type="graduatedSymbol" attr="pop" referencescale="25000">
        <ranges>
          <range lower="0" upper="100" symbol="0" label="few" render="true"/>
          <range lower="100" upper="1000" symbol="1" label="many" render="true"/>
        </ranges>
        <symbols>
          <symbol type="fill" name="0" alpha="1"><layer class="SimpleFill" enabled="1" pass="0">
            <Option type="Map"><Option name="color" type="QString" value="200,220,180,255,rgb:0.78,0.86,0.7,1"/><Option name="style" type="QString" value="solid"/>
            <Option name="outline_color" type="QString" value="0,0,0,255"/><Option name="outline_width" type="QString" value="0.5"/><Option name="outline_width_unit" type="QString" value="MM"/></Option>
          </layer></symbol>
          <symbol type="fill" name="1" alpha="0.5"><layer class="LinePatternFill" enabled="1">
            <Option type="Map"><Option name="distance" value="2"/></Option>
            <symbol type="line" name="@1@0" alpha="1"><layer class="SimpleLine" enabled="1"><prop k="line_color" v="0,0,255,255"/><prop k="line_width" v="0.2"/></layer></symbol>
          </layer></symbol>
        </symbols>
      </renderer-v2>
      <layerOpacity>0.75</layerOpacity>
      <blendMode>13</blendMode>
      <labeling type="simple">
        <settings><text-style fieldName="name" fontSize="12" fontSizeUnit="Point" textColor="20,40,60,255" namedStyle="Bold Italic" fontFamily="Liberation Sans Narrow">
          <text-buffer bufferDraw="1"/></text-style><placement placement="1" quadOffset="2"/></settings>
      </labeling>
    </maplayer>
    <maplayer type="vector" labelsEnabled="1">
      <id>roads_1</id>
      <datasource>/abs/roads.shp</datasource>
      <layername>Roads</layername>
      <provider>ogr</provider>
      <renderer-v2 type="categorizedSymbol" attr="&quot;class&quot;" symbollevels="1">
        <categories>
          <category value="motorway" symbol="0" label="Motorway" render="true"/>
          <category value="track" symbol="1" label="track" render="false"/>
          <category value="" symbol="2" label="other" render="true"/>
        </categories>
        <symbols>
          <symbol type="line" name="0" alpha="1">
            <layer class="SimpleLine" enabled="1" pass="0"><prop k="line_color" v="90,40,20,255"/><prop k="line_width" v="1.2"/><prop k="capstyle" v="flat"/></layer>
            <layer class="SimpleLine" enabled="1" pass="3"><prop k="line_color" v="240,140,40,255"/><prop k="line_width" v="0.8"/><prop k="use_custom_dash" v="1"/></layer>
          </symbol>
          <symbol type="line" name="1" alpha="1"><layer class="SimpleLine" enabled="1"><prop k="line_color" v="0,0,0,255"/></layer></symbol>
          <symbol type="line" name="2" alpha="1"><layer class="MarkerLine" enabled="1"/></symbol>
        </symbols>
      </renderer-v2>
      <labeling type="rule-based">
        <rules key="root">
          <rule key="a" filter="&quot;class&quot; = 'motorway'" description="Motorways">
            <settings><text-style fieldName="ref" fontSize="9"/><placement placement="3"/></settings>
          </rule>
          <rule key="p" filter="&quot;lanes&quot; &gt; 1">
            <rule key="b" filter="ELSE" description="Rest" scalemaxdenom="50000">
              <settings><text-style fieldName="name" fontSize="7"/><placement placement="2"/></settings>
            </rule>
            <rule key="c" filter="&quot;kind&quot; = 'x'"><settings><text-style fieldName="name" fontSize="7"/></settings></rule>
            <rule key="off" filter="upper(&quot;kind&quot;) = 'X'" description="Switched off" active="0"><settings><text-style fieldName="name"/></settings></rule>
          </rule>
          <rule key="d" filter="length(&quot;name&quot;) &gt; 3"><settings><text-style fieldName="name"/></settings></rule>
        </rules>
      </labeling>
    </maplayer>
    <maplayer type="vector">
      <id>towns_1</id>
      <datasource>type=xyz&amp;url=https://tile.example.org/%7Bz%7D/%7Bx%7D/%7By%7D.png&amp;zmax=19</datasource>
      <layername>Towns</layername>
      <provider>wms</provider>
      <renderer-v2 type="RuleRenderer">
        <rules key="r">
          <rule key="1" filter="&quot;rank&quot; IN (1, 2)" symbol="0"/>
          <rule key="2" filter="ELSE" symbol="1"/>
        </rules>
        <symbols>
          <symbol type="marker" name="0" alpha="1"><layer class="SimpleMarker" enabled="1"><prop k="name" v="star"/><prop k="color" v="200,0,0,255"/><prop k="size" v="4"/></layer></symbol>
          <symbol type="marker" name="1" alpha="1"><layer class="FontMarker" enabled="1"><prop k="color" v="0,0,0,255"/><prop k="chr" v="A"/></layer></symbol>
        </symbols>
      </renderer-v2>
    </maplayer>
  </projectlayers>
</qgis>"#;

#[test]
fn a_project_reads_into_layers_in_tree_order_with_their_styles() {
    let mut report = Report::default();
    let p = read_project(PROJECT, "/maps", &mut report).unwrap();
    assert_eq!(p.title, "Towns");
    assert_eq!(p.crs.as_deref(), Some("EPSG:2056"));
    assert_eq!(p.extent, Some([2_690_000.0, 1_250_000.0, 2_700_000.0, 1_260_000.0]));
    let names: Vec<&str> = p.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Towns", "Roads", "Land"], "top first");
    let visible: Vec<bool> = p.layers.iter().map(|l| l.visible).collect();
    assert_eq!(visible, [true, false, true], "Roads is in an unchecked group");

    // Land: a GeoPackage table, filtered; graduated; opacity, blending, scales, reference scale.
    let land = &p.layers[2];
    assert_eq!(
        land.source,
        Source::File {
            path: "/maps/data/base.gpkg".into(),
            layer: Some("land".into()),
            subset: Some("\"kind\" <> 'sea'".into()),
            ignored: Vec::new()
        }
    );
    assert_eq!(land.definition_query.as_deref(), Some("\"kind\" <> 'sea'"));
    assert_eq!(land.crs.as_deref(), Some("EPSG:2056"));
    let s = &land.style;
    assert_eq!((s.transparency, s.blend), (Some(25.0), Some(Blend::Multiply)));
    let (min, max) = (s.min_scale.flatten().unwrap(), s.max_scale.flatten().unwrap());
    assert!((min - 100_000.0 * QGIS_SCALE).abs() < 1e-6 && (max - 1000.0 * QGIS_SCALE).abs() < 1e-6);
    assert!((s.reference_scale.unwrap() - 25_000.0 * QGIS_SCALE).abs() < 1e-6);
    let Some(Renderer::Graduated { field, breaks, symbols, min, class_labels, .. }) = &s.renderer else { panic!("{:?}", s.renderer) };
    assert_eq!((field.as_str(), breaks.as_slice(), *min), ("pop", &[100.0, 1000.0][..], 0.0));
    assert_eq!(symbols[0].fill, Rgba::rgb(200, 220, 180));
    assert!((symbols[0].stroke_width - 0.5 * 96.0 / 25.4).abs() < 1e-4, "0.5 mm in points");
    // The pattern fill: a tint of its line colour, at the symbol's alpha too.
    assert_eq!((symbols[1].fill.b(), symbols[1].fill.a()), (255, 45));
    assert_eq!(class_labels.get(&1).map(String::as_str), Some("many"));
    let labels = s.labels.as_ref().unwrap();
    let d = &labels.default;
    assert!(labels.enabled && labels.classes.is_empty());
    assert_eq!((d.field.as_str(), d.size, d.bold, d.italic, d.halo), ("name", 12.0, true, true, true));
    assert_eq!((d.color, d.placement), (Rgba::rgb(20, 40, 60), LabelPlacement::AboveRight));
    assert_eq!(d.font.as_deref(), Some("Liberation Sans Narrow"), "QGIS's font family");

    // Roads: categories with stacked strokes in levels, a hidden one and the rest.
    let roads = &p.layers[1];
    assert_eq!(roads.source, Source::File { path: "/abs/roads.shp".into(), layer: None, subset: None, ignored: Vec::new() });
    let s = &roads.style;
    assert_eq!((s.stroke_levels, s.line_strokes.as_deref()), (Some(true), Some(&[][..])));
    let Some(Renderer::Unique { field, classes, class_strokes, class_labels, other, .. }) = &s.renderer else { panic!() };
    assert_eq!(field, "class");
    assert_eq!(classes.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), ["motorway", "track"]);
    assert_eq!(classes[1].1.stroke.a(), 0, "not rendered: hidden");
    assert_eq!(other.stroke.a(), 0, "a marker line only: nothing RustGIS draws");
    let motorway = &class_strokes["motorway"];
    assert_eq!(motorway.len(), 2);
    assert_eq!((motorway[0].cap, motorway[0].level, motorway[1].level), (LineCap::Butt, Some(0), Some(3)));
    assert_eq!(class_labels.get("motorway").map(String::as_str), Some("Motorway"));
    // Labels: a class per rule; the else rule takes what its sibling doesn't, under its parent.
    let l = s.labels.as_ref().unwrap();
    assert!(!l.default.visible);
    let by = |n: &str| l.classes.iter().find(|c| c.name == n).unwrap();
    assert_eq!((by("Motorways").where_.as_deref(), by("Motorways").placement), (Some("\"class\" = 'motorway'"), LabelPlacement::Curved));
    let rest = by("Rest");
    assert_eq!(rest.where_.as_deref(), Some("(\"lanes\" > 1) AND (NOT ((\"kind\" = 'x')))"));
    assert_eq!(rest.placement, LabelPlacement::Parallel);
    assert!((rest.min_scale.unwrap() - 50_000.0 * QGIS_SCALE).abs() < 1e-6);
    assert_eq!(l.classes.len(), 3, "the function filter's rule is left out");

    // Towns: XYZ tiles as the source; rules on one field read as categories.
    let towns = &p.layers[0];
    assert_eq!(
        towns.source,
        Source::Xyz { url: "https://tile.example.org/{z}/{x}/{y}.png".into(), zmin: None, zmax: Some(19), vector: false }
    );
    let Some(Renderer::Unique { field, classes, other, .. }) = &towns.style.renderer else { panic!() };
    assert_eq!((field.as_str(), classes.len()), ("rank", 2));
    assert_eq!((classes[0].1.marker, classes[0].1.fill), (Marker::Star, Rgba::rgb(200, 0, 0)));
    // A font marker: its character (of no named family: the label font's).
    assert_eq!((other.marker, other.fill), (Marker::Char(Glyph::new('A', "")), Rgba::rgb(0, 0, 0)));

    // What didn't carry over is said, once for each thing.
    let said =
        |layer: &str, kind: NoteKind, part: &str| report.notes.iter().any(|n| n.layer == layer && n.kind == kind && n.text.contains(part));
    // Its second class's line pattern fill: hatch lines of its blue line, 2 mm apart, at QGIS's
    // 45°, half see-through as the symbol is; read, not noted.
    assert!(!said("Land", NoteKind::Approximated, "line pattern"));
    let Some(Renderer::Graduated { class_fills, .. }) = &land.style.renderer else { panic!() };
    let hatch = class_fills.get(&1).and_then(|f| f.first()).copied().unwrap();
    assert_eq!(hatch.color, Some(Rgba::new(0, 0, 255, 128)));
    let crate::fills::FillPattern::Hatch { angle, spacing, .. } = hatch.pattern else { panic!("{hatch:?}") };
    assert!(angle == 45.0 && (spacing - 2.0 * 96.0 / 25.4).abs() < 1e-3, "{hatch:?}");
    assert!(!class_fills.contains_key(&0), "a plain colour is the symbol's fill, as before");
    assert!(said("Land", NoteKind::Approximated, "above the last"), "values outside the ranges are noted");
    // A QGIS font family is the label class's font, drawn in it where it's installed.
    assert!(!said("Land", NoteKind::Approximated, "Liberation Sans Narrow"), "no longer approximated");
    assert!(said("Roads", NoteKind::Approximated, "dashed"));
    // A marker line without the symbol it places draws nothing, in QGIS too: nothing to say.
    assert!(!said("Roads", NoteKind::Skipped, "markers"));
    assert!(said("Roads", NoteKind::Skipped, "length("));
    assert!(!report.notes.iter().any(|n| n.layer == "Towns" && n.text.contains("font markers")), "drawn as characters: {:?}", report.notes);
    // A switched-off label rule is never drawn, so its filter isn't noted.
    assert!(!report.notes.iter().any(|n| n.text.contains("Switched off")), "{:?}", report.notes);
}

#[test]
fn a_style_file_reads_in_either_settings_format() {
    // QGIS before 3.26 writes a symbol layer's settings as <prop k v>, after as <Option>.
    let qml = |settings: &str| {
        format!(
            r#"<!DOCTYPE qgis><qgis version="3.10"><renderer-v2 type="singleSymbol"><symbols><symbol type="line" name="0" alpha="1"><layer class="SimpleLine" enabled="1">{settings}</layer></symbol></symbols></renderer-v2></qgis>"#
        )
    };
    for settings in [
        r#"<prop k="line_color" v="10,20,30,255"/><prop k="line_width" v="2"/><prop k="line_width_unit" v="Pixel"/>"#,
        r#"<Option type="Map"><Option name="line_color" value="10,20,30,255"/><Option name="line_width" value="2"/><Option name="line_width_unit" value="Pixel"/></Option>"#,
    ] {
        let mut report = Report::default();
        let s = read_style(&qml(settings), "Rivers", &mut report).unwrap();
        let Some(Renderer::Single(sym)) = s.renderer else { panic!() };
        assert_eq!((sym.stroke, sym.stroke_width), (Rgba::rgb(10, 20, 30), 2.0));
        assert_eq!(s.line_strokes.as_deref(), Some(&[][..]), "one plain line: no stack");
        assert!(report.notes.is_empty(), "{:?}", report.notes);
    }
    // Applied to a layer: its parts only.
    let mut layer = crate::Layer::new(1, rustgis_data::Dataset::new("r", rustgis_data::GeometryType::Polyline, vec![]));
    layer.transparency = 40.0;
    let s = read_style(&qml(r#"<prop k="line_color" v="1,2,3,255"/>"#), "r", &mut Report::default()).unwrap();
    s.apply(&mut layer);
    assert_eq!((layer.transparency, matches!(layer.renderer, Renderer::Single(_))), (40.0, true));
}

#[test]
fn hostile_files_are_refused_or_bounded() {
    let mut r = Report::default();
    assert!(read_project("not xml", "", &mut r).is_err());
    assert!(read_project("<other/>", "", &mut r).is_err());
    assert!(read_style("<qgis><renderer-v2 type=\"x\"", "", &mut r).is_err());
    // An entity expansion bomb: no DTD is read.
    let bomb = r#"<!DOCTYPE qgis [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;">]><qgis><title>&b;</title></qgis>"#;
    assert!(read_project(bomb, "", &mut r).is_err());
    // Rules nested past any sensible depth, and nonsense values: read without a crash.
    let deep = format!(
        r#"<qgis><renderer-v2 type="RuleRenderer" referencescale="-5"><rules>{}{}</rules><symbols/></renderer-v2><layerOpacity>NaN</layerOpacity></qgis>"#,
        "<rule filter=\"a = 1\">".repeat(200),
        "</rule>".repeat(200)
    );
    let s = read_style(&deep, "x", &mut r).unwrap();
    assert_eq!((s.reference_scale, s.transparency), (None, None));
    // The same note many times is kept once, counted.
    let mut r = Report::default();
    for _ in 0..10_000 {
        r.skipped("a", "the same");
    }
    assert_eq!((r.notes.len(), r.notes[0].count), (1, 10_000));
    // Many different notes stop at the cap; the ones kept still count their repeats, each in its
    // own place (layer, kind and text all tell notes apart).
    for i in 0..(MAX_NOTES + 500) {
        r.approximated("a", format!("note {i}"));
    }
    assert_eq!(r.notes.len(), MAX_NOTES);
    r.approximated("a", "note 7");
    r.approximated("b", "note 7");
    r.skipped("a", "note 7");
    let at = r.notes.iter().position(|n| n.text == "note 7").unwrap();
    assert_eq!((r.notes[at].count, r.notes[at].kind), (2, NoteKind::Approximated));
    assert!(!r.notes.iter().any(|n| n.layer == "b"), "past the cap, a new note isn't kept");
}

#[test]
fn paths_on_a_drive_are_told_apart_from_absolute_ones() {
    for p in ["C:roads.gpkg", "d:data\\x.shp", "C:"] {
        assert!(is_drive_relative(p) && !is_relative(p), "{p}");
    }
    for p in ["C:/gis/roads.gpkg", "C:\\gis\\roads.gpkg", "/srv/x.shp", "https://x.org/a.geojson"] {
        assert!(!is_drive_relative(p) && !is_relative(p), "{p}");
    }
    assert!(is_relative("./data/x.shp") && is_relative("data/x.shp") && !is_drive_relative("data/x.shp"));
}

#[test]
fn merged_categories_and_dashes_carry_over() {
    // QGIS 4 writes a category of several values as <val>s; an empty value is all the others.
    let qml = r#"<qgis version="4.2.3"><renderer-v2 type="categorizedSymbol" attr="kind">
      <categories>
        <category value="a" symbol="0" render="true"/>
        <category label="c or d" symbol="1" render="true"><val type="string" value="c"/><val type="string" value="d"/></category>
        <category value="" symbol="2" label="other" render="true"/>
      </categories>
      <symbols>
        <symbol type="line" name="0" alpha="1"><layer class="SimpleLine" enabled="1"><Option type="Map">
          <Option name="line_color" value="10,0,0,255"/><Option name="line_width" value="1"/><Option name="capstyle" value="flat"/>
          <Option name="use_custom_dash" value="1"/><Option name="customdash" value="2.5;0.5;0"/><Option name="customdash_unit" value="MM"/></Option></layer></symbol>
        <symbol type="line" name="1" alpha="1"><layer class="SimpleLine" enabled="1"><Option type="Map">
          <Option name="line_color" value="20,0,0,255"/><Option name="line_width" value="3"/><Option name="line_width_unit" value="Pixel"/><Option name="line_style" value="dot"/></Option></layer></symbol>
        <symbol type="line" name="2" alpha="1"><layer class="SimpleLine" enabled="1"><Option type="Map"><Option name="line_color" value="30,0,0,255"/></Option></layer></symbol>
      </symbols></renderer-v2></qgis>"#;
    let mut report = Report::default();
    let s = read_style(qml, "Paths", &mut report).unwrap();
    let Some(Renderer::Unique { classes, class_strokes, class_labels, other, .. }) = &s.renderer else { panic!("{:?}", s.renderer) };
    let values: Vec<&str> = classes.iter().map(|c| c.0.as_str()).collect();
    assert_eq!(values, ["a", "c", "d"], "both values of the merged category, not \"all others\"");
    assert_eq!((classes[1].1.stroke, classes[2].1.stroke), (Rgba::rgb(20, 0, 0), Rgba::rgb(20, 0, 0)));
    assert_eq!(other.stroke, Rgba::rgb(30, 0, 0));
    assert_eq!(class_labels.get("c").map(String::as_str), Some("c or d"));
    // A custom dash in millimetres (its odd trailing step of nothing dropped); Qt's dots in widths.
    let mm = 96.0 / 25.4;
    let dash = class_strokes["a"][0].dash.unwrap();
    assert_eq!(dash.steps().len(), 2);
    assert!((dash.steps()[0] - 2.5 * mm).abs() < 1e-3 && (dash.steps()[1] - 0.5 * mm).abs() < 1e-3, "{:?}", dash.steps());
    assert_eq!(class_strokes["c"][0].dash.unwrap().steps(), &[3.0, 6.0]);
    assert!(!report.notes.iter().any(|n| n.text.contains("dash")), "{:?}", report.notes);
}

/// Pattern fills as stacked fills: a simple fill's hatched brushes as Qt draws them, line and point
/// pattern fills in their units, several fills bottom first; one plain colour stays the symbol's
/// fill alone, as before; a class's fills go with its class.
#[test]
fn pattern_fills_carry_over_as_stacked_fills() {
    use crate::fills::{FillLayer, FillPattern};
    let qml = |layers: &str| {
        format!(
            r#"<qgis version="4.2.3"><renderer-v2 type="singleSymbol"><symbols><symbol type="fill" name="0" alpha="1">{layers}</symbol></symbols></renderer-v2></qgis>"#
        )
    };
    let read = |layers: &str| {
        let mut report = Report::default();
        let s = read_style(&qml(layers), "Fields", &mut report).unwrap();
        (s.fill_layers.unwrap_or_default(), s.renderer, report)
    };
    let simple = |style: &str| {
        format!(
            r#"<layer class="SimpleFill" enabled="1"><Option type="Map"><Option name="color" value="255,0,0,255"/><Option name="style" value="{style}"/><Option name="outline_style" value="no"/></Option></layer>"#
        )
    };
    // One colour: the symbol's fill, no stack, as before.
    let (fills, renderer, _) = read(&simple("solid"));
    assert!(fills.is_empty());
    assert!(matches!(renderer, Some(Renderer::Single(s)) if s.fill == Rgba::rgb(255, 0, 0)));
    // Qt's hatched brushes: 8 points apart, the diagonals 8 along a row; b_diagonal rises.
    let angles = |style: &str| -> Vec<(f32, f32)> {
        read(&simple(style))
            .0
            .iter()
            .map(|f| match f.pattern {
                FillPattern::Hatch { angle, spacing, .. } => (angle, (spacing * 100.0).round() / 100.0),
                _ => (-1.0, 0.0),
            })
            .collect()
    };
    assert_eq!(angles("horizontal"), [(0.0, 8.0)]);
    assert_eq!(angles("vertical"), [(90.0, 8.0)]);
    assert_eq!(angles("b_diagonal"), [(45.0, 5.66)]);
    assert_eq!(angles("f_diagonal"), [(135.0, 5.66)]);
    assert_eq!(angles("cross"), [(0.0, 8.0), (90.0, 8.0)]);
    assert_eq!(angles("diagonal_x"), [(45.0, 5.66), (135.0, 5.66)]);
    let (fills, _, report) = read(&simple("dense4"));
    assert!(fills.is_empty(), "a dense dot brush is a tint, one colour: {fills:?}");
    assert!(report.notes.iter().any(|n| n.text.contains("dense dots")), "{:?}", report.notes);
    // SMV25's point pattern: blue dots 0.7 mm apart, the marker its sub-symbol's, over a fill.
    let dots = r#"<layer class="PointPatternFill" enabled="1"><Option type="Map"><Option name="distance_x" value="0.7"/><Option name="distance_x_unit" value="MM"/>
        <Option name="distance_y" value="0.7"/><Option name="distance_y_unit" value="MM"/><Option name="displacement_x" value="0.35"/><Option name="displacement_x_unit" value="MM"/></Option>
        <symbol type="marker" name="@0@1" alpha="1"><layer class="SimpleMarker" enabled="1"><Option type="Map"><Option name="name" value="circle"/><Option name="color" value="0,0,255,255"/>
        <Option name="size" value="0.2"/><Option name="size_unit" value="MM"/><Option name="outline_style" value="no"/></Option></layer></symbol></layer>"#;
    let (fills, renderer, report) = read(&format!("{}{dots}", simple("solid")));
    assert_eq!(fills.len(), 2, "{fills:?}");
    assert_eq!(fills[0], FillLayer { color: Some(Rgba::rgb(255, 0, 0)), pattern: FillPattern::Solid }, "bottom first");
    let mm = |v: f32| v * 96.0 / 25.4;
    let FillPattern::Dots { size, spacing, displacement, outline, .. } = fills[1].pattern else { panic!("{fills:?}") };
    assert_eq!(fills[1].color, Some(Rgba::rgb(0, 0, 255)));
    assert!((size - mm(0.2)).abs() < 1e-3 && (spacing[0] - mm(0.7)).abs() < 1e-3 && (displacement[0] - mm(0.35)).abs() < 1e-3, "{fills:?}");
    assert_eq!(outline, None);
    assert!(!report.notes.iter().any(|n| n.text.contains("point pattern")), "read, not noted: {:?}", report.notes);
    assert!(matches!(renderer, Some(Renderer::Single(s)) if s.fill == Rgba::rgb(0, 0, 255).with_alpha(s.fill.a())), "{renderer:?}");
    // A line pattern in its units, with its line's colour and width.
    let lines = r#"<layer class="LinePatternFill" enabled="1"><Option type="Map"><Option name="lineangle" value="30"/><Option name="distance" value="1"/><Option name="distance_unit" value="MM"/>
        <Option name="offset" value="0.5"/><Option name="offset_unit" value="MM"/></Option>
        <symbol type="line" name="@0@0" alpha="1"><layer class="SimpleLine" enabled="1"><Option type="Map"><Option name="line_color" value="0,128,0,255"/><Option name="line_width" value="0.3"/></Option></layer></symbol></layer>"#;
    let (fills, ..) = read(lines);
    assert_eq!(fills.len(), 1, "a pattern alone is a stack of one");
    let FillPattern::Hatch { angle, spacing, width, offset } = fills[0].pattern else { panic!("{fills:?}") };
    assert_eq!(fills[0].color, Some(Rgba::rgb(0, 128, 0)));
    assert!(
        angle == 30.0 && (spacing - mm(1.0)).abs() < 1e-3 && (width - mm(0.3)).abs() < 1e-3 && (offset - mm(0.5)).abs() < 1e-3,
        "{fills:?}"
    );
    // Two colours stacked: both, the top one over the bottom.
    let half = r#"<layer class="SimpleFill" enabled="1"><Option type="Map"><Option name="color" value="0,0,0,64"/><Option name="style" value="solid"/></Option></layer>"#;
    let (fills, ..) = read(&format!("{}{half}", simple("solid")));
    assert_eq!(fills.iter().map(|f| f.color).collect::<Vec<_>>(), [Some(Rgba::rgb(255, 0, 0)), Some(Rgba::new(0, 0, 0, 64))]);
    // In classes: a categorized layer's class with a hatch has fills of its own.
    let cat = format!(
        r#"<qgis version="4.2.3"><renderer-v2 type="categorizedSymbol" attr="kind"><categories><category value="a" symbol="0" render="true"/><category value="b" symbol="1" render="true"/></categories>
        <symbols><symbol type="fill" name="0" alpha="1">{}</symbol><symbol type="fill" name="1" alpha="1">{}</symbol></symbols></renderer-v2></qgis>"#,
        simple("solid"),
        simple("cross")
    );
    let mut report = Report::default();
    let s = read_style(&cat, "Fields", &mut report).unwrap();
    let Some(Renderer::Unique { class_fills, .. }) = &s.renderer else { panic!("{:?}", s.renderer) };
    assert_eq!(class_fills.keys().collect::<Vec<_>>(), ["b"]);
    assert_eq!(class_fills["b"].len(), 2);
    assert_eq!(s.fill_layers, Some(Vec::new()), "the layer's own: none");
}

#[test]
fn a_labels_buffer_and_letter_spacing_carry_over_in_their_units() {
    let qml = |text: &str| {
        format!(
            r#"<qgis version="4.2.3"><renderer-v2 type="singleSymbol"><symbols><symbol type="line" name="0" alpha="1"><layer class="SimpleLine" enabled="1"/></symbol></symbols></renderer-v2>
            <labeling type="simple"><settings>{text}</settings></labeling></qgis>"#
        )
    };
    let read = |text: &str| {
        let mut report = Report::default();
        let s = read_style(&qml(text), "Water", &mut report).unwrap();
        (s.labels.unwrap().default, report)
    };
    // SMV25's: 0.5 mm of white, letters 0.5 pt apart.
    let (c, report) = read(
        r#"<text-style fieldName="name" fontSize="9" fontSizeUnit="Point" fontLetterSpacing="0.5">
        <text-buffer bufferDraw="1" bufferSize="0.5" bufferSizeUnits="MM" bufferColor="255,255,255,255,rgb:1,1,1,1" bufferOpacity="1" bufferJoinStyle="128"/></text-style>"#,
    );
    assert!(c.halo && c.halo_color == Some(Rgba::rgb(255, 255, 255)) && c.spacing == 0.5, "{c:?}");
    assert!((c.halo_width.unwrap() - 0.5 * 72.0 / 25.4).abs() < 1e-4, "{:?}", c.halo_width);
    assert!(report.notes.is_empty(), "{:?}", report.notes);
    // Half see-through yellow, in points; spacing in the size's millimetres; corners not round.
    let (c, report) = read(
        r#"<text-style fieldName="name" fontSize="3" fontSizeUnit="MM" fontLetterSpacing="1">
        <text-buffer bufferDraw="1" bufferSize="2" bufferSizeUnits="Point" bufferColor="255,238,0,255" bufferOpacity="0.5" bufferJoinStyle="64" bufferBlendMode="13"/></text-style>"#,
    );
    assert_eq!((c.halo_color, c.halo_width), (Some(Rgba::new(255, 238, 0, 128)), Some(2.0)));
    assert!((c.spacing - 72.0 / 25.4).abs() < 1e-4, "{}", c.spacing);
    assert!(report.notes.iter().any(|n| n.text.contains("drawn round")), "{:?}", report.notes);
    assert!(report.notes.iter().any(|n| n.text.contains("blended")), "{:?}", report.notes);
    // A buffer that says nothing: QGIS's own, 1 mm of white; one switched off: no halo at all.
    let (c, _) = read(r#"<text-style fieldName="name"><text-buffer bufferDraw="1"/></text-style>"#);
    assert_eq!(c.halo_color, Some(Rgba::rgb(255, 255, 255)));
    assert!((c.halo_width.unwrap() - 72.0 / 25.4).abs() < 1e-4);
    let (c, _) = read(r#"<text-style fieldName="name" fontLetterSpacing="junk"><text-buffer bufferDraw="0" bufferSize="3"/></text-style>"#);
    assert_eq!((c.halo, c.halo_color, c.halo_width, c.spacing), (false, None, None, 0.0));
    // Crafted sizes stay within RustGIS's ranges.
    let (c, _) =
        read(r#"<text-style fieldName="name" fontLetterSpacing="1e30"><text-buffer bufferDraw="1" bufferSize="1e30"/></text-style>"#);
    assert_eq!((c.halo_width, c.spacing), (Some(10.0), 50.0));
}

#[test]
fn a_labels_background_and_offset_carry_over_in_their_units() {
    let qml = |settings: &str| {
        format!(
            r#"<qgis version="4.2.3"><renderer-v2 type="singleSymbol"><symbols><symbol type="marker" name="0" alpha="1"><layer class="SimpleMarker" enabled="1"/></symbol></symbols></renderer-v2>
            <labeling type="simple"><settings>{settings}</settings></labeling></qgis>"#
        )
    };
    let read = |settings: &str| {
        let mut report = Report::default();
        let s = read_style(&qml(settings), "Roads", &mut report).unwrap();
        (s.labels.unwrap().default, report)
    };
    let mm = |v: f32| v * 72.0 / 25.4;
    let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
    // SMV25's road numbers: a white box with a dark outline from its fill symbol (a marker symbol
    // beside it is for marker backgrounds), padded in millimetres, its corners rounded.
    let (c, report) = read(
        r#"<text-style fieldName="nr"><background shapeDraw="1" shapeType="0" shapeSizeType="0" shapeSizeX="0.4304" shapeSizeY="0.0988" shapeSizeUnit="MM"
            shapeRadiiX="0.25" shapeRadiiY="0.25" shapeRadiiUnit="MM" shapeFillColor="255,255,255,255" shapeBorderColor="38,38,38,255" shapeBorderWidth="0.0988"
            shapeBorderWidthUnit="MM" shapeOffsetX="0" shapeOffsetY="0" shapeRotation="0" shapeOpacity="1" shapeJoinStyle="64" shapeBlendMode="0">
            <symbol type="marker" name="markerSymbol" alpha="1"><layer class="SimpleMarker" enabled="1"><Option type="Map"><Option name="color" value="232,113,141,255"/></Option></layer></symbol>
            <symbol type="fill" name="fillSymbol" alpha="1"><layer class="SimpleFill" enabled="1"><Option type="Map"><Option name="color" value="250,250,250,255,rgb:1,1,1,1"/>
              <Option name="outline_color" value="38,38,38,255"/><Option name="outline_style" value="solid"/><Option name="outline_width" value="0.1"/>
              <Option name="outline_width_unit" value="MM"/><Option name="style" value="solid"/></Option></layer></symbol>
          </background></text-style><placement placement="2"/>"#,
    );
    let b = c.background.unwrap();
    assert_eq!((b.fill, b.stroke), (Some(Rgba::rgb(250, 250, 250)), Some(Rgba::rgb(38, 38, 38))));
    assert!(
        near(b.stroke_width, mm(0.1)) && near(b.padding[0], mm(0.4304)) && near(b.padding[1], mm(0.0988)) && near(b.radius, mm(0.25)),
        "{b:?}"
    );
    assert_eq!(c.offset, [0.0; 2]);
    assert!(report.notes.is_empty(), "{:?}", report.notes);
    // Without a fill symbol (older files): its colours, half see-through; no outline width, no
    // outline; padding in points; an ellipse drawn as a rectangle, and what else it can't do noted.
    let (c, report) = read(
        r#"<text-style fieldName="nr"><background shapeDraw="1" shapeType="2" shapeSizeX="2" shapeSizeY="1" shapeSizeUnit="Point" shapeFillColor="0,0,255,255"
            shapeBorderColor="0,0,0,255" shapeBorderWidth="0" shapeOpacity="0.5" shapeOffsetX="1" shapeRotation="30" shapeBlendMode="6"/></text-style>"#,
    );
    let b = c.background.unwrap();
    assert_eq!((b.fill, b.stroke, b.padding, b.radius), (Some(Rgba::new(0, 0, 255, 128)), None, [2.0, 1.0], 0.0));
    for part in ["drawn as rectangles", "moved off their text", "turned on their own", "blended"] {
        assert!(report.notes.iter().any(|n| n.text.contains(part)), "{part}: {:?}", report.notes);
    }
    // Corners rounded by a share of its size (QGIS's `Percentage`): square, and said so.
    let (c, report) = read(
        r#"<text-style fieldName="nr"><background shapeDraw="1" shapeRadiiX="10" shapeRadiiY="10" shapeRadiiUnit="Percentage"/></text-style>"#,
    );
    assert_eq!(c.background.map(|b| b.radius), Some(0.0));
    assert!(report.notes.iter().any(|n| n.text.contains("drawn square")), "{:?}", report.notes);
    assert!(!report.notes.iter().any(|n| n.text.contains("map units")), "{:?}", report.notes);
    // A fixed size, a picture, one switched off, a symbol filled otherwise.
    let (c, report) =
        read(r#"<text-style fieldName="nr"><background shapeDraw="1" shapeSizeType="1" shapeSizeX="20" shapeSizeY="10"/></text-style>"#);
    assert_eq!(c.background.map(|b| b.padding), Some([0.0; 2]));
    assert!(report.notes.iter().any(|n| n.text.contains("size of their own")), "{:?}", report.notes);
    let (c, report) = read(r#"<text-style fieldName="nr"><background shapeDraw="1" shapeType="4" shapeSVGFile="x.svg"/></text-style>"#);
    assert!(c.background.is_none() && report.notes.iter().any(|n| n.text.contains("SVG pictures")), "{:?}", report.notes);
    let (c, _) = read(r#"<text-style fieldName="nr"><background shapeDraw="0" shapeType="0"/></text-style>"#);
    assert!(c.background.is_none());
    let (c, report) = read(
        r#"<text-style fieldName="nr"><background shapeDraw="1" shapeFillColor="1,2,3,255"><symbol type="fill" name="f" alpha="1"><layer class="GradientFill" enabled="1"/></symbol></background></text-style>"#,
    );
    assert_eq!(c.background.and_then(|b| b.fill), Some(Rgba::rgb(1, 2, 3)));
    assert!(report.notes.iter().any(|n| n.text.contains("patterns or gradients")), "{:?}", report.notes);
    // Crafted sizes stay within RustGIS's ranges.
    let (c, _) = read(
        r#"<text-style fieldName="nr"><background shapeDraw="1" shapeSizeX="1e30" shapeSizeY="-5" shapeBorderWidth="1e30" shapeRadiiX="1e30" shapeRadiiY="1e30"/></text-style>"#,
    );
    let b = c.background.unwrap();
    assert_eq!((b.padding, b.stroke_width, b.radius), ([50.0, 0.0], 10.0, 50.0));

    // An offset from the point, in its units (down is positive, as QGIS has it); the distance
    // QGIS doesn't use there isn't noted.
    let (c, report) =
        read(r#"<text-style fieldName="nr"/><placement placement="1" quadOffset="2" xOffset="2" yOffset="-1" offsetUnits="MM" dist="3"/>"#);
    assert_eq!(c.placement, LabelPlacement::AboveRight);
    assert!(near(c.offset[0], mm(2.0)) && near(c.offset[1], mm(-1.0)), "{:?}", c.offset);
    assert!(report.notes.is_empty(), "{:?}", report.notes);
    let (c, _) = read(r#"<text-style fieldName="nr"/><placement placement="1" xOffset="1e30" yOffset="4" offsetUnits="Point"/>"#);
    assert_eq!(c.offset, [200.0, 4.0]);
    // An offset QGIS doesn't use (a label along a line) isn't read.
    let (c, _) = read(r#"<text-style fieldName="nr"/><placement placement="2" xOffset="2" yOffset="2"/>"#);
    assert_eq!(c.offset, [0.0; 2]);
    // A style doesn't say what its layer holds: a distance round its features is noted.
    let (c, report) = read(r#"<text-style fieldName="nr"/><placement placement="0" dist="1.5" distUnits="MM"/>"#);
    assert_eq!(c.offset, [0.0; 2]);
    assert!(report.notes.iter().any(|n| n.text.contains("label distances")), "{:?}", report.notes);

    // In a project, a point layer's distance round its points: the label (above them) that far up,
    // noted, as QGIS puts it out diagonally.
    let qgs = |geometry: &str| {
        format!(
            r#"<qgis version="4.2.3"><projectlayers><maplayer type="vector" geometry="{geometry}" labelsEnabled="1"><id>p</id><datasource>/d/p.shp</datasource><layername>P</layername><provider>ogr</provider>
            <labeling type="simple"><settings><text-style fieldName="nr"/><placement placement="6" dist="1.5" distUnits="MM"/></settings></labeling></maplayer></projectlayers></qgis>"#
        )
    };
    let mut report = Report::default();
    let p = read_project(&qgs("Point"), "", &mut report).unwrap();
    let c = &p.layers[0].style.labels.as_ref().unwrap().default;
    assert!(c.offset[0] == 0.0 && near(c.offset[1], -mm(1.5)), "{:?}", c.offset);
    assert!(report.notes.iter().any(|n| n.text == "label distances round points, moved straight up"), "{:?}", report.notes);
    let mut report = Report::default();
    let p = read_project(&qgs("Polygon"), "", &mut report).unwrap();
    assert_eq!(p.layers[0].style.labels.as_ref().unwrap().default.offset, [0.0; 2]);
    assert!(report.notes.iter().any(|n| n.text.contains("label distances")), "{:?}", report.notes);
}

#[test]
fn a_labels_wrapping_and_repeat_carry_over() {
    let read = |settings: &str| {
        let qml = format!(
            r#"<qgis version="4.2.3"><renderer-v2 type="singleSymbol"><symbols><symbol type="line" name="0" alpha="1"><layer class="SimpleLine" enabled="1"/></symbol></symbols></renderer-v2>
            <labeling type="simple"><settings><text-style fieldName="name"/>{settings}</settings></labeling></qgis>"#
        );
        let mut report = Report::default();
        let s = read_style(&qml, "Roads", &mut report).unwrap();
        (s.labels.unwrap().default, report)
    };
    // The corpus's: a space as the wrap character, ten characters a line, centred lines.
    let (c, report) = read(
        r#"<text-format wrapChar=" " autoWrapLength="10" useMaxLineLengthForAutoWrap="1" multilineAlign="1"/><placement placement="2"/>"#,
    );
    assert_eq!((c.wrap_char.as_deref(), c.wrap_length, c.line_align), (Some(" "), 10, TextAlign::Center));
    assert_eq!(c.wrap("Ober Winterthur"), "Ober\nWinterthur");
    assert!(report.notes.is_empty(), "{:?}", report.notes);
    // Lines following the placement: right-aligned left of the feature, left-aligned right of it.
    let follow =
        |quad: &str| read(&format!(r#"<text-format multilineAlign="3"/><placement placement="1" quadOffset="{quad}"/>"#)).0.line_align;
    assert_eq!((follow("3"), follow("5"), follow("4")), (TextAlign::Right, TextAlign::Left, TextAlign::Center));
    // Without a `<text-format>` too, as QGIS defaults to following the placement.
    assert_eq!(read(r#"<placement placement="1" quadOffset="3"/>"#).0.line_align, TextAlign::Right);
    // A shortest line length, and justified lines: as near as RustGIS draws them, and noted.
    let (c, report) = read(r#"<text-format autoWrapLength="1e9" useMaxLineLengthForAutoWrap="0" multilineAlign="4"/>"#);
    assert_eq!((c.wrap_length, c.line_align), (crate::symbology::MAX_WRAP_LENGTH, TextAlign::Center));
    for part in ["shortest line length", "justified"] {
        assert!(report.notes.iter().any(|n| n.text.contains(part)), "{part}: {:?}", report.notes);
    }
    // A repeat along curved labels, in millimetres (the corpus's 30 mm); none for labels off lines.
    let (c, _) = read(r#"<placement placement="3" repeatDistance="30" repeatDistanceUnits="MM"/>"#);
    assert!(c.repeat.is_some_and(|r| (r - 30.0 * 72.0 / 25.4).abs() < 1e-3), "{:?}", c.repeat);
    let (c, report) = read(r#"<placement placement="1" repeatDistance="30"/>"#);
    assert_eq!(c.repeat, None);
    assert!(report.notes.is_empty(), "QGIS repeats no label off a line: {:?}", report.notes);
    // Within RustGIS's range; one of no number is none.
    let (c, _) = read(r#"<placement placement="2" repeatDistance="0.001" repeatDistanceUnits="Point"/>"#);
    assert_eq!(c.repeat, Some(10.0));
    let (c, _) = read(r#"<placement placement="2" repeatDistance="lots"/>"#);
    assert_eq!(c.repeat, None);
    // A unit saved before QGIS 3, as a number: 0 points, else millimetres.
    let (c, _) = read(r#"<placement placement="2" repeatDistance="30" repeatDistanceUnit="0"/>"#);
    assert_eq!(c.repeat, Some(30.0));
    let (c, _) = read(r#"<placement placement="2" repeatDistance="30" repeatDistanceUnit="1"/>"#);
    assert!(c.repeat.is_some_and(|r| (r - 30.0 * 72.0 / 25.4).abs() < 1e-3), "{:?}", c.repeat);
    // Round polygons RustGIS labels once, and says so.
    let (c, report) = read(r#"<placement placement="7" repeatDistance="30"/>"#);
    assert_eq!(c.repeat, None);
    assert!(report.notes.iter().any(|n| n.text.contains("repeated round polygons")), "{:?}", report.notes);
    let qgs = r#"<qgis version="4.2.3"><projectlayers><maplayer type="vector" geometry="Polygon" labelsEnabled="1"><id>p</id><datasource>/d/p.shp</datasource><layername>P</layername><provider>ogr</provider>
        <labeling type="simple"><settings><text-style fieldName="nr"/><placement placement="2" repeatDistance="30"/></settings></labeling></maplayer></projectlayers></qgis>"#;
    let mut report = Report::default();
    let p = read_project(qgs, "", &mut report).unwrap();
    assert_eq!(p.layers[0].style.labels.as_ref().unwrap().default.repeat, None);
    assert!(report.notes.iter().any(|n| n.text.contains("repeated round polygons")), "{:?}", report.notes);
    // A placement QGIS doesn't repeat (horizontal, inside the polygon) keeps the distance unused:
    // not noted.
    let mut report = Report::default();
    read_project(&qgs.replace(r#"placement="2""#, r#"placement="4""#), "", &mut report).unwrap();
    assert!(!report.notes.iter().any(|n| n.text.contains("repeated round polygons")), "{:?}", report.notes);
}

#[test]
fn files_of_qgis_1_and_2_read_by_their_old_names() {
    // Before QGIS 3: no layer tree but a legend, the canvas's coordinate system, scale limits named
    // the other way round, opacity as transparency, and QGIS 2's names for line and fill settings.
    let qgs = r#"<qgis projectname="" version="2.14.6-Essen"><title>Old</title>
      <mapcanvas><units>meters</units><extent><xmin>0</xmin><ymin>0</ymin><xmax>10</xmax><ymax>10</ymax></extent>
        <destinationsrs><spatialrefsys><authid>EPSG:21781</authid></spatialrefsys></destinationsrs></mapcanvas>
      <legend updateDrawingOrder="true">
        <legendlayer checked="Qt::Checked" name="roads"><filegroup><legendlayerfile layerid="roads1" visible="1"/></filegroup></legendlayer>
        <legendgroup checked="Qt::Unchecked" name="base">
          <legendlayer checked="Qt::Checked" name="land"><filegroup><legendlayerfile layerid="land1" visible="1"/></filegroup></legendlayer>
        </legendgroup>
      </legend>
      <projectlayers>
        <maplayer type="vector" minimumScale="1000" maximumScale="50000" hasScaleBasedVisibilityFlag="1">
          <id>land1</id><datasource>/d/land.shp</datasource><layername>land</layername><provider>ogr</provider>
          <renderer-v2 type="singleSymbol"><symbols><symbol type="fill" name="0" alpha="1" outputUnit="MM"><layer class="SimpleFill" pass="0" locked="0">
            <prop k="color" v="200,200,150,255"/><prop k="color_border" v="10,20,30,255"/><prop k="style" v="solid"/><prop k="style_border" v="dash"/><prop k="width_border" v="0.5"/>
          </layer></symbol></symbols></renderer-v2>
          <layerTransparency>40</layerTransparency>
          <customproperties><property key="labeling/enabled" value="true"/><property key="labeling/fieldName" value="name"/></customproperties>
        </maplayer>
        <maplayer type="vector">
          <id>roads1</id><datasource>/d/roads.shp</datasource><layername>roads</layername><provider>ogr</provider>
          <renderer-v2 type="singleSymbol"><symbols><symbol type="line" name="0" alpha="1" outputUnit="Pixel"><layer class="SimpleLine" pass="0" locked="0">
            <prop k="color" v="40,40,40,255"/><prop k="width" v="2"/><prop k="penstyle" v="dash"/><prop k="capstyle" v="flat"/>
          </layer></symbol></symbols></renderer-v2>
          <transparencyLevelInt>255</transparencyLevelInt>
        </maplayer>
      </projectlayers></qgis>"#;
    let mut report = Report::default();
    let p = read_project(qgs, "", &mut report).unwrap();
    assert_eq!(p.crs.as_deref(), Some("EPSG:21781"), "the canvas's");
    let names: Vec<(&str, bool)> = p.layers.iter().map(|l| (l.name.as_str(), l.visible)).collect();
    assert_eq!(names, [("roads", true), ("land", false)], "the legend's order; land's group is unchecked");
    let (roads, land) = (&p.layers[0].style, &p.layers[1].style);
    // maximumScale was the out-beyond limit, minimumScale the in-beyond.
    let (min, max) = (land.min_scale.flatten().unwrap(), land.max_scale.flatten().unwrap());
    assert!((min - 50_000.0 * QGIS_SCALE).abs() < 1e-6 && (max - 1000.0 * QGIS_SCALE).abs() < 1e-6);
    assert_eq!((land.transparency, roads.transparency), (Some(40.0), Some(0.0)));
    let Some(Renderer::Single(fill)) = &land.renderer else { panic!() };
    assert_eq!((fill.fill, fill.stroke), (Rgba::rgb(200, 200, 150), Rgba::rgb(10, 20, 30)));
    assert!((fill.stroke_width - 0.5 * 96.0 / 25.4).abs() < 1e-4);
    // The line, in the symbol's unit (pixels), dashed as Qt dashes: 4 and 2 widths.
    let Some(Renderer::Single(line)) = &roads.renderer else { panic!() };
    assert_eq!((line.stroke, line.stroke_width), (Rgba::rgb(40, 40, 40), 2.0));
    let strokes = roads.line_strokes.as_deref().unwrap();
    assert_eq!((strokes.len(), strokes[0].dash.map(|d| d.steps().to_vec())), (1, Some(vec![8.0, 4.0])));
    let said = |layer: &str, part: &str| report.notes.iter().any(|n| n.layer == layer && n.text.contains(part));
    assert!(said("land", "dashed polygon outlines"));
    assert!(said("land", "format of QGIS 2"));
}

#[test]
fn what_rustgis_has_no_place_for_is_noted() {
    let effect = r#"<effect enabled="1" type="effectStack"><effect type="drawSource"><Option type="Map"><Option name="enabled" value="1"/></Option></effect>
        <effect type="dropShadow"><Option type="Map"><Option name="enabled" value="1"/></Option></effect></effect>"#;
    let plain = r#"<effect enabled="1" type="effectStack"><effect type="drawSource"><Option type="Map"><Option name="enabled" value="1"/></Option></effect>
        <effect type="outerGlow"><Option type="Map"><Option name="enabled" value="0"/></Option></effect></effect>"#;
    let qgs = format!(
        r#"<qgis version="3.42.0">
      <projectlayers>
        <maplayer type="vector" labelsEnabled="1"><id>a</id><datasource>/d/a.shp</datasource><layername>A</layername><provider>ogr</provider>
          <temporal enabled="1" mode="1" startField="day"/>
          <map-layer-style-manager current="default"><map-layer-style name="default"/><map-layer-style name="night"/></map-layer-style-manager>
          <vectorjoins><join joinLayerId="b"/></vectorjoins>
          <expressionfields><field name="twice" expression="&quot;v&quot; * 2"/></expressionfields>
          <SingleCategoryDiagramRenderer diagramType="Pie"><DiagramCategory enabled="1"/></SingleCategoryDiagramRenderer>
          <renderer-v2 type="singleSymbol" enableorderby="1">{effect}<symbols><symbol type="marker" name="0" alpha="1">
            <layer class="SimpleMarker" enabled="1"><Option type="Map"><Option name="color" value="1,2,3,255"/></Option>{plain}</layer></symbol></symbols></renderer-v2>
          <labeling type="simple"><settings calloutType="simple">
            <text-style fieldName="name"><text-buffer bufferDraw="1" bufferSize="1" bufferColor="255,255,255,255"/></text-style>
            <text-format wrapChar="" autoWrapLength="12"/>
            <placement placement="2" dist="2" xOffset="1" yOffset="0" repeatDistance="0" priority="8"/>
            <dd_properties><Option type="Map"><Option name="properties" type="Map"><Option name="Size" type="Map"><Option name="active" value="true"/><Option name="field" value="size"/></Option></Option></Option></dd_properties>
            <callout type="simple"><Option type="Map"><Option name="enabled" value="1"/></Option></callout>
          </settings></labeling>
        </maplayer>
      </projectlayers>
      <main-annotation-layer><items><item type="pointtext"/><item type="linetext"/></items></main-annotation-layer>
      <Layouts><Layout name="One"/></Layouts>
      <Bookmarks><Bookmark name="Here"/></Bookmarks>
      <visibility-presets><visibility-preset name="Only points"/></visibility-presets>
      <relations><relation id="r"/></relations>
    </qgis>"#
    );
    let mut report = Report::default();
    read_project(&qgs, "", &mut report).unwrap();
    let said = |layer: &str, part: &str| report.notes.iter().any(|n| n.layer == layer && n.text.contains(part));
    for part in ["print layouts (1)", "annotations (2", "bookmarks (1)", "map themes (1)", "relations between layers (1)"] {
        assert!(said("", part), "{part}: {:?}", report.notes);
    }
    for part in [
        "time settings",
        "other styles (night)",
        "joins",
        "virtual fields",
        "diagrams",
        "order-by",
        "draw effects",
        "label distances",
        "label priorities",
        "callouts",
        "data-defined label settings",
    ] {
        assert!(said("A", part), "{part}: {:?}", report.notes);
    }
    // The renderer's effect is noted once; the symbol layer's, only plain drawing, isn't an effect.
    assert_eq!(report.notes.iter().find(|n| n.text.contains("draw effects")).map(|n| n.count), Some(1));
    // Wrapping and repeats along lines are read now, not noted.
    assert!(!said("A", "wrapped") && !said("A", "repeated"), "{:?}", report.notes);
}

/// A raster style file with `renderer` as its renderer and `pipe` after it in the pipe.
fn raster_qml(renderer: &str, pipe: &str) -> String {
    format!(
        r#"<!DOCTYPE qgis PUBLIC 'http://mrcc.com/qgis.dtd' 'SYSTEM'><qgis version="4.2.3" layerType="Raster"><pipe>{renderer}{pipe}</pipe><blendMode>0</blendMode></qgis>"#
    )
}

#[test]
fn raster_renderers_read_as_rustgis_raster_styles() {
    use crate::raster::{RasterRenderer, Resampling, Stretch};
    let read = |renderer: &str, pipe: &str| {
        let mut report = Report::default();
        let s = read_style(&raster_qml(renderer, pipe), "R", &mut report).unwrap();
        (s, report)
    };
    // Pseudocolour, interpolated (SMV25's rock): a ramp from transparent white to grey over 0 to 1.
    let (s, report) = read(
        r##"<rasterrenderer type="singlebandpseudocolor" band="1" opacity="0.5" alphaBand="-1"><rastershader>
          <colorrampshader colorRampType="INTERPOLATED" clip="0" minimumValue="0" maximumValue="1">
            <item value="0" color="#ffffff" alpha="0" label="0"/><item value="1" color="#3f3f3f" alpha="255" label="1"/>
          </colorrampshader></rastershader></rasterrenderer>"##,
        r#"<brightnesscontrast brightness="51" contrast="-10" gamma="2"/><huesaturation saturation="0" grayscaleMode="0" colorizeOn="0" invertColors="0"/>
           <provider><resampling enabled="true" zoomedInResamplingMethod="bilinear" zoomedOutResamplingMethod="nearestNeighbour"/></provider>"#,
    );
    let r = s.raster.unwrap();
    let RasterRenderer::Stretch { band: 0, ramp, invert: false } = &r.renderer else { panic!("{:?}", r.renderer) };
    assert_eq!((ramp.at(0.0).a(), ramp.at(1.0)), (0, Rgba::rgb(0x3f, 0x3f, 0x3f)));
    assert_eq!(r.stretch, Stretch::custom(0.0, 1.0));
    assert_eq!((r.brightness, r.contrast, r.gamma, r.resampling), (20.0, -10.0, 2.0, Resampling::Bilinear));
    assert_eq!(s.transparency, Some(50.0));
    assert!(report.notes.is_empty(), "{:?}", report.notes);

    // Discrete: classes up to each entry's value, the last open-ended; values outside clipped.
    let (s, _) = read(
        r##"<rasterrenderer type="singlebandpseudocolor" band="2"><rastershader><colorrampshader colorRampType="DISCRETE" clip="1" minimumValue="400" maximumValue="600">
            <item value="450" color="#ff0000"/><item value="500" color="#00ff00"/><item value="inf" color="#0000ff"/></colorrampshader></rastershader></rasterrenderer>"##,
        "",
    );
    let r = s.raster.unwrap();
    let RasterRenderer::Classify { band: 1, ramp, breaks, .. } = &r.renderer else { panic!("{:?}", r.renderer) };
    assert_eq!((breaks.len(), breaks[0], breaks[1]), (3, 450.0, 500.0));
    assert!(breaks[2].is_finite() && breaks[2] > 1e30, "inf kept as a number a project can hold");
    assert_eq!((ramp.pick(0, 3), ramp.pick(2, 3)), (Rgba::rgb(255, 0, 0), Rgba::rgb(0, 0, 255)));
    assert_eq!((r.mask.min, r.mask.max), (Some(400.0), Some(600.0)));

    // Exact and paletted: a colour per value, sorted.
    for renderer in [
        r##"<rasterrenderer type="singlebandpseudocolor" band="1"><rastershader><colorrampshader colorRampType="EXACT">
            <item value="7" color="#00ff00"/><item value="3" color="#ff0000"/></colorrampshader></rastershader></rasterrenderer>"##,
        r##"<rasterrenderer type="paletted" band="1"><colorPalette><paletteEntry value="7" color="#00ff00" alpha="255"/><paletteEntry value="3" color="#ff0000" alpha="255"/></colorPalette></rasterrenderer>"##,
    ] {
        let (s, _) = read(renderer, "");
        let r = s.raster.unwrap();
        let RasterRenderer::UniqueValues { band: 0, values, .. } = &r.renderer else { panic!("{:?}", r.renderer) };
        assert_eq!(values, &[(3.0, Rgba::rgb(255, 0, 0)), (7.0, Rgba::rgb(0, 255, 0))]);
    }

    // Grey: black to white, stretched to its enhancement's bounds; a value made transparent and
    // a user no-data value hide those values.
    let gray = r#"<rasterrenderer type="singlebandgray" grayBand="1" gradient="BlackToWhite"><rasterTransparency><singleValuePixelList>
          <pixelListEntry min="-9999" max="-9999" percentTransparent="100"/><pixelListEntry min="1" max="5" percentTransparent="50"/></singleValuePixelList></rasterTransparency>
          <contrastEnhancement><minValue>10</minValue><maxValue>200</maxValue><algorithm>StretchToMinimumMaximum</algorithm></contrastEnhancement></rasterrenderer>"#;
    let qml = raster_qml(gray, "").replace(
        "</qgis>",
        r#"<noData><noDataList bandNo="1" useSrcNoData="1"><noDataRange min="0" max="0"/></noDataList></noData></qgis>"#,
    );
    let mut report = Report::default();
    let r = read_style(&qml, "R", &mut report).unwrap().raster.unwrap();
    let RasterRenderer::Stretch { ramp, .. } = &r.renderer else { panic!() };
    assert_eq!((ramp.at(0.0), ramp.at(1.0)), (Rgba::rgb(0, 0, 0), Rgba::rgb(255, 255, 255)));
    assert_eq!(r.stretch, Stretch::custom(10.0, 200.0));
    assert_eq!(r.mask.values, [-9999.0, 0.0]);
    assert!(report.notes.iter().any(|n| n.text.contains("in part")), "{:?}", report.notes);

    // Multiband: RGB from 1-based bands, a stretch per band; a canvas-wide cut follows the view.
    let (s, _) = read(
        r#"<rasterrenderer type="multibandcolor" redBand="3" greenBand="2" blueBand="1">
          <redContrastEnhancement><minValue>1</minValue><maxValue>2</maxValue><algorithm>StretchToMinimumMaximum</algorithm></redContrastEnhancement>
          <greenContrastEnhancement><minValue>3</minValue><maxValue>4</maxValue><algorithm>StretchToMinimumMaximum</algorithm></greenContrastEnhancement>
          <blueContrastEnhancement><minValue>5</minValue><maxValue>6</maxValue><algorithm>StretchToMinimumMaximum</algorithm></blueContrastEnhancement></rasterrenderer>"#,
        "",
    );
    let r = s.raster.unwrap();
    assert_eq!(r.renderer, RasterRenderer::Rgb { red: 2, green: 1, blue: 0 });
    assert_eq!(r.stretch, Stretch::custom_bands([[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]]));
    let (s, _) = read(
        r#"<rasterrenderer type="singlebandgray" grayBand="1"><minMaxOrigin><limits>CumulativeCut</limits><extent>UpdatedCanvas</extent><cumulativeCutLower>0.02</cumulativeCutLower><cumulativeCutUpper>0.98</cumulativeCutUpper></minMaxOrigin>
          <contrastEnhancement><minValue>10</minValue><maxValue>200</maxValue><algorithm>StretchToMinimumMaximum</algorithm></contrastEnhancement></rasterrenderer>"#,
        "",
    );
    let r = s.raster.unwrap();
    assert!(
        r.dra && matches!(r.stretch, Stretch::PercentClip { low, high } if (low - 2.0).abs() < 1e-9 && (high - 2.0).abs() < 1e-9),
        "{r:?}"
    );

    // Hillshade as shaded relief; hue and saturation and contours are noted.
    let (s, report) = read(
        r#"<rasterrenderer type="hillshade" band="1" azimuth="300" angle="35" zfactor="2" multidirection="0"/>"#,
        r#"<huesaturation saturation="-30" grayscaleMode="0" colorizeOn="0" invertColors="0"/>"#,
    );
    assert_eq!(
        s.raster.unwrap().renderer,
        RasterRenderer::ShadedRelief { band: 0, ramp: None, azimuth: 300.0, altitude: 35.0, z_factor: 2.0 }
    );
    assert!(report.notes.iter().any(|n| n.text.contains("saturation")));
    let (s, report) = read(r#"<rasterrenderer type="contour" band="1"/>"#, "");
    assert!(s.raster.is_none() && report.notes.iter().any(|n| n.text.contains("contour")));
    // Nonsense bands and values: nothing, without a crash, and noted.
    for bad in [r#"<rasterrenderer type="singlebandgray" grayBand="0"/>"#, r#"<rasterrenderer type="paletted" band="99999999999"/>"#] {
        let (s, report) = read(bad, "");
        assert!(s.raster.is_none() && report.notes.iter().any(|n| n.text.contains("couldn't read")), "{bad}: {report:?}");
    }
}

/// Every generated style and project in `testdata/qgis` reads, each project with its layers.
#[test]
fn every_corpus_file_reads() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/qgis");
    let files = |dir: &str, extensions: &[&str]| {
        let mut paths: Vec<_> = std::fs::read_dir(root.join(dir)).unwrap().map(|e| e.unwrap().path()).collect();
        paths.retain(|path| path.is_file() && path.extension().and_then(|e| e.to_str()).is_some_and(|e| extensions.contains(&e)));
        paths.sort();
        paths
    };
    // Pin the corpus sizes so a regeneration cannot silently drop fixtures.
    let styles = files("generated/qml", &["qml"]);
    assert_eq!(styles.len(), 90, "style corpus changed: {styles:?}");
    for path in &styles {
        let text = std::fs::read_to_string(path).unwrap();
        let mut report = Report::default();
        read_style(&text, "layer", &mut report).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
    let projects = files("generated/qgs", &["qgs", "qgz"]);
    assert_eq!(projects.len(), 21, "project corpus changed: {projects:?}");
    for path in &projects {
        let bytes = std::fs::read(path).unwrap();
        let text = if path.extension().is_some_and(|e| e == "qgz") {
            let entries = rustgis_data::zip::read_matching_within(&bytes, |n| n.ends_with(".qgs"), 64 << 20).unwrap();
            String::from_utf8(entries.into_iter().next().unwrap().1).unwrap()
        } else {
            String::from_utf8(bytes).unwrap()
        };
        let dir = path.parent().unwrap().to_string_lossy().to_string();
        let mut report = Report::default();
        let project = read_project(&text, &dir, &mut report).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(!project.layers.is_empty(), "{}: no layers", path.display());
        // The PostGIS and SQL Server projects' made-up password reaches neither the layers nor the notes.
        assert!(!format!("{project:?} {report:?}").contains("secret"), "{}: the password came through", path.display());
    }
}

#[test]
fn marker_and_hashed_lines_carry_over_as_strokes_of_marks() {
    // As SMV25 writes them: a line (offset to the left), then marks at its ends in three layers (a
    // picture, a triangle and a font character, offset), then QGIS 2's hash marks at an interval.
    let qml = r#"<qgis version="3.40.0"><renderer-v2 type="singleSymbol"><symbols>
      <symbol type="line" name="0" alpha="1">
        <layer class="SimpleLine" enabled="1"><Option type="Map"><Option name="line_color" value="0,0,0,255"/><Option name="line_width" value="0.5"/>
          <Option name="offset" value="-0.5"/></Option></layer>
        <layer class="MarkerLine" enabled="1" pass="2"><Option type="Map">
          <Option name="placements" value="FirstVertex|LastVertex"/><Option name="rotate" value="1"/><Option name="interval" value="3"/>
          <Option name="offset" value="0.5"/><Option name="offset_unit" value="MM"/><Option name="average_angle_length" value="4"/></Option>
          <symbol type="marker" name="@0@1" alpha="0.5">
            <layer class="SvgMarker" enabled="1"><Option type="Map"><Option name="name" value="arrow.svg"/>
              <Option name="color" value="242,242,242,255"/><Option name="size" value="1"/><Option name="offset" value="0.12,-0.35"/><Option name="offset_unit" value="MM"/></Option></layer>
            <layer class="SimpleMarker" enabled="1"><Option type="Map"><Option name="name" value="triangle"/><Option name="color" value="0,136,208,255"/>
              <Option name="outline_color" value="0,0,0,255"/><Option name="outline_width" value="0.2"/><Option name="size" value="2"/><Option name="angle" value="90"/></Option></layer>
            <layer class="FontMarker" enabled="1"><Option type="Map"><Option name="chr" value="{"/><Option name="font" value="DKM_Symbole"/>
              <Option name="color" value="0,0,0,255"/><Option name="size" value="1"/></Option></layer>
          </symbol>
        </layer>
        <layer class="HashLine" enabled="1"><prop k="placement" v="interval"/><prop k="interval" v="2"/><prop k="interval_unit" v="MM"/>
          <prop k="hash_length" v="3"/><prop k="hash_angle" v="30"/><prop k="rotate" v="0"/>
          <symbol type="line" name="@0@2" alpha="1"><layer class="SimpleLine" enabled="1"><prop k="line_color" v="0,0,255,255"/><prop k="line_width" v="0.3"/></layer></symbol>
        </layer>
      </symbol></symbols></renderer-v2></qgis>"#;
    let mut report = Report::default();
    let s = read_style(qml, "Rail", &mut report).unwrap();
    let strokes = s.line_strokes.unwrap();
    let mm = 96.0 / 25.4;
    let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
    assert_eq!(strokes.len(), 5, "{strokes:?}");
    assert!(strokes[0].markers.is_none());
    // An offset to the left stays to the left (it used to be taken as none).
    assert!(near(strokes[0].offset, -0.5 * mm), "{:?}", strokes[0]);

    // The picture: a circle in its colour (the symbol's opacity on it), offset along and across.
    let (pic, m) = (strokes[1], strokes[1].markers.unwrap());
    assert_eq!((m.mark, m.at), (LineMark::Marker(Marker::Circle), Placements::FIRST.with(Placements::LAST)));
    assert_eq!(pic.color, Some(Rgba::new(242, 242, 242, 128)));
    assert!(near(m.offset_along, 0.12 * mm) && near(pic.offset, (0.5 - 0.35) * mm), "{pic:?}");
    assert!(near(m.size, mm) && near(m.average_angle, 4.0 * mm) && m.rotate && m.outline.is_none());
    assert_eq!(pic.level, Some(2));

    // The triangle: its outline, its angle, offset only by the line's.
    let (tri, m) = (strokes[2], strokes[2].markers.unwrap());
    assert_eq!((m.mark, m.angle, m.outline), (LineMark::Marker(Marker::Triangle), 90.0, Some(Rgba::new(0, 0, 0, 128))));
    assert!(near(tri.width.unwrap(), 0.2 * mm) && near(tri.offset, 0.5 * mm) && near(m.size, 2.0 * mm));

    // The font character: its family's, in its colour, of its size.
    let (chr, m) = (strokes[3], strokes[3].markers.unwrap());
    assert_eq!(m.mark, LineMark::Marker(Marker::Char(Glyph::new('{', "DKM_Symbole"))));
    assert_eq!(chr.color, Some(Rgba::new(0, 0, 0, 128)));
    assert!(near(m.size, mm) && near(chr.offset, 0.5 * mm), "{chr:?}");

    // QGIS 2's hash marks: upright, at their angle, the line's colour and width.
    let (hash, m) = (strokes[4], strokes[4].markers.unwrap());
    assert_eq!((m.mark, m.at, m.rotate, m.angle), (LineMark::Hash, Placements::INTERVAL, false, 30.0));
    assert_eq!(hash.color, Some(Rgba::rgb(0, 0, 255)));
    assert!(near(m.interval, 2.0 * mm) && near(m.size, 3.0 * mm) && near(hash.width.unwrap(), 0.3 * mm));

    // Only the picture isn't drawn as QGIS draws it.
    assert_eq!(report.notes.len(), 1, "{:?}", report.notes);
    assert!(report.notes.iter().any(|n| n.kind == NoteKind::Approximated && n.text.contains("SVG markers")), "{:?}", report.notes);

    // A placement on curves is drawn at the vertices; one QGIS adds later is left out, and said.
    let other = qml.replace("FirstVertex|LastVertex", "CurvePoint|SomethingNew");
    let mut report = Report::default();
    let s = read_style(&other, "Rail", &mut report).unwrap();
    assert_eq!(s.line_strokes.unwrap()[1].markers.unwrap().at, Placements::VERTICES);
    let said = |kind: NoteKind, part: &str| report.notes.iter().any(|n| n.kind == kind && n.text.contains(part));
    assert!(said(NoteKind::Approximated, "curve points") && said(NoteKind::Skipped, "\"somethingnew\""), "{:?}", report.notes);
}

#[test]
fn a_font_marker_without_its_character_draws_qgiss_a_and_escaped_ones_are_decoded() {
    let props = |chr: Option<&str>| -> Vec<(String, String)> {
        chr.map(|c| vec![("chr".to_string(), c.to_string()), ("font".to_string(), "DKM_Symbole".to_string())]).unwrap_or_default()
    };
    let char_of = |chr: Option<&str>| match symbol::font_char(&props(chr), "Signs", &mut Report::default()) {
        Some(Marker::Char(g)) => g.ch,
        other => panic!("{other:?}"),
    };
    // QGIS draws its default `A` for a font marker with no character or an empty one.
    assert_eq!((char_of(None), char_of(Some(""))), ('A', 'A'));
    // It saves some characters as their code between two `~!_#!#_!~`.
    assert_eq!(char_of(Some("~!_#!#_!~1~!_#!#_!~")), '\u{1}');
    assert_eq!(char_of(Some("~!_#!#_!~10~!_#!#_!~x")), '\n');
    // A broken escape is read as it is.
    assert_eq!((char_of(Some("~!_#!#_!~1")), char_of(Some("~!_#!#_!~x~!_#!#_!~"))), ('~', '~'));
    assert_eq!(char_of(Some("@")), '@');
}

#[test]
fn marker_symbols_of_several_layers_carry_over_as_marks_turned_by_their_field() {
    // SMV25's signs: a white character under a coloured one, the symbol turned by `- "WINKEL"`;
    // then a square anchored at its top left, offset, at an angle.
    let dd = |expr: &str| {
        format!(
            r#"<data_defined_properties><Option type="Map"><Option value="" type="QString" name="name"/><Option type="Map" name="properties">
            <Option type="Map" name="angle"><Option value="true" type="bool" name="active"/><Option value="{expr}" type="QString" name="expression"/>
            <Option value="3" type="int" name="type"/></Option></Option><Option value="collection" type="QString" name="type"/></Option></data_defined_properties>"#
        )
    };
    let font = |chr: &str, color: &str, size: &str| {
        format!(
            r#"<layer class="FontMarker" enabled="1"><Option type="Map"><Option name="chr" value="{chr}"/><Option name="font" value="DKM_Symbole"/>
            <Option name="color" value="{color}"/><Option name="size" value="{size}"/><Option name="size_unit" value="MM"/>
            <Option name="horizontal_anchor_point" value="1"/><Option name="vertical_anchor_point" value="1"/></Option>{}</layer>"#,
            dd("- &quot;WINKEL&quot;")
        )
    };
    let qml = format!(
        r#"<qgis version="3.40.0"><renderer-v2 type="singleSymbol"><symbols><symbol type="marker" name="0" alpha="1">{}{}</symbol></symbols></renderer-v2></qgis>"#,
        font(";", "255,255,255,255", "0.9"),
        font("L", "0,136,208,255", "1")
    );
    let mut report = Report::default();
    let s = read_style(&qml, "Signs", &mut report).unwrap();
    let mm = 96.0 / 25.4;
    let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
    let marks = s.line_strokes.clone().unwrap();
    assert_eq!(marks.len(), 2, "{marks:?}");
    let (under, over) = (marks[0].markers.unwrap(), marks[1].markers.unwrap());
    assert_eq!(under.mark, LineMark::Marker(Marker::Char(Glyph::new(';', "DKM_Symbole"))));
    assert_eq!(marks[0].color, Some(Rgba::rgb(255, 255, 255)));
    assert_eq!(over.mark, LineMark::Marker(Marker::Char(Glyph::new('L', "DKM_Symbole"))));
    assert!(near(under.size, 0.9 * mm) && near(over.size, mm) && under.rotate && over.rotate, "they turn");
    // The top layer is the symbol (the legend's).
    let Some(Renderer::Single(sym)) = &s.renderer else { panic!() };
    assert_eq!((sym.marker, sym.fill), (Marker::Char(Glyph::new('L', "DKM_Symbole")), Rgba::rgb(0, 136, 208)));
    assert_eq!(s.marker_rotation, Some(Some(crate::symbology::MarkerRotation { field: "WINKEL".into(), counterclockwise: true })));
    assert!(!report.notes.iter().any(|n| n.text.contains("data-defined")), "{:?}", report.notes);
    // Applied to a point layer, its markers turn.
    let mut layer = crate::Layer::new(1, rustgis_data::Dataset::new("signs", rustgis_data::GeometryType::Point, vec![]));
    s.apply(&mut layer);
    assert_eq!(layer.marker_rotation.as_ref().map(|r| r.field.as_str()), Some("WINKEL"));
    assert_eq!(layer.line_strokes.len(), 2);

    // A square anchored at its top left, offset and at an angle: one mark, shifted by half its size.
    let qml = r#"<qgis version="3.40.0"><renderer-v2 type="singleSymbol"><symbols><symbol type="marker" name="0" alpha="1">
      <layer class="SimpleMarker" enabled="1"><Option type="Map"><Option name="name" value="square"/><Option name="color" value="0,0,0,255"/>
        <Option name="size" value="4"/><Option name="offset" value="1,-2"/><Option name="offset_unit" value="MM"/><Option name="angle" value="30"/>
        <Option name="horizontal_anchor_point" value="0"/><Option name="vertical_anchor_point" value="0"/></Option></layer>
      </symbol></symbols></renderer-v2></qgis>"#;
    let mut report = Report::default();
    let s = read_style(qml, "Signs", &mut report).unwrap();
    let marks = s.line_strokes.unwrap();
    let m = marks[0].markers.unwrap();
    assert_eq!(marks.len(), 1);
    assert!(near(m.offset_along, 3.0 * mm) && near(marks[0].offset, 0.0) && m.angle == 30.0, "{marks:?}");
    assert_eq!(s.marker_rotation, Some(None), "not turned by a field");
    assert!(!report.notes.iter().any(|n| n.text.contains("turned markers")), "{:?}", report.notes);

    // One plain layer stays the symbol alone, as before.
    let qml = r#"<qgis version="3.40.0"><renderer-v2 type="singleSymbol"><symbols><symbol type="marker" name="0" alpha="1">
      <layer class="SimpleMarker" enabled="1"><Option type="Map"><Option name="name" value="star"/><Option name="color" value="200,0,0,255"/></Option></layer>
      </symbol></symbols></renderer-v2></qgis>"#;
    let s = read_style(qml, "Signs", &mut Report::default()).unwrap();
    assert_eq!(s.line_strokes, Some(Vec::new()));
    let Some(Renderer::Single(sym)) = &s.renderer else { panic!() };
    assert_eq!(sym.marker, Marker::Star);

    // An angle from any other expression is noted, and turns nothing.
    let other = qml.replace("</layer>", &format!("{}</layer>", dd("&quot;A&quot; * 2")));
    let mut report = Report::default();
    let s = read_style(&other, "Signs", &mut report).unwrap();
    assert_eq!(s.marker_rotation, Some(None));
    assert!(report.notes.iter().any(|n| n.text.contains("data-defined")), "{:?}", report.notes);

    // Classes of one layer: an antenna that keeps still where a cliff turns by the field. The
    // antenna's single plain character becomes a mark that doesn't turn; the cliff's turns.
    let qml = format!(
        r#"<qgis version="3.40.0"><renderer-v2 type="categorizedSymbol" attr="Symbol"><categories>
          <category value="Antenne" symbol="0" label="Antenne" render="true"/><category value="Fels" symbol="1" label="Fels" render="true"/></categories>
          <symbols><symbol type="marker" name="0" alpha="1">{}</symbol><symbol type="marker" name="1" alpha="1">{}</symbol></symbols></renderer-v2></qgis>"#,
        font("D", "0,0,0,255", "1.3").replace(&dd("- &quot;WINKEL&quot;"), ""),
        font("P", "0,0,0,255", "1.5")
    );
    let mut report = Report::default();
    let s = read_style(&qml, "Signs", &mut report).unwrap();
    assert_eq!(s.marker_rotation.clone().flatten().map(|r| r.field), Some("WINKEL".into()));
    let Some(Renderer::Unique { class_strokes, .. }) = &s.renderer else { panic!("{:?}", s.renderer) };
    let still = class_strokes.get("Antenne").unwrap();
    assert_eq!(still.len(), 1);
    assert!(!still[0].markers.unwrap().rotate, "the antenna keeps still");
    // The cliff's one centred character turns as the symbol does: no marks of its own.
    assert!(!class_strokes.contains_key("Fels"), "{class_strokes:?}");
    let turned = |r: &Report| r.notes.iter().any(|n| n.text.contains("turned by different fields"));
    assert!(!turned(&report), "{:?}", report.notes);
    // Classes turned by different fields all turn by the first: said.
    let qml = format!(
        r#"<qgis version="3.40.0"><renderer-v2 type="categorizedSymbol" attr="Symbol"><categories>
          <category value="Antenne" symbol="0" label="Antenne" render="true"/><category value="Fels" symbol="1" label="Fels" render="true"/></categories>
          <symbols><symbol type="marker" name="0" alpha="1">{}</symbol><symbol type="marker" name="1" alpha="1">{}</symbol></symbols></renderer-v2></qgis>"#,
        font("D", "0,0,0,255", "1.3"),
        font("P", "0,0,0,255", "1.5").replace(&dd("- &quot;WINKEL&quot;"), &dd("&quot;RICHTUNG&quot;"))
    );
    let mut report = Report::default();
    let s = read_style(&qml, "Signs", &mut report).unwrap();
    assert_eq!(s.marker_rotation.flatten().map(|r| r.field), Some("WINKEL".into()));
    assert!(turned(&report), "{:?}", report.notes);
    // A switched-off class's field turns nothing: the shown one's does, and nothing is said.
    let qml = qml.replace(r#"label="Antenne" render="true""#, r#"label="Antenne" render="false""#).replace("WINKEL", "W2");
    let mut report = Report::default();
    let s = read_style(&qml, "Signs", &mut report).unwrap();
    assert_eq!(s.marker_rotation.flatten().map(|r| r.field), Some("RICHTUNG".into()));
    assert!(!turned(&report), "{:?}", report.notes);

    // QGIS 2's `dd_properties` are still noted.
    let old = r#"<qgis version="2.18.0"><renderer-v2 type="singleSymbol"><symbols><symbol type="marker" name="0" alpha="1">
      <layer class="SimpleMarker" enabled="1"><Option type="Map"><Option name="name" value="star"/></Option>
      <dd_properties><Option type="Map" name="angle"><Option name="active" value="true"/></Option></dd_properties></layer>
      </symbol></symbols></renderer-v2></qgis>"#;
    let mut report = Report::default();
    read_style(old, "Signs", &mut report).unwrap();
    assert!(report.notes.iter().any(|n| n.text.contains("data-defined")), "{:?}", report.notes);
}
