//! XMLTV fetch/parse — a deliberate copy of `native/src/api/guide.rs`'s
//! `GuideProgram`/`fetch_guide`/`parse_xmltv`, not a shared dependency: this
//! crate must stay GUI/GL/libmpv-free so it's trivially buildable/portable
//! to whatever headless host runs it (the DVR box itself, a NAS, a Pi —
//! not necessarily anything that could build the desktop app), so pulling
//! in `dvrdesk-native` as a lib dependency isn't worth it for ~150 lines.
//! Field names are kept in sync by hand so the JSON this service serves
//! deserializes directly into the client's own `GuideProgram` type.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuideProgram {
    pub channel: String,
    pub start: i64,
    pub stop: i64,
    pub title: String,
    pub desc: Option<String>,
    pub categories: Vec<String>,
    pub image: Option<String>,
    pub series_id: Option<String>,
    pub program_id: Option<String>,
    pub is_new: bool,
}

pub async fn fetch_guide(server_url: &str, duration_secs: u32) -> Result<Vec<GuideProgram>, String> {
    let base = server_url.trim_end_matches('/');
    let url = format!("{base}/devices/ANY/guide/xmltv?duration={duration_secs}");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Network error reaching {base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("API error {}: GET /devices/ANY/guide/xmltv", resp.status()));
    }
    let body = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read guide response: {e}"))?;
    parse_xmltv(&body)
}

fn parse_xmltv_time(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_str(s, "%Y%m%d%H%M%S %z")
        .ok()
        .map(|dt| dt.timestamp())
}

#[derive(Default)]
struct ProgBuilder {
    start: Option<String>,
    stop: Option<String>,
    channel: Option<String>,
    title: String,
    desc: Option<String>,
    categories: Vec<String>,
    image: Option<String>,
    series_id: Option<String>,
    program_id: Option<String>,
    is_new: bool,
}

impl ProgBuilder {
    fn build(self) -> Option<GuideProgram> {
        let channel = self.channel?;
        let start = parse_xmltv_time(self.start.as_deref()?)?;
        let stop = parse_xmltv_time(self.stop.as_deref()?)?;
        Some(GuideProgram {
            channel,
            start,
            stop,
            title: self.title,
            desc: self.desc,
            categories: self.categories,
            image: self.image,
            series_id: self.series_id.filter(|s| !s.is_empty()),
            program_id: self.program_id.filter(|s| !s.is_empty()),
            is_new: self.is_new,
        })
    }
}

fn parse_xmltv(xml: &str) -> Result<Vec<GuideProgram>, String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut programs = Vec::new();
    let mut cur: Option<ProgBuilder> = None;
    let mut cur_element: Vec<u8> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = e.name().as_ref().to_vec();
                match name.as_slice() {
                    b"programme" => {
                        let mut b = ProgBuilder::default();
                        for attr in e.attributes().flatten() {
                            let value = attr.decode_and_unescape_value(reader.decoder()).unwrap_or_default().to_string();
                            match attr.key.as_ref() {
                                b"start" => b.start = Some(value),
                                b"stop" => b.stop = Some(value),
                                b"channel" => b.channel = Some(value),
                                _ => {}
                            }
                        }
                        cur = Some(b);
                    }
                    b"new" => {
                        if let Some(c) = cur.as_mut() {
                            c.is_new = true;
                        }
                    }
                    b"icon" => {
                        if let Some(c) = cur.as_mut() {
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"src" {
                                    c.image = Some(
                                        attr.decode_and_unescape_value(reader.decoder())
                                            .unwrap_or_default()
                                            .to_string(),
                                    );
                                }
                            }
                        }
                    }
                    _ => {}
                }
                cur_element = name;
            }
            Ok(Event::Text(t)) => {
                if let Some(c) = cur.as_mut() {
                    let text = t.unescape().unwrap_or_default().to_string();
                    match cur_element.as_slice() {
                        b"title" => c.title = text,
                        b"desc" => c.desc = Some(text),
                        b"category" => c.categories.push(text),
                        b"series-id" => c.series_id = Some(text),
                        b"episode-num" => c.program_id = Some(text),
                        _ => {}
                    }
                }
            }
            Ok(Event::End(e)) => {
                if e.name().as_ref() == b"programme" {
                    if let Some(p) = cur.take().and_then(ProgBuilder::build) {
                        programs.push(p);
                    }
                }
                cur_element.clear();
            }
            Ok(_) => {}
            Err(e) => return Err(format!("Guide XML parse error: {e}")),
        }
        buf.clear();
    }

    Ok(programs)
}
