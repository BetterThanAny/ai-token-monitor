use serde_json::json;
use std::net::IpAddr;
use std::time::Duration;

use crate::providers::types::{AiKeys, WebhookConfig};

#[derive(Debug, Clone)]
pub enum WebhookAlertType {
    ThresholdCrossed {
        window_name: String,
        utilization: f64,
        threshold: u32,
        resets_at: Option<String>,
    },
    ResetCompleted {
        window_name: String,
    },
}

fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default()
}

#[derive(Debug, Clone, Copy)]
enum WebhookPlatform {
    Discord,
    Slack,
}

fn validate_webhook_url(platform: WebhookPlatform, raw_url: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw_url).map_err(|e| format!("Invalid webhook URL: {}", e))?;
    if url.scheme() != "https" {
        return Err("Webhook URL must use https.".to_string());
    }

    let host = url
        .host_str()
        .ok_or_else(|| "Webhook URL is missing a host.".to_string())?
        .to_ascii_lowercase();
    if is_blocked_webhook_host(&host) {
        return Err("Webhook URL host is not allowed.".to_string());
    }

    match platform {
        WebhookPlatform::Discord => {
            let allowed_host = matches!(
                host.as_str(),
                "discord.com" | "discordapp.com" | "canary.discord.com" | "ptb.discord.com"
            );
            if !allowed_host || !is_discord_webhook_path(url.path()) {
                return Err(
                    "Discord webhook URL must be an official Discord webhook endpoint.".to_string(),
                );
            }
        }
        WebhookPlatform::Slack => {
            let allowed_host = matches!(host.as_str(), "hooks.slack.com" | "hooks.slack-gov.com");
            if !allowed_host || !url.path().starts_with("/services/") {
                return Err(
                    "Slack webhook URL must be an official Slack incoming webhook endpoint."
                        .to_string(),
                );
            }
        }
    }

    Ok(url)
}

fn is_discord_webhook_path(path: &str) -> bool {
    if path.starts_with("/api/webhooks/") {
        return true;
    }
    let Some(rest) = path.strip_prefix("/api/v") else {
        return false;
    };
    let Some((version, suffix)) = rest.split_once('/') else {
        return false;
    };
    !version.is_empty()
        && version.chars().all(|c| c.is_ascii_digit())
        && suffix.starts_with("webhooks/")
}

fn is_blocked_webhook_host(host: &str) -> bool {
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
    {
        return true;
    }

    let Ok(ip) = host.parse::<IpAddr>() else {
        return false;
    };
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_unicast_link_local()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

fn threshold_color(threshold: u32) -> u32 {
    match threshold {
        90.. => 0xEF4444, // red
        80.. => 0xF97316, // orange
        50.. => 0xEAB308, // yellow
        _ => 0x22C55E,    // green
    }
}

fn threshold_emoji(threshold: u32) -> &'static str {
    match threshold {
        90.. => "🔴",
        80.. => "🟠",
        50.. => "🟡",
        _ => "🟢",
    }
}

fn format_resets_at(resets_at: &Option<String>) -> String {
    let Some(ts) = resets_at else {
        return String::new();
    };
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        let now = chrono::Utc::now();
        let diff = dt.signed_duration_since(now);
        if diff.num_seconds() <= 0 {
            return "Resetting...".to_string();
        }
        let hours = diff.num_hours();
        let minutes = diff.num_minutes() % 60;
        if hours >= 24 {
            let days = hours / 24;
            let remaining_hours = hours % 24;
            format!("Resets in {}d {}h {}m", days, remaining_hours, minutes)
        } else if hours > 0 {
            format!("Resets in {}h {}m", hours, minutes)
        } else {
            format!("Resets in {}m", minutes)
        }
    } else {
        String::new()
    }
}

