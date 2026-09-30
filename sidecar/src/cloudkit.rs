//! Small, typed wrapper around the CloudKit Web Services operations used by
//! Reminders. It deliberately exposes only query, lookup, modify and zone
//! changes; arbitrary URLs and containers are not accepted from UI input.

use std::collections::{BTreeMap, HashSet};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::auth::{AuthClient, bounded_response_text_with_limit};
use crate::error::{AppError, Result};

// Record pages contain notes and encoded fields and can exceed the 2 MiB
// authentication budget. Keep a finite cap on each decoded CloudKit response.
pub(crate) const MAX_CLOUDKIT_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

const CONTAINER: &str = "com.apple.reminders";
const ENVIRONMENT: &str = "production";
const DATABASE: &str = "private";

pub fn reminders_zone() -> Value {
    json!({
        "zoneName": "Reminders",
        "ownerRecordName": "_defaultOwner",
        "zoneType": "REGULAR_CUSTOM_ZONE"
    })
}

pub struct CloudKit<'a> {
    auth: &'a AuthClient,
    base: String,
}

impl<'a> CloudKit<'a> {
    pub fn new(auth: &'a AuthClient) -> Result<Self> {
        let service = auth
            .state
            .webservices
            .get("ckdatabasews")
            .or_else(|| auth.state.webservices.get("reminders"))
            .ok_or_else(|| AppError::internal(
                "This iCloud session did not expose the Reminders service",
                "Apple did not provide a usable ckdatabasews or reminders endpoint.",
            ))?;
        let parsed = reqwest::Url::parse(&service.url).map_err(|e| {
            AppError::internal("iCloud returned an invalid service URL", e.to_string())
        })?;
        let host = parsed.host_str().unwrap_or_default();
        if parsed.scheme() != "https"
            || !(host == "icloud.com" || host.ends_with(".icloud.com"))
            || parsed.username() != ""
            || parsed.password().is_some()
        {
            return Err(AppError::internal(
                "iCloud returned an unsafe service URL",
                "",
            ));
        }
        // Rebuild from the parsed URL rather than the raw string. Validating
        // one value and using another means a service URL carrying a query or
        // fragment would pass the checks above and then land the operation
        // name in the wrong place once `post` appends it.
        let port = parsed
            .port()
            .map(|port| format!(":{port}"))
            .unwrap_or_default();
        let path = parsed.path().trim_end_matches('/');
        let origin = format!("https://{host}{port}{path}");
        Ok(Self {
            auth,
            base: format!("{origin}/database/1/{CONTAINER}/{ENVIRONMENT}/{DATABASE}"),
        })
    }

    pub async fn query(&self, record_type: &str, filter_by: Vec<Value>) -> Result<Vec<Value>> {
        if !matches!(record_type, "List" | "Reminder" | "Hashtag" | "reminderList") {
            return Err(AppError::bad_request("unsupported CloudKit record type"));
        }
        let mut records = Vec::new();
        let mut continuation: Option<String> = None;
        let mut pages = PageGuard::default();
        loop {
            let mut body = json!({
                "query": {"recordType": record_type, "filterBy": filter_by.clone()},
                "zoneID": reminders_zone(),
                "resultsLimit": 200
            });
            if let Some(marker) = &continuation {
                body["continuationMarker"] = Value::String(marker.clone());
            }
            let response = self.post("records/query", &body).await?;
            if let Some(items) = response.get("records").and_then(Value::as_array) {
                records.extend(
                    items
                        .iter()
                        .filter(|item| item.get("recordName").is_some())
                        .cloned(),
                );
                reject_embedded_errors(items)?;
            }
            continuation = response
                .get("continuationMarker")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if continuation.is_none() {
                break;
            }
            pages.advance(continuation.as_deref())?;
            eprintln!("sync: query {record_type}, page={}, records={}", pages.seen.len(), records.len());
        }
        Ok(records)
    }

    pub async fn current_sync_token(&self) -> Result<Option<String>> {
        let response = self.post("records/query", &json!({
            "query":{"recordType":"reminderList"},
            "zoneID":reminders_zone(), "resultsLimit":1
        })).await?;
        reject_embedded_errors(std::slice::from_ref(&response))?;
        Ok(response.get("syncToken").and_then(Value::as_str)
            .filter(|token| !token.is_empty()).map(str::to_owned))
    }

