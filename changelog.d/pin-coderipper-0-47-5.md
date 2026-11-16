kind: CI: portfolio check pinned to coderipper 0.47.5
---
The Portfolio job calls the shared workflow at coderipper c72a6e0 (0.47.5) instead of 33cc3b7 (0.43.49). The rule-id migration tool finds nothing to rename here: `.coderipper.toml` already uses a numbered id (SEC-002). No code or version change.