/// Send alerts to all enabled webhook platforms.
pub async fn send_webhook_alerts(
    config: &WebhookConfig,
    secrets: &AiKeys,
    alert_type: &WebhookAlertType,
) {
    let client = build_client();

    if config.discord_enabled {
        if let Some(url) = &secrets.webhook_discord_url {
            if let Err(e) = send_discord(&client, url, alert_type).await {
                eprintln!("[WEBHOOK] Discord error: {}", e);
            }
        }
    }

    if config.slack_enabled {
        if let Some(url) = &secrets.webhook_slack_url {
            if let Err(e) = send_slack(&client, url, alert_type).await {
                eprintln!("[WEBHOOK] Slack error: {}", e);
            }
        }
    }

    if config.telegram_enabled {
        if let (Some(token), Some(chat_id)) = (
            &secrets.webhook_telegram_bot_token,
            &secrets.webhook_telegram_chat_id,
        ) {
            if let Err(e) = send_telegram(&client, token, chat_id, alert_type).await {
                eprintln!("[WEBHOOK] Telegram error: {}", e);
            }
        }
    }
}

async fn send_discord(
    client: &reqwest::Client,
    url: &str,
    alert_type: &WebhookAlertType,
) -> Result<(), String> {
    let url = validate_webhook_url(WebhookPlatform::Discord, url)?;
    let (title, description, color) = match alert_type {
        WebhookAlertType::ThresholdCrossed {
            window_name,
            utilization,
            threshold,
            resets_at,
        } => {
            let reset_str = format_resets_at(resets_at);
            let desc = if reset_str.is_empty() {
                format!("{} usage at **{:.0}%**", window_name, utilization)
            } else {
                format!(
                    "{} usage at **{:.0}%**\n{}",
                    window_name, utilization, reset_str
                )
            };
            (
                format!(
                    "{} Usage Alert — {}%",
                    threshold_emoji(*threshold),
                    threshold
                ),
                desc,
                threshold_color(*threshold),
            )
        }
        WebhookAlertType::ResetCompleted { window_name } => (
            "🔄 Usage Reset".to_string(),
            format!("{} usage has been reset!", window_name),
            0x22C55E,
        ),
    };

    let body = json!({
        "embeds": [{
            "title": title,
            "description": description,
            "color": color,
            "footer": { "text": "AI Token Monitor" },
            "timestamp": chrono::Utc::now().to_rfc3339()
        }]
    });

    let resp = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Discord returned {}", resp.status()));
    }
    Ok(())
}

async fn send_slack(
    client: &reqwest::Client,
    url: &str,
    alert_type: &WebhookAlertType,
) -> Result<(), String> {
    let url = validate_webhook_url(WebhookPlatform::Slack, url)?;
    let text = match alert_type {
        WebhookAlertType::ThresholdCrossed {
            window_name,
            utilization,
            threshold,
            resets_at,
        } => {
            let reset_str = format_resets_at(resets_at);
            let base = format!(
                "{} *{} Usage Alert* — {:.0}% (threshold: {}%)",
                threshold_emoji(*threshold),
                window_name,
                utilization,
                threshold
            );
            if reset_str.is_empty() {
                base
            } else {
                format!("{}\n_{}_", base, reset_str)
            }
        }
        WebhookAlertType::ResetCompleted { window_name } => {
            format!("🔄 *{} usage has been reset!*", window_name)
        }
    };

    let body = json!({ "text": text });
    let resp = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Slack returned {}", resp.status()));
    }
    Ok(())
}

async fn send_telegram(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    alert_type: &WebhookAlertType,
) -> Result<(), String> {
    let text = match alert_type {
        WebhookAlertType::ThresholdCrossed {
            window_name,
            utilization,
            threshold,
            resets_at,
        } => {
            let reset_str = format_resets_at(resets_at);
            let base = format!(
                "{} <b>{} Usage Alert</b>\nUsage: <code>{:.0}%</code> (threshold: {}%)",
                threshold_emoji(*threshold),
                window_name,
                utilization,
                threshold
            );
            if reset_str.is_empty() {
                base
            } else {
                format!("{}\n{}", base, reset_str)
            }
        }
        WebhookAlertType::ResetCompleted { window_name } => {
            format!("🔄 <b>{} usage has been reset!</b>", window_name)
        }
    };

    let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
    let body = json!({
        "chat_id": chat_id,
        "text": text,
        "parse_mode": "HTML"
    });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Telegram returned {}", resp.status()));
    }
    Ok(())
}