    pub async fn lookup(&self, names: &[String]) -> Result<Vec<Value>> {
        if names.len() > 200 {
            return Err(AppError::bad_request(
                "CloudKit lookup is limited to 200 records",
            ));
        }
        let records: Vec<_> = names
            .iter()
            .map(|name| json!({"recordName":name}))
            .collect();
        let response = self
            .post(
                "records/lookup",
                &json!({"records":records,"zoneID":reminders_zone()}),
            )
            .await?;
        let items = response
            .get("records")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        reject_embedded_errors(&items)?;
        Ok(items
            .into_iter()
            .filter(|item| item.get("recordName").is_some())
            .collect())
    }

    pub async fn modify(&self, operations: Vec<Value>) -> Result<Vec<Value>> {
        if operations.is_empty() || operations.len() > 200 {
            return Err(AppError::bad_request(
                "CloudKit modify requires 1 to 200 operations",
            ));
        }
        let response = self
            .post(
                "records/modify",
                &json!({
                    "operations": operations,
                    "zoneID": reminders_zone(),
                    "atomic": true
                }),
            )
            .await?;
        let records = response
            .get("records")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        reject_embedded_errors(&records)?;
        Ok(records)
    }

    pub async fn changes(
        &self,
        cursor: Option<&str>,
        desired_record_types: Option<&[&str]>,
    ) -> Result<(Vec<Value>, Option<String>)> {
        let mut changes = Vec::new();
        let mut marker = cursor.map(str::to_owned);
        let mut pages = PageGuard::default();
        if let Some(cursor) = cursor { pages.seen.insert(cursor.to_owned()); }
        loop {
            let mut zone = json!({"zoneID":reminders_zone()});
            if let Some(record_types) = desired_record_types {
                zone["desiredRecordTypes"] = json!(record_types);
                if record_types.is_empty() { zone["desiredKeys"] = json!([]); }
            }
            if let Some(value) = &marker {
                zone["syncToken"] = Value::String(value.clone());
            }
            let body = json!({"zones":[zone],"resultsLimit":200});
            let response = self.post("changes/zone", &body).await?;
            let page = response
                .get("zones")
                .and_then(Value::as_array)
                .and_then(|zones| zones.first())
                .unwrap_or(&response);
            reject_embedded_errors(std::slice::from_ref(page))?;
            if let Some(items) = page
                .get("records")
                .or_else(|| page.get("changes"))
                .and_then(Value::as_array)
            {
                reject_embedded_errors(items)?;
                changes.extend(items.iter().cloned());
            }
            let more = page
                .get("moreComing")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let next = page
                .get("syncToken")
                .or_else(|| page.get("continuationMarker"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            if more { pages.advance(next.as_deref())?; }
            marker = next.or(marker);
            eprintln!("sync: changes page, records={}, more={more}", changes.len());
            if !more {
                return Ok((changes, marker));
            }
        }
    }

    async fn post(&self, operation: &str, body: &Value) -> Result<Value> {
        let started = std::time::Instant::now();
        eprintln!("sync: requesting {operation}");
        let response = self
            .auth
            .http()
            .post(format!("{}/{operation}", self.base))
            .headers(self.auth.setup_headers()?)
            .query(&{
                let mut query = self.auth.query();
                // These are part of pyicloud's base parameters for every
                // Reminders CloudKit request. In particular, remapEnums keeps
                // the record field representation consistent with the web app.
                query.push(("remapEnums", "true".into()));
                query.push(("getCurrentSyncToken", "true".into()));
                query
            })
            .json(body)
            .send()
            .await?;
        let status = response.status();
        let text = bounded_response_text_with_limit(response, MAX_CLOUDKIT_RESPONSE_BYTES).await?;
        eprintln!("sync: {operation} HTTP {} bytes={} elapsed_ms={}", status.as_u16(), text.len(), started.elapsed().as_millis());
        if !status.is_success() {
            return Err(classify_cloudkit(status, &text));
        }
        serde_json::from_str(&text).map_err(Into::into)
    }
}

// Bound pagination and reject missing/repeated tokens rather than spinning forever.
#[derive(Default)]
struct PageGuard { seen: HashSet<String> }
impl PageGuard {
    fn advance(&mut self, token: Option<&str>) -> Result<()> {
        let token = token.filter(|s| !s.is_empty()).ok_or_else(||
            AppError::internal("iCloud pagination stopped advancing (missing token)", ""))?;
        if self.seen.len() >= 1000 || !self.seen.insert(token.to_owned()) {
            return Err(AppError::internal("iCloud pagination stopped advancing or exceeded 1,000 pages", ""));
        }
        Ok(())
    }
}

pub fn field<'a>(record: &'a Value, name: &str) -> Option<&'a Value> {
    record.get("fields")?.get(name)?.get("value")
}

