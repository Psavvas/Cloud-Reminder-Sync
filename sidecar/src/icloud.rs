use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::auth::{AuthClient, SessionState};
use crate::cloudkit::{self, CloudKit};
use crate::error::{AppError, Result};
use crate::model::{Reminder, ReminderList, Tag};
use crate::secrets;
use crate::timeutil::{
    floating_millis_to_instant, instant_to_floating_millis, parse_instant, to_iso,
};

/// Why a restore gave up, and whether trying again on its own could help.
///
/// A bare `false` conflated "Apple ended the session" with "we could not reach
/// Apple", and the app answered both with the sign-in screen. Most restores fail
/// the second way -- the machine launched before its Wi-Fi came up, or woke
/// mid-handshake -- and a password prompt is the wrong answer to that.
#[derive(Clone, Debug, Default)]
pub struct RestoreFailure {
    pub retryable: bool,
    pub detail: String,
}

/// Whether a failed restore is worth another attempt without asking anyone.
///
/// Not being able to reach Apple says nothing about the session. Apple actually
/// refusing the credential does, and so does having no credential left to try.
fn restore_is_worth_retrying(error: &AppError, has_credential: bool) -> bool {
    matches!(error, AppError::Network { .. }) && has_credential
}

pub struct ICloudClient {
    apple_id: String,
    auth: AuthClient,
    connected: bool,
    pending_2fa: bool,
    pub restoring: bool,
    restore_failure: Option<RestoreFailure>,
}

impl ICloudClient {
    pub fn new(apple_id: String) -> Result<Self> {
        let state = secrets::session(&apple_id)?
            .and_then(|raw| serde_json::from_str::<SessionState>(&raw).ok());
        Ok(Self {
            apple_id,
            auth: AuthClient::new(state)?,
            connected: false,
            pending_2fa: false,
            restoring: false,
            restore_failure: None,
        })
    }

    pub fn apple_id(&self) -> &str {
        &self.apple_id
    }
    pub fn connected(&self) -> bool {
        self.connected && !self.pending_2fa
    }

    /// A verification code is outstanding and someone is typing it right now.
    ///
    /// Worth asking before rebuilding the session. A restore runs the whole SRP
    /// handshake again, which makes Apple mint a new challenge and retire the
    /// code already in the user's hand -- so a background sync firing on its
    /// timer was enough to make a correctly typed code come back rejected.
    pub fn awaiting_2fa(&self) -> bool {
        self.pending_2fa
    }

    /// Whether the last failed restore is worth another attempt unprompted.
    pub fn restore_is_retryable(&self) -> bool {
        self.restore_failure
            .as_ref()
            .is_some_and(|failure| failure.retryable)
    }

    pub fn restore_detail(&self) -> String {
        self.restore_failure
            .as_ref()
            .map(|failure| failure.detail.clone())
            .unwrap_or_default()
    }
    pub fn set_apple_id(&mut self, value: String) -> Result<()> {
        if value == self.apple_id {
            return Ok(());
        }
        self.apple_id = value;
        self.auth = AuthClient::new(None)?;
        self.connected = false;
        self.pending_2fa = false;
        Ok(())
    }

    pub fn status(&self) -> Value {
        json!({
            "authenticated": self.connected(), "apple_id": self.apple_id,
            "needs_2fa": self.pending_2fa, "trusted_session":self.auth.state.trusted_session,
            "restoring":self.restoring, "can_restore":secrets::has_password(&self.apple_id),
            // So the sign-in screen can say where the code went, and so a gate
            // reopened later resumes at the code box rather than asking for a
            // password it already has.
            "two_factor": self.auth.two_factor_status()
        })
    }

