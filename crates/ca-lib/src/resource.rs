use crate::db::{Database, DbError};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricType {
    InputTokens,
    OutputTokens,
    CacheReadTokens,
    CacheCreationTokens,
    Cost,
}

impl MetricType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetricType::InputTokens => "input_tokens",
            MetricType::OutputTokens => "output_tokens",
            MetricType::CacheReadTokens => "cache_read_tokens",
            MetricType::CacheCreationTokens => "cache_creation_tokens",
            MetricType::Cost => "cost",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "input_tokens" => Some(MetricType::InputTokens),
            "output_tokens" => Some(MetricType::OutputTokens),
            "cache_read_tokens" => Some(MetricType::CacheReadTokens),
            "cache_creation_tokens" => Some(MetricType::CacheCreationTokens),
            "cost" => Some(MetricType::Cost),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceMetric {
    pub session_id: String,
    pub metric_type: MetricType,
    pub value: f64,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub model: Option<String>,
}

impl TokenUsage {
    pub fn has_data(&self) -> bool {
        self.input_tokens.is_some() || self.output_tokens.is_some() || self.cost_usd.is_some()
    }

    pub fn total_tokens(&self) -> u64 {
        self.input_tokens.unwrap_or(0)
            + self.output_tokens.unwrap_or(0)
            + self.cache_read_tokens.unwrap_or(0)
            + self.cache_creation_tokens.unwrap_or(0)
    }
}

/// Extract token usage from a Claude hook event payload.
///
/// Looks for usage data in these locations (in order):
/// - `result.usage.{input_tokens, output_tokens, ...}`
/// - `usage.{input_tokens, output_tokens, ...}`
/// - top-level `{input_tokens, output_tokens, ...}`
///
/// Cost is extracted from `result.total_cost` or `result.cost_usd` or top-level equivalents.
/// Model is extracted from `result.model` or top-level `model`.
pub fn parse_token_usage(payload: &Value) -> Option<TokenUsage> {
    let usage_obj = payload
        .pointer("/result/usage")
        .or_else(|| payload.get("usage"))
        .unwrap_or(payload);

    let input = usage_obj.get("input_tokens").and_then(|v| v.as_u64());
    let output = usage_obj.get("output_tokens").and_then(|v| v.as_u64());
    let cache_read = usage_obj
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64());
    let cache_create = usage_obj
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_u64());

    let cost = payload
        .pointer("/result/total_cost")
        .or_else(|| payload.pointer("/result/cost_usd"))
        .or_else(|| payload.get("total_cost"))
        .or_else(|| payload.get("cost_usd"))
        .and_then(|v| v.as_f64());

    let model = extract_model(payload);

    let usage = TokenUsage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_creation_tokens: cache_create,
        cost_usd: cost,
        model,
    };

    if usage.has_data() { Some(usage) } else { None }
}

