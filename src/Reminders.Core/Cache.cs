using Microsoft.Data.Sqlite;
using System.Text.Json.Nodes;

namespace Reminders.Core;

internal sealed class Cache : IDisposable
{
    private readonly SqliteConnection connection;
    private readonly object gate = new();
    private static readonly string[] Flags = ["completed", "flagged", "all_day", "deleted", "is_group", "resolved"];
    internal static readonly string[] EditFields = ["title", "description", "due_date", "priority", "completed", "flagged", "list_id", "deleted", "all_day"];
    public Cache(string path)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(path))!);
        connection = new SqliteConnection(new SqliteConnectionStringBuilder { DataSource = path }.ToString());
        connection.Open();
        Execute("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;");
        // Migrate before creating indexes, including caches from pre-v2 releases.
        if (Query("SELECT name FROM sqlite_master WHERE type='table' AND name='reminders'").Count > 0 && !Query("PRAGMA table_info(reminders)").Any(r => r.Text("name") == "completed_date")) Execute("ALTER TABLE reminders ADD COLUMN completed_date TEXT");
        using var stream = typeof(Cache).Assembly.GetManifestResourceStream("Reminders.Core.Schema.sql")!;
        using var reader = new StreamReader(stream);
        Execute(reader.ReadToEnd());
        SetMeta("schema_version", "2");
    }
    private SqliteCommand Command(string sql, object?[] args)
    {
        var command = connection.CreateCommand(); command.CommandText = sql;
        for (var i = 0; i < args.Length; i++) command.Parameters.AddWithValue("@p" + i, args[i] ?? DBNull.Value);
        return command;
    }
    public void Execute(string sql, params object?[] args) { lock (gate) { using var cmd = Command(sql, args); cmd.ExecuteNonQuery(); } }
    public JsonArray Query(string sql, params object?[] args)
    {
        lock (gate)
        {
            using var cmd = Command(sql, args); using var reader = cmd.ExecuteReader(); var rows = new JsonArray();
            while (reader.Read())
            {
                var row = new JsonObject();
                for (var i = 0; i < reader.FieldCount; i++)
                {
                    var name = reader.GetName(i); var value = reader.GetValue(i);
                    row[name] = value is DBNull ? null : Flags.Contains(name) ? JsonValue.Create(Convert.ToInt64(value) != 0) : J.Node(value);
                }
                rows.Add(row);
            }
            return rows;
        }
    }
    // Mutations and their queue entry commit together; a crash cannot save an
    // offline edit without saving the corresponding upload.
    public T Transaction<T>(Func<T> action)
    {
        lock (gate)
        {
            Execute("BEGIN IMMEDIATE");
            try { var result = action(); Execute("COMMIT"); return result; }
            catch { Execute("ROLLBACK"); throw; }
        }
    }
    public string? Meta(string key) => Query("SELECT value FROM meta WHERE key=@p0", key).FirstOrDefault().Text("value");
    public void SetMeta(string key, string? value)
    {
        if (value is null) Execute("DELETE FROM meta WHERE key=@p0", key);
        else Execute("INSERT INTO meta(key,value) VALUES(@p0,@p1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", key, value);
    }
    public JsonArray Lists => Query("SELECT l.*, (SELECT COUNT(*) FROM reminders r WHERE r.list_id=l.id AND r.completed=0 AND r.deleted=0) AS open_count FROM lists l ORDER BY l.position");
    public void ReplaceLists(JsonArray lists) => Transaction(() =>
    {
        Execute("DELETE FROM lists");
        for (var i = 0; i < lists.Count; i++) { var l = lists[i]!; Execute("INSERT INTO lists(id,title,color_hex,count,is_group,position) VALUES(@p0,@p1,@p2,@p3,@p4,@p5)", l.Text("id"), l.Text("title"), l.Text("color_hex"), l.Number("count"), l.Flag("is_group"), i); }
        return true;
    });
    public JsonObject? Reminder(string id)
    {
        lock (gate) { var row = Query("SELECT * FROM reminders WHERE id=@p0", id).FirstOrDefault() as JsonObject; if (row is not null) AttachTags(row); return row; }
    }
    private void AttachTags(JsonObject row)
    {
        row["tags"] = new JsonArray(Query("SELECT name FROM tags WHERE reminder_id=@p0 ORDER BY name", row.Text("id")).Select(t => JsonValue.Create(t.Text("name"))).ToArray());
        row["hashtag_ids"] = new JsonArray(); row["time_zone"] = null;
    }
    public JsonArray Reminders(JsonNode p)
    {
        lock (gate)
        {
            var scope = p.Text("scope"); var args = new List<object?>();
            string Param(object? v) { args.Add(v); return "@p" + (args.Count - 1); }
            var sql = "SELECT DISTINCT r.* FROM reminders r ";
            if (p.Text("tag") is { } tag) sql += "JOIN tags t ON t.reminder_id=r.id AND t.name=" + Param(tag) + " ";
            sql += scope == "deleted" ? "WHERE r.deleted=1 " : "WHERE r.deleted=0 ";
            sql += scope switch { "today" => "AND r.completed=0 AND r.due_date IS NOT NULL AND r.due_date<" + Param(TimeUtil.Tomorrow) + " ", "upcoming" => "AND r.completed=0 AND r.due_date IS NOT NULL ", "completed" => "AND r.completed=1 ", "deleted" => "", _ => p.Flag("include_completed") ? "" : "AND r.completed=0 " };
            if (p.Text("list_id") is { } list) sql += "AND r.list_id=" + Param(list) + " ";
            if (p.Optional("search", 4096) is { Length: > 0 } search) sql += "AND (r.title LIKE " + Param("%" + search + "%") + " OR r.description LIKE " + Param("%" + search + "%") + ") ";
            var order = p.Text("sort") switch
            {
                "title" => "r.title COLLATE NOCASE,r.due_date", "title_desc" => "r.title COLLATE NOCASE DESC", "due" => "r.due_date IS NULL,r.due_date,r.title COLLATE NOCASE", "due_desc" => "r.due_date IS NULL,r.due_date DESC", "priority" => "CASE r.priority WHEN 1 THEN 0 WHEN 5 THEN 1 WHEN 9 THEN 2 ELSE 3 END,r.due_date IS NULL,r.due_date", "created" => "r.created DESC", "created_asc" => "r.created", _ => scope == "completed" ? "r.completed_date DESC NULLS LAST,r.modified DESC" : "r.completed,r.due_date IS NULL,r.due_date,r.title COLLATE NOCASE"
            };
            var rows = Query(sql + "ORDER BY " + order + " LIMIT " + Param(Math.Clamp(p.Number("limit", 1000), 1, scope == "completed" ? 50 : 10000)), args.ToArray());
            foreach (var row in rows.OfType<JsonObject>()) AttachTags(row);
            return rows;
        }
    }
    private static object? Sql(JsonNode? v) => v is null ? null : v is JsonValue x && x.TryGetValue<bool>(out var b) ? b ? 1 : 0 : v is JsonValue y && y.TryGetValue<long>(out var n) ? n : v is JsonValue z && z.TryGetValue<string>(out var s) ? s : v.ToJsonString();
    public void Edit(string id, JsonObject fields, bool dirty = true)
    {
        var selected = fields.Where(f => EditFields.Contains(f.Key)).ToArray(); if (selected.Length == 0) return;
        var args = selected.Select(f => Sql(f.Value)).ToList(); args.Add(TimeUtil.Now); args.Add(dirty ? 1 : 0); args.Add(id);
        Execute("UPDATE reminders SET " + string.Join(",", selected.Select((f, i) => f.Key + "=@p" + i)) + $",modified=@p{selected.Length},dirty=@p{selected.Length + 1} WHERE id=@p{selected.Length + 2}", args.ToArray());
    }
    public void Upsert(JsonNode r, bool local = false)
    {
        lock (gate)
        {
            var id = r.Required("id"); var old = Reminder(id);
            if (!local && old.Number("dirty") != 0) return;
            string[] columns = ["id", "list_id", "title", "description", "due_date", "priority", "completed", "completed_date", "flagged", "all_day", "deleted", "created", "modified", "change_tag", "notified", "dirty"];
            var copy = (JsonObject)r.DeepClone(); copy["dirty"] = local ? 1 : 0; copy["notified"] = old.Number("notified");
            foreach (var key in new[] { "title", "description", "list_id" }) copy[key] ??= "";
            foreach (var key in new[] { "priority", "completed", "flagged", "all_day", "deleted" }) copy[key] ??= 0;
            Execute("INSERT INTO reminders(" + string.Join(",", columns) + ") VALUES(" + string.Join(",", columns.Select((_, i) => "@p" + i)) + ") ON CONFLICT(id) DO UPDATE SET " + string.Join(",", columns.Where(c => c != "id" && c != "created").Select(c => c + "=excluded." + c)), columns.Select(c => Sql(copy[c])).ToArray());
        }
    }
    public JsonArray Tags => Query("SELECT name,COUNT(*) AS n FROM tags t JOIN reminders r ON r.id=t.reminder_id WHERE r.deleted=0 AND r.completed=0 GROUP BY name ORDER BY name");
    public void ReplaceTags(string id, JsonArray tags) => Transaction(() => { Execute("DELETE FROM tags WHERE reminder_id=@p0", id); foreach (var t in tags) Execute("INSERT OR REPLACE INTO tags(id,name,reminder_id) VALUES(@p0,@p1,@p2)", t.Text("id"), t.Text("name"), id); return true; });
    public JsonNode Counts
    {
        get { lock (gate) { long Count(string where, params object?[] args) => Query("SELECT COUNT(*) AS n FROM reminders WHERE " + where, args)[0].Number("n"); return J.Node(new { today = Count("deleted=0 AND completed=0 AND due_date IS NOT NULL AND due_date<@p0", TimeUtil.Tomorrow), upcoming = Count("deleted=0 AND completed=0 AND due_date IS NOT NULL"), all = Count("deleted=0 AND completed=0"), completed = Count("deleted=0 AND completed=1"), deleted = Count("deleted=1") }); } }
    }
    public JsonObject Settings
    {
        get
        {
            var defaults = (JsonObject)J.Node(new { theme = "system", sync_minutes = 10, notifications_enabled = true, stale_after_minutes = 60, max_individual_toasts = 3, default_list_id = (string?)null, search_scope = "list", onboarded = false, print_group_by = "due", print_include_notes = true, print_include_completed = false, remember_password = true, sort_by = new { } });
            if (J.Parse(Meta("settings")) is JsonObject stored) foreach (var pair in stored) if (defaults.ContainsKey(pair.Key)) defaults[pair.Key] = pair.Value?.DeepClone();
            return defaults;
        }
    }
    public JsonNode SetSettings(JsonObject patch) { lock (gate) { var s = Settings; foreach (var p in patch) if (s.ContainsKey(p.Key)) s[p.Key] = p.Value?.DeepClone(); SetMeta("settings", s.ToJsonString()); return s; } }
    public void Enqueue(string id, string op, JsonNode payload, string? tag) => Execute("INSERT INTO outbox(reminder_id,op,payload,base_tag,created_at) VALUES(@p0,@p1,@p2,@p3,@p4)", id, op, payload.ToJsonString(), tag, TimeUtil.Now);
    public JsonArray Pending(int limit = 100) => Query("SELECT * FROM outbox ORDER BY seq LIMIT @p0", limit);
    public void Acknowledge(long seq, string id, string? tag) => Transaction(() => { Execute("DELETE FROM outbox WHERE seq=@p0", seq); Execute("UPDATE outbox SET base_tag=@p0 WHERE reminder_id=@p1 AND seq>@p2", tag, id, seq); Execute("UPDATE reminders SET change_tag=@p0,dirty=EXISTS(SELECT 1 FROM outbox WHERE reminder_id=@p1) WHERE id=@p1", tag, id); return true; });
    public void ReplaceId(string oldId, string newId) => Transaction(() => { Execute("UPDATE reminders SET id=@p0 WHERE id=@p1", newId, oldId); foreach (var table in new[] { "tags", "outbox" }) Execute($"UPDATE {table} SET reminder_id=@p0 WHERE reminder_id=@p1", newId, oldId); return true; });
    public JsonArray Conflicts
    {
        get { var rows = Query("SELECT * FROM conflicts WHERE resolved=0 ORDER BY detected_at DESC"); foreach (var row in rows.OfType<JsonObject>()) { row["local"] = J.Parse(row.Text("local_json")); row["remote"] = J.Parse(row.Text("remote_json")); row.Remove("local_json"); row.Remove("remote_json"); } return rows; }
    }
    public void Purge() => Transaction(() => { foreach (var table in new[] { "lists", "reminders", "tags", "outbox", "conflicts" }) Execute($"DELETE FROM {table}"); SetMeta("sync_cursor", null); SetMeta("last_sync", null); return true; });
    public void Dispose() { lock (gate) connection.Dispose(); }
}
