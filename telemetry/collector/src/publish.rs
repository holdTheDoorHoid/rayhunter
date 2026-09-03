//! The public site: a static directory anybody can host.
//!
//! Everything on it came from a summary bundle whose owner chose to send it,
//! after a reviewer marked it verified. Nothing dynamic faces the public,
//! so there is nothing on the public side to break into.
//!
//! Output:
//!
//! ```text
//! index.html              the master list, filterable in the browser
//! map.html                every submission with a location, on a map
//! s/<id>/index.html       one page per submission
//! files/<id>/...          the summary bundle and the files inside it
//! data/submissions.json   everything the pages show, for programs
//! data/map.geojson        the points
//! data/submissions.csv    one row per submission
//! ```

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, anyhow};
use serde::Serialize;
use telemetry_format::manifest::is_part_name;
use telemetry_format::summary::{CellMeta, EventMeta, Location, Summary};
use tokio::fs;

use crate::ingest;
use crate::store::{self, Record, Status};

/// Pinned, so a change on the CDN cannot change what the map runs.
const LEAFLET_CSS: (&str, &str) = (
    "https://unpkg.com/leaflet@1.9.4/dist/leaflet.css",
    "sha256-p4NxAoJBhIIN+hmNHrzRCf9tD/miZyoHS5obTRR9BMY=",
);
const LEAFLET_JS: (&str, &str) = (
    "https://unpkg.com/leaflet@1.9.4/dist/leaflet.js",
    "sha256-20nQCchB9co0qIjJZRGuk2/Z9VM+kNiyxNV1lvTlZBo=",
);
const CLUSTER_CSS: (&str, &str) = (
    "https://unpkg.com/leaflet.markercluster@1.5.3/dist/MarkerCluster.css",
    "sha256-YU3qCpj/P06tdPBJGPax0bm6Q1wltfwjsho5TR4+TYc=",
);
const CLUSTER_DEFAULT_CSS: (&str, &str) = (
    "https://unpkg.com/leaflet.markercluster@1.5.3/dist/MarkerCluster.Default.css",
    "sha256-YSWCMtmNZNwqex4CEw1nQhvFub2lmU7vcCKP+XVwwXA=",
);
const CLUSTER_JS: (&str, &str) = (
    "https://unpkg.com/leaflet.markercluster@1.5.3/dist/leaflet.markercluster.js",
    "sha256-Hk4dIpcqOSb0hZjgyvFOP+cEmDXUKKNE/tT542ZbNQg=",
);

/// What the feeds and pages carry for one submission.
#[derive(Debug, Clone, Serialize)]
pub struct Published {
    pub id: String,
    pub page: String,
    pub received_at: String,
    pub started: String,
    pub ended: Option<String>,
    pub device: String,
    pub model: Option<String>,
    pub rayhunter_version: String,
    pub max_severity: Option<String>,
    pub warnings_low: u32,
    pub warnings_medium: u32,
    pub warnings_high: u32,
    pub tags: Vec<String>,
    pub note: Option<String>,
    pub reviewed_at: Option<String>,
    pub events: Vec<EventMeta>,
    pub cells: Vec<CellMeta>,
    pub networks: Vec<String>,
    pub location: Option<Location>,
    pub analyzers: Vec<String>,
    pub files: Vec<String>,
}

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// JSON that is safe inside a `<script>` element.
fn json_for_script<T: Serialize + ?Sized>(value: &T) -> anyhow::Result<String> {
    Ok(serde_json::to_string(value)?.replace("</", "<\\/"))
}