/// Test a webhook endpoint by sending a test message.
pub async fn test_webhook_endpoint(platform: &str, secrets: &AiKeys) -> Result<String, String> {
    let client = build_client();

    match platform {
        "discord" => {
            let url = secrets
                .webhook_discord_url
                .as_deref()
                .ok_or("Discord webhook URL not configured")?;
            let url = validate_webhook_url(WebhookPlatform::Discord, url)?;
            let body = json!({
                "embeds": [{
                    "title": "🔔 Test Notification",
                    "description": "AI Token Monitor webhook is working!",
                    "color": 0x7C5CFC,
                    "footer": { "text": "AI Token Monitor" },
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }]
            });
            let resp = client
                .post(url)
                .json(&body)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if resp.status().is_success() {
                Ok("Discord test message sent!".to_string())
            } else {
                Err(format!("Discord returned {}", resp.status()))
            }
        }
        "slack" => {
            let url = secrets
                .webhook_slack_url
                .as_deref()
                .ok_or("Slack webhook URL not configured")?;
            let url = validate_webhook_url(WebhookPlatform::Slack, url)?;
            let body = json!({
                "text": "🔔 *Test Notification*\nAI Token Monitor webhook is working!"
            });
            let resp = client
                .post(url)
                .json(&body)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if resp.status().is_success() {
                Ok("Slack test message sent!".to_string())
            } else {
                Err(format!("Slack returned {}", resp.status()))
            }
        }
        "telegram" => {
            let token = secrets
                .webhook_telegram_bot_token
                .as_deref()
                .ok_or("Telegram bot token not configured")?;
            let chat_id = secrets
                .webhook_telegram_chat_id
                .as_deref()
                .ok_or("Telegram chat ID not configured")?;
            let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
            let body = json!({
                "chat_id": chat_id,
                "text": "🔔 <b>Test Notification</b>\nAI Token Monitor webhook is working!",
                "parse_mode": "HTML"
            });
            let resp = client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if resp.status().is_success() {
                Ok("Telegram test message sent!".to_string())
            } else {
                Err(format!("Telegram returned {}", resp.status()))
            }
        }
        _ => Err(format!("Unknown platform: {}", platform)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_official_webhook_urls() {
        assert!(validate_webhook_url(
            WebhookPlatform::Discord,
            "https://discord.com/api/webhooks/123/token"
        )
        .is_ok());
        assert!(validate_webhook_url(
            WebhookPlatform::Discord,
            "https://discord.com/api/v10/webhooks/123/token"
        )
        .is_ok());
        assert!(validate_webhook_url(
            WebhookPlatform::Slack,
            "https://hooks.slack.com/services/T000/B000/secret"
        )
        .is_ok());
    }

    #[test]
    fn rejects_non_https_or_private_webhook_urls() {
        assert!(validate_webhook_url(
            WebhookPlatform::Discord,
            "http://discord.com/api/webhooks/123/token"
        )
        .is_err());
        assert!(validate_webhook_url(
            WebhookPlatform::Slack,
            "https://127.0.0.1/services/T000/B000/secret"
        )
        .is_err());
        assert!(validate_webhook_url(
            WebhookPlatform::Slack,
            "https://example.local/services/T000/B000/secret"
        )
        .is_err());
    }

    #[test]
    fn rejects_wrong_provider_hosts() {
        assert!(validate_webhook_url(
            WebhookPlatform::Discord,
            "https://example.com/api/webhooks/123/token"
        )
        .is_err());
        assert!(validate_webhook_url(
            WebhookPlatform::Slack,
            "https://discord.com/services/T000/B000/secret"
        )
        .is_err());
    }
}
