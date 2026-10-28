//! Optional live check: cargo run -p ct-adapters --example pricing_probe
//! Uses public synthetic model/date inputs; prints no local session data.
use ct_adapters::pricing::LiteLlmPricing;
use ct_domain::pricing::PricingProvider;
use ct_domain::TokenUsage;

fn main() -> Result<(), String> {
    let provider = LiteLlmPricing::new(std::path::PathBuf::from("target/pricing-probe"));
    if let Some(path) = std::env::args().nth(1) {
        let inventory: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let mut results = Vec::new();
        for row in inventory["models"]
            .as_array()
            .ok_or("inventory needs a models array")?
        {
            let model = row["model"].as_str().ok_or("model ID required")?;
            let usage = TokenUsage {
                input: Some(1000),
                output: Some(1000),
                cache_read: Some(1000),
                cache_creation: model.starts_with("claude-").then_some(1000),
                ..Default::default()
            };
            let mut times = vec![None];
            for field in ["first_timestamp", "last_timestamp"] {
                if let Some(value) = row[field].as_str() {
                    times.push(Some(
                        chrono::DateTime::parse_from_rfc3339(value)
                            .map_err(|e| e.to_string())?
                            .with_timezone(&chrono::Utc),
                    ));
                }
            }
            for at in times {
                let result = provider.quote(model, at, usage);
                println!(
                    "{model} {}: {}",
                    at.map(|at| at.to_rfc3339())
                        .unwrap_or_else(|| "current".into()),
                    if result.is_ok() { "priced" } else { "unpriced" }
                );
                results.push(serde_json::json!({"model": model, "at": at, "quote": result}));
            }
        }
        std::fs::write(
            "target/local-pricing-coverage.json",
            serde_json::to_vec_pretty(&results).unwrap(),
        )
        .map_err(|e| e.to_string())?;
        return Ok(());
    }
    let historical = chrono::DateTime::parse_from_rfc3339("2025-05-01T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    for at in [None, Some(historical)] {
        let quote = provider.quote(
            "gpt-4.1-mini",
            at,
            TokenUsage {
                input: Some(1_000_000),
                output: Some(1_000_000),
                ..Default::default()
            },
        )?;
        println!("{}", serde_json::to_string_pretty(&quote).unwrap());
    }
    Ok(())
}