const CSS: &str = r#"
:root{--bg:#fff;--fg:#1a1a1a;--muted:#666;--line:#ddd;--low:#b58900;--med:#cb4b16;--high:#dc322f;--link:#0b5fa5}
@media(prefers-color-scheme:dark){:root{--bg:#121417;--fg:#e6e6e6;--muted:#9a9a9a;--line:#333;--link:#7ab8f5}}
body{margin:0;font:15px/1.45 system-ui,sans-serif;background:var(--bg);color:var(--fg)}
main{max-width:1100px;margin:0 auto;padding:1.5rem}
a{color:var(--link)}nav a{margin-right:1rem}
table{border-collapse:collapse;width:100%;font-size:14px}th,td{text-align:left;padding:.4rem .5rem;border-bottom:1px solid var(--line);vertical-align:top}
th{cursor:pointer;user-select:none}
.sev{display:inline-block;padding:0 .4em;border-radius:3px;color:#fff;font-size:12px}
.sev-Low{background:var(--low)}.sev-Medium{background:var(--med)}.sev-High{background:var(--high)}.sev-none{background:var(--muted)}
.tag{display:inline-block;padding:0 .4em;border:1px solid var(--line);border-radius:3px;font-size:12px;margin-right:.25em}
.muted{color:var(--muted)}.filters{display:flex;gap:.75rem;flex-wrap:wrap;margin:1rem 0}
input,select{font:inherit;padding:.3rem .4rem;background:var(--bg);color:var(--fg);border:1px solid var(--line);border-radius:3px}
dl{display:grid;grid-template-columns:max-content 1fr;gap:.25rem 1rem}dt{font-weight:600}
#map{height:75vh;border:1px solid var(--line)}
.notice{border-left:3px solid var(--low);padding:.5rem .75rem;margin:1rem 0;background:rgba(181,137,0,.08)}
"#;

fn page(title: &str, site_title: &str, depth: usize, body: &str, head_extra: &str) -> String {
    let up = "../".repeat(depth);
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} · {site}</title><style>{css}</style>{head_extra}</head>
<body><main>
<nav><a href="{up}index.html">List</a><a href="{up}map.html">Map</a><a href="{up}data/submissions.json">JSON</a><a href="{up}data/map.geojson">GeoJSON</a><a href="{up}data/submissions.csv">CSV</a></nav>
{body}
<p class="muted">Every entry here was sent by a Rayhunter owner who chose to contribute it, with their device's own identifiers removed, and was looked at by a reviewer before it appeared. A warning is a pattern consistent with an attack, not proof of one. Locations are rounded before they leave the device.</p>
</main></body></html>"#,
        title = esc(title),
        site = esc(site_title),
        css = CSS,
    )
}

fn severity_badge(severity: Option<&str>) -> String {
    match severity {
        Some(s) => format!(r#"<span class="sev sev-{s}">{s}</span>"#, s = esc(s)),
        None => r#"<span class="sev sev-none">none</span>"#.to_string(),
    }
}

fn published_from(record: &Record, summary: &Summary) -> Published {
    let review = record.review.as_ref();
    let mut networks: Vec<String> = summary
        .cells
        .iter()
        .filter_map(|c| match (&c.mcc, &c.mnc) {
            (Some(mcc), Some(mnc)) => Some(format!("{mcc}-{mnc}")),
            _ => None,
        })
        .collect();
    networks.sort();
    networks.dedup();
    Published {
        id: record.submission_id.clone(),
        page: format!("s/{}/", record.submission_id),
        received_at: record.received_at.clone(),
        started: summary.recording.started.clone(),
        ended: summary.recording.ended.clone(),
        device: summary.device.device.clone(),
        model: summary.device.model.clone(),
        rayhunter_version: summary.software.rayhunter_version.clone(),
        max_severity: summary.analysis.warnings.max_severity().map(String::from),
        warnings_low: summary.analysis.warnings.low,
        warnings_medium: summary.analysis.warnings.medium,
        warnings_high: summary.analysis.warnings.high,
        tags: review.map(|r| r.tags.clone()).unwrap_or_default(),
        note: review.and_then(|r| r.note.clone()),
        reviewed_at: review.map(|r| r.reviewed_at.clone()),
        events: summary.analysis.events.clone(),
        cells: summary.cells.clone(),
        networks,
        location: summary.location.clone(),
        analyzers: summary
            .analysis
            .analyzers
            .iter()
            .map(|a| format!("{} v{}", a.name, a.version))
            .collect(),
        files: Vec::new(),
    }
}

fn index_html(site_title: &str, entries: &[Published]) -> anyhow::Result<String> {
    let data = json_for_script(entries)?;
    let body = format!(
        r#"<h1>{title}</h1>
<p>{n} contributed recordings, reviewed. Filter and sort below, or open the <a href="map.html">map</a>.</p>
<div class="filters">
<input id="q" placeholder="search text" aria-label="search">
<select id="sev" aria-label="severity"><option value="">any severity</option><option>High</option><option>Medium</option><option>Low</option><option value="none">none</option></select>
<select id="tag" aria-label="tag"><option value="">any tag</option></select>
<select id="dev" aria-label="device"><option value="">any device</option></select>
<select id="net" aria-label="network"><option value="">any network</option></select>
</div>
<table id="t"><thead><tr><th data-k="started">Recorded</th><th data-k="max_severity">Worst</th><th data-k="warnings">Warnings</th><th data-k="device">Device</th><th data-k="networks">Networks</th><th data-k="tags">Tags</th><th>Note</th></tr></thead><tbody></tbody></table>
<script type="application/json" id="data">{data}</script>
<script>
(function(){{
const rows=JSON.parse(document.getElementById('data').textContent);
const $=s=>document.querySelector(s);const tb=$('#t tbody');
const fill=(sel,vals)=>{{[...new Set(vals)].sort().forEach(v=>{{const o=document.createElement('option');o.textContent=v;sel.appendChild(o)}})}};
fill($('#tag'),rows.flatMap(r=>r.tags));fill($('#dev'),rows.map(r=>r.device));fill($('#net'),rows.flatMap(r=>r.networks));
let key='started',dir=-1;
const sevRank={{High:3,Medium:2,Low:1}};
function render(){{
 const q=$('#q').value.toLowerCase(),sev=$('#sev').value,tag=$('#tag').value,dev=$('#dev').value,net=$('#net').value;
 let list=rows.filter(r=>(!q||JSON.stringify(r).toLowerCase().includes(q))&&(!sev||(sev==='none'?!r.max_severity:r.max_severity===sev))&&(!tag||r.tags.includes(tag))&&(!dev||r.device===dev)&&(!net||r.networks.includes(net)));
 list.sort((a,b)=>{{let x,y;if(key==='warnings'){{x=a.warnings_low+a.warnings_medium+a.warnings_high;y=b.warnings_low+b.warnings_medium+b.warnings_high}}else if(key==='max_severity'){{x=sevRank[a.max_severity]||0;y=sevRank[b.max_severity]||0}}else{{x=String(a[key]||'');y=String(b[key]||'')}}return (x<y?-1:x>y?1:0)*dir}});
 tb.innerHTML='';
 for(const r of list){{const tr=document.createElement('tr');const e=t=>{{const d=document.createElement('div');d.textContent=t;return d.innerHTML}};
  const sev=r.max_severity?`<span class="sev sev-${{r.max_severity}}">${{r.max_severity}}</span>`:'<span class="sev sev-none">none</span>';
  tr.innerHTML=`<td><a href="${{r.page}}">${{e(r.started.slice(0,16).replace('T',' '))}}</a></td><td>${{sev}}</td><td>${{r.warnings_high}} high, ${{r.warnings_medium}} med, ${{r.warnings_low}} low</td><td>${{e(r.device)}}${{r.model?' · '+e(r.model):''}}</td><td>${{r.networks.map(e).join(', ')}}</td><td>${{r.tags.map(t=>'<span class="tag">'+e(t)+'</span>').join('')}}</td><td>${{e(r.note||'')}}</td>`;
  tb.appendChild(tr)}}
}}
['#q','#sev','#tag','#dev','#net'].forEach(s=>$(s).addEventListener('input',render));
document.querySelectorAll('th[data-k]').forEach(th=>th.addEventListener('click',()=>{{const k=th.dataset.k;if(key===k)dir=-dir;else{{key=k;dir=-1}}render()}}));
render();
}})();
</script>"#,
        title = esc(site_title),
        n = entries.len(),
    );
    Ok(page("List", site_title, 0, &body, ""))
}

fn map_html(site_title: &str, geojson: &serde_json::Value) -> anyhow::Result<String> {
    let data = json_for_script(geojson)?;
    let head = format!(
        r#"<link rel="stylesheet" href="{}" integrity="{}" crossorigin="">
<link rel="stylesheet" href="{}" integrity="{}" crossorigin="">
<link rel="stylesheet" href="{}" integrity="{}" crossorigin="">
<script src="{}" integrity="{}" crossorigin=""></script>
<script src="{}" integrity="{}" crossorigin=""></script>"#,
        LEAFLET_CSS.0,
        LEAFLET_CSS.1,
        CLUSTER_CSS.0,
        CLUSTER_CSS.1,
        CLUSTER_DEFAULT_CSS.0,
        CLUSTER_DEFAULT_CSS.1,
        LEAFLET_JS.0,
        LEAFLET_JS.1,
        CLUSTER_JS.0,
        CLUSTER_JS.1,
    );
    let body = format!(
        r#"<h1>Map</h1>
<p class="notice">Each point is where a contributed recording was made, rounded before it left the device, mostly to about ten kilometres. A point marks a neighbourhood or a town, not an address, and not a tower.</p>
<div id="map"></div>
<script type="application/json" id="geo">{data}</script>
<script>
(function(){{
const geo=JSON.parse(document.getElementById('geo').textContent);
const map=L.map('map').setView([20,0],2);
L.tileLayer('https://tile.openstreetmap.org/{{z}}/{{x}}/{{y}}.png',{{maxZoom:18,attribution:'&copy; OpenStreetMap contributors'}}).addTo(map);
const colors={{High:'#dc322f',Medium:'#cb4b16',Low:'#b58900',none:'#666'}};
const group=L.markerClusterGroup();
const e=t=>{{const d=document.createElement('div');d.textContent=t;return d.innerHTML}};
L.geoJSON(geo,{{pointToLayer:(f,ll)=>L.circleMarker(ll,{{radius:8,color:colors[f.properties.severity||'none'],fillOpacity:.7}}),
 onEachFeature:(f,l)=>{{const p=f.properties;l.bindPopup(`<b>${{e(p.started.slice(0,10))}}</b> · ${{e(p.severity||'no warning')}}<br>${{e(p.device)}}<br>${{p.tags.map(e).join(', ')}}<br>within about ${{p.radius_m>=1000?Math.round(p.radius_m/1000)+' km':p.radius_m+' m'}}<br><a href="s/${{p.id}}/">details</a>`)}}}}).addTo(group);
map.addLayer(group);
if(geo.features.length)map.fitBounds(group.getBounds().pad(0.2));
}})();
</script>"#
    );
    Ok(page("Map", site_title, 0, &body, &head))
}

fn detail_html(site_title: &str, p: &Published) -> String {
    let mut body = String::new();
    let _ = write!(
        body,
        "<h1>Recording of {}</h1><p>{} · {}</p>",
        esc(&p.started[..p.started.len().min(16)].replace('T', " ")),
        severity_badge(p.max_severity.as_deref()),
        p.tags
            .iter()
            .map(|t| format!(r#"<span class="tag">{}</span>"#, esc(t)))
            .collect::<Vec<_>>()
            .join("")
    );
    if let Some(note) = &p.note {
        let _ = write!(body, r#"<p class="notice">Reviewer: {}</p>"#, esc(note));
    }
    let _ = write!(
        body,
        "<dl><dt>Recorded</dt><dd>{}{}</dd>",
        esc(&p.started),
        p.ended
            .as_deref()
            .map(|e| format!(" to {}", esc(e)))
            .unwrap_or_default()
    );
    let _ = write!(
        body,
        "<dt>Device</dt><dd>{}{}</dd>",
        esc(&p.device),
        p.model
            .as_deref()
            .map(|m| format!(", {}", esc(m)))
            .unwrap_or_default()
    );
    let _ = write!(
        body,
        "<dt>Rayhunter</dt><dd>{}</dd>",
        esc(&p.rayhunter_version)
    );
    let _ = write!(
        body,
        "<dt>Detectors</dt><dd>{}</dd>",
        esc(&p.analyzers.join(", "))
    );
    let _ = write!(
        body,
        "<dt>Warnings</dt><dd>{} high, {} medium, {} low</dd>",
        p.warnings_high, p.warnings_medium, p.warnings_low
    );
    match &p.location {
        Some(l) => {
            let radius = l.precision.radius_metres().unwrap_or(0);
            let _ = write!(
                body,
                "<dt>Location</dt><dd>{:.3}, {:.3} (rounded: within about {})</dd>",
                l.latitude,
                l.longitude,
                if radius >= 1000 {
                    format!("{} km", radius / 1000)
                } else {
                    format!("{radius} m")
                }
            );
        }
        None => body.push_str("<dt>Location</dt><dd>not shared</dd>"),
    }
    let _ = write!(body, "<dt>Received</dt><dd>{}</dd>", esc(&p.received_at));
    if let Some(r) = &p.reviewed_at {
        let _ = write!(body, "<dt>Reviewed</dt><dd>{}</dd>", esc(r));
    }
    body.push_str("</dl>");

    body.push_str("<h2>Warnings</h2>");
    if p.events.is_empty() {
        body.push_str(
            r#"<p class="muted">None. This recording was contributed as baseline data.</p>"#,
        );
    } else {
        body.push_str("<table><thead><tr><th>Time</th><th>Severity</th><th>Detector</th><th>Message</th><th>Packet</th></tr></thead><tbody>");
        for e in &p.events {
            let _ = write!(
                body,
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                esc(e.timestamp.as_deref().unwrap_or("")),
                severity_badge(Some(&e.severity)),
                esc(&e.analyzer),
                esc(&e.message),
                e.packet_num.map(|n| n.to_string()).unwrap_or_default()
            );
        }
        body.push_str("</tbody></table>");
    }

    body.push_str("<h2>Cells heard</h2>");
    if p.cells.is_empty() {
        body.push_str(r#"<p class="muted">None recorded.</p>"#);
    } else {
        body.push_str("<table><thead><tr><th>Network</th><th>Tracking area</th><th>Cell identity</th><th>PCI</th><th>EARFCN</th><th>Band</th><th>First heard</th><th>Last heard</th></tr></thead><tbody>");
        for c in &p.cells {
            let _ = write!(
                body,
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                match (&c.mcc, &c.mnc) {
                    (Some(a), Some(b)) => esc(&format!("{a}-{b}")),
                    _ => "?".into(),
                },
                c.tac.map(|t| t.to_string()).unwrap_or_default(),
                c.cell_id.map(|t| t.to_string()).unwrap_or_default(),
                c.pci.map(|t| t.to_string()).unwrap_or_default(),
                c.earfcn,
                c.band.map(|b| b.to_string()).unwrap_or_default(),
                esc(c.first_seen.as_deref().unwrap_or("")),
                esc(c.last_seen.as_deref().unwrap_or(""))
            );
        }
        body.push_str("</tbody></table>");
    }

    body.push_str("<h2>Files</h2><ul>");
    for f in &p.files {
        let _ = write!(
            body,
            r#"<li><a href="../../files/{}/{}">{}</a></li>"#,
            esc(&p.id),
            esc(f),
            esc(f)
        );
    }
    body.push_str("</ul><p class=\"muted\">The capture is a PCAP with this device's own IMSI, IMEI and temporary identity set to zero; see redaction-report.json for what was found and removed. The raw recording was never sent in this tier.</p>");
    page(
        &format!("Recording {}", &p.id[..8]),
        site_title,
        2,
        &body,
        "",
    )
}

fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

/// Write the site. Returns how many submissions it holds.
pub async fn publish(
    data: &Path,
    out: &Path,
    site_title: &str,
    base_url: Option<&str>,
) -> anyhow::Result<usize> {
    fs::create_dir_all(out.join("data")).await?;
    fs::create_dir_all(out.join("files")).await?;
    fs::create_dir_all(out.join("s")).await?;

    let mut entries: Vec<Published> = Vec::new();
    for record in store::list(data).await? {
        if record.status != Status::Verified {
            continue;
        }
        let Some(summary) = store::load_summary(data, &record.submission_id).await? else {
            continue;
        };
        let id = &record.submission_id;
        let dir = store::dir_for(data, id).ok_or_else(|| anyhow!("bad id"))?;
        let zip = dir.join("summary.zip");
        let files_dir = out.join("files").join(id);
        fs::create_dir_all(&files_dir).await?;
        let mut files = Vec::new();
        if fs::try_exists(&zip).await? {
            fs::copy(&zip, files_dir.join("summary.zip"))
                .await
                .with_context(|| format!("copying {}", zip.display()))?;
            files.push("summary.zip".to_string());
            for name in ingest::list_entries(&zip).await? {
                if !is_part_name(&name) {
                    continue;
                }
                if let Some(bytes) = ingest::extract_entry(&zip, &name).await? {
                    fs::write(files_dir.join(&name), bytes).await?;
                    files.push(name);
                }
            }
        }
        let mut published = published_from(&record, &summary);
        published.files = files;
        let page_dir = out.join("s").join(id);
        fs::create_dir_all(&page_dir).await?;
        fs::write(
            page_dir.join("index.html"),
            detail_html(site_title, &published),
        )
        .await?;
        entries.push(published);
    }
    entries.sort_by(|a, b| b.started.cmp(&a.started));

    // Feeds.
    let base = base_url.map(|b| b.trim_end_matches('/').to_string());
    let with_links: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            let mut v = serde_json::to_value(e).unwrap_or_default();
            if let (Some(base), Some(obj)) = (&base, v.as_object_mut()) {
                obj.insert("url".into(), format!("{base}/{}", e.page).into());
            }
            v
        })
        .collect();
    fs::write(
        out.join("data/submissions.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "format": telemetry_format::FORMAT,
            "title": site_title,
            "generated_at": store::now(),
            "submissions": with_links,
        }))?,
    )
    .await?;

    let features: Vec<serde_json::Value> = entries
        .iter()
        .filter_map(|e| {
            let l = e.location.as_ref()?;
            Some(serde_json::json!({
                "type": "Feature",
                "geometry": { "type": "Point", "coordinates": [l.longitude, l.latitude] },
                "properties": {
                    "id": e.id, "started": e.started, "severity": e.max_severity,
                    "device": e.device, "tags": e.tags, "radius_m": l.precision.radius_metres().unwrap_or(0),
                    "networks": e.networks,
                }
            }))
        })
        .collect();
    let geojson = serde_json::json!({ "type": "FeatureCollection", "features": features });
    fs::write(
        out.join("data/map.geojson"),
        serde_json::to_vec_pretty(&geojson)?,
    )
    .await?;

    let mut csv = String::from(
        "id,received_at,started,device,rayhunter_version,max_severity,high,medium,low,networks,latitude,longitude,precision,tags\n",
    );
    for e in &entries {
        let (lat, lon, prec) = match &e.location {
            Some(l) => (
                l.latitude.to_string(),
                l.longitude.to_string(),
                format!("{:?}", l.precision).to_lowercase(),
            ),
            None => (String::new(), String::new(), "none".into()),
        };
        let _ = writeln!(
            csv,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            e.id,
            e.received_at,
            e.started,
            csv_field(&e.device),
            csv_field(&e.rayhunter_version),
            e.max_severity.as_deref().unwrap_or(""),
            e.warnings_high,
            e.warnings_medium,
            e.warnings_low,
            csv_field(&e.networks.join(" ")),
            lat,
            lon,
            prec,
            csv_field(&e.tags.join(" "))
        );
    }
    fs::write(out.join("data/submissions.csv"), csv).await?;

    fs::write(out.join("index.html"), index_html(site_title, &entries)?).await?;
    fs::write(out.join("map.html"), map_html(site_title, &geojson)?).await?;
    Ok(entries.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escaping_and_csv_quoting_hold() {
        assert_eq!(
            esc("<a href=\"x\">&'"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        let json = json_for_script(&serde_json::json!({"x": "</script><script>"})).unwrap();
        assert!(!json.contains("</script>"), "{json}");
    }
}
