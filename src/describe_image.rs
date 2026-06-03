use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::Path;

const DEFAULT_OPENAI_MODEL: &str = "gpt-4o";
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-20250514";
const DEFAULT_PROMPT: &str = "Describe this image in detail.";

pub enum Provider {
    OpenAI,
    Anthropic,
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::OpenAI => write!(f, "openai"),
            Provider::Anthropic => write!(f, "anthropic"),
        }
    }
}

impl std::str::FromStr for Provider {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(Provider::OpenAI),
            "anthropic" => Ok(Provider::Anthropic),
            _ => Err(anyhow::anyhow!(
                "unknown vision provider '{}'. Valid: openai, anthropic",
                s
            )),
        }
    }
}

fn mime_from_path(path: &Path) -> Result<&'static str> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "gif" => Ok("image/gif"),
        "webp" => Ok("image/webp"),
        ext => Err(anyhow::anyhow!(
            "unsupported image format '{}'. Supported: png, jpg, jpeg, gif, webp",
            ext
        )),
    }
}

fn resolve_provider(cli_provider: Option<&str>) -> Result<Provider> {
    if let Some(p) = cli_provider {
        return p.parse();
    }
    if let Ok(p) = std::env::var("AGENT_DOC_VISION_PROVIDER") {
        return p.parse();
    }
    Ok(Provider::OpenAI)
}

fn resolve_api_key(provider: &Provider, cli_key: Option<&str>) -> Result<String> {
    if let Some(k) = cli_key {
        return Ok(k.to_string());
    }
    if let Ok(k) = std::env::var("AGENT_DOC_VISION_API_KEY") {
        return Ok(k);
    }
    let env_var = match provider {
        Provider::OpenAI => "OPENAI_API_KEY",
        Provider::Anthropic => "ANTHROPIC_API_KEY",
    };
    std::env::var(env_var).with_context(|| {
        format!(
            "no API key found. Set --api-key, AGENT_DOC_VISION_API_KEY, or {}",
            env_var
        )
    })
}

fn resolve_model(provider: &Provider, cli_model: Option<&str>) -> String {
    if let Some(m) = cli_model {
        return m.to_string();
    }
    if let Ok(m) = std::env::var("AGENT_DOC_VISION_MODEL") {
        return m;
    }
    match provider {
        Provider::OpenAI => DEFAULT_OPENAI_MODEL.to_string(),
        Provider::Anthropic => DEFAULT_ANTHROPIC_MODEL.to_string(),
    }
}

fn resolve_endpoint(provider: &Provider) -> String {
    if let Ok(e) = std::env::var("AGENT_DOC_VISION_ENDPOINT") {
        return e;
    }
    match provider {
        Provider::OpenAI => "https://api.openai.com/v1/chat/completions".to_string(),
        Provider::Anthropic => "https://api.anthropic.com/v1/messages".to_string(),
    }
}

fn build_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_read(std::time::Duration::from_secs(60))
        .timeout_write(std::time::Duration::from_secs(10))
        .build()
}

fn call_openai(
    agent: &ureq::Agent,
    endpoint: &str,
    api_key: &str,
    model: &str,
    mime: &str,
    b64_data: &str,
    prompt: &str,
) -> Result<String> {
    let body = json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": prompt},
                {"type": "image_url", "image_url": {"url": format!("data:{};base64,{}", mime, b64_data)}}
            ]
        }],
        "max_tokens": 1024
    });

    let resp = agent
        .post(endpoint)
        .set("Authorization", &format!("Bearer {}", api_key))
        .set("Content-Type", "application/json")
        .send_json(&body)
        .context("OpenAI vision API request failed")?;

    let v: Value = resp
        .into_json()
        .context("failed to parse OpenAI response")?;
    v["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .context("unexpected OpenAI response format")
}

fn call_anthropic(
    agent: &ureq::Agent,
    endpoint: &str,
    api_key: &str,
    model: &str,
    mime: &str,
    b64_data: &str,
    prompt: &str,
) -> Result<String> {
    let body = json!({
        "model": model,
        "max_tokens": 1024,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "image", "source": {"type": "base64", "media_type": mime, "data": b64_data}},
                {"type": "text", "text": prompt}
            ]
        }]
    });

    let resp = agent
        .post(endpoint)
        .set("x-api-key", api_key)
        .set("anthropic-version", "2023-06-01")
        .set("Content-Type", "application/json")
        .send_json(&body)
        .context("Anthropic vision API request failed")?;

    let v: Value = resp
        .into_json()
        .context("failed to parse Anthropic response")?;
    v["content"][0]["text"]
        .as_str()
        .map(|s| s.to_string())
        .context("unexpected Anthropic response format")
}