    pub async fn connect(
        &mut self,
        password: Option<Zeroizing<String>>,
        remember: bool,
        accept_terms: bool,
    ) -> Result<Value> {
        let password = match password {
            Some(value) => value,
            None => secrets::password(&self.apple_id)?.ok_or_else(|| AppError::AuthRequired {
                message: "No saved iCloud credential is available".into(),
                detail: String::new(),
            })?,
        };
        match self
            .auth
            .sign_in(&self.apple_id, password.as_str(), accept_terms)
            .await
        {
            Ok(_) => {
                self.pending_2fa = self.auth.requires_two_factor();
                self.connected = !self.pending_2fa;
                if remember {
                    secrets::set_password(&self.apple_id, &password)?;
                }
                self.persist_session()?;
                if self.pending_2fa {
                    return Err(AppError::TwoFactorRequired {
                        message: "Two-factor authentication required".into(),
                        detail: String::new(),
                    });
                }
                Ok(self.status())
            }
            Err(error @ AppError::TwoFactorRequired { .. }) => {
                self.pending_2fa = true;
                if remember {
                    secrets::set_password(&self.apple_id, &password)?;
                }
                self.persist_session()?;
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    pub async fn restore(&mut self) -> bool {
        if self.connected() {
            return true;
        }
        if self.pending_2fa {
            // Not ours to rebuild: the code the user is holding belongs to the
            // challenge on the session we would be throwing away.
            self.restore_failure = Some(RestoreFailure {
                retryable: false,
                detail: "waiting for a verification code".into(),
            });
            return false;
        }
        self.restoring = true;
        let result = match self.auth.resume_session().await {
            Ok(true) => self.persist_session().map(|()| {
                self.connected = true;
                self.status()
            }),
            Ok(false) => self.connect(None, true, false).await,
            Err(error) => Err(error),
        };
        self.restoring = false;
        match result {
            Ok(_) => {
                self.restore_failure = None;
                true
            }
            Err(error) => {
                self.restore_failure = Some(RestoreFailure {
                    retryable: restore_is_worth_retrying(
                        &error,
                        self.auth.state.session_token.is_some()
                            || secrets::has_password(&self.apple_id),
                    ),
                    detail: error.body().detail.to_owned(),
                });
                false
            }
        }
    }

    pub fn invalidate(&mut self) {
        self.connected = false;
        self.pending_2fa = false;
    }
    /// Ask Apple for a code. `sms` is the user explicitly choosing a text.
    pub async fn request_2fa(&mut self, sms: bool) -> Result<Value> {
        if sms {
            self.auth.request_sms_code().await
        } else {
            self.auth.request_2fa().await
        }
    }
    pub async fn submit_2fa(&mut self, code: &str) -> Result<Value> {
        self.auth.submit_2fa(code).await?;
        self.pending_2fa = false;
        self.connected = true;
        self.persist_session()?;
        Ok(self.status())
    }
    /// Force the pending-code state, so the guard above can be exercised
    /// without a live Apple challenge.
    #[cfg(test)]
    pub(crate) fn mark_awaiting_2fa(&mut self) {
        self.pending_2fa = true;
        self.connected = false;
    }

    pub fn sign_out(&mut self) -> Result<()> {
        secrets::delete_password(&self.apple_id)?;
        secrets::delete_session(&self.apple_id)?;
        self.invalidate();
        Ok(())
    }
    fn persist_session(&self) -> Result<()> {
        let session = Zeroizing::new(serde_json::to_string(&self.auth.state)?);
        secrets::set_session(&self.apple_id, &session)
    }
    fn cloudkit(&self) -> Result<CloudKit<'_>> {
        if !self.connected() {
            return Err(AppError::AuthRequired {
                message: "Not signed in to iCloud".into(),
                detail: String::new(),
            });
        }
        CloudKit::new(&self.auth)
    }

    pub async fn lists(&self) -> Result<Vec<ReminderList>> {
        // Reminders lists live in a custom CloudKit zone. Apple's web client
        // exposes the full list snapshot through /changes/zone; querying the
        // raw List record type can return a successful but empty response.
        let (records, _) = self.cloudkit()?.changes(None, Some(&["List"])).await?;
        let mut lists = Vec::new();
        for (position, record) in records.iter().enumerate() {
            if !active_list_record(record) {
                continue;
            }
            let id = record
                .get("recordName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if id.is_empty() {
                continue;
            }
            lists.push(ReminderList {
                id,
                title: cloudkit::text_field(record, "Name").unwrap_or_else(|| "Untitled".into()),
                color_hex: parse_color(cloudkit::text_field(record, "Color").as_deref()),
                count: cloudkit::int_field(record, "Count"),
                is_group: cloudkit::int_field(record, "IsGroup") != 0,
                position: position as i64,
            });
        }
        Ok(lists)
    }

    pub async fn reminders_for(&self, list_id: &str) -> Result<Vec<Reminder>> {
        // `reminderList` is Apple's compound query. It returns Reminder rows
        // together with their related records. A direct Reminder query is
        // accepted by CloudKit but currently returns no rows for this zone.
        let filters = vec![
            json!({"fieldName":"List","comparator":"EQUALS","fieldValue":cloudkit::reference(list_id)}),
            json!({"fieldName":"includeCompleted","comparator":"EQUALS","fieldValue":cloudkit::int(1)}),
            json!({"fieldName":"LookupValidatingReference","comparator":"EQUALS","fieldValue":cloudkit::int(1)}),
        ];
        let records = self.cloudkit()?.query("reminderList", filters).await?;
        Ok(records
            .iter()
            .filter(|record| record.get("recordType").and_then(Value::as_str) == Some("Reminder"))
            .filter_map(record_to_reminder)
            .collect())
    }

    pub async fn tags_for_reminders(&self, ids: &[String]) -> Result<BTreeMap<String, Vec<Tag>>> {
        let reminder_ids: std::collections::BTreeSet<_> = ids.iter().map(String::as_str).collect();
        let hashtags = self
            .cloudkit()?
            .query("Hashtag", Vec::new())
            .await?;
        let mut tags: BTreeMap<String, Vec<Tag>> = BTreeMap::new();
        for record in hashtags {
            if cloudkit::int_field(&record, "Deleted") != 0 {
                continue;
            }
            let Some(reminder_id) = cloudkit::reference_field(&record, "Reminder") else {
                continue;
            };
            if !reminder_ids.contains(reminder_id.as_str()) {
                continue;
            }
            let Some(name) = cloudkit::text_field(&record, "Name") else {
                continue;
            };
            let id = record
                .get("recordName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            tags.entry(reminder_id.clone()).or_default().push(Tag {
                id,
                name,
                reminder_id,
            });
        }
        Ok(tags)
    }

    pub async fn changes_since(&self, cursor: Option<&str>) -> Result<RemoteChanges> {
        let (records, cursor) = self.cloudkit()?.changes(cursor, Some(&["Reminder", "List", "Hashtag"])).await?;
        Ok(parse_changes(records, cursor))
    }

    pub async fn sync_cursor(&self) -> Result<Option<String>> {
        if let Some(token) = self.cloudkit()?.current_sync_token().await? {
            return Ok(Some(token));
        }
        let (_, cursor) = self.cloudkit()?.changes(None, Some(&[])).await?;
        Ok(cursor)
    }

    pub async fn create(&self, payload: &Value) -> Result<Reminder> {
        let object = payload
            .as_object()
            .ok_or_else(|| AppError::bad_request("create payload must be an object"))?;
        let list_id = required_string(object, "list_id")?;
        let record_name = format!(
            "Reminder/{}",
            Uuid::new_v4().hyphenated().to_string().to_uppercase()
        );
        let now = Utc::now().timestamp_millis();
        let mut values = Map::new();
        values.insert("List".into(), cloudkit::reference(list_id));
        values.insert("TitleDocument".into(), cloudkit::string(crate::document::encode(object.get("title").and_then(Value::as_str).unwrap_or(""))?));
        values.insert("NotesDocument".into(), cloudkit::string(crate::document::encode(object.get("description").and_then(Value::as_str).unwrap_or(""))?));
        values.insert(
            "Priority".into(),
            cloudkit::int(object.get("priority").and_then(Value::as_i64).unwrap_or(0)),
        );
        values.insert(
            "Flagged".into(),
            cloudkit::int(
                object
                    .get("flagged")
                    .and_then(Value::as_bool)
                    .unwrap_or(false) as i64,
            ),
        );
        values.insert(
            "AllDay".into(),
            cloudkit::int(
                object
                    .get("all_day")
                    .and_then(Value::as_bool)
                    .unwrap_or(false) as i64,
            ),
        );
        values.insert("Completed".into(), cloudkit::int(0));
        values.insert("CompletionDate".into(), json!({"type":"TIMESTAMP","value":null}));
        values.insert("Deleted".into(), cloudkit::int(0));
        values.insert("CreationDate".into(), cloudkit::timestamp(now));
        values.insert("LastModifiedDate".into(), cloudkit::timestamp(now));
        if let Some(raw) = object.get("due_date").and_then(Value::as_str) {
            values.insert(
                "DueDate".into(),
                cloudkit::timestamp(instant_to_floating_millis(parse_instant(raw)?, None)),
            );
        }
        values.insert(
            "ResolutionTokenMap".into(),
            cloudkit::resolution_tokens(&[
                "titleDocument",
                "notesDocument",
                "completed",
                "completionDate",
                "lastModifiedDate",
                "priority",
                "flagged",
                "allDay",
                "dueDate",
            ]),
        );
        let result = self
            .cloudkit()?
            .modify(vec![cloudkit::operation(
                "create",
                &record_name,
                "Reminder",
                None,
                Value::Object(values),
            )])
            .await?;
        result.first().and_then(record_to_reminder).ok_or_else(|| {
            AppError::internal(
                "iCloud did not return the created reminder",
                result.first().map(Value::to_string).unwrap_or_default(),
            )
        })
    }

    pub async fn update(
        &self,
        id: &str,
        payload: &Value,
        base_tag: Option<&str>,
    ) -> Result<Reminder> {
        let mut records = self.cloudkit()?.lookup(&[id.to_owned()]).await?;
        let record = records
            .pop()
            .ok_or_else(|| AppError::bad_request(format!("No such reminder: {id}")))?;
        let remote_tag = record.get("recordChangeTag").and_then(Value::as_str);
        if base_tag.is_some() && remote_tag != base_tag {
            return Err(AppError::Conflict {
                message: "This reminder changed on another device while you were editing it."
                    .into(),
                detail: record_to_reminder(&record)
                    .map(|r| serde_json::to_string(&r).unwrap_or_default())
                    .unwrap_or_default(),
            });
        }
        let fields = update_fields(&record, payload)?;
        let operation = cloudkit::operation("update", id, "Reminder", remote_tag, Value::Object(fields.clone()));
        let result = self
            .cloudkit()?
            .modify(vec![operation])
            .await?;
        let returned = result.first().ok_or_else(|| AppError::internal("iCloud did not return the updated reminder", ""))?;
        // CloudKit may return only changed fields. Keep the untouched title,
        // list reference, and dates from the lookup instead of blanking cache.
        record_to_reminder(&merge_updated_record(record, fields, returned))
            .ok_or_else(|| AppError::internal("iCloud returned an invalid updated reminder", ""))
    }

    pub async fn delete(&self, id: &str, base_tag: Option<&str>) -> Result<Reminder> {
        self.update(id, &json!({"deleted":true}), base_tag).await
    }
}

#[derive(Default)]
pub struct RemoteChanges {
    pub updated: Vec<Reminder>,
    pub deleted: Vec<String>,
    pub cursor: Option<String>,
    pub lists_changed: bool,
    pub tags_changed: bool,
}

fn parse_changes(records: Vec<Value>, cursor: Option<String>) -> RemoteChanges {
    let mut changes = RemoteChanges { cursor, ..Default::default() };
    for record in records {
        let id = record.get("recordName").and_then(Value::as_str).unwrap_or_default();
        let kind = record.get("recordType").and_then(Value::as_str)
            .unwrap_or_else(|| id.split('/').next().unwrap_or_default());
        match kind {
            "List" => changes.lists_changed = true,
            "Hashtag" => changes.tags_changed = true,
            "Reminder" => {
                if record.get("deleted").and_then(Value::as_bool) == Some(true)
                    || record.get("reason").and_then(Value::as_str) == Some("deleted") {
                    if !id.is_empty() { changes.deleted.push(id.to_owned()); }
                } else if let Some(reminder) = record_to_reminder(&record) {
                    changes.updated.push(reminder);
                }
            }
            _ => {}
        }
    }
    changes
}

fn merge_updated_record(mut original: Value, fields: Map<String, Value>, returned: &Value) -> Value {
    if !original["fields"].is_object() { original["fields"] = json!({}); }
    let target = original["fields"].as_object_mut().unwrap();
    target.extend(fields);
    if let Some(remote) = returned.get("fields").and_then(Value::as_object) { target.extend(remote.clone()); }
    if let Some(tag) = returned.get("recordChangeTag") { original["recordChangeTag"] = tag.clone(); }
    original
}

fn update_fields(record: &Value, payload: &Value) -> Result<Map<String, Value>> {
    let patch = payload
        .as_object()
        .ok_or_else(|| AppError::bad_request("update payload must be an object"))?;
    let zone_for_write = cloudkit::text_field(record, "TimeZone");
    let mut target = Map::new();
    let mut tokens = Vec::new();
    for (json_name, cloud_name, token) in [
        ("title", "TitleDocument", "titleDocument"),
        ("description", "NotesDocument", "notesDocument"),
    ] {
        if let Some(value) = patch.get(json_name) {
            target.insert(cloud_name.into(), cloudkit::string(crate::document::encode(value.as_str().unwrap_or(""))?));
            tokens.push(token);
        }
    }
    for (json_name, cloud_name, token) in [
        ("priority", "Priority", "priority"),
        ("completed", "Completed", "completed"),
        ("flagged", "Flagged", "flagged"),
        ("deleted", "Deleted", "deleted"),
        ("all_day", "AllDay", "allDay"),
    ] {
        if let Some(value) = patch.get(json_name) {
            target.insert(
                cloud_name.into(),
                cloudkit::int(
                    value
                        .as_i64()
                        .unwrap_or_else(|| value.as_bool().unwrap_or(false) as i64),
                ),
            );
            tokens.push(token);
        }
    }
    if let Some(completed) = patch.get("completed").and_then(|v| v.as_bool().or_else(|| v.as_i64().map(|n| n != 0))) {
        if completed {
            target.insert(
                "CompletionDate".into(),
                cloudkit::timestamp(Utc::now().timestamp_millis()),
            );
        } else {
            target.insert("CompletionDate".into(), json!({"type":"TIMESTAMP","value":null}));
        }
    }
    if patch.contains_key("completed") { tokens.push("completionDate"); }
    tokens.push("lastModifiedDate");
    if let Some(value) = patch.get("due_date") {
        if value.is_null() {
            target.insert("DueDate".into(), json!({"type":"TIMESTAMP","value":null}));
        } else if let Some(raw) = value.as_str() {
            target.insert(
                "DueDate".into(),
                cloudkit::timestamp(instant_to_floating_millis(
                    parse_instant(raw)?,
                    zone_for_write.as_deref(),
                )),
            );
        }
        tokens.push("dueDate");
    }
    target.insert(
        "LastModifiedDate".into(),
        cloudkit::timestamp(Utc::now().timestamp_millis()),
    );
    target.insert(
        "ResolutionTokenMap".into(),
        cloudkit::resolution_tokens(&tokens),
    );
    Ok(target)
}

fn active_list_record(record: &Value) -> bool {
    record.get("recordType").and_then(Value::as_str) == Some("List")
        && record.get("deleted").and_then(Value::as_bool) != Some(true)
        && record.get("reason").and_then(Value::as_str) != Some("deleted")
        && cloudkit::int_field(record, "Deleted") == 0
        && record.get("fields").and_then(Value::as_object).is_some()
}

fn record_to_reminder(record: &Value) -> Option<Reminder> {
    let id = record.get("recordName")?.as_str()?.to_owned();
    let time_zone = cloudkit::text_field(record, "TimeZone");
    let due_date = cloudkit::timestamp_field(record, "DueDate")
        .map(|ms| to_iso(floating_millis_to_instant(ms, time_zone.as_deref())));
    Some(Reminder {
        id,
        list_id: cloudkit::reference_field(record, "List").unwrap_or_default(),
        title: crate::document::text(record, "TitleDocument")
            .or_else(|| cloudkit::text_field(record, "Title")).unwrap_or_default(),
        description: crate::document::text(record, "NotesDocument")
            .or_else(|| cloudkit::text_field(record, "Description")).unwrap_or_default(),
        due_date,
        time_zone,
        priority: cloudkit::int_field(record, "Priority"),
        completed: cloudkit::int_field(record, "Completed") != 0,
        completed_date: real_timestamp(record, "CompletionDate"),
        flagged: cloudkit::int_field(record, "Flagged") != 0,
        all_day: cloudkit::int_field(record, "AllDay") != 0,
        deleted: cloudkit::int_field(record, "Deleted") != 0,
        created: real_timestamp(record, "CreationDate"),
        modified: real_timestamp(record, "LastModifiedDate"),
        change_tag: record
            .get("recordChangeTag")
            .and_then(Value::as_str)
            .map(str::to_owned),
        hashtag_ids: string_list(record, "HashtagIDs"),
        tags: Vec::new(),
        notified: 0,
        dirty: 0,
    })
}

fn real_timestamp(record: &Value, name: &str) -> Option<String> {
    cloudkit::timestamp_field(record, name)
        .and_then(DateTime::<Utc>::from_timestamp_millis)
        .map(to_iso)
}
fn string_list(record: &Value, name: &str) -> Vec<String> {
    cloudkit::field(record, name)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|v| {
                    v.as_str().map(str::to_owned).or_else(|| {
                        v.get("recordName")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}
fn required_string<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a str> {
    object
        .get(name)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| AppError::bad_request(format!("missing {name}")))
}
fn parse_color(raw: Option<&str>) -> Option<String> {
    let value = raw?;
    if value.starts_with('#') {
        return Some(value.to_owned());
    }
    let parsed: Value = serde_json::from_str(value).ok()?;
    parsed
        .get("daHexString")
        .and_then(Value::as_str)
        .filter(|v| v.starts_with('#'))
        .map(str::to_owned)
}

// ---------------------------------------------------------------------------
// Staying signed in.
//
// Two failures used to arrive here as the same bare `false`, and the app
// answered both with the sign-in screen: Apple ending the session, and the app
// not being able to reach Apple at all. The second is far more common -- a
// machine that launched before its Wi-Fi came up, or woke mid-handshake -- and
// a password prompt is the wrong answer to it.

#[cfg(test)]
mod session_tests {
    use super::{ICloudClient, restore_is_worth_retrying};
    use crate::error::AppError;

    fn network() -> AppError {
        AppError::Network {
            message: "Could not reach iCloud".into(),
            detail: "connection failed".into(),
        }
    }

    fn expired() -> AppError {
        AppError::AuthRequired {
            message: "iCloud rejected the sign-in".into(),
            detail: String::new(),
        }
    }

    #[test]
    fn an_unreachable_icloud_is_worth_another_go() {
        assert!(restore_is_worth_retrying(&network(), true));
    }

    #[test]
    fn a_credential_apple_refused_is_not() {
        // Only the user can fix this one; retrying just locks the account.
        assert!(!restore_is_worth_retrying(&expired(), true));
    }

    #[test]
    fn nothing_to_retry_with_is_not_worth_retrying() {
        assert!(!restore_is_worth_retrying(&network(), false));
    }

    #[tokio::test]
    async fn a_restore_with_no_saved_credential_gives_up_cleanly() {
        let mut client = ICloudClient::new("nobody@example.com".into()).expect("client");
        assert!(!client.restore().await);
        assert!(
            !client.restore_is_retryable(),
            "there is no credential to retry with, so retrying is pointless"
        );
    }

    #[tokio::test]
    async fn a_pending_code_survives_a_restore() {
        // The background sync recovers an expired session by rebuilding it.
        // Doing that while a code is outstanding runs the whole SRP handshake
        // again, which makes Apple mint a new challenge and retire the code
        // being typed -- the sync timer alone was enough to make a correct code
        // come back rejected.
        let mut client = ICloudClient::new("someone@example.com".into()).expect("client");
        client.mark_awaiting_2fa();

        assert!(!client.restore().await);
        assert!(
            client.awaiting_2fa(),
            "the live challenge must be left alone"
        );
        assert!(
            !client.restore_is_retryable(),
            "the person at the code box finishes this, not a retry loop"
        );
    }

    #[test]
    fn the_status_carries_the_delivery_route_for_the_gate() {
        let client = ICloudClient::new("someone@example.com".into()).expect("client");
        let status = client.status();
        assert_eq!(status["needs_2fa"], false);
        assert_eq!(status["two_factor"]["method"], "unknown");
    }
}

#[cfg(test)]
mod read_regression_tests {
    use super::*;
    #[test]
    fn partial_modify_response_preserves_unmodified_fields() {
        let original = json!({"recordName":"Reminder/a", "fields":{
            "Title":{"value":"Keep title"}, "List":{"value":{"recordName":"List/a"}}, "Completed":{"value":0}
        }});
        let patch = update_fields(&original, &json!({"completed":1})).unwrap();
        let merged = merge_updated_record(original, patch, &json!({"recordName":"Reminder/a","recordChangeTag":"new"}));
        let reminder = record_to_reminder(&merged).unwrap();
        assert!(reminder.completed);
        assert_eq!(reminder.title, "Keep title");
        assert_eq!(reminder.list_id, "List/a");
        assert_eq!(reminder.change_tag.as_deref(), Some("new"));
    }

    #[test]
    fn completion_patch_accepts_the_integer_outbox_and_boolean_inputs() {
        for completed in [json!(true), json!(1), json!(false), json!(0)] {
            let fields = update_fields(&json!({}), &json!({"completed":completed})).unwrap();
            let value = completed.as_bool().unwrap_or_else(|| completed.as_i64().unwrap() != 0);
            assert_eq!(fields["Completed"]["value"], json!(value as i64));
            assert_eq!(fields["CompletionDate"]["value"].is_number(), value);
            assert_eq!(fields["CompletionDate"]["value"].is_null(), !value);
            let metadata: Value = serde_json::from_str(fields["ResolutionTokenMap"]["value"].as_str().unwrap()).unwrap();
            for name in ["completed", "completionDate", "lastModifiedDate"] {
                assert_eq!(metadata["map"][name]["counter"], 1);
                assert!(metadata["map"][name]["modificationTime"].as_f64().unwrap() > 700_000_000.0);
                assert!(Uuid::parse_str(metadata["map"][name]["replicaID"].as_str().unwrap()).is_ok());
            }
            assert!(!fields.contains_key("TitleDocument"));
        }
    }
    #[test]
    fn text_updates_use_documents_and_clears_send_explicit_null() {
        let fields = update_fields(&json!({}), &json!({"title":"Test", "description":"Notes", "due_date":null})).unwrap();
        let record = json!({"fields":fields});
        assert_eq!(crate::document::text(&record, "TitleDocument").as_deref(), Some("Test"));
        assert_eq!(crate::document::text(&record, "NotesDocument").as_deref(), Some("Notes"));
        assert!(record["fields"]["DueDate"]["value"].is_null());
    }
    #[test]
    fn delta_detects_related_changes_and_tombstones_without_types() {
        let changes = parse_changes(vec![
            json!({"recordName":"Reminder/a","fields":{}}),
            json!({"recordName":"Reminder/b","deleted":true}),
            json!({"recordName":"List/c","deleted":true}),
            json!({"recordType":"Hashtag","recordName":"Hashtag/d"}),
        ], Some("next".into()));
        assert_eq!(changes.updated.len(), 1);
        assert_eq!(changes.deleted, vec!["Reminder/b"]);
        assert!(changes.lists_changed && changes.tags_changed);
        assert_eq!(changes.cursor.as_deref(), Some("next"));
        let empty = parse_changes(vec![], None);
        assert!(!empty.lists_changed && !empty.tags_changed);
    }

    #[test]
    fn ignores_tombstones_and_unrelated_records_in_list_snapshot() {
        assert!(active_list_record(&json!({"recordType":"List","fields":{"Name":{"value":"School"}}})));
        for record in [
            json!({"recordName":"List/old","deleted":true}),
            json!({"recordType":"List","deleted":true,"fields":{}}),
            json!({"recordType":"List","reason":"deleted","fields":{}}),
            json!({"recordType":"List","fields":{"Deleted":{"value":1}}}),
            json!({"recordType":"Reminder","fields":{}}),
        ] { assert!(!active_list_record(&record)); }
    }
    #[test]
    fn reads_document_titles_and_notes_before_legacy_fields() {
        let record = json!({"recordName":"Reminder/test", "fields":{
            "TitleDocument":{"value":"EgVIZWxsbw==","type":"ENCRYPTED_BYTES"},
            "NotesDocument":{"value":"EgVIZWxsbw==","type":"STRING"},
            "Title":{"value":"old"}
        }});
        let reminder = record_to_reminder(&record).unwrap();
        assert_eq!(reminder.title, "Hello");
        assert_eq!(reminder.description, "Hello");
    }
}
