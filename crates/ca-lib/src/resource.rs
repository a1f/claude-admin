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
}