pub fn run(
    image: &Path,
    provider: Option<&str>,
    model: Option<&str>,
    api_key: Option<&str>,
    prompt: Option<&str>,
) -> Result<()> {
    let provider = resolve_provider(provider)?;
    let api_key = resolve_api_key(&provider, api_key)?;
    let model = resolve_model(&provider, model);
    let mime = mime_from_path(image)?;
    let prompt = prompt.unwrap_or(DEFAULT_PROMPT);

    let image_data =
        std::fs::read(image).with_context(|| format!("failed to read {}", image.display()))?;
    let b64_data = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &image_data);

    let http_agent = build_agent();
    let endpoint = resolve_endpoint(&provider);

    let description = match provider {
        Provider::OpenAI => call_openai(
            &http_agent,
            &endpoint,
            &api_key,
            &model,
            mime,
            &b64_data,
            prompt,
        )?,
        Provider::Anthropic => call_anthropic(
            &http_agent,
            &endpoint,
            &api_key,
            &model,
            mime,
            &b64_data,
            prompt,
        )?,
    };

    println!("{}", description);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_png() {
        assert_eq!(mime_from_path(Path::new("test.png")).unwrap(), "image/png");
    }

    #[test]
    fn mime_jpg() {
        assert_eq!(mime_from_path(Path::new("test.jpg")).unwrap(), "image/jpeg");
    }

    #[test]
    fn mime_jpeg() {
        assert_eq!(
            mime_from_path(Path::new("photo.jpeg")).unwrap(),
            "image/jpeg"
        );
    }

    #[test]
    fn mime_gif() {
        assert_eq!(mime_from_path(Path::new("anim.gif")).unwrap(), "image/gif");
    }

    #[test]
    fn mime_webp() {
        assert_eq!(mime_from_path(Path::new("img.webp")).unwrap(), "image/webp");
    }

    #[test]
    fn mime_unsupported() {
        let err = mime_from_path(Path::new("doc.pdf")).unwrap_err();
        assert!(err.to_string().contains("unsupported"));
    }

    #[test]
    fn provider_parse_openai() {
        assert!(matches!("openai".parse::<Provider>(), Ok(Provider::OpenAI)));
    }

    #[test]
    fn provider_parse_anthropic() {
        assert!(matches!(
            "anthropic".parse::<Provider>(),
            Ok(Provider::Anthropic)
        ));
    }

    #[test]
    fn provider_parse_unknown() {
        let err = "google".parse::<Provider>().unwrap_err();
        assert!(err.to_string().contains("unknown vision provider"));
    }

    #[test]
    fn resolve_model_default_openai() {
        assert_eq!(resolve_model(&Provider::OpenAI, None), DEFAULT_OPENAI_MODEL);
    }

    #[test]
    fn resolve_model_default_anthropic() {
        assert_eq!(
            resolve_model(&Provider::Anthropic, None),
            DEFAULT_ANTHROPIC_MODEL
        );
    }

    #[test]
    fn resolve_model_cli_override() {
        assert_eq!(
            resolve_model(&Provider::OpenAI, Some("gpt-4o-mini")),
            "gpt-4o-mini"
        );
    }

    #[test]
    fn resolve_provider_default() {
        assert!(matches!(resolve_provider(None).unwrap(), Provider::OpenAI));
    }

    #[test]
    fn resolve_provider_cli() {
        assert!(matches!(
            resolve_provider(Some("anthropic")).unwrap(),
            Provider::Anthropic
        ));
    }

    #[test]
    fn resolve_api_key_missing() {
        std::env::remove_var("AGENT_DOC_VISION_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        let err = resolve_api_key(&Provider::OpenAI, None).unwrap_err();
        assert!(err.to_string().contains("no API key found"));
    }

    #[test]
    fn resolve_api_key_cli() {
        let key = resolve_api_key(&Provider::OpenAI, Some("test-key")).unwrap();
        assert_eq!(key, "test-key");
    }
}
