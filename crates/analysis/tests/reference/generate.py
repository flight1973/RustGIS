# /// script
# requires-python = ">=3.11"
# dependencies = ["shapely==2.1.*", "pyproj==3.7.*"]
# ///
"""Writes the reference layers and the answers RustGIS's tools must give for them.

The answers come from GEOS (through Shapely), the geometry engine of QGIS and PostGIS. Run it
from this directory with `uv run generate.py`; the Rust test `reference.rs` reads what it writes.

Overlays and spatial predicates are worked out in Web Mercator, the plane RustGIS's tools use, so
the comparison tests the geometry engine rather than the choice of plane. Distances are on the
WGS 84 ellipsoid, as RustGIS measures them: GeographicLib (through pyproj) gives the distance
between two points, and the nearest point of an edge, the geodesic between its vertices, is found
by searching along it (and checked against dense sampling).
"""

import json
import math
from pathlib import Path

import pyproj
import shapely
from shapely.geometry import LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon, box, mapping
from pyproj import Geod
from shapely.ops import transform, unary_union

HERE = Path(__file__).parent
MERC_R = 6378137.0
MAX_LAT = 85.051_128_779_806_59
GEOD = Geod(ellps="WGS84")


# --- the layers -------------------------------------------------------------------------------
# Coordinates are multiples of 1/16°, so "on the edge" is exactly on the edge in either plane.

def ring(*pts):
    return list(pts)


PARCELS = [
    ("P1", "A", box(10, 46, 10.5, 46.5)),
    # Clockwise, unlike the others: tools must not depend on the winding.
    ("P2", "A", Polygon([(10.5, 46), (10.5, 46.5), (11, 46.5), (11, 46), (10.5, 46)])),
    ("P3", "B", Polygon(box(11.25, 46, 12, 46.75).exterior.coords, [box(11.5, 46.25, 11.75, 46.5).exterior.coords])),
    ("P4", "B", MultiPolygon([box(10, 47, 10.25, 47.25), box(10.5, 47, 10.75, 47.25)])),
    ("P5", "C", Polygon([(12.5, 46), (13.25, 46), (13.25, 46.25), (12.75, 46.25), (12.75, 46.75), (12.5, 46.75)])),
    ("P6", "A", box(10.25, 46.25, 10.75, 46.75)),
    ("P7", "C", box(12, 46.75, 12.25, 47)),
    ("P8", "D", Polygon([(11, 47), (11.75, 47), (11.25, 47.5)])),
]
ZONES = [
    ("Z1", box(10.25, 45.75, 11.5, 46.375)),
    ("Z2", box(9.75, 46.875, 10.5, 47.5)),
    ("Z3", box(13.5, 46, 14, 46.5)),
    ("Z4", box(11.5, 46.25, 11.75, 46.5)),
    ("Z5", box(11.125, 47.125, 11.5, 47.75)),
    ("Z6", box(10.875, 46.125, 11.375, 46.625)),
]
POINTS = [
    ("a", Point(10.125, 46.125)),
    ("b", Point(10.5, 46.125)),
    ("c", Point(10.5, 46.5)),
    ("d", Point(11.625, 46.375)),
    ("e", Point(11.5, 46.375)),
    ("f", Point(14.5, 48)),
    ("g", Point(12.625, 46.125)),
    ("h", Point(12.625, 46.125)),
    ("i", Point(12, 46.75)),
    ("j", Point(11.25, 47.25)),
]
ROADS = [
    ("r1", LineString([(9.875, 46.25), (11.125, 46.25)])),
    ("r2", LineString([(10.5, 46.0625), (10.5, 46.4375)])),
    ("r3", LineString([(11.5625, 46.3125), (11.6875, 46.4375)])),
    ("r4", LineString([(12, 46.5), (12.5, 46.5)])),
    ("r5", LineString([(15, 49), (15.5, 49.5)])),
    ("r6", LineString([(10.875, 46.875), (11.875, 47.625)])),
    # Along Z1's top edge, and on into Z6.
    ("r7", LineString([(10.25, 46.375), (11.125, 46.375)])),
]
STATIONS = [
    ("s1", Point(10.75, 46.0)),
    ("s2", Point(12.25, 47.25)),
]


def write_layer(name, rows, fields):
    feats = [
        {"type": "Feature", "properties": dict(zip(fields, row[:-1])), "geometry": mapping(row[-1])}
        for row in rows
    ]
    (HERE / "layers").mkdir(exist_ok=True)
    text = json.dumps({"type": "FeatureCollection", "features": feats}, indent=1)
    (HERE / "layers" / f"{name}.geojson").write_text(text + "\n")


# --- planes and distances ---------------------------------------------------------------------

