use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;

const DEFAULT_OPENAI_MODEL: &str = "gpt-4o";
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-20250514";
const DEFAULT_PROMPT: &str = "Describe this image in detail.";

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[allow(dead_code)]
pub struct ImageRef {
    pub kind: ImageRefKind,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[allow(dead_code)]
pub enum ImageRefKind {
    Markdown,
    NakedPath,
    Url,
}

#[allow(dead_code)]
fn has_image_extension(s: &str) -> bool {
    let path = s.split('?').next().unwrap_or(s);
    let lower = path.to_lowercase();
    IMAGE_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}

#[allow(dead_code)]
pub fn extract_image_references(text: &str) -> Vec<ImageRef> {
    let mut refs = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Phase 1: markdown images ![alt](path)
    let mut pos = 0;
    let bytes = text.as_bytes();
    while pos < text.len() {
        if bytes[pos] == b'!' && pos + 1 < text.len() && bytes[pos + 1] == b'[' {
            let rest = &text[pos..];
            if let Some(close_bracket) = rest.find(']') {
                let after = &rest[close_bracket + 1..];
                if after.starts_with('(')
                    && let Some(close_paren) = after.find(')')
                {
                    let path = &after[1..close_paren];
                    if !path.is_empty() && seen.insert(path.to_string()) {
                        refs.push(ImageRef {
                            kind: ImageRefKind::Markdown,
                            path: path.to_string(),
                        });
                    }
                    pos += close_bracket + 1 + close_paren + 1;
                    continue;
                }
            }
        }
        pos += 1;
    }

    // Phase 2: image URLs (https://...png, http://...jpg)
    for word in text.split_whitespace() {
        let word = word.trim_matches(&['"', '\'', '(', ')', '[', ']', ',', ';', ':'][..]);
        if (word.starts_with("https://") || word.starts_with("http://"))
            && has_image_extension(word)
            && seen.insert(word.to_string())
        {
            refs.push(ImageRef {
                kind: ImageRefKind::Url,
                path: word.to_string(),
            });
        }
    }

    // Phase 3: naked file paths with image extensions
    for word in text.split_whitespace() {
        let word = word.trim_matches(&['"', '\'', '(', ')', '[', ']', ',', ';', ':'][..]);
        if word.starts_with("http://") || word.starts_with("https://") || word.starts_with("![") {
            continue;
        }
        if has_image_extension(word) && seen.insert(word.to_string()) {
            refs.push(ImageRef {
                kind: ImageRefKind::NakedPath,
                path: word.to_string(),
            });
        }
    }

    refs
}

#[derive(Debug)]
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

fn resolve_provider(cli_provider: Option<&str>, config_provider: Option<&str>) -> Result<Provider> {
    if let Some(p) = cli_provider {
        return p.parse();
    }
    if let Ok(p) = std::env::var("AGENT_DOC_VISION_PROVIDER") {
        return p.parse();
    }
    if let Some(p) = config_provider {
        return p.parse();
    }
    Ok(Provider::OpenAI)
}

fn shell_expand(value: &str) -> Result<String> {
    if !value.contains("$(") && !value.starts_with('$') {
        return Ok(value.to_string());
    }
    let script = format!("set -e; v={}; printf '%s' \"$v\"", value);
    let output = std::process::Command::new("sh")
        .args(["-c", &script])
        .output()
        .context("failed to run shell expansion for vision api_key")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("shell expansion failed for '{}': {}", value, stderr.trim());
    }
    let expanded = String::from_utf8_lossy(&output.stdout);
    Ok(expanded.to_string())
}

