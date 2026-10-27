//! Conversions between RustGIS geometry and `geo` types, in the analysis plane
//! ([`rustgis_geom::lonlat_to_plane`]): Web Mercator meters within ±85.05°, carried on to the poles
//! beyond, so polar geometry isn't clamped onto 85.05° (#1033).

use geo::{Coord as GC, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
use rustgis_data::Geometry;
use rustgis_geom::{Coord, lonlat_to_plane, plane_to_lonlat};

fn gc(c: Coord) -> GC<f64> {
    let m = lonlat_to_plane(c);
    GC { x: m.x, y: m.y }
}

fn back(c: &GC<f64>) -> Coord {
    plane_to_lonlat(Coord::new(c.x, c.y))
}

pub fn ring(r: &[Coord]) -> LineString<f64> {
    LineString::new(r.iter().map(|c| gc(*c)).collect())
}

pub fn to_multipolygon(g: &Geometry) -> Option<MultiPolygon<f64>> {
    match g {
        Geometry::Polygon(polys) => Some(MultiPolygon::new(
            polys
                .iter()
                .filter_map(|rings| {
                    let mut it = rings.iter();
                    let outer = ring(it.next()?);
                    Some(Polygon::new(outer, it.map(|r| ring(r)).collect()))
                })
                .collect(),
        )),
        _ => None,
    }
}

pub fn to_multiline(g: &Geometry) -> Option<MultiLineString<f64>> {
    match g {
        Geometry::Line(parts) => Some(MultiLineString::new(parts.iter().map(|p| ring(p)).collect())),
        _ => None,
    }
}

pub fn to_multipoint(g: &Geometry) -> Option<MultiPoint<f64>> {
    match g {
        Geometry::Point(c) => Some(MultiPoint::new(vec![Point(gc(*c))])),
        Geometry::MultiPoint(v) => Some(MultiPoint::new(v.iter().map(|c| Point(gc(*c))).collect())),
        _ => None,
    }
}

pub fn to_geo(g: &Geometry) -> geo::Geometry<f64> {
    match g {
        Geometry::Point(c) => geo::Geometry::Point(Point(gc(*c))),
        Geometry::MultiPoint(_) => {
            to_multipoint(g).map(geo::Geometry::MultiPoint).unwrap_or(geo::Geometry::MultiPoint(MultiPoint::new(vec![])))
        }
        Geometry::Line(_) => {
            to_multiline(g).map(geo::Geometry::MultiLineString).unwrap_or(geo::Geometry::MultiLineString(MultiLineString::new(vec![])))
        }
        Geometry::Polygon(_) => {
            to_multipolygon(g).map(geo::Geometry::MultiPolygon).unwrap_or(geo::Geometry::MultiPolygon(MultiPolygon::new(vec![])))
        }
    }
}

pub fn from_multipolygon(mp: &MultiPolygon<f64>) -> Option<Geometry> {
    let polys: Vec<Vec<Vec<Coord>>> =
        mp.0.iter()
            .map(|p| std::iter::once(p.exterior()).chain(p.interiors().iter()).map(|r| r.0.iter().map(back).collect()).collect())
            .collect();
    (!polys.is_empty()).then_some(Geometry::Polygon(polys))
}

pub fn from_line(ml: &MultiLineString<f64>) -> Option<Geometry> {
    let parts: Vec<Vec<Coord>> = ml.0.iter().filter(|l| l.0.len() >= 2).map(|l| l.0.iter().map(back).collect()).collect();
    (!parts.is_empty()).then_some(Geometry::Line(parts))
}

pub fn from_point(p: &Point<f64>) -> Geometry {
    Geometry::Point(back(&p.0))
}
