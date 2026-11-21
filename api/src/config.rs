use std::env;

pub struct EmailConfig {
    pub enabled: bool,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: String,
    pub smtp_pass: String,
    pub from: String,
    pub staff_to: String,
    pub confirm_reporter: bool,
}

pub struct Config {
    pub database_url: String,
    pub bind: String,
    pub attachments_dir: String,
    pub uploads_dir: String,
    pub admin_token: String,
    pub rate_limit_per_hour: u32,
    pub email: EmailConfig,
}

fn var(k: &str, d: &str) -> String {
    env::var(k).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| d.to_string())
}
fn flag(k: &str) -> bool {
    matches!(var(k, "false").to_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let database_url = env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?;
        let admin_token = var("ADMIN_TOKEN", "");
        if admin_token.len() < 16 {
            tracing::warn!("ADMIN_TOKEN missing or shorter than 16 chars: admin endpoints are disabled");
        }
        Ok(Self {
            database_url,
            bind: var("BIND", "0.0.0.0:3000"),
            attachments_dir: var("ATTACHMENTS_DIR", "/data/attachments"),
            uploads_dir: var("UPLOADS_DIR", "/data/uploads"),
            admin_token,
            rate_limit_per_hour: var("RATE_LIMIT_PER_HOUR", "10").parse().unwrap_or(10),
            email: EmailConfig {
                enabled: flag("EMAIL_ENABLED"),
                smtp_host: var("SMTP_HOST", ""),
                smtp_port: var("SMTP_PORT", "587").parse().unwrap_or(587),
                smtp_user: var("SMTP_USER", ""),
                smtp_pass: var("SMTP_PASS", ""),
                from: var("EMAIL_FROM", ""),
                staff_to: var("EMAIL_STAFF_TO", ""),
                confirm_reporter: flag("EMAIL_CONFIRM_REPORTER"),
            },
        })
    }
}
