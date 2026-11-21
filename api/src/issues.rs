//! Homeowner issue reporting (replaces Survey123 "Canyon Oaks Report an Issue").
use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    email,
    error::{ApiError, ApiResult},
    layers::serve_file,
    ratelimit::client_ip,
    AppState,
};

/// Categories and their issue-type lists, verbatim from the Survey123 XLSForm.
pub const CATEGORIES: &[(&str, &[&str])] = &[
    ("Streets and pavement", &["Pothole", "Cracking", "Trip hazard", "Raised edge", "Sinkhole", "Settling", "Debris in roadway", "Other (describe)"]),
    ("Signs and striping", &["Missing sign", "Damaged sign", "Leaning sign", "Faded sign", "Sign blocked from view", "Faded striping", "Faded curb paint", "Other (describe)"]),
    ("Lighting", &["Light out", "Light flickering", "Light on during the day", "Damaged pole", "Damaged fixture", "Other (describe)"]),
    ("Irrigation and water leaks", &["Broken head", "Missing head", "Flooding", "Pooling", "Water running onto street", "Water running onto sidewalk", "Watering at wrong time", "Dry spot", "Other (describe)"]),
    ("Landscaping and trees", &["Fallen limb", "Hanging limb", "Dead tree", "Diseased tree", "Plants blocking view", "Plants blocking path", "Weeds", "Pest concern", "Wildlife concern", "Other (describe)"]),
    ("Drainage and storm drains", &["Clogged drain", "Clogged inlet", "Standing water", "Erosion", "Washout", "Other (describe)"]),
    ("Common areas and amenities", &["Pool", "Spa", "Clubhouse", "Restroom", "Gate", "Access control", "Fence", "Wall", "Mailbox", "Playground", "Park equipment", "Other (describe)"]),
    ("Safety hazards", &["Exposed wiring", "Damaged railing", "Open hole", "Open trench", "Dry brush fire risk", "Other (describe)"]),
    ("Neighbor and CC&R concerns", &["Parking", "Trash cans out", "Unapproved change", "Noise", "Pets", "Other (describe)"]),
    ("Golf course boundary", &["Stray balls", "Property damage", "Cart path issue", "Damaged fence", "Damaged netting", "Other (describe)"]),
    ("Other", &[]),
];
pub const STATUSES: &[&str] = &["New", "In Progress", "Closed"];
/// Reports must fall within this lon/lat box (Canyon Oaks plus ~2 km).
const BOUNDS: (f64, f64, f64, f64) = (-121.811, 39.737, -121.729, 39.789);
const MAX_PHOTOS: usize = 3;
const MAX_PHOTO_BYTES: usize = 12 * 1024 * 1024;

// Same pattern the Survey123 form used for its email constraint (simplified, ASCII).
static EMAIL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^[^<>()\[\]\\.,;:\s@"]+(\.[^<>()\[\]\\.,;:\s@"]+)*@([A-Za-z0-9-]+\.)+[A-Za-z]{2,}$"#).unwrap()
});

fn is_admin(st: &AppState, headers: &HeaderMap) -> bool {
    let want = st.cfg.admin_token.as_bytes();
    if want.len() < 16 {
        return false;
    }
    let got = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
        .as_bytes();
    got.len() == want.len() && got.iter().zip(want).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

fn require_admin(st: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    if is_admin(st, headers) { Ok(()) } else { Err(ApiError::Unauthorized) }
}

pub async fn options() -> Json<Value> {
    let cats: Vec<Value> = CATEGORIES.iter().map(|(c, t)| json!({"category": c, "issue_types": t})).collect();
    Json(json!({"categories": cats, "statuses": STATUSES}))
}

fn photo_kind(data: &[u8]) -> Option<(&'static str, &'static str)> {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("image/jpeg", "jpg"))
    } else if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(("image/png", "png"))
    } else if data.len() > 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else if data.len() > 12 && &data[4..8] == b"ftyp" && (&data[8..12] == b"heic" || &data[8..12] == b"heix" || &data[8..12] == b"mif1" || &data[8..12] == b"msf1") {
        Some(("image/heic", "heic"))
    } else {
        None
    }
}

