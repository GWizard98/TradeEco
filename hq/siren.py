#!/usr/bin/env python3
import smtplib
import os
import sys
from email.mime.text import MIMEText
from datetime import datetime

def send_alert(message, alert_type="STATUS"):
    gmail_user = os.environ.get("GMAIL_USER")
    gmail_password = os.environ.get("GMAIL_APP_PASSWORD")
    siren_phone = os.environ.get("SIREN_PHONE")

    if not all([gmail_user, gmail_password, siren_phone]):
        print("❌ Siren: Missing credentials")
        return False

    icons = {
        "CRITICAL": "🚨",
        "WARNING": "⚠️",
        "STATUS": "✅",
        "INTELLIGENCE": "🧠",
        "MARKET": "📈",
        "SECURITY": "🛡️"
    }

    icon = icons.get(alert_type, "📡")
    subject = f"{icon} GorTech Siren"
    body = f"{icon} {message}\n— Siren/{datetime.now().strftime('%H:%M %Z')}"

    try:
        msg = MIMEText(body)
        msg["Subject"] = subject
        msg["From"] = gmail_user
        msg["To"] = siren_phone

        with smtplib.SMTP_SSL("smtp.gmail.com", 465) as server:
            server.login(gmail_user, gmail_password)
            server.send_message(msg)
        print(f"✅ Siren sent: {alert_type} — {message}")
        return True
    except Exception as e:
        print(f"❌ Siren error: {e}")
        return False

if __name__ == "__main__":
    msg = sys.argv[1] if len(sys.argv) > 1 else "Siren test"
    alert = sys.argv[2] if len(sys.argv) > 2 else "STATUS"
    send_alert(msg, alert)