fn resolve_api_key(
    provider: &Provider,
    cli_key: Option<&str>,
    config_key: Option<&str>,
) -> Result<String> {
    if let Some(k) = cli_key {
        return shell_expand(k);
    }
    if let Ok(k) = std::env::var("AGENT_DOC_VISION_API_KEY") {
        return Ok(k);
    }
    if let Some(k) = config_key {
        return shell_expand(k);
    }
    let env_var = match provider {
        Provider::OpenAI => "OPENAI_API_KEY",
        Provider::Anthropic => "ANTHROPIC_API_KEY",
    };
    std::env::var(env_var).with_context(|| {
        format!(
            "no API key found. Set --api-key, AGENT_DOC_VISION_API_KEY, config.toml [vision] api_key, or {}",
            env_var
        )
    })
}

fn resolve_model(
    provider: &Provider,
    cli_model: Option<&str>,
    config_model: Option<&str>,
) -> String {
    if let Some(m) = cli_model {
        return m.to_string();
    }
    if let Ok(m) = std::env::var("AGENT_DOC_VISION_MODEL") {
        return m;
    }
    if let Some(m) = config_model {
        return m.to_string();
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
    ureq::Agent::config_builder()
        .timeout_recv_body(Some(std::time::Duration::from_secs(60)))
        .timeout_send_body(Some(std::time::Duration::from_secs(10)))
        .build()
        .into()
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
        .header("Authorization", &format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .send_json(&body)
        .context("OpenAI vision API request failed")?;

    let v: Value = resp
        .into_body()
        .read_json()
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
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .send_json(&body)
        .context("Anthropic vision API request failed")?;

    let v: Value = resp
        .into_body()
        .read_json()
        .context("failed to parse Anthropic response")?;
    v["content"][0]["text"]
        .as_str()
        .map(|s| s.to_string())
        .context("unexpected Anthropic response format")
}

pub fn describe_image_data(
    image_path: &Path,
    provider: &Provider,
    api_key: &str,
    model: &str,
    prompt: &str,
) -> Result<String> {
    let mime = mime_from_path(image_path)?;
    let image_data = std::fs::read(image_path)
        .with_context(|| format!("failed to read {}", image_path.display()))?;
    let b64_data = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &image_data);
    let http_agent = build_agent();
    let endpoint = resolve_endpoint(provider);
    match provider {
        Provider::OpenAI => call_openai(
            &http_agent,
            &endpoint,
            api_key,
            model,
            mime,
            &b64_data,
            prompt,
        ),
        Provider::Anthropic => call_anthropic(
            &http_agent,
            &endpoint,
            api_key,
            model,
            mime,
            &b64_data,
            prompt,
        ),
    }
}

pub fn resolve_vision_config(
    cli_provider: Option<&str>,
    cli_model: Option<&str>,
    cli_api_key: Option<&str>,
    config_provider: Option<&str>,
    config_model: Option<&str>,
    config_api_key: Option<&str>,
) -> Result<(Provider, String, String)> {
    let provider = resolve_provider(cli_provider, config_provider)?;
    let api_key = resolve_api_key(&provider, cli_api_key, config_api_key)?;
    let model = resolve_model(&provider, cli_model, config_model);
    Ok((provider, api_key, model))
}

pub struct ImageDescription {
    pub reference: ImageRef,
    pub description: String,
}

pub fn describe_images_in_text(
    text: &str,
    provider: &Provider,
    api_key: &str,
    model: &str,
    base_dir: &Path,
) -> Result<Vec<ImageDescription>> {
    let refs = extract_image_references(text);
    if refs.is_empty() {
        return Ok(Vec::new());
    }
    let mut descriptions = Vec::new();
    for image_ref in &refs {
        let image_path = match image_ref.kind {
            ImageRefKind::Markdown | ImageRefKind::NakedPath => {
                let path = Path::new(&image_ref.path);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    base_dir.join(path)
                }
            }
            ImageRefKind::Url => {
                eprintln!(
                    "[describe_image] skipping URL image (not yet supported for preflight): {}",
                    image_ref.path
                );
                continue;
            }
        };
        if !image_path.exists() {
            eprintln!(
                "[describe_image] image not found, skipping: {}",
                image_path.display()
            );
            continue;
        }
        match describe_image_data(&image_path, provider, api_key, model, DEFAULT_PROMPT) {
            Ok(desc) => descriptions.push(ImageDescription {
                reference: image_ref.clone(),
                description: desc,
            }),
            Err(err) => {
                eprintln!(
                    "[describe_image] failed to describe {}: {}",
                    image_path.display(),
                    err
                );
            }
        }
    }
    Ok(descriptions)
}