pub fn text_field(record: &Value, name: &str) -> Option<String> {
    let wrapper = record.get("fields")?.get(name)?;
    let value = wrapper.get("value")?;
    if let Some(text) = value.as_str() {
        if wrapper.get("type").and_then(Value::as_str) == Some("ENCRYPTED_BYTES") {
            return B64
                .decode(text)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok());
        }
        return Some(text.to_owned());
    }
    None
}

pub fn int_field(record: &Value, name: &str) -> i64 {
    field(record, name).and_then(Value::as_i64).unwrap_or(0)
}

pub fn reference_field(record: &Value, name: &str) -> Option<String> {
    field(record, name)?
        .get("recordName")?
        .as_str()
        .map(str::to_owned)
}

pub fn timestamp_field(record: &Value, name: &str) -> Option<i64> {
    field(record, name).and_then(Value::as_i64)
}

pub fn string(value: impl Into<String>) -> Value {
    json!({"type":"STRING","value":value.into()})
}
pub fn int(value: i64) -> Value {
    json!({"type":"INT64","value":value})
}
pub fn timestamp(value: i64) -> Value {
    json!({"type":"TIMESTAMP","value":value})
}
pub fn reference(record_name: &str) -> Value {
    json!({"type":"REFERENCE","value":{"recordName":record_name,"action":"VALIDATE"}})
}
pub fn operation(
    kind: &str,
    record_name: &str,
    record_type: &str,
    change_tag: Option<&str>,
    record_fields: Value,
) -> Value {
    let mut record =
        json!({"recordName":record_name,"recordType":record_type,"fields":record_fields});
    if let Some(tag) = change_tag {
        record["recordChangeTag"] = Value::String(tag.to_owned());
    }
    json!({"operationType":kind,"record":record})
}

pub fn resolution_tokens(names: &[&str]) -> Value {
    // Apple's logical-field merge metadata uses seconds since 2001-01-01.
    let modification_time = chrono::Utc::now().timestamp_millis() as f64 / 1000.0 - 978_307_200.0;
    let map: BTreeMap<_, _> = names.iter().map(|name| (*name, json!({
        "counter":1, "modificationTime":modification_time,
        "replicaID":uuid::Uuid::new_v4().to_string().to_uppercase()
    }))).collect();
    string(json!({"map":map}).to_string())
}

fn reject_embedded_errors(items: &[Value]) -> Result<()> {
    if let Some(error) = items
        .iter()
        .find(|item| item.get("serverErrorCode").is_some())
    {
        let code = error
            .get("serverErrorCode")
            .and_then(Value::as_str)
            .unwrap_or("UNKNOWN");
        let reason = error
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("CloudKit rejected the operation");
        if matches!(code, "CONFLICT" | "SERVER_RECORD_CHANGED") {
            return Err(AppError::Conflict {
                message: "This reminder changed on another device while you were editing it."
                    .into(),
                detail: error.to_string(),
            });
        }
        return Err(AppError::Network {
            message: "iCloud rejected a reminder operation".into(),
            detail: format!("{code}: {reason}"),
        });
    }
    Ok(())
}

fn classify_cloudkit(status: StatusCode, text: &str) -> AppError {
    if text.contains("termsUpdateNeeded") {
        return AppError::TermsRequired {
            message: "Apple requires you to accept updated iCloud terms.".into(),
            detail: cloudkit_detail(text),
        };
    }
    if matches!(status.as_u16(), 401 | 403 | 421) {
        return AppError::AuthRequired {
            message: "Your iCloud session expired. Please sign in again.".into(),
            detail: cloudkit_detail(text),
        };
    }
    if status == StatusCode::CONFLICT {
        return AppError::Conflict {
            message: "This reminder changed on another device while you were editing it.".into(),
            detail: text.into(),
        };
    }
    AppError::Network {
        message: "iCloud request failed".into(),
        detail: format!("HTTP {}: {}", status.as_u16(), cloudkit_detail(text)),
    }
}