def to_merc(x, y, z=None):
    lat = min(max(y, -MAX_LAT), MAX_LAT)
    return MERC_R * math.radians(x), MERC_R * math.log(math.tan(math.pi / 4 + math.radians(lat) / 2))


def to_lonlat(x, y, z=None):
    return math.degrees(x / MERC_R), math.degrees(2 * math.atan(math.exp(y / MERC_R)) - math.pi / 2)


def m(g):
    return transform(to_merc, g)


def ll(g):
    return transform(to_lonlat, g)


def geodesic_m(p, q):
    """Distance on WGS 84 between two lon/lat points."""
    return GEOD.inv(p[0], p[1], q[0], q[1], return_back_azimuth=True)[2]


def point_arc_m(p, a, b):
    """Distance on WGS 84 from point p to the geodesic a→b (lon/lat pairs): its nearest point is
    bracketed by sampling along the geodesic, then found by golden-section search."""
    az, _, length = GEOD.inv(a[0], a[1], b[0], b[1], return_back_azimuth=True)
    if length == 0:
        return geodesic_m(p, a)

    def at(s):
        lon, lat, _ = GEOD.fwd(a[0], a[1], az, s)
        return geodesic_m(p, (lon, lat))

    n = 400
    ds = [at(length * k / n) for k in range(n + 1)]
    k = min(range(n + 1), key=ds.__getitem__)
    lo, hi = length * max(k - 1, 0) / n, length * min(k + 1, n) / n
    g = (math.sqrt(5) - 1) / 2
    while hi - lo > 1e-6:
        x1, x2 = hi - g * (hi - lo), lo + g * (hi - lo)
        if at(x1) <= at(x2):
            hi = x2
        else:
            lo = x1
    return min(at((lo + hi) / 2), ds[0], ds[-1])


def segments(g):
    if isinstance(g, (Polygon, MultiPolygon)):
        polys = [g] if isinstance(g, Polygon) else list(g.geoms)
        for poly in polys:
            for r in [poly.exterior, *poly.interiors]:
                cs = list(r.coords)
                yield from zip(cs, cs[1:])
    elif isinstance(g, (LineString, MultiLineString)):
        lines = [g] if isinstance(g, LineString) else list(g.geoms)
        for line in lines:
            cs = list(line.coords)
            yield from zip(cs, cs[1:])


def point_distance_m(p, g):
    """Ground distance from point p to g: zero when they intersect (in the tools' plane)."""
    if isinstance(g, Point):
        return geodesic_m((p.x, p.y), (g.x, g.y))
    if m(g).intersects(m(p)):
        return 0.0
    return min(point_arc_m((p.x, p.y), a, b) for a, b in segments(g))


def sampled_distance_m(p, g, step_m=2.0):
    """The same distance by sampling every edge's geodesic densely: a check on point_distance_m."""
    best = math.inf
    for a, b in segments(g):
        pts = GEOD.inv_intermediate(a[0], a[1], b[0], b[1], del_s=step_m, initial_idx=0, terminus_idx=0, return_back_azimuth=True)
        lons, lats = list(pts.lons), list(pts.lats)
        _, _, d = GEOD.inv([p.x] * len(lons), [p.y] * len(lats), lons, lats, return_back_azimuth=True)
        best = min(best, *d)
    return best


# --- the answers ------------------------------------------------------------------------------

def polygonal(g):
    parts = [p for p in shapely.get_parts(g) if isinstance(p, (Polygon, MultiPolygon)) and p.area > 0]
    return unary_union(parts) if parts else None


def linear(g):
    parts = [p for p in shapely.get_parts(shapely.line_merge(g) if g.geom_type.endswith("LineString") else g)
             if isinstance(p, (LineString, MultiLineString)) and p.length > 0]
    return shapely.line_merge(MultiLineString([q for p in parts for q in shapely.get_parts(p)])) if parts else None


def write_expected(name, rows):
    feats = [{"type": "Feature", "properties": {"id": i}, "geometry": mapping(g)} for i, g in rows]
    (HERE / "expected").mkdir(exist_ok=True)
    text = json.dumps({"type": "FeatureCollection", "features": feats}, indent=1)
    (HERE / "expected" / f"{name}.geojson").write_text(text + "\n")
    return f"expected/{name}.geojson"


def overlay(name, rows, op):
    out = []
    for i, g in rows:
        r = op(m(g))
        if r is not None and not r.is_empty:
            out.append((i, ll(r)))
    return write_expected(name, out)


