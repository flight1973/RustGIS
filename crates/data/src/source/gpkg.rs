//! GeoPackage: an SQLite database of feature tables, read with SQLite (desktop only).
//!
//! Extent queries use the table's R-tree spatial index (`rtree_<table>_<column>`) when it has one,
//! and otherwise scan, testing the envelope in each geometry's header before decoding it.

use super::gpkg_blob::{field_type, is_empty_blob, split_blob};
use super::{Batch, FeatureSource, Reproject, Request, Sampler, SourceInfo, clamp_limit, in_request, stride};
use crate::raster::Crs;
use crate::{DataError, Feature, Field, FieldType, GeometryType, Result, Value, wkb};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, params_from_iter};
use rustgis_geom::Extent;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub mod edit;

/// Rows fetched by id per query.
const CHUNK: usize = 500;

/// A feature table in a GeoPackage.
pub struct GpkgSource {
    info: SourceInfo,
    /// `None` only while the source is dropped.
    conn: Mutex<Option<Connection>>,
    table: String,
    geom: String,
    pk: Option<String>,
    rtree: Option<String>,
    reproject: Reproject,
    /// A copy written for a file opened from memory, removed when the source is dropped.
    temp: Option<PathBuf>,
}

impl Drop for GpkgSource {
    fn drop(&mut self) {
        // Closed first: Windows can't remove a file SQLite still holds open.
        if let Ok(c) = self.conn.get_mut() {
            drop(c.take());
        }
        if let Some(p) = &self.temp {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// The feature tables of a GeoPackage.
pub fn layers(path: &Path) -> Result<Vec<String>> {
    let conn = open_db(path)?;
    feature_tables(&conn)
}

fn open_db(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(err)?;
    // A cap on SQLite's page cache, so a huge database can't take all the memory; and the file is
    // hostile: nothing it defines (views, triggers, generated columns) may call functions, and
    // nothing writes.
    let _ = conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false);
    let _ = conn.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true);
    let _ = conn.execute_batch("PRAGMA cache_size = -65536; PRAGMA query_only = ON;");
    Ok(conn)
}

fn feature_tables(conn: &Connection) -> Result<Vec<String>> {
    let mut st = conn.prepare("SELECT table_name FROM gpkg_geometry_columns ORDER BY table_name").map_err(not_gpkg)?;
    let rows = st.query_map([], |r| r.get::<_, String>(0)).map_err(err)?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

impl GpkgSource {
    pub fn open(path: &Path, layer: Option<&str>) -> Result<Self> {
        Self::open_conn(open_db(path)?, layer, None)
    }

    /// Opens a GeoPackage held in memory, through a temporary copy (SQLite reads files). The copy
    /// is a new file with a name no one can guess, so another user can't plant one in its place.
    pub fn open_bytes(bytes: &[u8], layer: Option<&str>) -> Result<Self> {
        use std::io::Write;
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let mut tries = 0;
        let (p, mut f) = loop {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            let salt = std::collections::hash_map::RandomState::new();
            let tag = std::hash::BuildHasher::hash_one(&salt, (nanos, N.fetch_add(1, Ordering::Relaxed)));
            let p = std::env::temp_dir().join(format!("rustgis-{}-{tag:016x}.gpkg", std::process::id()));
            let mut o = std::fs::OpenOptions::new();
            o.write(true).create_new(true);
            // Readable by this user only: the copy holds their data, in a shared folder.
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
            match o.open(&p) {
                Ok(f) => break (p, f),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && tries < 8 => tries += 1,
                Err(e) => return Err(e.into()),
            }
        };
        if let Err(e) = f.write_all(bytes).and_then(|_| f.sync_all()) {
            let _ = std::fs::remove_file(&p);
            return Err(e.into());
        }
        drop(f);
        match open_db(&p).and_then(|c| Self::open_conn(c, layer, Some(p.clone()))) {
            Ok(s) => Ok(s),
            Err(e) => {
                let _ = std::fs::remove_file(&p);
                Err(e)
            }
        }
    }

    fn open_conn(conn: Connection, layer: Option<&str>, temp: Option<PathBuf>) -> Result<Self> {
        let tables = feature_tables(&conn)?;
        let table = match layer {
            Some(l) => tables
                .iter()
                .find(|t| t.eq_ignore_ascii_case(l))
                .cloned()
                .ok_or_else(|| DataError::Format(format!("no feature table '{l}' (tables: {})", tables.join(", "))))?,
            None => tables.first().cloned().ok_or_else(|| DataError::Format("the GeoPackage has no feature tables".into()))?,
        };
        let mut notes = Vec::new();
        if layer.is_none() && tables.len() > 1 {
            notes.push(format!("the first of {} feature tables ({})", tables.len(), tables.join(", ")));
        }
        let (geom, geom_type, srs_id): (String, String, i64) = conn
            .query_row("SELECT column_name, geometry_type_name, srs_id FROM gpkg_geometry_columns WHERE table_name = ?1", [&table], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(err)?;
        let epsg: Option<u32> = conn
            .query_row("SELECT organization, organization_coordsys_id FROM gpkg_spatial_ref_sys WHERE srs_id = ?1", [srs_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })
            .ok()
            .map_or_else(
                // No row for it: the srs_id is taken as an EPSG code only for WGS 84 and Web
                // Mercator, the ids writers commonly use without defining.
                || u32::try_from(srs_id).ok().filter(|c| matches!(c, 4326 | 3857)),
                // A row: an EPSG code only when the organization is EPSG (a table-local id such as
                // 100001 isn't one).
                |(org, id)| if org.eq_ignore_ascii_case("epsg") { u32::try_from(id).ok() } else { None },
            );
        if epsg.is_none() && srs_id > 0 && srs_id != 4326 {
            notes.push(format!("its coordinate system (srs_id {srs_id}) has no EPSG code; read as longitude/latitude"));
        }
        let reproject = Reproject::from_epsg(epsg, &mut notes);

        // Columns: the primary key, the geometry and the attribute fields.
        let mut pk = None;
        let mut fields = Vec::new();
        {
            let mut st = conn.prepare(&format!("PRAGMA table_info({})", quote(&table))).map_err(err)?;
            let cols = st
                .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?.unwrap_or_default(), r.get::<_, i64>(5)?)))
                .map_err(err)?;
            for c in cols.filter_map(std::result::Result::ok) {
                let (name, decl, is_pk) = c;
                if name.eq_ignore_ascii_case(&geom) {
                    continue;
                }
                if is_pk == 1 && pk.is_none() && decl.to_ascii_uppercase().contains("INT") {
                    pk = Some(name);
                    continue;
                }
                if let Some(ty) = field_type(&decl) {
                    fields.push(Field::new(name, ty));
                }
            }
        }
        // Looked up without regard to case, as SQLite names are; the real name is kept.
        let rtree_name = format!("rtree_{table}_{geom}");
        let rtree: Option<String> = conn
            .query_row("SELECT name FROM sqlite_master WHERE lower(name) = lower(?1) AND type = 'table'", [&rtree_name], |r| r.get(0))
            .ok()
            .filter(|_| pk.is_some());
        let declared: Option<Extent> = conn
            .query_row("SELECT min_x, min_y, max_x, max_y FROM gpkg_contents WHERE table_name = ?1", [&table], |r| {
                Ok((r.get::<_, Option<f64>>(0)?, r.get::<_, Option<f64>>(1)?, r.get::<_, Option<f64>>(2)?, r.get::<_, Option<f64>>(3)?))
            })
            .ok()
            .and_then(|e| match e {
                (Some(a), Some(b), Some(c), Some(d)) => Some(Extent::new(a, b, c, d)),
                _ => None,
            });
        let native = declared.filter(|e| !e.is_empty()).or_else(|| {
            let rt = rtree.as_ref()?;
            conn.query_row(&format!("SELECT min(minx), min(miny), max(maxx), max(maxy) FROM {}", quote(rt)), [], |r| {
                Ok(Extent::new(r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .ok()
        });
        let extent = native.map(|e| reproject.extent_to_lonlat(&e)).unwrap_or(Extent::EMPTY);
        let count = conn
            .query_row(&format!("SELECT COUNT(*) FROM {}", quote(&table)), [], |r| r.get::<_, i64>(0))
            .ok()
            .and_then(|n| u64::try_from(n).ok());
        let geometry_type = match geom_type.to_ascii_uppercase().as_str() {
            "POINT" | "MULTIPOINT" => GeometryType::Point,
            "LINESTRING" | "MULTILINESTRING" | "CURVE" | "MULTICURVE" => GeometryType::Polyline,
            "POLYGON" | "MULTIPOLYGON" | "SURFACE" | "MULTISURFACE" => GeometryType::Polygon,
            _ => GeometryType::None,
        };
        let info = SourceInfo {
            name: table.clone(),
            format: "GeoPackage",
            geometry_type,
            fields,
            extent,
            count,
            crs: reproject.label(),
            indexed: rtree.is_some(),
            notes,
        };
        let mut src = GpkgSource { info, conn: Mutex::new(Some(conn)), table, geom, pk, rtree, reproject, temp };
        if src.info.geometry_type == GeometryType::None {
            src.info.geometry_type = src.peek_kind();
        }
        Ok(src)
    }

    /// The geometry type of a table declared as plain `GEOMETRY`, from its first few rows (a
    /// query that stops there, rather than a read of the table).
    fn peek_kind(&self) -> GeometryType {
        let Ok(guard) = self.conn.lock() else { return GeometryType::None };
        let Some(conn) = guard.as_ref() else { return GeometryType::None };
        let sql = format!("SELECT {g} FROM {t} WHERE {g} IS NOT NULL LIMIT 16", g = quote(&self.geom), t = quote(&self.table));
        let Ok(mut st) = conn.prepare(&sql) else { return GeometryType::None };
        let Ok(mut rows) = st.query([]) else { return GeometryType::None };
        while let Ok(Some(r)) = rows.next() {
            if let Ok(ValueRef::Blob(b)) = r.get_ref(0)
                && let Some((_, body)) = split_blob(b)
                && let Some(g) = wkb::decode(body)
            {
                return g.kind();
            }
        }
        GeometryType::None
    }

    fn select_list(&self) -> String {
        let mut cols = vec![self.pk.as_deref().map(quote).unwrap_or_else(|| "rowid".into()), quote(&self.geom)];
        cols.extend(self.info.fields.iter().map(|f| quote(&f.name)));
        cols.join(", ")
    }

    /// Turns a row (id, geometry, fields…) into a feature; `None` for an unreadable geometry.
    fn feature(&self, r: &rusqlite::Row<'_>, native: Option<&Extent>, req: &Request) -> rusqlite::Result<Row> {
        // Integer keys, negative ones too, map to distinct ids.
        let Some(id) = r.get::<_, Option<i64>>(0)?.map(|i| i as u64) else { return Ok(Row::Bad) };
        let blob = match r.get_ref(1)? {
            ValueRef::Blob(b) => b,
            ValueRef::Null => return Ok(Row::Skip),
            _ => return Ok(Row::Bad),
        };
        if is_empty_blob(blob) {
            return Ok(Row::Skip);
        }
        let Some((envelope, body)) = split_blob(blob) else { return Ok(Row::Bad) };
        if let (Some(q), Some(e)) = (native, envelope)
            && !e.intersects(q)
        {
            return Ok(Row::Skip);
        }
        let Some(geometry) = wkb::decode(body).and_then(|g| self.reproject.geometry(g)) else { return Ok(Row::Bad) };
        if !in_request(&geometry, req) {
            return Ok(Row::Skip);
        }
        let attrs =
            self.info.fields.iter().enumerate().map(|(i, f)| r.get_ref(i + 2).map(|v| value(v, f.ty)).unwrap_or(Value::Null)).collect();
        Ok(Row::Feature(Feature { id, geometry: Some(geometry), attrs }))
    }
}

enum Row {
    Feature(Feature),
    /// Outside the request, or without geometry.
    Skip,
    /// Unreadable.
    Bad,
}

impl FeatureSource for GpkgSource {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn read(&self, req: &Request) -> Result<Batch> {
        let limit = clamp_limit(req.limit);
        let guard = self.conn.lock().map_err(|_| DataError::Format("GeoPackage connection poisoned".into()))?;
        let conn = guard.as_ref().ok_or_else(|| DataError::Format("GeoPackage connection closed".into()))?;
        let native = req.extent.map(|e| self.reproject.extent_from_lonlat(&e));
        let mut out = Batch::default();
        let push = |row: Row, out: &mut Batch| match row {
            Row::Feature(f) => out.features.push(f),
            Row::Skip => {}
            Row::Bad => out.skipped += 1,
        };
        match (native, &self.rtree, &self.pk) {
            (Some(e), Some(rt), Some(pk)) => {
                // The index gives the matching ids; an evenly spread sample of them is fetched.
                // Counted first, then streamed keeping every `step`-th, so a huge match never sits
                // in memory as a list of ids.
                let filter = format!("FROM {} WHERE maxx >= ?1 AND minx <= ?2 AND maxy >= ?3 AND miny <= ?4", quote(rt));
                let args = [e.min_x, e.max_x, e.min_y, e.max_y];
                let n: i64 = conn.query_row(&format!("SELECT COUNT(*) {filter}"), args, |r| r.get(0)).map_err(err)?;
                out.matched = u64::try_from(n).unwrap_or(0);
                let step = usize::try_from(stride(out.matched, limit)).unwrap_or(usize::MAX);
                let mut st = conn.prepare(&format!("SELECT id {filter}")).map_err(err)?;
                // A row error (a corrupt page) fails the read rather than truncating it quietly.
                let picked: Vec<i64> = st
                    .query_map(args, |r| r.get(0))
                    .map_err(err)?
                    .step_by(step.max(1))
                    .take(limit)
                    .collect::<rusqlite::Result<_>>()
                    .map_err(err)?;
                out.complete = step == 1;
                for chunk in picked.chunks(CHUNK) {
                    let marks = vec!["?"; chunk.len()].join(",");
                    let sql = format!("SELECT {} FROM {} WHERE {} IN ({marks})", self.select_list(), quote(&self.table), quote(pk));
                    let mut st = conn.prepare(&sql).map_err(err)?;
                    let mut rows = st.query(params_from_iter(chunk.iter())).map_err(err)?;
                    while let Some(r) = rows.next().map_err(err)? {
                        push(self.feature(r, native.as_ref(), req).map_err(err)?, &mut out);
                    }
                    if out.features.len() >= limit {
                        out.features.truncate(limit);
                        out.complete = false;
                        break;
                    }
                }
            }
            _ => {
                // A scan: the envelope in each geometry's header tests the extent before decoding.
                let sql = format!("SELECT {} FROM {}", self.select_list(), quote(&self.table));
                let mut st = conn.prepare(&sql).map_err(err)?;
                let mut rows = st.query([]).map_err(err)?;
                let mut sample =
                    if native.is_none() { Sampler::with_total(limit, self.info.count.unwrap_or(0)) } else { Sampler::new(limit) };
                while let Some(r) = rows.next().map_err(err)? {
                    // With an extent, the envelope in the geometry's header says whether the row
                    // matches before anything is decoded; without an envelope, only the decoded
                    // geometry can, so the match is counted after it.
                    let envelope = match (&native, r.get_ref(1)) {
                        (Some(_), Ok(ValueRef::Blob(b))) => split_blob(b).and_then(|(e, _)| e),
                        _ => None,
                    };
                    if let (Some(q), Some(e)) = (&native, envelope)
                        && !e.intersects(q)
                    {
                        continue;
                    }
                    let known = native.is_none() || envelope.is_some();
                    if known && !sample.wants() {
                        continue;
                    }
                    match self.feature(r, native.as_ref(), req).map_err(err)? {
                        Row::Feature(f) if known || sample.wants() => sample.keep(f),
                        Row::Feature(_) | Row::Skip => {}
                        Row::Bad => out.skipped += 1,
                    }
                }
                out.matched = sample.seen();
                out.complete = sample.complete();
                out.features = sample.items;
            }
        }
        Ok(out)
    }
}

/// WGS 84 as OGC WKT, the coordinate system GeoPackages are written in.
const WGS84_WKT: &str = "GEOGCS[\"WGS 84\",DATUM[\"WGS_1984\",SPHEROID[\"WGS 84\",6378137,298.257223563,AUTHORITY[\"EPSG\",\"7030\"]],AUTHORITY[\"EPSG\",\"6326\"]],PRIMEM[\"Greenwich\",0,AUTHORITY[\"EPSG\",\"8901\"]],UNIT[\"degree\",0.0174532925199433,AUTHORITY[\"EPSG\",\"9122\"]],AXIS[\"Latitude\",NORTH],AXIS[\"Longitude\",EAST],AUTHORITY[\"EPSG\",\"4326\"]]";

/// A geometry as a GeoPackage blob: the `GP` header (little-endian, WGS 84, an XY envelope) and
/// the WKB. Points of a multipoint layer are written as one-point multipoints.
fn blob(g: &crate::Geometry, multi_points: bool, srs_id: i32) -> Vec<u8> {
    let g = match g {
        crate::Geometry::Point(c) if multi_points => &crate::Geometry::MultiPoint(vec![*c]),
        g => g,
    };
    header_blob(&wkb::encode(g), &g.extent(), srs_id)
}

fn header_blob(wkb: &[u8], e: &Extent, srs_id: i32) -> Vec<u8> {
    let mut out = Vec::with_capacity(40 + wkb.len());
    out.extend_from_slice(b"GP");
    out.push(0);
    if e.is_empty() {
        // No coordinates: flagged empty, without an envelope.
        out.push(0b1_0001);
        out.extend_from_slice(&srs_id.to_le_bytes());
    } else {
        out.push(0b0000_0011);
        out.extend_from_slice(&srs_id.to_le_bytes());
        for v in [e.min_x, e.max_x, e.min_y, e.max_y] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out.extend_from_slice(wkb);
    out
}

/// A name not taken by a field (`base`, else `base_1`, `base_2`, …).
fn free_name(fields: &[Field], base: &str) -> String {
    let taken = |n: &str| fields.iter().any(|f| f.name.eq_ignore_ascii_case(n));
    (0..).map(|i| if i == 0 { base.to_string() } else { format!("{base}_{i}") }).find(|n| !taken(n)).unwrap_or_else(|| base.to_string())
}

/// Writes a dataset as a GeoPackage with one table (WGS 84, with an R-tree spatial index), or an
/// attributes table for a dataset without geometry. The destination must be new: an existing
/// GeoPackage may contain other tables. The file is built beside `path` and installed without
/// replacing anything, so a failed write leaves existing data untouched.
pub fn write(ds: &crate::Dataset, path: &Path) -> Result<()> {
    write_in(ds, path, Crs::LonLat)
}

/// Writes a dataset whose geometry is in `crs`'s coordinates as a new GeoPackage in that system.
/// Existing destinations are refused, as with [`write`].
pub fn write_in(ds: &crate::Dataset, path: &Path, crs: Crs) -> Result<()> {
    check_new_destination(path)?;
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    // Drop the file handle before SQLite opens it, including on Windows. TempPath cleans up on
    // every error, and persist_noclobber refuses files created after the check above as well.
    let mut builder = tempfile::Builder::new();
    builder.prefix(".rustgis-gpkg-").suffix(".tmp");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Match ordinary exports: request 0666 and let the user's umask restrict it.
        builder.permissions(std::fs::Permissions::from_mode(0o666));
    }
    let tmp = builder.tempfile_in(parent)?.into_temp_path();
    let conn = Connection::open(&tmp).map_err(err)?;
    build(&conn, ds, crs)?;
    conn.close().map_err(|(_, e)| err(e))?;
    // Built without syncing (fast); synced once before installation. Windows needs a write handle.
    std::fs::OpenOptions::new().write(true).open(&tmp)?.sync_all()?;
    // Refuse journals that appeared during the build; persistence also closes the file race.
    check_new_destination(path)?;
    tmp.persist_noclobber(path).map_err(|e| destination_error(path, e.error))?;
    Ok(())
}

/// Refuses existing files, including dangling symlinks and journals left without their database.
fn check_new_destination(path: &Path) -> Result<()> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut entry = path.as_os_str().to_owned();
        entry.push(suffix);
        match std::fs::symlink_metadata(Path::new(&entry)) {
            Ok(_) => return Err(destination_error(path, std::io::ErrorKind::AlreadyExists.into())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn destination_error(path: &Path, error: std::io::Error) -> DataError {
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "GeoPackage export cannot replace {} or its SQLite journals. Choose a new filename to preserve existing data.",
                path.display()
            ),
        )
        .into()
    } else {
        error.into()
    }
}

fn build(conn: &Connection, ds: &crate::Dataset, crs: Crs) -> Result<()> {
    use rusqlite::types::Value as Sql;
    // An EPSG system by its code; a custom one numbered as GDAL numbers them.
    let srs_id = match crs.epsg() {
        0 => 100_000,
        c => i32::from(c),
    };
    // Not a reserved name, and never empty.
    let mut table: String = ds.name.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string();
    if table.is_empty() {
        table = "layer".into();
    }
    if table.to_ascii_lowercase().starts_with("gpkg_")
        || table.to_ascii_lowercase().starts_with("sqlite_")
        || table.to_ascii_lowercase().starts_with("rtree_")
    {
        table = format!("layer_{table}");
    }
    let has_geom = ds.geometry_type != GeometryType::None;
    let fid = free_name(&ds.fields, "fid");
    let geom = free_name(&ds.fields, "geom");
    // SQLite column names are unique without regard to case: a later `a` beside `A` becomes `a_1`.
    let mut columns: Vec<String> = Vec::with_capacity(ds.fields.len());
    for f in &ds.fields {
        let taken = |n: &str, cols: &[String]| {
            cols.iter().any(|c| c.eq_ignore_ascii_case(n)) || n.eq_ignore_ascii_case(&fid) || n.eq_ignore_ascii_case(&geom)
        };
        let name = (0..)
            .map(|i| if i == 0 { f.name.clone() } else { format!("{}_{i}", f.name) })
            .find(|n| !taken(n, &columns))
            .unwrap_or_else(|| f.name.clone());
        columns.push(name);
    }
    let multi_points = ds.features.iter().any(|f| matches!(f.geometry, Some(crate::Geometry::MultiPoint(_))));
    let geom_type = match ds.geometry_type {
        GeometryType::Point if multi_points => "MULTIPOINT",
        GeometryType::Point => "POINT",
        GeometryType::Polyline => "MULTILINESTRING",
        GeometryType::Polygon => "MULTIPOLYGON",
        GeometryType::None => "",
    };
    let (t, g, i) = (quote(&table), quote(&geom), quote(&fid));
    let mut sql = String::from(
        "PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;
        PRAGMA application_id = 1196444487; PRAGMA user_version = 10400;
        CREATE TABLE gpkg_spatial_ref_sys (srs_name TEXT NOT NULL, srs_id INTEGER PRIMARY KEY, organization TEXT NOT NULL,
            organization_coordsys_id INTEGER NOT NULL, definition TEXT NOT NULL, description TEXT);
        CREATE TABLE gpkg_contents (table_name TEXT NOT NULL PRIMARY KEY, data_type TEXT NOT NULL, identifier TEXT UNIQUE,
            description TEXT DEFAULT '', last_change DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
            min_x DOUBLE, min_y DOUBLE, max_x DOUBLE, max_y DOUBLE, srs_id INTEGER,
            CONSTRAINT fk_gc_r_srs_id FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys(srs_id));
        CREATE TABLE gpkg_geometry_columns (table_name TEXT NOT NULL, column_name TEXT NOT NULL, geometry_type_name TEXT NOT NULL,
            srs_id INTEGER NOT NULL, z TINYINT NOT NULL, m TINYINT NOT NULL,
            CONSTRAINT pk_geom_cols PRIMARY KEY (table_name, column_name),
            CONSTRAINT uk_gc_table_name UNIQUE (table_name),
            CONSTRAINT fk_gc_tn FOREIGN KEY (table_name) REFERENCES gpkg_contents(table_name),
            CONSTRAINT fk_gc_srs FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys (srs_id));
        CREATE TABLE gpkg_extensions (table_name TEXT, column_name TEXT, extension_name TEXT NOT NULL, definition TEXT NOT NULL,
            scope TEXT NOT NULL, CONSTRAINT ge_tce UNIQUE (table_name, column_name, extension_name));
        BEGIN;",
    );
    let mut cols = vec![format!("{i} INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL")];
    if has_geom {
        cols.push(format!("{g} {geom_type}"));
    }
    for (f, col) in ds.fields.iter().zip(&columns) {
        let ty = match f.ty {
            FieldType::Integer => "INTEGER",
            FieldType::Double => "REAL",
            FieldType::Boolean => "BOOLEAN",
            FieldType::Text => "TEXT",
        };
        cols.push(format!("{} {ty}", quote(col)));
    }
    sql.push_str(&format!("CREATE TABLE {t} ({});", cols.join(", ")));
    let rt = quote(&format!("rtree_{table}_{geom}"));
    if has_geom {
        sql.push_str(&format!("CREATE VIRTUAL TABLE {rt} USING rtree(id, minx, maxx, miny, maxy);"));
    }
    conn.execute_batch(&sql).map_err(err)?;
    conn.execute(
        "INSERT INTO gpkg_spatial_ref_sys VALUES
            ('Undefined cartesian SRS', -1, 'NONE', -1, 'undefined', 'undefined cartesian coordinate reference system'),
            ('Undefined geographic SRS', 0, 'NONE', 0, 'undefined', 'undefined geographic coordinate reference system'),
            ('WGS 84 geodetic', 4326, 'EPSG', 4326, ?1, 'longitude/latitude coordinates in decimal degrees on the WGS 84 spheroid')",
        [WGS84_WKT],
    )
    .map_err(err)?;
    if srs_id != 4326 {
        // GeoPackage readers take an EPSG system by its code; a custom one has only its WKT
        // (`undefined` when it has none).
        let org = if crs.epsg() > 0 { "EPSG" } else { "NONE" };
        let wkt = crate::raster::wkt::to_wkt(crs);
        conn.execute(
            "INSERT INTO gpkg_spatial_ref_sys VALUES (?1, ?2, ?3, ?2, ?4, '')",
            rusqlite::params![crs.label(), srs_id, org, wkt.as_deref().unwrap_or("undefined")],
        )
        .map_err(err)?;
    }

    // Feature ids are kept when they are unique and fit; otherwise SQLite numbers the rows.
    let mut seen = std::collections::HashSet::new();
    let keep_ids = ds.features.iter().all(|f| i64::try_from(f.id).is_ok() && seen.insert(f.id));
    {
        let names: Vec<String> =
            std::iter::once(i.clone()).chain(has_geom.then(|| g.clone())).chain(columns.iter().map(|c| quote(c))).collect();
        let marks = vec!["?"; names.len()].join(", ");
        let mut st = conn.prepare(&format!("INSERT INTO {t} ({}) VALUES ({marks})", names.join(", "))).map_err(err)?;
        // The spatial index is filled here; its triggers (which call functions only GeoPackage
        // software defines) are created after.
        let mut index =
            if has_geom { Some(conn.prepare(&format!("INSERT INTO {rt} VALUES (?1, ?2, ?3, ?4, ?5)")).map_err(err)?) } else { None };
        for f in &ds.features {
            let mut row: Vec<Sql> = Vec::with_capacity(names.len());
            row.push(if keep_ids { i64::try_from(f.id).map(Sql::Integer).unwrap_or(Sql::Null) } else { Sql::Null });
            if has_geom {
                row.push(f.geometry.as_ref().map(|x| Sql::Blob(blob(x, multi_points, srs_id))).unwrap_or(Sql::Null));
            }
            for (k, fd) in ds.fields.iter().enumerate() {
                row.push(sql_value(f.attrs.get(k).unwrap_or(&Value::Null), fd.ty));
            }
            st.execute(params_from_iter(row)).map_err(err)?;
            let e = f.geometry.as_ref().map(|x| x.extent()).unwrap_or(Extent::EMPTY);
            if let (Some(ix), false) = (index.as_mut(), e.is_empty()) {
                ix.execute(rusqlite::params![conn.last_insert_rowid(), e.min_x, e.max_x, e.min_y, e.max_y]).map_err(err)?;
            }
        }
    }

    let e = ds.extent();
    let bounds: [Option<f64>; 4] = if e.is_empty() { [None; 4] } else { [Some(e.min_x), Some(e.min_y), Some(e.max_x), Some(e.max_y)] };
    let srs: Option<i64> = has_geom.then_some(i64::from(srs_id));
    conn.execute(
        "INSERT INTO gpkg_contents (table_name, data_type, identifier, min_x, min_y, max_x, max_y, srs_id) VALUES (?1, ?2, ?1, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![table, if has_geom { "features" } else { "attributes" }, bounds[0], bounds[1], bounds[2], bounds[3], srs],
    )
    .map_err(err)?;
    if has_geom {
        conn.execute("INSERT INTO gpkg_geometry_columns VALUES (?1, ?2, ?3, ?4, 0, 0)", rusqlite::params![table, geom, geom_type, srs_id])
            .map_err(err)?;
        // The GeoPackage 1.4 triggers that keep the index in step with edits.
        let trig = |s: &str| quote(&format!("rtree_{table}_{geom}_{s}"));
        conn.execute_batch(&format!(
            "CREATE TRIGGER {ins} AFTER INSERT ON {t} WHEN (new.{g} NOT NULL AND NOT ST_IsEmpty(NEW.{g}))
            BEGIN INSERT OR REPLACE INTO {rt} VALUES (NEW.{i}, ST_MinX(NEW.{g}), ST_MaxX(NEW.{g}), ST_MinY(NEW.{g}), ST_MaxY(NEW.{g})); END;
            CREATE TRIGGER {u6} AFTER UPDATE OF {g} ON {t} WHEN OLD.{i} = NEW.{i}
                AND (NEW.{g} NOTNULL AND NOT ST_IsEmpty(NEW.{g})) AND (OLD.{g} NOTNULL AND NOT ST_IsEmpty(OLD.{g}))
            BEGIN UPDATE {rt} SET minx = ST_MinX(NEW.{g}), maxx = ST_MaxX(NEW.{g}), miny = ST_MinY(NEW.{g}), maxy = ST_MaxY(NEW.{g})
                WHERE id = NEW.{i}; END;
            CREATE TRIGGER {u7} AFTER UPDATE OF {g} ON {t} WHEN OLD.{i} = NEW.{i}
                AND (NEW.{g} NOTNULL AND NOT ST_IsEmpty(NEW.{g})) AND (OLD.{g} ISNULL OR ST_IsEmpty(OLD.{g}))
            BEGIN INSERT INTO {rt} VALUES (NEW.{i}, ST_MinX(NEW.{g}), ST_MaxX(NEW.{g}), ST_MinY(NEW.{g}), ST_MaxY(NEW.{g})); END;
            CREATE TRIGGER {u2} AFTER UPDATE OF {g} ON {t} WHEN OLD.{i} = NEW.{i} AND (NEW.{g} ISNULL OR ST_IsEmpty(NEW.{g}))
            BEGIN DELETE FROM {rt} WHERE id = OLD.{i}; END;
            CREATE TRIGGER {u5} AFTER UPDATE ON {t} WHEN OLD.{i} != NEW.{i} AND (NEW.{g} NOTNULL AND NOT ST_IsEmpty(NEW.{g}))
            BEGIN DELETE FROM {rt} WHERE id = OLD.{i};
                INSERT INTO {rt} VALUES (NEW.{i}, ST_MinX(NEW.{g}), ST_MaxX(NEW.{g}), ST_MinY(NEW.{g}), ST_MaxY(NEW.{g})); END;
            CREATE TRIGGER {u4} AFTER UPDATE ON {t} WHEN OLD.{i} != NEW.{i} AND (NEW.{g} ISNULL OR ST_IsEmpty(NEW.{g}))
            BEGIN DELETE FROM {rt} WHERE id IN (OLD.{i}, NEW.{i}); END;
            CREATE TRIGGER {del} AFTER DELETE ON {t} WHEN old.{g} NOT NULL
            BEGIN DELETE FROM {rt} WHERE id = OLD.{i}; END;",
            ins = trig("insert"),
            u2 = trig("update2"),
            u4 = trig("update4"),
            u5 = trig("update5"),
            u6 = trig("update6"),
            u7 = trig("update7"),
            del = trig("delete"),
        ))
        .map_err(err)?;
        conn.execute(
            "INSERT INTO gpkg_extensions VALUES (?1, ?2, 'gpkg_rtree_index', 'http://www.geopackage.org/spec120/#extension_rtree', 'write-only')",
            rusqlite::params![table, geom],
        )
        .map_err(err)?;
    }
    conn.execute_batch("COMMIT;").map_err(err)?;
    Ok(())
}

fn sql_value(v: &Value, ty: FieldType) -> rusqlite::types::Value {
    use rusqlite::types::Value as Sql;
    match (v, ty) {
        (Value::Null, _) => Sql::Null,
        (Value::Int(x), _) => Sql::Integer(*x),
        (Value::Float(x), _) if x.is_finite() => Sql::Real(*x),
        (Value::Float(_), _) => Sql::Null,
        (Value::Bool(b), _) => Sql::Integer(i64::from(*b)),
        (Value::Text(s), FieldType::Text) => Sql::Text(s.clone()),
        (v, ty) => match Value::parse(&v.to_string(), ty) {
            Value::Int(x) => Sql::Integer(x),
            Value::Float(x) if x.is_finite() => Sql::Real(x),
            Value::Bool(b) => Sql::Integer(i64::from(b)),
            _ => Sql::Null,
        },
    }
}

fn value(v: ValueRef<'_>, ty: FieldType) -> Value {
    match (v, ty) {
        (ValueRef::Null, _) => Value::Null,
        (ValueRef::Integer(i), FieldType::Boolean) => Value::Bool(i != 0),
        (ValueRef::Integer(i), FieldType::Double) => Value::Float(i as f64),
        (ValueRef::Integer(i), _) => Value::Int(i),
        (ValueRef::Real(f), FieldType::Integer) if f.fract() == 0.0 && f.abs() < 9e15 => Value::Int(f as i64),
        (ValueRef::Real(f), _) => Value::Float(f),
        (ValueRef::Text(t), FieldType::Text) => Value::Text(String::from_utf8_lossy(t).into_owned()),
        (ValueRef::Text(t), ty) => Value::parse(&String::from_utf8_lossy(t), ty),
        (ValueRef::Blob(_), _) => Value::Null,
    }
}

/// An SQL identifier, quoted.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn err(e: rusqlite::Error) -> DataError {
    DataError::Format(format!("GeoPackage: {e}"))
}

fn not_gpkg(e: rusqlite::Error) -> DataError {
    DataError::Format(format!("not a GeoPackage: {e}"))
}
