//! Read-only GIS endpoints over the seeded `public` tables.
use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{error::{ApiError, ApiResult}, AppState};

/// Tables that are never exposed through the generic layer endpoints.
const HIDDEN: &[&str] = &["spatial_ref_sys", "attachments", "_sqlx_migrations"];

#[derive(Serialize, sqlx::FromRow)]
pub struct LayerInfo {
    pub name: String,
    pub geometry_type: Option<String>,
    pub feature_count: i64,
    pub attachment_count: i64,
}

pub async fn health(State(st): State<AppState>) -> Response {
    match sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&st.db).await {
        Ok(_) => Json(json!({"status": "ok", "db": "ok"})).into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"status": "error", "db": e.to_string()}))).into_response(),
    }
}

/// All base tables in `public` (minus HIDDEN), with geometry type if spatial.
async fn layer_catalog(st: &AppState) -> ApiResult<HashMap<String, Option<String>>> {
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT t.table_name::text, g.type::text
           FROM information_schema.tables t
           LEFT JOIN geometry_columns g ON g.f_table_schema = t.table_schema AND g.f_table_name = t.table_name
          WHERE t.table_schema = 'public' AND t.table_type = 'BASE TABLE'",
    )
    .fetch_all(&st.db)
    .await?;
    Ok(rows.into_iter().filter(|(n, _)| !HIDDEN.contains(&n.as_str())).collect())
}

async fn resolve(st: &AppState, layer: &str) -> ApiResult<Option<String>> {
    let cat = layer_catalog(st).await?;
    cat.get(layer).cloned().ok_or(ApiError::NotFound)
}

fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub async fn list_layers(State(st): State<AppState>) -> ApiResult<Json<Vec<LayerInfo>>> {
    let cat = layer_catalog(&st).await?;
    let att: HashMap<String, i64> = sqlx::query_as::<_, (String, i64)>(
        "SELECT table_name, count(*) FROM public.attachments GROUP BY 1",
    )
    .fetch_all(&st.db)
    .await
    .unwrap_or_default()
    .into_iter()
    .collect();
    let mut out = Vec::new();
    for (name, geom) in cat {
        let n: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM public.{}", ident(&name)))
            .fetch_one(&st.db)
            .await?;
        out.push(LayerInfo { attachment_count: *att.get(&name).unwrap_or(&0), name, geometry_type: geom, feature_count: n });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct FeatureQuery {
    bbox: Option<String>,
    /// ogc_fid (stable row id present in every table and in the vector tiles)
    fid: Option<i64>,
    limit: Option<i64>,
}

pub async fn features(
    State(st): State<AppState>,
    Path(layer): Path<String>,
    Query(q): Query<FeatureQuery>,
) -> ApiResult<Json<Value>> {
    let geom = resolve(&st, &layer).await?;
    let limit = q.limit.unwrap_or(5000).clamp(1, 20000);
    let mut where_ = vec!["TRUE".to_string()];
    let mut bbox_vals: Vec<f64> = Vec::new();
    if let Some(b) = &q.bbox {
        if geom.is_none() {
            return Err(ApiError::BadRequest("bbox not supported on a table without geometry".into()));
        }
        bbox_vals = b.split(',').map(|s| s.trim().parse::<f64>()).collect::<Result<_, _>>()
            .map_err(|_| ApiError::BadRequest("bbox must be minLon,minLat,maxLon,maxLat".into()))?;
        if bbox_vals.len() != 4 {
            return Err(ApiError::BadRequest("bbox must be minLon,minLat,maxLon,maxLat".into()));
        }
        where_.push("geom && ST_MakeEnvelope($1, $2, $3, $4, 4326)".into());
    }
    if q.fid.is_some() {
        where_.push(format!("ogc_fid = ${}", if bbox_vals.is_empty() { 1 } else { 5 }));
    }
    let feature_expr = if geom.is_some() {
        "ST_AsGeoJSON(t.*, 'geom', 7)::jsonb"
    } else {
        "jsonb_build_object('type','Feature','geometry',NULL,'properties',to_jsonb(t))"
    };
    let sql = format!(
        "SELECT jsonb_build_object('type','FeatureCollection','features', COALESCE(jsonb_agg({feature_expr}), '[]'::jsonb))
           FROM (SELECT * FROM public.{} WHERE {} ORDER BY 1 LIMIT {limit}) t",
        ident(&layer),
        where_.join(" AND ")
    );
    let mut query = sqlx::query_scalar::<_, Value>(&sql);
    for v in &bbox_vals {
        query = query.bind(*v);
    }
    if let Some(o) = q.fid {
        query = query.bind(o as i32);
    }
    Ok(Json(query.fetch_one(&st.db).await?))
}

#[derive(Deserialize)]
pub struct SearchQuery {
    q: String,
}

/// Parcel search by address or APN (replaces the AGO locator for in-community lookups).
pub async fn search(State(st): State<AppState>, Query(q): Query<SearchQuery>) -> ApiResult<Json<Value>> {
    let term = q.q.trim();
    if term.len() < 2 {
        return Ok(Json(json!([])));
    }
    let like = format!("%{}%", term.replace('%', "").replace('_', ""));
    let digits: String = term.chars().filter(|c| c.is_ascii_digit()).collect();
    let rows: Value = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(r), '[]'::jsonb) FROM (
           SELECT ogc_fid AS fid, addrlong AS address, apn,
                  ST_X(ST_PointOnSurface(geom)) AS lon, ST_Y(ST_PointOnSurface(geom)) AS lat,
                  jsonb_build_array(ST_XMin(geom), ST_YMin(geom), ST_XMax(geom), ST_YMax(geom)) AS bbox
             FROM public.parcels
            WHERE addrlong ILIKE $1 OR apn ILIKE $1
               OR ($2 <> '' AND regexp_replace(COALESCE(apn,''), '[^0-9]', '', 'g') LIKE '%' || $2 || '%')
            ORDER BY addrlong NULLS LAST
            LIMIT 10) r",
    )
    .bind(&like)
    .bind(if digits.len() >= 3 { digits } else { String::new() })
    .fetch_one(&st.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct AttachmentInfo {
    attachment_id: i32,
    name: Option<String>,
    content_type: Option<String>,
    size: Option<i64>,
    keywords: Option<String>,
    #[sqlx(skip)]
    url: String,
}

pub async fn list_attachments(
    State(st): State<AppState>,
    Path((layer, objectid)): Path<(String, i32)>,
) -> ApiResult<Json<Vec<AttachmentInfo>>> {
    let mut rows: Vec<AttachmentInfo> = sqlx::query_as(
        "SELECT attachment_id, name, content_type, size, keywords
           FROM public.attachments WHERE table_name = $1 AND objectid = $2 ORDER BY attachment_id",
    )
    .bind(&layer)
    .bind(objectid)
    .fetch_all(&st.db)
    .await?;
    for r in rows.iter_mut() {
        r.url = format!("/api/attachments/{layer}/{objectid}/{}", r.attachment_id);
    }
    Ok(Json(rows))
}

pub async fn attachment_file(
    State(st): State<AppState>,
    Path((layer, objectid, attachment_id)): Path<(String, i32, i32)>,
) -> ApiResult<Response> {
    let row: (String, Option<String>) = sqlx::query_as(
        "SELECT file_path, content_type FROM public.attachments
          WHERE table_name = $1 AND objectid = $2 AND attachment_id = $3",
    )
    .bind(&layer)
    .bind(objectid)
    .bind(attachment_id)
    .fetch_one(&st.db)
    .await?;
    serve_file(&st.cfg.attachments_dir, &row.0, row.1.as_deref()).await
}

/// Serve a file under `root`, refusing any path that escapes it.
pub async fn serve_file(root: &str, rel: &str, content_type: Option<&str>) -> ApiResult<Response> {
    if rel.contains("..") || rel.starts_with('/') || rel.contains('\\') {
        return Err(ApiError::NotFound);
    }
    let path = std::path::Path::new(root).join(rel);
    let bytes = tokio::fs::read(&path).await?;
    let ct = content_type
        .filter(|c| c.starts_with("image/") || *c == "application/pdf")
        .unwrap_or("application/octet-stream")
        .to_string();
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, ct)
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .header("X-Content-Type-Options", "nosniff")
        .body(Body::from(bytes))
        .unwrap())
}

