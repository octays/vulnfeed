#!/usr/bin/env python3
"""Simulate a Feishu card-action callback against the LOCAL vulnfeed server.

Authenticates like the Go bridge: Bearer = hex(HMAC-SHA256(token, "vulnfeed-feishu-bridge")).
Reads the token from data/.feishu-bridge-token (local-managed mode).

Usage:
  python3 dev/simulate-card-callback.py <dimension> <status> [owners]
  python3 dev/simulate-card-callback.py p none
  python3 dev/simulate-card-callback.py novo ops ou_aaa
  python3 dev/simulate-card-callback.py ops both ou_aaa,ou_bbb

Facts (task/message/chat/app/tenant) are read from the local DB so the
card-origin check passes. Defaults to the newest open delivery row.
"""
import hashlib
import hmac
import json
import sqlite3
import sys
import urllib.request
import uuid

BASE = "http://127.0.0.1:9000"
TOKEN_FILE = "data/.feishu-bridge-token"
DB_FILE = "data/vulnfeed.db"


def bearer():
    token = open(TOKEN_FILE, encoding="utf-8").read().strip().encode()
    return hmac.new(token, b"vulnfeed-feishu-bridge", hashlib.sha256).hexdigest()


def facts():
    con = sqlite3.connect(f"file:{DB_FILE}?mode=ro", uri=True)
    row = con.execute(
        """SELECT d.task_id,d.app_id,d.tenant_key,d.chat_id,d.message_id,t.version
           FROM receipt_delivery d
           JOIN receipt_task t ON t.id=d.task_id
           JOIN receipt_case c ON c.id=t.case_id
           WHERE d.withdrawn=0 AND d.message_id<>'' AND c.status='open'
           ORDER BY d.id DESC LIMIT 1"""
    ).fetchone()
    con.close()
    if not row:
        sys.exit("no live delivery row found in the local DB")
    return row


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        sys.exit(2)
    dimension, status = sys.argv[1], sys.argv[2]
    owners = [o for o in (sys.argv[3].split(",") if len(sys.argv) > 3 else []) if o.strip()]
    task_id, app_id, tenant, chat_id, message_id, version = facts()

    payload = {
        "header": {
            "event_id": f"local-{uuid.uuid4()}",
            "event_type": "card.action.trigger",
            "app_id": app_id,
            "tenant_key": tenant,
        },
        "event": {
            "operator": {"open_id": "ou_local_tester_001", "tenant_key": tenant},
            "context": {"open_message_id": message_id, "open_chat_id": chat_id},
            "action": {
                "value": {"task_id": task_id, "version": version},
                "form_value": {"dimension": dimension, "status": status, "owners": owners},
            },
        },
    }
    req = urllib.request.Request(
        f"{BASE}/api/internal/feishu/card-action",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {bearer()}"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req) as resp:
            body = json.load(resp)
    except urllib.error.HTTPError as e:
        body = e.read().decode()
    print(json.dumps(body, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