pub fn run(
    image: &Path,
    provider: Option<&str>,
    model: Option<&str>,
    api_key: Option<&str>,
    prompt: Option<&str>,
) -> Result<()> {
    let project_config = agent_doc_orchestration::project_config_io::load_project_for_doc(image);
    let vision = &project_config.vision;
    let (provider, api_key, model) = resolve_vision_config(
        provider,
        model,
        api_key,
        vision.provider.as_deref(),
        vision.model.as_deref(),
        vision.api_key.as_deref(),
    )?;
    let prompt = prompt.unwrap_or(DEFAULT_PROMPT);
    let description = describe_image_data(image, &provider, &api_key, &model, prompt)?;
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
        assert_eq!(
            resolve_model(&Provider::OpenAI, None, None),
            DEFAULT_OPENAI_MODEL
        );
    }

    #[test]
    fn resolve_model_default_anthropic() {
        assert_eq!(
            resolve_model(&Provider::Anthropic, None, None),
            DEFAULT_ANTHROPIC_MODEL
        );
    }

    #[test]
    fn resolve_model_cli_override() {
        assert_eq!(
            resolve_model(&Provider::OpenAI, Some("gpt-4o-mini"), None),
            "gpt-4o-mini"
        );
    }

    #[test]
    fn resolve_provider_default() {
        assert!(matches!(
            resolve_provider(None, None).unwrap(),
            Provider::OpenAI
        ));
    }

    #[test]
    fn resolve_provider_cli() {
        assert!(matches!(
            resolve_provider(Some("anthropic"), None).unwrap(),
            Provider::Anthropic
        ));
    }

    #[test]
    fn resolve_api_key_missing() {
        unsafe {
            std::env::remove_var("AGENT_DOC_VISION_API_KEY");
            std::env::remove_var("OPENAI_API_KEY");
        }
        let err = resolve_api_key(&Provider::OpenAI, None, None).unwrap_err();
        assert!(err.to_string().contains("no API key found"));
    }

    #[test]
    fn resolve_api_key_cli() {
        let key = resolve_api_key(&Provider::OpenAI, Some("test-key"), None).unwrap();
        assert_eq!(key, "test-key");
    }

    #[test]
    fn extract_markdown_image() {
        let refs = extract_image_references("See ![img](img_1.png) for details.");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].kind, ImageRefKind::Markdown);
        assert_eq!(refs[0].path, "img_1.png");
    }

    #[test]
    fn extract_naked_path() {
        let refs = extract_image_references("The bug is in screenshot.png");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].kind, ImageRefKind::NakedPath);
        assert_eq!(refs[0].path, "screenshot.png");
    }

    #[test]
    fn extract_image_url() {
        let refs = extract_image_references("See https://example.com/img.png");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].kind, ImageRefKind::Url);
        assert_eq!(refs[0].path, "https://example.com/img.png");
    }

    #[test]
    fn extract_mixed_references() {
        let refs = extract_image_references(
            "![alt](img.png) and screenshot.jpg plus https://host.com/photo.gif",
        );
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].kind, ImageRefKind::Markdown);
        assert_eq!(refs[1].kind, ImageRefKind::Url);
        assert_eq!(refs[2].kind, ImageRefKind::NakedPath);
    }

    #[test]
    fn extract_deduplication() {
        let refs = extract_image_references("![a](img.png) and also img.png again");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].kind, ImageRefKind::Markdown);
    }

    #[test]
    fn extract_no_matches() {
        let refs = extract_image_references("Just some text without images.");
        assert!(refs.is_empty());
    }

    #[test]
    fn extract_absolute_path() {
        let refs = extract_image_references("/tmp/opencode/img_16.png");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].kind, ImageRefKind::NakedPath);
        assert_eq!(refs[0].path, "/tmp/opencode/img_16.png");
    }

    #[test]
    fn extract_url_with_query() {
        let refs = extract_image_references("https://cdn.com/img.jpg?w=800");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].kind, ImageRefKind::Url);
    }

    #[test]
    fn describe_images_empty_text() {
        let provider = Provider::OpenAI;
        let result =
            describe_images_in_text("no images here", &provider, "key", "model", Path::new("."))
                .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn describe_images_url_skipped() {
        let provider = Provider::OpenAI;
        let result = describe_images_in_text(
            "see https://example.com/img.png",
            &provider,
            "key",
            "model",
            Path::new("."),
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn describe_images_missing_file_skipped() {
        let provider = Provider::OpenAI;
        let result = describe_images_in_text(
            "see nonexistent_file.png",
            &provider,
            "key",
            "model",
            Path::new("/tmp"),
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn resolve_vision_config_defaults() {
        unsafe {
            std::env::remove_var("AGENT_DOC_VISION_PROVIDER");
            std::env::remove_var("AGENT_DOC_VISION_API_KEY");
            std::env::remove_var("OPENAI_API_KEY");
        }
        let err = resolve_vision_config(None, None, None, None, None, None).unwrap_err();
        assert!(err.to_string().contains("no API key found"));
    }

    #[test]
    fn shell_expand_plain() {
        assert_eq!(shell_expand("plain-key").unwrap(), "plain-key");
    }

    #[test]
    fn shell_expand_env_var() {
        unsafe {
            std::env::set_var("AGENT_DOC_TEST_SHELL_EXPAND", "expanded-value");
        }
        let result = shell_expand("$AGENT_DOC_TEST_SHELL_EXPAND").unwrap();
        assert_eq!(result, "expanded-value");
        unsafe {
            std::env::remove_var("AGENT_DOC_TEST_SHELL_EXPAND");
        }
    }

    #[test]
    fn shell_expand_command_substitution() {
        let result = shell_expand("$(echo hello-world)").unwrap();
        assert_eq!(result, "hello-world");
    }

    #[test]
    fn shell_expand_failure() {
        let err = shell_expand("$(exit 1)").unwrap_err();
        assert!(err.to_string().contains("shell expansion failed"));
    }

    #[test]
    fn resolve_vision_config_with_key() {
        let (provider, api_key, model) =
            resolve_vision_config(None, None, Some("test-key"), None, None, None).unwrap();
        assert!(matches!(provider, Provider::OpenAI));
        assert_eq!(api_key, "test-key");
        assert_eq!(model, DEFAULT_OPENAI_MODEL);
    }

    #[test]
    fn resolve_vision_config_config_fallback() {
        unsafe {
            std::env::remove_var("AGENT_DOC_VISION_PROVIDER");
            std::env::remove_var("AGENT_DOC_VISION_API_KEY");
            std::env::remove_var("AGENT_DOC_VISION_MODEL");
            std::env::remove_var("OPENAI_API_KEY");
        }
        let (provider, api_key, model) = resolve_vision_config(
            None,
            None,
            None,
            Some("anthropic"),
            Some("claude-sonnet-4-20250514"),
            Some("config-key"),
        )
        .unwrap();
        assert!(matches!(provider, Provider::Anthropic));
        assert_eq!(api_key, "config-key");
        assert_eq!(model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn resolve_vision_config_cli_overrides_config() {
        let (provider, api_key, model) = resolve_vision_config(
            Some("openai"),
            Some("gpt-4o-mini"),
            Some("cli-key"),
            Some("anthropic"),
            Some("claude-sonnet-4-20250514"),
            Some("config-key"),
        )
        .unwrap();
        assert!(matches!(provider, Provider::OpenAI));
        assert_eq!(api_key, "cli-key");
        assert_eq!(model, "gpt-4o-mini");
    }
}
