#!/usr/bin/env python3
"""Cliente fictício para identidade e cota do trabalhador nos testes."""
import json
import os
from pathlib import Path
import sys

if sys.argv[1:4] == ["auth", "status", "--json"]:
    identity = Path(os.environ.get("CLAUDE_CONFIG_DIR", ".")) / "fixture-identity"
    email = identity.read_text().strip() if identity.exists() else "fixture@example.invalid"
    print(json.dumps({"loggedIn": True, "authMethod": "claude.ai", "apiProvider": "firstParty",
                      "subscriptionType": "max", "email": email, "orgId": "fixture-org"}))
elif sys.argv[1:3] == ["-p", "/usage"]:
    print(json.dumps({"result": "Current session: 10% used\nCurrent week (all models): 20% used"}))
else:
    sys.exit(1)
