#!/usr/bin/env python3
"""Seeds ReMa's private Codex home for an end-to-end run against the
stand-ins in mock-providers.mjs: a ChatGPT sign-in with test tokens (no real
account; unsigned JWTs that only name a test workspace) and the stand-in as
Codex's ChatGPT and Responses backend.

    python3 scripts/e2e/seed-codex-home.py "$XDG_DATA_HOME/cloud.datasound.rema/runtimes/codex"

The mock must serve HTTPS on port 8778 (MOCK_TLS_DIR) and ReMa must trust
its test CA (SSL_CERT_FILE); see the header of mock-providers.mjs.
"""
import base64
import json
import os
import sys


def b64(raw: bytes) -> str:
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


def jwt(claims: dict) -> str:
    header = b64(json.dumps({"alg": "RS256", "typ": "JWT"}).encode())
    return f"{header}.{b64(json.dumps(claims).encode())}.{b64(b'test')}"


def main() -> None:
    home = sys.argv[1]
    os.makedirs(home, exist_ok=True)
    claims = {
        "https://api.openai.com/auth": {
            "chatgpt_plan_type": "plus",
            "chatgpt_account_id": "acct_e2e",
            "chatgpt_user_id": "user_e2e",
            "user_id": "user_e2e",
        },
        "email": "ana@example.com",
        "exp": 4102444800,
        "iat": 1790000000,
    }
    auth = {
        "OPENAI_API_KEY": None,
        "auth_mode": "chatgpt",
        "tokens": {
            "id_token": jwt(claims),
            "access_token": jwt(claims),
            "refresh_token": "rt_e2e",
            "account_id": "acct_e2e",
        },
        "last_refresh": "2026-09-27T12:00:00Z",
    }
    with open(os.path.join(home, "auth.json"), "w") as f:
        json.dump(auth, f)
    with open(os.path.join(home, "config.toml"), "w") as f:
        f.write(
            'openai_base_url = "https://127.0.0.1:8778/codex/v1"\n'
            'chatgpt_base_url = "https://127.0.0.1:8778/chatgpt/backend-api/"\n\n'
            "[features]\nresponses_websockets = false\nresponses_websockets_v2 = false\n"
        )
    print("seeded", home)


if __name__ == "__main__":
    main()
