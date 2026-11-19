#!/usr/bin/env python3
"""CLIs de consulta do teste AC. Não implementam login, prompt ou execução."""
import json
import os
import sys

backend = os.path.basename(sys.argv[0]).split(".")[0]
if backend not in ("claude", "codex"):
    backend = sys.argv[1]

if backend == "claude":
    print(json.dumps({"loggedIn": True, "authMethod": "claude.ai", "apiProvider": "firstParty",
                      "subscriptionType": "max", "email": "wire-owner@example.invalid",
                      "orgId": "private-org", "configDirectory": "/private/profile",
                      "token": "TEST_SECRET_SENTINEL"}))
    sys.exit(0)

if sys.argv[-2:] == ["login", "status"]:
    print("Logged in using ChatGPT")
    sys.exit(0)

for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method == "initialized":
        continue
    if method == "initialize":
        result = {"userAgent": "ac-fixture"}
    elif method == "account/read" and request["params"] == {"refreshToken": False}:
        result = {"account": {"type": "chatgpt", "planType": "pro", "email": "wire-owner@example.invalid"}}
    elif method == "model/list":
        result = {"data": [{"id": "gpt-6.1-sol", "defaultReasoningEffort": "high",
                            "supportedReasoningEfforts": [{"reasoningEffort": "high"},
                                                           {"reasoningEffort": "xhigh"}],
                            "apiKey": "TEST_SECRET_SENTINEL", "path": "/private/profile"}]}
    else:
        raise AssertionError("consulta inesperada: " + str(method))
    print(json.dumps({"id": request["id"], "result": result}), flush=True)