struct Photo {
    data: Vec<u8>,
    content_type: &'static str,
    ext: &'static str,
    name: String,
}

fn clean(s: Option<String>, max: usize, field: &str) -> ApiResult<Option<String>> {
    match s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
        Some(v) if v.chars().count() > max => Err(ApiError::BadRequest(format!("{field} is too long (max {max} characters)"))),
        other => Ok(other),
    }
}

pub async fn create(
    State(st): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    mut mp: Multipart,
) -> ApiResult<Response> {
    if !st.limiter.check(&client_ip(&headers, peer)) {
        return Err(ApiError::TooMany);
    }
    let mut f: std::collections::HashMap<String, String> = Default::default();
    let mut photos: Vec<Photo> = Vec::new();
    while let Some(field) = mp.next_field().await.map_err(|e| ApiError::BadRequest(format!("bad form data: {e}")))? {
        let name = field.name().unwrap_or("").to_string();
        if name == "photos" || name == "photo" {
            let fname = field.file_name().unwrap_or("photo").chars().take(120).collect::<String>();
            let data = field.bytes().await.map_err(|e| ApiError::BadRequest(format!("photo upload failed: {e}")))?;
            if data.is_empty() {
                continue;
            }
            if photos.len() >= MAX_PHOTOS {
                return Err(ApiError::BadRequest(format!("at most {MAX_PHOTOS} photos")));
            }
            if data.len() > MAX_PHOTO_BYTES {
                return Err(ApiError::BadRequest("each photo must be 12 MB or smaller".into()));
            }
            let (ct, ext) = photo_kind(&data).ok_or_else(|| ApiError::BadRequest("photos must be JPEG, PNG, WebP or HEIC".into()))?;
            photos.push(Photo { data: data.to_vec(), content_type: ct, ext, name: fname });
        } else {
            let v = field.text().await.map_err(|e| ApiError::BadRequest(format!("bad field {name}: {e}")))?;
            if v.len() > 10_000 {
                return Err(ApiError::BadRequest(format!("{name} is too long")));
            }
            f.insert(name, v);
        }
    }
    let get = |k: &str| f.get(k).cloned();
    let name = clean(get("name"), 200, "Name")?.ok_or_else(|| ApiError::BadRequest("Name is required".into()))?;
    let address = clean(get("street_address").or(get("address")), 300, "Street address")?
        .ok_or_else(|| ApiError::BadRequest("Street address is required".into()))?;
    let email = clean(get("email"), 254, "Email")?.ok_or_else(|| ApiError::BadRequest("Email is required".into()))?;
    if !EMAIL_RE.is_match(&email) {
        return Err(ApiError::BadRequest("Email address is not valid".into()));
    }
    let hoa = clean(get("hoa_account_number").or(get("hoa_account")), 50, "HOA account number")?;
    let category = clean(get("issue_category").or(get("category")), 100, "Issue category")?
        .ok_or_else(|| ApiError::BadRequest("Issue category is required".into()))?;
    let types = CATEGORIES.iter().find(|(c, _)| *c == category).map(|(_, t)| *t)
        .ok_or_else(|| ApiError::BadRequest("Unknown issue category".into()))?;
    let issue_type = clean(get("issue_type"), 100, "Issue type")?;
    if let Some(t) = &issue_type {
        if !types.contains(&t.as_str()) {
            return Err(ApiError::BadRequest("Issue type does not match the category".into()));
        }
    }
    let description = clean(get("describe_the_issue").or(get("description")), 4000, "Description")?;
    let notes = clean(get("additional_notes").or(get("notes")), 4000, "Additional notes")?;
    let lat: f64 = get("lat").and_then(|v| v.trim().parse().ok()).ok_or_else(|| ApiError::BadRequest("Location is required".into()))?;
    let lon: f64 = get("lon").and_then(|v| v.trim().parse().ok()).ok_or_else(|| ApiError::BadRequest("Location is required".into()))?;
    if !(lon >= BOUNDS.0 && lon <= BOUNDS.2 && lat >= BOUNDS.1 && lat <= BOUNDS.3) {
        return Err(ApiError::BadRequest("Location must be in or near Canyon Oaks".into()));
    }

    let mut tx = st.db.begin().await?;
    let (id, ticket, assigned): (i32, String, String) = sqlx::query_as(
        "WITH n AS (SELECT nextval(pg_get_serial_sequence('app.issues','id'))::int AS id)
         INSERT INTO app.issues (id, ticket, name, street_address, email, hoa_account_number, issue_category,
                                 issue_type, describe_the_issue, additional_notes, status, assigned_to, geom, source)
         SELECT n.id, app.ticket_for(n.id, now()), $1, $2, $3, $4, $5, $6, $7, $8, 'New',
                app.assignee_for($5), ST_SetSRID(ST_MakePoint($9, $10), 4326), 'web'
           FROM n
         RETURNING id, ticket, assigned_to",
    )
    .bind(&name).bind(&address).bind(&email).bind(&hoa).bind(&category)
    .bind(&issue_type).bind(&description).bind(&notes).bind(lon).bind(lat)
    .fetch_one(&mut *tx)
    .await?;

    let dir = std::path::Path::new(&st.cfg.uploads_dir).join(&ticket);
    if !photos.is_empty() {
        tokio::fs::create_dir_all(&dir).await?;
    }
    for (i, p) in photos.iter().enumerate() {
        let rel = format!("{ticket}/{}_{}.{}", i + 1, uuid::Uuid::new_v4().simple(), p.ext);
        tokio::fs::write(std::path::Path::new(&st.cfg.uploads_dir).join(&rel), &p.data).await?;
        sqlx::query("INSERT INTO app.issue_photos (issue_id, storage, rel_path, content_type, size, original_name) VALUES ($1,'upload',$2,$3,$4,$5)")
            .bind(id).bind(&rel).bind(p.content_type).bind(p.data.len() as i64).bind(&p.name)
            .execute(&mut *tx).await?;
    }
    tx.commit().await?;
    tracing::info!("new issue {ticket} ({category}) with {} photo(s)", photos.len());

    email::notify_new_issue(&st.cfg.email, email::NewIssueMail {
        ticket: ticket.clone(),
        category: category.clone(),
        issue_type: issue_type.clone(),
        assigned_to: assigned.clone(),
        description: description.clone(),
        reporter_email: email.clone(),
    });

    Ok((StatusCode::CREATED, Json(json!({
        "ticket": ticket, "status": "New", "assigned_to": assigned, "photos": photos.len()
    }))).into_response())
}