fn cloudkit_detail(text: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return "Apple returned an unstructured error".into();
    };
    for pointer in [
        "/serverErrorCode",
        "/reason",
        "/records/0/serverErrorCode",
        "/records/0/reason",
    ] {
        if let Some(detail) = value.pointer(pointer).and_then(Value::as_str) {
            return detail.chars().take(300).collect();
        }
    }
    "Apple returned an error without a public reason".into()
}

#[cfg(test)]
mod tests {
    use super::{CloudKit, field, int_field, reference_field, text_field};
    use crate::auth::{AuthClient, Service, SessionState};
    use serde_json::json;

    fn client_for(url: &str) -> AuthClient {
        let mut state = SessionState::default();
        state.webservices.insert(
            "ckdatabasews".into(),
            Service { url: url.into(), status: "active".into() },
        );
        AuthClient::new(Some(state)).expect("build auth client")
    }

    #[test]
    fn accepts_apple_service_hosts() {
        // Apple hands back an explicit :443, which Url normalises away as the
        // default for https. A non-default port must survive.
        for (url, expected) in [
            (
                "https://p52-ckdatabasews.icloud.com:443",
                "https://p52-ckdatabasews.icloud.com/database/1/com.apple.reminders/production/private",
            ),
            (
                "https://p52-ckdatabasews.icloud.com:8443",
                "https://p52-ckdatabasews.icloud.com:8443/database/1/com.apple.reminders/production/private",
            ),
            (
                "https://icloud.com/",
                "https://icloud.com/database/1/com.apple.reminders/production/private",
            ),
        ] {
            let auth = client_for(url);
            let cloudkit = CloudKit::new(&auth).expect("an icloud.com host is allowed");
            assert_eq!(cloudkit.base, expected, "for {url}");
        }
    }

    #[test]
    fn rejects_hosts_outside_icloud() {
        for url in [
            "https://evil.example.com",
            "http://p52-ckdatabasews.icloud.com",   // plaintext
            "https://icloud.com.evil.example.com",  // suffix confusion
            "https://user:pw@p52-ckdatabasews.icloud.com", // embedded credentials
            "https://noticloud.com",
        ] {
            let auth = client_for(url);
            assert!(
                CloudKit::new(&auth).is_err(),
                "{url} must not be accepted as a CloudKit endpoint"
            );
        }
    }

    #[test]
    fn a_service_url_with_a_query_does_not_corrupt_the_request_path() {
        // The URL is validated as parsed, so it must also be *used* as parsed:
        // carrying the query through would put it ahead of the operation name.
        let auth = client_for("https://p52-ckdatabasews.icloud.com/x?token=abc#frag");
        let cloudkit = CloudKit::new(&auth).expect("host is still an Apple host");
        assert!(!cloudkit.base.contains('?'), "query must be dropped: {}", cloudkit.base);
        assert!(!cloudkit.base.contains('#'), "fragment must be dropped: {}", cloudkit.base);
    }

    #[test]
    fn record_fields_unwrap_cloudkit_value_envelopes() {
        let record = json!({
            "fields": {
                "title": {"value": "Buy milk", "type": "STRING"},
                "priority": {"value": 2, "type": "INT64"},
                "listRef": {"value": {"recordName": "list-1"}, "type": "REFERENCE"}
            }
        });
        assert_eq!(text_field(&record, "title").as_deref(), Some("Buy milk"));
        assert_eq!(int_field(&record, "priority"), 2);
        assert_eq!(reference_field(&record, "listRef").as_deref(), Some("list-1"));
        assert!(field(&record, "missing").is_none());
        assert_eq!(int_field(&record, "missing"), 0, "absent ints default to zero");
    }
}

#[cfg(test)]
mod pagination_tests {
    use super::*;
    #[test]
    fn rejects_missing_repeated_and_cyclic_tokens() {
        let mut pages = PageGuard::default();
        assert!(pages.advance(None).is_err());
        assert!(pages.advance(Some("")).is_err());
        assert!(pages.advance(Some("a")).is_ok());
        assert!(pages.advance(Some("b")).is_ok());
        assert!(pages.advance(Some("a")).is_err());
    }
}