def main():
    write_layer("parcels", PARCELS, ["id", "code"])
    write_layer("zones", ZONES, ["id"])
    write_layer("points", POINTS, ["id"])
    write_layer("roads", ROADS, ["id"])
    write_layer("stations", STATIONS, ["id"])

    zones_m = unary_union([m(g) for _, g in ZONES])
    parcels = [(i, g) for i, _, g in PARCELS]
    cases = []

    def case(name, tool, params, expect):
        cases.append({"name": name, "tool": tool, "params": params, "expect": expect})

    # Clip and Erase: each input feature's part inside (outside) the union of the zones; features
    # left with nothing (or only a shared edge or corner) are dropped.
    case("clip polygons", "clip", {"input": "parcels", "clip": "zones"},
         {"features": overlay("clip_parcels", parcels, lambda g: polygonal(g.intersection(zones_m)))})
    case("clip lines", "clip", {"input": "roads", "clip": "zones"},
         {"features": overlay("clip_roads", ROADS, lambda g: linear(g.intersection(zones_m)))})
    case("clip points", "clip", {"input": "points", "clip": "zones"},
         {"ids": [i for i, g in POINTS if zones_m.intersects(m(g))]})
    case("erase", "erase", {"input": "parcels", "erase": "zones"},
         {"features": overlay("erase_parcels", parcels, lambda g: polygonal(g.difference(zones_m)))})

    # Intersect: each input feature's part in each zone, keyed by both ids ("P1|Z1"), in input
    # order, then zone order. Parts that only touch (a shared edge or corner) are left out. The
    # zones' `id` is taken by the parcels', so it comes out as `id_2`.
    # Identity adds each input feature's part outside the zones it has a part in, keyed "P1|" (no
    # zone id); Union adds each zone's part outside the parcels that have a part in it, "|Z1".
    def pieces(name, rows, part, rest=False, zone_rest=False):
        out, covering = [], {zi: [] for zi, _ in ZONES}
        for i, g in rows:
            hit = []
            for zi, z in ZONES:
                r = part(m(g).intersection(m(z)))
                if r is not None and not r.is_empty:
                    out.append((f"{i}|{zi}", ll(r)))
                    hit.append(m(z))
                    covering[zi].append(m(g))
            if rest:
                r = part(m(g).difference(unary_union(hit))) if hit else m(g)
                if r is not None and not r.is_empty:
                    out.append((f"{i}|", ll(r)))
        if zone_rest:
            for zi, z in ZONES:
                r = polygonal(m(z).difference(unary_union(covering[zi]))) if covering[zi] else m(z)
                if r is not None and not r.is_empty:
                    out.append((f"|{zi}", ll(r)))
        return write_expected(name, out)

    both = ["id", "id_2"]
    case("intersect polygons", "intersect", {"input": "parcels", "overlay": "zones"},
         {"features": pieces("intersect_parcels", parcels, polygonal), "key": both})
    case("intersect lines", "intersect", {"input": "roads", "overlay": "zones"},
         {"features": pieces("intersect_roads", ROADS, linear), "key": both})
    case("intersect points", "intersect", {"input": "points", "overlay": "zones"},
         {"ids": [f"{i}|{zi}" for i, g in POINTS for zi, z in ZONES if m(z).intersects(m(g))], "key": both})
    case("identity polygons", "identity", {"input": "parcels", "overlay": "zones"},
         {"features": pieces("identity_parcels", parcels, polygonal, rest=True), "key": both})
    case("identity lines", "identity", {"input": "roads", "overlay": "zones"},
         {"features": pieces("identity_roads", ROADS, linear, rest=True), "key": both})
    ids = []
    for i, g in POINTS:
        hits = [f"{i}|{zi}" for zi, z in ZONES if m(z).intersects(m(g))]
        ids += hits or [f"{i}|"]
    case("identity points", "identity", {"input": "points", "overlay": "zones"}, {"ids": ids, "key": both})
    case("union", "union", {"input": "parcels", "overlay": "zones"},
         {"features": pieces("union_parcels", parcels, polygonal, rest=True, zone_rest=True), "key": both})

    # Dissolve: one feature per code, the union of its parcels.
    codes = sorted({c for _, c, _ in PARCELS})
    rows = [(c, ll(unary_union([m(g) for _, k, g in PARCELS if k == c]))) for c in codes]
    case("dissolve by field", "dissolve", {"input": "parcels", "field": "code"},
         {"features": write_expected("dissolve_code", rows), "key": "code",
          "counts": {c: sum(1 for _, k, _ in PARCELS if k == c) for c in codes}})
    case("dissolve all", "dissolve", {"input": "parcels"},
         {"features": write_expected("dissolve_all", [("all", ll(unary_union([m(g) for _, g in parcels])))]),
          "key": None})

    # Minimum Bounding Geometry: convex hulls in the tools' plane.
    case("convex hull of each", "convexHull", {"input": "parcels", "group": "None"},
         {"features": overlay("hull_each", parcels, lambda g: g.convex_hull)})
    case("convex hull of all", "convexHull", {"input": "parcels", "group": "All"},
         {"features": write_expected("hull_all", [("all", ll(MultiPolygon([m(g) for _, g in parcels if g.geom_type == "Polygon"]
                                                                                  + [p for _, g in parcels if g.geom_type == "MultiPolygon" for p in m(g).geoms]).convex_hull))]),
          "key": None})

    # Select Layer By Location: features that intersect any selecting feature (edges count).
    layers = {"parcels": parcels, "zones": ZONES, "points": POINTS, "roads": ROADS}
    for inp, sel in [("parcels", "zones"), ("roads", "parcels"), ("points", "parcels"), ("roads", "zones")]:
        sm = [m(g) for _, g in layers[sel]]
        hit = [i for i, g in layers[inp] if any(s.intersects(m(g)) for s in sm)]
        case(f"select {inp} by {sel}", "selectByLocation", {"input": inp, "select": sel}, {"ids": hit})
        if inp == "parcels":
            miss = [i for i, _ in layers[inp] if i not in hit]
            case(f"select {inp} by {sel}, inverted", "selectByLocation", {"input": inp, "select": sel, "invert": "Yes"},
                 {"ids": miss})

    # Summarize Within: points that intersect each polygon (edges count; duplicates count twice).
    case("summarize within", "countPoints", {"polygons": "parcels", "points": "points"},
         {"field": "point_count", "values": {i: sum(1 for _, p in POINTS if m(g).intersects(m(p))) for i, g in parcels}})

    # Spatial Join, one to many, keeping every target: the set of (target, join) pairs.
    def pairs(targets, joins, rel):
        out = []
        for ti, tg in targets:
            hits = [ji for ji, jg in joins if rel(m(tg), m(jg))]
            out += [[ti, ji] for ji in hits] or [[ti, None]]
        return out

    many = {"operation": "Join one to many"}
    case("join intersects", "spatialJoin", {"target": "parcels", "join": "zones", "match": "Intersects", **many},
         {"pairs": pairs(parcels, ZONES, lambda t, j: t.intersects(j))})
    case("join contains", "spatialJoin", {"target": "parcels", "join": "points", "match": "Contains", **many},
         {"pairs": pairs(parcels, POINTS, lambda t, j: t.contains(j))})
    case("join within", "spatialJoin", {"target": "points", "join": "parcels", "match": "Within", **many},
         {"pairs": pairs(POINTS, parcels, lambda t, j: t.within(j))})
    case("join roads intersect parcels", "spatialJoin", {"target": "roads", "join": "parcels", "match": "Intersects", **many},
         {"pairs": pairs(ROADS, parcels, lambda t, j: t.intersects(j))})

    # Closest and Within a distance: ground distances, checked against dense sampling.
    def dist(p, g):
        d = point_distance_m(p, g)
        if d > 0:
            # Sampling every 2 m overestimates by at most about step² / 8d.
            s = sampled_distance_m(p, g)
            assert -1e-6 < s - d < 1e-3 + 0.5 / d, (p, d, s)
        return d

    rows = []
    for ti, tp in POINTS:
        ds = [(dist(tp, g), ji) for ji, g in parcels]
        best = min(d for d, _ in ds)
        ji = next(j for d, j in ds if d == best)  # ties keep the join layer's order
        rows.append([ti, ji, best])
    case("join closest", "spatialJoin",
         {"target": "points", "join": "parcels", "match": "Closest", "units": "Meters", "distanceField": "dist", **many},
         {"pairs": rows})

    limit = 30_000.0
    rows = []
    for ti, tp in STATIONS + POINTS:
        ds = sorted((dist(tp, g), ji) for ji, g in ROADS)
        assert all(abs(d - limit) > 100 for d, _ in ds), "a distance too near the limit"
        hits = [[ti, ji, d] for d, ji in ds if d <= limit]
        rows += hits or [[ti, None, None]]
    case("join within a distance", "spatialJoin",
         {"target": "stations", "join": "roads", "match": "Within a distance", "distance": limit, "units": "Meters",
          "distanceField": "dist", **many},
         {"pairs": [r for r in rows if r[0].startswith("s")]})
    case("join points within a distance", "spatialJoin",
         {"target": "points", "join": "roads", "match": "Within a distance", "distance": limit, "units": "Meters",
          "distanceField": "dist", **many},
         {"pairs": [r for r in rows if not r[0].startswith("s")]})

    meta = {
        "about": "Answers from GEOS; regenerate with `uv run generate.py` (see its header).",
        "shapely": shapely.__version__,
        "geos": shapely.geos_version_string,
        "pyproj": pyproj.__version__,
        "proj": pyproj.proj_version_str,
        "cases": cases,
    }
    (HERE / "cases.json").write_text(json.dumps(meta, indent=1) + "\n")
    print(f"{len(cases)} cases")


if __name__ == "__main__":
    main()