/// SQL projection; private reporter fields only for admins.
fn projection(admin: bool) -> String {
    let private = if admin {
        ", 'name', i.name, 'street_address', i.street_address, 'email', i.email, 'hoa_account_number', i.hoa_account_number, 'staff_notes', i.staff_notes"
    } else {
        ""
    };
    format!(
        "jsonb_build_object('ticket', i.ticket, 'issue_category', i.issue_category, 'issue_type', i.issue_type,
           'describe_the_issue', i.describe_the_issue, 'additional_notes', i.additional_notes,
           'status', i.status, 'assigned_to', i.assigned_to, 'source', i.source,
           'lon', ST_X(i.geom), 'lat', ST_Y(i.geom),
           'created_at', to_char(i.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
           'updated_at', to_char(i.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
           'photos', COALESCE((SELECT jsonb_agg(jsonb_build_object('id', p.id, 'url', '/api/issues/' || i.ticket || '/photos/' || p.id) ORDER BY p.id)
                                 FROM app.issue_photos p WHERE p.issue_id = i.id), '[]'::jsonb){private})"
    )
}

#[derive(Deserialize)]
pub struct ListQuery {
    status: Option<String>,
    category: Option<String>,
    limit: Option<i64>,
}

pub async fn list(State(st): State<AppState>, headers: HeaderMap, Query(q): Query<ListQuery>) -> ApiResult<Json<Value>> {
    let admin = is_admin(&st, &headers);
    let sql = format!(
        "SELECT COALESCE(jsonb_agg(x ORDER BY (x->>'created_at') DESC), '[]'::jsonb) FROM (
           SELECT {} AS x FROM app.issues i
            WHERE ($1::text IS NULL OR i.status = $1) AND ($2::text IS NULL OR i.issue_category = $2)
            ORDER BY i.created_at DESC LIMIT $3) s",
        projection(admin)
    );
    let rows: Value = sqlx::query_scalar(&sql)
        .bind(q.status.filter(|s| !s.is_empty()))
        .bind(q.category.filter(|s| !s.is_empty()))
        .bind(q.limit.unwrap_or(1000).clamp(1, 5000))
        .fetch_one(&st.db)
        .await?;
    let counts: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('total', count(*), 'open', count(*) FILTER (WHERE status <> 'Closed'),
                'new', count(*) FILTER (WHERE status = 'New'), 'in_progress', count(*) FILTER (WHERE status = 'In Progress'),
                'closed', count(*) FILTER (WHERE status = 'Closed')) FROM app.issues",
    )
    .fetch_one(&st.db)
    .await?;
    Ok(Json(json!({"admin": admin, "counts": counts, "issues": rows})))
}