/// Asset Dashboard statistics (same definitions as the AGO dashboard widgets).
pub async fn stats(State(st): State<AppState>) -> ApiResult<Json<Value>> {
    let v: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object(
           'signs', (SELECT count(*) FROM public.signs),
           'fire_hydrants', (SELECT count(*) FROM public.fire_hydrants),
           'irrigation', (SELECT count(*) FROM public.irrigation),
           'hma_overlay_sqft', (SELECT round(COALESCE(sum(ST_Area(geom::geography)), 0)::numeric * 10.7639104) FROM public.hma_overlay_repairs),
           'full_repair_sqft', (SELECT round(COALESCE(sum(ST_Area(geom::geography)), 0)::numeric * 10.7639104) FROM public.full_repair),
           'hma_overlay_shape_area_sum', (SELECT round(COALESCE(sum(shape__area), 0)::numeric) FROM public.hma_overlay_repairs),
           'full_repair_shape_area_sum', (SELECT round(COALESCE(sum(shape__area), 0)::numeric) FROM public.full_repair),
           'signs_by_code', (SELECT COALESCE(jsonb_agg(jsonb_build_object('code', code, 'count', n) ORDER BY n DESC, code), '[]'::jsonb)
                               FROM (SELECT COALESCE(code, '(blank)') AS code, count(*) AS n FROM public.signs GROUP BY 1) s),
           'irrigation_by_description', (SELECT COALESCE(jsonb_agg(jsonb_build_object('description', description, 'count', n) ORDER BY n DESC), '[]'::jsonb)
                               FROM (SELECT COALESCE(description, '(blank)') AS description, count(*) AS n FROM public.irrigation GROUP BY 1) s),
           'sign_list', (SELECT COALESCE(jsonb_agg(jsonb_build_object('fid', ogc_fid, 'objectid', objectid, 'code', code, 'assetid', assetid,
                               'lon', ST_X(geom), 'lat', ST_Y(geom)) ORDER BY assetid), '[]'::jsonb) FROM public.signs))",
    )
    .fetch_one(&st.db)
    .await?;
    Ok(Json(v))
}