/// Extract model name from a hook payload.
pub fn extract_model(payload: &Value) -> Option<String> {
    payload
        .pointer("/result/model")
        .or_else(|| payload.get("model"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Accumulate token usage from a new event into an existing total.
pub fn accumulate_usage(existing: &mut TokenUsage, new: &TokenUsage) {
    fn add_opt(a: &mut Option<u64>, b: Option<u64>) {
        if let Some(val) = b {
            *a = Some(a.unwrap_or(0) + val);
        }
    }

    add_opt(&mut existing.input_tokens, new.input_tokens);
    add_opt(&mut existing.output_tokens, new.output_tokens);
    add_opt(&mut existing.cache_read_tokens, new.cache_read_tokens);
    add_opt(
        &mut existing.cache_creation_tokens,
        new.cache_creation_tokens,
    );

    if let Some(cost) = new.cost_usd {
        existing.cost_usd = Some(existing.cost_usd.unwrap_or(0.0) + cost);
    }

    // Keep latest model name
    if new.model.is_some() {
        existing.model.clone_from(&new.model);
    }
}

/// Estimate cost in USD from token usage and model name.
///
/// Uses approximate per-token pricing. Returns 0.0 for unknown models.
pub fn estimate_cost(usage: &TokenUsage, model: &str) -> f64 {
    // Prices per million tokens (input, output)
    let (input_price, output_price) = match model {
        m if m.contains("opus") => (15.0, 75.0),
        m if m.contains("sonnet") => (3.0, 15.0),
        m if m.contains("haiku") => (0.25, 1.25),
        _ => return 0.0,
    };

    let input = usage.input_tokens.unwrap_or(0) as f64;
    let output = usage.output_tokens.unwrap_or(0) as f64;
    let cache_read = usage.cache_read_tokens.unwrap_or(0) as f64;
    let cache_create = usage.cache_creation_tokens.unwrap_or(0) as f64;

    // Cache reads are 10% of input price, cache creation is 25% more than input
    let input_cost = input * input_price / 1_000_000.0;
    let output_cost = output * output_price / 1_000_000.0;
    let cache_read_cost = cache_read * input_price * 0.1 / 1_000_000.0;
    let cache_create_cost = cache_create * input_price * 1.25 / 1_000_000.0;

    input_cost + output_cost + cache_read_cost + cache_create_cost
}

/// Convert token usage into individual `ResourceMetric` records.
pub fn usage_to_metrics(
    session_id: &str,
    usage: &TokenUsage,
    timestamp: i64,
) -> Vec<ResourceMetric> {
    let sid = session_id.to_string();

    let candidates: [(MetricType, Option<f64>); 5] = [
        (
            MetricType::InputTokens,
            usage.input_tokens.map(|v| v as f64),
        ),
        (
            MetricType::OutputTokens,
            usage.output_tokens.map(|v| v as f64),
        ),
        (
            MetricType::CacheReadTokens,
            usage.cache_read_tokens.map(|v| v as f64),
        ),
        (
            MetricType::CacheCreationTokens,
            usage.cache_creation_tokens.map(|v| v as f64),
        ),
        (MetricType::Cost, usage.cost_usd),
    ];

    candidates
        .into_iter()
        .filter_map(|(metric_type, value)| {
            value.map(|v| ResourceMetric {
                session_id: sid.clone(),
                metric_type,
                value: v,
                timestamp,
            })
        })
        .collect()
}

/// Aggregated resource summary for a session or project.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResourceSummary {
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub cost: f64,
}

// -- Database CRUD & aggregation --

impl Database {
    pub fn insert_resource_metric(&self, metric: &ResourceMetric) -> Result<i64, DbError> {
        self.connection().execute(
            r#"INSERT INTO resource_usage (session_id, metric_type, value, timestamp)
               VALUES (?1, ?2, ?3, ?4)"#,
            params![
                metric.session_id,
                metric.metric_type.as_str(),
                metric.value,
                metric.timestamp,
            ],
        )?;
        Ok(self.connection().last_insert_rowid())
    }

    pub fn insert_resource_metrics(&self, metrics: &[ResourceMetric]) -> Result<usize, DbError> {
        for m in metrics {
            self.insert_resource_metric(m)?;
        }
        Ok(metrics.len())
    }

    pub fn get_resource_metrics_by_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<ResourceMetric>, DbError> {
        let mut stmt = self.connection().prepare(
            r#"SELECT session_id, metric_type, value, timestamp
               FROM resource_usage
               WHERE session_id = ?1
               ORDER BY timestamp DESC"#,
        )?;

        let rows = stmt.query_map(params![session_id], |row| Ok(row_to_resource_metric(row)))?;

        let mut metrics = Vec::new();
        for r in rows {
            metrics.extend(r?);
        }
        Ok(metrics)
    }

    pub fn total_tokens_by_session(&self, session_id: &str) -> Result<ResourceSummary, DbError> {
        self.aggregate_resources(
            r#"SELECT metric_type, COALESCE(SUM(value), 0)
               FROM resource_usage
               WHERE session_id = ?1
               GROUP BY metric_type"#,
            params![session_id],
        )
    }

    pub fn total_tokens_by_project(&self, project_id: i64) -> Result<ResourceSummary, DbError> {
        self.aggregate_resources(
            r#"SELECT ru.metric_type, COALESCE(SUM(ru.value), 0)
               FROM resource_usage ru
               JOIN sessions s ON ru.session_id = s.id
               WHERE s.project_id = ?1
               GROUP BY ru.metric_type"#,
            params![project_id],
        )
    }

    pub fn total_tokens_by_session_in_range(
        &self,
        session_id: &str,
        from_ts: i64,
        to_ts: i64,
    ) -> Result<ResourceSummary, DbError> {
        self.aggregate_resources(
            r#"SELECT metric_type, COALESCE(SUM(value), 0)
               FROM resource_usage
               WHERE session_id = ?1 AND timestamp >= ?2 AND timestamp <= ?3
               GROUP BY metric_type"#,
            params![session_id, from_ts, to_ts],
        )
    }

    pub fn total_tokens_by_project_in_range(
        &self,
        project_id: i64,
        from_ts: i64,
        to_ts: i64,
    ) -> Result<ResourceSummary, DbError> {
        self.aggregate_resources(
            r#"SELECT ru.metric_type, COALESCE(SUM(ru.value), 0)
               FROM resource_usage ru
               JOIN sessions s ON ru.session_id = s.id
               WHERE s.project_id = ?1 AND ru.timestamp >= ?2 AND ru.timestamp <= ?3
               GROUP BY ru.metric_type"#,
            params![project_id, from_ts, to_ts],
        )
    }

    pub fn cost_by_project(&self, project_id: i64) -> Result<f64, DbError> {
        let cost: f64 = self.connection().query_row(
            r#"SELECT COALESCE(SUM(ru.value), 0)
               FROM resource_usage ru
               JOIN sessions s ON ru.session_id = s.id
               WHERE s.project_id = ?1 AND ru.metric_type = 'cost'"#,
            params![project_id],
            |row| row.get(0),
        )?;
        Ok(cost)
    }

    fn aggregate_resources(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<ResourceSummary, DbError> {
        let mut stmt = self.connection().prepare(sql)?;
        let rows = stmt.query_map(params, |row| {
            let mt: String = row.get(0)?;
            let val: f64 = row.get(1)?;
            Ok((mt, val))
        })?;

        let mut summary = ResourceSummary::default();
        for r in rows {
            let (mt, val) = r?;
            match mt.as_str() {
                "input_tokens" => summary.input_tokens = val,
                "output_tokens" => summary.output_tokens = val,
                "cache_read_tokens" => summary.cache_read_tokens = val,
                "cache_creation_tokens" => summary.cache_creation_tokens = val,
                "cost" => summary.cost = val,
                _ => {}
            }
        }
        Ok(summary)
    }
}

fn row_to_resource_metric(row: &rusqlite::Row) -> Option<ResourceMetric> {
    let session_id: String = row.get(0).ok()?;
    let mt_str: String = row.get(1).ok()?;
    let value: f64 = row.get(2).ok()?;
    let timestamp: i64 = row.get(3).ok()?;

    MetricType::parse(&mt_str).map(|metric_type| ResourceMetric {
        session_id,
        metric_type,
        value,
        timestamp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_token_usage_from_result_usage() {
        let payload = json!({
            "result": {
                "model": "claude-sonnet-4-20250514",
                "total_cost": 0.0234,
                "usage": {
                    "input_tokens": 5000,
                    "output_tokens": 1200,
                    "cache_read_input_tokens": 800,
                    "cache_creation_input_tokens": 200
                }
            }
        });

        let usage = parse_token_usage(&payload).unwrap();
        assert_eq!(usage.input_tokens, Some(5000));
        assert_eq!(usage.output_tokens, Some(1200));
        assert_eq!(usage.cache_read_tokens, Some(800));
        assert_eq!(usage.cache_creation_tokens, Some(200));
        assert_eq!(usage.cost_usd, Some(0.0234));
        assert_eq!(usage.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    #[test]
    fn test_parse_token_usage_from_top_level_usage() {
        let payload = json!({
            "usage": {
                "input_tokens": 3000,
                "output_tokens": 700
            },
            "model": "claude-haiku-4-20250101"
        });

        let usage = parse_token_usage(&payload).unwrap();
        assert_eq!(usage.input_tokens, Some(3000));
        assert_eq!(usage.output_tokens, Some(700));
        assert_eq!(usage.cache_read_tokens, None);
        assert_eq!(usage.cache_creation_tokens, None);
        assert_eq!(usage.model.as_deref(), Some("claude-haiku-4-20250101"));
    }

    #[test]
    fn test_parse_token_usage_missing_fields_returns_none() {
        let payload = json!({"hook_type": "PreToolUse", "tool": "Read"});
        assert!(parse_token_usage(&payload).is_none());
    }

    #[test]
    fn test_parse_token_usage_partial_fields() {
        let payload = json!({
            "usage": {
                "input_tokens": 1000
            }
        });

        let usage = parse_token_usage(&payload).unwrap();
        assert_eq!(usage.input_tokens, Some(1000));
        assert_eq!(usage.output_tokens, None);
        assert!(usage.has_data());
    }

    #[test]
    fn test_parse_token_usage_top_level_tokens() {
        let payload = json!({
            "input_tokens": 2000,
            "output_tokens": 500,
            "cost_usd": 0.01
        });

        let usage = parse_token_usage(&payload).unwrap();
        assert_eq!(usage.input_tokens, Some(2000));
        assert_eq!(usage.output_tokens, Some(500));
        assert_eq!(usage.cost_usd, Some(0.01));
    }

    #[test]
    fn test_accumulate_usage_sums_correctly() {
        let mut total = TokenUsage {
            input_tokens: Some(1000),
            output_tokens: Some(200),
            cache_read_tokens: None,
            cache_creation_tokens: None,
            cost_usd: Some(0.01),
            model: Some("claude-sonnet-4-20250514".to_string()),
        };

        let new = TokenUsage {
            input_tokens: Some(500),
            output_tokens: Some(100),
            cache_read_tokens: Some(50),
            cache_creation_tokens: None,
            cost_usd: Some(0.005),
            model: None,
        };

        accumulate_usage(&mut total, &new);

        assert_eq!(total.input_tokens, Some(1500));
        assert_eq!(total.output_tokens, Some(300));
        assert_eq!(total.cache_read_tokens, Some(50));
        assert_eq!(total.cache_creation_tokens, None);
        assert!((total.cost_usd.unwrap() - 0.015).abs() < 1e-10);
        // Model kept from existing since new has None
        assert_eq!(total.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    #[test]
    fn test_accumulate_usage_updates_model() {
        let mut total = TokenUsage::default();
        let new = TokenUsage {
            input_tokens: Some(100),
            model: Some("claude-opus-4-20250514".to_string()),
            ..Default::default()
        };

        accumulate_usage(&mut total, &new);
        assert_eq!(total.model.as_deref(), Some("claude-opus-4-20250514"));
    }

    #[test]
    fn test_extract_model_from_result() {
        let payload = json!({"result": {"model": "claude-sonnet-4-20250514"}});
        assert_eq!(
            extract_model(&payload).as_deref(),
            Some("claude-sonnet-4-20250514")
        );
    }

    #[test]
    fn test_extract_model_missing() {
        let payload = json!({"result": {"usage": {}}});
        assert!(extract_model(&payload).is_none());
    }

    #[test]
    fn test_estimate_cost_sonnet() {
        let usage = TokenUsage {
            input_tokens: Some(1_000_000),
            output_tokens: Some(1_000_000),
            cache_read_tokens: Some(0),
            cache_creation_tokens: Some(0),
            cost_usd: None,
            model: None,
        };

        let cost = estimate_cost(&usage, "claude-sonnet-4-20250514");
        // input: 1M * $3/1M = $3, output: 1M * $15/1M = $15
        assert!((cost - 18.0).abs() < 0.01);
    }

    #[test]
    fn test_estimate_cost_unknown_model() {
        let usage = TokenUsage {
            input_tokens: Some(1000),
            output_tokens: Some(500),
            ..Default::default()
        };

        assert_eq!(estimate_cost(&usage, "gpt-4"), 0.0);
    }

    #[test]
    fn test_estimate_cost_haiku() {
        let usage = TokenUsage {
            input_tokens: Some(1_000_000),
            output_tokens: Some(1_000_000),
            ..Default::default()
        };

        let cost = estimate_cost(&usage, "claude-haiku-4-5-20251001");
        // input: 1M * $0.25/1M = $0.25, output: 1M * $1.25/1M = $1.25
        assert!((cost - 1.5).abs() < 0.01);
    }

    #[test]
    fn test_usage_to_metrics_complete() {
        let usage = TokenUsage {
            input_tokens: Some(5000),
            output_tokens: Some(1200),
            cache_read_tokens: Some(800),
            cache_creation_tokens: Some(200),
            cost_usd: Some(0.05),
            model: Some("claude-sonnet-4-20250514".to_string()),
        };

        let metrics = usage_to_metrics("sess-1", &usage, 1706500000);
        assert_eq!(metrics.len(), 5);

        assert_eq!(metrics[0].metric_type, MetricType::InputTokens);
        assert_eq!(metrics[0].value, 5000.0);
        assert_eq!(metrics[0].session_id, "sess-1");

        assert_eq!(metrics[1].metric_type, MetricType::OutputTokens);
        assert_eq!(metrics[1].value, 1200.0);

        assert_eq!(metrics[4].metric_type, MetricType::Cost);
        assert!((metrics[4].value - 0.05).abs() < 1e-10);
    }

    #[test]
    fn test_usage_to_metrics_partial() {
        let usage = TokenUsage {
            input_tokens: Some(1000),
            output_tokens: Some(200),
            ..Default::default()
        };

        let metrics = usage_to_metrics("sess-2", &usage, 1706500000);
        assert_eq!(metrics.len(), 2);
    }

    #[test]
    fn test_token_usage_total_tokens() {
        let usage = TokenUsage {
            input_tokens: Some(1000),
            output_tokens: Some(500),
            cache_read_tokens: Some(200),
            cache_creation_tokens: Some(100),
            ..Default::default()
        };
        assert_eq!(usage.total_tokens(), 1800);
    }

    #[test]
    fn test_token_usage_total_tokens_partial() {
        let usage = TokenUsage {
            input_tokens: Some(1000),
            ..Default::default()
        };
        assert_eq!(usage.total_tokens(), 1000);
    }

    #[test]
    fn test_metric_type_as_str() {
        assert_eq!(MetricType::InputTokens.as_str(), "input_tokens");
        assert_eq!(MetricType::OutputTokens.as_str(), "output_tokens");
        assert_eq!(MetricType::CacheReadTokens.as_str(), "cache_read_tokens");
        assert_eq!(
            MetricType::CacheCreationTokens.as_str(),
            "cache_creation_tokens"
        );
        assert_eq!(MetricType::Cost.as_str(), "cost");
    }

    #[test]
    fn test_resource_metric_serde_roundtrip() {
        let metric = ResourceMetric {
            session_id: "sess-1".to_string(),
            metric_type: MetricType::InputTokens,
            value: 5000.0,
            timestamp: 1706500000,
        };

        let json = serde_json::to_string(&metric).unwrap();
        let parsed: ResourceMetric = serde_json::from_str(&json).unwrap();
        assert_eq!(metric, parsed);
    }

    #[test]
    fn test_parse_realistic_stop_hook_payload() {
        let payload = json!({
            "stop_hook_active": true,
            "session_id": "abc-123",
            "result": {
                "num_turns": 12,
                "session_id": "abc-123",
                "total_cost": 0.1547,
                "model": "claude-sonnet-4-20250514",
                "usage": {
                    "input_tokens": 45000,
                    "output_tokens": 8500,
                    "cache_read_input_tokens": 12000,
                    "cache_creation_input_tokens": 3000
                }
            }
        });

        let usage = parse_token_usage(&payload).unwrap();
        assert_eq!(usage.input_tokens, Some(45000));
        assert_eq!(usage.output_tokens, Some(8500));
        assert_eq!(usage.cache_read_tokens, Some(12000));
        assert_eq!(usage.cache_creation_tokens, Some(3000));
        assert_eq!(usage.cost_usd, Some(0.1547));
        assert_eq!(usage.model.as_deref(), Some("claude-sonnet-4-20250514"));

        // Convert to metrics
        let metrics = usage_to_metrics("sess-abc", &usage, 1706500000);
        assert_eq!(metrics.len(), 5);
    }

    // -- DB tests --

    use crate::models::{Session, SessionState};

    fn make_db() -> (Database, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("test.db")).unwrap();
        (db, dir)
    }

    fn make_session(id: &str, pane_id: &str, project_id: Option<i64>) -> Session {
        Session {
            id: id.to_string(),
            pane_id: pane_id.to_string(),
            session_name: "main".to_string(),
            window_index: 0,
            pane_index: 0,
            working_dir: "/home/user/project".to_string(),
            state: SessionState::Idle,
            detection_method: "process_name".to_string(),
            last_activity: 1706500000,
            created_at: 1706400000,
            updated_at: 1706500000,
            project_id,
            plan_step_id: None,
            host: None,
        }
    }

    fn make_metric(session_id: &str, mt: MetricType, value: f64, ts: i64) -> ResourceMetric {
        ResourceMetric {
            session_id: session_id.to_string(),
            metric_type: mt,
            value,
            timestamp: ts,
        }
    }

    #[test]
    fn test_insert_and_query_resource_metric() {
        let (db, _dir) = make_db();
        let session = make_session("sess-1", "%0", None);
        db.create_session(&session).unwrap();

        let metric = make_metric("sess-1", MetricType::InputTokens, 5000.0, 1706500000);
        let id = db.insert_resource_metric(&metric).unwrap();
        assert!(id > 0);

        let metrics = db.get_resource_metrics_by_session("sess-1").unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].metric_type, MetricType::InputTokens);
        assert_eq!(metrics[0].value, 5000.0);
    }

    #[test]
    fn test_insert_multiple_metrics() {
        let (db, _dir) = make_db();
        let session = make_session("sess-1", "%0", None);
        db.create_session(&session).unwrap();

        let metrics = vec![
            make_metric("sess-1", MetricType::InputTokens, 5000.0, 1706500000),
            make_metric("sess-1", MetricType::OutputTokens, 1200.0, 1706500000),
            make_metric("sess-1", MetricType::Cost, 0.05, 1706500000),
        ];

        let count = db.insert_resource_metrics(&metrics).unwrap();
        assert_eq!(count, 3);

        let stored = db.get_resource_metrics_by_session("sess-1").unwrap();
        assert_eq!(stored.len(), 3);
    }

    #[test]
    fn test_query_by_session_empty() {
        let (db, _dir) = make_db();
        let session = make_session("sess-1", "%0", None);
        db.create_session(&session).unwrap();

        let metrics = db.get_resource_metrics_by_session("sess-1").unwrap();
        assert!(metrics.is_empty());
    }

    #[test]
    fn test_total_tokens_by_session() {
        let (db, _dir) = make_db();
        let session = make_session("sess-1", "%0", None);
        db.create_session(&session).unwrap();

        let metrics = vec![
            make_metric("sess-1", MetricType::InputTokens, 3000.0, 1706500000),
            make_metric("sess-1", MetricType::InputTokens, 2000.0, 1706500001),
            make_metric("sess-1", MetricType::OutputTokens, 1200.0, 1706500000),
            make_metric("sess-1", MetricType::Cost, 0.05, 1706500000),
        ];
        db.insert_resource_metrics(&metrics).unwrap();

        let summary = db.total_tokens_by_session("sess-1").unwrap();
        assert!((summary.input_tokens - 5000.0).abs() < 0.01);
        assert!((summary.output_tokens - 1200.0).abs() < 0.01);
        assert!((summary.cost - 0.05).abs() < 1e-10);
    }

    #[test]
    fn test_total_tokens_by_session_empty_returns_zero() {
        let (db, _dir) = make_db();
        let session = make_session("sess-1", "%0", None);
        db.create_session(&session).unwrap();

        let summary = db.total_tokens_by_session("sess-1").unwrap();
        assert_eq!(summary.input_tokens, 0.0);
        assert_eq!(summary.output_tokens, 0.0);
        assert_eq!(summary.cost, 0.0);
    }

    #[test]
    fn test_total_tokens_by_project() {
        let (db, _dir) = make_db();

        // Create workspace + project
        let ws = db.create_workspace("/tmp/ws", Some("ws")).unwrap();
        let proj = db.create_project(ws.id, "proj", None).unwrap();

        let s1 = make_session("sess-1", "%0", Some(proj.id));
        let s2 = make_session("sess-2", "%1", Some(proj.id));
        db.create_session(&s1).unwrap();
        db.create_session(&s2).unwrap();

        db.insert_resource_metrics(&[
            make_metric("sess-1", MetricType::InputTokens, 3000.0, 1706500000),
            make_metric("sess-2", MetricType::InputTokens, 2000.0, 1706500000),
            make_metric("sess-1", MetricType::Cost, 0.03, 1706500000),
            make_metric("sess-2", MetricType::Cost, 0.02, 1706500000),
        ])
        .unwrap();

        let summary = db.total_tokens_by_project(proj.id).unwrap();
        assert!((summary.input_tokens - 5000.0).abs() < 0.01);
        assert!((summary.cost - 0.05).abs() < 1e-10);
    }

    #[test]
    fn test_cost_by_project() {
        let (db, _dir) = make_db();

        let ws = db.create_workspace("/tmp/ws", Some("ws")).unwrap();
        let proj = db.create_project(ws.id, "proj", None).unwrap();

        let s1 = make_session("sess-1", "%0", Some(proj.id));
        db.create_session(&s1).unwrap();

        db.insert_resource_metrics(&[
            make_metric("sess-1", MetricType::Cost, 0.10, 1706500000),
            make_metric("sess-1", MetricType::Cost, 0.05, 1706500001),
            make_metric("sess-1", MetricType::InputTokens, 9000.0, 1706500000),
        ])
        .unwrap();

        let cost = db.cost_by_project(proj.id).unwrap();
        assert!((cost - 0.15).abs() < 1e-10);
    }

    #[test]
    fn test_cost_by_project_empty_returns_zero() {
        let (db, _dir) = make_db();

        let ws = db.create_workspace("/tmp/ws", Some("ws")).unwrap();
        let proj = db.create_project(ws.id, "proj", None).unwrap();

        let cost = db.cost_by_project(proj.id).unwrap();
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn test_time_range_filter_session() {
        let (db, _dir) = make_db();
        let session = make_session("sess-1", "%0", None);
        db.create_session(&session).unwrap();

        db.insert_resource_metrics(&[
            make_metric("sess-1", MetricType::InputTokens, 1000.0, 100),
            make_metric("sess-1", MetricType::InputTokens, 2000.0, 200),
            make_metric("sess-1", MetricType::InputTokens, 3000.0, 300),
        ])
        .unwrap();

        // Only include timestamps 150..250
        let summary = db
            .total_tokens_by_session_in_range("sess-1", 150, 250)
            .unwrap();
        assert!((summary.input_tokens - 2000.0).abs() < 0.01);
    }

    #[test]
    fn test_time_range_filter_project() {
        let (db, _dir) = make_db();

        let ws = db.create_workspace("/tmp/ws", Some("ws")).unwrap();
        let proj = db.create_project(ws.id, "proj", None).unwrap();

        let s1 = make_session("sess-1", "%0", Some(proj.id));
        db.create_session(&s1).unwrap();

        db.insert_resource_metrics(&[
            make_metric("sess-1", MetricType::Cost, 0.01, 100),
            make_metric("sess-1", MetricType::Cost, 0.02, 200),
            make_metric("sess-1", MetricType::Cost, 0.03, 300),
        ])
        .unwrap();

        let summary = db
            .total_tokens_by_project_in_range(proj.id, 150, 350)
            .unwrap();
        assert!((summary.cost - 0.05).abs() < 1e-10);
    }

    #[test]
    fn test_metric_type_from_str_roundtrip() {
        for mt in [
            MetricType::InputTokens,
            MetricType::OutputTokens,
            MetricType::CacheReadTokens,
            MetricType::CacheCreationTokens,
            MetricType::Cost,
        ] {
            assert_eq!(MetricType::parse(mt.as_str()), Some(mt));
        }
        assert_eq!(MetricType::parse("unknown"), None);
    }

    #[test]
    fn test_resource_summary_default_is_zero() {
        let summary = ResourceSummary::default();
        assert_eq!(summary.input_tokens, 0.0);
        assert_eq!(summary.output_tokens, 0.0);
        assert_eq!(summary.cache_read_tokens, 0.0);
        assert_eq!(summary.cache_creation_tokens, 0.0);
        assert_eq!(summary.cost, 0.0);
    }
}
