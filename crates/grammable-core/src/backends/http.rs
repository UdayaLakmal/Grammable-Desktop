// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// Local model servers: Ollama and any OpenAI-compatible server
// (LM Studio, Jan, llama.cpp, LocalAI, vLLM …).

use std::time::Duration;

use serde_json::{json, Value};

use super::{AiRequest, BackendConfig, BackendError, BackendId, BackendStatus};
use crate::prompt::{user_prompt, SYSTEM_PROMPT};

fn url(base: &str, path: &str) -> String {
    format!("{}{path}", base.trim().trim_end_matches('/'))
}

fn client(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder().timeout(timeout).build().unwrap_or_default()
}

fn map_error(e: reqwest::Error, id: BackendId, base: &str) -> BackendError {
    if e.is_timeout() {
        BackendError::TimedOut(0)
    } else if e.is_connect() {
        BackendError::NotReachable(id, format!("Is it running at {base}? {}", id.setup_hint()))
    } else {
        BackendError::Failed(e.to_string())
    }
}

async fn get_json(u: &str, timeout: Duration) -> Result<Value, reqwest::Error> {
    client(timeout).get(u).send().await?.error_for_status()?.json().await
}

async fn post_json(u: &str, body: &Value, timeout: Duration, id: BackendId, base: &str) -> Result<Value, BackendError> {
    let resp = client(timeout).post(u).json(body).send().await.map_err(|e| map_error(e, id, base))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        let snippet: String = text.chars().take(500).collect();
        return Err(BackendError::Failed(format!("HTTP {}: {snippet}", status.as_u16())));
    }
    resp.json().await.map_err(|e| map_error(e, id, base))
}

fn messages(request: &AiRequest) -> Value {
    json!([
        { "role": "system", "content": SYSTEM_PROMPT },
        { "role": "user", "content": user_prompt(&request.instructions, &request.text) },
    ])
}

// MARK: Ollama

pub async fn ollama_models(config: &BackendConfig) -> Result<Vec<String>, reqwest::Error> {
    let json = get_json(&url(&config.ollama_url, "/api/tags"), Duration::from_secs(3)).await?;
    Ok(json["models"]
        .as_array()
        .map(|m| m.iter().filter_map(|x| x["name"].as_str().map(String::from)).collect())
        .unwrap_or_default())
}

pub async fn ollama_status(config: &BackendConfig) -> BackendStatus {
    match ollama_models(config).await {
        Ok(models) if models.is_empty() => {
            BackendStatus::missing("Ollama is running but has no models. Run: ollama pull llama3.2")
        }
        Ok(models) => {
            let chosen = config.model(BackendId::Ollama, None).unwrap_or_else(|| models[0].clone());
            BackendStatus::available(format!("{} · {chosen}", config.ollama_url))
        }
        Err(_) => BackendStatus::missing(format!(
            "Not running at {}. {}",
            config.ollama_url,
            BackendId::Ollama.setup_hint()
        )),
    }
}

pub async fn ollama(config: &BackendConfig, request: &AiRequest) -> Result<String, BackendError> {
    let id = BackendId::Ollama;
    let base = &config.ollama_url;
    let model = match config.model(id, request.model.as_deref()) {
        Some(m) => m,
        None => ollama_models(config)
            .await
            .map_err(|e| map_error(e, id, base))?
            .into_iter()
            .next()
            .ok_or_else(|| BackendError::Unavailable("Ollama has no models installed. Run: ollama pull llama3.2".into()))?,
    };
    let body = json!({
        "model": model,
        "messages": messages(request),
        "stream": false,
        "options": { "temperature": 0.2 },
    });
    let json = post_json(&url(base, "/api/chat"), &body, config.timeout(), id, base).await?;
    json["message"]["content"].as_str().map(String::from).ok_or(BackendError::EmptyResponse)
}

// MARK: OpenAI-compatible

pub async fn openai_models(config: &BackendConfig) -> Result<Vec<String>, reqwest::Error> {
    let json = get_json(&url(&config.openai_base_url, "/models"), Duration::from_secs(3)).await?;
    Ok(json["data"]
        .as_array()
        .map(|m| m.iter().filter_map(|x| x["id"].as_str().map(String::from)).collect())
        .unwrap_or_default())
}

pub async fn openai_status(config: &BackendConfig) -> BackendStatus {
    match openai_models(config).await {
        Ok(models) => {
            let chosen = config
                .model(BackendId::OpenAiCompatible, None)
                .or_else(|| models.first().cloned())
                .unwrap_or_else(|| "server default".into());
            BackendStatus::available(format!("{} · {chosen}", config.openai_base_url))
        }
        Err(_) => BackendStatus::missing(format!(
            "No server at {}. {}",
            config.openai_base_url,
            BackendId::OpenAiCompatible.setup_hint()
        )),
    }
}

pub async fn openai_compatible(config: &BackendConfig, request: &AiRequest) -> Result<String, BackendError> {
    let id = BackendId::OpenAiCompatible;
    let base = &config.openai_base_url;
    let mut body = json!({ "messages": messages(request), "temperature": 0.2, "stream": false });
    if let Some(m) = config.model(id, request.model.as_deref()) {
        body["model"] = json!(m);
    } else if let Some(first) = openai_models(config).await.ok().and_then(|m| m.into_iter().next()) {
        body["model"] = json!(first);
    }
    let json = post_json(&url(base, "/chat/completions"), &body, config.timeout(), id, base).await?;
    json["choices"][0]["message"]["content"].as_str().map(String::from).ok_or(BackendError::EmptyResponse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_urls() {
        assert_eq!(url(" http://localhost:1234/v1/ ", "/models"), "http://localhost:1234/v1/models");
    }

    #[tokio::test]
    async fn unreachable_server_means_not_set_up() {
        let config = BackendConfig { ollama_url: "http://127.0.0.1:9".into(), ..Default::default() };
        let req = AiRequest { instructions: "x".into(), text: "y".into(), model: Some("m".into()) };
        let err = ollama(&config, &req).await.unwrap_err();
        assert!(err.means_not_set_up(), "{err:?}");
    }
}
