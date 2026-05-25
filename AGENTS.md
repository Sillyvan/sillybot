## Agent skills

### Issue tracker

Issues and PRDs are tracked in GitHub Issues; configure this clone's GitHub remote before using tracker operations. See `docs/agents/issue-tracker.md`.

### Triage labels

Use the default five canonical triage labels. See `docs/agents/triage-labels.md`.

### Domain docs

This is a single-context repository. See `docs/agents/domain.md`.

### Command modules

Keep command declarations thin: forward submitted inputs to the owning module, which keeps mutation, response content, audit facts, validation, and tests together.