pub async fn get_one(State(st): State<AppState>, headers: HeaderMap, Path(ticket): Path<String>) -> ApiResult<Json<Value>> {
    let sql = format!("SELECT {} FROM app.issues i WHERE i.ticket = $1", projection(is_admin(&st, &headers)));
    Ok(Json(sqlx::query_scalar(&sql).bind(&ticket).fetch_one(&st.db).await?))
}

#[derive(Deserialize)]
pub struct UpdateBody {
    status: Option<String>,
    assigned_to: Option<String>,
    notes: Option<String>,
}

pub async fn update(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(ticket): Path<String>,
    Json(b): Json<UpdateBody>,
) -> ApiResult<Json<Value>> {
    require_admin(&st, &headers)?;
    if let Some(s) = &b.status {
        if !STATUSES.contains(&s.as_str()) {
            return Err(ApiError::BadRequest("status must be New, In Progress or Closed".into()));
        }
    }
    let assigned = clean(b.assigned_to, 200, "Assigned to")?;
    let notes = b.notes.map(|n| n.trim().chars().take(4000).collect::<String>());
    let n = sqlx::query(
        "UPDATE app.issues SET status = COALESCE($2, status), assigned_to = COALESCE($3, assigned_to),
                staff_notes = COALESCE($4, staff_notes), updated_at = now() WHERE ticket = $1",
    )
    .bind(&ticket).bind(&b.status).bind(&assigned).bind(&notes)
    .execute(&st.db)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(ApiError::NotFound);
    }
    get_one(State(st), headers, Path(ticket)).await
}

pub async fn delete(State(st): State<AppState>, headers: HeaderMap, Path(ticket): Path<String>) -> ApiResult<StatusCode> {
    require_admin(&st, &headers)?;
    let uploads: Vec<String> = sqlx::query_scalar(
        "SELECT p.rel_path FROM app.issue_photos p JOIN app.issues i ON i.id = p.issue_id WHERE i.ticket = $1 AND p.storage = 'upload'",
    )
    .bind(&ticket)
    .fetch_all(&st.db)
    .await?;
    let n = sqlx::query("DELETE FROM app.issues WHERE ticket = $1").bind(&ticket).execute(&st.db).await?.rows_affected();
    if n == 0 {
        return Err(ApiError::NotFound);
    }
    for rel in uploads {
        if !rel.contains("..") {
            let _ = tokio::fs::remove_file(std::path::Path::new(&st.cfg.uploads_dir).join(&rel)).await;
        }
    }
    if !ticket.contains("..") && !ticket.contains('/') {
        let _ = tokio::fs::remove_dir(std::path::Path::new(&st.cfg.uploads_dir).join(&ticket)).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn photo(State(st): State<AppState>, Path((ticket, photo_id)): Path<(String, i32)>) -> ApiResult<Response> {
    let (storage, rel, ct): (String, String, Option<String>) = sqlx::query_as(
        "SELECT p.storage, p.rel_path, p.content_type FROM app.issue_photos p JOIN app.issues i ON i.id = p.issue_id
          WHERE i.ticket = $1 AND p.id = $2",
    )
    .bind(&ticket)
    .bind(photo_id)
    .fetch_one(&st.db)
    .await?;
    let root = if storage == "seed" { &st.cfg.attachments_dir } else { &st.cfg.uploads_dir };
    serve_file(root, &rel, ct.as_deref()).await
}
