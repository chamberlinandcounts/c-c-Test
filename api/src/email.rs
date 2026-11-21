//! Optional email notifications. Disabled unless EMAIL_ENABLED=true and SMTP is configured.
use lettre::{
    message::header::ContentType, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};

use crate::config::EmailConfig;

pub struct NewIssueMail {
    pub ticket: String,
    pub category: String,
    pub issue_type: Option<String>,
    pub assigned_to: String,
    pub description: Option<String>,
    pub reporter_email: String,
}

pub fn notify_new_issue(cfg: &EmailConfig, m: NewIssueMail) {
    if !cfg.enabled {
        return;
    }
    if cfg.smtp_host.is_empty() || cfg.from.is_empty() {
        tracing::warn!("EMAIL_ENABLED=true but SMTP_HOST/EMAIL_FROM not set; skipping email");
        return;
    }
    let host = cfg.smtp_host.clone();
    let port = cfg.smtp_port;
    let creds = Credentials::new(cfg.smtp_user.clone(), cfg.smtp_pass.clone());
    let from = cfg.from.clone();
    let staff = cfg.staff_to.clone();
    let confirm = cfg.confirm_reporter;
    tokio::spawn(async move {
        let transport = match AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host) {
            Ok(b) => b.port(port).credentials(creds).build(),
            Err(e) => return tracing::error!("smtp setup failed: {e}"),
        };
        let body = format!(
            "Ticket {}\nCategory: {}\nIssue type: {}\nAssigned to: {}\n\n{}\n",
            m.ticket,
            m.category,
            m.issue_type.clone().unwrap_or_default(),
            m.assigned_to,
            m.description.clone().unwrap_or_default()
        );
        let mut targets: Vec<(String, String)> = Vec::new();
        if !staff.is_empty() {
            targets.push((staff.clone(), format!("New Canyon Oaks issue {}", m.ticket)));
        }
        if confirm {
            targets.push((m.reporter_email.clone(), format!("We received your report ({})", m.ticket)));
        }
        for (to, subject) in targets {
            let msg = match (from.parse(), to.parse()) {
                (Ok(f), Ok(t)) => Message::builder().from(f).to(t).subject(subject)
                    .header(ContentType::TEXT_PLAIN).body(body.clone()),
                _ => continue,
            };
            match msg {
                Ok(msg) => {
                    if let Err(e) = transport.send(msg).await {
                        tracing::error!("email send failed: {e}");
                    }
                }
                Err(e) => tracing::error!("email build failed: {e}"),
            }
        }
    });
}
